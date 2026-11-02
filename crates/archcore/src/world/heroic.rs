//! Système héroïque : dégâts, défense et résistances héroïques (facteurs d'attribut 11 à 14).
//!
//! ```text
//!   Facteurs, envoyés avec les autres (la fenêtre du personnage d'Archonia les affiche ; le client ne calcule aucun
//!   dégât héroïque : ses lecteurs n'ont que la fenêtre pour appelant) :
//!     dégâts héroïques (type 7, attribut 11, min et max) : objets portés (« Heroic_Min_Damage », « Heroic_Max_Damage »
//!         d'itemdatatable), options (mêmes colonnes d'itemoptiontable), effets (« point.dmg_min_heroic »,
//!         « point.dmg_max_heroic ») ;
//!     défense héroïque (type 8, points, attribut 11) : objets et options (« Heroic_Defense_Point »), effets
//!         (« point.def_point_heroic ») ;
//!     résistances héroïques (type 8, taux, attributs 12 mêlée, 13 distance, 14 magie) : effets
//!         (« point.heroic_melee_Resistance », « …ranged… », « …magic… ») ; les options de résistance sont d'Archonia.
//!     Monstres : colonnes du modèle (CharacterDataTable de l'épisode 8 : toutes vides).
//!   Coup normal qui touche : jet uniforme entre min et max (le max si min > max), moins la défense héroïque de la cible,
//!     × (100 − r) / 100. r : résistance de la cible choisie par la classe de l'assaillant (archer : distance, mage :
//!     magie, autres : mêlée), 80 au plus, seulement quand l'assaillant est un joueur (un monstre : 0, la défense compte).
//!     0 au moins (sans dégâts héroïques, aucun point), pas doublé au critique ; point 19 du résultat (en négatif),
//!     ajouté aux PV ôtés.
//!   Compétence offensive qui touche : part de base (même calcul) × (100 + « dmg_heroic_percent ») / 100 +
//!     « dmg_heroic », le tout × (100 − p) / 100, p = écart de niveau (cible − lanceur, de 0 à 80). Dégâts sur la durée :
//!     « dot_dmg_heroic », part héroïque de chaque tic (`effects::DOT_COLUMNS`).
//! ```
//! Règle proposée, d'après les colonnes de l'épisode 8 (aucun binaire ne montre le calcul) ; `Rules.Heroic` (1) l'active.
//! Non fait : renvoi héroïque (« reflect_heroic_attack », « heroic_reflect_max » : forme inconnue).

use super::combat::{class, Attacker, Defender};
use crate::character::Factors;
use rand::Rng;

/// Attribut héroïque des facteurs de dégâts et de défense ; les résistances suivent (12 mêlée, 13 distance, 14 magie).
pub const ATTR: usize = 11;
/// Indice des dégâts héroïques dans le résultat d'une action (facteurs « points »).
pub const RESULT: usize = 19;
/// Résistance héroïque retenue au plus (pour cent).
pub const MAX_RESIST: f64 = 80.0;
/// Pénalité d'écart de niveau d'une compétence, au plus (pour cent).
pub const MAX_LEVEL_PENALTY: i32 = 80;

/// Dégâts héroïques (min, max) de facteurs.
pub fn damage_of(f: &Factors) -> (f64, f64) {
    (f.damage_min[ATTR].max(0) as f64, f.damage_max[ATTR].max(0) as f64)
}

/// Défense héroïque et résistances (mêlée, distance, magie) de facteurs.
pub fn defense_of(f: &Factors) -> (f64, [f64; 3]) {
    (f.defense_point[ATTR].max(0) as f64, std::array::from_fn(|i| f.defense_rate[ATTR + 1 + i] as f64))
}

/// Résistance de la cible qui vaut contre un assaillant de cette classe : 0 mêlée, 1 distance (archer), 2 magie (mage).
pub fn resist_slot(attacker_class: i32) -> usize {
    match attacker_class {
        class::ARCHER => 1,
        class::MAGE => 2,
        _ => 0,
    }
}

/// Part héroïque d'un coup qui touche (voir l'en-tête) ; 0 sans dégâts héroïques ou si la règle est coupée.
pub fn hit(rng: &mut impl Rng, a: &Attacker, d: &Defender) -> i32 {
    let (lo, hi) = a.heroic;
    if hi <= 0.0 || !crate::config::rules().heroic {
        return 0;
    }
    let raw = if lo < hi { rng.gen_range(lo.max(0.0)..=hi) } else { hi };
    let r = if a.player { d.heroic_resist[resist_slot(a.class)].clamp(0.0, MAX_RESIST) } else { 0.0 };
    (((raw - d.heroic_def).max(0.0) * (100.0 - r) / 100.0).round() as i32).max(0)
}

/// Part héroïque d'une compétence : base × (100 + pour cent) / 100 + bonus, diminuée de l'écart de niveau.
pub fn skill_part(base: i32, pct: f64, flat: f64, caster_level: i32, target_level: i32) -> i32 {
    if !crate::config::rules().heroic || base <= 0 && flat <= 0.0 {
        return 0;
    }
    let gap = (target_level - caster_level).clamp(0, MAX_LEVEL_PENALTY) as f64;
    (((base.max(0) as f64 * (100.0 + pct) / 100.0 + flat) * (100.0 - gap) / 100.0).round() as i32).max(0)
}

/// Ajoute une part héroïque non nulle aux dégâts d'un coup.
pub fn push(parts: &mut Vec<(usize, i32)>, x: i32) {
    if x > 0 {
        parts.push((RESULT, x));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn part_heroique_d_un_coup() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(5);
        let archer = Attacker { class: class::ARCHER, player: true, heroic: (100.0, 100.0), ..Default::default() };
        let target = Defender { heroic_def: 20.0, heroic_resist: [50.0, 25.0, 0.0], ..Default::default() };
        // 100 − 20 de défense, puis 25 % de résistance à distance (classe archer).
        assert_eq!(hit(&mut rng, &archer, &target), 60);
        // Guerrier : la résistance de mêlée, 50 %.
        assert_eq!(hit(&mut rng, &Attacker { class: 1, ..archer }, &target), 40);
        // Monstre assaillant : la défense compte, pas la résistance.
        assert_eq!(hit(&mut rng, &Attacker { player: false, ..archer }, &target), 80);
        // Résistance plafonnée à 80 %, jamais négatif, rien sans dégâts héroïques.
        assert_eq!(hit(&mut rng, &Attacker { class: class::MAGE, ..archer }, &Defender { heroic_resist: [0.0, 0.0, 300.0], ..target }), 16);
        assert_eq!(hit(&mut rng, &archer, &Defender { heroic_def: 500.0, ..target }), 0);
        assert_eq!(hit(&mut rng, &Attacker { heroic: (0.0, 0.0), ..archer }, &target), 0);
        // Min > max : le max.
        assert_eq!(hit(&mut rng, &Attacker { heroic: (300.0, 100.0), ..archer }, &Defender::default()), 100);
        for _ in 0..100 {
            let x = hit(&mut rng, &Attacker { heroic: (50.0, 70.0), ..archer }, &Defender::default());
            assert!((50..=70).contains(&x), "{x}");
        }
    }

    #[test]
    fn part_heroique_d_une_competence() {
        // Justice of Battle : 500 de plus, base 60, aucun écart de niveau.
        assert_eq!(skill_part(60, 0.0, 500.0, 100, 95), 560);
        // 50 % de plus, cible de 10 niveaux au-dessus : − 10 %.
        assert_eq!(skill_part(100, 50.0, 0.0, 90, 100), 135);
        // Écart plafonné à 80 %.
        assert_eq!(skill_part(100, 0.0, 0.0, 1, 120), 20);
        assert_eq!(skill_part(0, 0.0, 0.0, 1, 1), 0);
        assert_eq!((resist_slot(1), resist_slot(class::ARCHER), resist_slot(class::MAGE)), (0, 1, 2));
    }
}
