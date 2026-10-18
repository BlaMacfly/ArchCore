//! Manifeste `patch.toml`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PatchInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub min_archrust: String,
    #[serde(default)]
    pub homepage: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TextureOverride {
    /// Nom RenderWare de la texture (sans extension) ou motif avec `*`.
    #[serde(rename = "match")]
    pub pattern: String,
    pub file: String,
    #[serde(default = "one")]
    pub scale: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelOverride {
    /// Chemin logique du modèle (`character/a0000001.ecl`, `world/R272,272.dff`…) ou motif.
    #[serde(rename = "match")]
    pub pattern: String,
    pub file: String,
    /// Facteur d'échelle appliqué au glTF (défaut 1 ; 100 pour un modèle en mètres).
    #[serde(default = "one")]
    pub scale: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileOverride {
    /// Chemin du client relatif au dossier `Game` (`ini/npc.ini`, `sound/effect/aa.pk`…).
    pub path: String,
    pub file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShaderOverride {
    pub name: String,
    pub file: String,
}

fn one() -> f32 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub patch: PatchInfo,
    #[serde(default)]
    pub textures: Vec<TextureOverride>,
    #[serde(default)]
    pub models: Vec<ModelOverride>,
    #[serde(default)]
    pub files: Vec<FileOverride>,
    #[serde(default)]
    pub shaders: Vec<ShaderOverride>,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("manifeste sérialisable")
    }

    /// Tous les fichiers référencés par le manifeste.
    pub fn referenced_files(&self) -> impl Iterator<Item = &str> {
        self.textures
            .iter()
            .map(|t| t.file.as_str())
            .chain(self.models.iter().map(|m| m.file.as_str()))
            .chain(self.files.iter().map(|f| f.file.as_str()))
            .chain(self.shaders.iter().map(|s| s.file.as_str()))
    }
}

/// Correspondance insensible à la casse avec `*` (zéro ou plusieurs caractères).
pub fn glob_match(pattern: &str, name: &str) -> bool {
    fn rec(p: &[u8], n: &[u8]) -> bool {
        match (p.first(), n.first()) {
            (None, None) => true,
            (Some(b'*'), _) => rec(&p[1..], n) || (!n.is_empty() && rec(p, &n[1..])),
            (Some(a), Some(b)) if a.eq_ignore_ascii_case(b) => rec(&p[1..], &n[1..]),
            _ => false,
        }
    }
    let p = pattern.replace('\\', "/");
    let n = name.replace('\\', "/");
    rec(p.as_bytes(), n.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_manifest() {
        let m = Manifest::parse(
            r#"
[patch]
id = "test"
name = "Test"
priority = 10

[[textures]]
match = "a0057*"
file = "tex/a.png"

[[models]]
match = "character/a0000060.ecl"
file = "models/cube.glb"
scale = 100.0

[[models]]
match = "character/*.ecl"
file = "models/defaut.glb"

[[files]]
path = "ini/npc.ini"
file = "npc.ini"
"#,
        )
        .unwrap();
        assert_eq!(m.patch.priority, 10);
        assert_eq!(m.textures[0].scale, 1.0);
        assert_eq!(m.models[0].scale, 100.0);
        assert_eq!(m.models[1].scale, 1.0);
        assert_eq!(m.referenced_files().count(), 4);
        let back = Manifest::parse(&m.to_toml()).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn globs() {
        assert!(glob_match("a0057*", "A0057AAB"));
        assert!(glob_match("character/*.ecl", "character\\a0000001.ecl"));
        assert!(!glob_match("a0057*", "b0057aab"));
        assert!(glob_match("*", ""));
    }
}
