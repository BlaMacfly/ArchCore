//! Ciblage au clavier : Tab (cible suivante) et Maj+Tab (retour en arrière).
//!
//! Touches relevées dans Archonia : `UI_1024X768.ini`, `Hotkey=0:15` (Tab, DIK 15, sans
//! modificateur) « Target Nearest » → `Targeting_FuncAutoTargetingF`, et `Hotkey=1:15` (Maj+Tab) →
//! `Targeting_FuncAutoTargetingB`. Dans Archonia.exe (sha256 e9c7609b…), les deux fonctions
//! enregistrées par FUN_1404e2a30 sont vides : F (0x1404e1c80) rend seulement
//! `AgcmUIManager2 != NULL`, B (FUN_1400ef910) rend 0. Le client de référence ne cible donc rien.
//!
//! Écart : ArchRust donne un vrai comportement à ces touches. Il reprend la logique de benilla
//! (`crates/benilla-app/src/target/scan.rs`, MIT OR Apache-2.0), elle-même fondée sur la priorité
//! d'écran de WoW Classic :
//! - les candidats visibles dans l'écran rétréci de 10 % sur chaque bord passent d'abord ;
//! - le plus petit score l'emporte (écart au centre + distance relative, moins un bonus pour qui
//!   nous attaque) ;
//! - chaque appui saute les cibles prises dans les 4 dernières secondes ;
//! - Maj+Tab revient à la plus récente de ces cibles encore présente.

use std::time::{Duration, Instant};

/// Portée du Tab : les 41 yards de WoW (37,5 m), soit 3 750 unités d'ArchLord (1 unité ≈ 1 cm ;
/// un personnage mesure `HEIGHT=182`).
pub const TAB_RANGE: f32 = 3750.0;
/// Fraction de l'écran retirée de chaque bord pour le tri « visible d'abord ».
const PULL_IN: f32 = 0.10;
/// Durée pendant laquelle une cible déjà prise est sautée par les appuis suivants.
const HISTORY: Duration = Duration::from_secs(4);
/// Poids de l'écart au centre (0 au centre, environ 1 dans un coin).
const W_SCREEN: f32 = 1.0;
/// Poids de `distance / TAB_RANGE`.
const W_DIST: f32 = 1.0;
/// Supérieur à la somme des autres termes : qui nous attaque passe devant tout le reste.
const FIGHTING_ME_BONUS: f32 = 3.0;

/// Candidat au Tab ; `score` plus petit = meilleur.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candidate {
    pub cid: i32,
    pub on_screen: bool,
    pub score: f32,
}

impl Candidate {
    /// `screen` : position projetée (pixels) du point situé une centaine d'unités au-dessus des
    /// pieds, `None` derrière la caméra ; `size` : taille de la fenêtre.
    pub fn new(
        cid: i32,
        screen: Option<(f32, f32)>,
        size: (f32, f32),
        dist: f32,
        fighting_me: bool,
    ) -> Candidate {
        let off_center = screen.and_then(|p| off_center(p, size));
        Candidate {
            cid,
            on_screen: off_center.is_some(),
            score: priority_score(off_center, dist, fighting_me),
        }
    }
}

/// Écart au centre dans l'écran rétréci, `None` en dehors.
fn off_center((x, y): (f32, f32), (w, h): (f32, f32)) -> Option<f32> {
    let inside = x >= w * PULL_IN && x <= w * (1.0 - PULL_IN) && y >= h * PULL_IN && y <= h * (1.0 - PULL_IN);
    let half_diag = (w * w + h * h).sqrt() * 0.5;
    (inside && half_diag > 0.0).then(|| ((x - w * 0.5).powi(2) + (y - h * 0.5).powi(2)).sqrt() / half_diag)
}

fn priority_score(off_center: Option<f32>, dist: f32, fighting_me: bool) -> f32 {
    off_center.map_or(0.0, |c| W_SCREEN * c) + W_DIST * (dist / TAB_RANGE)
        - if fighting_me { FIGHTING_ME_BONUS } else { 0.0 }
}

/// Visibles d'abord, puis score croissant.
fn order(a: &Candidate, b: &Candidate) -> std::cmp::Ordering {
    b.on_screen.cmp(&a.on_screen).then(a.score.total_cmp(&b.score))
}

/// Les candidats visibles s'il y en a, sinon tous.
fn pool(cands: &[Candidate]) -> Vec<Candidate> {
    if cands.iter().any(|c| c.on_screen) {
        cands.iter().filter(|c| c.on_screen).copied().collect()
    } else {
        cands.to_vec()
    }
}

/// Meilleur candidat ni actuel ni déjà vu ; sinon le meilleur non actuel, avec reprise du tour
/// (`true` : l'historique est vidé) ; sinon l'actuel lui-même.
fn pick_forward(pool: &[Candidate], visited: &[i32], current: Option<i32>) -> Option<(usize, bool)> {
    if pool.is_empty() {
        return None;
    }
    if let Some(i) = pool.iter().position(|c| Some(c.cid) != current && !visited.contains(&c.cid)) {
        return Some((i, false));
    }
    if let Some(i) = pool.iter().position(|c| Some(c.cid) != current) {
        return Some((i, true));
    }
    Some((0, true))
}

/// Retour : la cible vue le plus récemment, différente de l'actuelle et encore candidate.
fn pick_back(visited: &[i32], pool: &[Candidate], current: Option<i32>) -> Option<usize> {
    visited.iter().rev().find_map(|cid| {
        (Some(*cid) != current)
            .then(|| pool.iter().position(|c| c.cid == *cid))
            .flatten()
    })
}

/// Cibles prises récemment, sautées par les appuis suivants.
#[derive(Debug, Default)]
pub struct TabHistory {
    /// `(cid, moment)`, du plus ancien au plus récent.
    visited: Vec<(i32, Instant)>,
}

impl TabHistory {
    fn push(&mut self, cid: i32, now: Instant) {
        self.visited.retain(|(c, _)| *c != cid);
        self.visited.push((cid, now));
    }

    /// Choisit la cible d'un appui sur Tab (`reverse` : Maj+Tab). Rend `None` sans candidat
    /// (la cible reste inchangée).
    pub fn cycle(
        &mut self,
        mut cands: Vec<Candidate>,
        current: Option<i32>,
        reverse: bool,
        now: Instant,
    ) -> Option<i32> {
        self.visited.retain(|(_, t)| now.duration_since(*t) < HISTORY);
        if cands.is_empty() {
            return None;
        }
        cands.sort_by(order);
        let pool = pool(&cands);
        let visited: Vec<i32> = self.visited.iter().map(|(c, _)| *c).collect();
        let back = reverse.then(|| pick_back(&visited, &pool, current)).flatten();
        let (cid, wrapped) = match back {
            Some(i) => (pool[i].cid, false),
            None => {
                let (i, wrapped) = pick_forward(&pool, &visited, current)?;
                (pool[i].cid, wrapped)
            }
        };
        if wrapped {
            self.visited.clear();
        }
        // La cible quittée rejoint l'historique, ce qui permet à Maj+Tab d'y revenir.
        if let Some(old) = current {
            self.push(old, now);
        }
        if Some(cid) != current {
            self.push(cid, now);
        }
        Some(cid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (f32, f32) = (1920.0, 1080.0);

    fn at(cid: i32, x: f32, dist: f32) -> Candidate {
        Candidate::new(cid, Some((x, 540.0)), SIZE, dist, false)
    }

    #[test]
    fn score_weighs_center_distance_and_attackers() {
        let center = at(1, 960.0, 1000.0);
        let side = at(2, 1600.0, 1000.0);
        assert!(center.on_screen && side.on_screen);
        assert!(center.score < side.score);
        let attacker = Candidate::new(3, Some((1600.0, 540.0)), SIZE, 3000.0, true);
        assert!(attacker.score < center.score);
        // Au bord (moins de 10 % de la largeur) : hors de l'écran rétréci.
        assert!(!at(4, 100.0, 500.0).on_screen);
        assert!(!Candidate::new(5, None, SIZE, 500.0, false).on_screen);
    }

    #[test]
    fn tab_cycles_skips_recent_and_wraps() {
        let mut h = TabHistory::default();
        let now = Instant::now();
        let cands = || vec![at(1, 960.0, 500.0), at(2, 1200.0, 800.0), at(3, 700.0, 2000.0), at(9, 50.0, 100.0)];
        // Le hors-écran (9) n'entre pas tant qu'il y a des visibles.
        let a = h.cycle(cands(), None, false, now).unwrap();
        let b = h.cycle(cands(), Some(a), false, now).unwrap();
        let c = h.cycle(cands(), Some(b), false, now).unwrap();
        assert_eq!((a, b, c), (1, 2, 3));
        // Tous vus : le tour reprend au meilleur.
        assert_eq!(h.cycle(cands(), Some(c), false, now), Some(1));
        // Maj+Tab revient à la cible précédente encore présente.
        assert_eq!(h.cycle(cands(), Some(1), true, now), Some(3));
        // Après 4 s, l'historique est oublié.
        let later = now + Duration::from_secs(5);
        assert_eq!(h.cycle(cands(), Some(3), false, later), Some(1));
    }

    #[test]
    fn offscreen_only_falls_back_to_distance() {
        let mut h = TabHistory::default();
        let cands = vec![
            Candidate::new(7, None, SIZE, 3000.0, false),
            Candidate::new(8, None, SIZE, 900.0, false),
        ];
        assert_eq!(h.cycle(cands, None, false, Instant::now()), Some(8));
        assert_eq!(h.cycle(Vec::new(), Some(8), false, Instant::now()), None);
    }
}
