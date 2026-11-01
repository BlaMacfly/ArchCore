//! Métamorphose : potions de métamorphose, Mutation, annulation.
//!
//! ```text
//!   Potion : objet d'itemdatatable.txt d'« ItemType » 1 et de « Type » 7 ; forme = « Polymorph Id » (TID d'un modèle de
//!            personnage ou de monstre), durée = « Polymorph Dur » (ms, 0 : jusqu'à l'annulation), niveau exigé = « Level ».
//!            Bonus d'ItemTransformTable.txt (table `item_transform`) ajoutés aux facteurs : PV et PM max, défense
//!            physique, dégâts physiques min et max, course (« Movement Run »), vitesse d'attaque.
//!   client → AGPMITEM op 3 (IID) : refusée par AGPMITEM op 27 (champ 1 = code, champ 4 = CID) : 2 en zone de siège en
//!            combat, 4 à cheval, déjà transformé ou évolué (le code 3, qui ouvre une fenêtre, n'est jamais envoyé) ;
//!            recharge commune de 10 min (constante du client) refusée par un message système.
//!            Acceptée : pile diminuée (op 2) ou objet retiré (op 1), op 4 (TID, CID ; le client lance son compte à
//!            rebours de la durée de l'objet), op 19 (champ 22 : recharge restante, 600 000 ms), facteurs (AGPMCHARACTER
//!            op 1), puis AGPMCHARACTER op 11 (champ 1 CID, champ 2 TID de la forme) au joueur et à ceux qui le voient.
//!   Vue : op 0 avec le TID d'origine et le bit 0 du champ 18, équipement, puis op 11 (le client ne garde le TID d'origine,
//!         qu'il rend à l'op 12, que s'il l'a reçu par l'op 11).
//!   Fin : durée écoulée, annulation (client → AGPMCHARACTER op 13, champ 1 = son CID), mort → op 12 (CID) à tous,
//!         facteurs d'origine. Rien n'est enregistré : la forme se perd à la déconnexion.
//!   Mutation (compétence de `transform_target` 4) : la cible, un joueur, prend 7 s (« duration ») une forme tirée au
//!         hasard dans `creature_tid` du niveau ; sans bonus ; l'annulation lui est refusée.
//! ```
//! Pendant la métamorphose (règles du client répétées par le serveur ; l'ArchLord y échappe, sauf pour une nouvelle
//! potion) : compétences, parchemins de téléportation, Potions of Quickness et Beam (type 12, sous-type 1 ou 2), portails,
//! inscription aux Shrines, dialogues et boutiques de PNJ, coiffeur, monture refusés ; attaques normales permises. La
//! classe reste celle d'origine (facteurs), la race est celle de la forme pour le combat entre joueurs (`race`).
//! Équipement (`FUN_1401bfa10` d'Archonia) : sous métamorphose ou évolution, le client contrôle un changement de place
//! d'objet (hors montures, « Kind » 0x20) contre la race, la classe et le sexe du modèle courant, si ce modèle porte un
//! drapeau dont le sens n'est pas établi. Faute de certitude pour les formes de monstres, ArchCore refuse tout
//! changement d'équipement pendant la métamorphose ; sous évolution, l'équipement se contrôle contre la forme (facteurs).

use super::items::{item_count, item_remove, InventoryItem};
use super::{send_system, Player, WorldCtx, WorldState};
use crate::character::{factors_mini, Factors, POINT_HP};
use crate::{debug, info};
use archcore_net::schema;
use archcore_net::{build_packet, Fields, Packet};
use rand::seq::SliceRandom;
use std::time::{Duration, Instant};

/// Bits du champ 18 d'AGPMCHARACTER (« transformation ») : métamorphose, évolution (`ride::TRANSFORM_RIDING` : bit 1).
pub const TRANSFORMED: i64 = 0x01;
pub const EVOLVED: i64 = 0x04;

/// Opérations AGPMCHARACTER de la métamorphose et de l'évolution.
pub mod op {
    pub const TRANSFORM: i64 = 11;
    pub const RESTORE_TRANSFORM: i64 = 12;
    /// Client → serveur : annulation demandée par l'icône « Cancel Metamorphosis ».
    pub const CANCEL_TRANSFORM: i64 = 13;
    pub const EVOLUTION: i64 = 26;
    pub const RESTORE_EVOLUTION: i64 = 27;
}

/// Opérations AGPMITEM : recharge des potions de métamorphose, refus d'une utilisation.
pub mod item_op {
    pub const UPDATE_REUSE_TIME_FOR_TRANSFORM: i64 = 19;
    pub const USE_ITEM_RESULT: i64 = 27;
}

/// Codes d'AGPMITEM op 27 (champ 1) : 2 « UseItemCannotTransformCreature » (siège), 4 « UseItemCannotTransformPotion ».
pub mod use_result {
    pub const SIEGE: i64 = 2;
    pub const CANNOT_TRANSFORM: i64 = 4;
}

/// Type d'usage (« Type » d'un objet d'« ItemType » 1) des potions de métamorphose, des Potions of Quickness et Beam
/// (sous-type 1 et 2 ; les ragoûts, sous-type 0, ont le même type) et des objets d'oubli des compétences.
pub const USABLE_MORPH: i64 = 7;
pub const USABLE_SPEED: i64 = 12;
pub const USABLE_INIT_SKILL: i64 = 27;
/// Recharge commune des potions de métamorphose (constante du client).
pub const POTION_REUSE: Duration = Duration::from_millis(600_000);
/// Durée de la Mutation quand son niveau n'en donne pas.
pub const MUTATION_DURATION: Duration = Duration::from_millis(7000);

/// Bonus d'une potion (ItemTransformTable.txt), ajoutés aux facteurs ; négatifs possibles (course du squelette).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bonus {
    pub movement_run: i32,
    pub atk_speed: i32,
    pub max_hp: i32,
    pub max_mp: i32,
    pub defense: i32,
    pub min_damage: i32,
    pub max_damage: i32,
}

impl Bonus {
    /// Ajoute les bonus aux facteurs (modèle + équipement), PV et PM bornés au nouveau maximum.
    pub fn apply(&self, f: &mut Factors) {
        f.point_max[0] = (f.point_max[0] + self.max_hp).max(1);
        f.point_max[1] = (f.point_max[1] + self.max_mp).max(0);
        f.defense_point[0] = (f.defense_point[0] + self.defense).max(0);
        f.damage_min[0] = (f.damage_min[0] + self.min_damage).max(0);
        f.damage_max[0] = (f.damage_max[0] + self.max_damage).max(f.damage_min[0]);
        f.status[9] = (f.status[9] + self.movement_run).max(0);
        f.attack[2] = (f.attack[2] + self.atk_speed).max(1);
        f.point[POINT_HP] = f.point[POINT_HP].min(f.point_max[0]);
        f.point[1] = f.point[1].min(f.point_max[1]);
    }
}

/// Potion de métamorphose : forme, durée (0 : jusqu'à l'annulation), niveau exigé, bonus.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Potion {
    pub tid: u32,
    pub duration_ms: u64,
    pub level: i32,
    pub bonus: Bonus,
}

impl Potion {
    /// Lit une ligne d'itemdatatable.txt : « ItemType » 1, « Type » 7 et « Polymorph Id » non nul.
    pub fn from_raw(raw: &serde_json::Value) -> Option<Self> {
        let num = |k: &str| raw.get(k).and_then(|v| v.as_str()).and_then(|t| t.trim().parse::<f64>().ok()).unwrap_or(0.0);
        let tid = num("Polymorph Id");
        (super::items::usable_type_of(raw) == USABLE_MORPH && tid > 0.0)
            .then(|| Potion { tid: tid as u32, duration_ms: num("Polymorph Dur").max(0.0) as u64, level: num("Level") as i32, bonus: Bonus::default() })
    }
}

/// D'où vient la forme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Potion (TID de l'objet).
    Potion(u32),
    /// Mutation d'un autre joueur : pas d'annulation.
    Mutation,
}

/// Métamorphose en cours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Morph {
    pub tid: u32,
    pub source: Source,
    /// Fin ; `None` : jusqu'à l'annulation.
    pub ends_at: Option<Instant>,
    pub bonus: Bonus,
}

/// Forme d'un joueur : métamorphose, évolution du Scion (`evolution.rs`), recharge des potions.
#[derive(Clone, Debug, Default)]
pub struct Form {
    pub morph: Option<Morph>,
    pub evolved: Option<super::evolution::Evolved>,
    pub potion_ready_at: Option<Instant>,
}

impl Form {
    /// Bits du champ 18 : 0 métamorphose, 2 évolution.
    pub fn bits(&self) -> i64 {
        (if self.morph.is_some() { TRANSFORMED } else { 0 }) | (if self.evolved.is_some() { EVOLVED } else { 0 })
    }
}

/// Le joueur est sous métamorphose et n'est pas l'ArchLord : refus du client répétés par le serveur.
pub fn restricted(st: &WorldState, sid: u32) -> bool {
    st.players.get(&sid).is_some_and(|p| p.form.morph.is_some()) && !super::archlord::is_lord(st, sid)
}

/// Race du joueur pour les règles qui en dépendent (combat entre joueurs) : celle de la forme sous métamorphose.
pub fn race(ctx: &WorldCtx, p: &Player) -> i32 {
    p.form.morph.and_then(|m| ctx.templates.get(m.tid)).map(|t| t.race).filter(|r| *r > 0).unwrap_or(p.factors.ftype[0])
}

/// AGPMCHARACTER op 11 : `cid` prend la forme `tid`.
pub fn transform_packet(cid: u32, tid: u32) -> Vec<u8> {
    let c = &schema::CHARACTER;
    build_packet(c, &Fields::new(c).with(0, op::TRANSFORM).with(1, cid).with(2, tid))
}

/// AGPMCHARACTER op 12 : `cid` reprend son apparence.
pub fn restore_packet(cid: u32) -> Vec<u8> {
    let c = &schema::CHARACTER;
    build_packet(c, &Fields::new(c).with(0, op::RESTORE_TRANSFORM).with(1, cid))
}

/// AGPMITEM op 27 : utilisation refusée (`use_result`), au joueur `cid`.
pub fn result_packet(cid: u32, code: i64) -> Vec<u8> {
    let s = &schema::ITEM;
    build_packet(s, &Fields::new(s).with(0, item_op::USE_ITEM_RESULT).with(1, code).with(4, cid))
}

/// AGPMITEM op 19 : recharge restante des potions de métamorphose (ms).
pub fn reuse_packet(cid: u32, left: Duration) -> Vec<u8> {
    let s = &schema::ITEM;
    build_packet(s, &Fields::new(s).with(0, item_op::UPDATE_REUSE_TIME_FOR_TRANSFORM).with(4, cid).with(22, left.as_millis() as u64))
}

/// Op 11 d'un joueur transformé, pour un joueur qui le voit (après son personnage et son équipement).
pub fn view_packet(p: &Player) -> Option<Vec<u8>> {
    p.form.morph.map(|m| transform_packet(p.ch.id, m.tid))
}

/// Facteurs du joueur (AGPMCHARACTER op 1) à lui et à ceux qui le voient.
fn factors_around(st: &WorldState, sid: u32) {
    let Some(p) = st.players.get(&sid) else { return };
    let c = &schema::CHARACTER;
    super::combat::send_around(st, sid, &build_packet(c, &Fields::new(c).with(0, 1).with(1, p.ch.id).with(7, factors_mini(&p.factors))));
}

/// Refus d'une potion pour ce joueur : code d'AGPMITEM op 27, sinon `None`.
pub fn refusal(ctx: &WorldCtx, st: &WorldState, sid: u32) -> Option<i64> {
    let p = st.players.get(&sid)?;
    if super::siege::war::war_zone(ctx, st, sid).is_some() {
        return Some(use_result::SIEGE);
    }
    (super::ride::is_mounted(&p.inventory) || p.form.morph.is_some() || p.form.evolved.is_some()).then_some(use_result::CANNOT_TRANSFORM)
}

/// AGPMITEM op 3 sur une potion de métamorphose.
pub async fn use_potion(ctx: &WorldCtx, st: &mut WorldState, sid: u32, item: &InventoryItem, potion: Potion, now: Instant) -> anyhow::Result<()> {
    let Some(p) = st.players.get(&sid) else { return Ok(()) };
    let (cid, name) = (p.ch.id, p.ch.name.clone());
    let label = ctx.catalog.get(item.tid).map(|d| d.name.clone()).unwrap_or_default();
    if let Some(code) = refusal(ctx, st, sid) {
        info!("[métamorphose] {name} : {label} refusée (code {code})");
        p.send_raw(result_packet(cid, code));
        return Ok(());
    }
    if p.ch.level < potion.level {
        send_system(st, sid, &format!("Niveau {} requis pour {label}.", potion.level));
        return Ok(());
    }
    if let Some(left) = p.form.potion_ready_at.map(|t| t.saturating_duration_since(now)).filter(|d| !d.is_zero()) {
        debug!("[métamorphose] {name} : recharge, encore {} s", left.as_secs());
        send_system(st, sid, &format!("Métamorphose impossible pour encore {} min.", left.as_secs().div_ceil(60)));
        return Ok(());
    }
    if ctx.templates.get(potion.tid).is_none() {
        debug!("[métamorphose] {name} : forme {} inconnue", potion.tid);
        return Ok(());
    }
    let player = st.players.get_mut(&sid).unwrap();
    match player.inventory.consume(&ctx.db, item.iid, 1).await? {
        Some(left) => player.send_raw(item_count(&left, cid)),
        None => player.send_raw(item_remove(item.iid, Some(cid))),
    }
    player.send_raw(super::recovery::used_packet(item.tid, cid, POTION_REUSE));
    player.form.potion_ready_at = Some(now + POTION_REUSE);
    player.send_raw(reuse_packet(cid, POTION_REUSE));
    let cap = ctx.config.world.item_transform_max_ms;
    let ms = if cap > 0 && (potion.duration_ms == 0 || potion.duration_ms > cap) { cap } else { potion.duration_ms };
    let ends_at = (ms > 0).then(|| now + Duration::from_millis(ms));
    let bonus = ctx.catalog.morph.get(&item.tid).map(|p| p.bonus).unwrap_or_default();
    start(ctx, st, sid, Morph { tid: potion.tid, source: Source::Potion(item.tid), ends_at, bonus });
    info!("[métamorphose] {name} boit {label} : forme {} pour {}", potion.tid, if ms > 0 { format!("{ms} ms") } else { "une durée illimitée".into() });
    Ok(())
}

/// Pose la forme : facteurs recalculés (bonus), puis op 11 au joueur et à ceux qui le voient.
fn start(ctx: &WorldCtx, st: &mut WorldState, sid: u32, m: Morph) {
    let Some(p) = st.players.get_mut(&sid) else { return };
    p.form.morph = Some(m);
    // Une incantation en cours tombe : les compétences sont refusées sous métamorphose.
    p.skills.cast = None;
    super::equipment::recompute_factors(ctx, p);
    let cid = p.ch.id;
    factors_around(st, sid);
    super::combat::send_around(st, sid, &transform_packet(cid, m.tid));
}

/// Fin de la métamorphose : facteurs d'origine, op 12 à tous.
pub fn restore(ctx: &WorldCtx, st: &mut WorldState, sid: u32, why: &str) {
    let Some(p) = st.players.get_mut(&sid) else { return };
    if p.form.morph.take().is_none() {
        return;
    }
    super::equipment::recompute_factors(ctx, p);
    let (cid, name) = (p.ch.id, p.ch.name.clone());
    factors_around(st, sid);
    super::combat::send_around(st, sid, &restore_packet(cid));
    info!("[métamorphose] {name} reprend son apparence ({why})");
}

/// AGPMCHARACTER op 13 reçu : annulation demandée par le joueur (refusée pour une Mutation subie).
pub fn on_cancel(ctx: &WorldCtx, st: &mut WorldState, sid: u32, packet: &Packet) {
    let Some(p) = st.players.get(&sid) else { return };
    if packet.int(1).is_some_and(|c| c as u32 != p.ch.id) {
        debug!("[métamorphose] #{sid} annulation pour un autre CID ({:?}) : ignorée", packet.int(1));
        return;
    }
    match p.form.morph.map(|m| m.source) {
        Some(Source::Potion(_)) => restore(ctx, st, sid, "annulation"),
        Some(Source::Mutation) => debug!("[métamorphose] {} : annulation d'une Mutation refusée", p.ch.name),
        None => {}
    }
}

/// Fin des métamorphoses échues et de celles des morts (boucle du monde).
pub fn step(ctx: &WorldCtx, st: &mut WorldState, now: Instant) {
    let due: Vec<(u32, &'static str)> = st
        .players
        .iter()
        .filter_map(|(sid, p)| {
            let m = p.form.morph?;
            if p.dead {
                Some((*sid, "mort"))
            } else {
                m.ends_at.filter(|t| now >= *t).map(|_| (*sid, "durée écoulée"))
            }
        })
        .collect();
    for (sid, why) in due {
        restore(ctx, st, sid, why);
    }
}

/// Forme tirée pour une Mutation : un TID de la liste du niveau que le serveur connaît.
pub fn pick_form(ctx: &WorldCtx, tids: &[u32], rng: &mut impl rand::Rng) -> Option<u32> {
    let known: Vec<u32> = tids.iter().copied().filter(|t| ctx.templates.get(*t).is_some()).collect();
    known.choose(rng).copied()
}

/// Mutation sur le joueur `tsid` : forme tirée dans `creature_tid`, pour `duration` (« duration » du niveau, 7 s à
/// défaut). Un cavalier descend d'abord (sinon rien) ; un joueur déjà transformé ou évolué, ou l'ArchLord, n'est pas
/// touché. Rend vrai si la forme est posée.
pub async fn mutate(ctx: &WorldCtx, st: &mut WorldState, tsid: u32, tids: &[u32], duration_ms: u64, now: Instant) -> anyhow::Result<bool> {
    let Some(v) = st.players.get(&tsid) else { return Ok(false) };
    if v.dead || v.form.morph.is_some() || v.form.evolved.is_some() || super::archlord::is_lord(st, tsid) {
        return Ok(false);
    }
    let Some(tid) = pick_form(ctx, tids, &mut rand::thread_rng()) else { return Ok(false) };
    if super::ride::is_mounted(&v.inventory) && !super::equipment::force_unequip(ctx, st, tsid, super::equipment::part::RIDE).await? {
        return Ok(false);
    }
    let life = if duration_ms > 0 { Duration::from_millis(duration_ms) } else { MUTATION_DURATION };
    start(ctx, st, tsid, Morph { tid, source: Source::Mutation, ends_at: Some(now + life), bonus: Bonus::default() });
    info!("[métamorphose] {} : Mutation en {tid} pour {} ms", st.players[&tsid].ch.name, life.as_millis());
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use archcore_net::decode_packet;

    #[test]
    fn lecture_d_une_potion() {
        let spider = serde_json::json!({"ItemType":"1","Type":"7","Polymorph Id":"272","Polymorph Dur":"240000","Level":"1"});
        assert_eq!(Potion::from_raw(&spider), Some(Potion { tid: 272, duration_ms: 240_000, level: 1, bonus: Bonus::default() }));
        let event = serde_json::json!({"ItemType":"1","Type":"7","Polymorph Id":"1276","Polymorph Dur":"","Level":""});
        assert_eq!(Potion::from_raw(&event).map(|p| (p.tid, p.duration_ms)), Some((1276, 0)), "durée vide : jusqu'à l'annulation");
        assert!(Potion::from_raw(&serde_json::json!({"ItemType":"1","Type":"7","Polymorph Id":""})).is_none());
        assert!(Potion::from_raw(&serde_json::json!({"ItemType":"1","Type":"12","Polymorph Id":"5"})).is_none());
    }

    #[test]
    fn bonus_ajoutes_aux_facteurs() {
        let mut f = Factors::default();
        f.point_max[0] = 400;
        f.point[POINT_HP] = 400;
        f.status[9] = 5000;
        f.attack[2] = 60;
        f.damage_min[0] = 10;
        f.damage_max[0] = 12;
        // 6118 Archlord Potion : PV +700, défense +60, dégâts +200 / +220 ; 402 Skeleton : course -1000 ; 4620 : vitesse -12.
        Bonus { max_hp: 700, defense: 60, min_damage: 200, max_damage: 220, ..Default::default() }.apply(&mut f);
        assert_eq!((f.point_max[0], f.point[POINT_HP], f.defense_point[0], f.damage_min[0], f.damage_max[0]), (1100, 400, 60, 210, 232));
        Bonus { movement_run: -1000, atk_speed: -12, max_hp: -2000, ..Default::default() }.apply(&mut f);
        assert_eq!((f.status[9], f.attack[2], f.point_max[0], f.point[POINT_HP]), (4000, 48, 1, 1), "négatifs : PV bornés à 1");
    }

    #[test]
    fn paquets_et_bits() {
        let p = decode_packet(&schema::CHARACTER, &transform_packet(7, 272)).unwrap();
        assert_eq!((p.int(0), p.int(1), p.int(2)), (Some(11), Some(7), Some(272)));
        let p = decode_packet(&schema::CHARACTER, &restore_packet(7)).unwrap();
        assert_eq!((p.int(0), p.int(1), p.int(2)), (Some(12), Some(7), None));
        let p = decode_packet(&schema::ITEM, &result_packet(7, use_result::CANNOT_TRANSFORM)).unwrap();
        assert_eq!((p.int(0), p.int(1), p.int(4)), (Some(27), Some(4), Some(7)));
        let p = decode_packet(&schema::ITEM, &reuse_packet(7, POTION_REUSE)).unwrap();
        assert_eq!((p.int(0), p.int(4), p.int(22)), (Some(19), Some(7), Some(600_000)));
        let mut form = Form::default();
        assert_eq!(form.bits(), 0);
        form.morph = Some(Morph { tid: 272, source: Source::Mutation, ends_at: None, bonus: Bonus::default() });
        assert_eq!(form.bits(), TRANSFORMED);
        form.evolved = Some(super::super::evolution::Evolved { tid: 1722, skill_tid: 1189, skill_id: 3 });
        assert_eq!(form.bits(), TRANSFORMED | EVOLVED);
    }
}
