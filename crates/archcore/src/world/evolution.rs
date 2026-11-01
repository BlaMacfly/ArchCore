//! Évolution du Dragon Scion : Slayer, Orbiter et Summoner of Evolution.
//!
//! ```text
//!   Compétences 1189 Slayer, 1192 Orbiter, 1195 Summoner of Evolution (`transform_target` 5 de SKILL_SPEC), apprises
//!   chez les maîtres des compétences du Dragon Scion dès le niveau 20, lancées sur soi (AGPMSKILL op 7, action 1) :
//!     au bout de l'incantation → AGPMSKILL op 4 (effet sans durée de la compétence, au lanceur : le client s'en sert pour
//!     l'annulation), puis AGPMCHARACTER op 26 (champ 1 CID, champ 2 TID de la forme : `creature_tid` du niveau, 1722
//!     Slayer, 1723 Orbiter, 1732 Summoner) au joueur et à ceux qui le voient.
//!     Depuis une autre forme : op 27 (TID d'origine) puis op 26 — le client ignore un op 26 sur un personnage évolué.
//!     Relancer la forme active est refusé ; sous métamorphose aussi.
//!   Annulation : icône « Cancel Metamorphosis » → AGPMSKILL op 7, action de type 4, identifiant de la compétence
//!     d'évolution → op 5 (effet retiré) au lanceur, op 27 (champ 2 = TID d'origine, 1724) à tous.
//!   Vue : AGPMCHARACTER op 0 avec le TID de la forme et le bit 2 (0x04) du champ 18.
//! ```
//! `characters.template_id` reste celui du Scion ; la forme est enregistrée dans `characters.evolution_tid` (0 : aucune)
//! et rendue à l'entrée en jeu ; la mort ne la retire pas (effet de `duration_type` 2). Facteurs : modèle et croissance
//! (GrowUpFactor) de la forme au niveau du personnage, équipement, effets ; PV et PM gardés, bornés au nouveau maximum.
//! Compétences : toutes conservées, utilisables et passives appliquées selon les colonnes `require_class_*` de la forme
//! (`skills::form_class`) ; une arme que la classe de la forme n'admet pas ne compte plus (`equipment::counted`), et
//! s'équiper d'un objet non admis par la forme est refusé (contrôle de l'équipement sur les facteurs). L'objet d'oubli
//! des compétences est refusé pendant l'évolution (`recovery.rs`).

use super::effects::{self, Effect, Mods};
use super::skills::{Owned, PlayerSkills, SkillLevel, SkillTemplate};
use super::{send_system, WorldCtx, WorldState};
use crate::character::{factors_mini, CharacterRow};
use crate::info;
use archcore_net::schema;
use archcore_net::{build_packet, Fields};

/// Race du Dragon Scion.
pub const DRAGON_SCION: i32 = 4;

/// Enregistre la forme d'un personnage : (TID de la forme, 0 : aucune ; CID).
pub const SAVE_FORM: &str = "UPDATE characters SET evolution_tid = ? WHERE id = ?";

/// Évolution en cours : TID de la forme, compétence qui l'a donnée (TID, identifiant de la session).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Evolved {
    pub tid: u32,
    pub skill_tid: u32,
    pub skill_id: u32,
}

/// AGPMCHARACTER op 26 : `cid` évolue vers `tid`.
pub fn evolution_packet(cid: u32, tid: u32) -> Vec<u8> {
    let c = &schema::CHARACTER;
    build_packet(c, &Fields::new(c).with(0, super::morph::op::EVOLUTION).with(1, cid).with(2, tid))
}

/// AGPMCHARACTER op 27 : `cid` revient au modèle `tid` (le client ne le connaît pas lui-même).
pub fn restore_packet(cid: u32, tid: u32) -> Vec<u8> {
    let c = &schema::CHARACTER;
    build_packet(c, &Fields::new(c).with(0, super::morph::op::RESTORE_EVOLUTION).with(1, cid).with(2, tid))
}

/// Effet annoncé de la compétence d'évolution (AGPMSKILL op 4 et 5) : sans durée.
fn effect(e: &Evolved, caster_tid: u32) -> Effect {
    Effect {
        skill_id: e.skill_id,
        tid: e.skill_tid,
        level: 1,
        caster_tid,
        duration_ms: 0,
        ends_at: None,
        mods: Mods::default(),
        dot: None,
        status: 0,
        status_until: None,
        caster_sid: effects::MONSTER_CASTER,
        purge: None,
    }
}

/// Forme donnée par un niveau de compétence d'évolution, si le serveur connaît son modèle.
pub fn form_of(ctx: &WorldCtx, lv: &SkillLevel) -> Option<u32> {
    lv.creature_tids.first().copied().filter(|t| ctx.templates.get(*t).is_some_and(|m| m.race == DRAGON_SCION))
}

/// Le lancement est-il permis ? Rend la forme visée, ou la raison du refus.
pub fn check(ctx: &WorldCtx, st: &WorldState, sid: u32, lv: &SkillLevel) -> Result<u32, &'static str> {
    let p = st.players.get(&sid).ok_or("joueur absent")?;
    if ctx.templates.get(p.ch.template_id).is_none_or(|t| t.race != DRAGON_SCION) {
        return Err("réservée au Dragon Scion");
    }
    if p.form.morph.is_some() {
        return Err("impossible sous métamorphose");
    }
    let tid = form_of(ctx, lv).ok_or("forme inconnue")?;
    if p.form.evolved.is_some_and(|e| e.tid == tid) {
        return Err("vous avez déjà cette forme");
    }
    Ok(tid)
}

/// Facteurs recalculés sur la forme courante, passives selon sa classe, envoyés au joueur et à ceux qui le voient.
fn apply_form(ctx: &WorldCtx, st: &mut WorldState, sid: u32) {
    let Some(p) = st.players.get_mut(&sid) else { return };
    super::equipment::recompute_factors(ctx, p);
    let (caster_tid, class) = (p.ch.template_id, super::skills::form_class(&p.factors));
    if super::skills::sync_passives(&ctx.skills, &mut p.skills, caster_tid, class) {
        super::equipment::recompute_factors(ctx, p);
    }
    let c = &schema::CHARACTER;
    let plain = build_packet(c, &Fields::new(c).with(0, 1).with(1, p.ch.id).with(7, factors_mini(&p.factors)));
    super::combat::send_around(st, sid, &plain);
}

/// Fin de l'incantation d'une compétence d'évolution : la forme change (op 27 puis op 26 depuis une autre forme).
pub async fn on_cast(ctx: &WorldCtx, st: &mut WorldState, sid: u32, owned: &Owned, t: &SkillTemplate, lv: &SkillLevel) -> anyhow::Result<()> {
    let tid = match check(ctx, st, sid, lv) {
        Ok(tid) => tid,
        Err(why) => {
            send_system(st, sid, &format!("{} : {why}.", t.name));
            return Ok(());
        }
    };
    let p = st.players.get_mut(&sid).unwrap();
    let (cid, origin) = (p.ch.id, p.ch.template_id);
    let previous = p.form.evolved.take();
    let now = Evolved { tid, skill_tid: owned.tid, skill_id: owned.id };
    p.form.evolved = Some(now);
    if let Some(old) = previous {
        p.send_raw(effects::remove_packet(cid, &effect(&old, origin)));
        super::combat::send_around(st, sid, &restore_packet(cid, origin));
    }
    ctx.db.characters.exec(SAVE_FORM, (tid, cid)).await?;
    apply_form(ctx, st, sid);
    let p = &st.players[&sid];
    p.send_raw(effects::add_packet(cid, &effect(&now, origin)));
    super::combat::send_around(st, sid, &evolution_packet(cid, tid));
    info!("[évolution] {} : {} → forme {tid}{}", p.ch.name, t.name, previous.map(|o| format!(" (depuis {})", o.tid)).unwrap_or_default());
    Ok(())
}

/// Retour au Scion : effet retiré (op 5), op 27 à tous, forme effacée en base. Rend vrai si le joueur était évolué.
pub async fn cancel(ctx: &WorldCtx, st: &mut WorldState, sid: u32, why: &str) -> anyhow::Result<bool> {
    let Some(p) = st.players.get_mut(&sid) else { return Ok(false) };
    let Some(old) = p.form.evolved.take() else { return Ok(false) };
    let (cid, origin, name) = (p.ch.id, p.ch.template_id, p.ch.name.clone());
    p.send_raw(effects::remove_packet(cid, &effect(&old, origin)));
    ctx.db.characters.exec(SAVE_FORM, (0, cid)).await?;
    apply_form(ctx, st, sid);
    super::combat::send_around(st, sid, &restore_packet(cid, origin));
    info!("[évolution] {name} revient au Scion ({why})");
    Ok(true)
}

/// AGPMSKILL op 7, action 4, sur une compétence d'évolution : annulation demandée par l'icône du client.
pub async fn on_cancel_request(ctx: &WorldCtx, st: &mut WorldState, sid: u32, skill_id: u32) -> anyhow::Result<()> {
    let Some(p) = st.players.get(&sid) else { return Ok(()) };
    let Some(e) = p.form.evolved else { return Ok(()) };
    let asked = p.skills.by_id(skill_id).map(|s| s.tid);
    if asked.and_then(|tid| ctx.skills.get(tid)).is_some_and(|t| t.is_evolution()) {
        cancel(ctx, st, sid, &format!("annulation (compétence {})", asked.unwrap_or(e.skill_tid))).await?;
    }
    Ok(())
}

/// Forme enregistrée rendue à l'entrée en jeu : un modèle du Dragon Scion que donne une compétence d'évolution connue.
pub fn at_entry(ctx: &WorldCtx, ch: &CharacterRow, skills: &PlayerSkills) -> Option<Evolved> {
    if ch.evolution_tid == 0 || ctx.templates.get(ch.template_id).is_none_or(|t| t.race != DRAGON_SCION) {
        return None;
    }
    skills.list.iter().find_map(|s| {
        let t = ctx.skills.get(s.tid).filter(|t| t.is_evolution())?;
        let lv = t.level(s.level)?;
        (form_of(ctx, lv) == Some(ch.evolution_tid)).then_some(Evolved { tid: ch.evolution_tid, skill_tid: s.tid, skill_id: s.id })
    })
}

/// Entrée en jeu évolué : l'effet de la compétence (op 4), après ses compétences.
pub fn on_enter(st: &WorldState, sid: u32) {
    let Some(p) = st.players.get(&sid) else { return };
    if let Some(e) = p.form.evolved {
        p.send_raw(effects::add_packet(p.ch.id, &effect(&e, p.ch.template_id)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archcore_net::decode_packet;

    #[test]
    fn paquets_d_evolution() {
        let p = decode_packet(&schema::CHARACTER, &evolution_packet(9, 1722)).unwrap();
        assert_eq!((p.int(0), p.int(1), p.int(2)), (Some(0x1a), Some(9), Some(1722)));
        let p = decode_packet(&schema::CHARACTER, &restore_packet(9, 1724)).unwrap();
        assert_eq!((p.int(0), p.int(1), p.int(2)), (Some(0x1b), Some(9), Some(1724)));
        // Effet annoncé : TID de la compétence, durée 0, TID du lanceur.
        let e = Evolved { tid: 1722, skill_tid: 1189, skill_id: 41 };
        let k = decode_packet(&schema::SKILL, &effects::add_packet(9, &effect(&e, 1724))).unwrap();
        assert_eq!((k.int(0), k.int(1)), (Some(4), Some(41)));
    }

    #[test]
    fn colonne_creature_tid() {
        let lv = SkillLevel::from_consts(&serde_json::json!({ "creature_tid": "1722", "require_class_slayer": "1", "require_class_scion": "1" }));
        assert_eq!(lv.creature_tids, vec![1722]);
        assert!(lv.allows(super::super::skills::class_column(4, 3)) && lv.allows(super::super::skills::class_column(4, 1)));
        assert!(!lv.allows(super::super::skills::class_column(4, 2)), "Orbiter : pas la colonne du Slayer");
        let mutation = SkillLevel::from_consts(&serde_json::json!({ "creature_tid": "100;103;0;104" }));
        assert_eq!(mutation.creature_tids, vec![100, 103, 104]);
    }
}
