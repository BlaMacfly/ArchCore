//! Compétences des monstres : celles de leur IA (`aidatatable.txt` des tables du serveur d'origine, table
//! `monster_ai`), désignée par le point d'apparition (`monster_spawn.ai_tid`).
//!
//! ```text
//!   IA : jusqu'à cinq compétences « SkillN » (TID;niveau), chacune avec sa condition « SkillConditionN »
//!        (« HP;100%;이하 » : PV au plus à 100 % du maximum ; « 이상 » : au moins) et son nombre « SkillCountN »
//!        (« 무한 » : sans limite ; « 횟수;1 » : une fois par vie). « UseNormalAttack » 0 : pas de coup normal.
//!   Lancement : une compétence offensive prête (relance écoulée, condition remplie, cible à portée) part à la
//!        place d'un coup normal, ou de la poursuite si la cible est déjà à sa portée : AGPMSKILL op 7
//!        (lancement, TID au champ 4) aux joueurs qui voient le monstre, sans op 0 : comme pour le lancement
//!        d'un autre joueur, le client n'a pas à connaître la compétence. Au bout de l'incantation, op 8 (résultat : PV restants, dégâts en négatif) aux joueurs qui voient chaque cible.
//!   Effet : dégâts comme ceux d'un joueur, à partir des dégâts physiques et élémentaires du monstre ; zone sur
//!        les joueurs voisins ; effet durable, dégâts sur la durée et états spéciaux sur chaque cible.
//!   Coups chargés : une compétence sans SKILL_SPEC, sans incantation ni relance, qui affaiblit (« buff_type »
//!        1, comme « <MOB>Water Elemental Skill ») accompagne les coups normaux qui touchent : son effet prend
//!        avec la chance « skill_rate ».
//! ```
//! Les tables ne disent pas à quel rythme le serveur d'origine lançait ces compétences : ArchCore lance chacune
//! dès qu'elle est prête, au rythme de sa relance (`recast_time`).

use super::combat::{self, Attacker, Defender};
use super::effects::{self, status};
use super::monsters::Monster;
use super::skills::{self, Area, Owned, SkillLevel, SkillTemplate};
use super::{WorldCtx, WorldState};
use crate::character::POINT_HP;
use crate::debug;
use archcore_net::Vec3;
use rand::Rng;
use std::time::{Duration, Instant};

/// Compétences au plus par IA.
pub const MAX_AI_SKILLS: usize = 5;
/// Rayon du corps d'un joueur, ajouté à la portée du monstre.
const PLAYER_HIT_RANGE: f64 = 50.0;
const REACH_TOLERANCE: f64 = 50.0;
/// Cibles au plus d'une zone sans « max_target_num ».
const MAX_TARGETS: usize = 12;

/// Une compétence de l'IA.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AiSkill {
    pub tid: u32,
    pub level: u32,
    /// Condition sur les PV : pour cent du maximum, et sens (vrai : au plus, faux : au moins).
    pub hp_pct: f64,
    pub hp_at_most: bool,
    /// Lancements au plus par vie (`None` : sans limite).
    pub max_uses: Option<u32>,
}

impl AiSkill {
    pub fn hp_ok(&self, hp: i32, max_hp: i32) -> bool {
        let pct = hp as f64 * 100.0 / max_hp.max(1) as f64;
        if self.hp_at_most { pct <= self.hp_pct } else { pct >= self.hp_pct }
    }
}

/// IA d'un monstre (colonnes utiles d'`aidatatable.txt`).
#[derive(Clone, Debug, PartialEq)]
pub struct MonsterAi {
    pub normal_attack: bool,
    pub skills: Vec<AiSkill>,
    /// Attaque préventive (« PreemptiveType » 1) : portée (« PreemptiveRange », en mètres de 100 unités) et
    /// conditions sur la cible.
    pub preemptive: Option<Preemptive>,
}

/// Agression d'un monstre « préventif » : il attaque de lui-même un joueur à portée qui remplit ses conditions.
#[derive(Clone, Debug, PartialEq)]
pub struct Preemptive {
    pub range: f64,
    /// Niveaux de cible interdits : (niveau, au-dessus) — « 타겟;LV;90;이상;금지 » interdit les cibles de niveau 90
    /// ou plus ; « 이하 » celles de ce niveau ou moins.
    pub forbidden_levels: Vec<(i32, bool)>,
}

impl Preemptive {
    pub fn allows(&self, level: i32) -> bool {
        !self.forbidden_levels.iter().any(|(l, above)| if *above { level >= *l } else { level <= *l })
    }
}

impl MonsterAi {
    /// Lit une ligne de `monster_ai.data`.
    pub fn from_data(d: &serde_json::Value) -> Self {
        let text = |k: String| d.get(&k).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let mut skills = Vec::new();
        for n in 1..=MAX_AI_SKILLS {
            let skill = text(format!("Skill{n}"));
            let mut parts = skill.split(';');
            let Some(tid) = parts.next().and_then(|t| t.trim().parse::<u32>().ok()).filter(|t| *t > 0) else { continue };
            let level = parts.next().and_then(|l| l.trim().parse::<u32>().ok()).unwrap_or(1).max(1);
            // « HP;100%;이하 » ; sans condition lisible : toujours.
            let cond = text(format!("SkillCondition{n}"));
            let c: Vec<&str> = cond.split(';').map(str::trim).collect();
            let (hp_pct, hp_at_most) = match c.as_slice() {
                ["HP", pct, sense, ..] => (pct.trim_end_matches('%').parse().unwrap_or(100.0), *sense != "이상"),
                _ => (100.0, true),
            };
            let count = text(format!("SkillCount{n}"));
            let max_uses = count.strip_prefix("횟수;").and_then(|x| x.trim_end_matches(';').trim().parse().ok());
            skills.push(AiSkill { tid, level, hp_pct, hp_at_most, max_uses });
        }
        let preemptive = (text("PreemptiveType".into()) == "1").then(|| {
            let forbidden_levels = (1..=3)
                .filter_map(|n| {
                    let cond = text(format!("PreemptiveCondition{n}"));
                    let c: Vec<&str> = cond.split(';').map(str::trim).collect();
                    match c.as_slice() {
                        ["타겟", "LV", level, sense, "금지", ..] => Some((level.parse().ok()?, *sense == "이상")),
                        _ => None,
                    }
                })
                .collect();
            let range = text("PreemptiveRange".into()).parse::<f64>().unwrap_or(0.0) * 100.0;
            Preemptive { range, forbidden_levels }
        });
        MonsterAi { normal_attack: text("UseNormalAttack".into()) != "0", skills, preemptive }
    }
}

/// État des compétences d'un monstre pendant sa vie : identifiants attribués, fin de relance, lancements faits.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkillState {
    pub ids: Vec<u32>,
    pub ready: Vec<Option<Instant>>,
    pub uses: Vec<u32>,
}

/// Incantation en cours d'un monstre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MonsterCast {
    pub slot: usize,
    /// Joueur visé (CID) et sa position au lancement.
    pub target: u32,
    pub target_pos: Vec3,
    pub complete_at: Instant,
}

/// Coup chargé : affaiblissement posé par un coup normal, sans incantation.
pub fn is_proc(t: &SkillTemplate, lv: &SkillLevel) -> bool {
    !t.offensive() && t.target == 0 && lv.debuff && lv.cast_ms == 0 && lv.recast_ms == 0
}

/// Portée de lancement : celle de la compétence, le rayon d'une zone autour du lanceur, sinon la portée du
/// monstre.
fn reach(t: &SkillTemplate, lv: &SkillLevel, melee: f64) -> f64 {
    let base = if t.area() == (Area::Sphere { around_target: false }) && lv.area_r > 0.0 {
        lv.area_r
    } else if lv.range > 0.0 {
        lv.range
    } else {
        melee
    };
    base + PLAYER_HIT_RANGE + REACH_TOLERANCE
}

/// Identifiant de la compétence `slot` du monstre, attribué au premier usage.
fn skill_id(st: &mut WorldState, cid: u32, slot: usize) -> u32 {
    let n = st.monsters.ai_of(cid).map(|a| a.skills.len()).unwrap_or(0);
    let state = st.monsters.skill_state.entry(cid).or_insert_with(|| SkillState { ids: vec![0; n], ready: vec![None; n], uses: vec![0; n] });
    if state.ids.get(slot).copied().unwrap_or(0) == 0 {
        st.next_skill_id += 1;
        if let Some(id) = st.monsters.skill_state.get_mut(&cid).and_then(|s| s.ids.get_mut(slot)) {
            *id = st.next_skill_id;
        }
    }
    st.monsters.skill_state.get(&cid).and_then(|s| s.ids.get(slot).copied()).unwrap_or(0)
}

/// Compétence offensive que le monstre peut lancer sur une cible à `distance`, s'il en a une prête.
pub fn ready_skill(ctx: &WorldCtx, st: &WorldState, m: &Monster, distance: f64, now: Instant) -> Option<usize> {
    let ai = st.monsters.ai_of(m.cid)?;
    if st.monsters.status(m.cid, now) & status::NO_SKILL != 0 {
        return None;
    }
    let melee = st.monsters.stats_of(m.tid).range.max(50) as f64;
    let state = st.monsters.skill_state.get(&m.cid);
    let ready: Vec<usize> = ai
        .skills
        .iter()
        .enumerate()
        .filter(|(slot, s)| {
            let Some((t, lv)) = ctx.skills.get(s.tid).and_then(|t| Some((t, t.level(s.level)?))) else { return false };
            let free = state.is_none_or(|x| x.ready.get(*slot).copied().flatten().is_none_or(|r| now >= r));
            let left = state.is_none_or(|x| s.max_uses.is_none_or(|n| x.uses.get(*slot).copied().unwrap_or(0) < n));
            t.offensive() && free && left && s.hp_ok(m.hp, m.max_hp) && distance <= reach(t, lv, melee)
        })
        .map(|(slot, _)| slot)
        .collect();
    (!ready.is_empty()).then(|| ready[rand::thread_rng().gen_range(0..ready.len())])
}

/// Commence l'incantation de la compétence `slot` sur le joueur `target` : coût de la relance, AGPMSKILL op 7
/// aux joueurs qui voient le monstre.
pub fn begin(ctx: &WorldCtx, st: &mut WorldState, cid: u32, slot: usize, target: u32, target_pos: Vec3, now: Instant) {
    let Some(s) = st.monsters.ai_of(cid).and_then(|a| a.skills.get(slot).copied()) else { return };
    let Some((t, lv)) = ctx.skills.get(s.tid).and_then(|t| Some((t, t.level(s.level)?))) else { return };
    let id = skill_id(st, cid, slot);
    let owned = Owned { id, tid: s.tid, level: s.level, ready_at: None };
    let complete_at = now + Duration::from_millis(lv.cast_ms);
    if let Some(state) = st.monsters.skill_state.get_mut(&cid) {
        state.ready[slot] = Some(now + Duration::from_millis(lv.recast_ms.max(lv.cast_ms)));
        state.uses[slot] += 1;
    }
    st.monsters.casts.insert(cid, MonsterCast { slot, target, target_pos, complete_at });
    st.monsters.walking.remove(&cid);
    if let Some(m) = st.monsters.get_mut(cid) {
        m.ready_at = complete_at;
    }
    let viewers = combat::viewers(st, cid);
    combat::broadcast(st, &viewers, &skills::cast_packet(cid, &owned, target, target_pos, lv));
    debug!("[ia] {cid:#x} lance {} niveau {} sur {target:#x} (incantation {} ms)", t.name, s.level, lv.cast_ms);
}

/// Incantations arrivées à leur terme.
pub fn step(ctx: &WorldCtx, st: &mut WorldState, now: Instant) {
    let due: Vec<(u32, MonsterCast)> = st.monsters.casts.iter().filter(|(_, c)| now >= c.complete_at).map(|(cid, c)| (*cid, *c)).collect();
    for (cid, cast) in due {
        st.monsters.casts.remove(&cid);
        let Some(m) = st.monsters.get(cid).filter(|m| m.is_alive()).cloned() else { continue };
        // Étourdi, gelé ou endormi pendant l'incantation : elle tombe.
        if st.monsters.status(cid, now) & status::HELPLESS != 0 {
            continue;
        }
        complete(ctx, st, &m, cast, now);
    }
}

/// Joueurs touchés : la cible si elle n'est pas partie au-delà de la poursuite (téléportée, reconnectée
/// ailleurs…), puis ceux de la zone, les plus proches d'abord.
fn targets(st: &WorldState, m: &Monster, t: &SkillTemplate, lv: &SkillLevel, cast: &MonsterCast) -> Vec<u32> {
    let limit = super::ai::CHASE_LIMIT.max(reach(t, lv, 0.0));
    let alive = |sid: &u32| st.players.get(sid).is_some_and(|p| p.in_game && !p.dead && combat::dist2d(p.ch.pos(), m.pos) <= limit);
    let primary = st.by_cid.get(&cast.target).copied().filter(alive);
    if primary.is_none() {
        let far = st.by_cid.get(&cast.target).and_then(|sid| st.players.get(sid)).map(|p| combat::dist2d(p.ch.pos(), m.pos));
        debug!("[ia] {} ({:#x}) : la cible {:#x} échappe à {} ({far:?} unités, portée {limit:.0})", m.name, m.cid, cast.target, t.name);
    }
    let toward = primary.map(|sid| st.players[&sid].ch.pos()).unwrap_or(cast.target_pos);
    let center = match t.area() {
        Area::Sphere { around_target: false } => m.pos,
        _ => toward,
    };
    let mut out: Vec<u32> = primary.into_iter().collect();
    if t.area() != Area::Single {
        let mut near: Vec<(f64, u32)> = st
            .players
            .iter()
            .filter(|(sid, p)| !out.contains(sid) && p.in_game && !p.dead)
            .filter(|(_, p)| skills::in_area(t.area(), p.ch.pos(), center, m.pos, toward, lv))
            .map(|(sid, p)| (combat::dist2d(p.ch.pos(), center), *sid))
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        out.extend(near.into_iter().map(|(_, sid)| sid));
    }
    out.truncate(if lv.max_targets > 0 { lv.max_targets } else { MAX_TARGETS });
    out
}

fn complete(ctx: &WorldCtx, st: &mut WorldState, m: &Monster, cast: MonsterCast, now: Instant) {
    let Some(s) = st.monsters.ai_of(m.cid).and_then(|a| a.skills.get(cast.slot).copied()) else { return };
    let Some((t, lv)) = ctx.skills.get(s.tid).and_then(|t| Some((t, t.level(s.level)?))) else { return };
    let id = st.monsters.skill_state.get(&m.cid).and_then(|x| x.ids.get(cast.slot).copied()).unwrap_or(0);
    let owned = Owned { id, tid: s.tid, level: s.level, ready_at: None };
    for sid in targets(st, m, t, lv, &cast) {
        hit_player(ctx, st, m, t, lv, &owned, sid, now);
    }
}

/// Effet d'une compétence de monstre sur un joueur : dégâts immédiats, effet durable, états spéciaux.
#[allow(clippy::too_many_arguments)]
fn hit_player(ctx: &WorldCtx, st: &mut WorldState, m: &Monster, t: &SkillTemplate, lv: &SkillLevel, owned: &Owned, sid: u32, now: Instant) {
    let Some(p) = st.players.get(&sid) else { return };
    let shielded = p.invincible_until.is_some_and(|u| now < u) || p.skills.status_sent & (status::UNTOUCHABLE | status::SKILL_ATK_INVINCIBLE) != 0;
    let stats = st.monsters.stats_of(m.tid);
    let mods = st.monsters.mods(m.cid);
    let (min_damage, max_damage) = effects::monster_damage(stats.min_damage, stats.max_damage, &mods);
    let attacker = Attacker { level: m.level as i32, ar: stats.ar, min_damage, max_damage, ..Default::default() };
    let defender = Defender::of_player(&p.factors);
    let spirit = stats.spirit.map(|(lo, hi)| (lo as f64, hi.max(lo) as f64));
    let mut rng = rand::thread_rng();
    let damages = if t.instant_damage() && !shielded { skills::skill_damage(&mut rng, &attacker, &spirit, &defender, lv) } else { Vec::new() };
    let damages = combat::divert(st, sid, &damages);
    let total: i32 = damages.iter().map(|(_, d)| *d).sum();
    let p = st.players.get_mut(&sid).unwrap();
    super::recovery::in_combat(&mut p.timers, now);
    let hp = (p.factors.point[POINT_HP] - total).max(0);
    p.factors.point[POINT_HP] = hp;
    if total > 0 {
        effects::lift_status(&mut p.skills.effects, status::SLEEP);
    }
    let (pcid, mp) = (p.ch.id, p.factors.point[1]);
    let player_mods = effects::total(&p.skills.effects);
    combat::send_around(st, sid, &skills::result_packet(m.cid, owned, pcid, skills::result_factors(hp, mp, &damages)));
    debug!("[ia] {} → {} : {} ({total} dégât(s), {hp} PV)", m.name, st.players[&sid].ch.name, t.name);
    if hp == 0 {
        return combat::player_death(st, sid);
    }
    if shielded {
        return;
    }
    let state = skills::roll_status(&mut rng, lv, &player_mods);
    if lv.leaves_effect(state) {
        put_effect(ctx, st, sid, m, owned, lv, state, now);
    }
}

/// Pose sur un joueur l'effet d'une compétence de monstre (dégâts sur la durée compris).
#[allow(clippy::too_many_arguments)]
fn put_effect(ctx: &WorldCtx, st: &mut WorldState, sid: u32, m: &Monster, owned: &Owned, lv: &SkillLevel, state: u64, now: Instant) {
    let level = st.players.get(&sid).map(|p| p.factors.status[7]).unwrap_or(1);
    let penalty = (5 * (level - m.level as i32).max(0)).min(80) as f64;
    let dot = (!lv.dot.is_empty()).then(|| effects::Dot {
        damages: lv.dot.damages.iter().map(|(i, d)| (*i, ((d * (100.0 - penalty) / 100.0).round() as i32).max(1))).collect(),
        interval: Duration::from_millis(lv.dot.interval_ms),
        next_at: now + Duration::from_millis(lv.dot.interval_ms),
        caster_sid: effects::MONSTER_CASTER,
    });
    let e = skills::new_effect(owned, lv, m.tid, state, dot, now);
    skills::add_player_effect(ctx, st, sid, e);
    skills::sync_player_status(st, sid, now);
}

/// Coups chargés d'un monstre dont le coup normal vient de toucher le joueur `sid`.
pub fn on_hit(ctx: &WorldCtx, st: &mut WorldState, cid: u32, sid: u32, now: Instant) {
    let Some(m) = st.monsters.get(cid).cloned() else { return };
    let Some(ai) = st.monsters.ai_of(cid).cloned() else { return };
    for (slot, s) in ai.skills.iter().enumerate() {
        let Some(lv) = ctx.skills.get(s.tid).filter(|t| t.level(s.level).is_some_and(|lv| is_proc(t, lv))).and_then(|t| t.level(s.level)) else { continue };
        if !s.hp_ok(m.hp, m.max_hp) || rand::thread_rng().gen_range(0.0..100.0) >= lv.rate {
            continue;
        }
        if !st.players.get(&sid).is_some_and(|p| p.in_game && !p.dead) {
            return;
        }
        let id = skill_id(st, cid, slot);
        let owned = Owned { id, tid: s.tid, level: s.level, ready_at: None };
        debug!("[ia] {} ({cid:#x}) : coup chargé {} sur {}", m.name, s.tid, st.players[&sid].ch.name);
        put_effect(ctx, st, sid, &m, &owned, lv, lv.status, now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lecture_de_l_ia() {
        let d = serde_json::json!({ "NAME": "Chef", "UseNormalAttack": "1", "Skill1": "179;1", "SkillCondition1": "HP;100%;이하", "SkillCount1": "무한;",
            "Skill2": "195;2", "SkillCondition2": "HP;30%;이하", "SkillCount2": "횟수;1", "Skill4": "0;1" });
        let ai = MonsterAi::from_data(&d);
        assert!(ai.normal_attack);
        assert_eq!(ai.skills.len(), 2, "TID 0 ignoré");
        assert_eq!(ai.skills[0], AiSkill { tid: 179, level: 1, hp_pct: 100.0, hp_at_most: true, max_uses: None });
        assert_eq!(ai.skills[1], AiSkill { tid: 195, level: 2, hp_pct: 30.0, hp_at_most: true, max_uses: Some(1) });
        assert!(ai.skills[1].hp_ok(25, 100) && !ai.skills[1].hp_ok(31, 100));
        assert!(!MonsterAi::from_data(&serde_json::json!({ "UseNormalAttack": "0" })).normal_attack);
        assert!(MonsterAi::from_data(&serde_json::json!({})).normal_attack, "sans colonne : coups normaux");
        let p = MonsterAi::from_data(&serde_json::json!({ "PreemptiveType": "1", "PreemptiveCondition1": "타겟;LV;90;이상;금지", "PreemptiveRange": "8" })).preemptive.unwrap();
        assert_eq!(p.range, 800.0);
        assert!(p.allows(89) && !p.allows(90));
        assert!(MonsterAi::from_data(&serde_json::json!({ "PreemptiveRange": "8" })).preemptive.is_none());
        let above = AiSkill { hp_at_most: false, hp_pct: 50.0, ..ai.skills[0] };
        assert!(above.hp_ok(60, 100) && !above.hp_ok(40, 100));
    }

    #[test]
    fn coups_charges_et_portee() {
        let water = SkillLevel::with_spec(&serde_json::json!({ "percent.atk_speed": "-15", "buff_type": "1", "duration": "4000", "skill_rate": "20" }), &Default::default());
        let elemental = SkillTemplate { tid: 193, ..Default::default() };
        assert!(is_proc(&elemental, &water) && water.rate == 20.0 && water.lasting());
        let line = SkillLevel { range: 900.0, area_f1: 900.0, cast_ms: 1800, recast_ms: 6000, ..Default::default() };
        let confusion = SkillTemplate { tid: 179, target: 1, range_code: 1404, melee_attack: 101, ..Default::default() };
        assert!(!is_proc(&confusion, &line));
        assert_eq!(reach(&confusion, &line, 300.0), 1000.0);
        let wheel = SkillTemplate { target: 101, range_code: 304, ..Default::default() };
        assert_eq!(reach(&wheel, &SkillLevel { area_r: 500.0, ..Default::default() }, 300.0), 600.0, "zone autour du monstre");
        assert_eq!(reach(&SkillTemplate { target: 1, ..Default::default() }, &SkillLevel::default(), 300.0), 400.0, "portée du monstre");
    }
}
