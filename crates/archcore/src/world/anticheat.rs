//! Anti-triche côté serveur : le client n'est pas cru sur parole, sans punir un joueur honnête à cause du lag.
//! Aucun contrôle côté client.
//!
//! - Déplacement (`presence.rs`). Le client d'origine n'envoie que sa destination : le serveur fait marcher le
//!   joueur lui-même, à sa vitesse, et un speedhack local n'y change rien. Seule une position fournie par le client
//!   (champ 1 du paquet 0x40 : simulateur, client modifié) est contrôlée, contre un crédit de distance : chaque
//!   seconde en donne la vitesse de course courante (facteurs : monture `Run_Buff`, renforcements, `/speed` du MJ)
//!   × `AntiCheat.SpeedTolerance`, plafonné à `CREDIT_SECS` secondes de course plus la marge fixe
//!   `AntiCheat.MoveSlack` ; la marche suivie par le serveur et chaque saut accepté en consomment. Des paquets
//!   retardés puis arrivés d'un coup passent donc ; une vitesse soutenue au-delà de la tolérance, non. Le saut se
//!   mesure au plus court depuis la dernière position acceptée ou depuis la position que suit le serveur.
//!   Refusé : le joueur reste à la position suivie par le serveur, renvoyée au client comme une téléportation
//!   (0x40 arrêt synchronisé, AGPMCHARACTER op 1) ; anomalie « vitesse », ou « téléportation » si le saut dépasse
//!   même le crédit plein. Les téléportations du serveur (`super::teleport` : portails, parchemins, `/move`,
//!   résurrection, sièges, Shrines, Battle Square), l'entrée en jeu et la reconnexion remettent le crédit à la
//!   marge. Les MJ ne sont pas contrôlés.
//! - Coups normaux (`combat.rs`, `pvp.rs`, `guild/battle.rs`). Le serveur porte les coups au rythme de la vitesse
//!   d'attaque ; seul un changement de cible ou une reprise d'attaque (après un déplacement) en donnait un tout de
//!   suite. Désormais le premier coup d'une nouvelle attaque attend l'intervalle de la vitesse d'attaque divisé par
//!   `AntiCheat.AttackTolerance` depuis le coup précédent : le coup en avance est retardé (le surplus ignoré), et
//!   `EARLY_ATTACKS` demandes en avance avant le coup suivant font une anomalie « cadence ».
//! - Paquets (`net.rs`) : seau de jetons par connexion (`AntiCheat.MaxPacketsPerSecond`, rafale
//!   `AntiCheat.PacketBurst`) ; au-delà, les paquets sont ignorés, et une inondation qui dure ferme la connexion
//!   (anomalie « inondation »), même avec `AntiCheat.Enabled = 0`.
//! - Anomalies : avertissement dans le journal du serveur, ligne dans `log_anomaly` (base des journaux,
//!   `ROWS_PER_MINUTE` au plus par personnage et par minute), et au-delà de `AntiCheat.Threshold` anomalies en
//!   `AntiCheat.WindowMinutes` minutes, la sanction `AntiCheat.Action` : `log` (rien de plus), `kick` (déconnexion
//!   sans attente de reconnexion) ou `ban` (bannissement du compte pour `AntiCheat.BanHours` heures, puis
//!   déconnexion). La commande MJ `/anomalies "nom"` relit les dernières.

use super::{WorldCtx, WorldState};
use crate::config::{AntiCheatAction, AntiCheatConfig};
use crate::{info, warn};
use archcore_net::Vec3;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// Secondes de course que le crédit de distance peut accumuler (lag toléré d'un coup).
pub const CREDIT_SECS: f64 = 5.0;
/// Demandes d'attaque en avance (changement de cible, reprise) avant le coup suivant qui font une anomalie.
pub const EARLY_ATTACKS: u32 = 3;
/// Lignes de `log_anomaly` au plus par personnage et par minute ; les suivantes ne sont que comptées.
pub const ROWS_PER_MINUTE: usize = 20;

/// Types d'anomalie (colonne `kind` de `log_anomaly`).
pub mod kind {
    pub const SPEED: &str = "vitesse";
    pub const TELEPORT: &str = "téléportation";
    pub const CADENCE: &str = "cadence";
    pub const FLOOD: &str = "inondation";
}

/// Vitesse de déplacement permise par les facteurs, en unités par seconde (marche ou course : la plus rapide ;
/// les facteurs la donnent en dixièmes d'unité).
pub fn units_per_sec(speed: i32) -> f64 {
    speed.max(1) as f64 / 10.0
}

/// Vitesse de déplacement des facteurs (marche ou course : la plus rapide), en dixièmes d'unité par seconde.
pub fn move_speed(f: &crate::character::Factors) -> i32 {
    f.status[8].max(f.status[9])
}

/// Contrôle des positions envoyées par le client : crédit de distance (voir l'en-tête).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoveGuard {
    /// Dernière position acceptée (ou posée par le serveur).
    pub pos: Vec3,
    /// Dernier calcul du crédit.
    pub at: Instant,
    /// Distance encore permise, en unités.
    pub credit: f64,
    /// Vitesse des facteurs au dernier calcul (dixièmes d'unité par seconde).
    pub speed: i32,
    /// Tolérance (1,5 pour 150 %) et marge fixe, tirées de la configuration.
    pub tolerance: f64,
    pub slack: f64,
}

/// Saut refusé : distance demandée, distance permise à cet instant, et avec le crédit plein.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Refused {
    pub distance: f64,
    pub allowed: f64,
    pub full: f64,
}

impl MoveGuard {
    pub fn new(cfg: &AntiCheatConfig, pos: Vec3, speed: i32, now: Instant) -> Self {
        MoveGuard { pos, at: now, credit: cfg.move_slack, speed, tolerance: cfg.speed_tolerance / 100.0, slack: cfg.move_slack }
    }

    /// Le serveur pose le joueur en `pos` (entrée, téléportation, position renvoyée) : crédit remis à la marge.
    pub fn reset(&mut self, pos: Vec3, speed: i32, now: Instant) {
        *self = MoveGuard { pos, at: now, credit: self.slack, speed, ..*self };
    }

    /// Crédit plein à la vitesse `speed`.
    pub fn full(&self, speed: i32) -> f64 {
        self.slack + units_per_sec(speed) * self.tolerance * CREDIT_SECS
    }

    /// Crédit gagné depuis le dernier calcul, à la plus grande des deux vitesses (une descente de monture ne retire
    /// pas ce que la course à cheval a permis).
    pub fn accrue(&mut self, speed: i32, now: Instant) {
        let fastest = self.speed.max(speed);
        let dt = now.saturating_duration_since(self.at).as_secs_f64();
        self.credit = (self.credit + units_per_sec(fastest) * self.tolerance * dt).min(self.full(fastest));
        self.at = now;
        self.speed = speed;
    }

    /// Marche suivie par le serveur jusqu'à `now` : elle consomme le crédit.
    pub fn walked(&mut self, distance: f64, speed: i32, now: Instant) {
        self.accrue(speed, now);
        self.credit = (self.credit - distance).max(0.0);
    }

    /// Position `given` envoyée par le client, le serveur suivant le joueur en `tracked`. Acceptée : rend la distance
    /// comptée et retient la position ; refusée : la position retenue devient `tracked`.
    pub fn check(&mut self, given: Vec3, tracked: Vec3, speed: i32, now: Instant) -> Result<f64, Refused> {
        self.accrue(speed, now);
        let d = dist2d(self.pos, given).min(dist2d(tracked, given));
        if d <= self.credit {
            self.credit -= d;
            self.pos = given;
            return Ok(d);
        }
        let refused = Refused { distance: d, allowed: self.credit, full: self.full(speed) };
        self.pos = tracked;
        Err(refused)
    }
}

/// Distance au sol (la hauteur ne compte pas : relief, chute).
pub fn dist2d(a: Vec3, b: Vec3) -> f64 {
    let (dx, dz) = ((a.x - b.x) as f64, (a.z - b.z) as f64);
    (dx * dx + dz * dz).sqrt()
}

/// Type d'un saut refusé : au-delà du crédit plein, c'est une téléportation.
pub fn move_kind(r: &Refused) -> &'static str {
    if r.distance > r.full { kind::TELEPORT } else { kind::SPEED }
}

/// Cadence des coups normaux d'un joueur.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HitGuard {
    /// Dernier coup porté et intervalle de la vitesse d'attaque d'alors.
    pub last: Option<(Instant, Duration)>,
    /// Demandes d'attaque en avance depuis ce coup.
    pub early: u32,
}

impl HitGuard {
    /// Coup porté à `now`, à la vitesse d'attaque `speed` (coups par minute ; facteur d'attaque 2).
    pub fn hit(&mut self, now: Instant, speed: i32) {
        self.last = Some((now, Duration::from_millis(60_000 / speed.max(1) as u64)));
        self.early = 0;
    }

    /// Nouvelle attaque demandée à `now` : instant du premier coup permis (`tolerance` 1,5 pour 150 %), et vrai si la
    /// demande arrive en avance.
    pub fn start(&mut self, now: Instant, tolerance: f64) -> (Instant, bool) {
        match self.last.map(|(at, interval)| at + interval.div_f64(tolerance.max(1.0))) {
            Some(earliest) if now < earliest => {
                self.early += 1;
                (earliest, true)
            }
            _ => (now, false),
        }
    }
}

/// Anomalies d'un personnage (par CID : elles survivent à une reconnexion) : instants dans la fenêtre de
/// `AntiCheat.WindowMinutes`, lignes écrites dans la dernière minute.
#[derive(Debug, Default)]
pub struct Record {
    pub recent: VecDeque<Instant>,
    pub rows: VecDeque<Instant>,
    /// Anomalies non écrites (au-delà de `ROWS_PER_MINUTE`), signalées avec la ligne suivante.
    pub unwritten: u32,
}

impl Record {
    /// Note une anomalie à `now` : rend le nombre d'anomalies dans la fenêtre et vrai si la ligne doit être écrite.
    pub fn note(&mut self, now: Instant, window: Duration) -> (usize, bool) {
        self.recent.push_back(now);
        while self.recent.front().is_some_and(|t| now.saturating_duration_since(*t) > window) {
            self.recent.pop_front();
        }
        while self.rows.front().is_some_and(|t| now.saturating_duration_since(*t) > Duration::from_secs(60)) {
            self.rows.pop_front();
        }
        let write = self.rows.len() < ROWS_PER_MINUTE;
        if write {
            self.rows.push_back(now);
        }
        (self.recent.len(), write)
    }
}

/// Anomalies de tous les personnages depuis le démarrage du serveur.
pub type Ledger = HashMap<u32, Record>;

/// Sanction due pour `count` anomalies dans la fenêtre.
pub fn sanction(cfg: &AntiCheatConfig, count: usize) -> AntiCheatAction {
    if count >= cfg.threshold { cfg.action } else { AntiCheatAction::Log }
}

/// Note une anomalie de `sid` : journal du serveur, `log_anomaly`, puis la sanction si le seuil est atteint.
pub fn report(ctx: &WorldCtx, st: &mut WorldState, sid: u32, what: &'static str, details: &str) {
    let cfg = &ctx.config.world.anticheat;
    let Some(p) = st.players.get(&sid) else { return };
    let (cid, account, name) = (p.ch.id, p.account.id, p.ch.name.clone());
    let window = Duration::from_secs(cfg.window_minutes * 60);
    let rec = st.anomalies.entry(cid).or_default();
    let (count, write) = rec.note(Instant::now(), window);
    let action = sanction(cfg, count);
    let label = match action {
        AntiCheatAction::Log => "",
        a => a.name(),
    };
    if write {
        let skipped = std::mem::take(&mut rec.unwritten);
        let details = if skipped > 0 { format!("{details} (+{skipped} non écrite(s))") } else { details.to_string() };
        warn!("[anti-triche] {name} : {what} — {details} ({count} anomalie(s) en {} min)", cfg.window_minutes);
        crate::journal::anomaly(&ctx.db, account, (cid, &name), what, &details, label);
    } else {
        rec.unwritten += 1;
        crate::debug!("[anti-triche] {name} : {what} — {details} (non écrite)");
    }
    match action {
        AntiCheatAction::Log => {}
        AntiCheatAction::Kick => kick(st, sid, &format!("{count} anomalies en {} min", cfg.window_minutes)),
        AntiCheatAction::Ban => {
            let hours = cfg.ban_hours;
            let reason = format!("anti-triche : {count} anomalies en {} min (dernière : {what})", cfg.window_minutes);
            if !st.players.get(&sid).is_some_and(|p| p.kicked) {
                let pool = ctx.db.auth.clone();
                let r = reason.clone();
                tokio::spawn(async move {
                    let sql = "INSERT INTO account_banned (account_id, banned_until, banned_by, reason) VALUES (?, IF(? > 0, NOW() + INTERVAL ? HOUR, NULL), 'anti-triche', ?)";
                    if let Err(e) = pool.exec(sql, (account, hours, hours, r)).await {
                        warn!("[anti-triche] bannissement du compte {account} non écrit : {e:#}");
                    }
                });
                info!("[anti-triche] {name} : compte {account} banni {}", if hours > 0 { format!("{hours} h") } else { "sans fin".into() });
            }
            kick(st, sid, &reason);
        }
    }
}

/// Dernières anomalies notées en base pour un personnage (`/anomalies "nom"`), avec le compte de la fenêtre en cours.
pub async fn describe(ctx: &WorldCtx, st: &WorldState, cid: u32, name: &str) -> anyhow::Result<String> {
    use crate::db::col;
    let cfg = &ctx.config.world.anticheat;
    let window = Duration::from_secs(cfg.window_minutes * 60);
    let now = Instant::now();
    let recent = st.anomalies.get(&cid).map(|r| r.recent.iter().filter(|t| now.saturating_duration_since(**t) <= window).count()).unwrap_or(0);
    let total: i64 = match ctx.db.logs.first("SELECT COUNT(*) AS n FROM log_anomaly WHERE character_id = ?", (cid,)).await? {
        Some(r) => col(&r, "n"),
        None => 0,
    };
    let rows = ctx
        .db
        .logs
        .rows("SELECT DATE_FORMAT(at, '%Y-%m-%d %H:%i:%s') AS at, kind, details, action FROM log_anomaly WHERE character_id = ? ORDER BY id DESC LIMIT 10", (cid,))
        .await?;
    let mut out = format!(
        "{name} : {total} anomalie(s) en base, {recent} depuis {} min (seuil {}, action {}).",
        cfg.window_minutes,
        cfg.threshold,
        cfg.action.name()
    );
    for r in rows {
        let (at, what, details, action): (String, String, String, String) = (col(&r, "at"), col(&r, "kind"), col(&r, "details"), col(&r, "action"));
        out.push_str(&format!("\n{at} {what}{} : {details}", if action.is_empty() { String::new() } else { format!(" [{action}]") }));
    }
    Ok(out)
}

/// Déconnecte `sid` sans attendre de reconnexion (sa clé est oubliée : il quitte le monde à la fermeture).
pub fn kick(st: &mut WorldState, sid: u32, why: &str) {
    let Some(p) = st.players.get_mut(&sid) else { return };
    if p.kicked {
        return;
    }
    p.kicked = true;
    p.link.key = 0;
    warn!("[anti-triche] {} déconnecté : {why}", p.ch.name);
    p.sess.close();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> AntiCheatConfig {
        AntiCheatConfig::default()
    }

    const RUN: i32 = 5000; // 500 unités par seconde

    #[test]
    fn deplacements_honnetes() {
        let t0 = Instant::now();
        let at = |s: f64| t0 + Duration::from_secs_f64(s);
        let mut g = MoveGuard::new(&cfg(), Vec3::new(0.0, 0.0, 0.0), RUN, t0);
        // Course à la vitesse permise, un paquet toutes les 200 ms, hauteur quelconque.
        for i in 1..=50 {
            let p = Vec3::new(100.0 * i as f32, (i * 37 % 200) as f32, 0.0);
            assert!(g.check(p, p, RUN, at(0.2 * i as f64)).is_ok(), "pas {i}");
        }
        // Lag : 3 s sans rien, puis trois paquets d'un coup (1500 unités parcourues).
        let base = g.pos;
        for (k, s) in [(1.0f32, 13.0), (2.0, 13.01), (3.0, 13.02)] {
            let p = Vec3::new(base.x + 500.0 * k, 0.0, 0.0);
            assert!(g.check(p, base, RUN, at(s)).is_ok(), "rafale après le lag ({k})");
        }
    }

    #[test]
    fn marche_du_serveur_puis_position() {
        // Le serveur fait marcher le joueur 30 s (le client n'envoie que sa destination), puis une position arrive.
        let t0 = Instant::now();
        let mut g = MoveGuard::new(&cfg(), Vec3::new(0.0, 0.0, 0.0), RUN, t0);
        for i in 1..=300 {
            g.walked(50.0, RUN, t0 + Duration::from_millis(100 * i));
        }
        let tracked = Vec3::new(15_000.0, 0.0, 0.0);
        let near = Vec3::new(15_120.0, 0.0, 0.0);
        assert!(g.check(near, tracked, RUN, t0 + Duration::from_millis(30_050)).is_ok(), "près de la position suivie");
        // Bloqué par un mur : le client est resté près de sa dernière position acceptée.
        let mut g = MoveGuard::new(&cfg(), Vec3::new(0.0, 0.0, 0.0), RUN, t0);
        for i in 1..=100 {
            g.walked(50.0, RUN, t0 + Duration::from_millis(100 * i));
        }
        assert!(g.check(Vec3::new(40.0, 0.0, 0.0), Vec3::new(5000.0, 0.0, 0.0), RUN, t0 + Duration::from_secs(10)).is_ok(), "client bloqué");
    }

    #[test]
    fn vitesse_et_teleportation_refusees() {
        let t0 = Instant::now();
        let mut g = MoveGuard::new(&cfg(), Vec3::new(0.0, 0.0, 0.0), RUN, t0);
        // Trois fois la vitesse permise : le crédit (marge comprise) s'épuise vite.
        let mut refused = None;
        for i in 1..=40 {
            let p = Vec3::new(300.0 * i as f32, 0.0, 0.0);
            if let Err(r) = g.check(p, g.pos, RUN, t0 + Duration::from_millis(200 * i)) {
                refused = Some((i, r));
                break;
            }
        }
        let (i, r) = refused.expect("speedhack refusé");
        assert!(i <= 5, "refusé au paquet {i}");
        assert_eq!(move_kind(&r), kind::SPEED);
        // Saut au-delà du crédit plein (300 + 750 × 5 = 4050) : téléportation, la position reste celle du serveur.
        let mut g = MoveGuard::new(&cfg(), Vec3::new(0.0, 0.0, 0.0), RUN, t0);
        let r = g.check(Vec3::new(30_000.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0), RUN, t0 + Duration::from_secs(60)).unwrap_err();
        assert_eq!((move_kind(&r), r.full, g.pos), (kind::TELEPORT, 4050.0, Vec3::new(10.0, 0.0, 0.0)));
        // Rester immobile ne fait pas accumuler plus que le crédit plein.
        assert!(g.check(Vec3::new(4000.0, 0.0, 0.0), g.pos, RUN, t0 + Duration::from_secs(600)).is_ok());
        assert!(g.check(Vec3::new(8000.0, 0.0, 0.0), g.pos, RUN, t0 + Duration::from_millis(600_100)).is_err());
    }

    #[test]
    fn monture_et_descente() {
        // À cheval (course 10500) : 1050 unités par seconde passent ; après la descente, le dernier saut aussi.
        let t0 = Instant::now();
        let mut g = MoveGuard::new(&cfg(), Vec3::new(0.0, 0.0, 0.0), 10_500, t0);
        for i in 1..=10 {
            let p = Vec3::new(210.0 * i as f32, 0.0, 0.0);
            assert!(g.check(p, p, 10_500, t0 + Duration::from_millis(200 * i)).is_ok(), "à cheval {i}");
        }
        assert!(g.check(Vec3::new(2310.0, 0.0, 0.0), g.pos, RUN, t0 + Duration::from_millis(2200)).is_ok(), "descendu");
        g.reset(Vec3::new(-50_000.0, 0.0, 0.0), RUN, t0 + Duration::from_secs(3));
        assert!(g.check(Vec3::new(-50_100.0, 0.0, 0.0), g.pos, RUN, t0 + Duration::from_secs(3)).is_ok(), "après une téléportation");
    }

    #[test]
    fn cadence_des_coups() {
        let t0 = Instant::now();
        let mut h = HitGuard::default();
        assert_eq!(h.start(t0, 1.5), (t0, false), "premier coup tout de suite");
        h.hit(t0, 60); // un coup par seconde
        let (at, early) = h.start(t0 + Duration::from_millis(100), 1.5);
        assert!(early && at.duration_since(t0) >= Duration::from_millis(666) && at.duration_since(t0) <= Duration::from_millis(667));
        h.start(t0 + Duration::from_millis(200), 1.5);
        h.start(t0 + Duration::from_millis(300), 1.5);
        assert_eq!(h.early, EARLY_ATTACKS);
        assert_eq!(h.start(t0 + Duration::from_millis(700), 1.5), (t0 + Duration::from_millis(700), false), "après l'intervalle toléré");
        h.hit(t0 + Duration::from_millis(700), 60);
        assert_eq!(h.early, 0);
    }

    #[test]
    fn seuil_et_lignes() {
        let t0 = Instant::now();
        let mut r = Record::default();
        let w = Duration::from_secs(600);
        for i in 0..ROWS_PER_MINUTE {
            assert_eq!(r.note(t0 + Duration::from_millis(i as u64), w), (i + 1, true));
        }
        assert_eq!(r.note(t0 + Duration::from_millis(100), w), (ROWS_PER_MINUTE + 1, false), "au-delà de 20 lignes par minute");
        assert_eq!(r.note(t0 + Duration::from_secs(61), w), (ROWS_PER_MINUTE + 2, true), "minute suivante");
        assert_eq!(r.note(t0 + Duration::from_secs(640), w).0, 2, "fenêtre glissante");
        let mut c = cfg();
        assert_eq!(sanction(&c, 50), AntiCheatAction::Log, "journal seul par défaut");
        c.action = AntiCheatAction::Kick;
        assert_eq!((sanction(&c, 9), sanction(&c, 10)), (AntiCheatAction::Log, AntiCheatAction::Kick));
    }
}
