//! Secteurs du monde et visibilité.
//!
//! Le monde est découpé en 800 × 800 secteurs de 6400 unités, à partir de (-2 560 000, -2 560 000) en X et Z.
//! Un personnage voit apparaître tout ce qui entre dans les 3 × 3 secteurs centrés sur le sien : PNJ, monstres,
//! joueurs et objets au sol (règle du serveur d'origine). Il ne les perd qu'au-delà des 5 × 5 secteurs (au
//! moins 12 800 unités) : le client dessine les personnages jusqu'à 10 000 unités environ, alors qu'un retrait
//! à la sortie des 3 × 3 secteurs peut tomber à 6400 unités, sous les yeux du joueur.

use archcore_net::Vec3;

/// Côté d'un secteur.
pub const WIDTH: f64 = 6400.0;
/// Coin du monde (X et Z).
const START: f64 = -400.0 * WIDTH;
/// Distance horizontale qui couvre les 3 × 3 secteurs autour d'un point (pour les recherches par distance).
pub const REACH: f64 = 2.0 * WIDTH * std::f64::consts::SQRT_2;

/// Indices (X, Z) du secteur de `pos`.
pub fn of(pos: Vec3) -> (i64, i64) {
    (((pos.x as f64 - START) / WIDTH).floor() as i64, ((pos.z as f64 - START) / WIDTH).floor() as i64)
}

/// Ce qui est en `target` est dans les 3 × 3 secteurs autour de `viewer` : il lui apparaît.
pub fn sees(viewer: Vec3, target: Vec3) -> bool {
    within(viewer, target, 1)
}

/// Ce qui est en `target` reste dans les 5 × 5 secteurs autour de `viewer` : il ne lui est pas retiré.
pub fn keeps(viewer: Vec3, target: Vec3) -> bool {
    within(viewer, target, 2)
}

fn within(viewer: Vec3, target: Vec3, n: i64) -> bool {
    let (a, b) = (of(viewer), of(target));
    (a.0 - b.0).abs() <= n && (a.1 - b.1).abs() <= n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secteurs() {
        assert_eq!(of(Vec3::new(-2_560_000.0, 0.0, -2_560_000.0)), (0, 0));
        assert_eq!(of(Vec3::new(0.0, 0.0, 0.0)), (400, 400));
        assert_eq!(of(Vec3::new(-1.0, 0.0, 6400.0)), (399, 401));
        // Anchorville : un PNJ à 13 km n'est pas vu, un autre à deux secteurs non plus, le voisin oui.
        let me = Vec3::new(-481_901.8, 0.0, -45_889.3);
        assert_eq!(of(me), (324, 392));
        assert!(sees(me, Vec3::new(-476_000.0, 0.0, -39_000.0)));
        assert!(!sees(me, Vec3::new(-481_901.8, 0.0, -58_800.0)));
        assert!(keeps(me, Vec3::new(-481_901.8, 0.0, -58_800.0)), "deux secteurs plus loin : gardé");
        assert!(!keeps(me, Vec3::new(-481_901.8, 0.0, -65_200.0)), "trois secteurs : retiré");
    }
}
