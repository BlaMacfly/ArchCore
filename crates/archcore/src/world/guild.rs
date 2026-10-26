//! Guildes (AgpmGuild, 0x15) et gestionnaires de guilde (AgpmEventGuild, 0x39).
//!
//! ```text
//!   Gestionnaire (PNJ à fonction 23) : op 0 (événement) ← op 1 (accord : le client ouvre la création).
//!   op 0  création : nom (champ 2, lettres et chiffres), mot de passe (9). Niveau 20, prix et taille de la
//!         première ligne de guildmaxmember.txt. ← op 0 (guilde), op 8 (membre lié au CID), message 11.
//!   op 1  invitation par le chef ou le second : nom de l'invité dans le membre (16) → op 1 à l'invité.
//!   op 2  refus de l'invité (nom de la guilde) → message 14 au chef.
//!   op 3  adhésion : sans guilde, nom de la guilde au champ 2 — membre si invité, sinon demande d'adhésion
//!         (rang 2) ; dans la guilde, le chef ou le second accepte la demande du membre nommé (16).
//!   op 4  départ : un membre le demande (rang 3, message 50) ; un demandeur d'adhésion part aussitôt.
//!   op 29 départ accepté par le chef ou le second (membre 16) ; sans réponse, il se fait au bout d'un jour.
//!   op 5  exclusion par le chef ou le second (membre 16).
//!   op 6  dissolution par le chef, avec le mot de passe donné à la création.
//!   op 9  annonce (champ 10), par le chef ou le second.
//!   op 10 message système : code (champ 1), textes (2, 3), nombre (4).
//! ```
//! À l'entrée en jeu, le membre reçoit sa guilde (op 0), chaque membre (op 3) et son lien (op 8) ; les autres
//! membres le voient passer en ligne (op 8). Le lien d'un joueur à sa guilde part aussi à ceux qui le voient.
//! En base : `guild`, `guild_member` (base des personnages) ; tailles et prix : `guild_max_member`.

use super::npc_events::{npc_distance, resolve, MAX_NPC_DISTANCE};
use super::npcs::event_function;
use super::{send_system, WorldCtx, WorldState};
use crate::db::{col, Databases};
use crate::{info, warn};
use archcore_net::schema::{self, mini};
use archcore_net::{build_mini, build_packet, decode_mini, Fields, Packet, Value};
use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub mod op {
    pub const CREATE: i64 = 0;
    pub const JOIN_REQUEST: i64 = 1;
    pub const JOIN_REJECT: i64 = 2;
    pub const JOIN: i64 = 3;
    pub const LEAVE: i64 = 4;
    pub const FORCED_LEAVE: i64 = 5;
    pub const DESTROY: i64 = 6;
    pub const CHAR_DATA: i64 = 8;
    pub const UPDATE_NOTICE: i64 = 9;
    pub const SYSTEM_MESSAGE: i64 = 10;
    pub const LEAVE_ALLOW: i64 = 29;
}

/// Rangs d'un membre.
pub mod rank {
    pub const NORMAL: i64 = 1;
    pub const JOIN_REQUEST: i64 = 2;
    pub const LEAVE_REQUEST: i64 = 3;
    pub const MASTER: i64 = 10;
    pub const SUBMASTER: i64 = 11;
}

/// Codes des messages système de guilde.
pub mod code {
    pub const EXIST_GUILD_ID: i64 = 0;
    pub const NEED_MORE_LEVEL: i64 = 1;
    pub const NEED_MORE_MONEY: i64 = 2;
    pub const USE_SPECIAL_CHAR: i64 = 4;
    pub const ALREADY_MEMBER: i64 = 5;
    pub const ALREADY_MEMBER2: i64 = 17;
    /// L'invité refuse les invitations de guilde (options) : « Option_Guild_Off_User ».
    pub const JOIN_REFUSE: i64 = 42;
    pub const MAX_MEMBER: i64 = 6;
    pub const JOIN_FAIL: i64 = 7;
    pub const LEAVE_FAIL: i64 = 8;
    pub const NOT_MASTER: i64 = 9;
    pub const INVALID_PASSWORD: i64 = 10;
    pub const CREATE_COMPLETE: i64 = 11;
    pub const DESTROY: i64 = 12;
    pub const JOIN: i64 = 13;
    pub const JOIN_REJECT: i64 = 14;
    pub const LEAVE: i64 = 15;
    pub const FORCED_LEAVE: i64 = 16;
    pub const JOIN_REQUEST_SELF: i64 = 48;
    pub const LEAVE_REQUEST: i64 = 50;
}

/// AgpmEventGuild : opérations.
pub mod event_op {
    pub const REQUEST: i64 = 0;
    pub const GRANT: i64 = 1;
}

/// Niveau exigé pour créer une guilde.
pub const CREATE_LEVEL: i32 = 20;
/// Une demande de départ sans réponse aboutit au bout d'un jour.
pub const LEAVE_DELAY_SECS: i64 = 24 * 60 * 60;
/// Longueurs maximales (octets) du nom, du mot de passe et de l'annonce.
pub const MAX_NAME: usize = 32;
pub const MAX_PASSWORD: usize = 12;
pub const MAX_NOTICE: usize = 89;
/// Invitations en attente gardées par guilde.
const MAX_INVITED: usize = 8;
/// Fréquence de la vérification des demandes de départ.
const SWEEP_EVERY: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    pub cid: u32,
    pub name: String,
    pub rank: i64,
    pub joined_at: i64,
    pub level: i64,
    pub tid: i64,
    pub online: bool,
    pub leave_requested_at: Option<i64>,
}

impl Member {
    /// Membre à part entière (pas une simple demande d'adhésion).
    pub fn active(&self) -> bool {
        self.rank != rank::JOIN_REQUEST
    }
}

#[derive(Clone, Debug, Default)]
pub struct Guild {
    pub id: u32,
    pub name: String,
    pub password: String,
    pub notice: String,
    pub created_at: i64,
    pub max_members: i64,
    pub members: BTreeMap<u32, Member>,
    /// Personnages invités, en attente de réponse.
    pub invited: Vec<u32>,
}

impl Guild {
    pub fn master(&self) -> Option<&Member> {
        self.members.values().find(|m| m.rank == rank::MASTER)
    }
    pub fn is_full(&self) -> bool {
        self.members.len() as i64 >= self.max_members
    }
    pub fn member_named(&self, name: &str) -> Option<&Member> {
        let key = name.to_lowercase();
        self.members.values().find(|m| m.name.to_lowercase() == key)
    }
}

/// Guildes en mémoire, chargées au démarrage.
#[derive(Default)]
pub struct Guilds {
    pub by_id: HashMap<u32, Guild>,
    /// Personnage → guilde.
    pub of_cid: HashMap<u32, u32>,
    /// Tailles successives et leur prix (guildmaxmember.txt) ; la première est celle d'une guilde neuve.
    pub sizes: Vec<(i64, i64)>,
    last_sweep: Option<Instant>,
}

impl Guilds {
    pub async fn load(db: &Databases) -> anyhow::Result<Self> {
        let mut r = Guilds::default();
        match db.world.rows("SELECT max_members, gold FROM guild_max_member ORDER BY max_members", ()).await {
            Ok(rows) => r.sizes = rows.iter().map(|x| (col(x, "max_members"), col(x, "gold"))).collect(),
            Err(e) => warn!("tailles des guildes non chargées ({e:#}) : lancer `archcore db-init` puis `archcore import`"),
        }
        let Ok(rows) = db.characters.rows("SELECT id, name, password, notice, max_members, UNIX_TIMESTAMP(created_at) AS created FROM guild", ()).await else {
            warn!("guildes non chargées : lancer `archcore db-init`");
            return Ok(r);
        };
        for row in &rows {
            let g = Guild {
                id: col(row, "id"),
                name: col(row, "name"),
                password: col(row, "password"),
                notice: col(row, "notice"),
                created_at: col::<Option<i64>>(row, "created").unwrap_or(0),
                max_members: col(row, "max_members"),
                ..Default::default()
            };
            r.by_id.insert(g.id, g);
        }
        let members = db
            .characters
            .rows(
                "SELECT m.character_id, m.guild_id, m.`rank`, UNIX_TIMESTAMP(m.joined_at) AS joined, UNIX_TIMESTAMP(m.leave_requested_at) AS leaving, \
                 c.name, c.level, c.template_id FROM guild_member m JOIN characters c ON c.id = m.character_id WHERE c.deleted_at IS NULL",
                (),
            )
            .await?;
        for row in &members {
            let gid: u32 = col(row, "guild_id");
            let Some(g) = r.by_id.get_mut(&gid) else { continue };
            let m = Member {
                cid: col(row, "character_id"),
                name: col(row, "name"),
                rank: col(row, "rank"),
                joined_at: col::<Option<i64>>(row, "joined").unwrap_or(0),
                level: col(row, "level"),
                tid: col(row, "template_id"),
                online: false,
                leave_requested_at: col(row, "leaving"),
            };
            r.of_cid.insert(m.cid, gid);
            g.members.insert(m.cid, m);
        }
        info!("guildes : {} guilde(s), {} membre(s)", r.by_id.len(), r.of_cid.len());
        Ok(r)
    }

    pub fn of(&self, cid: u32) -> Option<&Guild> {
        self.of_cid.get(&cid).and_then(|g| self.by_id.get(g))
    }

    pub fn named(&self, name: &str) -> Option<&Guild> {
        let key = name.to_lowercase();
        self.by_id.values().find(|g| g.name.to_lowercase() == key)
    }

    /// Taille et prix d'une guilde neuve.
    pub fn first_size(&self) -> (i64, i64) {
        self.sizes.first().copied().unwrap_or((50, 10_000))
    }
}

/// Rang du chef ou du second, qui dirigent la guilde.
fn leads(r: i64) -> bool {
    r == rank::MASTER || r == rank::SUBMASTER
}

/// Nom de guilde acceptable : 1 à 32 lettres ou chiffres ASCII.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= MAX_NAME && name.chars().all(|c| c.is_ascii_alphanumeric())
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn cut(s: &str, n: usize) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if archcore_net::text::encode(&out).len() + archcore_net::text::encode(&c.to_string()).len() > n {
            break;
        }
        out.push(c);
    }
    out
}

// ------------------------------------------------------------------------------------------- paquets

fn member_mini(m: &Member) -> Vec<u8> {
    let s = &mini::GUILD_MEMBER;
    build_mini(s, &Fields::new(s).with(0, m.name.as_str()).with(1, m.rank).with(2, m.joined_at).with(3, m.level).with(4, m.tid).with(5, m.online as i64))
}

fn name_mini(name: &str) -> Vec<u8> {
    let s = &mini::GUILD_MEMBER;
    build_mini(s, &Fields::new(s).with(0, name))
}

fn guild(op: i64) -> Fields {
    Fields::new(&schema::GUILD).with(0, op)
}

fn build(f: Fields) -> Vec<u8> {
    build_packet(&schema::GUILD, &f)
}

/// Op 0 : la guilde (nom, chef, date, taille, annonce).
pub fn add_packet(g: &Guild) -> Vec<u8> {
    let master = g.master().map(|m| m.name.clone()).unwrap_or_default();
    let mut f = guild(op::CREATE).with(2, g.name.as_str()).with(3, master.as_str()).with(5, 1).with(6, g.created_at).with(7, g.max_members);
    f = f.with(10, archcore_net::text::encode(&g.notice)).with(22, 0).with(23, 0);
    build(f)
}

/// Op 8 : lien d'un personnage (CID) à sa guilde.
pub fn char_data_packet(g: &Guild, m: &Member) -> Vec<u8> {
    build(guild(op::CHAR_DATA).with(1, m.cid).with(2, g.name.as_str()).with(16, member_mini(m)).with(22, 0).with(23, 0))
}

/// Op 3 : un membre (ajouté ou mis à jour).
pub fn join_packet(g: &Guild, m: &Member) -> Vec<u8> {
    build(guild(op::JOIN).with(2, g.name.as_str()).with(16, member_mini(m)))
}

/// Op 4 ou 5 : un membre quitte la guilde.
fn leave_packet(op: i64, g: &Guild, name: &str) -> Vec<u8> {
    build(guild(op).with(2, g.name.as_str()).with(16, name_mini(name)))
}

fn notice_packet(g: &Guild) -> Vec<u8> {
    build(guild(op::UPDATE_NOTICE).with(2, g.name.as_str()).with(10, archcore_net::text::encode(&g.notice)))
}

/// Op 10 : message système (code, texte, nombre).
pub fn system_packet(code: i64, text: Option<&str>, number: Option<i64>) -> Vec<u8> {
    let mut f = guild(op::SYSTEM_MESSAGE).with(1, code).with_opt(2, text);
    if let Some(n) = number {
        let mut list = vec![Value::Int(n)];
        list.resize(8, Value::Int(0));
        f = f.with(4, list);
    }
    build(f)
}

// ------------------------------------------------------------------------------------------- envois

fn send_to(st: &WorldState, sid: u32, pkt: Vec<u8>) {
    if let Some(p) = st.players.get(&sid) {
        p.send_raw(pkt);
    }
}

/// Envoie un paquet aux membres en ligne (demandes d'adhésion exclues), sauf `except`.
fn send_to_guild(st: &WorldState, g: &Guild, except: Option<u32>, pkt: &[u8]) {
    for m in g.members.values().filter(|m| m.active() && Some(m.cid) != except) {
        if let Some(p) = st.by_cid.get(&m.cid).and_then(|sid| st.players.get(sid)) {
            p.send_raw(pkt.to_vec());
        }
    }
}

/// Lien de `cid` à sa guilde, envoyé à lui et aux joueurs qui le voient.
fn announce_link(st: &WorldState, cid: u32) {
    let Some(g) = st.guilds.of(cid) else { return };
    let Some(m) = g.members.get(&cid) else { return };
    let pkt = char_data_packet(g, m);
    for p in st.players.values().filter(|p| p.ch.id == cid || p.seen_players.contains(&cid)) {
        p.send_raw(pkt.clone());
    }
}

/// Lien d'un joueur qui apparaît dans la vue d'un autre (appelé par la présence).
pub fn view_packet(st: &WorldState, cid: u32) -> Option<Vec<u8>> {
    let g = st.guilds.of(cid)?;
    g.members.get(&cid).map(|m| char_data_packet(g, m))
}

/// Entrée en jeu : la guilde, ses membres et le lien du joueur ; les autres membres le voient en ligne.
pub fn on_enter(st: &mut WorldState, sid: u32) {
    let Some(p) = st.players.get(&sid) else { return };
    let (cid, level, tid) = (p.ch.id, p.ch.level as i64, p.ch.template_id as i64);
    let Some(gid) = st.guilds.of_cid.get(&cid).copied() else { return };
    let Some(g) = st.guilds.by_id.get_mut(&gid) else { return };
    if let Some(m) = g.members.get_mut(&cid) {
        m.online = true;
        m.level = level;
        m.tid = tid;
    }
    let g = &st.guilds.by_id[&gid];
    let p = &st.players[&sid];
    p.send_raw(add_packet(g));
    for m in g.members.values() {
        p.send_raw(join_packet(g, m));
    }
    let me = &g.members[&cid];
    p.send_raw(char_data_packet(g, me));
    send_to_guild(st, g, Some(cid), &char_data_packet(g, me));
}

/// Sortie du jeu : les membres le voient hors ligne.
pub fn on_leave_world(st: &mut WorldState, cid: u32) {
    let Some(gid) = st.guilds.of_cid.get(&cid).copied() else { return };
    let Some(g) = st.guilds.by_id.get_mut(&gid) else { return };
    let Some(m) = g.members.get_mut(&cid) else { return };
    m.online = false;
    let g = &st.guilds.by_id[&gid];
    let pkt = char_data_packet(g, &g.members[&cid]);
    send_to_guild(st, g, Some(cid), &pkt);
}

/// Discussion de guilde : aux membres en ligne. Rend faux si le joueur n'a pas de guilde.
pub fn chat(st: &WorldState, cid: u32, pkt: &[u8]) -> bool {
    let Some(g) = st.guilds.of(cid) else { return false };
    if !g.members.get(&cid).is_some_and(Member::active) {
        return false;
    }
    send_to_guild(st, g, None, pkt);
    true
}

// ------------------------------------------------------------------------------------------- réception

/// Nom porté par le membre (champ 16).
fn member_name(p: &Packet) -> Option<String> {
    let f = decode_mini(&mini::GUILD_MEMBER, p.bytes(16)?).ok()?;
    f.text(0).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    if !st.players.get(&sid).is_some_and(|x| x.in_game) {
        return Ok(());
    }
    match p.int(0) {
        Some(op::CREATE) => create(ctx, st, sid, p).await,
        Some(op::JOIN_REQUEST) => {
            invite(st, sid, p);
            Ok(())
        }
        Some(op::JOIN_REJECT) => {
            reject(st, sid, p);
            Ok(())
        }
        Some(op::JOIN) => join(ctx, st, sid, p).await,
        Some(op::LEAVE) => leave(ctx, st, sid).await,
        Some(op::LEAVE_ALLOW) => allow_leave(ctx, st, sid, p).await,
        Some(op::FORCED_LEAVE) => kick(ctx, st, sid, p).await,
        Some(op::DESTROY) => destroy(ctx, st, sid, p).await,
        Some(op::UPDATE_NOTICE) => notice(ctx, st, sid, p).await,
        op => {
            crate::debug!("[guilde] #{sid} AGPMGUILD op {op:?} (pas encore géré)");
            Ok(())
        }
    }
}

/// Guilde et rang du joueur `sid`.
fn mine(st: &WorldState, sid: u32) -> Option<(u32, u32, i64)> {
    let cid = st.players.get(&sid)?.ch.id;
    let g = st.guilds.of(cid)?;
    Some((cid, g.id, g.members.get(&cid)?.rank))
}

async fn create(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let name = p.text(2).unwrap_or_default().trim().to_string();
    let password = cut(p.text(9).unwrap_or_default().trim(), MAX_PASSWORD);
    let player = &st.players[&sid];
    let (cid, level, money) = (player.ch.id, player.ch.level, player.ch.money as i64);
    let (size, cost) = st.guilds.first_size();
    let refusal = if st.guilds.of(cid).is_some() {
        Some((code::ALREADY_MEMBER, None))
    } else if st.guilds.named(&name).is_some() {
        Some((code::EXIST_GUILD_ID, None))
    } else if level < CREATE_LEVEL {
        Some((code::NEED_MORE_LEVEL, None))
    } else if money < cost {
        Some((code::NEED_MORE_MONEY, Some(cost)))
    } else if !valid_name(&name) {
        Some((code::USE_SPECIAL_CHAR, None))
    } else {
        None
    };
    if let Some((c, n)) = refusal {
        send_to(st, sid, system_packet(c, None, n));
        return Ok(());
    }
    let left = money - cost;
    let created = ctx
        .db
        .characters
        .atomic(vec![
            (super::SAVE_MONEY, (left, cid).into()),
            ("INSERT INTO guild (name, password, max_members) VALUES (?, ?, ?)", (name.as_str(), password.as_str(), size).into()),
            ("INSERT INTO guild_member (character_id, guild_id, `rank`) SELECT ?, id, ? FROM guild WHERE name = ?", (cid, rank::MASTER, name.as_str()).into()),
        ])
        .await;
    if let Err(e) = created {
        if format!("{e:#}").contains("1062") {
            send_to(st, sid, system_packet(code::EXIST_GUILD_ID, None, None));
            return Ok(());
        }
        return Err(e);
    }
    let Some(row) = ctx.db.characters.first("SELECT id FROM guild WHERE name = ?", (name.as_str(),)).await? else { return Ok(()) };
    let gid: u32 = col(&row, "id");
    let player = st.players.get_mut(&sid).unwrap();
    super::show_money(player, left as u64);
    let master = Member {
        cid,
        name: player.ch.name.clone(),
        rank: rank::MASTER,
        joined_at: now(),
        level: level as i64,
        tid: player.ch.template_id as i64,
        online: true,
        leave_requested_at: None,
    };
    let mut g = Guild { id: gid, name: name.clone(), password, created_at: now(), max_members: size, ..Default::default() };
    g.members.insert(cid, master);
    st.guilds.of_cid.insert(cid, gid);
    st.guilds.by_id.insert(gid, g);
    let g = &st.guilds.by_id[&gid];
    send_to(st, sid, add_packet(g));
    send_to(st, sid, join_packet(g, &g.members[&cid]));
    announce_link(st, cid);
    send_to(st, sid, system_packet(code::CREATE_COMPLETE, None, None));
    info!("[guilde] {} crée la guilde {name} ({cost} pièce(s))", st.players[&sid].ch.name);
    Ok(())
}

/// Op 1 : le chef ou le second invite un joueur en ligne sans guilde.
fn invite(st: &mut WorldState, sid: u32, p: &Packet) {
    let Some((_, gid, my_rank)) = mine(st, sid) else { return };
    if !leads(my_rank) {
        send_to(st, sid, system_packet(code::NOT_MASTER, None, None));
        return;
    }
    let Some(target_name) = member_name(p) else { return };
    let Some(tsid) = st.by_name(&target_name) else {
        send_system(st, sid, &format!("{target_name} n'est pas connecté."));
        return;
    };
    let tcid = st.players[&tsid].ch.id;
    if super::option_flags::refuses(st, tsid, super::option_flags::REFUSE_GUILD_IN) {
        info!("[guilde] {target_name} refuse les invitations de guilde");
        send_to(st, sid, system_packet(code::JOIN_REFUSE, Some(&target_name), None));
        return;
    }
    if st.guilds.of(tcid).is_some() {
        send_to(st, sid, system_packet(code::ALREADY_MEMBER2, Some(&target_name), None));
        return;
    }
    let g = st.guilds.by_id.get_mut(&gid).unwrap();
    if g.invited.contains(&tcid) {
        return;
    }
    if g.invited.len() >= MAX_INVITED {
        g.invited.clear();
    }
    g.invited.push(tcid);
    let from = st.players[&sid].ch.name.clone();
    let to = st.players[&tsid].ch.name.clone();
    let g = &st.guilds.by_id[&gid];
    send_to(st, tsid, build(guild(op::JOIN_REQUEST).with(2, g.name.as_str()).with(3, from.as_str()).with(16, name_mini(&to))));
    info!("[guilde] {from} invite {to} dans {}", g.name);
}

/// Op 2 : l'invité refuse.
fn reject(st: &mut WorldState, sid: u32, p: &Packet) {
    let cid = st.players[&sid].ch.id;
    let Some(gid) = st.guilds.named(&p.text(2).unwrap_or_default()).map(|g| g.id) else { return };
    let g = st.guilds.by_id.get_mut(&gid).unwrap();
    let Some(i) = g.invited.iter().position(|c| *c == cid) else { return };
    g.invited.swap_remove(i);
    let g = &st.guilds.by_id[&gid];
    if let Some(msid) = g.master().and_then(|m| st.by_cid.get(&m.cid)).copied() {
        send_to(st, msid, system_packet(code::JOIN_REJECT, Some(&st.players[&sid].ch.name), None));
    }
}

async fn join(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    if let Some((_, gid, my_rank)) = mine(st, sid) {
        // Dans une guilde : le chef ou le second accepte une demande d'adhésion.
        if !leads(my_rank) {
            return Ok(());
        }
        let Some(name) = member_name(p) else { return Ok(()) };
        let Some(m) = st.guilds.by_id[&gid].member_named(&name).filter(|m| m.rank == rank::JOIN_REQUEST).cloned() else { return Ok(()) };
        ctx.db.characters.exec("UPDATE guild_member SET `rank` = ? WHERE character_id = ?", (rank::NORMAL, m.cid)).await?;
        let g = st.guilds.by_id.get_mut(&gid).unwrap();
        g.members.get_mut(&m.cid).unwrap().rank = rank::NORMAL;
        let g = &st.guilds.by_id[&gid];
        let joined = &g.members[&m.cid];
        send_to_guild(st, g, None, &join_packet(g, joined));
        send_to_guild(st, g, Some(m.cid), &system_packet(code::JOIN, Some(&m.name), None));
        if let Some(msid) = st.by_cid.get(&m.cid).copied() {
            for other in g.members.values() {
                send_to(st, msid, join_packet(g, other));
            }
            send_to(st, msid, system_packet(code::JOIN, Some(&m.name), None));
        }
        announce_link(st, m.cid);
        info!("[guilde] {} accepte {} dans {}", st.players[&sid].ch.name, m.name, g.name);
        return Ok(());
    }
    // Sans guilde : réponse à une invitation, ou demande d'adhésion.
    let Some(gid) = st.guilds.named(&p.text(2).unwrap_or_default()).map(|g| g.id) else {
        send_to(st, sid, system_packet(code::JOIN_FAIL, None, None));
        return Ok(());
    };
    let player = &st.players[&sid];
    let (cid, name, level, tid) = (player.ch.id, player.ch.name.clone(), player.ch.level as i64, player.ch.template_id as i64);
    let g = &st.guilds.by_id[&gid];
    if g.is_full() {
        send_to(st, sid, system_packet(code::MAX_MEMBER, None, None));
        return Ok(());
    }
    let invited = g.invited.contains(&cid);
    let (r, c) = if invited { (rank::NORMAL, code::JOIN) } else { (rank::JOIN_REQUEST, code::JOIN_REQUEST_SELF) };
    ctx.db.characters.exec("INSERT INTO guild_member (character_id, guild_id, `rank`) VALUES (?, ?, ?)", (cid, gid, r)).await?;
    let m = Member { cid, name: name.clone(), rank: r, joined_at: now(), level, tid, online: true, leave_requested_at: None };
    let g = st.guilds.by_id.get_mut(&gid).unwrap();
    g.invited.retain(|x| *x != cid);
    g.members.insert(cid, m);
    st.guilds.of_cid.insert(cid, gid);
    let g = &st.guilds.by_id[&gid];
    let m = &g.members[&cid];
    send_to(st, sid, add_packet(g));
    send_to(st, sid, char_data_packet(g, m));
    send_to_guild(st, g, Some(cid), &join_packet(g, m));
    send_to_guild(st, g, Some(cid), &system_packet(c, Some(&name), None));
    if invited {
        for other in g.members.values() {
            send_to(st, sid, join_packet(g, other));
        }
        announce_link(st, cid);
    }
    send_to(st, sid, system_packet(c, Some(&name), None));
    info!("[guilde] {name} {} {}", if invited { "rejoint" } else { "demande à rejoindre" }, g.name);
    Ok(())
}

/// Retire un membre : base, mémoire, puis `op` (départ ou exclusion) à la guilde et à lui, et le message.
async fn remove_member(ctx: &WorldCtx, st: &mut WorldState, gid: u32, cid: u32, leave_op: i64, msg: i64) -> anyhow::Result<()> {
    ctx.db.characters.exec("DELETE FROM guild_member WHERE character_id = ?", (cid,)).await?;
    let Some(g) = st.guilds.by_id.get(&gid) else { return Ok(()) };
    let Some(m) = g.members.get(&cid).cloned() else { return Ok(()) };
    let pkt = leave_packet(leave_op, g, &m.name);
    let sys = system_packet(msg, Some(&m.name), None);
    send_to_guild(st, g, None, &pkt);
    send_to_guild(st, g, Some(cid), &sys);
    if let Some(msid) = st.by_cid.get(&cid).copied() {
        if !m.active() {
            send_to(st, msid, pkt.clone());
        }
        send_to(st, msid, sys);
    }
    st.guilds.by_id.get_mut(&gid).unwrap().members.remove(&cid);
    st.guilds.of_cid.remove(&cid);
    info!("[guilde] {} quitte {} (op {leave_op})", m.name, st.guilds.by_id[&gid].name);
    Ok(())
}

/// Op 4 : un membre demande à partir ; un demandeur d'adhésion retire sa demande.
async fn leave(ctx: &WorldCtx, st: &mut WorldState, sid: u32) -> anyhow::Result<()> {
    let Some((cid, gid, my_rank)) = mine(st, sid) else { return Ok(()) };
    match my_rank {
        rank::MASTER => send_to(st, sid, system_packet(code::LEAVE_FAIL, None, None)),
        rank::JOIN_REQUEST => remove_member(ctx, st, gid, cid, op::LEAVE, code::LEAVE).await?,
        rank::LEAVE_REQUEST => {}
        _ => {
            ctx.db.characters.exec("UPDATE guild_member SET `rank` = ?, leave_requested_at = NOW() WHERE character_id = ?", (rank::LEAVE_REQUEST, cid)).await?;
            let g = st.guilds.by_id.get_mut(&gid).unwrap();
            let m = g.members.get_mut(&cid).unwrap();
            m.rank = rank::LEAVE_REQUEST;
            m.leave_requested_at = Some(now());
            let g = &st.guilds.by_id[&gid];
            send_to_guild(st, g, None, &join_packet(g, &g.members[&cid]));
            // Le message nomme le membre : le chef et le second y répondent.
            let name = g.members[&cid].name.clone();
            send_to_guild(st, g, None, &system_packet(code::LEAVE_REQUEST, Some(&name), None));
            info!("[guilde] {} demande à quitter {}", st.players[&sid].ch.name, g.name);
        }
    }
    Ok(())
}

/// Op 29 : le chef ou le second accepte un départ demandé.
async fn allow_leave(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some((_, gid, my_rank)) = mine(st, sid) else { return Ok(()) };
    if !leads(my_rank) {
        return Ok(());
    }
    let Some(name) = member_name(p) else { return Ok(()) };
    let Some(cid) = st.guilds.by_id[&gid].member_named(&name).filter(|m| m.rank == rank::LEAVE_REQUEST).map(|m| m.cid) else { return Ok(()) };
    remove_member(ctx, st, gid, cid, op::LEAVE, code::LEAVE).await
}

/// Op 5 : exclusion par le chef ou le second (le chef ne peut pas être exclu).
async fn kick(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some((me, gid, my_rank)) = mine(st, sid) else { return Ok(()) };
    if !leads(my_rank) {
        send_to(st, sid, system_packet(code::NOT_MASTER, None, None));
        return Ok(());
    }
    let Some(name) = member_name(p) else { return Ok(()) };
    let Some(target) = st.guilds.by_id[&gid].member_named(&name).cloned() else { return Ok(()) };
    if target.cid == me || target.rank == rank::MASTER || (target.rank == rank::SUBMASTER && my_rank != rank::MASTER) {
        return Ok(());
    }
    remove_member(ctx, st, gid, target.cid, op::FORCED_LEAVE, code::FORCED_LEAVE).await
}

/// Op 6 : dissolution par le chef, avec le mot de passe de la guilde.
async fn destroy(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some((_, gid, my_rank)) = mine(st, sid) else { return Ok(()) };
    if my_rank != rank::MASTER {
        send_to(st, sid, system_packet(code::NOT_MASTER, None, None));
        return Ok(());
    }
    let given = p.text(9).unwrap_or_default();
    let g = &st.guilds.by_id[&gid];
    if !g.password.is_empty() && given.trim() != g.password {
        send_to(st, sid, system_packet(code::INVALID_PASSWORD, None, None));
        return Ok(());
    }
    ctx.db.characters.exec("DELETE FROM guild WHERE id = ?", (gid,)).await?;
    let pkt = build(guild(op::DESTROY).with(2, g.name.as_str()));
    send_to_guild(st, g, None, &pkt);
    send_to_guild(st, g, None, &system_packet(code::DESTROY, Some(&g.name), None));
    let g = st.guilds.by_id.remove(&gid).unwrap();
    for cid in g.members.keys() {
        st.guilds.of_cid.remove(cid);
    }
    info!("[guilde] {} dissout {}", st.players[&sid].ch.name, g.name);
    Ok(())
}

/// Op 9 : nouvelle annonce, par le chef ou le second.
async fn notice(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some((_, gid, my_rank)) = mine(st, sid) else { return Ok(()) };
    if !leads(my_rank) {
        send_to(st, sid, system_packet(code::NOT_MASTER, None, None));
        return Ok(());
    }
    let raw = p.bytes(10).unwrap_or_default();
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    let text = cut(&archcore_net::text::decode(&raw[..end]), MAX_NOTICE);
    ctx.db.characters.exec("UPDATE guild SET notice = ? WHERE id = ?", (text.as_str(), gid)).await?;
    let g = st.guilds.by_id.get_mut(&gid).unwrap();
    g.notice = text;
    let g = &st.guilds.by_id[&gid];
    send_to_guild(st, g, None, &notice_packet(g));
    Ok(())
}

/// Demandes de départ restées sans réponse un jour : le départ se fait.
pub async fn step(ctx: &WorldCtx, st: &mut WorldState, at: Instant) -> anyhow::Result<()> {
    if st.guilds.last_sweep.is_some_and(|t| at.duration_since(t) < SWEEP_EVERY) {
        return Ok(());
    }
    st.guilds.last_sweep = Some(at);
    let limit = now() - LEAVE_DELAY_SECS;
    let due: Vec<(u32, u32)> = st
        .guilds
        .by_id
        .values()
        .flat_map(|g| g.members.values().filter(|m| m.rank == rank::LEAVE_REQUEST && m.leave_requested_at.is_some_and(|t| t <= limit)).map(move |m| (g.id, m.cid)))
        .collect();
    for (gid, cid) in due {
        remove_member(ctx, st, gid, cid, op::LEAVE, code::LEAVE).await?;
    }
    Ok(())
}

/// AgpmEventGuild op 0 : le joueur s'adresse à un gestionnaire de guilde.
pub fn on_event(ctx: &WorldCtx, st: &WorldState, sid: u32, p: &Packet) {
    if p.int(0) != Some(event_op::REQUEST) {
        return;
    }
    let Some(player) = st.players.get(&sid) else { return };
    let base = p.bytes(1).and_then(|b| decode_mini(&mini::EVENT_BASE, b).ok()).unwrap_or_else(|| Fields::new(&mini::EVENT_BASE));
    let Some((npc, _)) = resolve(&ctx.npcs, &base) else { return };
    if !npc.events.iter().any(|e| e.function == event_function::GUILD) || npc_distance(npc, player.ch.pos_x, player.ch.pos_z) > MAX_NPC_DISTANCE {
        warn!("[guilde] #{sid} {} : gestionnaire {} refusé", player.ch.name, npc.name);
        return;
    }
    let s = &schema::EVENT_GUILD;
    player.send(s, Fields::new(s).with(0, event_op::GRANT).with(1, build_mini(&mini::EVENT_BASE, &base)).with(2, player.ch.id));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noms_et_paquets() {
        assert!(valid_name("Sylvania42") && !valid_name("") && !valid_name("Nom avec espace") && !valid_name("Été"));
        assert!(!valid_name(&"a".repeat(33)));
        assert_eq!(cut("abcdef", 4), "abcd");
        let m = Member { cid: 3, name: "GmTest".into(), rank: rank::MASTER, joined_at: 1, level: 20, tid: 96, online: true, leave_requested_at: None };
        let mut g = Guild { id: 1, name: "Sylvania".into(), max_members: 50, ..Default::default() };
        g.members.insert(3, m.clone());
        assert_eq!(g.master().map(|x| x.cid), Some(3));
        assert!(g.member_named("gmtest").is_some() && !g.is_full());
        let decoded = archcore_net::decode_packet(&schema::GUILD, &char_data_packet(&g, &m)).unwrap();
        assert_eq!((decoded.int(0), decoded.int(1), decoded.text(2).as_deref()), (Some(op::CHAR_DATA), Some(3), Some("Sylvania")));
        let who = decode_mini(&mini::GUILD_MEMBER, decoded.bytes(16).unwrap()).unwrap();
        assert_eq!((who.text(0).as_deref(), who.int(1), who.int(5)), (Some("GmTest"), Some(rank::MASTER), Some(1)));
        let sys = archcore_net::decode_packet(&schema::GUILD, &system_packet(code::NEED_MORE_MONEY, None, Some(10_000))).unwrap();
        assert_eq!(sys.list(4).map(|l| l.len()), Some(8));
    }
}
