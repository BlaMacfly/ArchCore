//! Guerres de siège des châteaux (AgpmSiegeWar 0x49, AgpmEventSiegeWarNPC 0x50, AgpmWarInfo 0x61 : `warinfo.rs`).
//!
//! ```text
//!   entrée en jeu                    ← op 0 complet pour chaque château ; ops 0xE et 0xF (assaillants et défenseurs
//!                                      retenus) dès les candidatures closes ; étape de WarInfo en pleine guerre
//!   commandant (fonction 33) C 0x50/1 ← op 0 du château du PNJ (région du PNJ), 0x50/2 : fenêtre UI_SiegeWarMain
//!   C op 1 (date)                     ← op 2 : 0x0B date fixée, 1 statut ≠ 0, 2 hors du créneau, 3 pas le propriétaire
//!   C op 3/5 (défense, attaque)       ← op 2 : 8/9, 2 hors candidatures, 4 pas le chef, 5 guilde trop petite ou
//!                                      trop jeune, 6 déjà inscrite (ici ou ailleurs), 0x15 liste pleine, 1 pas sur place
//!   C op 4/6 (retrait)                ← op 2 : 0 ; C op 7/8 (page) ← op 9/10 ; C op 0xB ← op 2 : 10 (et op 0xF)
//!   C op 0xC/0xD                      ← op 0xE/0xF
//!   calendrier (1 s)                  statut 0 → 1 (déclaration, sinon date fixée d'office) → 2 (fin des candidatures :
//!                                      assaillants retenus) → 3 (début, `war.rs`) → 6 (fin) → 0
//! ```
//! Règles du client appliquées telles quelles (code partagé d'Archonia, voir la spécification) : seul le propriétaire
//! (chef de la guilde qui tient le château) déclare, dans le créneau [jour de base à 20 h, + 26 h] où le jour de base est
//! le dernier siège + Duration4nextSW jours (reculé d'un jour un dimanche) ; candidatures du chef de guilde sur place
//! (régions du château ou de ses abords), au statut 1, 15 attaquants et 3 défenseurs au plus ; le propriétaire retient 2
//! défenseurs au plus ; une guilde ne figure qu'une fois, tous châteaux confondus (Rensphere à part).
//!
//! Choix d'ArchCore, là où le client ne dit rien (réglables) :
//! * conditions d'une guilde : `Siege.MinGuildMembers` (30) membres et `Siege.MinGuildAgeDays` (14) jours, valeurs du
//!   client par défaut ;
//! * château sans propriétaire au départ (PNJ) ; s'il n'a pas de propriétaire, ou si le propriétaire ne déclare pas
//!   dans les NextSWDeclaring heures qui suivent le dernier siège, le siège est fixé d'office au premier créneau proposé
//!   par la fenêtre (jour de base ou lendemain, 20, 21 ou 22 h : SWBeginTime n'est pas utilisé, comme le client) ;
//! * candidatures ouvertes SelectGuildForSW heures (au plus jusqu'au siège) ; à la clôture, tous les candidats
//!   attaquants sont retenus (15 au plus, par points de guilde) ; sans aucun, le siège est annulé et la date suivante
//!   calculée (jour de base + Duration4nextSW) ;
//! * Rensphere (0, château de l'Archlord) n'a pas de calendrier ici : le module de l'ArchLord (`archlord.rs`) ouvre sa
//!   guerre tous les Duration4nextSW jours, y inscrit les guildes propriétaires d'un autre château et suit ses étapes ;
//! * `Siege.Enabled = 0` coupe tout le calendrier : seules les commandes MJ `/siege` font avancer les sièges.
//! Un siège en cours au redémarrage du serveur est clos sans changement de propriétaire.

pub mod assault;
pub mod war;

use super::{WorldCtx, WorldState};
use crate::character::unix_now;
use crate::db::{col, Databases};
use crate::{info, warn};
use archcore_net::schema::{self, mini, siege_npc_op, siege_op, siege_result, war_info_op};
use archcore_net::{build_mini, build_packet, decode_mini, Fields, Packet, Value, Vec3};
use chrono::{Datelike, NaiveDate, NaiveDateTime, TimeZone};
use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

/// Instant « jamais » des objets cachés hors du siège (dix ans).
pub const NEVER: Duration = Duration::from_secs(10 * 365 * 86_400);
/// Rensphere, château de l'Archlord (ligne 0 de SiegeWar.txt).
pub const RENSPHERE: u8 = 0;
pub const MAX_ATTACK_APPLICATIONS: usize = 15;
pub const MAX_DEFENSE_APPLICATIONS: usize = 3;
pub const MAX_DEFENDERS: usize = 2;
/// Guildes d'une page des listes (op 9, 10, 0xE, 0xF).
pub const PAGE_SIZE: usize = 15;
/// Heures proposées par la fenêtre de déclaration (codées dans le client).
pub const SLOT_HOURS: [u32; 3] = [20, 21, 22];
/// Durée du combat sans `Siege.DurationMinutes` ni SWPlayingTime.
pub const DEFAULT_DURATION: Duration = Duration::from_secs(120 * 60);
/// Écart entre FILETIME (1601) et l'époque Unix, en secondes.
const FILETIME_EPOCH: i64 = 11_644_473_600;
/// Cadence du calendrier.
const STEP: Duration = Duration::from_secs(1);
/// Distance au commandant de siège pour ouvrir sa fenêtre.
const NPC_RANGE: f64 = super::npc_events::MAX_NPC_DISTANCE;

/// Statuts d'un château (`château+0x17c` du client).
pub mod status {
    pub const NONE: u8 = 0;
    pub const APPLYING: u8 = 1;
    pub const CLOSED: u8 = 2;
    pub const WAR: u8 = 3;
    pub const THRONE_RELEASED: u8 = 4;
    pub const EYES_OPEN: u8 = 5;
    pub const ENDED: u8 = 6;
    /// Phase à durée moitié, à Rensphere sans résurrection : bataille de l'Archlord (autre module).
    pub const ARCHLORD: u8 = 7;
    pub const LAST: u8 = 8;
    /// Statuts « en guerre » (règles de combat du siège).
    pub fn at_war(s: u8) -> bool {
        (WAR..=LAST).contains(&s)
    }
}

/// Types des objets de siège (`SiegeWarCharType` de CharacterDataTable.txt).
pub mod kind {
    pub const GUARD_TOWER: u8 = 1;
    pub const INNER_GATE: u8 = 2;
    pub const OUTER_GATE: u8 = 3;
    pub const LIFE_TOWER: u8 = 4;
    pub const REVIVAL_TOWER: u8 = 5;
    pub const THRONE: u8 = 6;
    pub const EYE: u8 = 7;
    pub const PROTECTOR: u8 = 8;
    pub const CAMP: u8 = 9;
    pub const CATAPULT: u8 = 10;
    // 11 : gardiens (Gate Keeper Titan, Looter Gargantua…), absents des apparitions : non pris en charge.
}

/// Camp d'une guilde dans un siège.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Attack,
    Defense,
}

/// Une ligne de `siege_war` (SiegeWar.txt).
#[derive(Clone, Debug, PartialEq)]
pub struct CastleDef {
    pub id: u8,
    pub name: String,
    pub castle_region: u16,
    pub outskirts_region: u16,
    pub playing: Duration,
    pub eye_open: Duration,
    pub stamping: Duration,
    pub declare_hours: i64,
    pub select_hours: i64,
    pub interval_days: i64,
    pub begin_hour: u32,
    pub stamp_skill: (u32, u32),
    pub disabled: bool,
}

impl CastleDef {
    /// La région est celle du château ou de ses abords.
    pub fn has_region(&self, region: u16) -> bool {
        region != 0 && (region == self.castle_region || region == self.outskirts_region)
    }
}

/// Objet d'assaut réparable (`siege_war_object`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AssaultDef {
    pub repair_count: i64,
    pub repair_cost: i64,
    pub repair_minutes: i64,
    pub items: Vec<(u32, i64)>,
}

/// Point de résurrection d'un siège (`siege_binding`).
#[derive(Clone, Debug, PartialEq)]
pub struct BindingPoint {
    pub region: u16,
    pub kind: u8,
    pub pos: Vec3,
}

/// Données des sièges, chargées au démarrage.
#[derive(Default)]
pub struct SiegeCatalog {
    pub castles: Vec<CastleDef>,
    pub assault: HashMap<u8, AssaultDef>,
    pub bindings: Vec<BindingPoint>,
    /// Type d'objet de siège par TID (`SiegeWarCharType`).
    pub kinds: HashMap<u32, u8>,
}

impl SiegeCatalog {
    pub async fn load(db: &Databases) -> Self {
        let mut c = SiegeCatalog::default();
        let sql = "SELECT id, name, castle_region, outskirts_region, playing_ms, eye_open_ms, stamping_ms, declare_hours, select_hours, interval_days, begin_hour, stamp_skill_tid, stamp_skill_level, disabled FROM siege_war ORDER BY id";
        match db.world.rows(sql, ()).await {
            Ok(rows) => {
                for r in &rows {
                    let ms = |k: &str| Duration::from_millis(col::<u64>(r, k));
                    c.castles.push(CastleDef {
                        id: col::<u32>(r, "id") as u8,
                        name: col(r, "name"),
                        castle_region: col::<u32>(r, "castle_region") as u16,
                        outskirts_region: col::<u32>(r, "outskirts_region") as u16,
                        playing: ms("playing_ms"),
                        eye_open: ms("eye_open_ms"),
                        stamping: ms("stamping_ms"),
                        declare_hours: col(r, "declare_hours"),
                        select_hours: col(r, "select_hours"),
                        interval_days: col::<i64>(r, "interval_days").max(1),
                        begin_hour: col::<u32>(r, "begin_hour").min(23),
                        stamp_skill: (col(r, "stamp_skill_tid"), col(r, "stamp_skill_level")),
                        disabled: col::<u32>(r, "disabled") != 0,
                    });
                }
            }
            Err(e) => warn!("[siège] table siege_war non lue ({e:#}) : lancer `archcore db-init` puis `archcore import`"),
        }
        if let Ok(rows) = db.world.rows("SELECT type, repair_count, repair_cost, repair_minutes, items FROM siege_war_object", ()).await {
            for r in &rows {
                let items: Vec<[i64; 2]> = serde_json::from_str(&col::<String>(r, "items")).unwrap_or_default();
                let def = AssaultDef {
                    repair_count: col(r, "repair_count"),
                    repair_cost: col(r, "repair_cost"),
                    repair_minutes: col(r, "repair_minutes"),
                    items: items.iter().map(|[t, n]| (*t as u32, *n)).collect(),
                };
                c.assault.insert(col::<u32>(r, "type") as u8, def);
            }
        }
        if let Ok(rows) = db.world.rows("SELECT region_id, binding_type, pos_x, pos_y, pos_z FROM siege_binding", ()).await {
            c.bindings = rows
                .iter()
                .map(|r| BindingPoint {
                    region: col::<u32>(r, "region_id") as u16,
                    kind: col::<u32>(r, "binding_type") as u8,
                    pos: Vec3::new(col::<f64>(r, "pos_x") as f32, col::<f64>(r, "pos_y") as f32, col::<f64>(r, "pos_z") as f32),
                })
                .collect();
        }
        let sql = "SELECT tid, JSON_VALUE(data, '$.SiegeWarCharType') AS k FROM character_data_table WHERE JSON_VALUE(data, '$.SiegeWarCharType') IS NOT NULL";
        if let Ok(rows) = db.world.rows(sql, ()).await {
            for r in &rows {
                if let Some(k) = col::<Option<String>>(r, "k").and_then(|k| k.trim().parse::<u8>().ok()).filter(|k| *k > 0) {
                    c.kinds.insert(col(r, "tid"), k);
                }
            }
        }
        info!("sièges : {} château(x), {} objet(s) d'assaut, {} point(s) de résurrection, {} modèle(s) d'objet de siège", c.castles.len(), c.assault.len(), c.bindings.len(), c.kinds.len());
        c
    }

    pub fn get(&self, id: u8) -> Option<&CastleDef> {
        self.castles.iter().find(|d| d.id == id)
    }
    pub fn named(&self, name: &str) -> Option<&CastleDef> {
        let key = name.trim().to_lowercase();
        self.castles.iter().find(|d| d.name.to_lowercase() == key || d.id.to_string() == key)
    }
    /// Château dont la région (château ou abords) est `region`.
    pub fn by_region(&self, region: u16) -> Option<&CastleDef> {
        self.castles.iter().find(|d| d.has_region(region))
    }
}

/// Variantes d'un objet détruit : (en destruction, ruine), d'après les noms de CharacterDataTable.txt (« Destroying … »,
/// « … Ruins », « Razed … ») ; pour l'Œil d'Archon, l'Œil éveillé.
pub fn variants(tid: u32) -> Option<(u32, u32)> {
    Some(match tid {
        1268 => (1278, 1279),
        1265 => (1283, 1284),
        1269 => (1287, 1288),
        1305 => (1397, 1397),
        367 => (369, 368),
        1317 => (1318, 1318),
        1264 => (1281, 1281),
        1263 => (1280, 1280),
        1258 => (1290, 1290),
        1266 => (1364, 1364),
        _ => return None,
    })
}

/// Candidature d'une guilde.
#[derive(Clone, Debug, PartialEq)]
pub struct Application {
    pub guild: u32,
    pub side: Side,
    pub selected: bool,
}

/// Carving en cours.
#[derive(Clone, Debug, PartialEq)]
pub struct Carving {
    pub cid: u32,
    pub guild: u32,
    pub pos: Vec3,
    pub until: Instant,
}

/// Combat en cours (statuts 3 à 6).
#[derive(Clone, Debug)]
pub struct War {
    pub ends_at: Instant,
    /// Dernière étape de WarInfo montrée.
    pub step: u8,
    /// Yeux d'Archon ouverts (CID, instant).
    pub eyes: Vec<(u32, Instant)>,
    pub carving: Option<Carving>,
    /// Prochain compte à rebours (op 0x1D) et prochain compteur de WarInfo.
    pub next_time: Instant,
}

/// État d'un château.
#[derive(Clone, Debug, Default)]
pub struct Castle {
    pub id: u8,
    pub owner: Option<u32>,
    pub carve: Option<u32>,
    pub status: u8,
    /// Secondes Unix (0 : aucune).
    pub last_siege: i64,
    pub next_siege: i64,
    pub apply_until: i64,
    pub declare_until: i64,
    pub applications: Vec<Application>,
    pub war: Option<War>,
    /// Fin de la phase 6 (retour au statut 0).
    pub close_at: Option<Instant>,
}

impl Castle {
    pub fn new(id: u8) -> Self {
        Castle { id, ..Default::default() }
    }
    /// Camp d'une guilde : propriétaire et défenseurs retenus d'un côté, assaillants retenus de l'autre.
    pub fn side_of(&self, gid: u32) -> Option<Side> {
        if self.owner == Some(gid) {
            return Some(Side::Defense);
        }
        self.applications.iter().find(|a| a.guild == gid && a.selected).map(|a| a.side)
    }
    pub fn applicants(&self, side: Side) -> Vec<u32> {
        self.applications.iter().filter(|a| a.side == side).map(|a| a.guild).collect()
    }
    pub fn selected(&self, side: Side) -> Vec<u32> {
        self.applications.iter().filter(|a| a.side == side && a.selected).map(|a| a.guild).collect()
    }
    /// Guildes défenseuses : le propriétaire d'abord, puis les guildes retenues.
    pub fn defenders(&self) -> Vec<u32> {
        self.owner.into_iter().chain(self.selected(Side::Defense)).collect()
    }
    /// Toutes les guildes du siège.
    pub fn participants(&self) -> Vec<u32> {
        let mut v = self.defenders();
        v.extend(self.selected(Side::Attack));
        v
    }
}

/// Objet ou garde d'un siège (monstre caché hors du combat).
#[derive(Clone, Debug, Default)]
pub struct SiegeObject {
    pub castle: u8,
    /// Type d'objet (0 : garde).
    pub kind: u8,
    pub base_tid: u32,
    pub guard: bool,
    pub destroyed: bool,
    /// Passage à la ruine (après la variante « en destruction »).
    pub ruin_at: Option<Instant>,
    /// Objets d'inventaire chargés (IID, TID), camp activé, réparations restantes, réparation en cours (début, fin,
    /// réparateur) : catapultes et camps (`assault.rs`).
    pub loaded: Vec<(u32, u32)>,
    pub active: bool,
    pub repairs_left: i64,
    pub repair: Option<(Instant, Instant, String)>,
}

#[derive(Default)]
pub struct Sieges {
    pub castles: BTreeMap<u8, Castle>,
    pub objects: BTreeMap<u32, SiegeObject>,
    next_step: Option<Instant>,
}

impl Sieges {
    /// État enregistré des châteaux, et objets de siège rattachés à leur château par la région de leur point.
    pub async fn load(db: &Databases, cat: &SiegeCatalog, map: &super::map::WorldMap, monsters: &super::monsters::MonsterRegistry) -> Self {
        let mut s = Sieges::default();
        for d in &cat.castles {
            s.castles.insert(d.id, Castle::new(d.id));
        }
        let sql = "SELECT id, owner_guild_id, carve_guild_id, status, UNIX_TIMESTAMP(last_siege_at) AS last_at, UNIX_TIMESTAMP(next_siege_at) AS next_at, \
                   UNIX_TIMESTAMP(apply_until) AS apply_at, UNIX_TIMESTAMP(declare_until) AS declare_at FROM castle";
        match db.characters.rows(sql, ()).await {
            Ok(rows) => {
                for r in &rows {
                    let Some(c) = s.castles.get_mut(&(col::<u32>(r, "id") as u8)) else { continue };
                    let t = |k: &str| col::<Option<i64>>(r, k).unwrap_or(0);
                    c.owner = col(r, "owner_guild_id");
                    c.carve = col(r, "carve_guild_id");
                    c.status = col::<u32>(r, "status") as u8;
                    (c.last_siege, c.next_siege, c.apply_until, c.declare_until) = (t("last_at"), t("next_at"), t("apply_at"), t("declare_at"));
                    // Siège interrompu par l'arrêt du serveur : clos sans changement de propriétaire.
                    if c.status > status::CLOSED {
                        warn!("[siège] château {} : siège interrompu au statut {}, clos sans changement", c.id, c.status);
                        c.status = status::NONE;
                        c.last_siege = c.next_siege.max(c.last_siege);
                        c.next_siege = 0;
                        c.applications.clear();
                    }
                }
            }
            Err(e) => warn!("[siège] châteaux non lus ({e:#}) : lancer `archcore db-init`"),
        }
        if let Ok(rows) = db.characters.rows("SELECT castle_id, guild_id, side, selected FROM castle_application ORDER BY applied_at, guild_id", ()).await {
            for r in &rows {
                let Some(c) = s.castles.get_mut(&(col::<u32>(r, "castle_id") as u8)).filter(|c| c.status == status::APPLYING || c.status == status::CLOSED) else { continue };
                let side = if col::<u32>(r, "side") == 2 { Side::Defense } else { Side::Attack };
                c.applications.push(Application { guild: col(r, "guild_id"), side, selected: col::<u32>(r, "selected") != 0 });
            }
        }
        for m in &monsters.monsters {
            let Some(&what) = monsters.siege_spawns.get(&m.spawn_id) else { continue };
            let Some(def) = map.region(m.home.x as f64, m.home.z as f64).and_then(|r| cat.by_region(r)) else { continue };
            let guard = what == 2;
            let kind = if guard { 0 } else { cat.kinds.get(&m.tid).copied().unwrap_or(0) };
            let repairs = cat.assault.get(&kind).map(|a| a.repair_count).unwrap_or(0);
            s.objects.insert(m.cid, SiegeObject { castle: def.id, kind, base_tid: m.tid, guard, repairs_left: repairs, ..Default::default() });
        }
        info!("sièges : {} objet(s) et garde(s) rattachés à leur château", s.objects.len());
        s
    }

    /// Objets (pas les gardes) d'un château.
    pub fn objects_of(&self, castle: u8) -> impl Iterator<Item = (&u32, &SiegeObject)> {
        self.objects.iter().filter(move |(_, o)| o.castle == castle && !o.guard)
    }
}

// ---------------------------------------------------------------------------------------------
// Heure locale (fuseau de `World.TimeZone`) et calendrier
// ---------------------------------------------------------------------------------------------

/// Heure locale d'un instant Unix.
pub fn to_local(tz: crate::config::TimeZone, unix: i64) -> NaiveDateTime {
    let utc = chrono::DateTime::from_timestamp(unix, 0).unwrap_or_default();
    match tz {
        crate::config::TimeZone::Utc => utc.naive_utc(),
        crate::config::TimeZone::Local => utc.with_timezone(&chrono::Local).naive_local(),
    }
}

/// Instant Unix d'une heure locale.
pub fn from_local(tz: crate::config::TimeZone, t: NaiveDateTime) -> i64 {
    match tz {
        crate::config::TimeZone::Utc => t.and_utc().timestamp(),
        crate::config::TimeZone::Local => chrono::Local.from_local_datetime(&t).earliest().map(|d| d.timestamp()).unwrap_or_else(|| t.and_utc().timestamp()),
    }
}

/// FILETIME en heure locale (dates des paquets) ; 0 pour « aucune ».
pub fn filetime(tz: crate::config::TimeZone, unix: i64) -> u64 {
    if unix <= 0 {
        return 0;
    }
    ((to_local(tz, unix).and_utc().timestamp() + FILETIME_EPOCH).max(0) as u64) * 10_000_000
}

/// Instant Unix d'un FILETIME en heure locale.
pub fn from_filetime(tz: crate::config::TimeZone, ft: u64) -> Option<i64> {
    let secs = (ft / 10_000_000) as i64 - FILETIME_EPOCH;
    let local = chrono::DateTime::from_timestamp(secs, 0)?.naive_utc();
    Some(from_local(tz, local))
}

/// Jour de base du prochain siège : dernier siège + `interval` jours, reculé d'un jour un dimanche ; sans siège passé, le
/// samedi de la semaine de « aujourd'hui + interval − 7 jours ».
pub fn base_day(last: Option<NaiveDate>, today: NaiveDate, interval: i64) -> NaiveDate {
    match last {
        Some(d) => {
            let b = d + chrono::Duration::days(interval);
            if b.weekday() == chrono::Weekday::Sun { b - chrono::Duration::days(1) } else { b }
        }
        None => {
            let t = today + chrono::Duration::days(interval - 7);
            t - chrono::Duration::days(t.weekday().num_days_from_monday() as i64) + chrono::Duration::days(5)
        }
    }
}

/// Créneaux proposés par la fenêtre : jour de base et lendemain, 20, 21 et 22 h.
pub fn slots(base: NaiveDate) -> Vec<NaiveDateTime> {
    [base, base + chrono::Duration::days(1)].iter().flat_map(|d| SLOT_HOURS.iter().filter_map(move |h| d.and_hms_opt(*h, 0, 0))).collect()
}

/// Date proposée acceptable (règle du client) : dans le futur ; après un siège, entre le jour de base à 20 h et 26 h plus
/// tard.
pub fn declare_ok(last: Option<NaiveDate>, d: NaiveDateTime, now: NaiveDateTime, interval: i64) -> bool {
    if d <= now {
        return false;
    }
    match last {
        None => true,
        Some(_) => {
            let Some(b) = base_day(last, now.date(), interval).and_hms_opt(20, 0, 0) else { return false };
            d >= b && d <= b + chrono::Duration::hours(26)
        }
    }
}

/// Premier créneau proposé encore à venir (plus de 10 minutes) ; à défaut, ceux des périodes suivantes.
pub fn first_slot(last: Option<NaiveDate>, now: NaiveDateTime, interval: i64) -> NaiveDateTime {
    let mut base = base_day(last, now.date(), interval);
    for _ in 0..60 {
        if let Some(s) = slots(base).into_iter().find(|s| *s > now + chrono::Duration::minutes(10)) {
            return s;
        }
        base += chrono::Duration::days(interval.max(1));
    }
    now + chrono::Duration::days(interval.max(1))
}

/// Objet d'assaut d'un type (vide s'il n'est pas dans SiegeWarObject.txt).
pub fn assault_def(ctx: &WorldCtx, k: u8) -> AssaultDef {
    ctx.sieges.assault.get(&k).cloned().unwrap_or_default()
}

/// Durée du combat : `Siege.DurationMinutes`, sinon SWPlayingTime, sinon 120 minutes.
pub fn duration(ctx: &WorldCtx, def: &CastleDef) -> Duration {
    match ctx.config.world.siege_duration_minutes {
        m if m > 0 => Duration::from_secs(m as u64 * 60),
        _ if !def.playing.is_zero() => def.playing,
        _ => DEFAULT_DURATION,
    }
}

// ---------------------------------------------------------------------------------------------
// Paquets
// ---------------------------------------------------------------------------------------------

/// Bloc mémoire d'un texte, zéro final compris (un texte vide reste un zéro : le client efface l'ancienne valeur).
pub fn mb(s: &str) -> Vec<u8> {
    let mut b = archcore_net::text::encode(s);
    b.push(0);
    b
}

fn base(op: i64) -> Fields {
    Fields::new(&schema::SIEGE_WAR).with(0, op)
}

fn build(f: Fields) -> Vec<u8> {
    build_packet(&schema::SIEGE_WAR, &f)
}

/// Mini château réduit à son nom.
pub fn castle_name_mini(name: &str) -> Vec<u8> {
    let m = &mini::SIEGE_CASTLE;
    build_mini(m, &Fields::new(m).with(0, mb(name)))
}

fn guild_name(st: &WorldState, gid: Option<u32>) -> String {
    gid.and_then(|g| st.guilds.by_id.get(&g)).map(|g| g.name.clone()).unwrap_or_default()
}

fn master_name(st: &WorldState, gid: Option<u32>) -> String {
    gid.and_then(|g| st.guilds.by_id.get(&g)).and_then(|g| g.master()).map(|m| m.name.clone()).unwrap_or_default()
}

/// Résurrections disponibles : (assaillants : un camp activé debout, défenseurs : une tour de résurrection debout ;
/// Rensphere : les deux, hors statut 7).
pub fn revive_flags(st: &WorldState, id: u8) -> (bool, bool) {
    let Some(c) = st.siege.castles.get(&id).filter(|c| status::at_war(c.status)) else { return (false, false) };
    if id == RENSPHERE {
        return (c.status != status::ARCHLORD, c.status != status::ARCHLORD);
    }
    let up = |k: u8, active: bool| st.siege.objects_of(id).any(|(_, o)| o.kind == k && !o.destroyed && (!active || o.active));
    (up(kind::CAMP, true), up(kind::REVIVAL_TOWER, false))
}

/// Mini château complet.
pub fn castle_mini(ctx: &WorldCtx, st: &WorldState, def: &CastleDef) -> Vec<u8> {
    let c = st.siege.castles.get(&def.id).cloned().unwrap_or_else(|| Castle::new(def.id));
    let tz = ctx.config.world.time_zone;
    let members = c.owner.and_then(|g| st.guilds.by_id.get(&g)).map(|g| g.active_members()).unwrap_or(0).min(255);
    let (attack, defense) = revive_flags(st, def.id);
    let m = &mini::SIEGE_CASTLE;
    build_mini(
        m,
        &Fields::new(m)
            .with(0, mb(&def.name))
            .with(1, mb(&master_name(st, c.owner)))
            .with(2, mb(&guild_name(st, c.owner)))
            .with(3, members)
            .with(4, filetime(tz, c.last_siege))
            .with(5, filetime(tz, c.next_siege))
            .with(6, c.status)
            .with(7, attack as i64)
            .with(8, defense as i64)
            .with(9, 0)
            .with(10, mb(&guild_name(st, c.carve))),
    )
}

/// Op 0 complet.
pub fn castle_packet(ctx: &WorldCtx, st: &WorldState, def: &CastleDef) -> Vec<u8> {
    build(base(siege_op::CASTLE).with(2, castle_mini(ctx, st, def)))
}

/// Op 2 : code de résultat.
pub fn result_packet(castle: &str, result: i64, cid: u32) -> Vec<u8> {
    build(base(siege_op::RESULT).with(1, result).with(2, castle_name_mini(castle)).with(3, cid))
}

/// Entrées d'une page : (nom, chef, points).
pub fn guild_page_mini(page: usize, total: usize, rows: &[(String, String, i64)]) -> Vec<u8> {
    let entry = &mini::SIEGE_GUILD_ENTRY;
    let entries: Vec<Value> =
        rows.iter().take(PAGE_SIZE).map(|(g, m, p)| Value::Bytes(build_mini(entry, &Fields::new(entry).with(0, mb(g)).with(1, mb(m)).with(2, (*p).max(0))))).collect();
    let pm = &mini::SIEGE_GUILD_PAGE;
    let f = Fields::new(pm).with(0, page.max(1) as i64).with(1, (total / PAGE_SIZE + 1) as i64);
    build_mini(pm, &if entries.is_empty() { f } else { f.with(2, entries) })
}

/// Lignes (nom, chef, points) des guildes, triées par points.
fn guild_rows(st: &WorldState, gids: &[u32]) -> Vec<(String, String, i64)> {
    let mut v: Vec<(String, String, i64)> = gids
        .iter()
        .filter_map(|g| st.guilds.by_id.get(g))
        .map(|g| (g.name.clone(), g.master().map(|m| m.name.clone()).unwrap_or_default(), g.record.points))
        .collect();
    v.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
    v
}

/// Ops 9, 10 : page `page` (1 et plus) d'une liste.
pub fn page_packet(op: i64, castle: &str, cid: u32, rows: &[(String, String, i64)], page: usize) -> Vec<u8> {
    let page = page.max(1);
    let slice: Vec<(String, String, i64)> = rows.iter().skip((page - 1) * PAGE_SIZE).take(PAGE_SIZE).cloned().collect();
    build(base(op).with(2, castle_name_mini(castle)).with(3, cid).with(4, guild_page_mini(page, rows.len(), &slice)))
}

/// Ops 0xE (assaillants retenus) et 0xF (défenseurs : le propriétaire d'abord).
pub fn lists_packets(st: &WorldState, def: &CastleDef) -> [Vec<u8>; 2] {
    let c = st.siege.castles.get(&def.id).cloned().unwrap_or_else(|| Castle::new(def.id));
    let attackers = guild_rows(st, &c.selected(Side::Attack));
    // Défenseurs : le propriétaire en tête, sans tri.
    let defenders: Vec<(String, String, i64)> = c
        .defenders()
        .iter()
        .filter_map(|g| st.guilds.by_id.get(g))
        .map(|g| (g.name.clone(), g.master().map(|m| m.name.clone()).unwrap_or_default(), g.record.points))
        .collect();
    [
        build(base(siege_op::ATTACKERS).with(2, castle_name_mini(&def.name)).with(4, guild_page_mini(1, attackers.len(), &attackers))),
        build(base(siege_op::DEFENDERS).with(2, castle_name_mini(&def.name)).with(4, guild_page_mini(1, defenders.len(), &defenders))),
    ]
}

/// Op 0x1C : message de siège.
pub fn status_msg_packet(code: i64, castle: &str, guild: &str, name: &str, object: u8, side: i64) -> Vec<u8> {
    let m = &mini::SIEGE_STATUS_MSG;
    let msg = build_mini(m, &Fields::new(m).with(0, code).with(1, mb(castle)).with(2, mb(guild)).with(3, mb(name)).with(4, object as i64).with(5, 0).with(6, side));
    build(base(siege_op::STATUS_MSG).with(8, msg))
}

/// Op 0x1D : compte à rebours (genre 1 avant le début, 2 avant la fin).
pub fn time_packet(kind: i64, secs: u64) -> Vec<u8> {
    let m = &mini::SIEGE_TIME;
    build(base(siege_op::TIME).with(10, build_mini(m, &Fields::new(m).with(0, kind).with(1, secs.min(u16::MAX as u64) as i64))))
}

pub fn send_all(st: &WorldState, plain: &[u8]) {
    for p in st.players.values().filter(|p| p.in_game) {
        p.send_raw(plain.to_vec());
    }
}

pub fn send_to(st: &WorldState, sids: &[u32], plain: &[u8]) {
    for sid in sids {
        if let Some(p) = st.players.get(sid) {
            p.send_raw(plain.to_vec());
        }
    }
}

/// Op 0 complet d'un château à tous.
pub fn broadcast_castle(ctx: &WorldCtx, st: &WorldState, id: u8) {
    if let Some(def) = ctx.sieges.get(id) {
        send_all(st, &castle_packet(ctx, st, def));
    }
}

/// Ops 0xE et 0xF d'un château à tous.
pub fn broadcast_lists(ctx: &WorldCtx, st: &WorldState, id: u8) {
    if let Some(def) = ctx.sieges.get(id) {
        for pkt in lists_packets(st, def) {
            send_all(st, &pkt);
        }
    }
}

/// Région d'un joueur.
pub fn region_of(ctx: &WorldCtx, st: &WorldState, sid: u32) -> Option<u16> {
    let p = st.players.get(&sid)?;
    ctx.map.region(p.ch.pos_x, p.ch.pos_z)
}

/// Joueurs concernés par un siège : présents dans les régions du château, ou membres d'une guilde du siège.
pub fn concerned(ctx: &WorldCtx, st: &WorldState, id: u8) -> Vec<u32> {
    let Some(def) = ctx.sieges.get(id) else { return Vec::new() };
    let guilds = st.siege.castles.get(&id).map(|c| c.participants()).unwrap_or_default();
    st.sids()
        .into_iter()
        .filter(|sid| {
            let p = &st.players[sid];
            p.in_game && (region_of(ctx, st, *sid).is_some_and(|r| def.has_region(r)) || st.guilds.of_cid.get(&p.ch.id).is_some_and(|g| guilds.contains(g)))
        })
        .collect()
}

/// Entrée en jeu : chaque château (op 0), les listes retenues, l'étape de WarInfo d'un siège en cours.
pub fn on_enter(ctx: &WorldCtx, st: &WorldState, sid: u32) {
    let Some(p) = st.players.get(&sid) else { return };
    for def in &ctx.sieges.castles {
        p.send_raw(castle_packet(ctx, st, def));
        let Some(c) = st.siege.castles.get(&def.id) else { continue };
        if c.status >= status::CLOSED {
            for pkt in lists_packets(st, def) {
                p.send_raw(pkt);
            }
        }
    }
    for (id, c) in &st.siege.castles {
        let Some(w) = c.war.as_ref().filter(|_| status::at_war(c.status)) else { continue };
        if concerned(ctx, st, *id).contains(&sid) {
            p.send_raw(super::warinfo::step(war_info_op::SIEGE_WAR, w.step as i64, *id as i64));
            p.send_raw(time_packet(2, w.ends_at.saturating_duration_since(Instant::now()).as_secs()));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Enregistrement
// ---------------------------------------------------------------------------------------------

/// Enregistre un château et ses candidatures.
pub async fn save(ctx: &WorldCtx, st: &WorldState, id: u8) -> anyhow::Result<()> {
    let Some(c) = st.siege.castles.get(&id) else { return Ok(()) };
    let t = |v: i64| (v > 0).then_some(v);
    // Un siège en cours (statuts 3 à 8) s'enregistre tel quel : un redémarrage le clôt sans changement (`Sieges::load`).
    let stored = c.status;
    ctx.db
        .characters
        .exec(
            "INSERT INTO castle (id, owner_guild_id, carve_guild_id, status, last_siege_at, next_siege_at, apply_until, declare_until) \
             VALUES (?, ?, ?, ?, FROM_UNIXTIME(?), FROM_UNIXTIME(?), FROM_UNIXTIME(?), FROM_UNIXTIME(?)) \
             ON DUPLICATE KEY UPDATE owner_guild_id = VALUES(owner_guild_id), carve_guild_id = VALUES(carve_guild_id), status = VALUES(status), \
             last_siege_at = VALUES(last_siege_at), next_siege_at = VALUES(next_siege_at), apply_until = VALUES(apply_until), declare_until = VALUES(declare_until)",
            (id, c.owner, c.carve, stored, t(c.last_siege), t(c.next_siege), t(c.apply_until), t(c.declare_until)),
        )
        .await?;
    ctx.db.characters.exec("DELETE FROM castle_application WHERE castle_id = ?", (id,)).await?;
    for a in &c.applications {
        let side = if a.side == Side::Defense { 2 } else { 1 };
        ctx.db.characters.exec("INSERT INTO castle_application (castle_id, guild_id, side, selected) VALUES (?, ?, ?, ?)", (id, a.guild, side, a.selected as u8)).await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Règles des candidatures
// ---------------------------------------------------------------------------------------------

/// La guilde remplit les conditions (`Siege.MinGuildMembers`, `Siege.MinGuildAgeDays`).
pub fn guild_valid(ctx: &WorldCtx, g: &super::guild::Guild, now: i64) -> bool {
    let w = &ctx.config.world;
    g.active_members() >= w.siege_min_guild_members && now - g.created_at >= w.siege_min_guild_age_days * 86_400
}

/// La guilde est déjà propriétaire ou candidate quelque part (pour Rensphere : dans ce seul château).
pub fn already_involved(st: &WorldState, id: u8, gid: u32) -> bool {
    st.siege
        .castles
        .values()
        .filter(|c| (c.id == RENSPHERE) == (id == RENSPHERE))
        .any(|c| c.owner == Some(gid) || c.applications.iter().any(|a| a.guild == gid))
}

/// Guilde dont le joueur est le chef.
pub fn mastered_guild_of(st: &WorldState, cid: u32) -> Option<u32> {
    let g = st.guilds.of(cid)?;
    (g.master().map(|m| m.cid) == Some(cid)).then_some(g.id)
}

/// Candidature (op 3 défense, op 5 attaque) : rend le code de résultat.
pub fn apply(ctx: &WorldCtx, st: &mut WorldState, sid: u32, id: u8, side: Side) -> i64 {
    let Some(def) = ctx.sieges.get(id) else { return siege_result::FAILED };
    let Some(p) = st.players.get(&sid) else { return siege_result::FAILED };
    let cid = p.ch.id;
    let Some(c) = st.siege.castles.get(&id) else { return siege_result::FAILED };
    if c.status != status::APPLYING {
        return siege_result::OUT_OF_TIME;
    }
    let Some(gid) = mastered_guild_of(st, cid) else { return siege_result::NOT_MASTER };
    if !region_of(ctx, st, sid).is_some_and(|r| def.has_region(r)) {
        return siege_result::FAILED;
    }
    if !st.guilds.by_id.get(&gid).is_some_and(|g| guild_valid(ctx, g, unix_now())) {
        return siege_result::GUILD_CONDITION;
    }
    if already_involved(st, id, gid) {
        return siege_result::UNABLE;
    }
    let limit = if side == Side::Attack { MAX_ATTACK_APPLICATIONS } else { MAX_DEFENSE_APPLICATIONS };
    if c.applicants(side).len() >= limit {
        return siege_result::FULL;
    }
    st.siege.castles.get_mut(&id).unwrap().applications.push(Application { guild: gid, side, selected: false });
    info!("[siège] {} : candidature {} de la guilde {}", def.name, if side == Side::Attack { "d'attaque" } else { "de défense" }, guild_name(st, Some(gid)));
    if side == Side::Attack { siege_result::ATTACK_APPLIED } else { siege_result::DEFENSE_APPLIED }
}

/// Retrait d'une candidature (ops 4 et 6).
pub fn cancel(st: &mut WorldState, sid: u32, id: u8, side: Side) -> i64 {
    let Some(cid) = st.players.get(&sid).map(|p| p.ch.id) else { return siege_result::FAILED };
    let Some(gid) = mastered_guild_of(st, cid) else { return siege_result::NOT_MASTER };
    let Some(c) = st.siege.castles.get_mut(&id) else { return siege_result::FAILED };
    if c.status != status::APPLYING {
        return siege_result::OUT_OF_TIME;
    }
    let before = c.applications.len();
    c.applications.retain(|a| !(a.guild == gid && a.side == side));
    if c.applications.len() == before { siege_result::FAILED } else { siege_result::OK }
}

/// Le propriétaire retient un défenseur (op 0xB).
pub fn select_defender(st: &mut WorldState, sid: u32, id: u8, guild: &str) -> i64 {
    let Some(cid) = st.players.get(&sid).map(|p| p.ch.id) else { return siege_result::FAILED };
    let gid = st.guilds.named(guild).map(|g| g.id);
    let mine = mastered_guild_of(st, cid);
    let Some(c) = st.siege.castles.get_mut(&id) else { return siege_result::FAILED };
    if c.status != status::APPLYING {
        return siege_result::OUT_OF_TIME;
    }
    if mine.is_none() || c.owner != mine || c.selected(Side::Defense).len() >= MAX_DEFENDERS {
        return siege_result::FAILED;
    }
    match c.applications.iter_mut().find(|a| Some(a.guild) == gid && a.side == Side::Defense && !a.selected) {
        Some(a) => {
            a.selected = true;
            siege_result::DEFENSE_SELECTED
        }
        None => siege_result::FAILED,
    }
}

/// Déclaration du prochain siège (op 1) à la date `ft` (FILETIME local).
pub fn declare(ctx: &WorldCtx, st: &mut WorldState, sid: u32, id: u8, ft: u64) -> i64 {
    let Some(def) = ctx.sieges.get(id) else { return siege_result::FAILED };
    let Some(p) = st.players.get(&sid) else { return siege_result::FAILED };
    let (cid, gm) = (p.ch.id, p.gm_level > 0);
    let Some(c) = st.siege.castles.get(&id) else { return siege_result::FAILED };
    if c.status != status::NONE {
        return siege_result::FAILED;
    }
    // Le propriétaire déclare ; un MJ, pour un château sans propriétaire.
    let owner_ok = c.owner.is_some() && mastered_guild_of(st, cid) == c.owner;
    if !owner_ok && !(gm && c.owner.is_none()) {
        return siege_result::NOT_OWNER;
    }
    let tz = ctx.config.world.time_zone;
    let now = unix_now();
    let Some(at) = from_filetime(tz, ft) else { return siege_result::OUT_OF_TIME };
    let last = (c.last_siege > 0).then(|| to_local(tz, c.last_siege).date());
    if !declare_ok(last, to_local(tz, at), to_local(tz, now), def.interval_days) {
        return siege_result::OUT_OF_TIME;
    }
    open_applications(ctx, st, id, at, now);
    info!("[siège] {} : siège déclaré pour le {}", def.name, to_local(tz, at));
    siege_result::DATE_SET
}

/// Statut 1 : date fixée, listes vidées, candidatures ouvertes.
pub fn open_applications(ctx: &WorldCtx, st: &mut WorldState, id: u8, at: i64, now: i64) {
    let Some(def) = ctx.sieges.get(id) else { return };
    let Some(c) = st.siege.castles.get_mut(&id) else { return };
    c.status = status::APPLYING;
    c.next_siege = at;
    c.apply_until = (now + def.select_hours.max(0) * 3600).min(at);
    c.applications.clear();
    c.carve = None;
}

/// Statut 2 : candidatures closes, assaillants retenus (tous, 15 au plus, par points de guilde). Rend faux (siège annulé,
/// date suivante fixée) s'il n'y a aucun assaillant.
pub fn close_applications(ctx: &WorldCtx, st: &mut WorldState, id: u8) -> bool {
    let Some(def) = ctx.sieges.get(id) else { return false };
    let points = |g: &u32| st.guilds.by_id.get(g).map(|g| g.record.points).unwrap_or(0);
    let mut attackers = st.siege.castles.get(&id).map(|c| c.applicants(Side::Attack)).unwrap_or_default();
    attackers.sort_by_key(|g| std::cmp::Reverse(points(g)));
    attackers.truncate(MAX_ATTACK_APPLICATIONS);
    let tz = ctx.config.world.time_zone;
    let c = st.siege.castles.get_mut(&id).unwrap();
    for a in c.applications.iter_mut().filter(|a| a.side == Side::Attack) {
        a.selected = attackers.contains(&a.guild);
    }
    if attackers.is_empty() {
        // Aucun assaillant : date suivante, candidatures rouvertes.
        let next = to_local(tz, c.next_siege) + chrono::Duration::days(def.interval_days);
        info!("[siège] {} : aucun assaillant, siège annulé ; prochaine date {next}", def.name);
        let now = unix_now();
        open_applications(ctx, st, id, from_local(tz, next), now);
        return false;
    }
    c.status = status::CLOSED;
    info!("[siège] {} : candidatures closes, {} assaillant(s)", def.name, attackers.len());
    true
}

/// Fixe d'office la date au premier créneau proposé.
fn auto_declare(ctx: &WorldCtx, st: &mut WorldState, id: u8, now: i64) {
    let Some(def) = ctx.sieges.get(id) else { return };
    let tz = ctx.config.world.time_zone;
    let Some(c) = st.siege.castles.get(&id) else { return };
    let last = (c.last_siege > 0).then(|| to_local(tz, c.last_siege).date());
    let at = from_local(tz, first_slot(last, to_local(tz, now), def.interval_days));
    open_applications(ctx, st, id, at, now);
    info!("[siège] {} : date fixée d'office au {}", def.name, to_local(tz, at));
}

// ---------------------------------------------------------------------------------------------
// Paquets reçus
// ---------------------------------------------------------------------------------------------

/// AgpmSiegeWar reçu du client.
pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some(player) = st.players.get(&sid).filter(|pl| pl.in_game) else { return Ok(()) };
    let cid = player.ch.id;
    if p.int(3).is_some_and(|c| c != 0 && c != cid as i64) {
        warn!("[siège] #{sid} CID {:?} usurpé", p.int(3));
        return Ok(());
    }
    let op = p.int(0).unwrap_or(-1);
    let castle = p.bytes(2).and_then(|b| decode_mini(&mini::SIEGE_CASTLE, b).ok()).unwrap_or_else(|| Fields::new(&mini::SIEGE_CASTLE));
    // Objets d'assaut et carving : désignés par leur CID.
    match op {
        siege_op::OBJECT_OPEN | siege_op::OBJECT_ACTIVATE | siege_op::OBJECT_REPAIR | siege_op::OBJECT_LOAD | siege_op::OBJECT_UNLOAD => {
            return assault::on_packet(ctx, st, sid, op, p.int(5).unwrap_or(0) as u32, p.int(7).unwrap_or(0) as u32).await;
        }
        siege_op::CARVE => {
            war::carve(ctx, st, sid, p.int(5).unwrap_or(0) as u32, Instant::now());
            return Ok(());
        }
        _ => {}
    }
    let name = castle.text(0).unwrap_or_default();
    let Some(def) = ctx.sieges.named(&name).cloned() else {
        warn!("[siège] #{sid} op {op} pour un château inconnu ({name:?})");
        return Ok(());
    };
    let reply = |st: &WorldState, r: i64| {
        if let Some(pl) = st.players.get(&sid) {
            pl.send_raw(result_packet(&def.name, r, cid));
        }
    };
    let page = || p.bytes(4).and_then(|b| decode_mini(&mini::SIEGE_GUILD_PAGE, b).ok()).and_then(|f| f.int(0)).unwrap_or(1).max(1) as usize;
    match op {
        siege_op::DECLARE => {
            let r = declare(ctx, st, sid, def.id, castle.int(5).unwrap_or(0) as u64);
            reply(st, r);
            if r == siege_result::DATE_SET {
                save(ctx, st, def.id).await?;
                broadcast_castle(ctx, st, def.id);
            }
        }
        siege_op::APPLY_DEFENSE | siege_op::APPLY_ATTACK => {
            let side = if op == siege_op::APPLY_ATTACK { Side::Attack } else { Side::Defense };
            let r = apply(ctx, st, sid, def.id, side);
            reply(st, r);
            if matches!(r, siege_result::ATTACK_APPLIED | siege_result::DEFENSE_APPLIED) {
                save(ctx, st, def.id).await?;
            }
        }
        siege_op::CANCEL_DEFENSE | siege_op::CANCEL_ATTACK => {
            let side = if op == siege_op::CANCEL_ATTACK { Side::Attack } else { Side::Defense };
            let r = cancel(st, sid, def.id, side);
            reply(st, r);
            if r == siege_result::OK {
                save(ctx, st, def.id).await?;
            }
        }
        siege_op::ATTACK_PAGE_REQUEST | siege_op::DEFENSE_PAGE_REQUEST => {
            let c = st.siege.castles.get(&def.id).cloned().unwrap_or_else(|| Castle::new(def.id));
            let (reply_op, gids) = if op == siege_op::ATTACK_PAGE_REQUEST {
                (siege_op::ATTACK_PAGE, if c.status < status::CLOSED { c.applicants(Side::Attack) } else { c.selected(Side::Attack) })
            } else {
                (siege_op::DEFENSE_PAGE, c.applicants(Side::Defense))
            };
            let rows = guild_rows(st, &gids);
            if let Some(pl) = st.players.get(&sid) {
                pl.send_raw(page_packet(reply_op, &def.name, cid, &rows, page()));
            }
        }
        siege_op::SELECT_DEFENSE => {
            let guild = castle.text(2).unwrap_or_default();
            let r = select_defender(st, sid, def.id, &guild);
            reply(st, r);
            if r == siege_result::DEFENSE_SELECTED {
                save(ctx, st, def.id).await?;
                broadcast_lists(ctx, st, def.id);
            }
        }
        siege_op::ATTACKERS_REQUEST | siege_op::DEFENDERS_REQUEST => {
            let [attackers, defenders] = lists_packets(st, &def);
            if let Some(pl) = st.players.get(&sid) {
                pl.send_raw(if op == siege_op::ATTACKERS_REQUEST { attackers } else { defenders });
            }
        }
        other => warn!("[siège] #{sid} op {other} refusée (réservée au serveur ou inconnue)"),
    }
    Ok(())
}

/// AgpmEventSiegeWarNPC reçu : un commandant de siège (fonction 33) ouvre la fenêtre de son château.
pub fn on_npc_packet(ctx: &WorldCtx, st: &WorldState, sid: u32, p: &Packet) {
    let Some(player) = st.players.get(&sid).filter(|pl| pl.in_game && !pl.dead) else { return };
    if p.int(0) != Some(siege_npc_op::REQUEST) {
        warn!("[siège] #{sid} AgpmEventSiegeWarNPC op {:?} inattendue", p.int(0));
        return;
    }
    let ev = p.bytes(1).and_then(|b| decode_mini(&mini::EVENT_BASE, b).ok()).unwrap_or_else(|| Fields::new(&mini::EVENT_BASE));
    let Some((npc, _)) = super::npc_events::resolve(&ctx.npcs, &ev) else {
        warn!("[siège] #{sid} commandant de siège inconnu");
        return;
    };
    if !npc.events.iter().any(|e| e.function == super::npcs::event_function::SIEGE_WAR_NPC) {
        warn!("[siège] #{sid} {} n'est pas un commandant de siège", npc.name);
        return;
    }
    if super::npc_events::npc_distance(npc, player.ch.pos_x, player.ch.pos_z) > NPC_RANGE {
        return;
    }
    let Some(def) = ctx.map.region(npc.pos.x as f64, npc.pos.z as f64).and_then(|r| ctx.sieges.by_region(r)) else {
        warn!("[siège] {} : aucun château pour la région de ce commandant", npc.name);
        return;
    };
    player.send_raw(castle_packet(ctx, st, def));
    let echo = if p.bytes(1).is_some() { build_mini(&mini::EVENT_BASE, &ev) } else { super::npcs::event_base(npc.cid, super::npcs::event_function::SIEGE_WAR_NPC) };
    let s = &schema::EVENT_SIEGE_WAR_NPC;
    player.send(s, Fields::new(s).with(0, siege_npc_op::GRANT).with(1, echo).with(2, player.ch.id));
    info!("[siège] {} ouvre la fenêtre de {} chez {}", player.ch.name, def.name, npc.name);
}

// ---------------------------------------------------------------------------------------------
// Calendrier
// ---------------------------------------------------------------------------------------------

/// Boucle des sièges (appelée par la boucle de combat, une fois par seconde au plus).
pub async fn step(ctx: &WorldCtx, st: &mut WorldState, now: Instant) -> anyhow::Result<()> {
    if ctx.sieges.castles.is_empty() || st.siege.next_step.is_some_and(|t| now < t) {
        return Ok(());
    }
    st.siege.next_step = Some(now + STEP);
    let unix = unix_now();
    let ids: Vec<u8> = ctx.sieges.castles.iter().filter(|d| !d.disabled).map(|d| d.id).collect();
    for id in ids {
        let Some(c) = st.siege.castles.get(&id).cloned() else { continue };
        let auto = ctx.config.world.siege_enabled && id != RENSPHERE;
        match c.status {
            status::NONE if auto => {
                let declare_until = match c.declare_until {
                    0 if c.owner.is_some() => {
                        // Premier passage avec un propriétaire : sa fenêtre de déclaration commence.
                        let def = ctx.sieges.get(id).unwrap();
                        st.siege.castles.get_mut(&id).unwrap().declare_until = unix + def.declare_hours * 3600;
                        save(ctx, st, id).await?;
                        continue;
                    }
                    t => t,
                };
                if c.owner.is_none() || unix >= declare_until {
                    auto_declare(ctx, st, id, unix);
                    save(ctx, st, id).await?;
                    broadcast_castle(ctx, st, id);
                }
            }
            status::APPLYING if auto && unix >= c.apply_until => {
                if close_applications(ctx, st, id) {
                    broadcast_lists(ctx, st, id);
                }
                save(ctx, st, id).await?;
                broadcast_castle(ctx, st, id);
            }
            status::CLOSED if auto && unix >= c.next_siege => war::start(ctx, st, id, now).await?,
            s if status::at_war(s) => war::advance(ctx, st, id, now).await?,
            _ => {}
        }
    }
    Ok(())
}

/// Accroche du changement de propriétaire d'un château (fin de siège, commande MJ) : les taux de taxe du château
/// repartent de 0 pour le nouveau propriétaire (`tax.rs`). L'ArchLord se décide à la fin de la guerre de Rensphere
/// (`archlord.rs`), qu'elle change ou non le propriétaire.
pub fn on_owner_change(ctx: &WorldCtx, st: &mut WorldState, id: u8, before: Option<u32>, after: Option<u32>) {
    let name = ctx.sieges.get(id).map(|d| d.name.as_str()).unwrap_or("?");
    info!("[siège] {name} : propriétaire {:?} → {:?}{}", guild_name(st, before), guild_name(st, after), if id == RENSPHERE { " (château de l'Archlord)" } else { "" });
    super::tax::on_owner_change(ctx, st, id);
}

// ---------------------------------------------------------------------------------------------
// Commandes MJ
// ---------------------------------------------------------------------------------------------

/// `/siege` : état des châteaux.
pub fn describe(ctx: &WorldCtx, st: &WorldState) -> String {
    let tz = ctx.config.world.time_zone;
    let mut lines = Vec::new();
    for d in &ctx.sieges.castles {
        let Some(c) = st.siege.castles.get(&d.id) else { continue };
        let owner = guild_name(st, c.owner);
        let next = if c.next_siege > 0 { to_local(tz, c.next_siege).format("%Y-%m-%d %H:%M").to_string() } else { "-".into() };
        lines.push(format!(
            "{} {} : statut {}, propriétaire {}, prochain siège {next}, {} attaquant(s), {} défenseur(s)",
            d.id,
            d.name,
            c.status,
            if owner.is_empty() { "aucun".into() } else { owner },
            c.applicants(Side::Attack).len(),
            c.defenders().len()
        ));
    }
    if lines.is_empty() { "Aucun château (table siege_war vide).".into() } else { lines.join("\n") }
}

/// `/siege owner <château> <guilde>` (« - » : aucun) : propriétaire changé, calendrier remis au statut 0.
pub async fn set_owner(ctx: &WorldCtx, st: &mut WorldState, castle: &str, guild: &str) -> anyhow::Result<Result<String, String>> {
    let Some(def) = ctx.sieges.named(castle).cloned() else { return Ok(Err(format!("château inconnu : {castle}"))) };
    let gid = match guild {
        "-" | "" => None,
        g => match st.guilds.named(g) {
            Some(g) => Some(g.id),
            None => return Ok(Err(format!("guilde inconnue : {g}"))),
        },
    };
    if st.siege.castles.get(&def.id).is_some_and(|c| status::at_war(c.status)) {
        return Ok(Err(format!("{} est en guerre : /siege end d'abord", def.name)));
    }
    if let Some(g) = gid {
        // Une guilde ne tient qu'un château (Rensphere à part) : elle quitte les autres listes.
        for c in st.siege.castles.values_mut().filter(|c| c.id != def.id && (c.id == RENSPHERE) == (def.id == RENSPHERE)) {
            c.applications.retain(|a| a.guild != g);
            if c.owner == Some(g) {
                c.owner = None;
            }
        }
    }
    let c = st.siege.castles.entry(def.id).or_insert_with(|| Castle::new(def.id));
    let before = c.owner;
    c.owner = gid;
    c.status = status::NONE;
    c.next_siege = 0;
    c.apply_until = 0;
    c.declare_until = unix_now() + def.declare_hours * 3600;
    c.applications.clear();
    let ids: Vec<u8> = st.siege.castles.keys().copied().collect();
    for id in ids {
        save(ctx, st, id).await?;
    }
    on_owner_change(ctx, st, def.id, before, gid);
    broadcast_castle(ctx, st, def.id);
    Ok(Ok(format!("{} : propriétaire {}.", def.name, if guild.is_empty() || guild == "-" { "aucun" } else { guild })))
}

/// `/siege start <château>` : candidatures closes (assaillants retenus) et combat ouvert tout de suite.
pub async fn force_start(ctx: &WorldCtx, st: &mut WorldState, castle: &str, now: Instant) -> anyhow::Result<Result<String, String>> {
    let Some(def) = ctx.sieges.named(castle).cloned() else { return Ok(Err(format!("château inconnu : {castle}"))) };
    let c = st.siege.castles.entry(def.id).or_insert_with(|| Castle::new(def.id));
    if status::at_war(c.status) {
        return Ok(Err(format!("{} est déjà en guerre", def.name)));
    }
    if c.status == status::APPLYING {
        // Clôture forcée : tous les candidats attaquants retenus, même s'il n'y en a aucun.
        for a in c.applications.iter_mut().filter(|a| a.side == Side::Attack).take(MAX_ATTACK_APPLICATIONS) {
            a.selected = true;
        }
    }
    c.status = status::CLOSED;
    c.next_siege = unix_now();
    war::start(ctx, st, def.id, now).await?;
    Ok(Ok(format!("{} : siège commencé.", def.name)))
}

/// `/siege end <château>` : fin du combat tout de suite.
pub async fn force_end(ctx: &WorldCtx, st: &mut WorldState, castle: &str, now: Instant) -> anyhow::Result<Result<String, String>> {
    let Some(def) = ctx.sieges.named(castle).cloned() else { return Ok(Err(format!("château inconnu : {castle}"))) };
    if !st.siege.castles.get(&def.id).is_some_and(|c| status::at_war(c.status) && c.status != status::ENDED) {
        return Ok(Err(format!("{} n'est pas en guerre", def.name)));
    }
    war::finish(ctx, st, def.id, now).await?;
    let owner = guild_name(st, st.siege.castles.get(&def.id).and_then(|c| c.owner));
    Ok(Ok(format!("{} : siège terminé, propriétaire {}.", def.name, if owner.is_empty() { "aucun".into() } else { owner })))
}

#[cfg(test)]
mod tests;
