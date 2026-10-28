//! Combats de Shrine de l'épisode 8 (AgpmShrineBattle, 0x62, structure brute dans les deux sens).
//!
//! ```text
//!   entrée en jeu               ← op 2 : liste des Shrines (10 entrées de 0xF0, Shrine de 0xD0 en +0x20 de chacune)
//!   C op 0 (fenêtre ouverte)    ← op 2, op 11 (victoires et défaites du joueur), op 12 (celles de sa race)
//!   T − 10, − 5, − 1 min        ← op 7 messages 1, 2 (+ 3), 4 à tous ; op 10 (minutes) aux joueurs du niveau voulu
//!   C op 9 (entrer)             → refus : AgpmSystemMessage code 47 (niveau), 48 (effectif de la race plein), message 8
//!                                 hors des heures ; sinon téléportation au point de la zone de sa race (244 à 275)
//!   ouverture                   ← op 1 état 1 à tous ; messages 5 (tous), 6 (joueurs du Shrine)
//!   gravure (région centrale)   ← op 1 état 2, op 3 jauge (pour cent) ; messages 9, 13 (graveur), 14 (sa race), 15 (les
//!                                 autres) ; échec (mort, pas, sortie du centre) : message 10, op 6 état 2, op 3 à 0
//!   occupation                  ← op 1 état 4 ; messages 12, 11, 7 (minutes) ; perdue (mort, sortie du Shrine) : 16
//!   victoire                    ← op 1 état 6 ; messages 17, 22 à tous ; 18 (charisme), 19 (pièces), 23 aux présents
//!   fin                         ← messages 24, 25 (10 et 5 min avant), 20, 21 sans vainqueur ; 15 s après la fin, 27
//!                                 et retour en ville des présents, 26 à tous, op 1 état 0
//!   C op 8                      → sens inconnu (absent du client d'Archonia) : journalisé
//! ```
//! Régions (RegionTemplate.ini) : centres 236 à 243 (Foudre, Syripus, Eau, Lave, Vent, Poison, Feu, Esprit), zones de
//! race 244 à 275 (quatre par Shrine : humain, orc, elfe de lune, Dragon Scion ; un point de résurrection de la race dans
//! chacune), extérieurs 276 à 283. Le client ne connaît que ces régions et les messages : horaires, règles d'entrée,
//! gravure, occupation et récompenses sont des **choix d'ArchCore**, réglables dans les tables `shrine` et
//! `shrine_reward` (couche de contenu) :
//! * chaque Shrine ouvre selon son horaire (`Shrine.Enabled`) ou par la commande MJ `/shrine open <n>` ;
//! * la gravure se lance en restant immobile dans la région centrale d'un Shrine ouvert (le client n'envoie rien de
//!   connu pour cela ; l'op 8 est seulement journalisée) ; elle dure `carve_sec` secondes ;
//! * la race du graveur occupe alors le Shrine : il doit rester en vie et dans le Shrine `occupy_sec` secondes ;
//! * pendant le combat (états 1 à 4), dans les régions du Shrine, deux races différentes sont ennemies sans Ctrl, une
//!   même race ne se bat pas, et un meurtre ne compte ni points de meurtrier ni avis de recherche (`pvp.rs`) ; un mort
//!   ressuscite au point de la zone de sa race (`battlesquare::resurrection_point`). Hors combat, règles normales.

use super::skills::CharismaSource;
use super::{WorldCtx, WorldState};
use crate::db::{col, Databases};
use crate::{info, warn};
use archcore_net::schema::{self, ptype, shrine_op};
use archcore_net::{Fields, Packet};
use chrono::{Datelike, NaiveDateTime};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::{Duration, Instant};

pub const CENTER_FIRST: u16 = 236;
pub const CENTER_LAST: u16 = 243;
const ZONE_FIRST: u16 = 244;
const OUTER_FIRST: u16 = 276;
pub const REGION_LAST: u16 = 283;
/// Entrées de la liste du client (op 2).
pub const LIST_SIZE: usize = 10;
/// Taille de la structure d'un Shrine (copiée par 0x140729200).
const STRUCT_SIZE: usize = 0xd0;
/// Cadence de la boucle des Shrines.
const STEP: Duration = Duration::from_secs(1);
/// Délai entre la fin du combat et le retour en ville des présents.
pub const RETURN_DELAY: Duration = Duration::from_secs(15);
/// L'entrée est permise dès 5 minutes avant l'ouverture (icône « Move to Shrine »).
const EARLY_ENTRY: Duration = Duration::from_secs(300);

/// États d'un Shrine (champ +0x80 de la structure).
pub mod state {
    pub const CLOSED: i32 = 0;
    pub const OPEN: i32 = 1;
    pub const CARVING: i32 = 2;
    pub const OCCUPIED: i32 = 4;
    pub const ENDED: i32 = 6;
}

/// Messages de `shrinemessage.xml` (MessageTid, op 7).
pub mod msg {
    pub const IN_10_MIN: i64 = 1;
    pub const IN_5_MIN: i64 = 2;
    pub const MOVE_ICON: i64 = 3;
    pub const IN_1_MIN: i64 = 4;
    pub const STARTED: i64 = 5;
    pub const DOOR_OPENED: i64 = 6;
    pub const KEEP_SOUL: i64 = 7;
    pub const CANNOT_MOVE: i64 = 8;
    pub const CARVING: i64 = 9;
    pub const CARVE_FAILED: i64 = 10;
    pub const TAKING_OVER: i64 = 11;
    pub const CARVED: i64 = 12;
    pub const STAY_INSIDE: i64 = 13;
    pub const PROTECT: i64 = 14;
    pub const KILL: i64 = 15;
    pub const OCCUPY_FAILED: i64 = 16;
    pub const WON: i64 = 17;
    pub const CHARISMA: i64 = 18;
    pub const COINS: i64 = 19;
    pub const GATE_CLOSED: i64 = 20;
    pub const ENDED: i64 = 21;
    pub const RACE_WON: i64 = 22;
    pub const REWARDED: i64 = 23;
    pub const ENDS_10_MIN: i64 = 24;
    pub const ENDS_5_MIN: i64 = 25;
    pub const OVER: i64 = 26;
    pub const TO_TOWN: i64 = 27;
}

/// Codes AgpmSystemMessage des refus d'entrée.
pub mod code {
    pub const LEVEL: i64 = 47;
    pub const FULL: i64 = 48;
}

/// Une ligne de la table `shrine`.
#[derive(Clone, Debug, PartialEq)]
pub struct ShrineDef {
    pub id: u8,
    pub name: String,
    /// Région centrale (236 à 243).
    pub region: u16,
    /// Jours d'ouverture : bit 0 lundi … bit 6 dimanche.
    pub days: u8,
    pub open_hour: u32,
    pub open_minute: u32,
    pub duration: Duration,
    pub carve: Duration,
    pub occupy: Duration,
    pub level_min: i32,
    /// 0 : sans limite.
    pub level_max: i32,
    pub max_per_race: usize,
}

impl ShrineDef {
    /// Rang du Shrine (0 à 7) dans les blocs de régions.
    pub fn index(&self) -> u16 {
        self.region.saturating_sub(CENTER_FIRST)
    }
    pub fn level_ok(&self, level: i32) -> bool {
        level >= self.level_min && (self.level_max <= 0 || level <= self.level_max)
    }
}

/// Une récompense de `shrine_reward`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reward {
    pub item_tid: u32,
    pub count: i64,
    pub charisma: u32,
}

#[derive(Default)]
pub struct ShrineCatalog {
    pub list: Vec<ShrineDef>,
    pub carving: Vec<Reward>,
    pub win_race: Vec<Reward>,
    pub common: Vec<Reward>,
}

impl ShrineCatalog {
    pub async fn load(db: &Databases) -> Self {
        let mut c = ShrineCatalog::default();
        let sql = "SELECT id, name, region_id, days, open_hour, open_minute, duration_min, carve_sec, occupy_sec, level_min, level_max, max_per_race FROM shrine ORDER BY id";
        match db.world.rows(sql, ()).await {
            Ok(rows) => {
                for r in &rows {
                    let region: u16 = col::<u32>(r, "region_id") as u16;
                    if !(CENTER_FIRST..=CENTER_LAST).contains(&region) {
                        warn!("[shrine] Shrine {} : région centrale {region} hors de 236 à 243, ignoré", col::<u32>(r, "id"));
                        continue;
                    }
                    c.list.push(ShrineDef {
                        id: col::<u32>(r, "id") as u8,
                        name: col(r, "name"),
                        region,
                        days: col::<u32>(r, "days") as u8,
                        open_hour: col::<u32>(r, "open_hour").min(23),
                        open_minute: col::<u32>(r, "open_minute").min(59),
                        duration: Duration::from_secs(col::<u64>(r, "duration_min").max(1) * 60),
                        carve: Duration::from_secs(col::<u64>(r, "carve_sec").max(1)),
                        occupy: Duration::from_secs(col::<u64>(r, "occupy_sec")),
                        level_min: col(r, "level_min"),
                        level_max: col(r, "level_max"),
                        max_per_race: col::<u32>(r, "max_per_race").max(1) as usize,
                    });
                }
                c.list.truncate(LIST_SIZE);
            }
            Err(e) => warn!("[shrine] table shrine non lue ({e:#}) : lancer `archcore db-init` puis `archcore import` (couche de contenu)"),
        }
        if let Ok(rows) = db.world.rows("SELECT category, item_tid, item_count, charisma FROM shrine_reward", ()).await {
            for r in &rows {
                let reward = Reward { item_tid: col(r, "item_tid"), count: col(r, "item_count"), charisma: col::<i64>(r, "charisma").max(0) as u32 };
                match col::<String>(r, "category").to_ascii_lowercase().as_str() {
                    "carving" => c.carving.push(reward),
                    "winrace" => c.win_race.push(reward),
                    "common" => c.common.push(reward),
                    other => warn!("[shrine] catégorie de récompense inconnue : {other}"),
                }
            }
        }
        info!("Shrines : {} (récompenses : {} gravure, {} race, {} commune(s))", c.list.len(), c.carving.len(), c.win_race.len(), c.common.len());
        c
    }

    pub fn get(&self, id: u8) -> Option<&ShrineDef> {
        self.list.iter().find(|d| d.id == id)
    }
    fn by_index(&self, index: u16) -> Option<&ShrineDef> {
        self.list.iter().find(|d| d.index() == index)
    }
}

/// Rang du Shrine (0 à 7) d'une région 236 à 283.
pub fn index_of(region: u16) -> Option<u16> {
    match region {
        CENTER_FIRST..=CENTER_LAST => Some(region - CENTER_FIRST),
        ZONE_FIRST..=275 => Some((region - ZONE_FIRST) / 4),
        OUTER_FIRST..=REGION_LAST => Some(region - OUTER_FIRST),
        _ => None,
    }
}

/// Zone de la race `race` (1 à 4) du Shrine de rang `index`.
pub fn race_zone(index: u16, race: i32) -> u16 {
    ZONE_FIRST + 4 * index + (race.clamp(1, 4) - 1) as u16
}

/// Graveur, puis occupant, d'un Shrine.
#[derive(Clone, Debug, PartialEq)]
pub struct Carver {
    pub cid: u32,
    pub name: String,
    pub race: i32,
}

/// Un combat annoncé ou en cours.
#[derive(Clone, Debug)]
pub struct Battle {
    pub id: u8,
    pub state: i32,
    pub opens_at: Instant,
    pub ends_at: Instant,
    /// Annonces faites : bits 0-2 à 10, 5, 1 min de l'ouverture, bits 3-4 à 10 et 5 min de la fin.
    pub announced: u8,
    pub carver: Option<Carver>,
    /// Début de la gravure, puis fin de l'occupation.
    pub carve_from: Option<Instant>,
    pub occupy_until: Option<Instant>,
    /// Dernière jauge envoyée (pour cent).
    pub gauge: i64,
    /// Joueurs entrés par l'op 9 (CID).
    pub entered: BTreeSet<u32>,
    /// Retour en ville des présents (état 6).
    pub close_at: Option<Instant>,
    pub winner: Option<i32>,
}

impl Battle {
    fn new(id: u8, opens_at: Instant, duration: Duration) -> Self {
        Battle {
            id,
            state: state::CLOSED,
            opens_at,
            ends_at: opens_at + duration,
            announced: 0,
            carver: None,
            carve_from: None,
            occupy_until: None,
            gauge: 0,
            entered: BTreeSet::new(),
            close_at: None,
            winner: None,
        }
    }
    /// Combat en cours : les règles du Shrine s'appliquent.
    pub fn is_active(&self) -> bool {
        (state::OPEN..=state::OCCUPIED).contains(&self.state)
    }
}

#[derive(Default)]
pub struct Shrines {
    pub battles: BTreeMap<u8, Battle>,
    next_step: Option<Instant>,
    /// Ouverture planifiée déjà lancée, par Shrine (jour, pour ne pas rouvrir après la fin).
    done: HashMap<u8, i32>,
}

/// Ouverture du jour d'un Shrine vue de `now` (heure de `World.TimeZone`) : (jour, secondes avant l'ouverture,
/// négatives si elle est passée), seulement de 10 minutes avant l'ouverture jusqu'à la fin du combat.
pub fn schedule_delta(def: &ShrineDef, now: NaiveDateTime) -> Option<(i32, i64)> {
    let weekday = now.weekday().num_days_from_monday();
    if def.days & (1 << weekday) == 0 {
        return None;
    }
    let open = now.date().and_hms_opt(def.open_hour, def.open_minute, 0)?;
    let delta = (open - now).num_seconds();
    (delta <= 600 && -delta < def.duration.as_secs() as i64).then_some((now.date().num_days_from_ce(), delta))
}

fn local_now(ctx: &WorldCtx) -> NaiveDateTime {
    match ctx.config.world.time_zone {
        crate::config::TimeZone::Utc => chrono::Utc::now().naive_utc(),
        crate::config::TimeZone::Local => chrono::Local::now().naive_local(),
    }
}

// ---------------------------------------------------------------------------------------------
// Paquets (structure brute : 0x13 = 1, opération U16 en 0x14, corps dès 0x18)
// ---------------------------------------------------------------------------------------------

/// Paquet de `size` octets (pied exclu) : en-tête, 1 en 0x13, opération en 0x14, puis ce que `fill` écrit.
pub fn raw_packet(op: i64, size: usize, fill: impl FnOnce(&mut [u8])) -> Vec<u8> {
    let mut out = vec![0u8; size];
    out[0] = archcore_net::packet::GUARD_HEAD;
    out[5] = ptype::SHRINE_BATTLE;
    out[0x13] = 1;
    out[0x14..0x16].copy_from_slice(&(op as u16).to_le_bytes());
    fill(&mut out);
    out.push(archcore_net::packet::GUARD_TAIL);
    let len = out.len() as u32;
    out[1..5].copy_from_slice(&len.to_le_bytes());
    out
}

fn put_i32(b: &mut [u8], off: usize, v: i64) {
    b[off..off + 4].copy_from_slice(&(v as i32).to_le_bytes());
}

fn get_u32(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4).map(|x| u32::from_le_bytes(x.try_into().unwrap()))
}

/// Structure d'un Shrine : Id, nom, nom du graveur, état, race occupante, CID du graveur.
pub fn shrine_struct(def: &ShrineDef, battle: Option<&Battle>) -> Vec<u8> {
    let mut b = vec![0u8; STRUCT_SIZE];
    put_i32(&mut b, 0x00, def.id as i64);
    super::guild::society::put_text(&mut b, 0x04, 30, &def.name);
    if let Some(bt) = battle {
        if let Some(c) = &bt.carver {
            super::guild::society::put_text(&mut b, 0x22, 49, &c.name);
            put_i32(&mut b, 0x84, c.race as i64);
            put_i32(&mut b, 0xb8, c.cid as i64);
        }
        put_i32(&mut b, 0x80, bt.state as i64);
    }
    b
}

/// Op 1 : changement d'état d'un Shrine.
pub fn state_packet(def: &ShrineDef, battle: Option<&Battle>) -> Vec<u8> {
    let s = shrine_struct(def, battle);
    raw_packet(shrine_op::STATE, 0xf0, |b| {
        put_i32(b, 0x18, def.id as i64);
        b[0x20..0x20 + STRUCT_SIZE].copy_from_slice(&s);
    })
}

/// Op 2 : les dix entrées de la liste.
pub fn list_packet(cat: &ShrineCatalog, shrines: &Shrines) -> Vec<u8> {
    raw_packet(shrine_op::LIST, 0x20 + LIST_SIZE * 0xf0, |b| {
        for (i, def) in cat.list.iter().take(LIST_SIZE).enumerate() {
            let entry = 0x20 + i * 0xf0;
            put_i32(b, entry + 0x18, def.id as i64);
            let s = shrine_struct(def, shrines.battles.get(&def.id));
            b[entry + 0x20..entry + 0x20 + STRUCT_SIZE].copy_from_slice(&s);
        }
    })
}

/// Op 3 : jauge de gravure (0 la masque).
pub fn gauge_packet(id: u8, value: i64) -> Vec<u8> {
    raw_packet(shrine_op::GAUGE, 0x20, |b| {
        put_i32(b, 0x18, id as i64);
        put_i32(b, 0x1c, value);
    })
}

/// Op 6 : état d'un joueur (drapeau 0, état 2 : gravure annulée).
pub fn player_state_packet(id: u8, cid: u32, flag: i64, st: i64, value: i64) -> Vec<u8> {
    raw_packet(shrine_op::PLAYER_STATE, 0x2c, |b| {
        put_i32(b, 0x18, id as i64);
        put_i32(b, 0x1c, cid as i64);
        put_i32(b, 0x20, flag);
        put_i32(b, 0x24, st);
        put_i32(b, 0x28, value);
    })
}

/// Op 7 : message `tid` de shrinemessage.xml ; `value` : race, ou nombre des messages en « %d » ; `name` : joueur.
pub fn message_packet(id: u8, tid: i64, value: i64, name: &str) -> Vec<u8> {
    raw_packet(shrine_op::MESSAGE, 0x58, |b| {
        put_i32(b, 0x18, id as i64);
        put_i32(b, 0x1c, tid);
        put_i32(b, 0x20, value);
        super::guild::society::put_text(b, 0x24, 0x34, name);
    })
}

/// Op 10 : ouverture dans `minutes` minutes (proposition d'entrer).
pub fn opens_in_packet(id: u8, minutes: i64) -> Vec<u8> {
    raw_packet(shrine_op::OPENS_IN, 0x20, |b| {
        put_i32(b, 0x18, id as i64);
        put_i32(b, 0x1c, minutes);
    })
}

/// Op 11 (joueur) ou 12 (race) : victoires et défaites.
pub fn score_packet(op: i64, wins: i64, losses: i64) -> Vec<u8> {
    raw_packet(op, 0x24, |b| {
        put_i32(b, 0x1c, wins);
        put_i32(b, 0x20, losses);
    })
}

// ---------------------------------------------------------------------------------------------
// Règles
// ---------------------------------------------------------------------------------------------

fn region_of(ctx: &WorldCtx, st: &WorldState, sid: u32) -> Option<u16> {
    let p = st.players.get(&sid)?;
    let pos = p.ch.pos();
    ctx.map.region(pos.x as f64, pos.z as f64)
}

/// Shrine dont le joueur occupe une région pendant un combat en cours (états 1 à 4).
pub fn active_shrine(ctx: &WorldCtx, st: &WorldState, sid: u32) -> Option<u8> {
    let index = index_of(region_of(ctx, st, sid)?)?;
    let def = ctx.shrines.by_index(index)?;
    st.shrines.battles.get(&def.id).filter(|b| b.is_active()).map(|b| b.id)
}

/// Point de résurrection d'un joueur mort dans un Shrine en combat : celui de la zone de sa race.
pub fn resurrection_point(ctx: &WorldCtx, st: &WorldState, sid: u32) -> Option<archcore_net::Vec3> {
    let id = active_shrine(ctx, st, sid)?;
    let def = ctx.shrines.get(id)?;
    let p = st.players.get(&sid)?;
    let race = p.factors.ftype[0];
    ctx.regions.race_point(race_zone(def.index(), race), p.ch.pos(), race as i64)
}

/// Joueurs en jeu dans les régions du Shrine de rang `index`.
fn present(ctx: &WorldCtx, st: &WorldState, index: u16) -> Vec<u32> {
    st.sids()
        .into_iter()
        .filter(|sid| st.players[sid].in_game && region_of(ctx, st, *sid).and_then(index_of) == Some(index))
        .collect()
}

fn send_all(st: &WorldState, plain: &[u8]) {
    for p in st.players.values().filter(|p| p.in_game) {
        p.send_raw(plain.to_vec());
    }
}

fn send_to(st: &WorldState, sids: &[u32], plain: &[u8]) {
    for sid in sids {
        if let Some(p) = st.players.get(sid) {
            p.send_raw(plain.to_vec());
        }
    }
}

fn race_of(st: &WorldState, sid: u32) -> i32 {
    st.players.get(&sid).map(|p| p.factors.ftype[0]).unwrap_or(0)
}

/// Op 1 de l'état courant à tous les joueurs en jeu.
fn broadcast_state(ctx: &WorldCtx, st: &WorldState, id: u8) {
    if let Some(def) = ctx.shrines.get(id) {
        send_all(st, &state_packet(def, st.shrines.battles.get(&id)));
    }
}

/// Entrée en jeu : la liste des Shrines (le client n'a pas ShrineTable.txt : sa table vient de l'op 2).
pub fn on_enter(ctx: &WorldCtx, st: &WorldState, sid: u32) {
    if ctx.shrines.list.is_empty() {
        return;
    }
    if let Some(p) = st.players.get(&sid) {
        p.send_raw(list_packet(&ctx.shrines, &st.shrines));
    }
}

/// Ouvre un combat (commande MJ `/shrine open <n>`) : annonces sautées, ouverture au prochain tour. Rend le nom du
/// Shrine, ou l'erreur.
pub fn force_open(ctx: &WorldCtx, st: &mut WorldState, id: u8, now: Instant) -> Result<String, String> {
    let Some(def) = ctx.shrines.get(id) else { return Err(format!("Shrine inconnu : {id}")) };
    if st.shrines.battles.get(&id).is_some_and(|b| b.state != state::CLOSED) {
        return Err(format!("{} est déjà ouvert", def.name));
    }
    let mut b = Battle::new(id, now, def.duration);
    b.announced = 0b111;
    if let Some(old) = st.shrines.battles.get(&id) {
        b.entered = old.entered.clone();
    }
    st.shrines.battles.insert(id, b);
    st.shrines.next_step = None;
    info!("[shrine] {} ouvert par un MJ", def.name);
    Ok(def.name.clone())
}

/// Ferme un combat sans vainqueur (commande MJ `/shrine close <n>`).
pub fn force_close(ctx: &WorldCtx, st: &mut WorldState, id: u8, now: Instant) -> Result<String, String> {
    let Some(def) = ctx.shrines.get(id) else { return Err(format!("Shrine inconnu : {id}")) };
    if !st.shrines.battles.get(&id).is_some_and(|b| b.state != state::ENDED) {
        return Err(format!("{} n'est pas ouvert", def.name));
    }
    finish(ctx, st, id, now);
    Ok(def.name.clone())
}

/// État des Shrines pour la commande MJ `/shrine`.
pub fn describe(ctx: &WorldCtx, st: &WorldState) -> String {
    let mut lines = Vec::new();
    for d in &ctx.shrines.list {
        let what = match st.shrines.battles.get(&d.id) {
            None => "fermé".to_string(),
            Some(b) => format!("état {}{}", b.state, b.carver.as_ref().map(|c| format!(", graveur {}", c.name)).unwrap_or_default()),
        };
        lines.push(format!("{} {} ({:02}:{:02}, {} min) : {what}", d.id, d.name, d.open_hour, d.open_minute, d.duration.as_secs() / 60));
    }
    if lines.is_empty() { "Aucun Shrine (table shrine vide).".into() } else { lines.join("\n") }
}

/// Paquet AgpmShrineBattle reçu du client.
pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let raw = &p.raw;
    match p.int(0) {
        Some(shrine_op::LIST_REQUEST) => {
            on_enter(ctx, st, sid);
            send_scores(ctx, st, sid).await?;
        }
        Some(shrine_op::JOIN) => {
            let id = get_u32(raw, 0x18).unwrap_or(0);
            join(ctx, st, sid, id.min(255) as u8).await?;
        }
        op => {
            let body = raw.get(0x18..raw.len().saturating_sub(1)).unwrap_or_default();
            info!("[shrine] #{sid} op {op:?} reçue ({} o) : {}", raw.len(), archcore_net::hex_dump(body, 48));
        }
    }
    Ok(())
}

/// Op 11 et 12 : scores du joueur et de sa race.
async fn send_scores(ctx: &WorldCtx, st: &WorldState, sid: u32) -> anyhow::Result<()> {
    let Some(p) = st.players.get(&sid) else { return Ok(()) };
    let me = ctx.db.characters.first("SELECT wins, losses FROM shrine_score WHERE character_id = ?", (p.ch.id,)).await?;
    let race = ctx.db.characters.first("SELECT wins, losses FROM shrine_race_score WHERE race = ?", (p.factors.ftype[0],)).await?;
    let pair = |r: Option<mysql_async::Row>| r.map(|r| (col::<i64>(&r, "wins"), col::<i64>(&r, "losses"))).unwrap_or((0, 0));
    let ((w, l), (rw, rl)) = (pair(me), pair(race));
    p.send_raw(score_packet(shrine_op::MY_SCORE, w, l));
    p.send_raw(score_packet(shrine_op::RACE_SCORE, rw, rl));
    Ok(())
}

fn system_message(st: &WorldState, sid: u32, code: i64, param: i64) {
    if let Some(p) = st.players.get(&sid) {
        let s = &schema::SYSTEM_MESSAGE;
        p.send(s, Fields::new(s).with(0, 0).with(1, code).with(2, param));
    }
}

/// Op 9 : entrer dans un Shrine, jusqu'à 5 minutes avant l'ouverture.
async fn join(ctx: &WorldCtx, st: &mut WorldState, sid: u32, id: u8) -> anyhow::Result<()> {
    let Some(def) = ctx.shrines.get(id).cloned() else {
        warn!("[shrine] #{sid} demande à entrer dans un Shrine inconnu ({id})");
        return Ok(());
    };
    let Some(p) = st.players.get(&sid).filter(|p| p.in_game && !p.dead) else { return Ok(()) };
    let (cid, name, level, race, pos) = (p.ch.id, p.ch.name.clone(), p.factors.status[7], p.factors.ftype[0], p.ch.pos());
    let now = Instant::now();
    let open = st.shrines.battles.get(&id).is_some_and(|b| b.is_active() || (b.state == state::CLOSED && b.opens_at <= now + EARLY_ENTRY));
    if !open {
        p.send_raw(message_packet(id, msg::CANNOT_MOVE, 0, ""));
        info!("[shrine] {name} : entrée dans {} refusée (fermé)", def.name);
        return Ok(());
    }
    if !def.level_ok(level) {
        system_message(st, sid, code::LEVEL, def.level_min as i64);
        info!("[shrine] {name} : entrée dans {} refusée (niveau {level}, {} exigé)", def.name, def.level_min);
        return Ok(());
    }
    let same_race = st.shrines.battles[&id].entered.iter().filter(|c| **c != cid && st.by_cid.get(c).is_some_and(|s| race_of(st, *s) == race)).count();
    if same_race >= def.max_per_race {
        system_message(st, sid, code::FULL, def.max_per_race as i64);
        info!("[shrine] {name} : entrée dans {} refusée (effectif de la race plein)", def.name);
        return Ok(());
    }
    let zone = race_zone(def.index(), race);
    let Some(to) = ctx.regions.race_point(zone, pos, race as i64) else {
        warn!("[shrine] aucun point de la race {race} dans la région {zone} : entrée dans {} impossible", def.name);
        return Ok(());
    };
    st.shrines.battles.get_mut(&id).unwrap().entered.insert(cid);
    super::teleport(ctx, st, sid, to.x as f64, Some(to.y as f64), to.z as f64).await?;
    info!("[shrine] {name} entre dans {} (zone {zone})", def.name);
    if let Some(p) = st.players.get(&sid) {
        p.send_raw(state_packet(&def, st.shrines.battles.get(&id)));
    }
    Ok(())
}

/// Boucle des Shrines (appelée par la boucle de combat, une fois par seconde au plus) : horaires, puis chaque combat.
pub async fn step(ctx: &WorldCtx, st: &mut WorldState, now: Instant) -> anyhow::Result<()> {
    if ctx.shrines.list.is_empty() || st.shrines.next_step.is_some_and(|t| now < t) {
        return Ok(());
    }
    st.shrines.next_step = Some(now + STEP);
    if ctx.config.world.shrine_enabled {
        schedule(ctx, st, now);
    }
    let ids: Vec<u8> = st.shrines.battles.keys().copied().collect();
    for id in ids {
        advance(ctx, st, id, now).await?;
    }
    Ok(())
}

/// Crée les combats dont l'ouverture approche (10 minutes avant), ou est en cours au démarrage du serveur.
fn schedule(ctx: &WorldCtx, st: &mut WorldState, now: Instant) {
    let local = local_now(ctx);
    for def in &ctx.shrines.list {
        if st.shrines.battles.contains_key(&def.id) {
            continue;
        }
        let Some((day, delta)) = schedule_delta(def, local) else { continue };
        if st.shrines.done.get(&def.id) == Some(&day) {
            continue;
        }
        st.shrines.done.insert(def.id, day);
        let opens_at = if delta >= 0 { now + Duration::from_secs(delta as u64) } else { now.checked_sub(Duration::from_secs((-delta) as u64)).unwrap_or(now) };
        let mut b = Battle::new(def.id, opens_at, def.duration);
        // Annonces déjà passées de plus de 30 s (serveur démarré tard) : sautées.
        for (bit, minutes) in [(0, 10), (1, 5), (2, 1)] {
            if delta < minutes * 60 - 30 {
                b.announced |= 1 << bit;
            }
        }
        info!("[shrine] {} : ouverture dans {delta} s", def.name);
        st.shrines.battles.insert(def.id, b);
    }
}

/// Fait avancer un combat d'un tour.
async fn advance(ctx: &WorldCtx, st: &mut WorldState, id: u8, now: Instant) -> anyhow::Result<()> {
    let Some(def) = ctx.shrines.get(id).cloned() else {
        st.shrines.battles.remove(&id);
        return Ok(());
    };
    let Some(b) = st.shrines.battles.get(&id).cloned() else { return Ok(()) };
    let index = def.index();
    match b.state {
        state::CLOSED => {
            let left = b.opens_at.saturating_duration_since(now).as_secs() as i64;
            for (bit, minutes, m) in [(0u8, 10i64, msg::IN_10_MIN), (1, 5, msg::IN_5_MIN), (2, 1, msg::IN_1_MIN)] {
                if b.announced & (1 << bit) != 0 || left > minutes * 60 {
                    continue;
                }
                st.shrines.battles.get_mut(&id).unwrap().announced |= 1 << bit;
                send_all(st, &message_packet(id, m, 0, ""));
                if minutes == 5 {
                    send_all(st, &message_packet(id, msg::MOVE_ICON, 0, ""));
                }
                // Proposition d'entrer aux joueurs du niveau voulu.
                let invite = opens_in_packet(id, minutes);
                for pl in st.players.values().filter(|pl| pl.in_game && def.level_ok(pl.factors.status[7])) {
                    pl.send_raw(invite.clone());
                }
            }
            if now >= b.opens_at {
                st.shrines.battles.get_mut(&id).unwrap().state = state::OPEN;
                info!("[shrine] {} ouvert", def.name);
                broadcast_state(ctx, st, id);
                send_all(st, &message_packet(id, msg::STARTED, 0, ""));
                send_to(st, &present(ctx, st, index), &message_packet(id, msg::DOOR_OPENED, 0, ""));
            }
        }
        state::OPEN | state::CARVING | state::OCCUPIED => {
            if now >= b.ends_at {
                finish(ctx, st, id, now);
                return Ok(());
            }
            let left = b.ends_at.saturating_duration_since(now).as_secs() as i64;
            for (bit, minutes, m) in [(3u8, 10i64, msg::ENDS_10_MIN), (4, 5, msg::ENDS_5_MIN)] {
                if b.announced & (1 << bit) == 0 && left <= minutes * 60 && def.duration.as_secs() as i64 > minutes * 60 {
                    st.shrines.battles.get_mut(&id).unwrap().announced |= 1 << bit;
                    send_to(st, &present(ctx, st, index), &message_packet(id, m, 0, ""));
                }
            }
            match b.state {
                state::OPEN => try_carve(ctx, st, &def, now),
                state::CARVING => carving(ctx, st, &def, now),
                _ => occupation(ctx, st, &def, now).await?,
            }
        }
        _ => {
            if b.close_at.is_some_and(|t| now >= t) {
                close(ctx, st, &def).await?;
            }
        }
    }
    Ok(())
}

/// Le joueur peut graver (ou garder) : en jeu, vivant, immobile pour la gravure.
fn carver_sid(st: &WorldState, cid: u32) -> Option<u32> {
    st.by_cid.get(&cid).copied().filter(|sid| st.players.get(sid).is_some_and(|p| p.in_game && !p.dead))
}

/// État 1 : un joueur immobile dans la région centrale commence à graver.
fn try_carve(ctx: &WorldCtx, st: &mut WorldState, def: &ShrineDef, now: Instant) {
    let Some(sid) = st.sids().into_iter().find(|sid| {
        let p = &st.players[sid];
        p.in_game && !p.dead && p.dest.is_none() && region_of(ctx, st, *sid) == Some(def.region)
    }) else {
        return;
    };
    let p = &st.players[&sid];
    let carver = Carver { cid: p.ch.id, name: p.ch.name.clone(), race: p.factors.ftype[0] };
    let id = def.id;
    {
        let b = st.shrines.battles.get_mut(&id).unwrap();
        b.state = state::CARVING;
        b.carve_from = Some(now);
        b.gauge = 0;
        b.carver = Some(carver.clone());
    }
    info!("[shrine] {} grave le sceau de {}", carver.name, def.name);
    broadcast_state(ctx, st, id);
    let inside = present(ctx, st, def.index());
    send_to(st, &inside, &message_packet(id, msg::CARVING, carver.race as i64, &carver.name));
    send_to(st, &[sid], &message_packet(id, msg::STAY_INSIDE, 0, ""));
    let (allies, foes): (Vec<u32>, Vec<u32>) = inside.iter().copied().filter(|s| *s != sid).partition(|s| race_of(st, *s) == carver.race);
    send_to(st, &allies, &message_packet(id, msg::PROTECT, carver.race as i64, &carver.name));
    send_to(st, &foes, &message_packet(id, msg::KILL, carver.race as i64, &carver.name));
}

/// Retour à l'état 1 (gravure ou occupation perdue).
fn reopen(ctx: &WorldCtx, st: &mut WorldState, def: &ShrineDef, message: i64) {
    let id = def.id;
    let Some(b) = st.shrines.battles.get_mut(&id) else { return };
    let carver = b.carver.take();
    let was_carving = b.state == state::CARVING;
    b.state = state::OPEN;
    b.carve_from = None;
    b.occupy_until = None;
    b.gauge = 0;
    let Some(c) = carver else { return };
    info!("[shrine] {} perd {} (message {message})", c.name, def.name);
    let inside = present(ctx, st, def.index());
    send_to(st, &inside, &message_packet(id, message, c.race as i64, &c.name));
    if was_carving {
        send_to(st, &inside, &gauge_packet(id, 0));
        if let Some(sid) = st.by_cid.get(&c.cid).copied() {
            send_to(st, &[sid], &player_state_packet(id, c.cid, 0, 2, 0));
        }
    }
    broadcast_state(ctx, st, id);
}

/// État 2 : gravure en cours (jauge), annulée si le graveur meurt, bouge ou quitte le centre.
fn carving(ctx: &WorldCtx, st: &mut WorldState, def: &ShrineDef, now: Instant) {
    let id = def.id;
    let b = &st.shrines.battles[&id];
    let (Some(c), Some(from)) = (b.carver.clone(), b.carve_from) else { return reopen(ctx, st, def, msg::CARVE_FAILED) };
    let ok = carver_sid(st, c.cid).is_some_and(|sid| st.players[&sid].dest.is_none() && region_of(ctx, st, sid) == Some(def.region));
    if !ok {
        return reopen(ctx, st, def, msg::CARVE_FAILED);
    }
    let done = now.saturating_duration_since(from).as_secs_f64() / def.carve.as_secs_f64().max(0.001);
    let pct = ((done * 100.0) as i64).clamp(1, 100);
    let inside = present(ctx, st, def.index());
    if done < 1.0 {
        if pct != b.gauge {
            st.shrines.battles.get_mut(&id).unwrap().gauge = pct;
            send_to(st, &inside, &gauge_packet(id, pct));
        }
        return;
    }
    // Gravure réussie : la race du graveur occupe le Shrine.
    {
        let b = st.shrines.battles.get_mut(&id).unwrap();
        b.state = state::OCCUPIED;
        b.gauge = 0;
        b.occupy_until = Some(now + def.occupy);
    }
    info!("[shrine] {} a gravé le sceau de {} : la race {} l'occupe", c.name, def.name, c.race);
    send_to(st, &inside, &gauge_packet(id, 0));
    broadcast_state(ctx, st, id);
    send_to(st, &inside, &message_packet(id, msg::CARVED, c.race as i64, &c.name));
    send_to(st, &inside, &message_packet(id, msg::TAKING_OVER, c.race as i64, &c.name));
    send_to(st, &inside, &message_packet(id, msg::KEEP_SOUL, def.occupy.as_secs().div_ceil(60) as i64, &c.name));
}

/// État 4 : occupation ; perdue si l'occupant meurt ou quitte le Shrine ; tenue jusqu'au bout, victoire de sa race.
async fn occupation(ctx: &WorldCtx, st: &mut WorldState, def: &ShrineDef, now: Instant) -> anyhow::Result<()> {
    let b = &st.shrines.battles[&def.id];
    let (Some(c), Some(until)) = (b.carver.clone(), b.occupy_until) else {
        reopen(ctx, st, def, msg::OCCUPY_FAILED);
        return Ok(());
    };
    let inside = carver_sid(st, c.cid).is_some_and(|sid| region_of(ctx, st, sid).and_then(index_of) == Some(def.index()));
    if !inside {
        reopen(ctx, st, def, msg::OCCUPY_FAILED);
        return Ok(());
    }
    if now >= until {
        victory(ctx, st, def, &c, now).await?;
    }
    Ok(())
}

/// Victoire de la race de l'occupant : annonces, récompenses et scores des présents.
async fn victory(ctx: &WorldCtx, st: &mut WorldState, def: &ShrineDef, c: &Carver, now: Instant) -> anyhow::Result<()> {
    let id = def.id;
    {
        let b = st.shrines.battles.get_mut(&id).unwrap();
        b.state = state::ENDED;
        b.winner = Some(c.race);
        b.close_at = Some(now + RETURN_DELAY);
    }
    info!("[shrine] la race {} prend {} ({})", c.race, def.name, c.name);
    broadcast_state(ctx, st, id);
    send_all(st, &message_packet(id, msg::WON, c.race as i64, &c.name));
    send_all(st, &message_packet(id, msg::RACE_WON, c.race as i64, &c.name));
    let inside = present(ctx, st, def.index());
    for sid in &inside {
        let Some(p) = st.players.get(sid) else { continue };
        let (cid, race) = (p.ch.id, p.factors.ftype[0]);
        let mut rewards: Vec<&Reward> = ctx.shrines.common.iter().collect();
        if race == c.race {
            rewards.extend(&ctx.shrines.win_race);
        }
        if cid == c.cid {
            rewards.extend(&ctx.shrines.carving);
        }
        give(ctx, st, *sid, id, &rewards).await?;
        let won = race == c.race;
        let sql = if won {
            "INSERT INTO shrine_score (character_id, wins) VALUES (?, 1) ON DUPLICATE KEY UPDATE wins = wins + 1"
        } else {
            "INSERT INTO shrine_score (character_id, losses) VALUES (?, 1) ON DUPLICATE KEY UPDATE losses = losses + 1"
        };
        ctx.db.characters.exec(sql, (cid,)).await?;
    }
    let races: BTreeSet<i32> = inside.iter().map(|s| race_of(st, *s)).filter(|r| *r > 0).collect();
    for race in races {
        let sql = if race == c.race {
            "INSERT INTO shrine_race_score (race, wins) VALUES (?, 1) ON DUPLICATE KEY UPDATE wins = wins + 1"
        } else {
            "INSERT INTO shrine_race_score (race, losses) VALUES (?, 1) ON DUPLICATE KEY UPDATE losses = losses + 1"
        };
        ctx.db.characters.exec(sql, (race,)).await?;
    }
    for sid in &inside {
        send_to(st, &[*sid], &message_packet(id, msg::REWARDED, 0, ""));
        send_scores(ctx, st, *sid).await?;
    }
    Ok(())
}

/// Récompenses d'un joueur : objets, charisme ; messages 18 et 19.
async fn give(ctx: &WorldCtx, st: &mut WorldState, sid: u32, id: u8, rewards: &[&Reward]) -> anyhow::Result<()> {
    let Some(name) = st.players.get(&sid).map(|p| p.ch.name.clone()) else { return Ok(()) };
    let (mut coins, mut charisma) = (0i64, 0u32);
    for r in rewards {
        if r.item_tid != 0 && r.count > 0 {
            match super::create_item(ctx, st, &name, r.item_tid, r.count, false).await? {
                Ok(_) => coins += r.count,
                Err(why) => warn!("[shrine] récompense {} × {} pour {name} : {why}", r.count, r.item_tid),
            }
        }
        charisma += r.charisma;
    }
    let gained = if charisma > 0 { super::skills::gain_charisma(ctx, st, sid, charisma, CharismaSource::Shrine).await? } else { 0 };
    info!("[shrine] {name} reçoit {coins} pièce(s) et {gained} point(s) de charisme");
    if gained > 0 {
        send_to(st, &[sid], &message_packet(id, msg::CHARISMA, gained as i64, &name));
    }
    if coins > 0 {
        send_to(st, &[sid], &message_packet(id, msg::COINS, coins, &name));
    }
    Ok(())
}

/// Fin du temps sans vainqueur (ou fermeture par un MJ).
fn finish(ctx: &WorldCtx, st: &mut WorldState, id: u8, now: Instant) {
    let Some(def) = ctx.shrines.get(id) else { return };
    let inside = present(ctx, st, def.index());
    let was_carving = {
        let b = st.shrines.battles.get_mut(&id).unwrap();
        let carving = b.state == state::CARVING;
        b.state = state::ENDED;
        b.carver = None;
        b.close_at = Some(now + RETURN_DELAY);
        carving
    };
    info!("[shrine] {} fermé sans vainqueur", def.name);
    if was_carving {
        send_to(st, &inside, &gauge_packet(id, 0));
    }
    broadcast_state(ctx, st, id);
    send_to(st, &inside, &message_packet(id, msg::GATE_CLOSED, 0, ""));
    send_all(st, &message_packet(id, msg::ENDED, 0, ""));
}

/// Après la fin : les présents rentrent en ville, le Shrine repasse à l'état 0.
async fn close(ctx: &WorldCtx, st: &mut WorldState, def: &ShrineDef) -> anyhow::Result<()> {
    let id = def.id;
    let inside = present(ctx, st, def.index());
    send_to(st, &inside, &message_packet(id, msg::TO_TOWN, 0, ""));
    for sid in inside {
        let Some(p) = st.players.get_mut(&sid) else { continue };
        // Un mort est relevé avant de partir, comme une résurrection au village.
        if p.dead {
            super::combat::resurrect(ctx, st, sid).await?;
            continue;
        }
        let (x, y, z) = super::travel::home_village(ctx, &st.players[&sid]);
        super::teleport(ctx, st, sid, x, y, z).await?;
    }
    st.shrines.battles.remove(&id);
    send_all(st, &message_packet(id, msg::OVER, 0, ""));
    broadcast_state(ctx, st, id);
    info!("[shrine] {} : combat terminé", def.name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use archcore_net::{decode_packet, parse_header};

    pub fn lightning() -> ShrineDef {
        ShrineDef {
            id: 1,
            name: "Shrine of Lightning".into(),
            region: 236,
            days: 127,
            open_hour: 20,
            open_minute: 0,
            duration: Duration::from_secs(1800),
            carve: Duration::from_secs(60),
            occupy: Duration::from_secs(300),
            level_min: 50,
            level_max: 0,
            max_per_race: 50,
        }
    }

    #[test]
    fn regions_des_shrines() {
        assert_eq!([236, 243, 244, 247, 248, 275, 276, 283].map(index_of), [Some(0), Some(7), Some(0), Some(0), Some(1), Some(7), Some(0), Some(7)]);
        assert_eq!((index_of(235), index_of(284)), (None, None));
        assert_eq!((race_zone(0, 1), race_zone(0, 4), race_zone(2, 3), race_zone(7, 4)), (244, 247, 254, 275));
        assert!(lightning().level_ok(50) && !lightning().level_ok(49));
    }

    #[test]
    fn horaire_d_ouverture() {
        let d = lightning();
        let at = |h, m, s| chrono::NaiveDate::from_ymd_opt(2026, 10, 28).unwrap().and_hms_opt(h, m, s).unwrap();
        assert_eq!(schedule_delta(&d, at(19, 49, 0)), None, "11 minutes avant : trop tôt");
        assert_eq!(schedule_delta(&d, at(19, 50, 0)).map(|x| x.1), Some(600));
        assert_eq!(schedule_delta(&d, at(20, 10, 0)).map(|x| x.1), Some(-600), "en cours");
        assert_eq!(schedule_delta(&d, at(20, 30, 0)), None, "fini");
        // Mercredi seulement exclu (bit 2).
        assert_eq!(schedule_delta(&ShrineDef { days: 127 & !(1 << 2), ..d }, at(19, 55, 0)), None);
    }

    #[test]
    fn paquets_bruts() {
        let d = lightning();
        let mut b = Battle::new(1, Instant::now(), d.duration);
        b.state = state::OCCUPIED;
        b.carver = Some(Carver { cid: 0x1234, name: "Grokk".into(), race: 2 });
        let p = state_packet(&d, Some(&b));
        assert_eq!(p.len(), 0xf1);
        assert_eq!(parse_header(&p).map(|h| (h.ptype, h.size)), Some((0x62, 0xf1)));
        let decoded = decode_packet(&schema::SHRINE_BATTLE, &p).unwrap();
        assert_eq!(decoded.int(0), Some(shrine_op::STATE));
        let u = |o: usize| get_u32(&p, o).unwrap();
        assert_eq!((u(0x18), u(0x20), u(0x20 + 0x80), u(0x20 + 0x84), u(0x20 + 0xb8)), (1, 1, 4, 2, 0x1234));
        assert_eq!(super::super::guild::society::get_text(&p, 0x24, 30), "Shrine of Lightning");
        assert_eq!(super::super::guild::society::get_text(&p, 0x20 + 0x22, 49), "Grokk");
        let cat = ShrineCatalog { list: vec![d.clone(), ShrineDef { id: 2, region: 237, ..d.clone() }], ..Default::default() };
        let list = list_packet(&cat, &Shrines::default());
        assert_eq!(list.len(), 0x981);
        assert_eq!((get_u32(&list, 0x40), get_u32(&list, 0x40 + 0xf0), get_u32(&list, 0x40 + 0x1e0)), (Some(1), Some(2), Some(0)));
        let m = message_packet(1, msg::CARVING, 2, "Grokk");
        assert_eq!((m.len(), get_u32(&m, 0x1c), get_u32(&m, 0x20)), (0x59, Some(9), Some(2)));
        assert_eq!(super::super::guild::society::get_text(&m, 0x24, 0x34), "Grokk");
        assert_eq!((gauge_packet(1, 50).len(), player_state_packet(1, 7, 0, 2, 0).len(), opens_in_packet(1, 5).len(), score_packet(11, 3, 1).len()), (0x21, 0x2d, 0x21, 0x25));
        assert_eq!(get_u32(&score_packet(12, 3, 1), 0x20), Some(1));
    }
}
