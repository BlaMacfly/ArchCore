//! Titre d'ArchLord (AgpmArchlord 0x55) : guerre de Rensphere, règne, gardes royaux et compétences de l'ArchLord.
//!
//! ```text
//!   entrée en jeu                  ← op 0 {nom} s'il y a un ArchLord ; op 5 {étape} pendant la guerre de Rensphere ;
//!                                    l'ArchLord reçoit ses compétences (AGPMSKILL op 0)
//!   nouvel ArchLord                ← op 1 {ancien} puis op 0 {nouveau} à tous (sysstr 631, 630)
//!   guerre de Rensphere            ← op 5 à tous : 1 portail ouvert (début), 2 combat (`Archlord.PortalMinutes` plus tard),
//!                                    3 bataille de l'ArchLord (sceau gravé), 4 fin
//!   /guardactivate                 ← op 2 {nom du garde, vide : refusé}, op 4 {gardes, au plus} ; pas l'ArchLord : op 6 {1}
//!   /guarddeactivate               ← op 3 {nom du garde, vide : aucun}, op 4 ; pas l'ArchLord : op 6 {2}
//!   /guardinformation              ← op 4
//!   /archlord <message>            message de l'ArchLord à tout le serveur (infobulle de Lord Howling)
//!   C op 7                         journalisée : aucun objet du client ne porte d'événement de fonction 36
//! ```
//! Règles du client appliquées : l'ArchLord est connu par son nom (`IsArchlord` compare les noms) ; Rensphere (château 0
//! de SiegeWar.txt) est son château, assiégé tous les Duration4nextSW jours (30) à SWBeginTime (20 h) ; les compétences de
//! `skill_type` 9 (liste « Archlord » de SKILLMASTERY.txt) sont les siennes.
//!
//! Choix d'ArchCore (le serveur d'origine n'est pas connu), réglables :
//! * guerre de Rensphere ouverte par le calendrier (`Siege.Enabled`) ou `/siege start Rensphere` (`/archlordbegin`) ; ses
//!   assaillants sont les guildes propriétaires d'un autre château (le portail de `SpecialType` 4 leur est ouvert), ses
//!   défenseurs la guilde qui tient Rensphere ; le carving est permis dès le début. Le sceau gravé, la guerre passe au
//!   statut 7 (« bataille de l'ArchLord », sans résurrection à Rensphere) pour la moitié de SWPlayingTime au plus, et le
//!   graveur reçoit la compétence de sceau de SiegeWar.txt (930 « Stamper Buff »). À la fin, le chef de la guilde qui a
//!   gravé le sceau devient ArchLord jusqu'à la guerre suivante ; sans sceau, l'ArchLord en place le reste ;
//! * ArchLord enregistré (`archlord`), désignable par un MJ (`/archlord set <nom>`, `/archlord clear`) ; ses compétences
//!   lui sont données en mémoire à l'entrée en jeu (pas en base) ; tout autre joueur qui en lance une est refusé
//!   (`skills.rs`) ;
//! * gardes royaux : invocations de combat forcées (`summons.rs`) du modèle de Call Royal Knight (475 : TID 1277,
//!   600 s), `Archlord.MaxGuards` au plus (10), sans objet d'Archon exigé ; Call Royal Knight en appelle autant que sa
//!   compétence le dit, dans la même limite ;
//! * Call Dragon (474, `ride`) : monture Alcadia (1295) par AgpmRide op 6, sans objet ; relancée, elle descend (op 4) ;
//! * Rain Control (522) et Snow Control (523) : pluie ou neige pour tous pendant leur durée (5 min), puis la météo
//!   d'avant ; Day and Night (478) : horloge envoyée à tous décalée de 12 h pendant `Archlord.SkyMinutes` (`nature.rs`) ;
//! * les autres (472, 473, 476, 711 à 719…) passent par le moteur de compétences ordinaire.

use super::siege::{self, status, Application, Castle, Side, RENSPHERE};
use super::skills::{self, Owned, SkillLevel, SkillTemplate};
use super::{combat, nature, ride, send_system, summons, WorldCtx, WorldState};
use crate::character::unix_now;
use crate::db::{col, Databases};
use crate::{info, warn};
use archcore_net::schema::{self, archlord_msg, archlord_op, archlord_step, chat_type, weather};
use archcore_net::{build_packet, Fields, Packet};
use chrono::{NaiveDate, NaiveDateTime};
use std::time::{Duration, Instant};

/// Call Royal Knight : modèle et durée des gardes royaux.
pub const CALL_ROYAL_KNIGHT: u32 = 475;
/// Royal Knight et durée, si la compétence 475 manque.
const ROYAL_KNIGHT: u32 = 1277;
const GUARD_DURATION: Duration = Duration::from_secs(600);
/// Alcadia, monture de Call Dragon si la compétence ne donne pas de TID.
pub const ALCADIA: u32 = 1295;
/// Météo de Rain Control et Snow Control sans durée connue.
const SKY_DURATION: Duration = Duration::from_secs(300);
/// Décalage de l'horloge de Day and Night.
const DAY_AND_NIGHT_MS: i64 = 12 * nature::HOUR_MS;
/// Effets de `game_effect` (colonne de SKILL_SPEC).
const EFFECT_DAY_AND_NIGHT: i64 = 1;
const EFFECT_RAIN: i64 = 2;
const EFFECT_SNOW: i64 = 4;
/// Niveau MJ des commandes d'essai, sauf réglage `GM.CommandLevels`.
const GM_LEVEL: i64 = 3;
/// Cadence de la boucle.
const STEP: Duration = Duration::from_secs(1);

/// État du titre et de la guerre de Rensphere.
#[derive(Clone, Debug, Default)]
pub struct Archlord {
    /// ArchLord en titre (CID, nom).
    pub lord: Option<(u32, String)>,
    pub since: i64,
    /// Étape de la guerre de Rensphere (`archlord_step`) ; guerre suivie, fin de l'étape « portail ».
    pub step: i64,
    pub running: bool,
    pub fight_at: Option<Instant>,
    /// Météo imposée (fin, météo d'avant) ; fin du décalage de l'horloge.
    pub weather_until: Option<(Instant, i64)>,
    pub night_until: Option<Instant>,
    /// Gardes royaux (CID des invocations), du plus ancien au plus récent.
    pub guards: Vec<u32>,
    /// ArchLord monté sur Alcadia (son CID).
    pub dragon: Option<u32>,
    next_step: Option<Instant>,
}

impl Archlord {
    pub async fn load(db: &Databases) -> Self {
        let mut a = Archlord::default();
        match db.characters.first("SELECT character_id, name, UNIX_TIMESTAMP(since) AS since FROM archlord WHERE id = 1", ()).await {
            Ok(Some(r)) => {
                a.lord = col::<Option<u32>>(&r, "character_id").map(|cid| (cid, col(&r, "name")));
                a.since = col::<Option<i64>>(&r, "since").unwrap_or(0);
            }
            Ok(None) => {}
            Err(e) => warn!("[archlord] titre non lu ({e:#}) : lancer `archcore db-init`"),
        }
        info!("archlord : {}", a.lord.as_ref().map(|(_, n)| n.as_str()).unwrap_or("aucun"));
        a
    }
}

// ---------------------------------------------------------------------------------------------
// Paquets
// ---------------------------------------------------------------------------------------------

fn packet(f: Fields) -> Vec<u8> {
    build_packet(&schema::ARCHLORD, &f)
}

fn base(op: i64) -> Fields {
    Fields::new(&schema::ARCHLORD).with(0, op)
}

/// Op 0 (désigné) ou 1 (fin de règne) : nom de l'ArchLord.
pub fn name_packet(op: i64, name: &str) -> Vec<u8> {
    packet(base(op).with(1, name))
}

/// Op 5 : étape de la guerre de Rensphere.
pub fn step_packet(step: i64) -> Vec<u8> {
    packet(base(archlord_op::STEP).with(4, step))
}

/// Op 2 (activation) ou 3 (retrait) : nom du garde, vide pour un refus.
pub fn guard_packet(op: i64, guard: &str) -> Vec<u8> {
    packet(base(op).with(2, guard).with(3, i64::from(!guard.is_empty())))
}

/// Op 4 : gardes actifs et au plus.
pub fn guards_packet(current: usize, max: usize) -> Vec<u8> {
    packet(base(archlord_op::GUARD_INFO).with(5, current as i64).with(6, max as i64))
}

/// Op 6 : message (`archlord_msg`).
pub fn message_packet(id: i64) -> Vec<u8> {
    packet(base(archlord_op::MESSAGE).with(7, id))
}

/// Le joueur est l'ArchLord.
pub fn is_lord(st: &WorldState, sid: u32) -> bool {
    let cid = st.players.get(&sid).map(|p| p.ch.id);
    cid.is_some() && st.archlord.lord.as_ref().map(|(c, _)| *c) == cid
}

/// AgpmRide op 6 d'un ArchLord monté sur Alcadia, pour un joueur qui l'aperçoit (`presence.rs`).
pub fn dragon_view(st: &WorldState, cid: u32) -> Option<Vec<u8>> {
    (st.archlord.dragon == Some(cid)).then(|| ride::ride_packet(ride::ride_op::RIDE_TID, cid, ALCADIA))
}

/// Entrée en jeu : l'ArchLord (op 0), l'étape de la guerre en cours (op 5) ; ses compétences à l'ArchLord.
pub fn on_enter(ctx: &WorldCtx, st: &mut WorldState, sid: u32) {
    let Some(p) = st.players.get(&sid) else { return };
    if let Some((_, name)) = &st.archlord.lord {
        p.send_raw(name_packet(archlord_op::SET, name));
    }
    if st.archlord.step != archlord_step::NONE {
        p.send_raw(step_packet(st.archlord.step));
    }
    if is_lord(st, sid) {
        grant_skills(ctx, st, sid);
    }
}

/// AgpmArchlord reçu du client : seule l'op 7 en vient, sans objet qui porte la fonction 36.
pub fn on_packet(st: &WorldState, sid: u32, p: &Packet) {
    let name = st.players.get(&sid).map(|p| p.ch.name.clone()).unwrap_or_default();
    match p.int(0) {
        Some(archlord_op::EVENT_REQUEST) => {
            info!("[archlord] {name} : demande d'événement ArchLord (op 7), aucun objet ne porte la fonction {}", super::npcs::event_function::ARCHLORD)
        }
        other => warn!("[archlord] #{sid} op {other:?} refusée (réservée au serveur)"),
    }
}

fn send_all(st: &WorldState, plain: &[u8]) {
    siege::send_all(st, plain);
}

// ---------------------------------------------------------------------------------------------
// Titre
// ---------------------------------------------------------------------------------------------

/// Compétences de l'ArchLord (`skill_type` 9) qu'il n'a pas encore, données en mémoire (AGPMSKILL op 0).
fn grant_skills(ctx: &WorldCtx, st: &mut WorldState, sid: u32) {
    let mut given = 0;
    for tid in ctx.skills.archlord_skills() {
        if st.players.get(&sid).is_none_or(|p| p.skills.by_tid(tid).is_some()) {
            continue;
        }
        st.next_skill_id += 1;
        let owned = Owned { id: st.next_skill_id, tid, level: 1, ready_at: None, exp: 0 };
        let p = st.players.get_mut(&sid).unwrap();
        p.skills.list.push(owned);
        p.send_raw(skills::add_packet(p.ch.id, &owned));
        given += 1;
    }
    if given > 0 {
        info!("[archlord] {} reçoit {given} compétence(s) d'ArchLord", st.players[&sid].ch.name);
    }
}

/// Fin de règne d'un joueur en jeu : gardes renvoyés, descente d'Alcadia. Ses compétences restent connues jusqu'à sa
/// déconnexion, mais refusées (`skills.rs`).
fn revoke(st: &mut WorldState, cid: u32) {
    for g in std::mem::take(&mut st.archlord.guards) {
        summons::dismiss(st, g, "fin du règne de l'ArchLord", true);
    }
    if st.archlord.dragon == Some(cid) {
        st.archlord.dragon = None;
        if let Some(sid) = st.by_cid.get(&cid).copied() {
            combat::send_around(st, sid, &ride::ride_packet(ride::ride_op::DISMOUNT_ACK, cid, 0));
        }
    }
}

/// Change d'ArchLord (`None` : aucun) : op 1 de l'ancien puis op 0 du nouveau à tous, titre enregistré.
pub async fn set_lord(ctx: &WorldCtx, st: &mut WorldState, who: Option<(u32, String)>, why: &str) -> anyhow::Result<()> {
    let old = st.archlord.lord.clone();
    if old.as_ref().map(|o| o.0) == who.as_ref().map(|w| w.0) {
        return Ok(());
    }
    if let Some((ocid, oname)) = &old {
        revoke(st, *ocid);
        send_all(st, &name_packet(archlord_op::CANCEL, oname));
    }
    st.archlord.lord = who.clone();
    st.archlord.since = unix_now();
    if let Some((cid, name)) = &who {
        send_all(st, &name_packet(archlord_op::SET, name));
        if let Some(sid) = st.by_cid.get(cid).copied() {
            grant_skills(ctx, st, sid);
        }
    }
    let (cid, name) = who.clone().map(|(c, n)| (Some(c), n)).unwrap_or((None, String::new()));
    ctx.db
        .characters
        .exec(
            "INSERT INTO archlord (id, character_id, name, since) VALUES (1, ?, ?, FROM_UNIXTIME(?)) \
             ON DUPLICATE KEY UPDATE character_id = VALUES(character_id), name = VALUES(name), since = VALUES(since)",
            (cid, name.clone(), st.archlord.since),
        )
        .await?;
    info!("[archlord] {} → {} ({why})", old.map(|o| o.1).unwrap_or_else(|| "aucun".into()), if name.is_empty() { "aucun".into() } else { name });
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Guerre de Rensphere
// ---------------------------------------------------------------------------------------------

/// Change d'étape et l'annonce à tous (op 5).
fn set_step(st: &mut WorldState, step: i64) {
    st.archlord.step = step;
    send_all(st, &step_packet(step));
    info!("[archlord] guerre de Rensphere : étape {step}");
}

/// Assaillants de Rensphere : les guildes propriétaires d'un autre château, sauf celle qui tient Rensphere.
pub fn attackers(st: &WorldState) -> Vec<u32> {
    let holder = st.siege.castles.get(&RENSPHERE).and_then(|c| c.owner);
    let mut v: Vec<u32> = st.siege.castles.values().filter(|c| c.id != RENSPHERE).filter_map(|c| c.owner).filter(|g| Some(*g) != holder).collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// Début de la guerre de Rensphere (`siege::war::start`) : assaillants inscrits, étape 1 (portail ouvert).
pub fn on_war_start(ctx: &WorldCtx, st: &mut WorldState, now: Instant) {
    let list = attackers(st);
    let c = st.siege.castles.entry(RENSPHERE).or_insert_with(|| Castle::new(RENSPHERE));
    c.applications.retain(|a| a.side == Side::Defense);
    c.applications.extend(list.iter().map(|g| Application { guild: *g, side: Side::Attack, selected: true }));
    st.archlord.running = true;
    st.archlord.fight_at = Some(now + Duration::from_secs(ctx.config.world.archlord_portal_minutes.max(0) as u64 * 60));
    set_step(st, archlord_step::PORTAL);
    info!("[archlord] guerre de Rensphere : {} guilde(s) assaillante(s)", list.len());
}

/// Date de la prochaine guerre : `interval` jours après la dernière à `hour` h (jamais avant maintenant : sinon le
/// prochain `hour` h) ; sans guerre passée, `interval` jours après aujourd'hui.
pub fn next_war(last: Option<NaiveDate>, now: NaiveDateTime, interval: i64, hour: u32) -> NaiveDateTime {
    let day = last.unwrap_or(now.date()) + chrono::Duration::days(interval.max(1));
    let at = day.and_hms_opt(hour.min(23), 0, 0).unwrap_or(now);
    if at > now {
        return at;
    }
    let today = now.date().and_hms_opt(hour.min(23), 0, 0).unwrap_or(now);
    if today > now { today } else { today + chrono::Duration::days(1) }
}

/// Le sceau de Rensphere gravé : statut 7 (bataille de l'ArchLord) pour la moitié de la durée au plus, compétence de sceau
/// au graveur.
async fn begin_battle(ctx: &WorldCtx, st: &mut WorldState, now: Instant) -> anyhow::Result<()> {
    let Some(def) = ctx.sieges.get(RENSPHERE).cloned() else { return Ok(()) };
    let half = siege::duration(ctx, &def) / 2;
    let Some(c) = st.siege.castles.get_mut(&RENSPHERE) else { return Ok(()) };
    c.status = status::ARCHLORD;
    if let Some(w) = c.war.as_mut() {
        w.ends_at = w.ends_at.min(now + half);
    }
    let carver = c.carve.and_then(|g| st.guilds.by_id.get(&g)).and_then(|g| g.master()).and_then(|m| st.by_cid.get(&m.cid)).copied();
    set_step(st, archlord_step::BATTLE);
    siege::save(ctx, st, RENSPHERE).await?;
    siege::broadcast_castle(ctx, st, RENSPHERE);
    // Compétence posée au carving (StampSkillTID / StampSkillLevel de SiegeWar.txt).
    let (tid, level) = def.stamp_skill;
    let Some((sid, lv)) = carver.zip(ctx.skills.get(tid).and_then(|t| t.level(level.max(1))).cloned()) else { return Ok(()) };
    let caster_tid = st.players[&sid].ch.template_id;
    let owned = Owned { id: 0, tid, level: level.max(1), ready_at: None, exp: 0 };
    let e = skills::new_effect(&owned, &lv, caster_tid, lv.status, None, now);
    skills::add_player_effect(ctx, st, sid, super::effects::Effect { caster_sid: sid, ..e });
    info!("[archlord] {} reçoit la compétence de sceau {tid}", st.players[&sid].ch.name);
    Ok(())
}

/// Fin de la guerre de Rensphere : le chef de la guilde qui a gravé le sceau devient ArchLord, sinon rien ne change.
async fn end_war(ctx: &WorldCtx, st: &mut WorldState) -> anyhow::Result<()> {
    st.archlord.running = false;
    st.archlord.fight_at = None;
    set_step(st, archlord_step::END);
    st.archlord.step = archlord_step::NONE;
    let carve = st.siege.castles.get(&RENSPHERE).and_then(|c| c.carve);
    let winner = carve.and_then(|g| st.guilds.by_id.get(&g)).and_then(|g| g.master()).map(|m| (m.cid, m.name.clone()));
    match winner {
        Some(w) => set_lord(ctx, st, Some(w), "guerre de Rensphere").await?,
        None => info!("[archlord] guerre de Rensphere sans sceau gravé : l'ArchLord en place le reste"),
    }
    Ok(())
}

/// Boucle de l'ArchLord (appelée par la boucle de combat) : effets échus, gardes, étapes et calendrier de la guerre.
pub async fn step(ctx: &WorldCtx, st: &mut WorldState, now: Instant) -> anyhow::Result<()> {
    if st.archlord.next_step.is_some_and(|t| now < t) {
        return Ok(());
    }
    st.archlord.next_step = Some(now + STEP);
    if let Some((_, before)) = st.archlord.weather_until.filter(|(t, _)| now >= *t) {
        st.archlord.weather_until = None;
        nature::set_weather(st, before, "fin de la météo de l'ArchLord");
    }
    if st.archlord.night_until.is_some_and(|t| now >= t) {
        st.archlord.night_until = None;
        st.nature.shift_ms = 0;
        nature::broadcast_timer(ctx, st);
        info!("[archlord] fin de Day and Night : horloge rétablie");
    }
    let guards: Vec<u32> = st.archlord.guards.iter().copied().filter(|g| st.summons.list.contains_key(g)).collect();
    st.archlord.guards = guards;
    // Alcadia : cavalier parti, mort, ou plus ArchLord.
    if let Some(cid) = st.archlord.dragon {
        match st.by_cid.get(&cid).copied() {
            None => st.archlord.dragon = None,
            Some(sid) if st.players[&sid].dead || !is_lord(st, sid) => {
                st.archlord.dragon = None;
                combat::send_around(st, sid, &ride::ride_packet(ride::ride_op::DISMOUNT_ACK, cid, 0));
            }
            _ => {}
        }
    }
    let Some(c) = st.siege.castles.get(&RENSPHERE).cloned() else { return Ok(()) };
    let fighting = status::at_war(c.status) && c.status != status::ENDED;
    if fighting {
        if !st.archlord.running {
            st.archlord.running = true;
        }
        if st.archlord.step == archlord_step::PORTAL && st.archlord.fight_at.is_none_or(|t| now >= t) {
            set_step(st, archlord_step::FIGHT);
        }
        if c.status == status::WAR && c.carve.is_some() && st.archlord.step < archlord_step::BATTLE {
            begin_battle(ctx, st, now).await?;
        }
        return Ok(());
    }
    if st.archlord.running {
        return end_war(ctx, st).await;
    }
    // Calendrier : une guerre tous les Duration4nextSW jours, à SWBeginTime.
    if !ctx.config.world.siege_enabled || c.status != status::NONE {
        return Ok(());
    }
    let Some(def) = ctx.sieges.get(RENSPHERE).cloned().filter(|d| !d.disabled) else { return Ok(()) };
    let (tz, unix) = (ctx.config.world.time_zone, unix_now());
    let schedule = |st: &mut WorldState, from: Option<NaiveDate>| {
        let at = siege::from_local(tz, next_war(from, siege::to_local(tz, unix), def.interval_days, def.begin_hour));
        st.siege.castles.get_mut(&RENSPHERE).unwrap().next_siege = at;
        info!("[archlord] prochaine guerre de Rensphere le {}", siege::to_local(tz, at));
    };
    if c.next_siege == 0 {
        schedule(st, (c.last_siege > 0).then(|| siege::to_local(tz, c.last_siege).date()));
    } else if unix >= c.next_siege {
        if attackers(st).is_empty() {
            info!("[archlord] guerre de Rensphere sans assaillant (aucun château n'a de propriétaire) : remise");
            schedule(st, Some(siege::to_local(tz, c.next_siege).date()));
        } else {
            if let Err(e) = siege::force_start(ctx, st, "0", now).await? {
                warn!("[archlord] guerre de Rensphere non ouverte : {e}");
            }
            return Ok(());
        }
    } else {
        return Ok(());
    }
    siege::save(ctx, st, RENSPHERE).await?;
    siege::broadcast_castle(ctx, st, RENSPHERE);
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Compétences
// ---------------------------------------------------------------------------------------------

/// Gardes royaux de l'ArchLord `sid` : jusqu'à `n` de plus, `Archlord.MaxGuards` en tout. Rend le nom du dernier appelé.
fn call_guards(ctx: &WorldCtx, st: &mut WorldState, sid: u32, n: usize, now: Instant) -> Option<String> {
    let max = ctx.config.world.archlord_max_guards;
    let (tid, ms) = ctx
        .skills
        .get(CALL_ROYAL_KNIGHT)
        .and_then(|t| t.level(1))
        .filter(|l| l.summons_tid > 0)
        .map(|l| (l.summons_tid, if l.duration_ms > 0 { l.duration_ms } else { GUARD_DURATION.as_millis() as u64 }))
        .unwrap_or((ROYAL_KNIGHT, GUARD_DURATION.as_millis() as u64));
    let mut last = None;
    for _ in 0..n {
        if st.archlord.guards.len() >= max {
            break;
        }
        let Some(cid) = summons::spawn_forced(ctx, st, sid, tid, CALL_ROYAL_KNIGHT, Some(ms), now) else { break };
        st.archlord.guards.push(cid);
        last = st.monsters.get(cid).map(|m| m.name.clone());
    }
    last
}

/// Monte sur Alcadia, ou en descend.
fn toggle_dragon(st: &mut WorldState, sid: u32, tid: u32) {
    let p = &st.players[&sid];
    let (cid, name) = (p.ch.id, p.ch.name.clone());
    if st.archlord.dragon == Some(cid) {
        st.archlord.dragon = None;
        combat::send_around(st, sid, &ride::ride_packet(ride::ride_op::DISMOUNT_ACK, cid, 0));
        info!("[archlord] {name} descend d'Alcadia");
        return;
    }
    if ride::is_mounted(&p.inventory) {
        send_system(st, sid, "Descendez d'abord de votre monture.");
        return;
    }
    st.archlord.dragon = Some(cid);
    combat::send_around(st, sid, &ride::ride_packet(ride::ride_op::RIDE_TID, cid, tid));
    info!("[archlord] {name} monte Alcadia ({tid})");
}

/// Fin d'incantation d'une compétence de l'ArchLord qui agit sur le monde (`skills::complete`) : rend vrai si elle est
/// prise en charge ici (monture, gardes, ciel), faux pour le moteur ordinaire.
pub fn on_cast(ctx: &WorldCtx, st: &mut WorldState, sid: u32, t: &SkillTemplate, lv: &SkillLevel, now: Instant) -> bool {
    if !t.archlord_only() {
        return false;
    }
    let name = st.players.get(&sid).map(|p| p.ch.name.clone()).unwrap_or_default();
    if t.ride {
        toggle_dragon(st, sid, if lv.summons_tid > 0 { lv.summons_tid } else { ALCADIA });
        return true;
    }
    if t.summons == summons::skill_kind::SUMMONS {
        let called = call_guards(ctx, st, sid, lv.summons_count.max(1) as usize, now);
        if let Some(p) = st.players.get(&sid) {
            p.send_raw(guard_packet(archlord_op::GUARD_ADD, called.as_deref().unwrap_or("")));
            p.send_raw(guards_packet(st.archlord.guards.len(), ctx.config.world.archlord_max_guards));
        }
        info!("[archlord] {name} lance {} : {} garde(s)", t.name, st.archlord.guards.len());
        return true;
    }
    match t.game_effect {
        EFFECT_RAIN | EFFECT_SNOW => {
            let code = if t.game_effect == EFFECT_RAIN { weather::RAIN } else { weather::SNOW };
            let before = st.archlord.weather_until.map(|(_, b)| b).unwrap_or(st.nature.weather);
            let lasting = if lv.duration_ms > 0 { Duration::from_millis(lv.duration_ms) } else { SKY_DURATION };
            st.archlord.weather_until = Some((now + lasting, before));
            nature::set_weather(st, code, &format!("{} de {name}", t.name));
        }
        EFFECT_DAY_AND_NIGHT => {
            st.nature.shift_ms = DAY_AND_NIGHT_MS;
            st.archlord.night_until = Some(now + Duration::from_secs(ctx.config.world.archlord_sky_minutes.max(1) as u64 * 60));
            nature::broadcast_timer(ctx, st);
            info!("[archlord] {name} lance {} : horloge décalée de 12 h", t.name);
        }
        _ => return false,
    }
    true
}

// ---------------------------------------------------------------------------------------------
// Commandes de discussion
// ---------------------------------------------------------------------------------------------

/// Commandes de l'ArchLord et commandes d'essai des MJ ; rend faux pour un texte qui n'en est pas une (commandes MJ
/// ordinaires, `gm.rs`).
pub async fn chat_command(ctx: &WorldCtx, st: &mut WorldState, sid: u32, text: &str) -> anyhow::Result<bool> {
    let body = text.trim().trim_start_matches('/');
    let (cmd, rest) = body.split_once(char::is_whitespace).map(|(c, r)| (c, r.trim())).unwrap_or((body, ""));
    let cmd = cmd.to_lowercase();
    let Some(p) = st.players.get(&sid) else { return Ok(false) };
    let (gm_level, cid, me) = (p.gm_level, p.ch.id, p.ch.name.clone());
    let gm = gm_level >= ctx.config.world.gm_command_levels.get(cmd.as_str()).copied().unwrap_or(GM_LEVEL);
    let max = ctx.config.world.archlord_max_guards;
    let lord = is_lord(st, sid);
    let reply: Option<Result<String, String>> = match cmd.as_str() {
        "guardactivate" => {
            if !lord {
                p.send_raw(message_packet(archlord_msg::GUARD_ADD_UNAVAILABLE));
                return Ok(true);
            }
            let called = call_guards(ctx, st, sid, 1, Instant::now());
            let p = &st.players[&sid];
            p.send_raw(guard_packet(archlord_op::GUARD_ADD, called.as_deref().unwrap_or("")));
            p.send_raw(guards_packet(st.archlord.guards.len(), max));
            None
        }
        "guarddeactivate" => {
            if !lord {
                p.send_raw(message_packet(archlord_msg::GUARD_REMOVE_UNAVAILABLE));
                return Ok(true);
            }
            let gone = st.archlord.guards.pop().map(|g| (g, st.monsters.get(g).map(|m| m.name.clone()).unwrap_or_default()));
            if let Some((g, _)) = gone {
                summons::dismiss(st, g, "renvoyé par l'ArchLord", true);
            }
            let p = &st.players[&sid];
            p.send_raw(guard_packet(archlord_op::GUARD_REMOVE, gone.map(|(_, n)| n).as_deref().unwrap_or("")));
            p.send_raw(guards_packet(st.archlord.guards.len(), max));
            None
        }
        "guardinformation" => {
            p.send_raw(guards_packet(if lord { st.archlord.guards.len() } else { 0 }, max));
            None
        }
        "archlord" => match rest.split_once(char::is_whitespace).map(|(a, b)| (a, b.trim())).unwrap_or((rest, "")) {
            ("set", who) if gm => match super::find_character(ctx, st, who.trim_matches('"')).await? {
                Some(w) => {
                    let name = w.1.clone();
                    set_lord(ctx, st, Some(w), &format!("désigné par {me}")).await?;
                    Some(Ok(format!("{name} est l'ArchLord.")))
                }
                None => Some(Err(format!("personnage introuvable : {who}"))),
            },
            ("clear", _) if gm => {
                set_lord(ctx, st, None, &format!("retiré par {me}")).await?;
                Some(Ok("Plus d'ArchLord.".into()))
            }
            ("", _) => Some(Ok(match &st.archlord.lord {
                Some((_, n)) => format!("ArchLord : {n}."),
                None => "Aucun ArchLord.".into(),
            })),
            _ if lord => {
                // Lord Howling : message de l'ArchLord à tout le serveur.
                let pkt = super::chat::chat_packet(chat_type::NOTICE_LEVEL1, None, None, None, &format!("[ArchLord] {me} : {rest}"));
                send_all(st, &pkt);
                info!("[archlord] {me} à tous : {rest}");
                None
            }
            _ => Some(Err("seul l'ArchLord parle à tout le serveur".into())),
        },
        // Commandes d'essai des MJ (sysstr 622 à 629, 636, 657, 658).
        "archlordbegin" if gm => Some(siege::force_start(ctx, st, "0", Instant::now()).await?),
        "archlordend" if gm => Some(siege::force_end(ctx, st, "0", Instant::now()).await?),
        "nextstep" if gm => {
            let next = (st.archlord.step + 1).min(archlord_step::BATTLE);
            if st.archlord.running {
                set_step(st, next);
                Some(Ok(format!("Étape {next}.")))
            } else {
                Some(Err("aucune guerre de Rensphere en cours".into()))
            }
        }
        "archlordactivate" if gm => Some(match super::find_character(ctx, st, rest.trim_matches('"')).await? {
            Some(w) => {
                set_lord(ctx, st, Some(w), &format!("désigné par {me}")).await?;
                Ok("ArchLord désigné.".into())
            }
            None => Err(format!("personnage introuvable : {rest}")),
        }),
        "archlorddeactivate" if gm => {
            set_lord(ctx, st, None, &format!("retiré par {me}")).await?;
            Some(Ok("Plus d'ArchLord.".into()))
        }
        "castleowner" if gm => {
            let (castle, guild) = rest.split_once(char::is_whitespace).map(|(a, b)| (a, b.trim())).unwrap_or((rest, ""));
            Some(siege::set_owner(ctx, st, castle, guild).await?)
        }
        "deletecastleowner" if gm => Some(siege::set_owner(ctx, st, rest, "-").await?),
        _ => return Ok(false),
    };
    if let Some(r) = reply {
        let (ok, line) = match &r {
            Ok(t) => (true, t.clone()),
            Err(e) => (false, format!("/{cmd} : {e}.")),
        };
        if gm_level > 0 {
            if let Some(p) = st.players.get(&sid) {
                crate::journal::gm(&ctx.db, p.account.id, (cid, &me), gm_level, &cmd, rest, ok, &line);
            }
        }
        send_system(st, sid, &line);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use archcore_net::decode_packet;

    fn at(y: i32, m: u32, d: u32, h: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, 0, 0).unwrap()
    }

    #[test]
    fn paquets() {
        let s = &schema::ARCHLORD;
        let p = decode_packet(s, &name_packet(archlord_op::SET, "GmTest")).unwrap();
        assert_eq!((p.int(0), p.text(1).as_deref(), p.bytes(2)), (Some(0), Some("GmTest"), None));
        let p = decode_packet(s, &step_packet(archlord_step::BATTLE)).unwrap();
        assert_eq!((p.int(0), p.int(4)), (Some(5), Some(3)));
        let p = decode_packet(s, &guard_packet(archlord_op::GUARD_ADD, "Royal Knight")).unwrap();
        assert_eq!((p.int(0), p.text(2).as_deref(), p.int(3)), (Some(2), Some("Royal Knight"), Some(1)));
        let p = decode_packet(s, &guard_packet(archlord_op::GUARD_REMOVE, "")).unwrap();
        assert_eq!((p.int(0), p.text(2).as_deref(), p.int(3)), (Some(3), Some(""), Some(0)), "vide : refus (641)");
        let p = decode_packet(s, &guards_packet(2, 10)).unwrap();
        assert_eq!((p.int(0), p.int(5), p.int(6)), (Some(4), Some(2), Some(10)));
        let p = decode_packet(s, &message_packet(archlord_msg::GUARD_ADD_UNAVAILABLE)).unwrap();
        assert_eq!((p.int(0), p.int(7)), (Some(6), Some(1)));
        // Demande du client : l'op seule (drapeaux 0x0001), puis l'octet de fin.
        let raw = build_packet(s, &base(archlord_op::EVENT_REQUEST));
        assert_eq!(raw.len(), 19 + 2 + 1 + 1);
        assert_eq!(decode_packet(s, &raw).unwrap().int(0), Some(7));
    }

    #[test]
    fn date_de_la_guerre() {
        let now = at(2026, 10, 28, 12);
        assert_eq!(next_war(Some(NaiveDate::from_ymd_opt(2026, 10, 20).unwrap()), now, 30, 20), at(2026, 11, 19, 20), "30 jours après la dernière");
        assert_eq!(next_war(None, now, 30, 20), at(2026, 11, 27, 20), "jamais eu lieu : dans 30 jours");
        assert_eq!(next_war(Some(NaiveDate::from_ymd_opt(2026, 9, 1).unwrap()), now, 30, 20), at(2026, 10, 28, 20), "date passée : ce soir");
        assert_eq!(next_war(Some(NaiveDate::from_ymd_opt(2026, 9, 1).unwrap()), at(2026, 10, 28, 21), 30, 20), at(2026, 10, 29, 20), "passée et ce soir aussi : demain");
    }
}
