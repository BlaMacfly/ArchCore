//! Arme ou bouclier exigé par une compétence (colonne `requirement` de SKILL_SPEC).
//!
//! ```text
//!   Masque (rangé comme le fait le chargeur de SKILL_SPEC d'Archonia) : 1 et 2 → 0x1 arme, 3 → 0x4 bouclier, 4 → 0x8
//!     outil de production (`product.rs`), 5 → 0x11 épée à une main, 6 → 0x21 hache à une main, 7 → 0x41 arme
//!     contondante, 8 → 0x81 arme à deux mains, 9 → 0x101 bâton, 10 → 0x201 baguette, 11 → 0x401 arc, 12 → 0x801
//!     arbalète, 13 → 0x1001 katariya, 14 → 0x2001 chakram, 15 → 0x4001 épée de la main gauche, 16 → 0x8001 arme de la
//!     main droite seule, 17 → 0x10001 Karon, 18 → 0x20001 Xenon, 19 → 0x40001 Karon et Xenon.
//!   Bits remplis par l'équipement porté (objets cassés exclus, armes des mains seulement) : 0x1 une arme (« Kind » 2),
//!     0x4 un bouclier (« Kind » 4) ; types d'arme (« Type » d'itemdatatable) : 0 épée → 0x10, 1 hache → 0x20, 2 et 3
//!     contondantes → 0x40, 4 à 7 à deux mains → 0x80, 14 bâton → 0x100, 16 baguette → 0x200, 10 arc → 0x400, 11
//!     arbalète → 0x800, 18 katariya → 0x1000, 19 chakram → 0x2000, deux armes dont une rapière ou une dague (21 à 23)
//!     → 0x4000, une arme en main droite et rien en main gauche → 0x8000, 24-25 Karon → 0x10000, 26-27 Xenon →
//!     0x20000, Karon et Xenon → 0x40000. Désarmé (état 18) : aucun bit d'arme.
//!   Lancement (AGPMSKILL op 7) : exigence non remplie → refus au seul lanceur, type d'action 8 (« This skill cannot be
//!     used with the equipped weapon »).
//!   Passive (unités 9 de `attribute`) : ne compte dans les facteurs que si l'exigence est remplie ; revue à chaque
//!     recalcul des facteurs (changement d'équipement, réparation, monture…).
//! ```
//! Règle proposée (le client ne contrôle rien) ; `Rules.WeaponRequirement` (1) l'active. Les `arg1` des passives
//! (Power Bow 6, Power Staff 10…) et la colonne `condition` (1000 : déclenchement à la chance, déjà joué ; 2000, 4000 :
//! rien) ne changent rien ici.

use super::equipment::{part, KIND_WEAPON};
use super::items::{Inventory, ItemCatalog};

/// Bits du masque.
pub mod bit {
    pub const WEAPON: u32 = 0x1;
    pub const SHIELD: u32 = 0x4;
    pub const TOOL: u32 = 0x8;
    pub const SWORD: u32 = 0x10;
    pub const AXE: u32 = 0x20;
    pub const BLUNT: u32 = 0x40;
    pub const TWO_HANDED: u32 = 0x80;
    pub const STAFF: u32 = 0x100;
    pub const WAND: u32 = 0x200;
    pub const BOW: u32 = 0x400;
    pub const CROSSBOW: u32 = 0x800;
    pub const KATARIYA: u32 = 0x1000;
    pub const CHAKRAM: u32 = 0x2000;
    pub const OFF_HAND_SWORD: u32 = 0x4000;
    pub const RIGHT_HAND_ONLY: u32 = 0x8000;
    pub const KARON: u32 = 0x1_0000;
    pub const XENON: u32 = 0x2_0000;
    pub const KARON_XENON: u32 = 0x4_0000;
}

/// Type d'action du refus (mini SKILL_ACTION) : « NEED_REQUIREMENT ».
pub const REFUSAL: i64 = 8;
/// « Kind » d'un bouclier.
pub const KIND_SHIELD: i64 = 4;

/// Masque d'une valeur de la colonne `requirement`.
pub fn mask_of(requirement: i64) -> u32 {
    use bit::*;
    match requirement {
        1 | 2 => WEAPON,
        3 => SHIELD,
        4 => TOOL,
        5 => WEAPON | SWORD,
        6 => WEAPON | AXE,
        7 => WEAPON | BLUNT,
        8 => WEAPON | TWO_HANDED,
        9 => WEAPON | STAFF,
        10 => WEAPON | WAND,
        11 => WEAPON | BOW,
        12 => WEAPON | CROSSBOW,
        13 => WEAPON | KATARIYA,
        14 => WEAPON | CHAKRAM,
        15 => WEAPON | OFF_HAND_SWORD,
        16 => WEAPON | RIGHT_HAND_ONLY,
        17 => WEAPON | KARON,
        18 => WEAPON | XENON,
        19 => WEAPON | KARON_XENON,
        _ => 0,
    }
}

/// Bit d'un type d'arme.
pub fn type_bit(weapon_type: i64) -> u32 {
    use bit::*;
    match weapon_type {
        0 => SWORD,
        1 => AXE,
        2 | 3 => BLUNT,
        4..=7 => TWO_HANDED,
        14 => STAFF,
        16 => WAND,
        10 => BOW,
        11 => CROSSBOW,
        18 => KATARIYA,
        19 => CHAKRAM,
        24 | 25 => KARON,
        26 | 27 => XENON,
        _ => 0,
    }
}

/// Bits remplis par ce que tiennent les mains : (partie, « Kind », « Type ») de chaque objet. `disarmed` : aucune arme.
pub fn held_bits(hands: &[(i64, i64, i64)], disarmed: bool) -> u32 {
    use bit::*;
    let mut out = 0;
    let weapons: Vec<(i64, i64)> = hands.iter().filter(|(_, k, _)| *k == KIND_WEAPON && !disarmed).map(|(p, _, t)| (*p, *t)).collect();
    if hands.iter().any(|(_, k, _)| *k == KIND_SHIELD) {
        out |= SHIELD;
    }
    if weapons.is_empty() {
        return out;
    }
    out |= WEAPON;
    for (_, t) in &weapons {
        out |= type_bit(*t);
    }
    if weapons.len() >= 2 && weapons.iter().any(|(_, t)| (21..=23).contains(t)) || weapons.iter().any(|(_, t)| *t == 23) {
        out |= OFF_HAND_SWORD;
    }
    if weapons.iter().any(|(p, _)| *p == part::HAND_RIGHT) && !hands.iter().any(|(p, _, _)| *p == part::HAND_LEFT) {
        out |= RIGHT_HAND_ONLY;
    }
    if out & (KARON | XENON) == KARON | XENON {
        out |= KARON_XENON;
    }
    out
}

/// Bits remplis par l'équipement porté d'un joueur (mains, objets cassés exclus).
pub fn held(inv: &Inventory, catalog: &ItemCatalog, disarmed: bool) -> u32 {
    let hands: Vec<(i64, i64, i64)> = inv
        .equipment
        .iter()
        .filter(|(at, _)| matches!(**at, part::HAND_LEFT | part::HAND_RIGHT))
        .filter_map(|(at, i)| catalog.get(i.tid).and_then(|d| d.equip.as_ref()).filter(|e| !e.is_broken(i.durability)).map(|e| (*at, e.kind, e.weapon_type)))
        .collect();
    held_bits(&hands, disarmed)
}

/// L'exigence `mask` est remplie par `held` (l'outil de production est contrôlé par `product.rs`). Toujours vrai si la
/// règle est coupée.
pub fn met(mask: u32, held: u32) -> bool {
    !crate::config::rules().weapon_requirement || mask & !bit::TOOL & !held == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masque_des_exigences() {
        assert_eq!((mask_of(1), mask_of(2), mask_of(3), mask_of(4)), (0x1, 0x1, 0x4, 0x8));
        assert_eq!((mask_of(5), mask_of(11), mask_of(13), mask_of(17), mask_of(19)), (0x11, 0x401, 0x1001, 0x10001, 0x40001));
        assert_eq!((mask_of(0), mask_of(20)), (0, 0));
    }

    #[test]
    fn armes_tenues() {
        let (l, r) = (part::HAND_LEFT, part::HAND_RIGHT);
        // Arc en main gauche : arme, arc.
        let bow = held_bits(&[(l, KIND_WEAPON, 10)], false);
        assert!(met(mask_of(11), bow) && met(mask_of(1), bow) && !met(mask_of(12), bow) && !met(mask_of(3), bow));
        // Épée en main droite, bouclier en main gauche : épée, bouclier, pas « main droite seule ».
        let knight = held_bits(&[(r, KIND_WEAPON, 0), (l, KIND_SHIELD, 0)], false);
        assert!(met(mask_of(5), knight) && met(mask_of(3), knight) && !met(mask_of(16), knight));
        assert!(met(mask_of(16), held_bits(&[(r, KIND_WEAPON, 0)], false)), "main droite seule");
        // Rapière et dague : épée de la main gauche.
        assert!(met(mask_of(15), held_bits(&[(r, KIND_WEAPON, 21), (l, KIND_WEAPON, 22)], false)));
        assert!(!met(mask_of(15), held_bits(&[(r, KIND_WEAPON, 21)], false)));
        // Karon et Xenon.
        let both = held_bits(&[(r, KIND_WEAPON, 26), (l, KIND_WEAPON, 24)], false);
        assert!(met(mask_of(17), both) && met(mask_of(18), both) && met(mask_of(19), both));
        assert!(!met(mask_of(19), held_bits(&[(l, KIND_WEAPON, 24)], false)));
        // Mains vides ou désarmé : aucune compétence d'arme ; l'outil n'est pas contrôlé ici.
        assert!(!met(mask_of(1), 0) && met(0, 0) && met(mask_of(4), 0));
        assert!(!met(mask_of(11), held_bits(&[(l, KIND_WEAPON, 10)], true)));
        assert_eq!(held_bits(&[(l, KIND_SHIELD, 0)], true), bit::SHIELD, "le bouclier reste");
    }
}
