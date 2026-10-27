//! Combat entre joueurs (AgpmPvP, 0x4A, du serveur au client).
//!
//! Règles d'attaque d'Archonia (`FUN_1402d8e80`, refaites ici puisque le client ne fait que refuser localement) :
//! jamais en zone sûre ; ailleurs, il faut un ennemi déclaré ou une attaque forcée (Ctrl, champ 6 de l'action),
//! et un niveau 20 au moins des deux côtés (message 25) ; pas de coup sur son groupe en zone libre, ni sur un ami
//! (même guilde, guilde alliée) en zone de combat (message 20) ; un meurtrier de 100 points ou plus ne force plus
//! d'attaque ; une cible invincible donne le message 17. Dans les Battlegrounds (régions 110-114, 123-125, 175,
//! 176), seules des races différentes se battent, sans Ctrl.
//!
//! Une attaque forcée fait des deux joueurs des ennemis l'un pour l'autre (op 2, mode de la zone), pour 5 minutes
//! prolongées à chaque coup ; le premier ennemi donne le message 3 (« en combat »), le dernier retiré le 4.
//! Forcer une attaque en zone libre contre un innocent (ni premier attaquant ni meurtrier) fait de l'agresseur un
//! « premier attaquant » pendant 20 minutes (champ 12, nom violet, champ 25 = secondes restantes).
//!
//! À la mort : victoire et défaite comptées (op 0, messages 12 et 5), inimitié levée des deux côtés. Tuer, après
//! l'avoir attaqué en premier, un innocent de plus de 4 niveaux en dessous donne des points de meurtrier
//! (`PvP.MurdererPointsPerKill`, hors Battleground), qui ouvrent un délai de 2 heures avant la rémission
//! (champ 26) ; tuer un joueur de son niveau ou plus rapporte autant de charisme que son niveau (message 24). Les
//! montants sont ceux de la configuration : le serveur d'origine ne les a pas laissés. Paliers de meurtrier (40, 60,
//! 100) : voir [`murderer_level`].
use super::combat::{self, Attacker, Defender, REACH_TOLERANCE};
use super::skills::CharismaSource;
use super::{WorldCtx, WorldState};
use crate::character::unix_now;
use crate::{debug, info};
use archcore_net::schema;
use archcore_net::{build_packet, Fields};
use std::time::{Duration, Instant};

pub mod op {
    pub const INFO: i64 = 0;
    pub const ADD_ENEMY: i64 = 2;
    pub const REMOVE_ENEMY: i64 = 4;
    pub const SYSTEM_MESSAGE: i64 = 18;
}

/// Codes des messages système (op 18, champ 1), affichés par le client avec les textes `PvP_Msg_*`.
pub mod code {
    pub const NOW_PVP: i64 = 3;
    pub const NONE_PVP: i64 = 4;
    pub const DEAD: i64 = 5;
    pub const KILL_PLAYER: i64 = 12;
    pub const TARGET_INVINCIBLE: i64 = 17;
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
/// Durée d'une inimitié, prolongée à chaque coup.
pub const ENEMY_TIME: Duration = Duration::from_secs(300);
/// Durée du statut de premier attaquant et du délai de meurtrier (plafonds du client).
pub const CRIMINAL_SECS: i64 = 1200;
pub const MURDERER_SECS: i64 = 7200;
/// Paliers de points de meurtrier.
pub const MURDERER_LEVELS: [u32; 3] = [40, 60, 100];
/// Écart de niveau au-delà duquel tuer un innocent rend meurtrier.
const MURDER_LEVEL_GAP: i32 = 4;
/// Régions des Battlegrounds, en dur dans le client (`FUN_1402ea830`).
const BATTLEGROUNDS: [u16; 10] = [110, 111, 112, 113, 114, 123, 124, 125, 175, 176];

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
}

#[derive(Default)]
pub struct PvpState {
    pub enemies: Vec<Enemy>,
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
    let Some(p) = st.players.get(&sid) else { return false };
    let pos = p.ch.pos();
    ctx.map.region(pos.x as f64, pos.z as f64).is_some_and(|r| BATTLEGROUNDS.contains(&r))
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
    let now = Instant::now();
    if c.invincible_until.is_some_and(|u| now < u) || super::effects::active_status(&c.skills.effects, now) & super::effects::status::UNTOUCHABLE != 0 {
        return Err(Refusal(Some(code::TARGET_INVINCIBLE)));
    }
    if super::guild::battle::enemies(st, a.ch.id, c.ch.id) {
        return Ok(());
    }
    // Battlegrounds : races différentes seulement.
    if in_battleground(ctx, st, sid) && in_battleground(ctx, st, tsid) {
        return if a.factors.ftype[0] != c.factors.ftype[0] { Ok(()) } else { Err(Refusal(None)) };
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
    // Guildes hostiles : ennemies hors zone sûre.
    if matches!((ga, gc), (Some(x), Some(y)) if super::guild::relation::hostile(st, x, y)) {
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
        for (me, other) in [(sid, tcid), (tsid, acid)] {
            if let Some(e) = st.players.get_mut(&me).and_then(|p| p.pvp.enemies.iter_mut().find(|e| e.cid == other)) {
                e.until = now + ENEMY_TIME;
            }
        }
        return;
    }
    for (me, other, aggressor) in [(sid, tsid, true), (tsid, sid, false)] {
        let other_cid = cid_of(st, other);
        let Some(p) = st.players.get_mut(&me) else { continue };
        p.pvp.enemies.push(Enemy { cid: other_cid, until: now + ENEMY_TIME, aggressor });
        if p.pvp.enemies.len() == 1 {
            p.send_raw(message(code::NOW_PVP, None, None));
        }
        p.send_raw(build_packet(&schema::PVP, &base(op::ADD_ENEMY, p.ch.id).with(2, other_cid).with(5, zone)));
    }
    // Premier attaquant : attaque forcée en zone libre sur un innocent.
    let innocent = st.players.get(&tsid).is_some_and(|t| t.ch.criminal_until <= unix_now() && murderer_level(t.ch.murderer) == 0);
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
    let (Some(k), Some(v)) = (st.players.get(&sid), st.players.get(&tsid)) else { return Ok(()) };
    if super::guild::battle::enemies(st, k.ch.id, v.ch.id) {
        return Ok(());
    }
    let (kname, vname) = (k.ch.name.clone(), v.ch.name.clone());
    let (klevel, vlevel) = (k.factors.status[7], v.factors.status[7]);
    let aggressor = k.pvp.enemy(v.ch.id).is_some_and(|e| e.aggressor);
    let innocent = v.ch.criminal_until <= unix_now() && murderer_level(v.ch.murderer) == 0;
    let battleground = in_battleground(ctx, st, sid);
    part(st, sid, tsid);
    info!("[pvp] {kname} tue {vname}");
    let zone = zone_mode(ctx, st, sid);
    if let Some(k) = st.players.get_mut(&sid) {
        k.ch.pvp_win += 1;
        k.send_raw(info_packet(k.ch.id, k.ch.pvp_win, k.ch.pvp_lose, zone));
        k.send_raw(message(code::KILL_PLAYER, None, Some(&vname)));
        if aggressor && innocent && !battleground && klevel - vlevel > MURDER_LEVEL_GAP {
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
    let attacker = Attacker::of_player(p);
    let defender = attacker.pierce(&Defender::of_player(&t.factors));
    let (outcome, parts) = {
        let mut rng = rand::thread_rng();
        let (outcome, damage) = combat::strike(&mut rng, &attacker, &defender);
        let (outcome, damage) = super::effects::critical(&mut rng, &super::effects::of_player(p), outcome, damage);
        let mut parts = vec![(combat::RESULT_PHYSICAL, damage)];
        if damage > 0 {
            let chance = combat::spirit_chance(true, attacker.level, defender.level);
            parts.extend(combat::spirit_hits(&mut rng, chance, &combat::spirit_ranges(&p.factors), &defender.resist, true));
        }
        (outcome, parts)
    };
    let speed = p.factors.attack[2].max(1) as u64;
    let cid = p.ch.id;
    let me = st.players.get_mut(&sid).unwrap();
    me.attack = Some(combat::Attack { target, ready_at: now + Duration::from_millis(60_000 / speed), forced });
    me.dest = None;
    super::recovery::in_combat(&mut me.timers, now);
    if st.players[&tsid].invincible_until.is_some_and(|u| now < u) {
        return Some(false);
    }
    combat::hit_player(st, tsid, cid, outcome, &parts);
    debug!("[pvp] coup de {cid:#x} sur {target:#x} : résultat {outcome}");
    Some(st.players.get(&tsid).is_some_and(|x| x.dead))
}

/// Fin des inimitiés échues et des statuts de premier attaquant.
pub fn step(ctx: &WorldCtx, st: &mut WorldState, now: Instant) {
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
    fn message_systeme() {
        let plain = message(code::CHARISMA_UP, Some(42), None);
        let p = decode_packet(&schema::PVP, &plain).unwrap();
        assert_eq!((p.int(0), p.int(1), p.int(3)), (Some(op::SYSTEM_MESSAGE), Some(code::CHARISMA_UP), Some(42)));
        let kill = decode_packet(&schema::PVP, &message(code::KILL_PLAYER, None, Some("SimB"))).unwrap();
        assert_eq!(kill.text(8).as_deref(), Some("SimB"));
    }
}
