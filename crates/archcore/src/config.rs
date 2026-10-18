//! Configuration ArchCore (`config/archcore.json`, même fichier que le serveur Node).
//! `ARCHCORE_CONFIG=<fichier>` permet d'utiliser une autre configuration (serveur de test…).

use serde::Deserialize;
use std::collections::HashMap;
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
    /// Villages de départ par race (clés "1".."4") ; les autres clés (commentaires) sont ignorées.
    #[serde(deserialize_with = "start_positions")]
    pub start_positions: HashMap<String, StartPosition>,
    pub view_radius: f64,
    pub language: String,
    pub rates: HashMap<String, f64>,
    pub gm_command_levels: HashMap<String, i64>,
    pub exit_on_restart: bool,
}

fn start_positions<'de, D: serde::Deserializer<'de>>(d: D) -> Result<HashMap<String, StartPosition>, D::Error> {
    let raw: HashMap<String, serde_json::Value> = HashMap::deserialize(d)?;
    Ok(raw
        .into_iter()
        .filter_map(|(k, v)| serde_json::from_value::<StartPosition>(v).ok().map(|p| (k, p)))
        .collect())
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
    #[serde(skip)]
    pub file: PathBuf,
    #[serde(skip)]
    pub root: PathBuf,
}

impl Default for Listen {
    fn default() -> Self {
        Listen { port: 11000, bind: "0.0.0.0".into() }
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
            language: "en".into(),
            rates: HashMap::new(),
            gm_command_levels: HashMap::new(),
            exit_on_restart: false,
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

/// Racine du projet : `ARCHCORE_ROOT`, sinon le dossier courant.
pub fn root() -> PathBuf {
    std::env::var_os("ARCHCORE_ROOT").map(PathBuf::from).unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}

pub fn load() -> anyhow::Result<Config> {
    let root = root();
    let file = std::env::var_os("ARCHCORE_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("config").join("archcore.json"));
    load_from(&file, &root)
}

pub fn load_from(file: &Path, root: &Path) -> anyhow::Result<Config> {
    let mut cfg: Config = match std::fs::read_to_string(file) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("configuration {} : {e}", file.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
        Err(e) => return Err(e.into()),
    };
    cfg.file = file.to_path_buf();
    cfg.root = root.to_path_buf();
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    #[test]
    fn lit_la_configuration_du_projet() {
        let text = r#"{ "login": { "port": 11002 }, "world": { "startPositions": { "1": { "village": "Anchorville", "x": 1, "y": 2, "z": 3 }, "_comment": "x" } } }"#;
        let cfg: super::Config = serde_json::from_str(text).unwrap();
        assert_eq!(cfg.login.port, 11002);
        assert_eq!(cfg.login.bind, "0.0.0.0");
        assert_eq!(cfg.world.port, 11008);
        assert_eq!(cfg.world.start_positions["1"].x, 1.0);
        assert_eq!(cfg.world.start_positions.len(), 1);
    }
}
