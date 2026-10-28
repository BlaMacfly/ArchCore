//! Invocations (AgpmSummons, 0x4F) : créatures des compétences (esprits de l'Élémentaliste, Sacred One, Legion of the
//! Dead, pièges…), parchemins d'invocation et familiers des objets.
//!
//! ```text
//!   Compétence : colonne `summons` de SKILL_SPEC, valeurs de SKILL_CONST au niveau de la compétence :
//!     1 invocation de combat      TID `summons_tid`, `summons_count` exemplaires (1 à défaut), durée `duration`
//!     2 créature fixe (piège)     paquet de type 4, au plus `limit_quantity` de la même compétence
//!     4 Increase Gate             limite des invocations relevée à 1 + `limit_quantity` pendant `duration` (op 3)
//!     5 Elemental Heal (passive)  son `percent.max_hp` s'ajoute aux PV des invocations du maître
//!     6 parchemin d'invocation    créature de combat comme le 1, « forcée » (hors limite), une par parchemin
//!     8 familier                  objet à « Use skill id » : suit le maître, ne combat pas ; réutiliser l'objet le renvoie
//!   Apparition, dans cet ordre (exigé par le client) : vue de la créature (0x42 op 1, facteurs), son état spécial
//!     (AGPMCHARACTER op 1, champ 17 : 0x800 pour les types 1 et 8, 0xC00 pour le type 4), puis l'op 0 au maître et à
//!     ceux qui la voient (aussi à chaque joueur qui l'aperçoit ensuite).
//!   Fin : op 1 au maître et à ceux qui la voient, puis retrait de la vue. Durée écoulée, mort de la créature, maître
//!     mort ou parti, téléportation du maître (portail, parchemin, MJ) pour les invocations de combat, nouvelle
//!     invocation au-delà de la limite (la plus ancienne part).
//!   Propension (op 2 du client, invocations de type 1 seulement) : appliquée puis renvoyée au maître.
//! ```
//! Choix d'ArchCore (règles d'origine inconnues) :
//! - Caractéristiques : celles du modèle (CharacterDataTable), multipliées par niveau du maître / niveau du modèle
//!   (« LVL ») puis par `Summons.StatScale` (pour cent) ; niveau de la créature = niveau du maître. Un familier garde
//!   celles de son modèle.
//! - Horodatages de l'op 0 : apparition et fin prévue en ms Unix (un familier : dans dix ans).
//! - Comportement : une invocation de combat reste à ~200 unités de son maître et le rejoint d'un saut au-delà de
//!   3000 ; propension « attaque » (0) : elle frappe la cible du maître, sinon ceux qui s'en prennent à lui ou à elle ;
//!   « défense » (1) : ces derniers seulement ; « ombre » (2) : elle ne combat pas. Une créature fixe frappe les monstres
//!   à sa portée (ceux qui visent le maître d'abord). Les monstres frappés la prennent pour cible (`ai.rs`).
//! - Un monstre tué par une invocation l'est par son maître : expérience (partage de groupe compris), butin, quêtes.
//! - Combat entre joueurs : une invocation n'attaque un joueur que si son maître le peut (`pvp.rs`) ; l'attaquer, c'est
//!   suivre les mêmes règles qu'envers son maître.
//! - Non faits : apprivoisement (type 2, compétence inconnue), Call on Nature et Evil Light (7 : pas de TID de
//!   créature), pièges de faille (11), arrêt d'un familier par un paquet dédié (inconnu : seule la réutilisation de
//!   l'objet le renvoie), renforcements des familiers (colonnes « point.* » de leur compétence).

use super::combat::{self, Attacker, Defender};
use super::effects::{self, status};
use super::items::InventoryItem;
use super::monsters::{Life, Monster, MonsterStats};
use super::pvp::Refusal;
use super::skills::{SkillLevel, SkillTemplate};
use super::{send_system, WorldCtx, WorldState};
use crate::{debug, info, warn};
use archcore_net::schema::{self, summons_op};
use archcore_net::{build_packet, Fields, Packet, Vec3};
use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

/// Les créatures invoquées reçoivent des CID dans une plage à part, après celle des monstres du monde.
pub const SUMMON_CID_BASE: u32 = 0x5800_0000;
const SUMMON_CID_SPAN: u32 = 0x0800_0000;

/// Type d'invocation du paquet (champ 4 de l'op 0).
pub mod kind {
    pub const SUMMONS: i64 = 1;
    pub const TAME: i64 = 2;
    pub const FIXED: i64 = 4;
    pub const PET: i64 = 8;
}

/// Nature d'invocation d'une compétence (colonne `summons` de SKILL_SPEC).
pub mod skill_kind {
    pub const SUMMONS: i64 = 1;
    pub const FIXED: i64 = 2;
    pub const LIMIT: i64 = 4;
    pub const HEAL: i64 = 5;
    pub const SCROLL: i64 = 6;
    pub const PET: i64 = 8;
}

/// Propension d'une invocation de combat (champ 5).
pub mod propensity {
    pub const ATTACK: i64 = 0;
    pub const DEFENSE: i64 = 1;
    pub const SHADOW: i64 = 2;
}

/// Limite des invocations ordinaires d'un maître (valeur du client au départ).
pub const DEFAULT_LIMIT: i64 = 1;
/// Distance gardée au maître, marge avant de le rejoindre, distance au-delà de laquelle la créature le rejoint d'un saut.
pub const FOLLOW_DISTANCE: f64 = 200.0;
const FOLLOW_SLACK: f64 = 300.0;
pub const TELEPORT_DISTANCE: f64 = 3000.0;
/// Rythme de décision d'une invocation, et distance au maître au-delà de laquelle elle ne poursuit plus.
const THINK: Duration = Duration::from_millis(300);
const ASSIST_RANGE: f64 = 5000.0;
/// Marge de portée, pour les positions arrondies.
const REACH_TOLERANCE: f64 = 50.0;
/// Rayon du corps d'un joueur, ajouté à la portée de la créature.
const PLAYER_HIT_RANGE: f64 = 50.0;
/// Fin affichée d'un familier, sans durée : dix ans.
const PET_SPAN_MS: u64 = 10 * 365 * 24 * 3600 * 1000;

/// Une créature invoquée.
#[derive(Clone, Debug, PartialEq)]
pub struct Summon {
    pub cid: u32,
    /// CID du maître.
    pub owner: u32,
    pub tid: u32,
    /// Type du paquet (`kind`).
    pub kind: i64,
    /// Compétence qui l'a invoquée.
    pub skill_tid: u32,
    pub propensity: i64,
    /// Hors limite (familier, parchemin), champ 8.
    pub force: bool,
    /// Horodatages de l'op 0 (ms Unix) et fin réelle (aucune pour un familier).
    pub time_a: u64,
    pub time_b: u64,
    pub ends_at: Option<Instant>,
    /// Objet qui l'a appelée (familier : IID, pour le renvoyer en réutilisant l'objet).
    pub item: Option<u32>,
    /// Ordre d'invocation : la plus ancienne part la première.
    pub seq: u64,
}

/// Limite des invocations ordinaires d'un maître.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Limits {
    /// Limite relevée (Increase Gate) jusqu'à cet instant.
    pub raised: i64,
    pub raised_until: Option<Instant>,
    /// Dernière limite annoncée au client (op 3), hors emplacements « forcés » ; 0 : jamais annoncée (1 côté client).
    pub sent: i64,
}

impl Limits {
    pub fn base(&self, now: Instant) -> i64 {
        if self.raised_until.is_some_and(|t| now < t) { self.raised.max(DEFAULT_LIMIT) } else { DEFAULT_LIMIT }
    }
    fn sent_or_default(&self) -> i64 {
        if self.sent > 0 { self.sent } else { DEFAULT_LIMIT }
    }
}

/// Invocations en cours et limites des maîtres (par CID du maître).
#[derive(Default)]
pub struct Summons {
    pub list: BTreeMap<u32, Summon>,
    pub limits: HashMap<u32, Limits>,
    next_cid: u32,
    next_seq: u64,
}

impl Summons {
    /// Invocations d'un maître, de la plus ancienne à la plus récente.
    pub fn of_owner(&self, owner: u32) -> Vec<Summon> {
        let mut v: Vec<Summon> = self.list.values().filter(|s| s.owner == owner).cloned().collect();
        v.sort_by_key(|s| s.seq);
        v
    }

    /// Paquets qui suivent la vue d'une créature invoquée chez un joueur qui l'aperçoit : son état spécial (`status`,
    /// permanents compris), puis l'op 0. Rien pour un autre monstre.
    pub fn view_extras(&self, cid: u32, status: u64) -> Vec<Vec<u8>> {
        match self.list.get(&cid) {
            Some(s) => vec![effects::status_packet(cid, status), add_packet(s)],
            None => Vec::new(),
        }
    }

    fn alloc_cid(&mut self, taken: impl Fn(u32) -> bool) -> u32 {
        loop {
            self.next_cid = (self.next_cid + 1) % SUMMON_CID_SPAN;
            let cid = SUMMON_CID_BASE + self.next_cid;
            if !taken(cid) && !self.list.contains_key(&cid) {
                return cid;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Paquets
// ---------------------------------------------------------------------------------------------

fn base(op: i64, owner: u32) -> Fields {
    let s = &schema::SUMMONS;
    Fields::new(s).with(0, op).with(1, owner)
}

/// Op 0 : invocation ajoutée chez son maître.
pub fn add_packet(s: &Summon) -> Vec<u8> {
    let f = base(summons_op::ADD, s.owner).with(2, s.cid).with(3, s.tid).with(4, s.kind).with(5, s.propensity);
    build_packet(&schema::SUMMONS, &f.with(6, s.time_a).with(7, s.time_b).with(8, u8::from(s.force) as i64))
}

/// Op 1 : invocation retirée.
pub fn remove_packet(owner: u32, cid: u32) -> Vec<u8> {
    build_packet(&schema::SUMMONS, &base(summons_op::REMOVE, owner).with(2, cid).with(8, 0))
}

/// Op 2 : propension appliquée, renvoyée au maître.
pub fn propensity_packet(owner: u32, cid: u32, propensity: i64) -> Vec<u8> {
    build_packet(&schema::SUMMONS, &base(summons_op::SET_PROPENSITY, owner).with(2, cid).with(5, propensity).with(8, 0))
}

/// Op 3 : limite des invocations.
pub fn limit_packet(owner: u32, max: i64) -> Vec<u8> {
    build_packet(&schema::SUMMONS, &base(summons_op::SET_MAX_SUMMONS, owner).with(4, max.clamp(0, 127)).with(8, 0))
}

/// AGPMCHARACTER op 2 : la créature disparaît aussitôt (l'op 3, retrait de la vue, la laisse à l'écran du client).
fn delete_packet(cid: u32) -> Vec<u8> {
    let c = &schema::CHARACTER;
    build_packet(c, &Fields::new(c).with(0, 2).with(1, cid))
}

// ---------------------------------------------------------------------------------------------
// Règles
// ---------------------------------------------------------------------------------------------

/// Type de paquet et état spécial d'une nature d'invocation (colonne `summons`) ; `None` : rien à faire apparaître.
pub fn packet_kind(skill_kind: i64) -> Option<(i64, u64)> {
    let k = match skill_kind {
        skill_kind::SUMMONS | skill_kind::SCROLL => kind::SUMMONS,
        skill_kind::FIXED => kind::FIXED,
        skill_kind::PET => kind::PET,
        _ => return None,
    };
    Some((k, state_of(k)))
}

/// État spécial que la créature doit porter avant l'op 0 de son type : 0x800 (types 1 et 8), 0x1000 (2), 0xC00 (4).
pub fn state_of(k: i64) -> u64 {
    match k {
        kind::TAME => status::TAME,
        kind::FIXED => status::DONT_MOVE | status::SUMMONER,
        _ => status::SUMMONER,
    }
}

/// Caractéristiques d'une invocation : celles du modèle × niveau du maître / niveau du modèle × `scale_pct` %, PV
/// majorés de `hp_bonus_pct` % (Elemental Heal). Vitesses, portée, résistances et type restent ceux du modèle ; ni
/// expérience ni butin.
pub fn scaled_stats(model: &MonsterStats, master_level: i32, scale_pct: f64, hp_bonus_pct: f64) -> MonsterStats {
    let k = master_level.max(1) as f64 / model.level.max(1) as f64 * scale_pct / 100.0;
    let x = |v: i32| ((v as f64 * k).round() as i32).max(if v > 0 { 1 } else { 0 });
    MonsterStats {
        max_hp: ((x(model.max_hp) as f64 * (100.0 + hp_bonus_pct) / 100.0).round() as i32).max(1),
        max_mp: x(model.max_mp),
        max_sp: x(model.max_sp),
        ar: x(model.ar),
        min_damage: x(model.min_damage),
        max_damage: x(model.max_damage),
        ac: x(model.ac),
        spirit: model.spirit.map(|(lo, hi)| (x(lo), x(hi))),
        exp: 0,
        geld_min: 0,
        geld_max: 0,
        drop_rank: 0,
        level: master_level.max(1),
        ..model.clone()
    }
}

/// Invocations à renvoyer (les plus anciennes d'abord, `existing` étant de la plus ancienne à la plus récente) pour
/// que `incoming` nouvelles tiennent dans `cap`.
pub fn to_dismiss(existing: &[u32], incoming: usize, cap: usize) -> &[u32] {
    let excess = (existing.len() + incoming).saturating_sub(cap).min(existing.len());
    &existing[..excess]
}

/// Cible d'une invocation de combat : `master_target` (déjà validée) en propension « attaque », sinon la cible
/// actuelle si elle s'en prend encore au maître ou à l'invocation, sinon le plus proche de ces agresseurs
/// (`attackers` : CID, distance). Rien en propension « ombre ».
pub fn pick(propensity: i64, master_target: Option<u32>, current: Option<u32>, attackers: &[(u32, f64)]) -> Option<u32> {
    let defend = || current.filter(|c| attackers.iter().any(|a| a.0 == *c)).or_else(|| attackers.iter().min_by(|a, b| a.1.total_cmp(&b.1)).map(|a| a.0));
    match propensity {
        propensity::SHADOW => None,
        propensity::DEFENSE => defend(),
        _ => master_target.or_else(defend),
    }
}

/// Temps Unix en ms.
fn unix_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(1).max(1)
}

/// La compétence fait apparaître quelque chose (pour l'utilisation d'un objet).
pub fn is_summons_skill(ctx: &WorldCtx, skill_tid: u32) -> bool {
    ctx.skills.get(skill_tid).is_some_and(|t| packet_kind(t.summons).is_some())
}

/// Le joueur `sid` peut-il attaquer la créature `cid` ? Toujours pour un monstre ; pour une invocation, jamais la sienne,
/// et les règles du combat entre joueurs envers son maître.
pub fn may_attack(ctx: &WorldCtx, st: &WorldState, sid: u32, cid: u32, forced: bool) -> Result<(), Refusal> {
    let Some(owner) = st.monsters.owner_of(cid) else { return Ok(()) };
    if st.players.get(&sid).is_some_and(|p| p.ch.id == owner) {
        return Err(Refusal(None));
    }
    match st.by_cid.get(&owner).copied() {
        Some(osid) => super::pvp::check(ctx, st, sid, osid, forced),
        None => Err(Refusal(None)),
    }
}

/// Dégâts reçus par le monstre `cid`, par agresseur : ceux d'une invocation comptent pour son maître.
pub fn damage_by_owner(st: &WorldState, cid: u32) -> HashMap<u32, u64> {
    let mut out = HashMap::new();
    for a in st.monsters.get(cid).map(|m| m.aggro.as_slice()).unwrap_or(&[]) {
        let who = st.monsters.owner_of(a.cid).or_else(|| st.summons.list.get(&a.cid).map(|s| s.owner)).unwrap_or(a.cid);
        *out.entry(who).or_insert(0) += a.damage;
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Apparition et disparition
// ---------------------------------------------------------------------------------------------

/// Ce qu'il faut faire apparaître.
struct Spawn {
    tid: u32,
    skill_kind: i64,
    skill_tid: u32,
    duration_ms: Option<u64>,
    item: Option<u32>,
}

/// Fait apparaître une créature près du maître `sid` : registre des monstres, vue, état, op 0. Rend son CID.
fn spawn(ctx: &WorldCtx, st: &mut WorldState, sid: u32, sp: &Spawn, now: Instant) -> Option<u32> {
    let (pkind, state) = packet_kind(sp.skill_kind)?;
    let p = st.players.get(&sid)?;
    let (owner, master_level, master_pos, master_name) = (p.ch.id, p.factors.status[7], p.ch.pos(), p.ch.name.clone());
    let Some(model) = st.monsters.stats.get(&sp.tid).cloned() else {
        warn!("[invocation] {master_name} : créature {} inconnue (CharacterDataTable)", sp.tid);
        send_system(st, sid, "Cette créature ne peut pas être invoquée.");
        return None;
    };
    // Elemental Heal (passive, `summons` 5) : PV des invocations majorés.
    let hp_bonus: f64 = p
        .skills
        .list
        .iter()
        .filter_map(|o| ctx.skills.get(o.tid).filter(|t| t.summons == skill_kind::HEAL).and_then(|t| t.level(o.level)))
        .map(|lv| lv.mods.max_hp_pct)
        .sum();
    let stats = if pkind == kind::PET { model.clone() } else { scaled_stats(&model, master_level, ctx.config.world.summons_stat_scale, hp_bonus) };
    let seq = st.summons.next_seq;
    st.summons.next_seq += 1;
    let cid = st.summons.alloc_cid(|c| st.monsters.get(c).is_some());
    // Autour du maître, chaque nouvelle créature un peu plus loin dans le cercle.
    let angle = seq as f64 * 2.4;
    let mut pos = Vec3::new(master_pos.x + (FOLLOW_DISTANCE * angle.cos()) as f32, master_pos.y, master_pos.z + (FOLLOW_DISTANCE * angle.sin()) as f32);
    let grounded = match ctx.map.height(pos.x as f64, pos.z as f64) {
        Some(y) => {
            pos.y = y;
            true
        }
        None => false,
    };
    let name = st.monsters.names.get(&sp.tid).cloned().unwrap_or_else(|| format!("Créature {}", sp.tid));
    st.monsters.insert(Monster {
        cid,
        spawn_id: 0,
        tid: sp.tid,
        level: master_level.max(1) as i64,
        name: name.clone(),
        pos,
        degree_x: 0.0,
        degree_y: 0.0,
        hp: stats.max_hp.max(1),
        max_hp: stats.max_hp.max(1),
        respawn_sec: 0,
        respawn_max_sec: 0,
        life: Life::Alive,
        home: pos,
        aggro: Vec::new(),
        target: None,
        ready_at: now,
        scan_at: now,
        roam: 0.0,
        patrol_at: now,
        ai_tid: 0,
        owner: Some(owner),
    });
    if grounded {
        st.monsters.grounded.insert(cid);
    }
    if pkind != kind::PET {
        st.monsters.custom.insert(cid, stats.clone());
    }
    st.monsters.fixed_status.insert(cid, state);
    st.monsters.status_sent.insert(cid, state);
    let time_a = unix_ms();
    let summon = Summon {
        cid,
        owner,
        tid: sp.tid,
        kind: pkind,
        skill_tid: sp.skill_tid,
        propensity: propensity::ATTACK,
        force: matches!(sp.skill_kind, skill_kind::PET | skill_kind::SCROLL),
        time_a,
        time_b: time_a + sp.duration_ms.unwrap_or(PET_SPAN_MS),
        ends_at: sp.duration_ms.map(|ms| now + Duration::from_millis(ms)),
        item: sp.item,
        seq,
    };
    st.summons.list.insert(cid, summon);
    show(ctx, st, cid, Some(sid));
    info!("[invocation] {master_name} invoque {name} ({cid:#x}, TID {}, type {pkind}, {} PV, niveau {master_level}{})", sp.tid, stats.max_hp, sp.duration_ms.map(|ms| format!(", {} s", ms / 1000)).unwrap_or_default());
    Some(cid)
}

/// Montre la créature à ceux qui voient sa place : vue, état spécial, op 0 ; au maître `owner_sid` en tout cas.
fn show(ctx: &WorldCtx, st: &mut WorldState, cid: u32, owner_sid: Option<u32>) {
    let Some(m) = st.monsters.get(cid) else { return };
    let pos = m.pos;
    let extras = st.summons.view_extras(cid, st.monsters.status(cid, Instant::now()));
    for (psid, p) in st.players.iter_mut().filter(|(_, p)| p.in_game) {
        if !(super::sector::sees(p.view_pos, pos) || Some(*psid) == owner_sid) || !p.seen_monsters.insert(cid) {
            continue;
        }
        let name = ctx.locale.resolve(crate::locale::kind::MONSTER_NAME, m.tid, &p.language, &m.name);
        p.send_raw(st.monsters.view_packet_named(m, &name));
        for e in &extras {
            p.send_raw(e.clone());
        }
    }
}

/// Renvoie une invocation : op 1 au maître et à ceux qui la voient, puis retrait de la vue (aussitôt, ou après la
/// mort laissée à l'écran : `immediate` faux), et retrait du registre.
pub fn dismiss(st: &mut WorldState, cid: u32, why: &str, immediate: bool) {
    let Some(s) = st.summons.list.remove(&cid) else { return };
    let gone = st.monsters.get(cid).map(|m| m.name.clone()).unwrap_or_default();
    let op1 = remove_packet(s.owner, cid);
    let view = if immediate { delete_packet(cid) } else { super::character_remove_packet(cid) };
    let owner_sid = st.by_cid.get(&s.owner).copied();
    for (psid, p) in st.players.iter_mut() {
        let seen = p.seen_monsters.remove(&cid);
        if seen || Some(*psid) == owner_sid {
            p.send_raw(op1.clone());
        }
        if seen {
            p.send_raw(view.clone());
        }
    }
    st.monsters.remove(cid);
    info!("[invocation] {gone} ({cid:#x}) de {:#x} disparaît : {why}", s.owner);
}

/// Mort d'une invocation (PV à 0) : statut « mort » chez ceux qui la voient, puis disparition.
pub fn died(st: &mut WorldState, cid: u32) {
    if let Some(m) = st.monsters.get_mut(cid) {
        m.life = Life::Dead { remove_at: Instant::now() };
    }
    let c = &schema::CHARACTER;
    let dead = build_packet(c, &Fields::new(c).with(0, 1).with(1, cid).with(11, 2));
    let watchers = combat::viewers(st, cid);
    combat::broadcast(st, &watchers, &dead);
    dismiss(st, cid, "tuée", false);
}

fn owner_cid(st: &WorldState, sid: u32) -> Option<u32> {
    st.players.get(&sid).map(|p| p.ch.id)
}

/// Le maître quitte le monde : toutes ses créatures disparaissent.
pub fn on_owner_leave(st: &mut WorldState, sid: u32) {
    let Some(owner) = owner_cid(st, sid) else { return };
    for s in st.summons.of_owner(owner) {
        dismiss(st, s.cid, "maître parti", true);
    }
    st.summons.limits.remove(&owner);
}

/// Mort du maître : ses invocations et son familier disparaissent ; les créatures fixes restent jusqu'à leur fin.
pub fn on_owner_death(st: &mut WorldState, sid: u32) {
    let Some(owner) = owner_cid(st, sid) else { return };
    for s in st.summons.of_owner(owner).into_iter().filter(|s| s.kind != kind::FIXED) {
        dismiss(st, s.cid, "maître mort", true);
    }
}

/// Téléportation du maître (portail, parchemin, résurrection, MJ) : ses invocations de combat disparaissent.
pub fn on_owner_teleport(st: &mut WorldState, sid: u32) {
    let Some(owner) = owner_cid(st, sid) else { return };
    for s in st.summons.of_owner(owner).into_iter().filter(|s| s.kind == kind::SUMMONS) {
        dismiss(st, s.cid, "maître téléporté", true);
    }
}

/// Après la téléportation du maître : son familier le rejoint.
pub fn follow_teleport(ctx: &WorldCtx, st: &mut WorldState, sid: u32) {
    let Some(p) = st.players.get(&sid) else { return };
    let (owner, at) = (p.ch.id, p.ch.pos());
    for s in st.summons.of_owner(owner).into_iter().filter(|s| s.kind == kind::PET) {
        let spot = beside(at, st.monsters.get(s.cid).map(|m| m.pos).unwrap_or(at));
        jump(ctx, st, s.cid, spot);
    }
}

/// Place à `FOLLOW_DISTANCE` du maître en `at`, du côté de `from`.
fn beside(at: Vec3, from: Vec3) -> Vec3 {
    let d = combat::dist2d(at, from);
    if d < 1.0 {
        return Vec3::new(at.x + FOLLOW_DISTANCE as f32, at.y, at.z);
    }
    let k = (FOLLOW_DISTANCE / d) as f32;
    Vec3::new(at.x + (from.x - at.x) * k, at.y, at.z + (from.z - at.z) * k)
}

/// Saut de la créature en `to` : vue revue chez chacun (apparition, retrait), arrêt net chez ceux qui la gardent.
fn jump(ctx: &WorldCtx, st: &mut WorldState, cid: u32, mut to: Vec3) {
    if st.monsters.grounded.contains(&cid) {
        if let Some(y) = ctx.map.height(to.x as f64, to.z as f64) {
            to.y = y;
        }
    }
    st.monsters.walking.remove(&cid);
    st.monsters.relocate(cid, to);
    if let Some(m) = st.monsters.get_mut(cid) {
        m.home = to;
    }
    let mv = &schema::OPT_CHAR_MOVE;
    let stop = Fields::new(mv).with(0, cid).with(1, to).with(2, to).with(5, super::MOVE_STOP_SYNC).with(6, 0);
    let gone = super::character_remove_packet(cid);
    let mut kept = Vec::new();
    for (psid, p) in st.players.iter_mut().filter(|(_, p)| p.in_game) {
        if p.seen_monsters.contains(&cid) {
            if super::sector::keeps(p.view_pos, to) {
                kept.push(*psid);
            } else {
                p.seen_monsters.remove(&cid);
                p.send_raw(gone.clone());
            }
        }
    }
    for psid in kept {
        st.players[&psid].send(mv, stop.clone());
    }
    show(ctx, st, cid, None);
}

// ---------------------------------------------------------------------------------------------
// Lancement
// ---------------------------------------------------------------------------------------------

/// Fin de l'incantation d'une compétence d'invocation (`skills::complete`).
pub fn on_cast(ctx: &WorldCtx, st: &mut WorldState, sid: u32, t: &SkillTemplate, lv: &SkillLevel, now: Instant) {
    let Some(owner) = owner_cid(st, sid) else { return };
    let duration = (lv.duration_ms > 0).then_some(lv.duration_ms);
    match t.summons {
        skill_kind::LIMIT => {
            let lim = st.summons.limits.entry(owner).or_default();
            lim.raised = DEFAULT_LIMIT + lv.limit_quantity as i64;
            lim.raised_until = Some(now + Duration::from_millis(lv.duration_ms.max(1)));
            info!("[invocation] {} : {} relève la limite des invocations à {}", st.players[&sid].ch.name, t.name, lim.raised);
            announce_limit(st, owner, 0);
        }
        skill_kind::SUMMONS | skill_kind::SCROLL | skill_kind::FIXED | skill_kind::PET if lv.summons_tid > 0 => {
            let count = lv.summons_count.max(1) as usize;
            make_room(st, owner, t.summons, t.tid, lv, count, now);
            for _ in 0..count {
                spawn(ctx, st, sid, &Spawn { tid: lv.summons_tid, skill_kind: t.summons, skill_tid: t.tid, duration_ms: duration, item: None }, now);
            }
        }
        other => {
            info!("[invocation] {} lance {} : invocation de nature {other} non prise en charge", st.players[&sid].ch.name, t.name);
            send_system(st, sid, "Cette invocation n'est pas encore prise en charge par le serveur.");
        }
    }
}

/// Fait la place pour `count` nouvelles créatures : invocations ordinaires (limite du maître, relevée au besoin jusqu'au
/// nombre invoqué), créatures fixes (`limit_quantity` de la compétence), parchemin (un par compétence), familier (un).
fn make_room(st: &mut WorldState, owner: u32, skill: i64, skill_tid: u32, lv: &SkillLevel, count: usize, now: Instant) {
    let mine = st.summons.of_owner(owner);
    let (existing, cap): (Vec<u32>, usize) = match skill {
        skill_kind::SUMMONS => {
            let cap = st.summons.limits.get(&owner).copied().unwrap_or_default().base(now).max(count as i64);
            (mine.iter().filter(|s| s.kind == kind::SUMMONS && !s.force).map(|s| s.cid).collect(), cap as usize)
        }
        skill_kind::FIXED => (mine.iter().filter(|s| s.kind == kind::FIXED && s.skill_tid == skill_tid).map(|s| s.cid).collect(), lv.limit_quantity.max(1) as usize),
        skill_kind::SCROLL => (mine.iter().filter(|s| s.force && s.kind == kind::SUMMONS && s.skill_tid == skill_tid).map(|s| s.cid).collect(), count),
        _ => (mine.iter().filter(|s| s.kind == kind::PET).map(|s| s.cid).collect(), count),
    };
    for cid in to_dismiss(&existing, count, cap).to_vec() {
        dismiss(st, cid, "limite atteinte, remplacée", true);
    }
    if skill == skill_kind::SUMMONS {
        announce_limit(st, owner, cap as i64);
    }
}

/// Annonce au client la limite des invocations ordinaires si elle change : `at_least` (nombre invoqué d'un coup) ou
/// la limite de base. Le client compte en plus un emplacement par invocation « forcée » présente.
fn announce_limit(st: &mut WorldState, owner: u32, at_least: i64) {
    let now = Instant::now();
    let forced = st.summons.list.values().filter(|s| s.owner == owner && s.force).count() as i64;
    let lim = st.summons.limits.entry(owner).or_default();
    let cap = lim.base(now).max(at_least);
    if cap == lim.sent_or_default() {
        return;
    }
    lim.sent = cap;
    if let Some(p) = st.by_cid.get(&owner).and_then(|sid| st.players.get(sid)) {
        p.send_raw(limit_packet(owner, cap + forced));
        debug!("[invocation] {} : limite des invocations {cap} (+{forced} forcée(s))", p.ch.name);
    }
}

/// AGPMITEM op 3 d'un objet à compétence d'invocation : familier (renvoyé si l'objet le tient déjà), parchemin (un
/// exemplaire consommé).
pub async fn use_item(ctx: &WorldCtx, st: &mut WorldState, sid: u32, item: &InventoryItem, u: super::items::UseSkill, now: Instant) -> anyhow::Result<()> {
    let Some(owner) = owner_cid(st, sid) else { return Ok(()) };
    let Some((t, lv)) = ctx.skills.get(u.skill_tid).and_then(|t| Some((t, t.level(u.level).or_else(|| t.levels.first())?))) else { return Ok(()) };
    // Familier déjà sorti par cet objet : l'utiliser de nouveau le renvoie.
    if let Some(s) = st.summons.list.values().find(|s| s.owner == owner && s.item == Some(item.iid)).cloned() {
        if u.can_stop || t.summons == skill_kind::PET {
            dismiss(st, s.cid, "renvoyée par son objet", true);
            return Ok(());
        }
    }
    if lv.summons_tid == 0 {
        return Ok(());
    }
    if t.summons == skill_kind::SCROLL {
        let p = st.players.get_mut(&sid).unwrap();
        let cid = p.ch.id;
        match p.inventory.consume(&ctx.db, item.iid, 1).await? {
            Some(left) => p.send_raw(super::items::item_count(&left, cid)),
            None => p.send_raw(super::items::item_remove(item.iid, Some(cid))),
        }
    }
    let count = lv.summons_count.max(1) as usize;
    make_room(st, owner, t.summons, t.tid, lv, count, now);
    let duration = (lv.duration_ms > 0 && t.summons != skill_kind::PET).then_some(lv.duration_ms);
    let keep = (t.summons == skill_kind::PET).then_some(item.iid);
    for _ in 0..count {
        spawn(ctx, st, sid, &Spawn { tid: lv.summons_tid, skill_kind: t.summons, skill_tid: t.tid, duration_ms: duration, item: keep }, now);
    }
    Ok(())
}

/// AgpmSummons reçu du client : seule l'op 2 (propension) en vient.
pub fn on_packet(st: &mut WorldState, sid: u32, p: &Packet) {
    match p.int(0) {
        Some(summons_op::SET_PROPENSITY) => set_propensity(st, sid, p.int(1).unwrap_or(0), p.int(2).unwrap_or(0) as u32, p.int(5).unwrap_or(-1)),
        other => debug!("[invocation] #{sid} AgpmSummons op {other:?} (ignoré)"),
    }
}

/// Propension d'une invocation de combat de son maître ; renvoyée au maître (le client ne change son étiquette qu'à la
/// réception de l'op 2).
fn set_propensity(st: &mut WorldState, sid: u32, owner: i64, cid: u32, value: i64) {
    let Some(me) = owner_cid(st, sid) else { return };
    let ok = owner == me as i64 && (propensity::ATTACK..=propensity::SHADOW).contains(&value);
    let Some(s) = st.summons.list.get_mut(&cid).filter(|s| ok && s.owner == me && s.kind == kind::SUMMONS) else {
        debug!("[invocation] #{sid} propension {value} refusée pour {cid:#x}");
        return;
    };
    s.propensity = value;
    if value == propensity::SHADOW {
        if let Some(m) = st.monsters.get_mut(cid) {
            m.target = None;
        }
    }
    let p = &st.players[&sid];
    p.send_raw(propensity_packet(me, cid, value));
    info!("[invocation] {} : propension de {cid:#x} à {value}", p.ch.name);
}

// ---------------------------------------------------------------------------------------------
// IA
// ---------------------------------------------------------------------------------------------

/// Cible d'une invocation.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Foe {
    Monster(u32),
    Player(u32),
}

/// Un tour des invocations : fins (durée, maître parti, limite relevée échue), suivi du maître, combat.
pub async fn step(ctx: &WorldCtx, st: &mut WorldState, now: Instant) -> anyhow::Result<()> {
    // Limites relevées échues : retour à la limite de base, les plus anciennes en trop disparaissent.
    let lapsed: Vec<u32> = st.summons.limits.iter().filter(|(_, l)| l.raised_until.is_some_and(|t| now >= t)).map(|(o, _)| *o).collect();
    for owner in lapsed {
        if let Some(l) = st.summons.limits.get_mut(&owner) {
            l.raised_until = None;
        }
        let ordinary: Vec<u32> = st.summons.of_owner(owner).iter().filter(|s| s.kind == kind::SUMMONS && !s.force).map(|s| s.cid).collect();
        for cid in to_dismiss(&ordinary, 0, DEFAULT_LIMIT as usize).to_vec() {
            dismiss(st, cid, "fin de la limite relevée", true);
        }
        announce_limit(st, owner, 0);
    }
    let cids: Vec<u32> = st.summons.list.keys().copied().collect();
    for cid in cids {
        let Some(s) = st.summons.list.get(&cid).cloned() else { continue };
        let Some(osid) = st.by_cid.get(&s.owner).copied().filter(|o| st.players.get(o).is_some_and(|p| p.in_game)) else {
            dismiss(st, cid, "maître absent", true);
            continue;
        };
        if s.ends_at.is_some_and(|t| now >= t) {
            dismiss(st, cid, "durée écoulée", true);
            continue;
        }
        let Some(m) = st.monsters.get(cid).filter(|m| m.is_alive()).cloned() else {
            if st.monsters.get(cid).is_none() {
                st.summons.list.remove(&cid);
            }
            continue;
        };
        if now < m.ready_at || st.monsters.status(cid, now) & status::HELPLESS != 0 {
            continue;
        }
        match s.kind {
            kind::PET => follow(ctx, st, cid, &m, osid, now),
            kind::FIXED => {
                let foe = trap_target(st, &s, &m);
                match foe {
                    Some(t) => strike(ctx, st, cid, &m, &s, osid, Foe::Monster(t), now).await?,
                    None => st.monsters.get_mut(cid).unwrap().ready_at = now + THINK,
                }
            }
            _ => match choose(ctx, st, &s, &m, osid) {
                Some(foe) => strike(ctx, st, cid, &m, &s, osid, foe, now).await?,
                None => follow(ctx, st, cid, &m, osid, now),
            },
        }
    }
    Ok(())
}

/// Garde la créature près de son maître : marche jusqu'à `FOLLOW_DISTANCE` de lui quand il s'éloigne, saut au-delà de
/// `TELEPORT_DISTANCE`.
fn follow(ctx: &WorldCtx, st: &mut WorldState, cid: u32, m: &Monster, osid: u32, now: Instant) {
    let at = st.players[&osid].ch.pos();
    let d = combat::dist2d(m.pos, at);
    if let Some(mm) = st.monsters.get_mut(cid) {
        mm.target = None;
        mm.ready_at = now + THINK;
    }
    if d > TELEPORT_DISTANCE {
        jump(ctx, st, cid, beside(at, m.pos));
        return;
    }
    if d <= FOLLOW_DISTANCE + FOLLOW_SLACK {
        return;
    }
    let dest = beside(at, m.pos);
    // Déjà en route vers une place encore bonne : rien à renvoyer.
    if st.monsters.walking.get(&cid).is_some_and(|w| combat::dist2d(w.dest, dest) < FOLLOW_SLACK) {
        return;
    }
    let run = st.monsters.stats_for(m).run;
    super::ai::walk(&ctx.map, st, cid, dest, run);
}

/// Cible d'une invocation de combat selon sa propension.
fn choose(ctx: &WorldCtx, st: &WorldState, s: &Summon, m: &Monster, osid: u32) -> Option<Foe> {
    if s.propensity == propensity::SHADOW {
        return None;
    }
    let p = &st.players[&osid];
    let at = p.ch.pos();
    // Cible du maître : monstre vivant et attaquable (pas l'une de ses invocations), ou joueur qu'il peut attaquer.
    let master = p.attack.and_then(|a| {
        if let Some(t) = st.monsters.get(a.target).filter(|t| t.is_alive() && combat::dist2d(t.pos, at) <= ASSIST_RANGE) {
            let ok = st.monsters.stats_for(t).is_attackable() && may_attack(ctx, st, osid, t.cid, a.forced).is_ok();
            return ok.then_some(a.target);
        }
        st.by_cid.get(&a.target).filter(|t| super::pvp::check(ctx, st, osid, **t, a.forced).is_ok()).map(|_| a.target)
    });
    // Agresseurs : monstres qui visent le maître ou l'invocation, joueurs qui les attaquent (et que le maître peut
    // attaquer sans Ctrl).
    let mut attackers: Vec<(u32, f64)> = st
        .monsters
        .engaged
        .iter()
        .filter_map(|c| st.monsters.get(*c))
        .filter(|t| t.is_alive() && (t.target == Some(s.owner) || t.target == Some(s.cid)) && combat::dist2d(t.pos, at) <= ASSIST_RANGE)
        .map(|t| (t.cid, combat::dist2d(t.pos, m.pos)))
        .collect();
    for (tsid, t) in st.players.iter().filter(|(t, x)| **t != osid && x.in_game && !x.dead) {
        let hits = t.attack.is_some_and(|a| a.target == s.owner || a.target == s.cid) || m.aggro.iter().any(|a| a.cid == t.ch.id);
        if hits && super::pvp::check(ctx, st, osid, *tsid, false).is_ok() {
            attackers.push((t.ch.id, combat::dist2d(t.ch.pos(), m.pos)));
        }
    }
    let cid = pick(s.propensity, master, m.target, &attackers)?;
    Some(match st.by_cid.get(&cid) {
        Some(tsid) => Foe::Player(*tsid),
        None => Foe::Monster(cid),
    })
}

/// Cible d'une créature fixe : un monstre vivant à sa portée, ceux qui visent son maître d'abord, puis le plus proche.
fn trap_target(st: &WorldState, s: &Summon, m: &Monster) -> Option<u32> {
    let range = st.monsters.stats_for(m).range as f64;
    if range <= 0.0 {
        return None;
    }
    st.monsters
        .near(m.pos, range + 500.0)
        .filter(|t| t.is_alive() && t.owner.is_none() && st.monsters.stats_of(t.tid).is_attackable())
        .filter(|t| combat::dist2d(t.pos, m.pos) <= range + st.monsters.stats_of(t.tid).hit_range as f64 + REACH_TOLERANCE)
        .min_by(|a, b| (a.target != Some(s.owner)).cmp(&(b.target != Some(s.owner))).then(combat::dist2d(a.pos, m.pos).total_cmp(&combat::dist2d(b.pos, m.pos))))
        .map(|t| t.cid)
}

/// Poursuite de la cible jusqu'à la portée de la créature (sauf créature fixe), puis coup normal au rythme de sa vitesse
/// d'attaque. Un monstre tué l'est par le maître (expérience, butin, quêtes) ; un joueur, selon le combat entre joueurs.
#[allow(clippy::too_many_arguments)]
async fn strike(ctx: &WorldCtx, st: &mut WorldState, cid: u32, m: &Monster, s: &Summon, osid: u32, foe: Foe, now: Instant) -> anyhow::Result<()> {
    let stats = st.monsters.stats_for(m);
    let (tpos, tlevel, hit_range, defender) = match foe {
        Foe::Monster(t) => {
            let Some(tm) = st.monsters.get(t).filter(|x| x.is_alive()) else { return Ok(()) };
            let ts = st.monsters.stats_for(tm);
            (tm.pos, tm.level as i32, ts.hit_range as f64, Defender::of_monster(tm.level as i32, &ts, &st.monsters.mods(t)))
        }
        Foe::Player(tsid) => {
            let Some(tp) = st.players.get(&tsid).filter(|x| x.in_game && !x.dead) else { return Ok(()) };
            (tp.ch.pos(), tp.factors.status[7], PLAYER_HIT_RANGE, Defender::of_player(&tp.factors))
        }
    };
    let target_cid = match foe {
        Foe::Monster(t) => t,
        Foe::Player(tsid) => st.players[&tsid].ch.id,
    };
    if let Some(mm) = st.monsters.get_mut(cid) {
        mm.target = Some(target_cid);
    }
    let state = st.monsters.status(cid, now);
    let reach = stats.range.max(50) as f64 + hit_range;
    let distance = combat::dist2d(m.pos, tpos);
    if distance > reach + REACH_TOLERANCE {
        if s.kind == kind::FIXED || state & status::NO_MOVE != 0 {
            st.monsters.get_mut(cid).unwrap().ready_at = now + THINK;
            return Ok(());
        }
        let k = ((distance - reach * 0.8) / distance).max(0.0) as f32;
        let dest = Vec3::new(m.pos.x + (tpos.x - m.pos.x) * k, tpos.y, m.pos.z + (tpos.z - m.pos.z) * k);
        let run = effects::speed(stats.run, st.monsters.mods(cid).movement_pct);
        let t = super::ai::walk(&ctx.map, st, cid, dest, run);
        st.monsters.get_mut(cid).unwrap().ready_at = now + t.min(Duration::from_secs(1));
        return Ok(());
    }
    if state & status::NO_ATTACK != 0 {
        st.monsters.get_mut(cid).unwrap().ready_at = now + THINK;
        return Ok(());
    }
    let mods = st.monsters.mods(cid);
    let (min_damage, max_damage) = effects::monster_damage(stats.min_damage, stats.max_damage, &mods);
    let attacker = Attacker { level: m.level as i32, ar: stats.ar, min_damage, max_damage, ..Default::default() };
    let (outcome, parts) = {
        let mut rng = rand::thread_rng();
        let (outcome, physical) = combat::strike_at(&mut rng, combat::monster_hit_chance(&attacker, &defender), &attacker, &defender);
        let mut parts = vec![(combat::RESULT_PHYSICAL, physical)];
        if outcome == combat::result::SUCCESS {
            let spirit = stats.spirit.map(|(lo, hi)| (lo as f64, hi.max(lo) as f64));
            parts.extend(combat::spirit_hits(&mut rng, combat::spirit_chance(false, m.level as i32, tlevel), &spirit, &defender.resist, matches!(foe, Foe::Player(_))));
        }
        (outcome, parts)
    };
    let damage: i32 = parts.iter().map(|(_, d)| d).sum();
    let speed = effects::speed(stats.attack_speed, mods.atk_speed_pct).max(1) as u64;
    st.monsters.get_mut(cid).unwrap().ready_at = now + Duration::from_millis(60_000 / speed);
    st.monsters.walking.remove(&cid);
    match foe {
        Foe::Monster(t) => {
            let tm = st.monsters.get_mut(t).unwrap();
            tm.hp = (tm.hp - damage).max(0);
            let (hp, tname) = (tm.hp, tm.name.clone());
            let max_mp = st.monsters.stats_for(st.monsters.get(t).unwrap()).max_mp;
            if damage > 0 {
                st.monsters.wake(t);
            }
            let watchers = combat::viewers(st, t);
            combat::broadcast(st, &watchers, &combat::action_packet(cid, t, outcome, (damage > 0).then(|| combat::parts_factors(hp, max_mp, &parts))));
            if damage > 0 {
                super::ai::add_aggro(st, t, cid, damage as u64, now);
            }
            debug!("[invocation] {} ({cid:#x}) → {tname} : résultat {outcome}, {damage} dégât(s), {hp} PV", m.name);
            if hp == 0 {
                // Tué par le maître ; son attaque en cours sur un autre monstre continue.
                let before = st.players.get(&osid).and_then(|p| p.attack);
                combat::kill(ctx, st, osid, t, now).await?;
                if let (Some(a), Some(p)) = (before.filter(|a| a.target != t), st.players.get_mut(&osid)) {
                    p.attack = Some(a);
                }
            }
        }
        Foe::Player(tsid) => {
            if st.players[&tsid].invincible_until.is_some_and(|u| now < u) {
                return Ok(());
            }
            combat::hit_player(st, tsid, cid, outcome, &parts);
            super::pvp::on_hit(ctx, st, osid, tsid, now);
            if st.players.get(&tsid).is_some_and(|x| x.dead) {
                super::pvp::on_kill(ctx, st, osid, tsid).await?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use archcore_net::decode_packet;

    fn griolli() -> MonsterStats {
        // Griolli (TID 543) : modèle de niveau 46.
        let mut spirit = [(0, 0); 8];
        spirit[1] = (24, 26);
        MonsterStats { max_hp: 3775, max_mp: 872, ar: 126, min_damage: 250, max_damage: 262, ac: 26, attack_speed: 36, range: 1500, run: 12000, char_type: 196_612, exp: 100, geld_max: 50, spirit, level: 46, ..Default::default() }
    }

    #[test]
    fn mise_a_l_echelle() {
        let s = scaled_stats(&griolli(), 23, 100.0, 0.0);
        assert_eq!((s.max_hp, s.min_damage, s.max_damage, s.ar, s.ac, s.level), (1888, 125, 131, 63, 13, 23), "moitié du niveau du modèle");
        assert_eq!((s.range, s.run, s.attack_speed, s.char_type), (1500, 12000, 36, 196_612), "vitesses, portée et type gardés");
        assert_eq!((s.exp, s.geld_max, s.spirit[1]), (0, 0, (12, 13)), "ni expérience ni or");
        let full = scaled_stats(&griolli(), 46, 200.0, 5.0);
        assert_eq!((full.max_hp, full.min_damage), (7928, 500), "× 2 (Summons.StatScale) puis +5 % de PV (Elemental Heal)");
        assert_eq!(scaled_stats(&griolli(), 1, 1.0, 0.0).max_hp, 1, "jamais 0 PV");
    }

    #[test]
    fn natures_d_invocation() {
        assert_eq!(packet_kind(skill_kind::SUMMONS), Some((kind::SUMMONS, 0x800)));
        assert_eq!(packet_kind(skill_kind::SCROLL), Some((kind::SUMMONS, 0x800)));
        assert_eq!(packet_kind(skill_kind::FIXED), Some((kind::FIXED, 0xC00)));
        assert_eq!(packet_kind(skill_kind::PET), Some((kind::PET, 0x800)));
        assert_eq!(packet_kind(skill_kind::LIMIT), None);
        assert_eq!(packet_kind(7), None);
        assert_eq!(state_of(kind::TAME), 0x1000);
    }

    #[test]
    fn limite_les_plus_anciennes_partent() {
        assert_eq!(to_dismiss(&[1], 1, 1), &[1], "limite 1 : l'ancienne part");
        assert!(to_dismiss(&[1], 1, 3).is_empty());
        assert_eq!(to_dismiss(&[1, 2, 3], 2, 3), &[1, 2]);
        assert_eq!(to_dismiss(&[1, 2], 0, 1), &[1], "limite revenue à 1");
        assert_eq!(to_dismiss(&[1, 2], 5, 3), &[1, 2], "jamais plus que les présentes");
    }

    #[test]
    fn choix_de_la_cible() {
        let attackers = [(10, 500.0), (11, 200.0)];
        assert_eq!(pick(propensity::ATTACK, Some(7), None, &attackers), Some(7), "attaque : la cible du maître d'abord");
        assert_eq!(pick(propensity::ATTACK, None, None, &attackers), Some(11), "puis l'agresseur le plus proche");
        assert_eq!(pick(propensity::DEFENSE, Some(7), None, &attackers), Some(11), "défense : jamais la cible du maître");
        assert_eq!(pick(propensity::DEFENSE, None, Some(10), &attackers), Some(10), "garde sa cible tant qu'elle attaque");
        assert_eq!(pick(propensity::DEFENSE, None, Some(99), &[]), None, "personne ne l'attaque");
        assert_eq!(pick(propensity::SHADOW, Some(7), Some(10), &attackers), None, "ombre : ne combat pas");
    }

    #[test]
    fn paquets() {
        let s = Summon { cid: 0x5800_0001, owner: 3, tid: 543, kind: kind::SUMMONS, skill_tid: 315, propensity: 1, force: false, time_a: 1000, time_b: 601_000, ends_at: None, item: None, seq: 0 };
        let add = decode_packet(&schema::SUMMONS, &add_packet(&s)).unwrap();
        assert_eq!((add.int(0), add.int(1), add.int(2), add.int(3), add.int(4), add.int(5)), (Some(0), Some(3), Some(0x5800_0001), Some(543), Some(1), Some(1)));
        assert_eq!((add.int(6), add.int(7), add.int(8)), (Some(1000), Some(601_000), Some(0)));
        let pet = decode_packet(&schema::SUMMONS, &add_packet(&Summon { kind: kind::PET, force: true, ..s.clone() })).unwrap();
        assert_eq!((pet.int(4), pet.int(8)), (Some(8), Some(1)), "familier forcé");
        let rm = decode_packet(&schema::SUMMONS, &remove_packet(3, 0x5800_0001)).unwrap();
        assert_eq!((rm.int(0), rm.int(1), rm.int(2), rm.int(3), rm.int(8)), (Some(1), Some(3), Some(0x5800_0001), None, Some(0)));
        let pr = decode_packet(&schema::SUMMONS, &propensity_packet(3, 0x5800_0001, 2)).unwrap();
        assert_eq!((pr.int(0), pr.int(5), pr.int(8)), (Some(2), Some(2), Some(0)));
        let lim = decode_packet(&schema::SUMMONS, &limit_packet(3, 3)).unwrap();
        assert_eq!((lim.int(0), lim.int(1), lim.int(4)), (Some(3), Some(3), Some(3)));
        // Vue d'un autre monstre : rien de plus ; d'une invocation : état puis op 0.
        let mut all = Summons::default();
        all.list.insert(s.cid, s.clone());
        assert!(all.view_extras(0x5000_0001, 0).is_empty());
        let extras = all.view_extras(s.cid, 0x800);
        let state = decode_packet(&schema::CHARACTER, &extras[0]).unwrap();
        assert_eq!((state.int(0), state.int(1), state.int(17)), (Some(1), Some(0x5800_0001), Some(0x800)));
        assert_eq!(decode_packet(&schema::SUMMONS, &extras[1]).unwrap().int(0), Some(0));
    }

    #[test]
    fn limite_relevee() {
        let now = Instant::now();
        let mut l = Limits::default();
        assert_eq!((l.base(now), l.sent_or_default()), (1, 1));
        l.raised = 3;
        l.raised_until = Some(now + Duration::from_secs(60));
        assert_eq!(l.base(now), 3, "Increase Gate : 1 + limit_quantity");
        assert_eq!(l.base(now + Duration::from_secs(61)), 1, "échue");
    }

    #[test]
    fn objet_a_competence() {
        let doogie = serde_json::json!({"Use skill id":"970","Use skill level":"1","CanStopUsingItem":"1","Type":"12"});
        assert_eq!(super::super::items::UseSkill::from_raw(&doogie), Some(super::super::items::UseSkill { skill_tid: 970, level: 1, can_stop: true }));
        assert!(super::super::items::UseSkill::from_raw(&serde_json::json!({"Use skill id":""})).is_none());
    }
}
