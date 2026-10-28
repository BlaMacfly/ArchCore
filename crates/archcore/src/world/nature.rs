//! Horloge du monde et météo, du serveur au client (AgpmTimer 0x11, AgpmEventNature 0x30 ; AgpmNatureEffect 0x47
//! n'est pas utilisé : le client tire lui-même l'effet de pluie ou de neige de la météo).
//!
//! ```text
//!   Entrée en jeu        ← AGPMTIMER {0 vitesse, 1 temps du jeu en ms}, avant le paquet personnage (world/mod.rs)
//!                        ← AgpmEventNature {0 météo} après les autres états (temps clair compris)
//!   Toutes les 10 min    ← AGPMTIMER à tous les joueurs en jeu : le client recale son horloge (renvoi inoffensif)
//!   /weather pluie…      ← AgpmEventNature {0 : 0 clair, 3 pluie, 5 neige} à tous les joueurs en jeu
//! ```
//! Le client calcule l'heure du ciel par `(temps / 3 600 000) % 24`, et avance son temps de `vitesse` ms par ms réelle
//! depuis la réception. ArchCore tient une horloge **globale** (choix d'ArchCore, la politique du serveur d'origine
//! n'est pas connue) : avec `World.TimeStart` = h (0 à 23), le temps vaut h heures au démarrage du serveur puis avance
//! de `World.TimeSpeed` ; avec -1 (défaut), il vaut l'horloge de la machine (`World.TimeZone`) multipliée par la
//! vitesse, donc la même heure d'un démarrage à l'autre. Deux joueurs entrés à des moments différents voient la même
//! heure.
//!
//! Météo (choix d'ArchCore) : la même pour tout le monde, fixée par la commande MJ `/weather` ou, si
//! `World.WeatherAuto` vaut 1, tirée toutes les `World.WeatherMinutes` minutes (pluie, neige ou temps clair selon
//! `World.WeatherRainChance` et `World.WeatherSnowChance`). Elle n'est pas enregistrée : temps clair au démarrage.
//! Compétences de l'ArchLord sur le ciel (`archlord.rs`) : Rain Control et Snow Control imposent la pluie ou la neige
//! à tous pendant leur durée, puis la météo d'avant revient ; Day and Night décale de 12 h l'horloge envoyée à tous
//! (`shift_ms`) pendant `Archlord.SkyMinutes`.

use super::{World, WorldCtx, WorldState};
use crate::info;
use archcore_net::schema::{self, weather};
use archcore_net::{build_packet, Fields};
use rand::Rng;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 24 * HOUR_MS;
/// Intervalle entre deux renvois de l'horloge.
pub const TIMER_RESEND: Duration = Duration::from_secs(600);
/// Intervalle de la boucle de l'horloge et de la météo.
const TICK: Duration = Duration::from_secs(30);

/// Horloge et météo du monde.
#[derive(Clone, Debug)]
pub struct Nature {
    /// Météo en cours (`weather`).
    pub weather: i64,
    /// Horloge de la machine (ms, fuseau `World.TimeZone`) au démarrage du serveur.
    pub started_ms: i64,
    /// Dernier envoi de l'horloge à tous.
    pub timer_sent: Instant,
    /// Prochain tirage de la météo automatique.
    pub next_weather: Option<Instant>,
    /// Décalage de l'horloge (ms) : Day and Night de l'ArchLord (`archlord.rs`), 0 sinon.
    pub shift_ms: i64,
}

impl Default for Nature {
    fn default() -> Self {
        Nature { weather: weather::CLEAR, started_ms: crate::config::TimeZone::Local.game_time_ms(), timer_sent: Instant::now(), next_weather: None, shift_ms: 0 }
    }
}

impl Nature {
    /// Horloge de départ prise dans le fuseau configuré.
    pub fn new(ctx_zone: crate::config::TimeZone) -> Self {
        Nature { started_ms: ctx_zone.game_time_ms(), ..Default::default() }
    }
}

/// Temps du jeu (ms) quand l'horloge de la machine vaut `now_ms` : heure de départ `start_hour` (0 à 23) au démarrage
/// (`started_ms`) puis `speed` fois l'écoulé ; `start_hour` négatif : `now_ms` × `speed`. Jamais 0 (le client
/// ignorerait la valeur).
pub fn game_time_ms(speed: u8, start_hour: i64, started_ms: i64, now_ms: i64) -> i64 {
    let speed = i64::from(speed.max(1));
    if start_hour < 0 {
        return now_ms.max(1).saturating_mul(speed);
    }
    DAY_MS + start_hour.min(23) * HOUR_MS + (now_ms - started_ms).max(0).saturating_mul(speed)
}

/// Temps du jeu maintenant (décalage de Day and Night compris).
pub fn now_ms(ctx: &WorldCtx, st: &WorldState) -> i64 {
    let w = &ctx.config.world;
    game_time_ms(w.time_speed, w.time_start, st.nature.started_ms, w.time_zone.game_time_ms()) + st.nature.shift_ms
}

/// AGPMTIMER à tous les joueurs en jeu (horloge changée).
pub fn broadcast_timer(ctx: &WorldCtx, st: &mut WorldState) {
    st.nature.timer_sent = Instant::now();
    let pkt = build_packet(&schema::TIMER, &timer_fields(ctx, st));
    for p in st.players.values().filter(|p| p.in_game) {
        p.send_raw(pkt.clone());
    }
}

/// Champs d'AGPMTIMER : vitesse et temps du jeu ; heure du serveur (2 : secondes Unix, UTC) et décalage de son fuseau
/// (3 : secondes, `World.TimeZone`), lus par la fenêtre des boss d'Archonia (« Server Time », comptes à rebours).
pub fn timer_fields(ctx: &WorldCtx, st: &WorldState) -> Fields {
    let s = &schema::TIMER;
    let offset = match ctx.config.world.time_zone {
        crate::config::TimeZone::Utc => 0,
        crate::config::TimeZone::Local => chrono::Local::now().offset().local_minus_utc(),
    };
    Fields::new(s)
        .with(0, ctx.config.world.time_speed.max(1))
        .with(1, now_ms(ctx, st))
        .with(2, crate::character::unix_now() as u32)
        .with(3, offset as u32)
}

/// Heure et minute du jeu d'un temps en ms.
pub fn hour_minute(ms: i64) -> (i64, i64) {
    ((ms / HOUR_MS) % 24, (ms / 60_000) % 60)
}

/// Paquet AgpmEventNature d'une météo.
pub fn weather_packet(code: i64) -> Vec<u8> {
    let s = &schema::EVENT_NATURE;
    build_packet(s, &Fields::new(s).with(0, code))
}

/// Météo désignée par un mot de la commande MJ (anglais ou français).
pub fn parse_weather(word: &str) -> Option<i64> {
    match word.to_lowercase().as_str() {
        "clear" | "clair" | "sun" | "soleil" => Some(weather::CLEAR),
        "rain" | "pluie" => Some(weather::RAIN),
        "snow" | "neige" => Some(weather::SNOW),
        _ => None,
    }
}

pub fn weather_label(code: i64) -> &'static str {
    match code {
        weather::RAIN => "pluie",
        weather::SNOW => "neige",
        _ => "temps clair",
    }
}

/// Météo tirée : `roll` dans [0, 100), pluie puis neige selon leurs probabilités (en pour cent), sinon temps clair.
pub fn roll_weather(roll: f64, rain: f64, snow: f64) -> i64 {
    if roll < rain {
        weather::RAIN
    } else if roll < rain + snow {
        weather::SNOW
    } else {
        weather::CLEAR
    }
}

/// Change la météo et l'envoie à tous les joueurs en jeu.
pub fn set_weather(st: &mut WorldState, code: i64, why: &str) {
    st.nature.weather = code;
    let pkt = weather_packet(code);
    let mut n = 0;
    for p in st.players.values().filter(|p| p.in_game) {
        p.send_raw(pkt.clone());
        n += 1;
    }
    info!("[nature] météo : {} ({why}, {n} joueur(s))", weather_label(code));
}

/// Entrée en jeu : la météo en cours.
pub fn on_enter(st: &WorldState, sid: u32) {
    if let Some(p) = st.players.get(&sid) {
        p.send_raw(weather_packet(st.nature.weather));
    }
}

/// Réponse de `/weather` sans argument : météo et heure du jeu.
pub fn describe(ctx: &WorldCtx, st: &WorldState) -> String {
    let (h, m) = hour_minute(now_ms(ctx, st));
    format!("Météo : {}. Heure du jeu : {h:02}:{m:02} (vitesse ×{}).", weather_label(st.nature.weather), ctx.config.world.time_speed.max(1))
}

/// Tour de la boucle : renvoi de l'horloge, tirage de la météo automatique.
fn step(ctx: &WorldCtx, st: &mut WorldState, now: Instant) {
    if now.duration_since(st.nature.timer_sent) >= TIMER_RESEND {
        st.nature.timer_sent = now;
        let pkt = build_packet(&schema::TIMER, &timer_fields(ctx, st));
        for p in st.players.values().filter(|p| p.in_game) {
            p.send_raw(pkt.clone());
        }
    }
    let w = &ctx.config.world;
    if !w.weather_auto {
        return;
    }
    let every = Duration::from_secs(w.weather_minutes.max(1) * 60);
    match st.nature.next_weather {
        Some(at) if now >= at => {
            st.nature.next_weather = Some(now + every);
            let code = roll_weather(rand::thread_rng().gen_range(0.0..100.0), w.weather_rain_chance, w.weather_snow_chance);
            if code != st.nature.weather {
                set_weather(st, code, "météo automatique");
            }
        }
        Some(_) => {}
        None => st.nature.next_weather = Some(now + every),
    }
}

/// Boucle de l'horloge et de la météo.
pub fn spawn_ticker(world: Arc<World>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(TICK);
        tick.tick().await;
        loop {
            tick.tick().await;
            let mut st = world.state.lock().await;
            step(&world.ctx, &mut st, Instant::now());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use archcore_net::{decode_packet, Fields};

    #[test]
    fn horloge_globale() {
        // Départ à 12 h : 10 minutes réelles plus tard, ×6, il est 13 h.
        let t0 = 1_800_000_000_000;
        assert_eq!(hour_minute(game_time_ms(6, 12, t0, t0)), (12, 0));
        assert_eq!(hour_minute(game_time_ms(6, 12, t0, t0 + 600_000)), (13, 0));
        assert_eq!(hour_minute(game_time_ms(6, 0, t0, t0)), (0, 0));
        assert!(game_time_ms(6, 0, t0, t0) > 0, "jamais 0");
        // Dérivée de l'horloge de la machine : deux connexions décalées de 7 s voient 42 s de jeu d'écart.
        let (a, b) = (game_time_ms(6, -1, t0, t0 + 1_000), game_time_ms(6, -1, t0, t0 + 8_000));
        assert_eq!(b - a, 42_000);
        assert_eq!(game_time_ms(1, -1, 0, t0), t0, "vitesse 1 : l'heure de la machine");
    }

    #[test]
    fn meteo() {
        assert_eq!(parse_weather("Rain"), Some(weather::RAIN));
        assert_eq!(parse_weather("neige"), Some(weather::SNOW));
        assert_eq!(parse_weather("clear"), Some(weather::CLEAR));
        assert_eq!(parse_weather("grêle"), None);
        assert_eq!(roll_weather(5.0, 20.0, 10.0), weather::RAIN);
        assert_eq!(roll_weather(25.0, 20.0, 10.0), weather::SNOW);
        assert_eq!(roll_weather(30.0, 20.0, 10.0), weather::CLEAR);
        let p = decode_packet(&schema::EVENT_NATURE, &weather_packet(weather::RAIN)).unwrap();
        assert_eq!((p.int(0), p.int(1), p.int(2)), (Some(3), None, None));
    }

    #[test]
    fn effet_de_nature() {
        // Drapeaux sur 2 octets, un seul champ.
        let s = &schema::NATURE_EFFECT;
        let raw = build_packet(s, &Fields::new(s).with(0, -1));
        assert_eq!(raw.len(), 19 + 2 + 4 + 1);
        assert_eq!(decode_packet(s, &raw).unwrap().int(0), Some(-1));
    }
}
