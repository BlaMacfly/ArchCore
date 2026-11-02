//! Munitions : flèches (194 « Arrow », usage 5) et carreaux (238 « Bolt », usage 6), consommables de l'inventaire
//! (aucun emplacement équipé : ni « Part » ni contrôle côté client).
//!
//! ```text
//!   Attaque normale d'un joueur dont l'arme est un arc (« Type » 10) : une flèche de l'inventaire par coup porté (touché
//!     ou non) ; arbalète (11) : un carreau. Sans munition : pas de coup, AGPMOPTIMIZEDCHARACTION au seul joueur avec le
//!     résultat 2 (« You do not have any arrows ») ou 3 (« You do not have any bolts »), attaque arrêtée.
//!   Compétence à « cost_arrow » n > 0 : même munition selon l'arme portée ; contrôle au début (à côté des PM), moins de
//!     n → refus au seul lanceur, type d'action 12 (« Not enough arrows to use this skill » : Archonia n'a pas de texte
//!     pour les carreaux, 12 vaut aussi pour l'arbalète). Paiement à la fin de l'incantation. Katariya, chakram et toute
//!     arme qui n'est ni arc ni arbalète : rien à payer (l'infobulle du client le dit : « Except for Kataria »).
//!   Piles : la plus entamée d'abord (la plus petite), puis l'ordre de l'inventaire ; AGPMITEM op 2 (quantité) ou op 1
//!     (pile vide). Monstres et invocations : jamais de munitions.
//! ```
//! Règle proposée (le refus vient du serveur, le client n'en contrôle rien) ; `Rules.Ammunition` (1) l'active.

use super::equipment::{part, KIND_WEAPON};
use super::items::{item_remove, Inventory, ItemCatalog};
use super::{Player, WorldCtx, WorldState};
use crate::debug;

/// TID des flèches et des carreaux.
pub const ARROW: u32 = 194;
pub const BOLT: u32 = 238;
/// « Type » d'un arc, d'une arbalète.
pub const WEAPON_BOW: i64 = 10;
pub const WEAPON_CROSSBOW: i64 = 11;
/// Résultats d'attaque (AGPMOPTIMIZEDCHARACTION champ 2) : plus de flèche, plus de carreau.
pub const RESULT_NO_ARROW: i64 = 2;
pub const RESULT_NO_BOLT: i64 = 3;
/// Type d'action du refus d'une compétence (mini SKILL_ACTION) : pas assez de flèches.
pub const SKILL_REFUSAL: i64 = 12;

/// Munition d'une arme : (TID, résultat d'attaque sans munition) ; `None` pour une arme qui n'en use pas.
pub fn for_weapon(weapon_type: i64) -> Option<(u32, i64)> {
    match weapon_type {
        WEAPON_BOW => Some((ARROW, RESULT_NO_ARROW)),
        WEAPON_CROSSBOW => Some((BOLT, RESULT_NO_BOLT)),
        _ => None,
    }
}

/// Type de l'arme tenue en main (arc ou arbalète d'abord), objets cassés exclus.
pub fn weapon_type(inv: &Inventory, catalog: &ItemCatalog) -> Option<i64> {
    let types: Vec<i64> = inv
        .equipment
        .iter()
        .filter(|(at, _)| matches!(**at, part::HAND_LEFT | part::HAND_RIGHT))
        .filter_map(|(_, i)| catalog.get(i.tid).and_then(|d| d.equip.as_ref()).filter(|e| e.kind == KIND_WEAPON && !e.is_broken(i.durability)).map(|e| e.weapon_type))
        .collect();
    types.iter().copied().find(|t| for_weapon(*t).is_some()).or_else(|| types.first().copied())
}

/// Munition qu'use le joueur, si la règle est active et son arme en use une.
pub fn needed(ctx: &WorldCtx, p: &Player) -> Option<(u32, i64)> {
    if !crate::config::rules().ammunition {
        return None;
    }
    weapon_type(&p.inventory, &ctx.catalog).and_then(for_weapon)
}

/// Munitions de ce TID dans l'inventaire.
pub fn count(inv: &Inventory, tid: u32) -> i64 {
    inv.items.values().filter(|i| i.tid == tid).map(|i| i.count).sum()
}

/// Piles à entamer pour `n` munitions : la plus petite d'abord, puis l'ordre de l'inventaire (IID). Rend (IID, nombre).
pub fn plan(piles: &[(u32, i64)], n: i64) -> Vec<(u32, i64)> {
    let mut sorted = piles.to_vec();
    sorted.sort_by_key(|(iid, count)| (*count, *iid));
    let mut left = n;
    let mut out = Vec::new();
    for (iid, count) in sorted {
        if left <= 0 {
            break;
        }
        let take = count.min(left);
        out.push((iid, take));
        left -= take;
    }
    out
}

/// Consomme `n` munitions `tid` du joueur (autant qu'il en a) : quantités (op 2) ou retraits (op 1) envoyés.
pub async fn consume(ctx: &WorldCtx, st: &mut WorldState, sid: u32, tid: u32, n: i64) -> anyhow::Result<()> {
    let Some(p) = st.players.get_mut(&sid) else { return Ok(()) };
    let piles: Vec<(u32, i64)> = p.inventory.items.values().filter(|i| i.tid == tid).map(|i| (i.iid, i.count)).collect();
    let cid = p.ch.id;
    for (iid, take) in plan(&piles, n) {
        match p.inventory.consume(&ctx.db, iid, take).await? {
            Some(left) => p.send_raw(p.inventory.count_packet(&left, cid)),
            None => p.send_raw(item_remove(iid, Some(cid))),
        }
    }
    debug!("[munitions] {} : {n} × {tid} consommé(s), {} restant(s)", p.ch.name, count(&p.inventory, tid));
    Ok(())
}

/// Coup normal sans munition : résultat 2 ou 3 au seul joueur, attaque arrêtée. Rend vrai si le coup ne part pas.
pub fn refuse_attack(ctx: &WorldCtx, st: &mut WorldState, sid: u32, target: u32) -> bool {
    let Some(p) = st.players.get(&sid) else { return false };
    let Some((tid, result)) = needed(ctx, p) else { return false };
    if count(&p.inventory, tid) > 0 {
        return false;
    }
    p.send_raw(super::combat::action_packet(p.ch.id, target, result, None));
    debug!("[munitions] {} : plus de munition {tid}, attaque arrêtée", p.ch.name);
    st.players.get_mut(&sid).unwrap().attack = None;
    true
}

/// Munitions qu'un lancement coûtera : (TID, nombre), si la compétence en coûte et l'arme en use.
pub fn skill_cost(ctx: &WorldCtx, p: &Player, cost_arrow: u32) -> Option<(u32, i64)> {
    if cost_arrow == 0 {
        return None;
    }
    needed(ctx, p).map(|(tid, _)| (tid, cost_arrow as i64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn munitions_et_piles() {
        assert_eq!((for_weapon(10), for_weapon(11)), (Some((ARROW, 2)), Some((BOLT, 3))));
        assert_eq!((for_weapon(18), for_weapon(19), for_weapon(0)), (None, None, None), "katariya, chakram, épée : rien");
        // La plus petite pile d'abord, puis l'IID.
        assert_eq!(plan(&[(1, 50), (2, 3), (3, 3)], 5), vec![(2, 3), (3, 2)]);
        assert_eq!(plan(&[(1, 50)], 1), vec![(1, 1)]);
        assert_eq!(plan(&[(1, 2)], 3), vec![(1, 2)], "autant qu'il y en a");
        assert!(plan(&[], 1).is_empty());
    }
}
