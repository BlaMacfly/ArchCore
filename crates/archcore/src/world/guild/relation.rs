//! Alliances (« union ») et hostilités entre guildes : AgpmGuild ops 34 à 45.
//!
//! Relevé dans Archonia (fabricants FUN_140202390…FUN_140203130, envois FUN_140605a40…FUN_140606630,
//! réception : table de sauts 0x1401f98e0, rappels d'AgcmUIGuild FUN_1408cbbf0, FUN_1408cc1c0,
//! FUN_1408cc440, FUN_1408cbb00, FUN_1408cb2f0, FUN_1408cba20, FUN_1408cb890, FUN_1408cbd20,
//! FUN_1408cb400). Champs : guilde (2), nom (3), date (6), rôle (11), autre guilde (17 = GUILD_BATTLE,
//! champ 0) ; 37 et 43 portent aussi le CID (1).
//!
//! ```text
//!   → 34 demande d'alliance   2 sa guilde, 3 son nom, 17 { guilde visée }   ← au chef visé : 2 sa guilde,
//!                                                                              3 chef demandeur, 17 { demandeuse }
//!   → 35 refus                2, 17 { demandeuse }                         ← code 0x44 au demandeur
//!   → 36 accord               2, 17 { demandeuse }                         ← 36 à chaque guilde de l'alliance :
//!                                                                              2, 17 { alliée }, 6 date, 11 rôle
//!   → 37 départ de l'alliance 1 CID, 2 et 17 { sa guilde }                 ← 37 : 2, 17 { partante }, 3 nouveau chef
//!   → 38 déclaration d'hostilité, 39 refus (ou refus de la trêve), 40 accord ← 40 aux deux : 2, 17 { hostile }, 6
//!   → 41 demande de trêve     2, 3, 17 { visée }                           ← au chef visé, comme 38
//!   → 43 trêve acceptée       1, 2, 3 et 17 { demandeuse }                 ← 43 aux deux : 2, 17 { l'autre }
//!   ← 44 détails des alliées, 45 des hostiles : 21 = GUILD_LIST (3 entrées au plus par paquet)
//! ```
//! Seuls les chefs agissent. Une alliance a un chef (la guilde qui l'a fondée, rôle 2 ; les autres
//! rôle 1) et 3 autres guildes au plus (FUN_1401ff510) ; 20 membres exigés de part et d'autre (code 0x41,
//! nombre écrit par Archonia), 10 pour une hostilité (0x4f) ; 5 hostilités au plus. Une demande sans
//! réponse tombe au bout d'une minute. Le chef qui refuse les relations de guilde (option 0x40, que le
//! client vérifie avant d'afficher la question) n'est pas sollicité : code « impossible » au demandeur.

use super::{rank, send_to, send_to_guild, system_packet, Guild};
use crate::world::{WorldCtx, WorldState};
use crate::{debug, info};
use archcore_net::schema::{self, mini};
use archcore_net::{build_mini, build_packet, decode_mini, Fields, Packet};

pub mod op {
    pub const JOINT_REQUEST: i64 = 34;
    pub const JOINT_REJECT: i64 = 35;
    pub const JOINT: i64 = 36;
    pub const JOINT_LEAVE: i64 = 37;
    pub const HOSTILE_REQUEST: i64 = 38;
    pub const HOSTILE_REJECT: i64 = 39;
    pub const HOSTILE: i64 = 40;
    pub const HOSTILE_LEAVE_REQUEST: i64 = 41;
    pub const HOSTILE_LEAVE_REJECT: i64 = 42;
    pub const HOSTILE_LEAVE: i64 = 43;
    pub const JOINT_DETAIL: i64 = 44;
    pub const HOSTILE_DETAIL: i64 = 45;
}

/// Codes du message système (0x3c à 0x59).
pub mod code {
    pub const JOINT_DISABLE: i64 = 0x3c;
    pub const JOINT_NOT_LEADER: i64 = 0x3d;
    pub const JOINT_ALREADY: i64 = 0x3e;
    pub const JOINT_MAX: i64 = 0x3f;
    pub const JOINT_MASTER_OFFLINE: i64 = 0x40;
    pub const JOINT_NOT_ENOUGH_MEMBER: i64 = 0x41;
    pub const JOINT_OTHER_NOT_ENOUGH_MEMBER: i64 = 0x42;
    pub const JOINT_WAIT: i64 = 0x43;
    pub const JOINT_REJECT: i64 = 0x44;
    pub const JOINT_SUCCESS: i64 = 0x46;
    pub const JOINT_LEAVE_SUCCESS: i64 = 0x47;
    pub const JOINT_LEAVE_OTHER: i64 = 0x48;
    pub const JOINT_DESTROY: i64 = 0x49;
    pub const JOINT_SUCCESS2: i64 = 0x4a;
    pub const HOSTILE_DISABLE: i64 = 0x4b;
    pub const HOSTILE_ALREADY: i64 = 0x4c;
    pub const HOSTILE_MAX: i64 = 0x4d;
    pub const HOSTILE_MASTER_OFFLINE: i64 = 0x4e;
    pub const HOSTILE_NOT_ENOUGH_MEMBER: i64 = 0x4f;
    pub const HOSTILE_OTHER_NOT_ENOUGH_MEMBER: i64 = 0x50;
    pub const HOSTILE_WAIT: i64 = 0x51;
    pub const HOSTILE_REJECT: i64 = 0x52;
    pub const HOSTILE_SUCCESS: i64 = 0x54;
    pub const HOSTILE_LEAVE_DISABLE: i64 = 0x55;
    pub const HOSTILE_LEAVE_WAIT: i64 = 0x56;
    pub const HOSTILE_LEAVE_REJECT: i64 = 0x57;
    pub const HOSTILE_LEAVE_SUCCESS: i64 = 0x59;
}

/// Autres guildes d'une alliance (FUN_1401ff510), hostilités, membres exigés.
pub const JOINT_MAX: usize = 3;
pub const HOSTILE_MAX: usize = 5;
pub const JOINT_MIN_MEMBERS: usize = 20;
pub const HOSTILE_MIN_MEMBERS: usize = 10;
/// Rôle dans l'alliance (champ 11) : la guilde fondatrice, les autres.
pub const ROLE_LEADER: i64 = 2;
pub const ROLE_MEMBER: i64 = 1;
/// Option du chef : refuser les relations de guilde (drapeaux du personnage, FUN_1408cc440).
pub const REFUSE_RELATION: u32 = 0x40;
/// Une demande sans réponse tombe au bout de ce temps (s).
const OFFER_SECS: i64 = 60;

/// Demande en attente.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Joint,
    Hostile,
    HostileLeave,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    pub kind: Kind,
    pub from: u32,
    pub to: u32,
    pub at: i64,
}

/// Relations en mémoire : alliance (guilde fondatrice, date d'entrée) et hostilités de chaque guilde.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Relations {
    pub joint_leader: u32,
    pub joint_at: i64,
    pub hostile: Vec<(u32, i64)>,
}

fn now() -> i64 {
    super::now()
}

fn other_mini(name: &str) -> Vec<u8> {
    let m = &mini::GUILD_BATTLE;
    build_mini(m, &Fields::new(m).with(0, name))
}

fn base(o: i64, g: &Guild, other: &str) -> Fields {
    Fields::new(&schema::GUILD).with(0, o).with(2, g.name.as_str()).with(17, other_mini(other))
}

fn build(f: Fields) -> Vec<u8> {
    build_packet(&schema::GUILD, &f)
}

/// Guildes de l'alliance de `gid` (elle comprise), la fondatrice d'abord puis par date d'entrée.
pub fn alliance(st: &WorldState, gid: u32) -> Vec<u32> {
    let Some(leader) = st.guilds.by_id.get(&gid).map(|g| g.relations.joint_leader).filter(|l| *l != 0) else { return Vec::new() };
    let mut v: Vec<&Guild> = st.guilds.by_id.values().filter(|g| g.relations.joint_leader == leader).collect();
    v.sort_by_key(|g| (g.id != leader, g.relations.joint_at, g.id));
    v.iter().map(|g| g.id).collect()
}

pub fn allied(st: &WorldState, a: u32, b: u32) -> bool {
    let la = st.guilds.by_id.get(&a).map_or(0, |g| g.relations.joint_leader);
    la != 0 && a != b && st.guilds.by_id.get(&b).is_some_and(|g| g.relations.joint_leader == la)
}

pub fn hostile(st: &WorldState, a: u32, b: u32) -> bool {
    st.guilds.by_id.get(&a).is_some_and(|g| g.relations.hostile.iter().any(|(o, _)| *o == b))
}

/// Op 36 : `other` est l'alliée de `g`.
fn joint_packet(g: &Guild, other: &Guild) -> Vec<u8> {
    let role = if other.relations.joint_leader == other.id { ROLE_LEADER } else { ROLE_MEMBER };
    build(base(op::JOINT, g, &other.name).with(6, other.relations.joint_at).with(11, role))
}

/// Op 40 : `other` est hostile à `g` depuis `since`.
fn hostile_packet(g: &Guild, other: &str, since: i64) -> Vec<u8> {
    build(base(op::HOSTILE, g, other).with(6, since))
}

/// Op 44 ou 45 : détails (entrées de la liste des guildes), 3 par paquet (FUN_1408cbd20, FUN_1408cb400).
fn detail_packets(o: i64, g: &Guild, others: &[&Guild]) -> Vec<Vec<u8>> {
    let s = &mini::GUILD_LIST;
    others
        .chunks(3)
        .map(|chunk| {
            let mut f = Fields::new(s).with(0, chunk.len() as i64).with(1, 0).with(2, chunk.len() as i64);
            for (i, x) in chunk.iter().enumerate() {
                f = f.with(3 + i, super::list::item(x));
            }
            build(Fields::new(&schema::GUILD).with(0, o).with(2, g.name.as_str()).with(21, build_mini(s, &f)))
        })
        .collect()
}

/// Alliées et hostiles de `gid`, avec leurs détails, envoyés au joueur `sid` (entrée en jeu) ou à toute la
/// guilde.
fn send_relations(st: &WorldState, gid: u32, sid: Option<u32>) {
    let Some(g) = st.guilds.by_id.get(&gid) else { return };
    let mut pkts = Vec::new();
    let allies: Vec<&Guild> = alliance(st, gid).into_iter().filter(|id| *id != gid).filter_map(|id| st.guilds.by_id.get(&id)).collect();
    for a in &allies {
        pkts.push(joint_packet(g, a));
    }
    pkts.extend(detail_packets(op::JOINT_DETAIL, g, &allies));
    let hostiles: Vec<&Guild> = g.relations.hostile.iter().filter_map(|(id, _)| st.guilds.by_id.get(id)).collect();
    for (h, (_, since)) in hostiles.iter().zip(&g.relations.hostile) {
        pkts.push(hostile_packet(g, &h.name, *since));
    }
    pkts.extend(detail_packets(op::HOSTILE_DETAIL, g, &hostiles));
    for pkt in pkts {
        match sid {
            Some(s) => send_to(st, s, pkt),
            None => send_to_guild(st, g, None, &pkt),
        }
    }
}

/// À l'entrée en jeu d'un membre.
pub fn on_enter(st: &WorldState, sid: u32, gid: u32) {
    send_relations(st, gid, Some(sid));
}

fn master_sid(st: &WorldState, g: &Guild) -> Option<u32> {
    g.master().and_then(|m| st.by_cid.get(&m.cid)).copied()
}

fn master_name(g: &Guild) -> String {
    g.master().map(|m| m.name.clone()).unwrap_or_default()
}

fn send_master(st: &WorldState, gid: u32, pkt: Vec<u8>) {
    if let Some(s) = st.guilds.by_id.get(&gid).and_then(|g| master_sid(st, g)) {
        send_to(st, s, pkt);
    }
}

fn sys(c: i64) -> Vec<u8> {
    system_packet(c, None, None)
}

pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some((_, gid, my_rank)) = super::mine(st, sid) else { return Ok(()) };
    if my_rank != rank::MASTER {
        send_to(st, sid, system_packet(super::code::NOT_MASTER, None, None));
        return Ok(());
    }
    let other_name = p.bytes(17).and_then(|b| decode_mini(&mini::GUILD_BATTLE, b).ok()).and_then(|m| m.text(0)).unwrap_or_default();
    let other = st.guilds.named(other_name.trim()).map(|g| g.id);
    match p.int(0) {
        Some(op::JOINT_REQUEST) => joint_request(st, sid, gid, other),
        Some(op::JOINT_REJECT) => {
            answer(st, gid, other, Kind::Joint, false);
        }
        Some(op::JOINT) => {
            if answer(st, gid, other, Kind::Joint, true) {
                joint(ctx, st, other.unwrap(), gid).await?;
            }
        }
        Some(op::JOINT_LEAVE) => joint_leave(ctx, st, gid).await?,
        Some(op::HOSTILE_REQUEST) => hostile_request(st, sid, gid, other),
        Some(op::HOSTILE_REJECT | op::HOSTILE_LEAVE_REJECT) => {
            // Archonia refuse aussi la trêve par l'op 39 (FUN_1408cada0).
            if !answer(st, gid, other, Kind::Hostile, false) {
                answer(st, gid, other, Kind::HostileLeave, false);
            }
        }
        Some(op::HOSTILE) => {
            if answer(st, gid, other, Kind::Hostile, true) {
                make_hostile(ctx, st, other.unwrap(), gid).await?;
            }
        }
        Some(op::HOSTILE_LEAVE_REQUEST) => hostile_leave_request(st, sid, gid, other),
        Some(op::HOSTILE_LEAVE) => {
            if answer(st, gid, other, Kind::HostileLeave, true) {
                end_hostile(ctx, st, other.unwrap(), gid).await?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Demande en attente de `from` à `to`.
fn pending(st: &WorldState, kind: Kind, from: u32, to: u32) -> bool {
    st.guilds.relation_offers.iter().any(|o| o.kind == kind && o.from == from && o.to == to)
}

/// Réponse de `gid` à la demande de `other` ; rend vrai si la demande existait et est acceptée.
fn answer(st: &mut WorldState, gid: u32, other: Option<u32>, kind: Kind, yes: bool) -> bool {
    let Some(from) = other else { return false };
    if !pending(st, kind, from, gid) {
        return false;
    }
    st.guilds.relation_offers.retain(|o| !(o.kind == kind && o.from == from && o.to == gid));
    if !yes {
        let c = match kind {
            Kind::Joint => code::JOINT_REJECT,
            Kind::Hostile => code::HOSTILE_REJECT,
            Kind::HostileLeave => code::HOSTILE_LEAVE_REJECT,
        };
        send_master(st, from, sys(c));
    }
    yes
}

/// Transmet la demande au chef visé et la garde en attente.
fn offer(st: &mut WorldState, sid: u32, kind: Kind, from: u32, to: u32, o: i64, wait: i64) {
    let (g, e) = (&st.guilds.by_id[&from], &st.guilds.by_id[&to]);
    let ask = build(base(o, e, &g.name).with(3, master_name(g).as_str()));
    send_master(st, to, ask);
    send_to(st, sid, sys(wait));
    st.guilds.relation_offers.retain(|x| !(x.kind == kind && x.from == from && x.to == to));
    st.guilds.relation_offers.push(Offer { kind, from, to, at: now() });
    debug!("[guilde] {} → {} : demande {kind:?}", st.guilds.by_id[&from].name, st.guilds.by_id[&to].name);
}

fn refuses(st: &WorldState, g: &Guild) -> bool {
    master_sid(st, g).is_some_and(|s| super::super::option_flags::refuses(st, s, REFUSE_RELATION))
}

/// Op 34 : `gid` propose l'alliance à `other`.
fn joint_request(st: &mut WorldState, sid: u32, gid: u32, other: Option<u32>) {
    let refuse = |st: &WorldState, c: i64, n: Option<i64>| send_to(st, sid, system_packet(c, None, n));
    let Some(to) = other.filter(|t| *t != gid) else { return refuse(st, code::JOINT_DISABLE, None) };
    let (g, e) = (&st.guilds.by_id[&gid], &st.guilds.by_id[&to]);
    if allied(st, gid, to) {
        return refuse(st, code::JOINT_ALREADY, None);
    }
    if g.relations.joint_leader != 0 && g.relations.joint_leader != gid {
        return refuse(st, code::JOINT_NOT_LEADER, None);
    }
    if e.relations.joint_leader != 0 || hostile(st, gid, to) {
        return refuse(st, code::JOINT_DISABLE, None);
    }
    if alliance(st, gid).len().saturating_sub(1) >= JOINT_MAX {
        return refuse(st, code::JOINT_MAX, None);
    }
    if g.active_members() < JOINT_MIN_MEMBERS {
        return refuse(st, code::JOINT_NOT_ENOUGH_MEMBER, Some(JOINT_MIN_MEMBERS as i64));
    }
    if e.active_members() < JOINT_MIN_MEMBERS {
        return refuse(st, code::JOINT_OTHER_NOT_ENOUGH_MEMBER, None);
    }
    if master_sid(st, e).is_none() {
        return refuse(st, code::JOINT_MASTER_OFFLINE, None);
    }
    if refuses(st, e) {
        return refuse(st, code::JOINT_DISABLE, None);
    }
    offer(st, sid, Kind::Joint, gid, to, op::JOINT_REQUEST, code::JOINT_WAIT);
}

/// Op 36 accepté : `to` rejoint l'alliance de `from` (fondée s'il n'y en avait pas).
async fn joint(ctx: &WorldCtx, st: &mut WorldState, from: u32, to: u32) -> anyhow::Result<()> {
    if allied(st, from, to) || st.guilds.by_id[&to].relations.joint_leader != 0 || alliance(st, from).len().saturating_sub(1) >= JOINT_MAX {
        return Ok(());
    }
    let t = now();
    let others = alliance(st, from);
    let leader = if others.is_empty() { from } else { st.guilds.by_id[&from].relations.joint_leader };
    for (id, at) in [(from, if others.is_empty() { t } else { st.guilds.by_id[&from].relations.joint_at }), (to, t)] {
        let g = st.guilds.by_id.get_mut(&id).unwrap();
        g.relations.joint_leader = leader;
        g.relations.joint_at = at;
        ctx.db.characters.exec("UPDATE guild SET joint_leader = ?, joint_at = FROM_UNIXTIME(?) WHERE id = ?", (leader, at, id)).await?;
    }
    let members = alliance(st, to);
    let newcomer = st.guilds.by_id[&to].name.clone();
    for &id in &members {
        let g = &st.guilds.by_id[&id];
        if id == to {
            send_relations(st, id, None);
        } else {
            let n = &st.guilds.by_id[&to];
            send_to_guild(st, g, None, &joint_packet(g, n));
            for pkt in detail_packets(op::JOINT_DETAIL, g, &[n]) {
                send_to_guild(st, g, None, &pkt);
            }
        }
        // Les deux guildes de l'accord : Guild_Joint_Success ; les autres alliées : Success2.
        let msg = if id == from || id == to { sys(code::JOINT_SUCCESS) } else { system_packet(code::JOINT_SUCCESS2, Some(&newcomer), None) };
        send_to_guild(st, g, None, &msg);
    }
    info!("[guilde] {} rejoint l'alliance de {}", newcomer, st.guilds.by_id[&leader].name);
    Ok(())
}

/// Op 37 : `gid` quitte son alliance ; une alliance réduite à une guilde est dissoute.
async fn joint_leave(ctx: &WorldCtx, st: &mut WorldState, gid: u32) -> anyhow::Result<()> {
    let members = alliance(st, gid);
    if members.is_empty() {
        return Ok(());
    }
    let leaving = st.guilds.by_id[&gid].name.clone();
    let old_leader = st.guilds.by_id[&gid].relations.joint_leader;
    let rest: Vec<u32> = members.iter().copied().filter(|id| *id != gid).collect();
    clear_joint(ctx, st, gid).await?;
    {
        let g = &st.guilds.by_id[&gid];
        send_to_guild(st, g, None, &build(base(op::JOINT_LEAVE, g, &leaving)));
        send_to_guild(st, g, None, &sys(code::JOINT_LEAVE_SUCCESS));
    }
    if rest.len() < 2 {
        for id in rest {
            clear_joint(ctx, st, id).await?;
            let g = &st.guilds.by_id[&id];
            send_to_guild(st, g, None, &build(base(op::JOINT_LEAVE, g, &leaving)));
            send_to_guild(st, g, None, &sys(code::JOINT_DESTROY));
        }
        info!("[guilde] {leaving} quitte son alliance, dissoute");
        return Ok(());
    }
    // La fondatrice partie, la plus ancienne des autres mène l'alliance.
    let leader = if old_leader == gid { rest[0] } else { old_leader };
    let leader_name = st.guilds.by_id[&leader].name.clone();
    for &id in &rest {
        st.guilds.by_id.get_mut(&id).unwrap().relations.joint_leader = leader;
        ctx.db.characters.exec("UPDATE guild SET joint_leader = ? WHERE id = ?", (leader, id)).await?;
        let g = &st.guilds.by_id[&id];
        let new_leader = if old_leader == gid { leader_name.as_str() } else { "" };
        send_to_guild(st, g, None, &build(base(op::JOINT_LEAVE, g, &leaving).with(3, new_leader)));
        send_to_guild(st, g, None, &system_packet(code::JOINT_LEAVE_OTHER, Some(&leaving), None));
    }
    info!("[guilde] {leaving} quitte l'alliance de {leader_name}");
    Ok(())
}

async fn clear_joint(ctx: &WorldCtx, st: &mut WorldState, gid: u32) -> anyhow::Result<()> {
    if let Some(g) = st.guilds.by_id.get_mut(&gid) {
        g.relations.joint_leader = 0;
        g.relations.joint_at = 0;
    }
    ctx.db.characters.exec("UPDATE guild SET joint_leader = NULL, joint_at = NULL WHERE id = ?", (gid,)).await?;
    Ok(())
}

/// Op 38 : `gid` déclare `other` hostile.
fn hostile_request(st: &mut WorldState, sid: u32, gid: u32, other: Option<u32>) {
    let refuse = |st: &WorldState, c: i64, n: Option<i64>| send_to(st, sid, system_packet(c, None, n));
    let Some(to) = other.filter(|t| *t != gid) else { return refuse(st, code::HOSTILE_DISABLE, None) };
    let (g, e) = (&st.guilds.by_id[&gid], &st.guilds.by_id[&to]);
    if hostile(st, gid, to) {
        return refuse(st, code::HOSTILE_ALREADY, None);
    }
    if allied(st, gid, to) || e.relations.hostile.len() >= HOSTILE_MAX {
        return refuse(st, code::HOSTILE_DISABLE, None);
    }
    if g.relations.hostile.len() >= HOSTILE_MAX {
        return refuse(st, code::HOSTILE_MAX, None);
    }
    if g.active_members() < HOSTILE_MIN_MEMBERS {
        return refuse(st, code::HOSTILE_NOT_ENOUGH_MEMBER, Some(HOSTILE_MIN_MEMBERS as i64));
    }
    if e.active_members() < HOSTILE_MIN_MEMBERS {
        return refuse(st, code::HOSTILE_OTHER_NOT_ENOUGH_MEMBER, None);
    }
    if master_sid(st, e).is_none() {
        return refuse(st, code::HOSTILE_MASTER_OFFLINE, None);
    }
    if refuses(st, e) {
        return refuse(st, code::HOSTILE_DISABLE, None);
    }
    offer(st, sid, Kind::Hostile, gid, to, op::HOSTILE_REQUEST, code::HOSTILE_WAIT);
}

/// Op 40 accepté : `a` et `b` deviennent hostiles.
async fn make_hostile(ctx: &WorldCtx, st: &mut WorldState, a: u32, b: u32) -> anyhow::Result<()> {
    if hostile(st, a, b) {
        return Ok(());
    }
    let t = now();
    for (x, y) in [(a, b), (b, a)] {
        st.guilds.by_id.get_mut(&x).unwrap().relations.hostile.push((y, t));
        ctx.db.characters.exec("INSERT IGNORE INTO guild_hostile (guild_id, other_id, since) VALUES (?, ?, FROM_UNIXTIME(?))", (x, y, t)).await?;
    }
    for (x, y) in [(a, b), (b, a)] {
        let (g, o) = (&st.guilds.by_id[&x], &st.guilds.by_id[&y]);
        send_to_guild(st, g, None, &hostile_packet(g, &o.name, t));
        for pkt in detail_packets(op::HOSTILE_DETAIL, g, &[o]) {
            send_to_guild(st, g, None, &pkt);
        }
        send_to_guild(st, g, None, &sys(code::HOSTILE_SUCCESS));
    }
    info!("[guilde] {} et {} sont hostiles", st.guilds.by_id[&a].name, st.guilds.by_id[&b].name);
    Ok(())
}

/// Op 41 : `gid` propose la trêve à `other`.
fn hostile_leave_request(st: &mut WorldState, sid: u32, gid: u32, other: Option<u32>) {
    let Some(to) = other.filter(|t| hostile(st, gid, *t)) else { return send_to(st, sid, sys(code::HOSTILE_LEAVE_DISABLE)) };
    if master_sid(st, &st.guilds.by_id[&to]).is_none() {
        return send_to(st, sid, sys(code::HOSTILE_LEAVE_DISABLE));
    }
    offer(st, sid, Kind::HostileLeave, gid, to, op::HOSTILE_LEAVE_REQUEST, code::HOSTILE_LEAVE_WAIT);
}

/// Op 43 accepté : fin de l'hostilité entre `a` et `b`.
async fn end_hostile(ctx: &WorldCtx, st: &mut WorldState, a: u32, b: u32) -> anyhow::Result<()> {
    remove_hostile(ctx, st, a, b).await?;
    for id in [a, b] {
        if let Some(g) = st.guilds.by_id.get(&id) {
            send_to_guild(st, g, None, &sys(code::HOSTILE_LEAVE_SUCCESS));
        }
    }
    Ok(())
}

/// Retire l'hostilité et l'annonce (op 43) aux deux guildes encore là.
async fn remove_hostile(ctx: &WorldCtx, st: &mut WorldState, a: u32, b: u32) -> anyhow::Result<()> {
    let names: Vec<(u32, String)> = [a, b].iter().filter_map(|id| st.guilds.by_id.get(id).map(|g| (*id, g.name.clone()))).collect();
    for (x, y) in [(a, b), (b, a)] {
        if let Some(g) = st.guilds.by_id.get_mut(&x) {
            g.relations.hostile.retain(|(o, _)| *o != y);
        }
        let other = names.iter().find(|(id, _)| *id == y).map(|(_, n)| n.clone()).unwrap_or_default();
        if let Some(g) = st.guilds.by_id.get(&x) {
            send_to_guild(st, g, None, &build(base(op::HOSTILE_LEAVE, g, &other).with(3, other.as_str())));
        }
    }
    ctx.db.characters.exec("DELETE FROM guild_hostile WHERE (guild_id = ? AND other_id = ?) OR (guild_id = ? AND other_id = ?)", (a, b, b, a)).await?;
    info!("[guilde] fin de l'hostilité entre {:?}", names.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>());
    Ok(())
}

/// Dissolution de `gid` : elle quitte son alliance et ses hostilités prennent fin.
pub async fn on_destroy(ctx: &WorldCtx, st: &mut WorldState, gid: u32) -> anyhow::Result<()> {
    if st.guilds.by_id.get(&gid).is_some_and(|g| g.relations.joint_leader != 0) {
        joint_leave(ctx, st, gid).await?;
    }
    let hostiles: Vec<u32> = st.guilds.by_id.get(&gid).map(|g| g.relations.hostile.iter().map(|(o, _)| *o).collect()).unwrap_or_default();
    for o in hostiles {
        remove_hostile(ctx, st, gid, o).await?;
    }
    st.guilds.relation_offers.retain(|o| o.from != gid && o.to != gid);
    Ok(())
}

/// Demandes échues (appelé à chaque balayage).
pub fn sweep(st: &mut WorldState) {
    let t = now();
    st.guilds.relation_offers.retain(|o| t - o.at <= OFFER_SECS);
}
