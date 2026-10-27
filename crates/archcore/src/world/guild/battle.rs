//! Combats de guildes : AgpmGuild ops 11 à 25 (statut, bilan, demande, acceptation, refus, annulation, début,
//! temps, score, abandon, résultat) et coups entre membres des deux guildes.
//!
//! ```text
//!   15 → chef A : guilde (2), chef (3), GUILD_BATTLE (17) { 0 guilde B, 5 type, 6 durée (s), 7 nombre }
//!   15 ← chef B : guilde B (2), chef A (3), GUILD_BATTLE { 0 guilde A, 5-7 }        → Guild_Battle_Accept_ConfirmUI_Open
//!   16 → chef B accepte (même GUILD_BATTLE) ; 17 → refuse (code 30 au chef A)
//!   16 ← les deux guildes : GUILD_BATTLE { adverse, chef adverse, acceptation, délai, début, type, durée, nombre,
//!        horloge } → « Guild_Msg_Battle_Accept » (délai / 60 minutes)
//!   18 → un chef demande l'annulation → 18 à l'autre chef ; 19 → accord (19 aux deux guildes) ; 20 → refus
//!        (code 33) ; à moins d'une minute du début : code 31
//!   21 ← début ; 22 ← horloge, chaque seconde ; 23 ← scores (soi, adverse, gain, gain adverse)
//!   24 → abandon d'un chef (avant le début : code 35) ; 25 ← résultat (1 victoire, 2 nul, 3 défaite,
//!        4 victoire et 5 défaite par abandon) ; 12 ← bilan (victoires, nuls, défaites, points) ; 11 ← statut
//! ```
//! Les horodatages sont en secondes ; le client affiche délai + acceptation − horloge avant le début, puis
//! durée + début − horloge (FUN_1408505a0). Types (« GuildBattleMode_* ») : 0 points, 1 élimination, 2 mort
//! du chef, 3 survie de groupe, 4 survie individuelle ; durées et effectifs : listes du client
//! (`DAT_140f6b690` 5, 10, 30, 60, 90 et 120 minutes ; 5 à 50 combattants pour le groupe, 3 à 9 pour
//! l'individuel).
//!
//! Choix d'ArchCore, faute de règle dans les données du client : 10 membres et 100 niveaux au moins de
//! chaque côté (les messages d'Archonia ont ces nombres en paramètre), délai de préparation de
//! `Guild.BattleReadySeconds` (3 minutes par défaut), un point par adversaire tué ; la mort d'un chef finit le type 2 ; aux types 3 et 4, les
//! combattants sont les membres en ligne au début, jusqu'à l'effectif choisi, et le dernier côté debout
//! gagne ; à la fin du temps, le meilleur score gagne. Le bilan ajoute le score aux points de la guilde.
//! Pendant le combat, les membres des deux guildes peuvent se frapper (`attack_step`).

use super::{code, guild, mine, rank, send_to, send_to_guild, system_numbers, system_packet, Guild, Record};
use crate::world::combat::{self, Attacker, Defender};
use crate::world::{WorldCtx, WorldState};
use crate::{debug, info};
use archcore_net::schema::{self, mini};
use archcore_net::{build_mini, build_packet, decode_mini, Fields, Packet};
use std::time::{Duration, Instant};

pub mod op {
    pub const UPDATE_STATUS: i64 = 11;
    pub const UPDATE_RECORD: i64 = 12;
    pub const BATTLE_INFO: i64 = 13;
    pub const REQUEST: i64 = 15;
    pub const ACCEPT: i64 = 16;
    pub const REJECT: i64 = 17;
    pub const CANCEL_REQUEST: i64 = 18;
    pub const CANCEL_ACCEPT: i64 = 19;
    pub const CANCEL_REJECT: i64 = 20;
    pub const START: i64 = 21;
    pub const UPDATE_TIME: i64 = 22;
    pub const UPDATE_SCORE: i64 = 23;
    pub const WITHDRAW: i64 = 24;
    pub const RESULT: i64 = 25;
}

/// Statut de la guilde (champ 11).
pub mod status {
    pub const NONE: i64 = 0;
    pub const DECLARE: i64 = 1;
    pub const READY: i64 = 2;
    pub const BATTLE: i64 = 3;
}

/// Résultat (GUILD_BATTLE champ 13).
pub mod result {
    pub const WIN: i64 = 1;
    pub const DRAW: i64 = 2;
    pub const LOSE: i64 = 3;
    pub const WIN_BY_WITHDRAW: i64 = 4;
    pub const LOSE_BY_WITHDRAW: i64 = 5;
}

/// Types de combat.
pub mod kind {
    pub const POINT: i64 = 0;
    pub const KNOCKOUT: i64 = 1;
    pub const CHIEF_DEATH: i64 = 2;
    pub const TOTAL_SURVIVAL: i64 = 3;
    pub const PRIVATE_SURVIVAL: i64 = 4;
}

/// Codes du message système liés aux combats.
pub mod bcode {
    pub const NOT_ENOUGH_MEMBER: i64 = 20;
    pub const NOT_ENOUGH_MEMBER2: i64 = 21;
    pub const NOT_ENOUGH_LEVEL: i64 = 22;
    pub const NOT_ENOUGH_LEVEL2: i64 = 23;
    pub const NOT_REQUEST_STATUS: i64 = 24;
    pub const NOT_REQUEST_STATUS2: i64 = 25;
    pub const ENEMY_MASTER_OFFLINE: i64 = 26;
    pub const NOT_EXIST_GUILD: i64 = 27;
    pub const REQUEST: i64 = 28;
    pub const REJECT_BY_OTHER: i64 = 30;
    pub const CANCEL_PASSED_TIME: i64 = 31;
    pub const CANCEL_REJECT_BY_OTHER: i64 = 33;
    pub const NOT_WITHDRAW_UNTIL_START: i64 = 35;
    pub const REFUSE: i64 = 43;
}

/// Effectifs et niveaux exigés de chaque guilde (paramètres des messages 20 et 22).
pub const NEED_MEMBERS: usize = 10;
pub const NEED_LEVEL_SUM: i64 = 100;
/// Durées acceptées (secondes). Le délai entre l'acceptation et le début est `Guild.BattleReadySeconds`.
pub const MIN_DURATION: i64 = 300;
pub const MAX_DURATION: i64 = 10_800;
/// Une annulation n'est plus possible à moins de tant de secondes du début (code 31, en minutes chez le client).
pub const CANCEL_ENABLE_SECS: i64 = 60;
/// Une demande sans réponse tombe au bout de ce temps.
const OFFER_SECS: i64 = 60;
/// Option du joueur : refuser les combats de guilde (AgpmCharacter op 0x11, champ 22).
pub const REFUSE_GUILD_BATTLE: u32 = 0x04;
/// Marge de portée d'un coup (comme pour les monstres).
const REACH_TOLERANCE: f64 = 50.0;
/// Fréquence de l'horloge envoyée (op 22).
const CLOCK_EVERY: Duration = Duration::from_secs(1);

/// Combat d'une guilde, vu de son côté.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Battle {
    pub enemy: u32,
    pub kind: i64,
    pub duration: i64,
    pub person: i64,
    pub accept_at: i64,
    pub ready: i64,
    pub start_at: i64,
    pub score: i64,
    pub up_score: i64,
    pub result: i64,
    /// Combattants (types 3 et 4) et morts au combat.
    pub fighters: Vec<u32>,
    pub dead: Vec<u32>,
    /// Demande d'annulation de cette guilde, en attente.
    pub cancel_asked: bool,
}

/// Demande reçue par une guilde, ou faite par elle.
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    pub from: u32,
    pub to: u32,
    pub kind: i64,
    pub duration: i64,
    pub person: i64,
    pub at: i64,
}

fn now() -> i64 {
    super::now()
}

fn battle_mini(f: Fields) -> Vec<u8> {
    build_mini(&mini::GUILD_BATTLE, &f)
}

fn bfields() -> Fields {
    Fields::new(&mini::GUILD_BATTLE)
}

/// GUILD_BATTLE complet d'une guilde (op 13, 16, 21) : adverse, chef adverse, temps, type, scores.
fn full_battle(st: &WorldState, g: &Guild, b: &Battle) -> Vec<u8> {
    let e = st.guilds.by_id.get(&b.enemy);
    let enemy = e.map(|x| x.name.clone()).unwrap_or_default();
    let enemy_master = e.and_then(|x| x.master()).map(|m| m.name.clone()).unwrap_or_default();
    let eb = e.and_then(|x| x.battle.as_ref());
    let _ = g;
    battle_mini(
        bfields()
            .with(0, enemy.as_str())
            .with(1, enemy_master.as_str())
            .with(2, b.accept_at)
            .with(3, b.ready)
            .with(4, b.start_at)
            .with(5, b.kind)
            .with(6, b.duration)
            .with(7, b.person)
            .with(8, now())
            .with(9, b.score)
            .with(10, eb.map_or(0, |x| x.score))
            .with(11, b.up_score)
            .with(12, eb.map_or(0, |x| x.up_score))
            .with(13, b.result)
            .with(14, 0),
    )
}

fn packet(o: i64, g: &Guild, battle: Option<Vec<u8>>) -> Vec<u8> {
    let mut f = guild(o).with(2, g.name.as_str());
    if let Some(b) = battle {
        f = f.with(17, b);
    }
    build_packet(&schema::GUILD, &f)
}

fn status_packet(g: &Guild) -> Vec<u8> {
    build_packet(&schema::GUILD, &guild(op::UPDATE_STATUS).with(2, g.name.as_str()).with(11, g.status))
}

pub fn record_packet(g: &Guild) -> Vec<u8> {
    let r = g.record;
    build_packet(&schema::GUILD, &guild(op::UPDATE_RECORD).with(2, g.name.as_str()).with(12, r.win).with(13, r.draw).with(14, r.lose).with(15, r.points))
}

fn master_sid(st: &WorldState, g: &Guild) -> Option<u32> {
    g.master().and_then(|m| st.by_cid.get(&m.cid)).copied()
}

fn set_status(st: &mut WorldState, gid: u32, s: i64) {
    let Some(g) = st.guilds.by_id.get_mut(&gid) else { return };
    g.status = s;
    let g = &st.guilds.by_id[&gid];
    send_to_guild(st, g, None, &status_packet(g));
}

fn level_sum(st: &WorldState, g: &Guild) -> i64 {
    g.members.values().filter(|m| m.active()).map(|m| st.by_cid.get(&m.cid).and_then(|s| st.players.get(s)).map_or(m.level, |p| p.ch.level as i64)).sum()
}

/// À l'entrée en jeu d'un membre : statut, bilan, combat en cours.
pub fn on_enter(st: &WorldState, sid: u32, g: &Guild) {
    if g.record != Record::default() {
        send_to(st, sid, record_packet(g));
    }
    if let Some(b) = &g.battle {
        send_to(st, sid, packet(op::BATTLE_INFO, g, Some(full_battle(st, g, b))));
    }
    if g.status != status::NONE {
        send_to(st, sid, status_packet(g));
    }
}

pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some((_, gid, my_rank)) = mine(st, sid) else { return Ok(()) };
    if my_rank != rank::MASTER {
        send_to(st, sid, system_packet(code::NOT_MASTER, None, None));
        return Ok(());
    }
    let b = p.bytes(17).and_then(|b| decode_mini(&mini::GUILD_BATTLE, b).ok()).unwrap_or_else(bfields);
    let enemy_name = b.text(0).unwrap_or_default();
    match p.int(0) {
        Some(op::REQUEST) => request(st, sid, gid, &enemy_name, b.int(5).unwrap_or(0), b.int(6).unwrap_or(0), b.int(7).unwrap_or(0)),
        Some(op::ACCEPT) => accept(st, gid, ctx.config.world.guild_battle_ready_secs.max(0)),
        Some(op::REJECT) => reject(st, gid),
        Some(op::CANCEL_REQUEST) => cancel_request(st, sid, gid),
        Some(op::CANCEL_ACCEPT) => cancel_answer(st, gid, true),
        Some(op::CANCEL_REJECT) => cancel_answer(st, gid, false),
        Some(op::WITHDRAW) => withdraw(ctx, st, sid, gid).await?,
        _ => {}
    }
    Ok(())
}

/// Op 15 : le chef de `gid` défie la guilde `enemy_name`.
fn request(st: &mut WorldState, sid: u32, gid: u32, enemy_name: &str, kind_: i64, duration: i64, person: i64) {
    let refuse = |st: &WorldState, c: i64, n: Option<i64>| send_to(st, sid, system_packet(c, None, n));
    let Some(eid) = st.guilds.named(enemy_name).map(|g| g.id) else { return refuse(st, bcode::NOT_EXIST_GUILD, None) };
    if eid == gid || !(kind::POINT..=kind::PRIVATE_SURVIVAL).contains(&kind_) {
        return;
    }
    let (g, e) = (&st.guilds.by_id[&gid], &st.guilds.by_id[&eid]);
    if g.status != status::NONE || g.offer.is_some() {
        return refuse(st, bcode::NOT_REQUEST_STATUS, None);
    }
    if e.status != status::NONE || e.offer.is_some() {
        return refuse(st, bcode::NOT_REQUEST_STATUS2, None);
    }
    let Some(esid) = master_sid(st, e) else { return refuse(st, bcode::ENEMY_MASTER_OFFLINE, None) };
    if super::super::option_flags::refuses(st, esid, REFUSE_GUILD_BATTLE) {
        let name = st.players[&esid].ch.name.clone();
        return send_to(st, sid, system_packet(bcode::REFUSE, Some(&name), None));
    }
    if g.active_members() < NEED_MEMBERS {
        return refuse(st, bcode::NOT_ENOUGH_MEMBER, Some(NEED_MEMBERS as i64));
    }
    if e.active_members() < NEED_MEMBERS {
        return refuse(st, bcode::NOT_ENOUGH_MEMBER2, Some(NEED_MEMBERS as i64));
    }
    if level_sum(st, g) < NEED_LEVEL_SUM {
        return send_to(st, sid, system_numbers(bcode::NOT_ENOUGH_LEVEL, None, &[0, NEED_LEVEL_SUM]));
    }
    if level_sum(st, e) < NEED_LEVEL_SUM {
        return send_to(st, sid, system_numbers(bcode::NOT_ENOUGH_LEVEL2, None, &[0, NEED_LEVEL_SUM]));
    }
    let duration = duration.clamp(MIN_DURATION, MAX_DURATION);
    let offer = Offer { from: gid, to: eid, kind: kind_, duration, person: person.max(0), at: now() };
    let master = g.master().map(|m| m.name.clone()).unwrap_or_default();
    let ask = build_packet(
        &schema::GUILD,
        &guild(op::REQUEST).with(2, e.name.as_str()).with(3, master.as_str()).with(17, battle_mini(bfields().with(0, g.name.as_str()).with(1, master.as_str()).with(5, kind_).with(6, duration).with(7, offer.person))),
    );
    send_to(st, esid, ask);
    info!("[guilde] {} défie {} (type {kind_}, {} min)", g.name, e.name, duration / 60);
    st.guilds.by_id.get_mut(&gid).unwrap().offer = Some(offer.clone());
    st.guilds.by_id.get_mut(&eid).unwrap().offer = Some(offer);
    send_to(st, sid, system_packet(bcode::REQUEST, None, None));
    set_status(st, gid, status::DECLARE);
}

/// Demande en cours adressée à `gid`.
fn offer_to(st: &WorldState, gid: u32) -> Option<Offer> {
    st.guilds.by_id.get(&gid)?.offer.clone().filter(|o| o.to == gid)
}

fn clear_offer(st: &mut WorldState, o: &Offer) {
    for id in [o.from, o.to] {
        if let Some(g) = st.guilds.by_id.get_mut(&id) {
            g.offer = None;
        }
    }
}

/// Op 16 : la guilde défiée accepte ; le combat commence dans `ready` secondes.
fn accept(st: &mut WorldState, gid: u32, ready: i64) {
    let Some(o) = offer_to(st, gid) else { return };
    clear_offer(st, &o);
    if !st.guilds.by_id.contains_key(&o.from) {
        return;
    }
    let t = now();
    for (me, enemy) in [(o.from, o.to), (o.to, o.from)] {
        let g = st.guilds.by_id.get_mut(&me).unwrap();
        g.battle = Some(Battle { enemy, kind: o.kind, duration: o.duration, person: o.person, accept_at: t, ready, start_at: t + ready, ..Default::default() });
    }
    for id in [o.from, o.to] {
        set_status(st, id, status::READY);
        let g = &st.guilds.by_id[&id];
        let b = g.battle.clone().unwrap();
        send_to_guild(st, g, None, &packet(op::ACCEPT, g, Some(full_battle(st, g, &b))));
    }
    info!("[guilde] {} accepte le combat contre {} : début dans {ready} s", st.guilds.by_id[&o.to].name, st.guilds.by_id[&o.from].name);
}

/// Op 17 : la guilde défiée refuse.
fn reject(st: &mut WorldState, gid: u32) {
    let Some(o) = offer_to(st, gid) else { return };
    clear_offer(st, &o);
    set_status(st, o.from, status::NONE);
    if let Some(msid) = st.guilds.by_id.get(&o.from).and_then(|g| master_sid(st, g)) {
        send_to(st, msid, system_packet(bcode::REJECT_BY_OTHER, None, None));
    }
}

/// Op 18 : un chef demande l'annulation (avant le début, plus d'une minute avant ; ou pendant le combat).
fn cancel_request(st: &mut WorldState, sid: u32, gid: u32) {
    let Some(b) = st.guilds.by_id[&gid].battle.clone() else { return };
    let left = b.start_at - now();
    if st.guilds.by_id[&gid].status == status::READY && left < CANCEL_ENABLE_SECS {
        return send_to(st, sid, system_packet(bcode::CANCEL_PASSED_TIME, None, Some(CANCEL_ENABLE_SECS)));
    }
    let Some(e) = st.guilds.by_id.get(&b.enemy) else { return };
    let Some(esid) = master_sid(st, e) else { return send_to(st, sid, system_packet(bcode::ENEMY_MASTER_OFFLINE, None, None)) };
    let g = &st.guilds.by_id[&gid];
    send_to(st, esid, packet(op::CANCEL_REQUEST, e, Some(battle_mini(bfields().with(0, g.name.as_str())))));
    st.guilds.by_id.get_mut(&gid).unwrap().battle.as_mut().unwrap().cancel_asked = true;
}

/// Op 19 ou 20 : l'autre chef répond à la demande d'annulation.
fn cancel_answer(st: &mut WorldState, gid: u32, yes: bool) {
    let Some(b) = st.guilds.by_id[&gid].battle.clone() else { return };
    let enemy = b.enemy;
    if !st.guilds.by_id.get(&enemy).and_then(|e| e.battle.as_ref()).is_some_and(|eb| eb.cancel_asked) {
        return;
    }
    if !yes {
        st.guilds.by_id.get_mut(&enemy).unwrap().battle.as_mut().unwrap().cancel_asked = false;
        if let Some(msid) = master_sid(st, &st.guilds.by_id[&enemy]) {
            send_to(st, msid, system_packet(bcode::CANCEL_REJECT_BY_OTHER, None, None));
        }
        return;
    }
    for (me, other) in [(gid, enemy), (enemy, gid)] {
        let g = &st.guilds.by_id[&me];
        let name = st.guilds.by_id[&other].name.clone();
        send_to_guild(st, g, None, &packet(op::CANCEL_ACCEPT, g, Some(battle_mini(bfields().with(0, name.as_str())))));
        st.guilds.by_id.get_mut(&me).unwrap().battle = None;
        set_status(st, me, status::NONE);
    }
    info!("[guilde] combat {} contre {} annulé", st.guilds.by_id[&gid].name, st.guilds.by_id[&enemy].name);
}

/// Op 24 : abandon d'un chef ; l'autre guilde gagne.
async fn withdraw(ctx: &WorldCtx, st: &mut WorldState, sid: u32, gid: u32) -> anyhow::Result<()> {
    let g = &st.guilds.by_id[&gid];
    if g.status != status::BATTLE {
        send_to(st, sid, system_packet(bcode::NOT_WITHDRAW_UNTIL_START, None, None));
        return Ok(());
    }
    let Some(enemy) = g.battle.as_ref().map(|b| b.enemy) else { return Ok(()) };
    finish(ctx, st, enemy, gid, true).await
}

/// Fin du combat : `winner` gagne (`None` : nul, d'après les scores) ; `by_withdraw` : par abandon.
async fn finish(ctx: &WorldCtx, st: &mut WorldState, a: u32, b: u32, by_withdraw: bool) -> anyhow::Result<()> {
    let score = |st: &WorldState, id: u32| st.guilds.by_id.get(&id).and_then(|g| g.battle.as_ref()).map_or(0, |x| x.score);
    let (sa, sb) = (score(st, a), score(st, b));
    // `a` est le vainqueur désigné (abandon, mort du chef, dernier debout) ; sinon on compare les scores.
    let (ra, rb) = if by_withdraw {
        (result::WIN_BY_WITHDRAW, result::LOSE_BY_WITHDRAW)
    } else if sa > sb || st.guilds.by_id.get(&b).and_then(|g| g.battle.as_ref()).is_some_and(|x| x.result == result::LOSE) {
        (result::WIN, result::LOSE)
    } else if sa < sb {
        (result::LOSE, result::WIN)
    } else {
        (result::DRAW, result::DRAW)
    };
    for (id, r) in [(a, ra), (b, rb)] {
        let Some(g) = st.guilds.by_id.get_mut(&id) else { continue };
        let Some(bt) = g.battle.as_mut() else { continue };
        bt.result = r;
        let points = bt.score;
        match r {
            result::WIN | result::WIN_BY_WITHDRAW => g.record.win += 1,
            result::LOSE | result::LOSE_BY_WITHDRAW => g.record.lose += 1,
            _ => g.record.draw += 1,
        }
        g.record.points += points;
        let rec = g.record;
        ctx.db.characters.exec("UPDATE guild SET win = ?, draw = ?, lose = ?, points = ? WHERE id = ?", (rec.win, rec.draw, rec.lose, rec.points, id)).await?;
    }
    for id in [a, b] {
        let Some(g) = st.guilds.by_id.get(&id) else { continue };
        let Some(bt) = g.battle.clone() else { continue };
        send_to_guild(st, g, None, &packet(op::RESULT, g, Some(full_battle(st, g, &bt))));
        send_to_guild(st, g, None, &record_packet(g));
    }
    for id in [a, b] {
        if let Some(g) = st.guilds.by_id.get_mut(&id) {
            g.battle = None;
        }
        set_status(st, id, status::NONE);
    }
    info!("[guilde] fin du combat : {} {sa} – {sb} {} (résultat {ra})", st.guilds.by_id.get(&a).map_or("?", |g| g.name.as_str()), st.guilds.by_id.get(&b).map_or("?", |g| g.name.as_str()));
    Ok(())
}

/// Chaque tick : demandes échues, débuts, horloge, fins.
pub async fn step(ctx: &WorldCtx, st: &mut WorldState, at: Instant) -> anyhow::Result<()> {
    if st.guilds.battle_clock.is_some_and(|t| at.duration_since(t) < CLOCK_EVERY) {
        return Ok(());
    }
    st.guilds.battle_clock = Some(at);
    let t = now();
    let stale: Vec<Offer> = st.guilds.by_id.values().filter_map(|g| g.offer.clone()).filter(|o| t - o.at > OFFER_SECS).collect();
    for o in stale {
        clear_offer(st, &o);
        set_status(st, o.from, status::NONE);
    }
    let ids: Vec<u32> = st.guilds.by_id.values().filter(|g| g.battle.is_some()).map(|g| g.id).collect();
    let mut ended: Vec<(u32, u32)> = Vec::new();
    for id in ids {
        let Some(g) = st.guilds.by_id.get(&id) else { continue };
        let Some(b) = g.battle.clone() else { continue };
        if g.status == status::READY && t >= b.start_at {
            start(st, id);
        } else if g.status == status::BATTLE {
            if t >= b.start_at + b.duration {
                if id < b.enemy {
                    ended.push((id, b.enemy));
                }
                continue;
            }
            let enemy = st.guilds.by_id.get(&b.enemy).map(|e| e.name.clone()).unwrap_or_default();
            send_to_guild(st, g, None, &packet(op::UPDATE_TIME, g, Some(battle_mini(bfields().with(0, enemy.as_str()).with(8, t)))));
        }
    }
    for (a, b) in ended {
        finish(ctx, st, a, b, false).await?;
    }
    Ok(())
}

/// Début du combat de `gid` (op 21) ; aux types 3 et 4, les combattants sont les membres en ligne.
fn start(st: &mut WorldState, gid: u32) {
    let g = &st.guilds.by_id[&gid];
    let mut b = g.battle.clone().unwrap();
    if matches!(b.kind, kind::TOTAL_SURVIVAL | kind::PRIVATE_SURVIVAL) {
        let mut online: Vec<u32> = g.members.values().filter(|m| m.active() && st.by_cid.contains_key(&m.cid)).map(|m| m.cid).collect();
        online.truncate(if b.person > 0 { b.person as usize } else { online.len() });
        b.fighters = online;
    }
    st.guilds.by_id.get_mut(&gid).unwrap().battle = Some(b.clone());
    set_status(st, gid, status::BATTLE);
    let g = &st.guilds.by_id[&gid];
    send_to_guild(st, g, None, &packet(op::START, g, Some(full_battle(st, g, &b))));
    info!("[guilde] {} entre en combat", g.name);
}

/// Deux personnages se battent-ils dans un combat de guildes en cours ?
pub fn enemies(st: &WorldState, a: u32, b: u32) -> bool {
    let (Some(ga), Some(gb)) = (st.guilds.of(a), st.guilds.of(b)) else { return false };
    let Some(ba) = ga.battle.as_ref() else { return false };
    if ga.status != status::BATTLE || ba.enemy != gb.id {
        return false;
    }
    let fights = |g: &Guild, cid: u32| g.battle.as_ref().is_some_and(|x| x.fighters.is_empty() || (x.fighters.contains(&cid) && !x.dead.contains(&cid)));
    fights(ga, a) && fights(gb, b)
}

/// Un membre de guilde en tue un autre : score, mort du chef, derniers debout.
pub async fn on_kill(ctx: &WorldCtx, st: &mut WorldState, killer: u32, victim: u32) -> anyhow::Result<()> {
    if !enemies(st, killer, victim) {
        return Ok(());
    }
    let (ka, vb) = (st.guilds.of_cid[&killer], st.guilds.of_cid[&victim]);
    {
        let g = st.guilds.by_id.get_mut(&ka).unwrap();
        let b = g.battle.as_mut().unwrap();
        b.score += 1;
        b.up_score = 1;
        let e = st.guilds.by_id.get_mut(&vb).unwrap();
        let eb = e.battle.as_mut().unwrap();
        eb.up_score = 0;
        if !eb.dead.contains(&victim) {
            eb.dead.push(victim);
        }
    }
    for (me, other) in [(ka, vb), (vb, ka)] {
        let g = &st.guilds.by_id[&me];
        let (b, eb) = (g.battle.as_ref().unwrap(), st.guilds.by_id[&other].battle.as_ref().unwrap());
        let other_name = st.guilds.by_id[&other].name.clone();
        let f = bfields().with(0, other_name.as_str()).with(9, b.score).with(10, eb.score).with(11, b.up_score).with(12, eb.up_score);
        send_to_guild(st, g, None, &packet(op::UPDATE_SCORE, g, Some(battle_mini(f))));
    }
    let (vg, kg) = (&st.guilds.by_id[&vb], &st.guilds.by_id[&ka]);
    let b = vg.battle.as_ref().unwrap();
    let over = match b.kind {
        kind::CHIEF_DEATH => vg.members.get(&victim).is_some_and(|m| m.rank == rank::MASTER),
        kind::TOTAL_SURVIVAL | kind::PRIVATE_SURVIVAL => !b.fighters.is_empty() && b.fighters.iter().all(|c| b.dead.contains(c)),
        kind::POINT | kind::KNOCKOUT => false,
        _ => false,
    };
    debug!("[guilde] {} : {} tue un membre de {}", kg.name, killer, vg.name);
    if over {
        st.guilds.by_id.get_mut(&vb).unwrap().battle.as_mut().unwrap().result = result::LOSE;
        finish(ctx, st, ka, vb, false).await?;
    }
    Ok(())
}

/// Coup d'un joueur sur un membre de la guilde adverse (comme sur un monstre : portée, précision, dégâts
/// physiques et élémentaires, critiques ; cadence de l'attaque).
pub async fn attack_step(ctx: &WorldCtx, st: &mut WorldState, sid: u32, target: u32, now_: Instant) -> anyhow::Result<()> {
    let Some(tsid) = st.by_cid.get(&target).copied() else {
        st.players.get_mut(&sid).unwrap().attack = None;
        return Ok(());
    };
    let (p, t) = (&st.players[&sid], &st.players[&tsid]);
    if t.dead || !t.in_game || !enemies(st, p.ch.id, target) {
        st.players.get_mut(&sid).unwrap().attack = None;
        return Ok(());
    }
    let reach = p.factors.attack[0].max(50) as f64 + 50.0;
    if combat::dist2d(p.ch.pos(), t.ch.pos()) > reach + REACH_TOLERANCE {
        return Ok(());
    }
    let attacker = Attacker::of_player(p);
    let defender = attacker.pierce(&Defender::of_player(&t.factors));
    let parts = {
        let mut rng = rand::thread_rng();
        let (outcome, damage) = combat::strike(&mut rng, &attacker, &defender);
        let (outcome, damage) = crate::world::effects::critical(&mut rng, &crate::world::effects::of_player(p), outcome, damage);
        let mut parts = vec![(combat::RESULT_PHYSICAL, damage)];
        if damage > 0 {
            let chance = combat::spirit_chance(true, attacker.level, defender.level);
            parts.extend(combat::spirit_hits(&mut rng, chance, &combat::spirit_ranges(&p.factors), &defender.resist, true));
        }
        (outcome, parts)
    };
    let (outcome, parts) = parts;
    let speed = p.factors.attack[2].max(1) as u64;
    let cid = p.ch.id;
    let me = st.players.get_mut(&sid).unwrap();
    me.attack = Some(combat::Attack { target, ready_at: now_ + Duration::from_millis(60_000 / speed), forced: false });
    me.dest = None;
    crate::world::recovery::in_combat(&mut me.timers, now_);
    if st.players[&tsid].invincible_until.is_some_and(|u| now_ < u) {
        return Ok(());
    }
    combat::hit_player(st, tsid, cid, outcome, &parts);
    if st.players.get(&tsid).is_some_and(|x| x.dead) {
        st.players.get_mut(&sid).unwrap().attack = None;
        on_kill(ctx, st, cid, target).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mini_du_combat() {
        let b = battle_mini(bfields().with(0, "SimLead").with(1, "GmTest").with(3, 180).with(5, kind::CHIEF_DEATH).with(6, 600).with(9, 3).with(13, result::WIN));
        let f = decode_mini(&mini::GUILD_BATTLE, archcore_net::packet::mini_payload(&b)).unwrap();
        assert_eq!((f.text(0).as_deref(), f.int(3), f.int(5), f.int(6), f.int(9), f.int(13)), (Some("SimLead"), Some(180), Some(2), Some(600), Some(3), Some(1)));
    }
}
