//! Combat entre joueurs (AgpmPvP, 0x4A, du serveur au client).
//!
//! Règles d'attaque d'Archonia (`FUN_1402d8e80`, refaites ici puisque le client ne fait que refuser localement) :
//! jamais en zone sûre ; ailleurs, il faut un ennemi déclaré ou une attaque forcée (Ctrl, champ 6 de l'action),
//! et un niveau 20 au moins des deux côtés (message 25) ; pas de coup sur son groupe en zone libre, ni sur un ami
//! (même guilde, guilde alliée) en zone de combat (message 20) ; un meurtrier de 100 points ou plus ne force plus
//! d'attaque ; une cible invincible donne le message 17. Dans les Battlegrounds (régions 110-114, 123-125, 175,
//! 176) et dans un Shrine en combat (`shrine.rs`), seules des races différentes se battent, sans Ctrl (race d'origine
//! sous métamorphose). Un joueur sous une forme de créature (Type 8, `morph.rs`) n'attaque personne et n'est attaqué
//! par aucun joueur.
//!
//! Une attaque forcée fait des deux joueurs des ennemis l'un pour l'autre (op 2, mode de la zone), pour 5 minutes
//! prolongées à chaque coup ; le premier ennemi donne le message 3 (« en combat »), le dernier retiré le 4.
//! Forcer une attaque en zone libre contre un innocent (ni premier attaquant ni meurtrier) fait de l'agresseur un
//! « premier attaquant » pendant 20 minutes (champ 12, nom violet, champ 25 = secondes restantes).
//!
//! À la mort : victoire et défaite comptées (op 0, messages 12 et 5), inimitié levée des deux côtés. Tuer, après
//! l'avoir attaqué en premier, un innocent de plus de 4 niveaux en dessous donne des points de meurtrier
//! (`PvP.MurdererPointsPerKill`, hors Battleground, Battle Square et Shrine en combat : `battlesquare.rs`), qui
//! ouvrent un délai de 2 heures avant la rémission (champ 26) ; tuer un joueur de son niveau ou plus rapporte autant de charisme que son niveau (message 24). Les
//! montants sont ceux de la configuration : le serveur d'origine ne les a pas laissés. Paliers de meurtrier (40, 60,
//! 100) : voir [`murderer_level`].
//! Avis de recherche (droit de la victime de mettre son tueur à prix, prime touchée sur un criminel recherché, qui
//! n'est pas innocent) : voir `wanted.rs`.
//!
//! Listes d'ennemis tenues par le client (le serveur d'origine ne les a pas laissées : moments choisis par ArchCore) :
//! ```text
//!   ← op 6 INIT_ENEMY (1)            à l'entrée en jeu, et après la mort du joueur (ses inimitiés tombent : op 4)
//!   ← op 8 UPDATE_ENEMY (1, 2, 5)    ennemi connu qui frappe de nouveau, si le mode de la zone a changé ou toutes les
//!                                    30 s (au lieu d'un nouvel op 2)
//!   ← op 10 ADD_ENEMY_GUILD (1, 8, 5) un membre de la guilde G attaque (attaque forcée, hors zone sûre) un membre de la
//!                                    guilde H : G est envoyée aux membres en ligne de H, qui peuvent alors frapper sans
//!                                    Ctrl tout membre de G hors zone sûre (comme le client, `FUN_1402d8b90`)
//!   ← op 16 UPDATE_ENEMY_GUILD       nouvelle agression de G (même rythme que l'op 8) ; l'inimitié dure `ENEMY_TIME`
//!   ← op 12 REMOVE_ENEMY_GUILD (1, 8) à son expiration
//!   ← op 14 INIT_ENEMY_GUILD (1)     à l'entrée en jeu et à chaque changement de guilde, suivie des guildes ennemies de
//!                                    la nouvelle guilde (op 10)
//!   ← op 18 code 11 (« PvP_Msg_CannotUseTeleport ») : portail refusé à un joueur qui a des ennemis (l'op 17 du client
//!                                    lève un événement d'interface absent de UI_1024X768.ini : rien ne s'afficherait)
//! ```
use super::combat::{self, REACH_TOLERANCE};
use super::skills::CharismaSource;
use super::{WorldCtx, WorldState};
use crate::character::unix_now;
use crate::{debug, info};
use archcore_net::schema;
use archcore_net::{build_packet, Fields};
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub mod op {
    pub const INFO: i64 = 0;
    pub const ADD_ENEMY: i64 = 2;
    pub const REMOVE_ENEMY: i64 = 4;
    pub const INIT_ENEMY: i64 = 6;
    pub const UPDATE_ENEMY: i64 = 8;
    pub const ADD_ENEMY_GUILD: i64 = 10;
    pub const REMOVE_ENEMY_GUILD: i64 = 12;
    pub const INIT_ENEMY_GUILD: i64 = 14;
    pub const UPDATE_ENEMY_GUILD: i64 = 16;
    pub const SYSTEM_MESSAGE: i64 = 18;
}

/// Codes des messages système (op 18, champ 1), affichés par le client avec les textes `PvP_Msg_*`.
pub mod code {
    /// Entrée dans une zone de combat (Battle Square) : « PvP_Msg_EnterCombatArea ».
    pub const ENTER_COMBAT_AREA: i64 = 2;
    pub const NOW_PVP: i64 = 3;
    pub const NONE_PVP: i64 = 4;
    pub const DEAD: i64 = 5;
    /// Portail refusé à un joueur en combat entre joueurs : « PvP_Msg_CannotUseTeleport ».
    pub const CANNOT_USE_TELEPORT: i64 = 11;
    pub const KILL_PLAYER: i64 = 12;
    /// « You moved to a Battle Square. »
    pub const MOVE_BATTLE_SQUARE: i64 = 13;
    pub const TARGET_INVINCIBLE: i64 = 17;
    /// Niveau insuffisant pour la Battle Square (le client affiche son propre seuil).
    pub const NOT_ENOUGH_LEVEL: i64 = 18;
    pub const CANNOT_ATTACK_FRIEND: i64 = 20;
    pub const CHARISMA_UP: i64 = 24;
    pub const CANNOT_PVP_BY_LEVEL: i64 = 25;
}

/// Mode PvP d'une zone (sûreté de RegionTemplate.ini + 1).
pub mod mode {
    pub const SAFE: i64 = 1;
    pub const FREE: i64 = 2;
    pub const COMBAT: i64 = 3;
}

/// Niveau minimal des deux joueurs (valeur du client).
pub const MIN_LEVEL: i32 = 20;
/// Durée d'une inimitié, prolongée à chaque coup (aussi celle d'une guilde ennemie).
pub const ENEMY_TIME: Duration = Duration::from_secs(300);
/// Un ennemi (ou une guilde ennemie) connu qui frappe de nouveau est rafraîchi chez le client (op 8, op 16) au plus
/// tous les tant, ou dès que le mode de la zone change.
pub const ENEMY_REFRESH: Duration = Duration::from_secs(30);
/// Durée du statut de premier attaquant et du délai de meurtrier (plafonds du client).
pub const CRIMINAL_SECS: i64 = 1200;
pub const MURDERER_SECS: i64 = 7200;
/// Paliers de points de meurtrier.
pub const MURDERER_LEVELS: [u32; 3] = [40, 60, 100];
/// Écart de niveau au-delà duquel tuer un innocent rend meurtrier.
const MURDER_LEVEL_GAP: i32 = 4;

/// Palier de meurtrier : 0 sous 40 points, puis 1, 2 et 3.
pub fn murderer_level(points: u32) -> usize {
    MURDERER_LEVELS.iter().filter(|l| points >= **l).count()
}

/// Un ennemi déclaré.
#[derive(Clone, Debug)]
pub struct Enemy {
    pub cid: u32,
    pub until: Instant,
    /// Ce joueur a attaqué l'autre le premier.
    pub aggressor: bool,
    /// Mode de la zone et instant annoncés au client (op 2, puis op 8).
    pub mode: i64,
    pub shown_at: Instant,
}

#[derive(Default)]
pub struct PvpState {
    pub enemies: Vec<Enemy>,
    /// Mort : inimitiés levées et liste vidée (op 6) au prochain tour, après le bilan du meurtre (`on_kill`).
    pub reset_pending: bool,
}

/// Guilde ennemie : jusqu'à quand, mode de la zone de la dernière agression, instant annoncé.
#[derive(Clone, Copy, Debug)]
pub struct GuildEnemy {
    pub until: Instant,
    pub mode: i64,
    pub shown_at: Instant,
}

/// Guildes ennemies : (guilde qui tient la liste, guilde ennemie, qui a attaqué l'un de ses membres).
#[derive(Default)]
pub struct GuildEnemies {
    pub list: HashMap<(u32, u32), GuildEnemy>,
}

impl GuildEnemies {
    /// La guilde `owner` tient `enemy` pour ennemie.
    pub fn holds(&self, owner: u32, enemy: u32) -> bool {
        self.list.contains_key(&(owner, enemy))
    }

    /// Agression d'un membre de `enemy` sur un membre de `owner` en `mode` : op 10 (nouvelle), op 16 (rafraîchie : mode
    /// changé ou plus de `ENEMY_REFRESH`), ou rien.
    pub fn aggression(&mut self, owner: u32, enemy: u32, mode: i64, now: Instant) -> Option<i64> {
        let until = now + ENEMY_TIME;
        match self.list.get_mut(&(owner, enemy)) {
            None => {
                self.list.insert((owner, enemy), GuildEnemy { until, mode, shown_at: now });
                Some(op::ADD_ENEMY_GUILD)
            }
            Some(e) => {
                e.until = until;
                if e.mode == mode && now.duration_since(e.shown_at) < ENEMY_REFRESH {
                    return None;
                }
                (e.mode, e.shown_at) = (mode, now);
                Some(op::UPDATE_ENEMY_GUILD)
            }
        }
    }

    /// Retire et rend les inimitiés échues.
    pub fn expired(&mut self, now: Instant) -> Vec<(u32, u32)> {
        let gone: Vec<(u32, u32)> = self.list.iter().filter(|(_, e)| now >= e.until).map(|(k, _)| *k).collect();
        for k in &gone {
            self.list.remove(k);
        }
        gone
    }
}

impl PvpState {
    pub fn enemy(&self, cid: u32) -> Option<&Enemy> {
        self.enemies.iter().find(|e| e.cid == cid)
    }
}

fn base(op: i64, cid: u32) -> Fields {
    Fields::new(&schema::PVP).with(0, op).with(1, cid)
}

/// Op 18 : message système `code`, avec un entier et une chaîne.
pub fn message(code: i64, param: Option<i64>, text: Option<&str>) -> Vec<u8> {
    let f = Fields::new(&schema::PVP).with(0, op::SYSTEM_MESSAGE).with(1, code).with_opt(3, param);
    build_packet(&schema::PVP, &f.with_opt(8, text))
}

/// Ops 10, 12 et 16 : guilde ennemie `guild` ajoutée, retirée ou rafraîchie chez `cid` ; op 14 (sans guilde) : liste
/// vidée.
pub fn enemy_guild_packet(o: i64, cid: u32, guild: Option<&str>, zone: Option<i64>) -> Vec<u8> {
    build_packet(&schema::PVP, &base(o, cid).with_opt(8, guild).with_opt(5, zone))
}

/// Op 14 puis op 10 pour chaque guilde ennemie de la guilde du joueur (entrée en jeu, changement de guilde).
pub fn send_enemy_guilds(st: &WorldState, sid: u32) {
    let Some(p) = st.players.get(&sid) else { return };
    let cid = p.ch.id;
    p.send_raw(enemy_guild_packet(op::INIT_ENEMY_GUILD, cid, None, None));
    let Some(gid) = st.guilds.of_cid.get(&cid).copied() else { return };
    for ((owner, enemy), e) in &st.pvp_guilds.list {
        if *owner == gid {
            if let Some(g) = st.guilds.by_id.get(enemy) {
                p.send_raw(enemy_guild_packet(op::ADD_ENEMY_GUILD, cid, Some(&g.name), Some(e.mode)));
            }
        }
    }
}

/// Changement de guilde du personnage `cid` (adhésion, départ, exclusion, dissolution) : listes renvoyées s'il est en jeu.
pub fn on_guild_change(st: &WorldState, cid: u32) {
    if let Some(sid) = st.by_cid.get(&cid).copied() {
        send_enemy_guilds(st, sid);
    }
}

/// Entrée en jeu : liste d'ennemis vide (op 6), guildes ennemies (op 14, op 10).
pub fn on_enter(st: &WorldState, sid: u32) {
    let Some(p) = st.players.get(&sid) else { return };
    p.send_raw(build_packet(&schema::PVP, &base(op::INIT_ENEMY, p.ch.id)));
    send_enemy_guilds(st, sid);
}

/// Envoie un paquet de guilde ennemie (ops 10, 12, 16) à chaque membre en ligne de la guilde `owner`.
fn to_guild(st: &WorldState, owner: u32, o: i64, enemy_name: &str, zone: Option<i64>) {
    let Some(g) = st.guilds.by_id.get(&owner) else { return };
    for m in g.members.values().filter(|m| m.active()) {
        if let Some(p) = st.by_cid.get(&m.cid).and_then(|s| st.players.get(s)).filter(|p| p.in_game) {
            p.send_raw(enemy_guild_packet(o, p.ch.id, Some(enemy_name), zone));
        }
    }
}

/// Coup de `sid` sur `tsid` hors zone sûre : la guilde de l'agresseur devient ennemie de celle de la victime (guildes
/// différentes, non alliées).
fn guild_aggression(st: &mut WorldState, sid: u32, tsid: u32, zone: i64, now: Instant) {
    if zone == mode::SAFE {
        return;
    }
    let (Some(g), Some(h)) = (st.guilds.of_cid.get(&cid_of(st, sid)).copied(), st.guilds.of_cid.get(&cid_of(st, tsid)).copied()) else { return };
    if g == h || super::guild::relation::allied(st, g, h) {
        return;
    }
    let Some(o) = st.pvp_guilds.aggression(h, g, zone, now) else { return };
    let name = st.guilds.by_id.get(&g).map(|x| x.name.clone()).unwrap_or_default();
    if o == op::ADD_ENEMY_GUILD {
        info!("[pvp] la guilde {name} devient ennemie de {}", st.guilds.by_id.get(&h).map_or("?", |x| x.name.as_str()));
    }
    to_guild(st, h, o, &name, Some(zone));
}

/// Op 0 : victoires, défaites et mode de la zone du joueur.
pub fn info_packet(cid: u32, win: u32, lose: u32, zone: i64) -> Vec<u8> {
    build_packet(&schema::PVP, &base(op::INFO, cid).with(3, win).with(4, lose).with(5, zone).with(6, 0))
}

/// Mode PvP de la position d'un joueur.
pub fn zone_mode(ctx: &WorldCtx, st: &WorldState, sid: u32) -> i64 {
    let Some(p) = st.players.get(&sid) else { return mode::SAFE };
    let pos = p.ch.pos();
    ctx.map.region(pos.x as f64, pos.z as f64).map(|r| ctx.regions.safety(r) + 1).unwrap_or(mode::SAFE)
}

fn in_battleground(ctx: &WorldCtx, st: &WorldState, sid: u32) -> bool {
    super::battlesquare::in_battleground(ctx, st, sid)
}

/// Refus d'une attaque, avec le message à montrer à l'agresseur s'il y en a un.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refusal(pub Option<i64>);

/// L'attaque de `sid` sur le joueur `tsid` est-elle permise ? `forced` : attaque forcée (Ctrl).
pub fn check(ctx: &WorldCtx, st: &WorldState, sid: u32, tsid: u32, forced: bool) -> Result<(), Refusal> {
    let (Some(a), Some(c)) = (st.players.get(&sid), st.players.get(&tsid)) else { return Err(Refusal(None)) };
    if sid == tsid || !c.in_game || c.dead || !a.in_game || a.dead {
        return Err(Refusal(None));
    }
    // Forme d'animal (Type 8 de CharacterDataTable, morph.rs) : pas d'attaque d'un joueur, ni sur un joueur.
    if super::morph::harmless(st, sid) || super::morph::harmless(st, tsid) {
        return Err(Refusal(None));
    }
    let now = Instant::now();
    if c.invincible_until.is_some_and(|u| now < u) || super::effects::active_status(&c.skills.effects, now) & super::effects::status::UNTOUCHABLE != 0 {
        return Err(Refusal(Some(code::TARGET_INVINCIBLE)));
    }
    if super::guild::battle::enemies(st, a.ch.id, c.ch.id) {
        return Ok(());
    }
    // Battlegrounds : races différentes seulement (race d'origine sous métamorphose : facteurs, morph.rs).
    if in_battleground(ctx, st, sid) && in_battleground(ctx, st, tsid) {
        return if a.factors.ftype[0] != c.factors.ftype[0] { Ok(()) } else { Err(Refusal(None)) };
    }
    // Shrine en combat (shrine.rs) : de même, entre joueurs du même Shrine.
    if let (Some(x), Some(y)) = (super::shrine::active_shrine(ctx, st, sid), super::shrine::active_shrine(ctx, st, tsid)) {
        return if x == y && a.factors.ftype[0] != c.factors.ftype[0] { Ok(()) } else { Err(Refusal(None)) };
    }
    // Siège en cours (siege/war.rs) : guildes de camps opposés sans Ctrl, jamais dans un même camp ; les autres joueurs
    // suivent les règles ordinaires.
    if let Some(ok) = super::siege::war::pvp_rule(ctx, st, sid, tsid) {
        return if ok { Ok(()) } else { Err(Refusal(Some(code::CANNOT_ATTACK_FRIEND))) };
    }
    let (ma, mc) = (zone_mode(ctx, st, sid), zone_mode(ctx, st, tsid));
    let enemy = a.pvp.enemy(c.ch.id).is_some();
    if enemy && mc != mode::SAFE {
        return Ok(());
    }
    if a.factors.status[7] < MIN_LEVEL || c.factors.status[7] < MIN_LEVEL {
        return Err(Refusal(Some(code::CANNOT_PVP_BY_LEVEL)));
    }
    if ma == mode::SAFE || mc == mode::SAFE {
        return Err(Refusal(None));
    }
    if ma == mode::FREE && st.parties.party_of(a.ch.id).is_some_and(|p| p.members.contains(&c.ch.id)) {
        return Err(Refusal(None));
    }
    let (ga, gc) = (st.guilds.of_cid.get(&a.ch.id).copied(), st.guilds.of_cid.get(&c.ch.id).copied());
    let friend = matches!((ga, gc), (Some(x), Some(y)) if x == y || super::guild::relation::allied(st, x, y));
    if ma == mode::COMBAT && friend {
        return Err(Refusal(Some(code::CANNOT_ATTACK_FRIEND)));
    }
    // Guildes hostiles : ennemies hors zone sûre ; de même la guilde qui a attaqué un membre de celle de l'agresseur
    // (guilde ennemie, op 10).
    if matches!((ga, gc), (Some(x), Some(y)) if super::guild::relation::hostile(st, x, y) || st.pvp_guilds.holds(x, y)) {
        return Ok(());
    }
    if !forced {
        return Err(Refusal(None));
    }
    if murderer_level(a.ch.murderer) >= 3 {
        return Err(Refusal(None));
    }
    Ok(())
}

/// Montre un refus à l'agresseur.
pub fn refuse(st: &WorldState, sid: u32, r: Refusal) {
    if let (Some(code), Some(p)) = (r.0, st.players.get(&sid)) {
        let param = (code == code::CANNOT_PVP_BY_LEVEL).then_some(MIN_LEVEL as i64);
        p.send_raw(message(code, param, None));
    }
}

/// Un coup a été porté par `sid` sur `tsid` : inimitié créée (attaque forcée) ou prolongée, statut de premier
/// attaquant.
pub fn on_hit(ctx: &WorldCtx, st: &mut WorldState, sid: u32, tsid: u32, now: Instant) {
    if super::guild::battle::enemies(st, cid_of(st, sid), cid_of(st, tsid)) {
        return;
    }
    let zone = zone_mode(ctx, st, sid);
    let (acid, tcid) = (cid_of(st, sid), cid_of(st, tsid));
    let known = st.players.get(&sid).is_some_and(|p| p.pvp.enemy(tcid).is_some());
    if known {
        guild_aggression(st, sid, tsid, zone, now);
        for (me, other) in [(sid, tcid), (tsid, acid)] {
            let Some(p) = st.players.get_mut(&me) else { continue };
            let my_cid = p.ch.id;
            let Some(e) = p.pvp.enemies.iter_mut().find(|e| e.cid == other) else { continue };
            e.until = now + ENEMY_TIME;
            // Op 8 : mode et instant rafraîchis chez le client.
            if e.mode != zone || now.duration_since(e.shown_at) >= ENEMY_REFRESH {
                (e.mode, e.shown_at) = (zone, now);
                p.send_raw(build_packet(&schema::PVP, &base(op::UPDATE_ENEMY, my_cid).with(2, other).with(5, zone)));
            }
        }
        return;
    }
    for (me, other, aggressor) in [(sid, tsid, true), (tsid, sid, false)] {
        let other_cid = cid_of(st, other);
        let Some(p) = st.players.get_mut(&me) else { continue };
        p.pvp.enemies.push(Enemy { cid: other_cid, until: now + ENEMY_TIME, aggressor, mode: zone, shown_at: now });
        if p.pvp.enemies.len() == 1 {
            p.send_raw(message(code::NOW_PVP, None, None));
        }
        p.send_raw(build_packet(&schema::PVP, &base(op::ADD_ENEMY, p.ch.id).with(2, other_cid).with(5, zone)));
    }
    // Guilde de l'agresseur ennemie de celle de la victime (op 10), après l'inimitié individuelle (op 2).
    guild_aggression(st, sid, tsid, zone, now);
    // Premier attaquant : attaque forcée en zone libre sur un innocent (un criminel recherché ne l'est pas) ; jamais dans
    // les zones de combat organisé (Battleground, Battle Square, Shrine).
    if super::battlesquare::free_kill_zone(ctx, st, sid) {
        return;
    }
    let innocent = st.players.get(&tsid).is_some_and(|t| t.ch.criminal_until <= unix_now() && murderer_level(t.ch.murderer) == 0 && !st.wanted.is_wanted(t.ch.id));
    if zone == mode::FREE && innocent {
        set_criminal(ctx, st, sid);
    }
}

fn cid_of(st: &WorldState, sid: u32) -> u32 {
    st.players.get(&sid).map(|p| p.ch.id).unwrap_or(0)
}

/// Statut de premier attaquant pour 20 minutes : enregistré et montré à tous ceux qui voient le joueur.
fn set_criminal(ctx: &WorldCtx, st: &mut WorldState, sid: u32) {
    let Some(p) = st.players.get_mut(&sid) else { return };
    p.ch.criminal_until = unix_now() + CRIMINAL_SECS;
    info!("[pvp] {} devient premier attaquant", p.ch.name);
    save_status(ctx, p.ch.id, &p.ch);
    send_status(st, sid);
}

/// Champs 12, 25 et 26 du paquet personnage (op 1) au joueur et à ceux qui le voient.
fn send_status(st: &WorldState, sid: u32) {
    let Some(p) = st.players.get(&sid) else { return };
    let v = p.view();
    let c = &schema::CHARACTER;
    let plain = build_packet(c, &Fields::new(c).with(0, 1).with(1, p.ch.id).with(12, v.criminal).with(25, v.criminal_left).with(26, v.murderer_left));
    combat::send_around(st, sid, &plain);
}

fn save_status(ctx: &WorldCtx, cid: u32, ch: &crate::character::CharacterRow) {
    let db = ctx.db.characters.clone();
    let (win, lose, murderer, criminal, until) = (ch.pvp_win, ch.pvp_lose, ch.murderer, ch.criminal_until, ch.murderer_until);
    tokio::spawn(async move {
        let sql = "UPDATE characters SET pvp_win = ?, pvp_lose = ?, murderer_points = ?, criminal_until = ?, murderer_until = ? WHERE id = ?";
        if let Err(e) = db.exec(sql, (win, lose, murderer, criminal, until, cid)).await {
            crate::warn!("[pvp] enregistrement du personnage {cid} : {e:#}");
        }
    });
}

/// Retire l'inimitié entre deux joueurs, des deux côtés (op 4, message 4 quand il n'en reste plus).
fn part(st: &mut WorldState, a: u32, b: u32) {
    for (me, other) in [(a, b), (b, a)] {
        let other_cid = cid_of(st, other);
        let Some(p) = st.players.get_mut(&me) else { continue };
        let before = p.pvp.enemies.len();
        p.pvp.enemies.retain(|e| e.cid != other_cid);
        if p.pvp.enemies.len() == before {
            continue;
        }
        p.send_raw(build_packet(&schema::PVP, &base(op::REMOVE_ENEMY, p.ch.id).with(2, other_cid)));
        if p.pvp.enemies.is_empty() {
            p.send_raw(message(code::NONE_PVP, None, None));
        }
    }
}

/// Le joueur `sid` a tué le joueur `tsid`.
pub async fn on_kill(ctx: &WorldCtx, st: &mut WorldState, sid: u32, tsid: u32) -> anyhow::Result<()> {
    // Mort en combat entre joueurs : pas de perte d'expérience au village (consumables.rs).
    if let Some(v) = st.players.get_mut(&tsid) {
        v.uses.pk_death = true;
    }
    let (Some(k), Some(v)) = (st.players.get(&sid), st.players.get(&tsid)) else { return Ok(()) };
    if super::guild::battle::enemies(st, k.ch.id, v.ch.id) {
        return Ok(());
    }
    let (kname, vname) = (k.ch.name.clone(), v.ch.name.clone());
    let (klevel, vlevel) = (k.factors.status[7], v.factors.status[7]);
    let aggressor = k.pvp.enemy(v.ch.id).is_some_and(|e| e.aggressor);
    // Un criminel recherché n'est pas innocent (wanted.rs).
    let innocent = v.ch.criminal_until <= unix_now() && murderer_level(v.ch.murderer) == 0 && !st.wanted.is_wanted(v.ch.id);
    // Battleground, Battle Square, Shrine en combat : ni points de meurtrier ni mise à prix.
    let battleground = super::battlesquare::free_kill_zone(ctx, st, sid) || super::battlesquare::free_kill_zone(ctx, st, tsid);
    // Droit de la victime de mettre son tueur à prix : frappée la première en étant innocente, ou tuée par un meurtrier.
    let guilty = !battleground && ((aggressor && innocent) || murderer_level(k.ch.murderer) >= 1);
    part(st, sid, tsid);
    info!("[pvp] {kname} tue {vname}");
    // Objectifs de titre Killer (joueurs tués).
    super::titles::on_pk(ctx, st, sid).await?;
    // Prime touchée sur un criminel recherché : ce meurtre ne donne pas de points de meurtrier.
    let bounty = super::wanted::on_kill(ctx, st, sid, tsid, guilty).await?;
    let zone = zone_mode(ctx, st, sid);
    if let Some(k) = st.players.get_mut(&sid) {
        k.ch.pvp_win += 1;
        k.send_raw(info_packet(k.ch.id, k.ch.pvp_win, k.ch.pvp_lose, zone));
        k.send_raw(message(code::KILL_PLAYER, None, Some(&vname)));
        if aggressor && innocent && !bounty && !battleground && klevel - vlevel > MURDER_LEVEL_GAP {
            let before = murderer_level(k.ch.murderer);
            k.ch.murderer = (k.ch.murderer + ctx.config.world.pvp_murderer_points).min(99_999);
            k.ch.murderer_until = unix_now() + MURDERER_SECS;
            k.factors.status[11] = k.ch.murderer as i32;
            info!("[pvp] {kname} : {} point(s) de meurtrier (palier {before} → {})", k.ch.murderer, murderer_level(k.ch.murderer));
            super::skills::send_points(k);
        }
        save_status(ctx, k.ch.id, &k.ch);
    }
    send_status(st, sid);
    if let Some(v) = st.players.get_mut(&tsid) {
        v.ch.pvp_lose += 1;
        v.send_raw(info_packet(v.ch.id, v.ch.pvp_win, v.ch.pvp_lose, zone));
        v.send_raw(message(code::DEAD, None, Some(&kname)));
        save_status(ctx, v.ch.id, &v.ch);
    }
    if murderer_level(st.players.get(&sid).map_or(0, |k| k.ch.murderer)) >= 1 {
        super::party::expel_murderer(st, sid);
    }
    // Battle Square : crâne lâché par la victime.
    super::battlesquare::on_pvp_kill(ctx, st, sid, tsid, klevel, vlevel);
    if vlevel >= klevel {
        let gain = super::skills::gain_charisma(ctx, st, sid, vlevel.max(0) as u32, CharismaSource::Pvp).await?;
        if let Some(k) = st.players.get(&sid).filter(|_| gain > 0) {
            k.send_raw(message(code::CHARISMA_UP, Some(gain as i64), None));
        }
    }
    Ok(())
}

/// Coup normal d'un joueur sur un autre (hors combat de guildes) : règles, portée, frappe, inimitié, mort.
pub async fn attack_step(ctx: &WorldCtx, st: &mut WorldState, sid: u32, target: u32, forced: bool, now: Instant) -> anyhow::Result<()> {
    let Some(tsid) = st.by_cid.get(&target).copied() else {
        st.players.get_mut(&sid).unwrap().attack = None;
        return Ok(());
    };
    if super::guild::battle::enemies(st, cid_of(st, sid), target) {
        return super::guild::battle::attack_step(ctx, st, sid, target, now).await;
    }
    if let Err(r) = check(ctx, st, sid, tsid, forced) {
        st.players.get_mut(&sid).unwrap().attack = None;
        refuse(st, sid, r);
        return Ok(());
    }
    let Some(killed) = strike(st, sid, tsid, target, forced, now) else { return Ok(()) };
    on_hit(ctx, st, sid, tsid, now);
    // Enchaînement 3 : l'effet additionnel de la cible sur son assaillant (skill_extra.rs).
    if !killed {
        let me = cid_of(st, sid);
        super::skill_extra::on_bearer_hit(ctx, st, tsid, me, now);
    }
    if killed {
        st.players.get_mut(&sid).unwrap().attack = None;
        on_kill(ctx, st, sid, tsid).await?;
    }
    Ok(())
}

/// Frappe de `sid` sur le joueur `tsid` s'il est à portée : `None` hors de portée, sinon s'il en meurt.
pub fn strike(st: &mut WorldState, sid: u32, tsid: u32, target: u32, forced: bool, now: Instant) -> Option<bool> {
    let (p, t) = (&st.players[&sid], &st.players[&tsid]);
    let reach = p.factors.attack[0].max(50) as f64 + 50.0;
    if combat::dist2d(p.ch.pos(), t.ch.pos()) > reach + REACH_TOLERANCE {
        return None;
    }
    // Toucher, esquive et blocage de la cible, critique, part élémentaire (combat.rs).
    let (outcome, parts) = combat::versus(&mut rand::thread_rng(), p, t);
    let speed = p.factors.attack[2].max(1) as u64;
    let cid = p.ch.id;
    let me = st.players.get_mut(&sid).unwrap();
    me.attack = Some(combat::Attack { target, ready_at: now + Duration::from_millis(60_000 / speed), forced });
    me.hits.hit(now, speed as i32);
    me.dest = None;
    super::recovery::in_combat(&mut me.timers, now);
    if st.players[&tsid].invincible_until.is_some_and(|u| now < u) {
        return Some(false);
    }
    combat::hit_player(st, tsid, cid, outcome, &parts);
    debug!("[pvp] coup de {cid:#x} sur {target:#x} : résultat {outcome}");
    Some(st.players.get(&tsid).is_some_and(|x| x.dead))
}

/// Fin des inimitiés échues et des statuts de premier attaquant ; listes des joueurs morts vidées ; guildes ennemies
/// échues.
pub fn step(ctx: &WorldCtx, st: &mut WorldState, now: Instant) {
    // Mort : chaque inimitié levée des deux côtés (op 4), puis la liste vidée (op 6).
    let dead: Vec<u32> = st.players.iter().filter(|(_, p)| p.pvp.reset_pending).map(|(sid, _)| *sid).collect();
    for sid in dead {
        let others: Vec<u32> = st.players[&sid].pvp.enemies.iter().filter_map(|e| st.by_cid.get(&e.cid).copied()).collect();
        for o in others {
            part(st, sid, o);
        }
        let p = st.players.get_mut(&sid).unwrap();
        p.pvp.reset_pending = false;
        p.pvp.enemies.clear();
        p.send_raw(build_packet(&schema::PVP, &base(op::INIT_ENEMY, p.ch.id)));
    }
    for (owner, enemy) in st.pvp_guilds.expired(now) {
        let name = st.guilds.by_id.get(&enemy).map(|g| g.name.clone()).unwrap_or_default();
        debug!("[pvp] guilde {name} : plus ennemie de la guilde {owner}");
        to_guild(st, owner, op::REMOVE_ENEMY_GUILD, &name, None);
    }
    let mut ended = Vec::new();
    let mut cleared = Vec::new();
    let unix = unix_now();
    for (sid, p) in &st.players {
        for e in p.pvp.enemies.iter().filter(|e| now >= e.until) {
            if let Some(o) = st.by_cid.get(&e.cid) {
                ended.push((*sid, *o));
            }
        }
        if p.ch.criminal_until != 0 && p.ch.criminal_until <= unix {
            cleared.push(*sid);
        }
    }
    for (a, b) in ended {
        part(st, a, b);
    }
    for sid in cleared {
        if let Some(p) = st.players.get_mut(&sid) {
            p.ch.criminal_until = 0;
            info!("[pvp] {} n'est plus premier attaquant", p.ch.name);
            save_status(ctx, p.ch.id, &p.ch);
        }
        send_status(st, sid);
    }
    // Inimitiés envers un joueur parti : retirées.
    let gone: Vec<(u32, u32)> = st
        .players
        .iter()
        .flat_map(|(sid, p)| p.pvp.enemies.iter().filter(|e| !st.by_cid.contains_key(&e.cid)).map(move |e| (*sid, e.cid)))
        .collect();
    for (sid, cid) in gone {
        if let Some(p) = st.players.get_mut(&sid) {
            p.pvp.enemies.retain(|e| e.cid != cid);
            p.send_raw(build_packet(&schema::PVP, &base(op::REMOVE_ENEMY, p.ch.id).with(2, cid)));
            if p.pvp.enemies.is_empty() {
                p.send_raw(message(code::NONE_PVP, None, None));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archcore_net::decode_packet;

    #[test]
    fn paliers_de_meurtrier() {
        assert_eq!([0, 39, 40, 59, 60, 99, 100, 5000].map(murderer_level), [0, 0, 1, 1, 2, 2, 3, 3]);
    }

    #[test]
    fn guildes_ennemies() {
        let now = Instant::now();
        let mut g = GuildEnemies::default();
        assert_eq!(g.aggression(1, 2, mode::FREE, now), Some(op::ADD_ENEMY_GUILD));
        assert!(g.holds(1, 2) && !g.holds(2, 1), "seule la guilde attaquée la tient pour ennemie");
        assert_eq!(g.aggression(1, 2, mode::FREE, now + Duration::from_secs(5)), None, "rafraîchie trop tôt");
        assert_eq!(g.aggression(1, 2, mode::COMBAT, now + Duration::from_secs(6)), Some(op::UPDATE_ENEMY_GUILD), "mode changé");
        assert_eq!(g.aggression(1, 2, mode::COMBAT, now + Duration::from_secs(40)), Some(op::UPDATE_ENEMY_GUILD));
        assert!(g.expired(now + Duration::from_secs(300)).is_empty(), "prolongée à chaque agression");
        assert_eq!(g.expired(now + Duration::from_secs(341)), vec![(1, 2)]);
        assert!(!g.holds(1, 2));
        let p = decode_packet(&schema::PVP, &enemy_guild_packet(op::ADD_ENEMY_GUILD, 7, Some("SimLead"), Some(mode::FREE))).unwrap();
        assert_eq!((p.int(0), p.int(1), p.text(8).as_deref(), p.int(5)), (Some(10), Some(7), Some("SimLead"), Some(2)));
        let p = decode_packet(&schema::PVP, &enemy_guild_packet(op::INIT_ENEMY_GUILD, 7, None, None)).unwrap();
        assert_eq!((p.int(0), p.int(1), p.text(8)), (Some(14), Some(7), None));
    }

    #[test]
    fn message_systeme() {
        let plain = message(code::CHARISMA_UP, Some(42), None);
        let p = decode_packet(&schema::PVP, &plain).unwrap();
        assert_eq!((p.int(0), p.int(1), p.int(3)), (Some(op::SYSTEM_MESSAGE), Some(code::CHARISMA_UP), Some(42)));
        let kill = decode_packet(&schema::PVP, &message(code::KILL_PLAYER, None, Some("SimB"))).unwrap();
        assert_eq!(kill.text(8).as_deref(), Some("SimB"));
    }
}
