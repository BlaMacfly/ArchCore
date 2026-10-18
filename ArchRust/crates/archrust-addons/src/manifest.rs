//! Fichier `.toc` d'un AddOn.
//!
//! ```text
//! ## Title: Hello ArchLord
//! ## Version: 1.0
//! ## Author: BlaMacfly
//! ## Interface: 10000
//! ## SavedVariables: HelloArchDB
//! ## Dependencies: LibCore
//! ## OptionalDeps: LibDataBroker
//! ## Notes: Exemple d'AddOn
//! ## LoadOnDemand: 0
//!
//! core.lua
//! ui/frames.lua
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manifest {
    /// Nom du dossier (identifiant de l'AddOn).
    pub name: String,
    pub title: String,
    pub version: String,
    pub author: String,
    pub notes: String,
    pub interface: u32,
    pub saved_variables: Vec<String>,
    pub saved_variables_per_character: Vec<String>,
    pub dependencies: Vec<String>,
    pub optional_deps: Vec<String>,
    pub load_on_demand: bool,
    /// Autres directives `## Clé: valeur` (dont `X-…`).
    pub extra: BTreeMap<String, String>,
    /// Fichiers à charger, relatifs au dossier de l'AddOn, dans l'ordre.
    pub files: Vec<PathBuf>,
    pub dir: PathBuf,
}

fn split_list(v: &str) -> Vec<String> {
    v.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

impl Manifest {
    /// Analyse le texte d'un `.toc`. `name` est le nom du dossier, `dir` son chemin.
    pub fn parse(name: &str, dir: &Path, text: &str) -> Manifest {
        let mut m = Manifest {
            name: name.to_string(),
            title: name.to_string(),
            dir: dir.to_path_buf(),
            ..Default::default()
        };
        for raw in text.lines() {
            let line = raw.trim_start_matches('\u{feff}').trim();
            if line.is_empty() || line.starts_with('#') && !line.starts_with("##") {
                continue;
            }
            if let Some(rest) = line.strip_prefix("##") {
                let Some((k, v)) = rest.split_once(':') else {
                    continue;
                };
                let (k, v) = (k.trim(), v.trim());
                match k.to_ascii_lowercase().as_str() {
                    "title" => m.title = v.to_string(),
                    "version" => m.version = v.to_string(),
                    "author" => m.author = v.to_string(),
                    "notes" => m.notes = v.to_string(),
                    "interface" => m.interface = v.parse().unwrap_or(0),
                    "savedvariables" => m.saved_variables = split_list(v),
                    "savedvariablespercharacter" => m.saved_variables_per_character = split_list(v),
                    "dependencies" | "requireddeps" | "dependancies" => {
                        m.dependencies = split_list(v)
                    }
                    "optionaldeps" | "optionaldependencies" => m.optional_deps = split_list(v),
                    "loadondemand" => m.load_on_demand = v == "1" || v.eq_ignore_ascii_case("true"),
                    _ => {
                        m.extra.insert(k.to_string(), v.to_string());
                    }
                }
                continue;
            }
            // Ligne de fichier (les commentaires de fin de ligne ne sont pas admis, comme sous WoW).
            m.files.push(PathBuf::from(line.replace('\\', "/")));
        }
        m
    }

    /// Lit `<dir>/<nom>.toc`.
    pub fn load(dir: &Path) -> std::io::Result<Manifest> {
        let name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("AddOn")
            .to_string();
        let toc = dir.join(format!("{name}.toc"));
        let text = std::fs::read_to_string(&toc)?;
        Ok(Self::parse(&name, dir, &text))
    }

    /// Fichiers Lua (les `.xml` sont signalés mais ignorés dans cette version).
    pub fn lua_files(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.files
            .iter()
            .filter(|f| {
                f.extension()
                    .map(|e| e.eq_ignore_ascii_case("lua"))
                    .unwrap_or(false)
            })
            .map(|f| self.dir.join(f))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_toc() {
        let t = "## Title: Hello\n## Interface: 10000\n## SavedVariables: HelloDB, HelloOpts\n## Dependencies: LibA\n## X-Website: https://x\n\ncore.lua\nui\\frames.lua\nlayout.xml\n";
        let m = Manifest::parse("Hello", Path::new("addons/Hello"), t);
        assert_eq!(m.title, "Hello");
        assert_eq!(m.interface, 10000);
        assert_eq!(m.saved_variables, vec!["HelloDB", "HelloOpts"]);
        assert_eq!(m.dependencies, vec!["LibA"]);
        assert_eq!(
            m.extra.get("X-Website").map(String::as_str),
            Some("https://x")
        );
        assert_eq!(m.files.len(), 3);
        assert_eq!(m.lua_files().count(), 2);
    }
}
