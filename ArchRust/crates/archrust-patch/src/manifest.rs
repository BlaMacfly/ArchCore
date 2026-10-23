//! Manifeste `patch.toml`.

use serde::{Deserialize, Serialize};

/// Catégorie d'un pack : graphismes (textures, modèles, interface, shaders), sons, ou les deux.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Graphics,
    Sounds,
    Mixed,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Category::Graphics => "graphismes",
            Category::Sounds => "sons",
            Category::Mixed => "graphismes et sons",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PatchInfo {
    pub id: String,
    pub name: String,
    /// Catégorie déclarée ; absente, elle est déduite du contenu ([`Manifest::category`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<Category>,
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

/// Son remplacé : chemin du client sous `sound/` (`ui/click.wav`, `effect/sd0000.wav`, `bgm/x.mp3`).
/// Un son rangé dans un pack `.pk` se remplace par son nom d'entrée, comme un fichier isolé.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SoundOverride {
    pub path: String,
    pub file: String,
}

impl SoundOverride {
    /// Chemin complet du client (`sound/…`).
    pub fn client_path(&self) -> String {
        let p = self.path.replace('\\', "/");
        let p = p.trim_start_matches("./").trim_start_matches('/');
        if p.to_ascii_lowercase().starts_with("sound/") {
            p.to_string()
        } else {
            format!("sound/{p}")
        }
    }
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
    #[serde(default)]
    pub sounds: Vec<SoundOverride>,
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
            .chain(self.sounds.iter().map(|s| s.file.as_str()))
    }

    /// Catégorie : celle déclarée, sinon déduite — sons seulement (`[[sounds]]` ou `[[files]]` sous
    /// `sound/`), graphismes seulement, ou les deux.
    pub fn category(&self) -> Category {
        if let Some(c) = self.patch.category {
            return c;
        }
        let is_sound = |p: &str| p.replace('\\', "/").to_ascii_lowercase().starts_with("sound/");
        let sound = !self.sounds.is_empty() || self.files.iter().any(|f| is_sound(&f.path));
        let graphics = !self.textures.is_empty() || !self.models.is_empty() || !self.shaders.is_empty() || self.files.iter().any(|f| !is_sound(&f.path));
        match (graphics, sound) {
            (_, false) => Category::Graphics,
            (false, true) => Category::Sounds,
            (true, true) => Category::Mixed,
        }
    }

    /// Le client `version` (« 0.1.0 ») satisfait-il `min_archrust` ? Une version absente ou illisible
    /// est acceptée.
    pub fn supports(&self, version: &str) -> bool {
        version_at_least(version, &self.patch.min_archrust)
    }
}

/// `have` ≥ `need`, en comparant les nombres séparés par des points (« 0.1 » = « 0.1.0 »).
pub fn version_at_least(have: &str, need: &str) -> bool {
    let parse = |v: &str| -> Option<Vec<u64>> { v.trim().split('.').map(|n| n.trim().parse().ok()).collect() };
    let (Some(mut h), Some(mut n)) = (parse(have), parse(need)) else { return true };
    let len = h.len().max(n.len());
    h.resize(len, 0);
    n.resize(len, 0);
    h >= n
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
        assert_eq!(m.category(), Category::Graphics, "ini/npc.ini n'est pas un son");
    }

    #[test]
    fn categories_and_sounds() {
        let sounds = Manifest::parse("[patch]\nid = \"s\"\nname = \"S\"\n\n[[sounds]]\npath = \"ui/click.wav\"\nfile = \"click.wav\"\n").unwrap();
        assert_eq!(sounds.category(), Category::Sounds);
        assert_eq!(sounds.sounds[0].client_path(), "sound/ui/click.wav");
        let raw = Manifest::parse("[patch]\nid = \"r\"\nname = \"R\"\n\n[[files]]\npath = \"sound\\\\bgm\\\\a.mp3\"\nfile = \"a.mp3\"\n").unwrap();
        assert_eq!(raw.category(), Category::Sounds);
        let both = Manifest::parse("[patch]\nid = \"b\"\nname = \"B\"\n\n[[textures]]\nmatch = \"a\"\nfile = \"a.png\"\n\n[[sounds]]\npath = \"sound/ui/x.wav\"\nfile = \"x.wav\"\n").unwrap();
        assert_eq!((both.category(), both.sounds[0].client_path().as_str()), (Category::Mixed, "sound/ui/x.wav"));
        let declared = Manifest::parse("[patch]\nid = \"d\"\nname = \"D\"\ncategory = \"sounds\"\n").unwrap();
        assert_eq!(declared.category(), Category::Sounds);
        assert!(Manifest::parse("[patch]\nid = \"x\"\nname = \"X\"\ncategory = \"video\"\n").is_err());
    }

    #[test]
    fn versions() {
        assert!(version_at_least("0.1.0", "0.1"));
        assert!(version_at_least("0.2.0", "0.1.9"));
        assert!(!version_at_least("0.1.0", "0.2"));
        assert!(version_at_least("0.1.0", ""), "absente : acceptée");
        assert!(version_at_least("0.1.0", "abc"), "illisible : acceptée");
        assert!(!version_at_least("1.0", "1.0.1"));
    }

    #[test]
    fn globs() {
        assert!(glob_match("a0057*", "A0057AAB"));
        assert!(glob_match("character/*.ecl", "character\\a0000001.ecl"));
        assert!(!glob_match("a0057*", "b0057aab"));
        assert!(glob_match("*", ""));
    }
}
