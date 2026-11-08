//! Effets de compétence que les tables de l'épisode 8 décrivent sans que le client en joue la règle : déplacements
//! forcés, mort instantanée, zones au sol, enchaînements, compétences additionnelles, bonus de niveau, détection,
//! résurrection d'autrui, provocation, cibles interdites.
//!
//! ```text
//!   Déplacement forcé (`move_pos` de SKILL_SPEC) : 3 la cible (un monstre) est posée au contact du lanceur à l'impact
//!     (Strike of Judgement, Forced Battle, Extreme Blade) ; 2 le lanceur (un monstre : Lightning Rush) est posé au contact
//!     de sa cible avant l'impact. Recul (« move_distance » : Body Check) : la cible reculée de move_distance ×
//!     `Rules.KnockbackUnit` (100) unités dans l'axe lanceur → cible. Boss de la fenêtre des boss et objets de siège :
//!     jamais déplacés. Paquet : arrêt net à la nouvelle place (AGPMOPTIMIZEDCHARMOVE). Un joueur n'est pas déplacé.
//!   Death Strike (passive, `melee_attack` 107, « death » 15 à 90 %) : chaque coup normal qui touche un monstre ordinaire
//!     (ni boss, ni objet de siège, ni invocation, ni joueur) le tue avec cette chance : PV à 0, résultat 13
//!     (`Rules.DeathStrike`).
//!   Zone au sol (`at_field` : 10001, 10002 attaque, 10003 aura) : pendant la durée, toutes les « interval » ms, les
//!     monstres à moins de « area_r » du centre prennent les dégâts fixes « ea_damage_* » (moins leur résistance à
//!     l'élément, 1 au moins), ou, pour une aura, le maître et son groupe y regagnent « regen_hp » PV et « regen_mp » PM.
//!     Centre : le lanceur pour une compétence sur soi (Flame Force, Aura of Decay : elle le suit), la créature pour une
//!     invocation (pièges Lightning Trap, Tuuga, Holy Ground : compétence additionnelle de l'invocation). « skill_rate » :
//!     chance de chaque période. Les monstres touchés prennent le maître pour agresseur ; il en a l'expérience.
//!   Compétence additionnelle (« additional_skilltid », « additional_duration » s) :
//!     invocation : sa compétence additionnelle (TID de son lanceur) est une zone (ci-dessus) ou une passive à chance
//!       (« skill_rate ») dont la propre compétence additionnelle (affaiblissement : « <Summons>Griolli <Additional
//!       Effect> ») se pose sur le monstre que la créature touche ;
//!     enchaînement 3 (`action_on_action` : Frost Armor, Absolute Territory, Deadman Awaken) : le monstre (ou le joueur)
//!       qui frappe le porteur reçoit l'effet de la compétence additionnelle (additional_duration s, sinon sa durée).
//!   Enchaînement 4 (Lens Stone) : l'effet tombe au premier coup normal du porteur qui touche.
//!   Enchaînement 1 (Time Attack, « Timed Attack ») : l'effet est posé sur la cible, les dégâts du niveau tombent à sa fin.
//!   Enchaînement 2 (Shadow Chaser, Ambush : « Teleport Attack », refusée sur un objet de siège) : le lanceur est posé au
//!     contact de sa cible avant l'impact.
//!   Enchaînement 6 (Bomba → VictimOfGlory : « by making your pet explode ») : à sa mort ou à la fin de sa durée, la créature
//!     frappe autour d'elle par la compétence additionnelle de sa passive. Sans objet : <Battle> et <Event> Ability, ni
//!     objet ni compétence ne les lance.
//!   Enchaînements 7 et 8 (combos du Scion, infobulles « The Skill Effect increases when the target is affected by X »,
//!     « … when you are invisible ») : les colonnes « …2 » de Skill_Const2 (second coup, modificateurs, dégâts sur la durée,
//!     étourdissement) ne valent que si la cible porte l'effet de « Connection_TID2 » (7) ou si le lanceur est invisible (8).
//!   PV absorbés (`convert_type` 4 : Life Extinction, coups normaux du porteur ; 6 : Vampiric Impact, son propre coup) :
//!     avec la chance « skill_rate », « damage_convert_hp » % des dégâts portés reviennent au lanceur.
//!   Effets visuels (AGPMSKILL op 10, types de `effects::additional`) : 1 fin de Time Attack, 2 déplacement forcé et attaque
//!     par téléportation, 4 PV absorbés, 6 coup changé en PV, 7 invisibilité posée (les autres clients lâchent leur cible
//!     et leur attaque sur ce personnage), 8 Lens Stone consommée, 9 détection posée, 10 purge amie. Le client ne fait rien
//!     du 5 ; aucune ligne des tables ne porte 3.
//!   Bonus de niveau (`skill_levelup` de SKILL_SPEC, valeur « skill_levelup » de SKILL_CONST) : 1 toutes les compétences
//!     (s'ajoute au bonus des options, AGPMSKILL op 14), 2 celles des arbres de maîtrise (choix : « même maîtrise »),
//!     4 celles de « specific_skill_levelup2 » ; tant que dure l'effet. « Skill_plus » des objets (champ 21) : l'option
//!     « skill_levelup » vaut toujours
//!     pour toutes les compétences. Masquerade (type 4) n'a pas de valeur : chaque compétence de sa liste gagne 1 (choix).
//!     Les types 2 et 4 partent par AGPMSKILL op 16, compétence par compétence : le client
//!     ajoute cette valeur au niveau de ses lancements.
//!   Détection (`detect`) : pendant l'effet, le porteur peut viser un joueur invisible (état 10) ; sans elle, refusé.
//!   Redemption (`resurrection` 1) : vise un joueur mort à portée ; AGPMCHARACTER op 24 à la cible (champ 13 : lanceur,
//!     champ 3 : son nom) → « … wants to resurrect you » ; réponse du client par l'op 24 (champ 4 à 0 : refus) ou l'op
//!     10, dans les 60 s : résurrection sur place, PV et PM pleins, sans perte d'expérience.
//!   Provocation (Provoke, Zone Provoke : sans ligne de SKILL_CONST, niveau implicite : portée d'attaque, zone de 600
//!     autour de la cible) : chaque monstre touché prend le lanceur pour cible pendant `Rules.ProvokeSeconds` (10) et lui
//!     ajoute une haine égale à la plus forte de sa liste + 1 ; un monstre berserk n'y est pas sensible.
//!   Cibles interdites (`target_restriction`, bits : 1 objet de siège, 2 monstre, 4 joueur ennemi) : refus au lancement,
//!     type d'action 7 (cible invalide).
//!   Interruption (`disturb`, modulo 1000 : 1 attaque, 2 déplacement, 4 objet, 8 compétence) : états posés comme les
//!     autres (`effects::disturb_status`) ; l'incantation en cours tombe quand les compétences sont interdites.
//! ```
//! Règles proposées (aucune n'est jouée par le client) ; réglages `Rules.*` (`config.rs`).

use super::effects::{self, status, Effect};
use super::monsters::Monster;
use super::skills::{self, Owned, SkillLevel, SkillTemplate};
use super::{combat, Player, WorldCtx, WorldState};
use crate::{debug, info};
use archcore_net::schema;
use archcore_net::{build_packet, Fields, Packet, Vec3};
use rand::Rng;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Natures du bonus de niveau (`skill_levelup` de SKILL_SPEC).
pub const LEVELUP_ALL: i64 = 1;
pub const LEVELUP_MASTERY: i64 = 2;
pub const LEVELUP_LIST: i64 = 4;
/// `melee_attack` de Death Strike, résultat d'une mort instantanée.
pub const DEATH_STRIKE_ATTACK: i64 = 107;
pub const RESULT_DEATH_STRIKE: i64 = 13;
/// `move_pos` : le lanceur va au contact de la cible, la cible vient au contact du lanceur.
pub const MOVE_SELF: i64 = 2;
pub const MOVE_TARGET: i64 = 3;
/// `action_on_action` : dégâts à la fin de l'effet, attaque par téléportation, effet posé sur l'assaillant du porteur,
/// effet consommé au coup du porteur, explosion d'une créature, effet accru si la cible porte un autre effet, si le lanceur
/// est invisible.
pub const CHAIN_TIMED: i64 = 1;
pub const CHAIN_BLINK: i64 = 2;
pub const CHAIN_ON_ATTACKER: i64 = 3;
pub const CHAIN_ONE_HIT: i64 = 4;
pub const CHAIN_BURST: i64 = 6;
pub const CHAIN_ON_TARGET_EFFECT: i64 = 7;
pub const CHAIN_ON_INVISIBLE: i64 = 8;
/// Bits de `target_restriction` : cibles interdites.
pub mod restriction {
    pub const SIEGE_OBJECT: i64 = 0x1;
    pub const MONSTER: i64 = 0x2;
    pub const PLAYER: i64 = 0x4;
}
/// Type d'action du refus pour une cible interdite (mini SKILL_ACTION) : cible invalide.
pub const INVALID_TARGET: i64 = 7;
/// AGPMCHARACTER : résurrection proposée par un autre joueur (S→C et réponse), résurrection sur place.
pub const RESURRECTION_BY_OTHER: i64 = 24;
pub const REQUEST_RESURRECTION_NOW: i64 = 10;
/// Délai laissé au mort pour accepter.
pub const OFFER_TIME: Duration = Duration::from_secs(60);
/// Contact : distance laissée entre les deux personnages d'un déplacement forcé.
const CONTACT: f64 = 100.0;
/// Zone de Zone Provoke autour de la cible (aucune ligne de SKILL_CONST : choix).
const PROVOKE_AREA: f64 = 600.0;

/// Zone au sol en cours.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    /// Session du maître (lanceur ou maître de l'invocation).
    pub owner_sid: u32,
    pub anchor: Anchor,
    pub tid: u32,
    pub level: u32,
    pub radius: f64,
    /// Dégâts fixes par élément (zone d'attaque) ; PV et PM rendus (aura).
    pub damages: Vec<(usize, f64)>,
    pub regen: (f64, f64),
    pub aura: bool,
    /// Chance de chaque période (pour cent ; 0 : toujours).
    pub rate: f64,
    pub interval: Duration,
    pub next_at: Instant,
    pub until: Instant,
}

/// Centre d'une zone : un joueur (session) ou une créature (CID), qu'elle suit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anchor {
    Player(u32),
    Monster(u32),
}

/// État de ces effets dans le monde.
#[derive(Debug, Default)]
pub struct ExtraState {
    pub fields: Vec<Field>,
    /// Résurrections proposées : session du mort → (session du lanceur, fin de l'offre).
    pub offers: HashMap<u32, (u32, Instant)>,
    /// Monstres provoqués : CID du monstre → (CID du lanceur, fin).
    pub provoked: HashMap<u32, (u32, Instant)>,
    /// Coups à venir : fin de Time Attack, explosion d'une créature.
    pub pending: Vec<Pending>,
}

/// Coup différé d'une compétence.
#[derive(Clone, Debug)]
pub enum Pending {
    /// Fin de l'effet de Time Attack sur `target` : les dégâts du niveau.
    Timed { sid: u32, owned: Owned, target: u32, caster_tid: u32, at: Instant },
    /// Créature du maître `sid` morte ou arrivée à la fin de sa durée : `skill_tid` est la compétence qui l'a invoquée.
    Burst { sid: u32, skill_tid: u32, pos: Vec3 },
}

// ---------------------------------------------------------------------------------------------
// Bonus de niveau
// ---------------------------------------------------------------------------------------------

/// Bonus de niveau de toutes les compétences : options portées et effets de type 1.
pub fn all_levels(p: &Player) -> i64 {
    p.gear.skill_levels + effects::total(&p.skills.effects).skill_levels.round() as i64
}

/// Bonus de niveau de la seule compétence `tid` : effets de type 2 (arbres de maîtrise) et 4 (liste) qui la visent.
pub fn specific_bonus(catalog: &skills::SkillCatalog, p: &Player, tid: u32) -> i64 {
    let mut n = 0;
    for e in &p.skills.effects {
        let Some((t, lv)) = catalog.get(e.tid).and_then(|t| Some((t, t.level(e.level)?))) else { continue };
        n += match t.levelup_kind {
            LEVELUP_MASTERY if catalog.in_mastery(tid) => lv.levelup.max(0) as i64,
            // Masquerade : la liste de Skill_Const2 s'allonge avec le niveau, « skill_levelup » reste vide : +1 (choix).
            LEVELUP_LIST if lv.levelup_list.contains(&tid) => lv.levelup.max(1) as i64,
            _ => 0,
        };
    }
    n
}

/// Bonus de niveau de la compétence `tid` pour un lancement : celui de toutes les compétences, plus le sien.
pub fn level_bonus(ctx: &WorldCtx, p: &Player, tid: u32) -> i64 {
    all_levels(p) + specific_bonus(&ctx.skills, p, tid)
}

/// Annonce les bonus de niveau propres à chaque compétence qui ont changé (AGPMSKILL op 16).
pub fn sync_specific_levels(catalog: &skills::SkillCatalog, p: &mut Player) {
    if !p.in_game {
        return;
    }
    let cid = p.ch.id;
    let changed: Vec<(u32, i64)> = p
        .skills
        .list
        .iter()
        .map(|s| (s.id, specific_bonus(catalog, p, s.tid)))
        .filter(|(id, n)| p.skills.specific_sent.get(id).copied().unwrap_or(0) != *n)
        .collect();
    for (id, n) in changed {
        p.send_raw(effects::specific_level_packet(cid, id, n));
        p.skills.specific_sent.insert(id, n);
    }
}

// ---------------------------------------------------------------------------------------------
// Death Strike, Lens Stone, effet posé sur l'assaillant
// ---------------------------------------------------------------------------------------------

/// Chance de mort instantanée d'un joueur : la plus forte de ses passives Death Strike.
pub fn death_chance(ctx: &WorldCtx, p: &Player) -> f64 {
    p.skills
        .list
        .iter()
        .filter_map(|s| ctx.skills.get(s.tid).filter(|t| t.melee_attack == DEATH_STRIKE_ATTACK).and_then(|t| t.level(s.level)))
        .map(|lv| lv.death)
        .fold(0.0, f64::max)
}

/// Monstre « ordinaire » : ni boss de la fenêtre des boss, ni objet de siège, ni invocation.
pub fn ordinary(ctx: &WorldCtx, st: &WorldState, m: &Monster) -> bool {
    m.owner.is_none() && !ctx.bosses.tracks(m.tid) && !st.monsters.siege_spawns.contains_key(&m.spawn_id)
}

/// Death Strike : le coup normal qui vient de toucher le monstre `m` le tue-t-il ?
pub fn death_strike(ctx: &WorldCtx, st: &WorldState, p: &Player, m: &Monster, rng: &mut impl Rng) -> bool {
    if !crate::config::rules().death_strike || !ordinary(ctx, st, m) {
        return false;
    }
    let chance = death_chance(ctx, p);
    chance > 0.0 && rng.gen_range(0.0..100.0) < chance
}

/// Enchaînement 4 (Lens Stone) : les effets du joueur qui ne valent qu'un coup tombent après son coup qui touche `target`.
pub fn spend_one_hit(ctx: &WorldCtx, st: &mut WorldState, sid: u32, target: u32) {
    let Some(p) = st.players.get_mut(&sid) else { return };
    let spent: Vec<Effect> =
        p.skills.effects.iter().filter(|e| e.ends_at.is_some() && ctx.skills.get(e.tid).is_some_and(|t| t.action_on_action == CHAIN_ONE_HIT)).cloned().collect();
    if spent.is_empty() {
        return;
    }
    p.skills.effects.retain(|e| !spent.iter().any(|s| s.tid == e.tid));
    let cid = p.ch.id;
    for e in &spent {
        combat::send_around(st, sid, &effects::remove_packet(cid, e));
    }
    combat::send_around(st, sid, &effects::additional_effect_packet(cid, effects::additional::LENS_STONE, target));
    super::equipment::refresh_factors(ctx, st, sid);
    debug!("[compétence] effet d'un coup consommé ({} effet(s))", spent.len());
}

/// Compétence d'enchaînement 7 ou 8, dont les colonnes « …2 » sont soumises à une condition.
pub fn is_combo(t: &SkillTemplate) -> bool {
    matches!(t.action_on_action, CHAIN_ON_TARGET_EFFECT | CHAIN_ON_INVISIBLE)
}

/// La condition de l'enchaînement est remplie : la cible `target` porte l'effet de « Connection_TID2 » (7), le lanceur est
/// invisible (8).
pub fn combo_ready(st: &WorldState, sid: u32, t: &SkillTemplate, lv: &SkillLevel, target: u32) -> bool {
    match t.action_on_action {
        CHAIN_ON_TARGET_EFFECT => bears(st, target, lv.combo_on),
        CHAIN_ON_INVISIBLE => st.players.get(&sid).is_some_and(|p| p.skills.status_sent & status::INVISIBLE != 0),
        _ => false,
    }
}

/// Le personnage `cid` (monstre ou joueur) porte l'effet de la compétence `tid`.
fn bears(st: &WorldState, cid: u32, tid: u32) -> bool {
    if tid == 0 {
        return false;
    }
    match st.by_cid.get(&cid).and_then(|s| st.players.get(s)) {
        Some(p) => p.skills.effects.iter().any(|e| e.tid == tid),
        None => st.monsters.effects.get(&cid).is_some_and(|l| l.iter().any(|e| e.tid == tid)),
    }
}

/// Part des dégâts `damage` portés à `target` absorbée en PV par le joueur `sid`, avec la chance de `(rate, pct)`.
pub fn absorb(ctx: &WorldCtx, st: &mut WorldState, sid: u32, target: u32, damage: i32, (rate, pct): (f64, f64)) {
    if damage <= 0 || pct <= 0.0 || rate <= 0.0 || rand::thread_rng().gen_range(0.0..100.0) >= rate {
        return;
    }
    let Some(p) = st.players.get_mut(&sid).filter(|p| !p.dead) else { return };
    let gain = absorbed(damage, pct);
    p.factors.point[crate::character::POINT_HP] = (p.factors.point[crate::character::POINT_HP] + gain).min(p.factors.point_max[0]);
    let cid = p.ch.id;
    combat::send_around(st, sid, &effects::additional_effect_packet(cid, effects::additional::ABSORB_HP, target));
    super::equipment::refresh_factors(ctx, st, sid);
    debug!("[compétence] {cid:#x} absorbe {gain} PV");
}

/// PV gagnés pour `damage` dégâts portés, `pct` pour cent absorbés ; 1 au moins.
pub fn absorbed(damage: i32, pct: f64) -> i32 {
    ((damage as f64 * pct / 100.0) as i32).max(1)
}

/// Membres de la guilde de `sid` (lui excepté), en jeu, vivants et à moins de `radius` de `center`.
pub fn guild_near(st: &WorldState, sid: u32, center: Vec3, radius: f64) -> Vec<u32> {
    let Some(gid) = st.players.get(&sid).and_then(|p| st.guilds.of(p.ch.id)).map(|g| g.id) else { return Vec::new() };
    st.players
        .iter()
        .filter(|(o, p)| **o != sid && p.in_game && !p.dead && combat::dist2d(p.dest.unwrap_or_else(|| p.ch.pos()), center) <= radius)
        .filter(|(_, p)| st.guilds.of(p.ch.id).is_some_and(|g| g.id == gid))
        .map(|(o, _)| *o)
        .collect()
}

/// Attaque par téléportation : le joueur `sid` est posé au contact de `target` ; arrêt net chez lui et ceux qui le voient,
/// effet visuel. Rend sa nouvelle position ; `None` si la cible n'est plus là ou que la place est bloquée.
pub fn blink(ctx: &WorldCtx, st: &mut WorldState, sid: u32, target: u32, now: Instant) -> Option<Vec3> {
    let at = st.monsters.get(target).filter(|m| m.is_alive()).map(|m| m.pos).or_else(|| st.by_cid.get(&target).and_then(|s| st.players.get(s)).map(|p| p.ch.pos()))?;
    let p = st.players.get_mut(&sid)?;
    let to = contact(at, p.ch.pos(), CONTACT);
    if ctx.map.is_blocked(to.x as f64, to.z as f64) {
        return None;
    }
    let pos = Vec3::new(to.x, ctx.map.height(to.x as f64, to.z as f64).unwrap_or(to.y), to.z);
    p.ch.set_pos(pos);
    p.dest = None;
    p.move_guard.reset(pos, super::anticheat::move_speed(&p.factors), now);
    let cid = p.ch.id;
    let mv = &schema::OPT_CHAR_MOVE;
    let stopped = build_packet(mv, &Fields::new(mv).with(0, cid).with(1, pos).with(2, pos).with(5, super::MOVE_STOP_SYNC).with(6, 0));
    combat::send_around(st, sid, &stopped);
    combat::send_around(st, sid, &effects::additional_effect_packet(cid, effects::additional::TELEPORT, target));
    super::presence::refresh(st, sid);
    debug!("[compétence] {cid:#x} se téléporte au contact de {target:#x}");
    Some(pos)
}

/// Time Attack : effet posé sur `target` (monstre ou joueur), dégâts du niveau à sa fin.
#[allow(clippy::too_many_arguments)]
pub fn arm_timed(ctx: &WorldCtx, st: &mut WorldState, sid: u32, owned: &Owned, lv: &SkillLevel, target: u32, caster_tid: u32, now: Instant) {
    let at = now + Duration::from_millis(lv.duration_ms);
    let e = Effect { caster_sid: sid, ..skills::new_effect(owned, lv, caster_tid, 0, None, now) };
    match st.by_cid.get(&target).copied() {
        Some(tsid) => skills::add_player_effect(ctx, st, tsid, e),
        None => put_on_monster(st, target, e, now),
    }
    st.extra.pending.push(Pending::Timed { sid, owned: *owned, target, caster_tid, at });
}

/// Une créature disparaît par la mort ou à la fin de sa durée : son explosion éventuelle (enchaînement 6) est programmée.
pub fn on_summon_end(st: &mut WorldState, cid: u32) {
    let Some(s) = st.summons.list.get(&cid) else { return };
    let Some(sid) = st.by_cid.get(&s.owner).copied() else { return };
    let (skill_tid, pos) = (s.skill_tid, st.monsters.get(cid).map(|m| m.pos));
    if let Some(pos) = pos {
        st.extra.pending.push(Pending::Burst { sid, skill_tid, pos });
    }
}

/// Compétence qui frappe à l'explosion d'une créature invoquée par `skill_tid` : la compétence additionnelle de sa passive
/// d'enchaînement 6 (Bomba → VictimOfGlory → son effet additionnel).
pub fn burst_skill<'a>(catalog: &'a skills::SkillCatalog, skill_tid: u32, level: u32) -> Option<(&'a SkillTemplate, u32)> {
    let passive = catalog.get(skill_tid).and_then(|t| t.level(level).or_else(|| t.levels.first())).map(|lv| lv.additional_skill).filter(|a| *a != 0)?;
    let chained = catalog.get(passive).filter(|t| t.action_on_action == CHAIN_BURST)?;
    let add = chained.level(level).or_else(|| chained.levels.first()).map(|lv| lv.additional_skill).filter(|a| *a != 0)?;
    let t = catalog.get(add)?;
    Some((t, if t.level(level).is_some() { level } else { 1 }))
}

/// Coups différés arrivés à échéance.
pub async fn pending_step(ctx: &WorldCtx, st: &mut WorldState, now: Instant) -> anyhow::Result<()> {
    if st.extra.pending.is_empty() {
        return Ok(());
    }
    let (due, keep): (Vec<Pending>, Vec<Pending>) = std::mem::take(&mut st.extra.pending).into_iter().partition(|p| match p {
        Pending::Timed { at, .. } => now >= *at,
        Pending::Burst { .. } => true,
    });
    st.extra.pending = keep;
    for p in due {
        match p {
            Pending::Timed { sid, owned, target, caster_tid, .. } => timed_hit(ctx, st, sid, owned, target, caster_tid, now).await?,
            Pending::Burst { sid, skill_tid, pos } => burst(ctx, st, sid, skill_tid, pos, now).await?,
        }
    }
    Ok(())
}

/// Fin de Time Attack : effet visuel, puis les dégâts du niveau, comme un coup de compétence sans effet durable.
async fn timed_hit(ctx: &WorldCtx, st: &mut WorldState, sid: u32, owned: Owned, target: u32, caster_tid: u32, now: Instant) -> anyhow::Result<()> {
    let Some(caster) = st.players.get(&sid).filter(|p| p.in_game).map(|p| p.ch.id) else { return Ok(()) };
    let Some((t, lv)) = ctx.skills.get(owned.tid).and_then(|t| Some((t, t.level(owned.level)?))) else { return Ok(()) };
    let lv = SkillLevel { duration_ms: 0, ..lv.clone() };
    let packet = effects::additional_effect_packet(caster, effects::additional::END_BUFF_EXPLOSION, target);
    if let Some(tsid) = st.by_cid.get(&target).copied() {
        combat::send_around(st, tsid, &packet);
        return timed_on_player(ctx, st, sid, &owned, &lv, tsid, now).await;
    }
    if st.monsters.get(target).is_some_and(|m| m.is_alive()) {
        let watchers = combat::viewers(st, target);
        combat::broadcast(st, &watchers, &packet);
        let strike = SkillTemplate { melee_attack: 101, action_on_action: 0, ..t.clone() };
        skills::hit_monster(ctx, st, sid, &owned, &strike, &lv, target, caster_tid, now).await?;
    }
    Ok(())
}

/// Dégâts de la fin de Time Attack sur un joueur (`skills::hit_player` ne frappe qu'à dégâts immédiats).
async fn timed_on_player(ctx: &WorldCtx, st: &mut WorldState, sid: u32, owned: &Owned, lv: &SkillLevel, tsid: u32, now: Instant) -> anyhow::Result<()> {
    let (Some(p), Some(v)) = (st.players.get(&sid), st.players.get(&tsid)) else { return Ok(()) };
    if v.dead || !v.in_game {
        return Ok(());
    }
    let (attacker, spirit) = (combat::Attacker::of_player(p), combat::spirit_ranges(&p.factors));
    let defender = attacker.pierce(&combat::Defender::of_player(&v.factors));
    let damages = skills::skill_damage(&mut rand::thread_rng(), &attacker, &spirit, &defender, lv);
    let total: i32 = damages.iter().map(|(_, d)| *d).sum();
    let (caster, target) = (p.ch.id, v.ch.id);
    let v = st.players.get_mut(&tsid).unwrap();
    let hp = (v.factors.point[crate::character::POINT_HP] - total).max(0);
    v.factors.point[crate::character::POINT_HP] = hp;
    let max_mp = v.factors.point[1];
    combat::send_around(st, tsid, &skills::result_packet(caster, owned, target, skills::result_factors(hp, max_mp, &damages)));
    super::pvp::on_hit(ctx, st, sid, tsid, now);
    if hp == 0 {
        combat::player_death(st, tsid);
        return super::pvp::on_kill(ctx, st, sid, tsid).await;
    }
    Ok(())
}

/// Explosion d'une créature : les monstres autour d'elle prennent la compétence additionnelle.
async fn burst(ctx: &WorldCtx, st: &mut WorldState, sid: u32, skill_tid: u32, pos: Vec3, now: Instant) -> anyhow::Result<()> {
    let Some(p) = st.players.get(&sid).filter(|p| p.in_game) else { return Ok(()) };
    let level = p.skills.by_tid(skill_tid).map(|s| s.level).unwrap_or(1);
    let caster_tid = p.ch.template_id;
    let Some((t, n)) = burst_skill(&ctx.skills, skill_tid, level) else { return Ok(()) };
    let Some(lv) = t.level(n) else { return Ok(()) };
    let targets: Vec<u32> = st
        .monsters
        .near(pos, lv.area_r + 200.0)
        .filter(|m| m.is_alive() && m.owner.is_none() && st.monsters.attackable(m) && super::siege::war::may_hit_auto(st, m.cid) && combat::dist2d(m.pos, pos) <= lv.area_r)
        .map(|m| m.cid)
        .collect();
    debug!("[invocation] explosion de {} : {} monstre(s)", t.name, targets.len());
    let owned = Owned { id: 0, tid: t.tid, level: n, ready_at: None, exp: 0 };
    for cid in targets {
        skills::hit_monster(ctx, st, sid, &owned, t, lv, cid, caster_tid, now).await?;
    }
    Ok(())
}

/// Effet d'une compétence additionnelle `tid` (au niveau `level`, à défaut le premier), durée `secs` (0 : la sienne).
fn additional_effect(ctx: &WorldCtx, tid: u32, level: u32, secs: u64, caster_tid: u32, caster_sid: u32, now: Instant) -> Option<Effect> {
    let t = ctx.skills.get(tid)?;
    let lv = t.level(level).or_else(|| t.levels.first())?;
    let n = if t.level(level).is_some() { level } else { 1 };
    let lv = if secs > 0 { SkillLevel { duration_ms: secs * 1000, ..lv.clone() } } else { lv.clone() };
    if !lv.leaves_effect(lv.status) {
        return None;
    }
    let owned = Owned { id: 0, tid, level: n, ready_at: None, exp: 0 };
    Some(Effect { caster_sid, ..skills::new_effect(&owned, &lv, caster_tid, lv.status, None, now) })
}

/// Pose un effet sur un monstre : annoncé, états recalculés.
pub fn put_on_monster(st: &mut WorldState, cid: u32, e: Effect, now: Instant) {
    let watchers = combat::viewers(st, cid);
    let list = st.monsters.effects.entry(cid).or_default();
    if let Some(old) = effects::push(list, e.clone()) {
        combat::broadcast(st, &watchers, &effects::remove_packet(cid, &old));
    }
    combat::broadcast(st, &watchers, &effects::add_packet(cid, &e));
    skills::sync_monster_status(st, cid, now);
}

/// Enchaînement 3 : le joueur `sid` vient d'être frappé par `attacker` (monstre ou joueur) ; chaque effet qu'il porte
/// avec une compétence additionnelle la pose sur l'assaillant.
pub fn on_bearer_hit(ctx: &WorldCtx, st: &mut WorldState, sid: u32, attacker: u32, now: Instant) {
    let Some(p) = st.players.get(&sid) else { return };
    let (caster_tid, chained): (u32, Vec<(u32, u32, u64)>) = (
        p.ch.template_id,
        p.skills
            .effects
            .iter()
            .filter_map(|e| {
                let t = ctx.skills.get(e.tid).filter(|t| t.action_on_action == CHAIN_ON_ATTACKER)?;
                let lv = t.level(e.level)?;
                (lv.additional_skill != 0).then_some((lv.additional_skill, e.level, lv.additional_secs))
            })
            .collect(),
    );
    for (tid, level, secs) in chained {
        let Some(e) = additional_effect(ctx, tid, level, secs, caster_tid, sid, now) else { continue };
        if st.monsters.get(attacker).is_some_and(|m| m.is_alive()) {
            debug!("[compétence] {} renvoie {tid} sur {attacker:#x}", st.players[&sid].ch.name);
            put_on_monster(st, attacker, e, now);
        } else if let Some(asid) = st.by_cid.get(&attacker).copied().filter(|a| *a != sid) {
            skills::add_player_effect(ctx, st, asid, e);
            skills::sync_player_status(st, asid, now);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Déplacements forcés
// ---------------------------------------------------------------------------------------------

/// Point au contact de `at`, du côté de `from`.
pub fn contact(at: Vec3, from: Vec3, gap: f64) -> Vec3 {
    let d = combat::dist2d(at, from);
    if d < 1.0 {
        return Vec3::new(at.x + gap as f32, at.y, at.z);
    }
    let k = (gap / d) as f32;
    Vec3::new(at.x + (from.x - at.x) * k, at.y, at.z + (from.z - at.z) * k)
}

/// Point reculé de `distance` depuis `at`, dans l'axe `from` → `at`.
pub fn pushed(at: Vec3, from: Vec3, distance: f64) -> Vec3 {
    let d = combat::dist2d(at, from).max(1.0);
    let k = (distance / d) as f32;
    Vec3::new(at.x + (at.x - from.x) * k, at.y, at.z + (at.z - from.z) * k)
}

/// Pose le monstre `cid` en `to` (point d'apparition gardé), sauf boss et objet de siège.
pub fn place_monster(ctx: &WorldCtx, st: &mut WorldState, cid: u32, to: Vec3) -> bool {
    let Some(m) = st.monsters.get(cid).cloned() else { return false };
    if ctx.bosses.tracks(m.tid) || st.monsters.siege_spawns.contains_key(&m.spawn_id) || ctx.map.is_blocked(to.x as f64, to.z as f64) {
        return false;
    }
    super::summons::jump(ctx, st, cid, to);
    if let Some(x) = st.monsters.get_mut(cid) {
        x.home = m.home;
    }
    true
}

/// Déplacement forcé d'un monstre touché par une compétence de joueur lancée depuis `from` : tiré au contact
/// (`move_pos` 3) ou reculé (« move_distance »).
pub fn after_skill_hit(ctx: &WorldCtx, st: &mut WorldState, t: &SkillTemplate, lv: &SkillLevel, from: Vec3, caster: u32, cid: u32) {
    let r = crate::config::rules();
    if !r.forced_move {
        return;
    }
    let Some(at) = st.monsters.get(cid).filter(|m| m.is_alive()).map(|m| m.pos) else { return };
    let to = if t.move_pos == MOVE_TARGET {
        contact(from, at, CONTACT)
    } else if lv.move_distance > 0.0 {
        pushed(at, from, lv.move_distance * r.knockback_unit)
    } else {
        return;
    };
    if place_monster(ctx, st, cid, to) {
        let watchers = combat::viewers(st, cid);
        combat::broadcast(st, &watchers, &effects::additional_effect_packet(caster, effects::additional::TELEPORT, cid));
        debug!("[compétence] {} déplace {cid:#x} en {:.0}, {:.0}", t.name, to.x, to.z);
    }
}

/// Ruée d'un monstre (`move_pos` 2) : il est posé au contact de sa cible avant l'impact.
pub fn rush(ctx: &WorldCtx, st: &mut WorldState, cid: u32, target: Vec3) {
    if !crate::config::rules().forced_move {
        return;
    }
    let Some(from) = st.monsters.get(cid).map(|m| m.pos) else { return };
    if place_monster(ctx, st, cid, contact(target, from, CONTACT)) {
        let watchers = combat::viewers(st, cid);
        combat::broadcast(st, &watchers, &effects::additional_effect_packet(cid, effects::additional::TELEPORT, cid));
    }
}

// ---------------------------------------------------------------------------------------------
// Zones au sol
// ---------------------------------------------------------------------------------------------

/// Zone d'une compétence à `at_field` (10001, 10002 attaque ; 10003 aura) au niveau `lv`.
pub fn field_of(t: &SkillTemplate, lv: &SkillLevel, owner_sid: u32, anchor: Anchor, ms: u64, now: Instant) -> Option<Field> {
    if t.at_field == 0 || lv.area_r <= 0.0 || ms == 0 {
        return None;
    }
    let aura = matches!(t.at_field % 10, 3 | 4);
    let interval = Duration::from_millis(if lv.interval_ms > 0 { lv.interval_ms } else { 1000 });
    Some(Field {
        owner_sid,
        anchor,
        tid: t.tid,
        level: 1,
        radius: lv.area_r,
        damages: lv.area_damage.clone(),
        regen: lv.regen,
        aura,
        rate: lv.rate,
        interval,
        next_at: now + interval,
        until: now + Duration::from_millis(ms),
    })
}

/// Compétence à zone lancée par un joueur sur lui-même : la zone le suit pendant la durée du niveau.
pub fn cast_field(st: &mut WorldState, sid: u32, owned: &Owned, t: &SkillTemplate, lv: &SkillLevel, now: Instant) -> bool {
    let Some(mut f) = field_of(t, lv, sid, Anchor::Player(sid), lv.duration_ms, now) else { return false };
    f.level = owned.level;
    st.extra.fields.retain(|x| !(x.owner_sid == sid && x.tid == t.tid));
    info!("[compétence] {} : zone {} ({:.0} unités, {} s)", st.players.get(&sid).map(|p| p.ch.name.as_str()).unwrap_or("?"), t.name, f.radius, lv.duration_ms / 1000);
    st.extra.fields.push(f);
    true
}

/// Une créature vient d'apparaître pour le maître `sid` par la compétence `skill_tid` : sa compétence additionnelle,
/// si c'est une zone, la suit jusqu'à sa fin.
pub fn on_summoned(ctx: &WorldCtx, st: &mut WorldState, sid: u32, cid: u32, skill_tid: u32, until: Option<Instant>, now: Instant) {
    let level = st.players.get(&sid).and_then(|p| p.skills.by_tid(skill_tid)).map(|s| s.level).unwrap_or(1);
    let Some(add) = ctx.skills.get(skill_tid).and_then(|t| t.level(level).or_else(|| t.levels.first())).map(|lv| lv.additional_skill).filter(|a| *a != 0) else { return };
    let Some((t, lv)) = ctx.skills.get(add).and_then(|t| Some((t, t.level(level).or_else(|| t.levels.first())?))) else { return };
    let ms = until.map(|u| u.saturating_duration_since(now).as_millis() as u64).unwrap_or(3_600_000);
    if let Some(f) = field_of(t, lv, sid, Anchor::Monster(cid), ms, now) {
        debug!("[invocation] {cid:#x} : zone {} ({:.0} unités)", t.name, f.radius);
        st.extra.fields.push(f);
    }
}

/// Coup d'une invocation qui touche le monstre `target` : sa compétence additionnelle à chance (passive) pose, avec
/// cette chance, l'effet de sa propre compétence additionnelle sur la cible.
pub fn summon_proc(ctx: &WorldCtx, st: &mut WorldState, summon: u32, owner_sid: u32, target: u32, now: Instant) {
    let Some(skill_tid) = st.summons.list.get(&summon).map(|s| s.skill_tid) else { return };
    let level = st.players.get(&owner_sid).and_then(|p| p.skills.by_tid(skill_tid)).map(|s| s.level).unwrap_or(1);
    let Some(passive) = ctx.skills.get(skill_tid).and_then(|t| t.level(level).or_else(|| t.levels.first())).map(|lv| lv.additional_skill).filter(|a| *a != 0) else { return };
    let Some(plv) = ctx.skills.get(passive).filter(|t| t.at_field == 0).and_then(|t| t.level(level).or_else(|| t.levels.first())) else { return };
    if plv.additional_skill == 0 || plv.rate <= 0.0 || rand::thread_rng().gen_range(0.0..100.0) >= plv.rate {
        return;
    }
    let caster_tid = st.monsters.get(summon).map(|m| m.tid).unwrap_or(0);
    if let Some(e) = additional_effect(ctx, plv.additional_skill, level, plv.additional_secs, caster_tid, owner_sid, now) {
        if st.monsters.get(target).is_some_and(|m| m.is_alive()) {
            debug!("[invocation] {summon:#x} : {} sur {target:#x}", plv.additional_skill);
            put_on_monster(st, target, e, now);
        }
    }
}

/// Centre actuel d'une zone ; `None` si ce qui la porte n'est plus là.
fn center(st: &WorldState, f: &Field) -> Option<Vec3> {
    match f.anchor {
        Anchor::Player(sid) => st.players.get(&sid).filter(|p| p.in_game && !p.dead).map(|p| p.ch.pos()),
        Anchor::Monster(cid) => st.monsters.get(cid).filter(|m| m.is_alive()).map(|m| m.pos),
    }
}

/// Dégâts d'une période de zone sur un monstre : dégâts fixes moins sa résistance à l'élément, 1 au moins.
pub fn field_damage(damages: &[(usize, f64)], resist: &[i32; 8]) -> Vec<(usize, i32)> {
    damages
        .iter()
        .map(|(i, d)| {
            let r = if (10..=17).contains(i) { resist[i - 10].clamp(0, 100) } else { 0 };
            (*i, ((d * (100 - r) as f64 / 100.0).round() as i32).max(1))
        })
        .collect()
}

/// Périodes des zones au sol.
pub async fn fields_step(ctx: &WorldCtx, st: &mut WorldState, now: Instant) -> anyhow::Result<()> {
    if st.extra.fields.is_empty() {
        return Ok(());
    }
    let fields = std::mem::take(&mut st.extra.fields);
    let mut keep = Vec::new();
    let mut due = Vec::new();
    for mut f in fields {
        let Some(at) = center(st, &f).filter(|_| now < f.until && st.players.contains_key(&f.owner_sid)) else { continue };
        if now >= f.next_at {
            f.next_at = now + f.interval;
            if f.rate <= 0.0 || rand::thread_rng().gen_range(0.0..100.0) < f.rate {
                due.push((f.clone(), at));
            }
        }
        keep.push(f);
    }
    st.extra.fields = keep;
    for (f, at) in due {
        if f.aura {
            aura_tick(ctx, st, &f, at);
        } else {
            attack_tick(ctx, st, &f, at, now).await?;
        }
    }
    Ok(())
}

/// Aura : le maître et les membres de son groupe dans la zone regagnent PV et PM.
fn aura_tick(ctx: &WorldCtx, st: &mut WorldState, f: &Field, at: Vec3) {
    if f.regen == (0.0, 0.0) {
        return;
    }
    let mut friends = vec![f.owner_sid];
    friends.extend(skills::party_near(st, f.owner_sid, at, f.radius));
    for sid in friends {
        let Some(p) = st.players.get_mut(&sid).filter(|p| !p.dead && combat::dist2d(p.ch.pos(), at) <= f.radius) else { continue };
        effects::heal(&mut p.factors, &effects::Heal { hp: f.regen.0, mp: f.regen.1, ..Default::default() });
        super::equipment::refresh_factors(ctx, st, sid);
    }
}

/// Zone d'attaque : chaque monstre attaquable dans la zone prend les dégâts de la période.
async fn attack_tick(ctx: &WorldCtx, st: &mut WorldState, f: &Field, at: Vec3, now: Instant) -> anyhow::Result<()> {
    let Some(owner) = st.players.get(&f.owner_sid).map(|p| p.ch.id) else { return Ok(()) };
    let caster = match f.anchor {
        Anchor::Monster(cid) => cid,
        _ => owner,
    };
    let targets: Vec<u32> = st
        .monsters
        .near(at, f.radius + 200.0)
        .filter(|m| m.is_alive() && m.owner.is_none() && st.monsters.attackable(m) && super::siege::war::may_hit_auto(st, m.cid) && combat::dist2d(m.pos, at) <= f.radius)
        .map(|m| m.cid)
        .collect();
    let owned = Owned { id: 0, tid: f.tid, level: f.level, ready_at: None, exp: 0 };
    for cid in targets {
        let Some(m) = st.monsters.get(cid).filter(|m| m.is_alive()).cloned() else { continue };
        let stats = st.monsters.stats_for(&m);
        let damages = field_damage(&f.damages, &stats.resist);
        let total: i32 = damages.iter().map(|(_, d)| *d).sum();
        let target = st.monsters.get_mut(cid).unwrap();
        target.hp = (target.hp - total).max(0);
        let hp = target.hp;
        st.monsters.wake(cid);
        let mut watchers = combat::viewers(st, cid);
        if !watchers.contains(&f.owner_sid) {
            watchers.push(f.owner_sid);
        }
        combat::broadcast(st, &watchers, &skills::result_packet(caster, &owned, cid, skills::result_factors(hp, stats.max_mp, &damages)));
        super::ai::add_aggro(st, cid, owner, total.max(1) as u64, now);
        if hp == 0 {
            combat::kill(ctx, st, f.owner_sid, cid, now).await?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Détection, cibles interdites, provocation
// ---------------------------------------------------------------------------------------------

/// Le joueur porte un effet de détection.
pub fn detects(ctx: &WorldCtx, p: &Player) -> bool {
    p.skills.effects.iter().any(|e| ctx.skills.get(e.tid).is_some_and(|t| t.detect != 0))
}

/// Le joueur `tsid` est invisible pour `sid` (état 10 sans détection).
pub fn hidden_from(ctx: &WorldCtx, st: &WorldState, sid: u32, tsid: u32) -> bool {
    let (Some(p), Some(t)) = (st.players.get(&sid), st.players.get(&tsid)) else { return false };
    sid != tsid && t.skills.status_sent & status::INVISIBLE != 0 && !detects(ctx, p)
}

/// La compétence interdit cette cible (`target_restriction`) : objet de siège, monstre, joueur.
pub fn restricted(st: &WorldState, t: &SkillTemplate, target: u32) -> bool {
    let r = t.target_restriction;
    if r == 0 {
        return false;
    }
    match st.monsters.get(target) {
        Some(m) if st.monsters.siege_spawns.contains_key(&m.spawn_id) => r & restriction::SIEGE_OBJECT != 0,
        Some(_) => r & restriction::MONSTER != 0,
        None => st.by_cid.contains_key(&target) && r & restriction::PLAYER != 0,
    }
}

/// Provoke, Zone Provoke : compétences de provocation (sans ligne de SKILL_CONST).
pub fn is_provoke(t: &SkillTemplate) -> bool {
    matches!(t.name.as_str(), "Provoke" | "Zone Provoke")
}

/// Niveau implicite d'une provocation : portée d'attaque, zone autour de la cible pour Zone Provoke.
pub fn provoke_level(t: &SkillTemplate) -> Option<&'static SkillLevel> {
    static SINGLE: std::sync::OnceLock<SkillLevel> = std::sync::OnceLock::new();
    static ZONE: std::sync::OnceLock<SkillLevel> = std::sync::OnceLock::new();
    if !is_provoke(t) {
        return None;
    }
    Some(if t.area() == skills::Area::Single { SINGLE.get_or_init(SkillLevel::default) } else { ZONE.get_or_init(|| SkillLevel { area_r: PROVOKE_AREA, ..Default::default() }) })
}

/// Le monstre `cid` est provoqué par le joueur `caster` (CID) : il le prend pour cible et lui donne la plus forte haine
/// de sa liste + 1 ; sans effet sur un monstre berserk.
pub fn provoke(st: &mut WorldState, cid: u32, caster: u32, now: Instant) {
    if st.monsters.status(cid, now) & status::BERSERK != 0 {
        return;
    }
    let Some(m) = st.monsters.get_mut(cid).filter(|m| m.is_alive()) else { return };
    let top = m.aggro.iter().map(|a| a.damage).max().unwrap_or(0);
    let mine = m.aggro.iter().find(|a| a.cid == caster).map(|a| a.damage).unwrap_or(0);
    m.target = Some(caster);
    super::ai::add_aggro(st, cid, caster, top.saturating_sub(mine) + 1, now);
    st.extra.provoked.insert(cid, (caster, now + Duration::from_secs(crate::config::rules().provoke_secs)));
    debug!("[compétence] {cid:#x} provoqué par {caster:#x}");
}

/// Joueur qui provoque encore le monstre `cid` (en jeu et vivant), sinon `None`.
pub fn provoker(st: &mut WorldState, cid: u32, now: Instant) -> Option<u32> {
    let (caster, until) = st.extra.provoked.get(&cid).copied()?;
    let alive = st.by_cid.get(&caster).and_then(|s| st.players.get(s)).is_some_and(|p| p.in_game && !p.dead);
    if now >= until || !alive || st.monsters.status(cid, now) & status::BERSERK != 0 {
        st.extra.provoked.remove(&cid);
        return None;
    }
    Some(caster)
}

// ---------------------------------------------------------------------------------------------
// Résurrection par un autre joueur (Redemption)
// ---------------------------------------------------------------------------------------------

/// AGPMCHARACTER op 24 : `caster` (CID, nom) propose de ressusciter `target`.
pub fn offer_packet(target: u32, caster: u32, caster_name: &str) -> Vec<u8> {
    let c = &schema::CHARACTER;
    build_packet(c, &Fields::new(c).with(0, RESURRECTION_BY_OTHER).with(1, target).with(3, caster_name.as_bytes().to_vec()).with(13, caster))
}

/// Redemption lancée par `sid` sur le joueur mort `tsid` : l'offre lui part.
pub fn offer_resurrection(st: &mut WorldState, sid: u32, tsid: u32, now: Instant) {
    let (Some(p), Some(t)) = (st.players.get(&sid), st.players.get(&tsid)) else { return };
    if !t.dead || !t.in_game {
        return;
    }
    t.send_raw(offer_packet(t.ch.id, p.ch.id, &p.ch.name));
    info!("[compétence] {} propose de ressusciter {}", p.ch.name, t.ch.name);
    st.extra.offers.insert(tsid, (sid, now + OFFER_TIME));
}

/// Réponse du mort (AGPMCHARACTER op 24, champ 4 à 0 : refus ; ou op 10) : résurrection sur place, sans perte
/// d'expérience.
pub fn on_reply(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) {
    let Some((caster, until)) = st.extra.offers.remove(&sid) else { return };
    let refused = p.int(0) == Some(RESURRECTION_BY_OTHER) && p.int(4) == Some(0);
    if refused || Instant::now() >= until || !st.players.get(&sid).is_some_and(|x| x.dead) {
        debug!("[compétence] résurrection de #{sid} par #{caster} : refusée ou échue");
        return;
    }
    combat::revive(ctx, st, sid);
    if let Some(x) = st.players.get(&sid) {
        info!("[compétence] {} ressuscite sur place (Redemption)", x.ch.name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enchainements_de_combo() {
        // Colonnes de Precise Assault (8) et Recite of Breeze (7) : second coup, modificateurs, étourdissement, dégâts sur la durée.
        let sp = skills::Spec { action_on_action: CHAIN_ON_INVISIBLE, offensive: true, ..Default::default() };
        let c = serde_json::json!({
            "damage_a_PERCENT": "130", "damage_a_PERCENT2": "30", "damage_water_PERCENT2": "15", "percent.movement": "-10", "percent2.movement2": "-5",
            "skill_rate2": "30", "stun_time2": "3000", "dot_dmg_earth": "100", "dot_dmg_earth2": "23", "duration": "8000", "Connection_TID2": "673"
        });
        let lv = SkillLevel::with_spec(&c, &sp);
        assert_eq!((lv.combo_on, lv.combo_stun), (673, (30.0, 3000)));
        assert_eq!(lv.combo_mods.movement_pct, -5.0);
        let off = lv.with_combo(false);
        assert!(!off.has_second_hit(), "sans condition : pas de second coup");
        assert_eq!(off.mods.movement_pct, -10.0);
        let on = lv.with_combo(true);
        assert!(on.has_second_hit());
        assert_eq!(on.mods.movement_pct, -15.0);
        assert_eq!((on.status & status::STUN != 0, on.status_rate, on.status_ms), (true, 30.0, 3000));
        assert_eq!(on.dot.damages, vec![(13, 123.0)], "dégâts sur la durée de l'élément ajoutés");
        // Colonnes « …2 » lues seulement pour un enchaînement 7 ou 8.
        assert_eq!(SkillLevel::with_spec(&c, &skills::Spec::default()).combo_mods.movement_pct, 0.0);
    }

    #[test]
    fn absorption_et_explosion() {
        assert_eq!(absorbed(500, 10.0), 50);
        assert_eq!(absorbed(3, 10.0), 1, "1 au moins");
        // Life Extinction (4) : effet durable ; Vampiric Impact (6) : son propre coup.
        let c = serde_json::json!({ "skill_rate": "90", "damage_convert_hp": "10", "duration": "60000" });
        let life = SkillLevel::with_spec(&c, &skills::Spec { convert_type: 4, ..Default::default() });
        assert_eq!((life.mods.absorb_rate, life.mods.absorb, life.drain), (90.0, 10.0, (0.0, 0.0)));
        assert!(life.lasting());
        let vamp = SkillLevel::with_spec(&c, &skills::Spec { convert_type: 6, ..Default::default() });
        assert_eq!((vamp.mods.absorb_rate, vamp.drain), (0.0, (90.0, 10.0)));
        // Paquets : op 10 (type, lanceur, cible), op 16 (identifiant, propriétaire, bonus).
        let p = archcore_net::decode_packet(&schema::SKILL, &effects::additional_effect_packet(0x10, effects::additional::TELEPORT, 0x20)).unwrap();
        assert_eq!((p.int(0), p.int(1), p.int(4)), (Some(10), Some(2), Some(0x20)));
        let b = archcore_net::decode_mini(&schema::mini::SKILL_BASE, p.bytes(2).unwrap()).unwrap();
        assert_eq!((b.int(0), b.int(1)), (Some(3), Some(0x10)));
        let p = archcore_net::decode_packet(&schema::SKILL, &effects::specific_level_packet(0x10, 7, 3)).unwrap();
        assert_eq!((p.int(0), p.int(1), p.int(4)), (Some(16), Some(7), Some(3)));
    }

    #[test]
    fn visee_au_sol() {
        let narcolepsy = SkillTemplate { target: 1, range_code: 701, ..Default::default() };
        let meteor = SkillTemplate { target: 100, range_code: 701, ..Default::default() };
        let concealment = SkillTemplate { target: 111, range_code: 702, ..Default::default() };
        assert_eq!(narcolepsy.area(), skills::Area::Ground);
        assert!(!narcolepsy.needs_target() && narcolepsy.hits_foes() && !narcolepsy.ground_guild());
        assert!(meteor.hits_foes(), "champ target 100, mais visée sur les ennemis");
        assert!(concealment.ground_guild() && !concealment.hits_foes());
        let lv = SkillLevel { area_r: 700.0, ..Default::default() };
        let at = Vec3::new(1000.0, 0.0, 0.0);
        assert!(skills::in_area(skills::Area::Ground, Vec3::new(1500.0, 0.0, 0.0), at, Vec3::new(0.0, 0.0, 0.0), at, &lv));
        assert!(!skills::in_area(skills::Area::Ground, Vec3::new(1800.0, 0.0, 0.0), at, Vec3::new(0.0, 0.0, 0.0), at, &lv));
    }

    #[test]
    fn deplacements_forces() {
        let caster = Vec3::new(0.0, 0.0, 0.0);
        let target = Vec3::new(1000.0, 0.0, 0.0);
        // Tirée au contact du lanceur, du côté d'où elle vient.
        assert_eq!(contact(caster, target, 100.0), Vec3::new(100.0, 0.0, 0.0));
        // Body Check niveau 1 : 4 × 100 unités de recul.
        assert_eq!(pushed(target, caster, 400.0), Vec3::new(1400.0, 0.0, 0.0));
        assert_eq!(contact(caster, caster, 100.0).x, 100.0, "même point : à côté");
    }

    #[test]
    fn degats_de_zone() {
        // 60 de feu (Flame Force), 25 % de résistance au feu ; 120 de foudre sans résistance.
        let mut resist = [0; 8];
        resist[2] = 25;
        assert_eq!(field_damage(&[(12, 60.0), (16, 120.0)], &resist), vec![(12, 45), (16, 120)]);
        resist[2] = 100;
        assert_eq!(field_damage(&[(12, 60.0)], &resist), vec![(12, 1)], "1 au moins");
        let t = SkillTemplate { tid: 370, at_field: 10001, ..Default::default() };
        let lv = SkillLevel { area_r: 400.0, interval_ms: 2500, duration_ms: 480_000, area_damage: vec![(12, 60.0)], ..Default::default() };
        let now = Instant::now();
        let f = field_of(&t, &lv, 1, Anchor::Player(1), lv.duration_ms, now).unwrap();
        assert!(!f.aura && f.interval == Duration::from_millis(2500) && f.radius == 400.0);
        assert!(field_of(&SkillTemplate { at_field: 10003, ..t.clone() }, &lv, 1, Anchor::Player(1), 1, now).unwrap().aura, "Holy Ground");
        assert!(field_of(&SkillTemplate { at_field: 0, ..t }, &lv, 1, Anchor::Player(1), 1, now).is_none());
    }

    #[test]
    fn provocation_et_restrictions() {
        let provoke = SkillTemplate { name: "Provoke".into(), range_code: 202, ..Default::default() };
        let zone = SkillTemplate { name: "Zone Provoke".into(), range_code: 1304, ..Default::default() };
        assert_eq!(provoke_level(&provoke).map(|l| l.area_r), Some(0.0));
        assert_eq!(provoke_level(&zone).map(|l| l.area_r), Some(PROVOKE_AREA));
        assert!(provoke_level(&SkillTemplate { name: "Provocation".into(), ..Default::default() }).is_none());
        let mut st = WorldState::default();
        st.by_cid.insert(7, 1);
        let strike = SkillTemplate { target_restriction: restriction::SIEGE_OBJECT, ..Default::default() };
        assert!(!restricted(&st, &strike, 7), "un joueur n'est pas un objet de siège");
        assert!(restricted(&st, &SkillTemplate { target_restriction: 7, ..Default::default() }, 7));
    }
}
