//! Configuration ArchCore : `config/auth.conf` (serveur de connexion) et `config/world.conf` (serveur monde).
//!
//! Chaque fichier est une suite de lignes `Clé = valeur` commentées (`#`), avec un en-tête de section
//! (`[authserver]`, `[worldserver]`) ignoré. Les modèles `auth.conf.dist` et `world.conf.dist`, versionnés,
//! documentent chaque option ; au premier lancement, un `.conf` absent est créé en copiant son modèle. Les
//! `.conf` ne sont pas versionnés.
//!
//! - `archcore login` lit `auth.conf`, `archcore world` lit `world.conf`, `archcore all` et les outils lisent
//!   les deux (les réglages communs, comme `Database.*`, sont pris dans `world.conf` en dernier).
//! - Toute clé peut être remplacée par une variable d'environnement `ARCHCORE_<CLÉ>` : points et majuscules
//!   intérieures deviennent des `_`, le tout en majuscules (`World.ViewRadius` → `ARCHCORE_WORLD_VIEW_RADIUS`).
//! - Une clé absente prend sa valeur par défaut, avec un avertissement ; une clé inconnue est signalée.
//! - `ARCHCORE_CONF_DIR` désigne un autre dossier de configuration (serveur de test…).

use crate::warn;
use serde::Deserialize;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Listen {
    pub port: u16,
    pub bind: String,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct StartPosition {
    #[serde(default)]
    pub village: Option<String>,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WorldConfig {
    pub port: u16,
    pub bind: String,
    pub name: String,
    pub group: String,
    pub public_address: String,
    /// Villages de départ par race (clés "1".."4").
    #[serde(deserialize_with = "start_positions")]
    pub start_positions: HashMap<String, StartPosition>,
    pub view_radius: f64,
    /// Multiplicateur du nombre de monstres par point d'apparition (1 : tables du serveur).
    pub monster_density: f64,
    pub language: String,
    pub rates: HashMap<String, f64>,
    pub gm_command_levels: HashMap<String, i64>,
    pub exit_on_restart: bool,
    /// Fuseau de l'heure du jeu envoyée dans AGPMTIMER : `local` (défaut) ou `utc`.
    pub time_zone: TimeZone,
}

/// Fuseau de l'horloge envoyée au client (`World.TimeZone`). Le client calcule l'heure du ciel par
/// `(ms / 3 600 000) % 24` sans correction de fuseau : `Local` lui donne l'heure de la machine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimeZone {
    #[default]
    Local,
    Utc,
}

impl TimeZone {
    /// Millisecondes à envoyer dans AGPMTIMER champ 1 : époque Unix, décalée du fuseau local pour
    /// `Local` (le même instant « lu » comme s'il était UTC).
    pub fn game_time_ms(self) -> i64 {
        match self {
            TimeZone::Utc => chrono::Utc::now().timestamp_millis(),
            TimeZone::Local => chrono::Local::now().naive_local().and_utc().timestamp_millis(),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            TimeZone::Local => "local",
            TimeZone::Utc => "utc",
        }
    }
}

fn start_positions<'de, D: serde::Deserializer<'de>>(d: D) -> Result<HashMap<String, StartPosition>, D::Error> {
    let raw: HashMap<String, serde_json::Value> = HashMap::deserialize(d)?;
    Ok(raw.into_iter().filter_map(|(k, v)| serde_json::from_value::<StartPosition>(v).ok().map(|p| (k, p))).collect())
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DatabaseConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub connection_limit: usize,
    pub auth: String,
    pub characters: String,
    pub world: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ClientConfig {
    pub data_dir: String,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub login: Listen,
    pub world: WorldConfig,
    pub database: DatabaseConfig,
    pub client: ClientConfig,
    /// Fichiers lus.
    #[serde(skip)]
    pub files: Vec<PathBuf>,
    #[serde(skip)]
    pub root: PathBuf,
}

impl Default for Listen {
    fn default() -> Self {
        Listen { port: 11002, bind: "0.0.0.0".into() }
    }
}

impl Default for WorldConfig {
    fn default() -> Self {
        WorldConfig {
            port: 11008,
            bind: "0.0.0.0".into(),
            name: "ArchCore".into(),
            group: "ArchCore".into(),
            public_address: "127.0.0.1".into(),
            start_positions: HashMap::new(),
            view_radius: 8000.0,
            monster_density: 1.0,
            language: "fr".into(),
            rates: HashMap::new(),
            gm_command_levels: HashMap::new(),
            exit_on_restart: false,
            time_zone: TimeZone::Local,
        }
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        DatabaseConfig {
            host: "127.0.0.1".into(),
            port: 3306,
            user: "root".into(),
            password: "archcore".into(),
            connection_limit: 8,
            auth: "archcore_auth".into(),
            characters: "archcore_characters".into(),
            world: "archcore_world".into(),
        }
    }
}

impl Default for ClientConfig {
    fn default() -> Self {
        ClientConfig { data_dir: "../client/ArchLord/Game".into() }
    }
}

/// Fichiers de configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    Auth,
    World,
}

impl Which {
    pub fn file_name(self) -> &'static str {
        match self {
            Which::Auth => "auth.conf",
            Which::World => "world.conf",
        }
    }
}

/// Racine du projet : `ARCHCORE_ROOT`, sinon le dossier courant.
pub fn root() -> PathBuf {
    std::env::var_os("ARCHCORE_ROOT").map(PathBuf::from).unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}

/// Dossier de configuration : `ARCHCORE_CONF_DIR`, sinon `<racine>/config`.
pub fn conf_dir(root: &Path) -> PathBuf {
    std::env::var_os("ARCHCORE_CONF_DIR").map(PathBuf::from).unwrap_or_else(|| root.join("config"))
}

/// Lignes `Clé = valeur` d'un fichier .conf (commentaires `#` et en-têtes `[…]` ignorés, guillemets ôtés).
pub fn parse_conf(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
                return None;
            }
            let (k, v) = line.split_once('=')?;
            let v = v.trim();
            let v = v.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(v);
            Some((k.trim().to_string(), v.to_string()))
        })
        .collect()
}

/// Variable d'environnement qui remplace une clé : `World.ViewRadius` → `ARCHCORE_WORLD_VIEW_RADIUS`.
pub fn env_name(key: &str) -> String {
    let mut out = String::from("ARCHCORE_");
    let mut prev: Option<char> = None;
    for c in key.chars() {
        if c == '.' {
            out.push('_');
        } else {
            if c.is_ascii_uppercase() && prev.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit()) {
                out.push('_');
            }
            out.push(c.to_ascii_uppercase());
        }
        prev = Some(c);
    }
    out
}

/// Valeurs lues, avec les variables d'environnement par-dessus, et le suivi des clés utilisées.
pub struct Settings {
    values: BTreeMap<String, String>,
    used: RefCell<BTreeSet<String>>,
    env: Box<dyn Fn(&str) -> Option<String>>,
    /// Signaler les clés absentes (désactivé pour un fichier qui ne les concerne pas).
    pub warn_missing: bool,
    pub source: String,
}

impl Settings {
    pub fn new(values: BTreeMap<String, String>, source: &str) -> Self {
        Settings { values, used: RefCell::default(), env: Box::new(|k| std::env::var(env_name(k)).ok()), warn_missing: true, source: source.into() }
    }

    #[cfg(test)]
    fn with_env(mut self, env: impl Fn(&str) -> Option<String> + 'static) -> Self {
        self.env = Box::new(env);
        self
    }

    fn raw(&self, key: &str) -> Option<String> {
        self.used.borrow_mut().insert(key.to_string());
        (self.env)(key).or_else(|| self.values.get(key).cloned())
    }

    pub fn string(&self, key: &str, default: &str) -> String {
        match self.raw(key) {
            Some(v) => v,
            None => {
                if self.warn_missing {
                    warn!("configuration : {key} absente de {}, valeur par défaut « {default} »", self.source);
                }
                default.to_string()
            }
        }
    }

    pub fn parse<T: std::str::FromStr + std::fmt::Display>(&self, key: &str, default: T) -> T {
        match self.raw(key) {
            Some(v) => v.trim().parse().unwrap_or_else(|_| {
                warn!("configuration : {key} = « {v} » illisible dans {}, valeur par défaut {default}", self.source);
                default
            }),
            None => {
                if self.warn_missing {
                    warn!("configuration : {key} absente de {}, valeur par défaut {default}", self.source);
                }
                default
            }
        }
    }

    pub fn flag(&self, key: &str, default: bool) -> bool {
        self.parse::<i64>(key, default as i64) != 0
    }

    /// Clés `<préfixe>.<suite>` présentes : (suite, valeur).
    pub fn prefixed(&self, prefix: &str) -> Vec<(String, String)> {
        let p = format!("{prefix}.");
        let keys: Vec<String> = self.values.keys().filter(|k| k.starts_with(&p)).cloned().collect();
        keys.into_iter().filter_map(|k| self.raw(&k).map(|v| (k[p.len()..].to_string(), v))).collect()
    }

    /// Clés présentes que personne n'a lues (fautes de frappe…).
    pub fn unused(&self) -> Vec<String> {
        let used = self.used.borrow();
        self.values.keys().filter(|k| !used.contains(*k)).cloned().collect()
    }
}

/// Réglages de `auth.conf` et des clés communes.
fn read_common(s: &Settings, cfg: &mut Config) {
    let d = DatabaseConfig::default();
    cfg.database = DatabaseConfig {
        host: s.string("Database.Host", &d.host),
        port: s.parse("Database.Port", d.port),
        user: s.string("Database.User", &d.user),
        password: s.string("Database.Password", &d.password),
        connection_limit: s.parse("Database.ConnectionLimit", d.connection_limit),
        auth: s.string("Database.Auth", &d.auth),
        characters: s.string("Database.Characters", &d.characters),
        world: s.string("Database.World", &d.world),
    };
}

fn read_auth(s: &Settings, cfg: &mut Config) {
    let d = Listen::default();
    cfg.login = Listen { port: s.parse("LoginServer.Port", d.port), bind: s.string("LoginServer.Bind", &d.bind) };
    read_common(s, cfg);
}

fn read_world(s: &Settings, cfg: &mut Config) {
    let d = WorldConfig::default();
    read_common(s, cfg);
    cfg.client.data_dir = s.string("Client.DataDir", &ClientConfig::default().data_dir);
    let time_zone = match s.string("World.TimeZone", "local").to_ascii_lowercase().as_str() {
        "utc" => TimeZone::Utc,
        "local" => TimeZone::Local,
        other => {
            warn!("configuration : World.TimeZone = « {other} » inconnu (local ou utc), heure locale");
            TimeZone::Local
        }
    };
    let mut start_positions: HashMap<String, StartPosition> = HashMap::new();
    for (rest, v) in s.prefixed("StartPosition") {
        let Some((race, field)) = rest.split_once('.') else { continue };
        let p = start_positions.entry(race.to_string()).or_default();
        match field {
            "Village" => p.village = Some(v),
            "X" => p.x = v.trim().parse().unwrap_or(0.0),
            "Y" => p.y = v.trim().parse().unwrap_or(0.0),
            "Z" => p.z = v.trim().parse().unwrap_or(0.0),
            _ => warn!("configuration : StartPosition.{rest} inconnue"),
        }
    }
    // Rate.Exp → « exp », Rate.UniqueDrop → « uniqueDrop » : première lettre en minuscule.
    let rates = s
        .prefixed("Rate")
        .into_iter()
        .filter_map(|(k, v)| {
            let mut c = k.chars();
            let name = c.next().map(|f| f.to_ascii_lowercase().to_string() + c.as_str())?;
            v.trim().parse::<f64>().ok().map(|x| (name, x))
        })
        .collect();
    let gm_command_levels = s.prefixed("GM.CommandLevel").into_iter().filter_map(|(k, v)| v.trim().parse::<i64>().ok().map(|x| (k, x))).collect();
    cfg.world = WorldConfig {
        port: s.parse("WorldServer.Port", d.port),
        bind: s.string("WorldServer.Bind", &d.bind),
        name: s.string("WorldServer.Name", &d.name),
        group: s.string("WorldServer.Group", &d.group),
        public_address: s.string("WorldServer.PublicAddress", &d.public_address),
        start_positions,
        view_radius: s.parse("World.ViewRadius", d.view_radius),
        monster_density: s.parse("World.MonsterDensity", d.monster_density),
        language: s.string("World.Language", &d.language),
        rates,
        gm_command_levels,
        exit_on_restart: s.flag("World.ExitOnRestart", d.exit_on_restart),
        time_zone,
    };
}

/// Chemin d'un .conf, créé depuis son modèle .dist s'il manque.
pub fn ensure_conf(dir: &Path, which: Which) -> anyhow::Result<PathBuf> {
    let path = dir.join(which.file_name());
    if !path.exists() {
        let dist = dir.join(format!("{}.dist", which.file_name()));
        anyhow::ensure!(dist.exists(), "{} introuvable, ni son modèle {}", path.display(), dist.display());
        std::fs::copy(&dist, &path)?;
        crate::info!("configuration : {} créé à partir de {}", path.display(), dist.display());
    }
    Ok(path)
}

fn read_file(path: &Path) -> anyhow::Result<Settings> {
    let text = std::fs::read_to_string(path).map_err(|e| anyhow::anyhow!("{} : {e}", path.display()))?;
    let values: BTreeMap<String, String> = parse_conf(&text).into_iter().collect();
    Ok(Settings::new(values, &path.display().to_string()))
}

/// Configuration de `archcore all` et des outils : `auth.conf` puis `world.conf`.
pub fn load() -> anyhow::Result<Config> {
    load_mode("all")
}

/// Configuration pour `login` (auth.conf), `world` (world.conf) ou `all` (les deux).
pub fn load_mode(mode: &str) -> anyhow::Result<Config> {
    let root = root();
    let dir = conf_dir(&root);
    let mut cfg = Config { root, ..Default::default() };
    let wanted: &[Which] = match mode {
        "login" => &[Which::Auth],
        "world" => &[Which::World],
        _ => &[Which::Auth, Which::World],
    };
    for which in wanted {
        let path = ensure_conf(&dir, *which)?;
        let s = read_file(&path)?;
        let before = (!cfg.files.is_empty()).then(|| format!("{:?}", cfg.database));
        match which {
            Which::Auth => read_auth(&s, &mut cfg),
            Which::World => read_world(&s, &mut cfg),
        }
        if before.is_some_and(|b| b != format!("{:?}", cfg.database)) {
            warn!("configuration : Database.* diffère entre auth.conf et world.conf, celle de {} est retenue", path.display());
        }
        for k in s.unused() {
            warn!("configuration : clé inconnue {k} dans {}", path.display());
        }
        cfg.files.push(path);
    }
    Ok(cfg)
}

/// Lit une ancienne configuration JSON (`archcore.json`) pour la migration.
pub fn load_json(file: &Path) -> anyhow::Result<Config> {
    let text = std::fs::read_to_string(file).map_err(|e| anyhow::anyhow!("{} : {e}", file.display()))?;
    serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("configuration {} : {e}", file.display()))
}

/// Remplace dans un modèle .dist les valeurs des clés connues (`Clé = valeur`) et ajoute les autres à la fin.
pub fn fill_template(template: &str, values: &[(String, String)]) -> String {
    let mut pending: Vec<&(String, String)> = values.iter().collect();
    let mut out = String::new();
    for line in template.lines() {
        let t = line.trim_start();
        let key = (!t.starts_with('#') && !t.starts_with('[')).then(|| t.split_once('=').map(|(k, _)| k.trim())).flatten();
        match key.and_then(|k| pending.iter().position(|(pk, _)| pk == k)) {
            Some(i) => {
                let (k, v) = pending.remove(i);
                out.push_str(&format!("{k} = {}\n", quote(v)));
            }
            None => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    if !pending.is_empty() {
        out.push_str("\n# Réglages ajoutés par la migration\n");
        for (k, v) in pending {
            out.push_str(&format!("{k} = {}\n", quote(v)));
        }
    }
    out
}

/// Les textes sont entre guillemets, les nombres non.
fn quote(v: &str) -> String {
    if v.parse::<f64>().is_ok() { v.to_string() } else { format!("\"{v}\"") }
}

fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 { format!("{}", v as i64) } else { v.to_string() }
}

/// Réglages d'une configuration, sous forme de clés .conf, pour `auth.conf` ou `world.conf`.
pub fn to_values(cfg: &Config, which: Which) -> Vec<(String, String)> {
    let d = &cfg.database;
    let mut v: Vec<(String, String)> = Vec::new();
    let mut put = |k: &str, x: String| v.push((k.to_string(), x));
    match which {
        Which::Auth => {
            put("LoginServer.Bind", cfg.login.bind.clone());
            put("LoginServer.Port", cfg.login.port.to_string());
        }
        Which::World => {
            let w = &cfg.world;
            put("WorldServer.Bind", w.bind.clone());
            put("WorldServer.Port", w.port.to_string());
            put("WorldServer.Name", w.name.clone());
            put("WorldServer.Group", w.group.clone());
            put("WorldServer.PublicAddress", w.public_address.clone());
            put("World.Language", w.language.clone());
            put("World.ViewRadius", fmt_num(w.view_radius));
            put("World.MonsterDensity", fmt_num(w.monster_density));
            put("World.TimeZone", w.time_zone.name().into());
            put("World.ExitOnRestart", (w.exit_on_restart as i64).to_string());
            put("Client.DataDir", cfg.client.data_dir.clone());
            let mut races: Vec<&String> = w.start_positions.keys().collect();
            races.sort();
            for r in races {
                let p = &w.start_positions[r];
                if let Some(village) = &p.village {
                    put(&format!("StartPosition.{r}.Village"), village.clone());
                }
                put(&format!("StartPosition.{r}.X"), fmt_num(p.x));
                put(&format!("StartPosition.{r}.Y"), fmt_num(p.y));
                put(&format!("StartPosition.{r}.Z"), fmt_num(p.z));
            }
            let mut rates: Vec<(&String, &f64)> = w.rates.iter().collect();
            rates.sort_by_key(|r| r.0);
            for (name, x) in rates {
                let mut c = name.chars();
                let key = c.next().map(|f| f.to_ascii_uppercase().to_string() + c.as_str()).unwrap_or_default();
                put(&format!("Rate.{key}"), fmt_num(*x));
            }
            let mut levels: Vec<(&String, &i64)> = w.gm_command_levels.iter().collect();
            levels.sort();
            for (cmd, lvl) in levels {
                put(&format!("GM.CommandLevel.{cmd}"), lvl.to_string());
            }
        }
    }
    put("Database.Host", d.host.clone());
    put("Database.Port", d.port.to_string());
    put("Database.User", d.user.clone());
    put("Database.Password", d.password.clone());
    put("Database.ConnectionLimit", d.connection_limit.to_string());
    put("Database.Auth", d.auth.clone());
    put("Database.Characters", d.characters.clone());
    put("Database.World", d.world.clone());
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(text: &str) -> Settings {
        Settings::new(parse_conf(text).into_iter().collect(), "test").with_env(|_| None)
    }

    #[test]
    fn lecture_d_un_fichier_conf() {
        let text = "[worldserver]\n# commentaire\nWorldServer.Port = 12008\nWorldServer.Name = \"Mon monde\"\n\nWorld.ViewRadius = 6000\n\
                    StartPosition.1.Village = \"Anchorville\"\nStartPosition.1.X = -475955\nStartPosition.1.Y = 4608.5\nStartPosition.1.Z = -66219\n\
                    Rate.Exp = 2\nRate.UniqueDrop = 0.5\nGM.CommandLevel.kill = 3\nWorld.TimeZone = utc\nWorld.ExitOnRestart = 1\nFaute.Frappe = 1\n";
        let s = settings(text);
        let mut cfg = Config::default();
        read_world(&s, &mut cfg);
        let w = &cfg.world;
        assert_eq!((w.port, w.name.as_str(), w.view_radius), (12008, "Mon monde", 6000.0));
        assert_eq!((w.start_positions["1"].village.as_deref(), w.start_positions["1"].y), (Some("Anchorville"), 4608.5));
        assert_eq!((w.rates["exp"], w.rates["uniqueDrop"]), (2.0, 0.5));
        assert_eq!(w.gm_command_levels["kill"], 3);
        assert_eq!((w.time_zone, w.exit_on_restart), (TimeZone::Utc, true));
        assert_eq!(w.bind, "0.0.0.0", "clé absente : défaut");
        assert_eq!(s.unused(), vec!["Faute.Frappe".to_string()]);
    }

    #[test]
    fn variables_d_environnement() {
        assert_eq!(env_name("World.ViewRadius"), "ARCHCORE_WORLD_VIEW_RADIUS");
        assert_eq!(env_name("Database.Host"), "ARCHCORE_DATABASE_HOST");
        assert_eq!(env_name("StartPosition.1.X"), "ARCHCORE_START_POSITION_1_X");
        assert_eq!(env_name("LoginServer.Port"), "ARCHCORE_LOGIN_SERVER_PORT");
        let s = Settings::new(parse_conf("LoginServer.Port = 11002\n").into_iter().collect(), "test")
            .with_env(|k| (k == "LoginServer.Port").then(|| "13002".to_string()));
        let mut cfg = Config::default();
        read_auth(&s, &mut cfg);
        assert_eq!(cfg.login.port, 13002, "la variable d'environnement l'emporte");
    }

    #[test]
    fn migration_depuis_le_json() {
        let json = r#"{ "login": { "port": 11102 }, "world": { "port": 11108, "viewRadius": 8000, "timeZone": "utc",
            "startPositions": { "1": { "village": "Anchorville", "x": 1, "y": 2.5, "z": 3 }, "_comment": "x" },
            "rates": { "exp": 2, "uniqueDrop": 0.5 }, "gmCommandLevels": { "kill": 3 } },
            "database": { "auth": "archcore_test_auth" } }"#;
        let cfg: Config = serde_json::from_str(json).unwrap();
        let template = "[worldserver]\n# Port\nWorldServer.Port = 11008\n#\nWorld.ViewRadius = 8000\n";
        let world = fill_template(template, &to_values(&cfg, Which::World));
        assert!(world.contains("WorldServer.Port = 11108\n") && world.contains("# Port\n"));
        assert!(world.contains("Rate.UniqueDrop = 0.5") && world.contains("GM.CommandLevel.kill = 3"));
        assert!(world.contains("World.TimeZone = \"utc\"") && world.contains("StartPosition.1.Y = 2.5"));
        // Relu, il redonne la même configuration.
        let mut back = Config::default();
        read_world(&settings(&world), &mut back);
        assert_eq!((back.world.port, back.world.rates["uniqueDrop"], back.world.time_zone), (11108, 0.5, TimeZone::Utc));
        assert_eq!(back.world.start_positions["1"].village.as_deref(), Some("Anchorville"));
        assert_eq!(back.database.auth, "archcore_test_auth");
        let auth = fill_template("[authserver]\nLoginServer.Port = 11002\n", &to_values(&cfg, Which::Auth));
        let mut back = Config::default();
        read_auth(&settings(&auth), &mut back);
        assert_eq!(back.login.port, 11102);
    }

    #[test]
    fn heure_du_jeu_locale_et_utc() {
        use chrono::Timelike;
        let local = TimeZone::Local.game_time_ms();
        let utc = TimeZone::Utc.game_time_ms();
        let offset_ms = i64::from(chrono::Local::now().offset().local_minus_utc()) * 1000;
        assert!((local - utc - offset_ms).abs() < 1000, "local {local} utc {utc} décalage {offset_ms}");
        // Comme AgpmEventNature : (ms / 3 600 000) % 24 doit valoir l'heure de la machine.
        let heure = (local / 3_600_000) % 24;
        let attendue = i64::from(chrono::Local::now().hour());
        assert!(heure == attendue || (heure + 1) % 24 == attendue, "heure du jeu {heure}, locale {attendue}");
    }
}
