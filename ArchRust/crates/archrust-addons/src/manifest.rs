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

    /// Fichiers Lua listés directement dans le `.toc`.
    pub fn lua_files(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.files
            .iter()
            .filter(|f| has_ext(f, "lua"))
            .map(|f| self.dir.join(f))
    }

    /// Tous les fichiers Lua à exécuter, dans l'ordre : ceux du `.toc`, et pour chaque `.xml`
    /// (ex. `embeds.xml`, `libs\AceAddon-3.0\AceAddon-3.0.xml`) les `<Script file="…"/>` et
    /// `<Include file="…"/>` qu'il contient, récursivement, relatifs au dossier du `.xml`. Les
    /// définitions de frames en XML ne sont pas interprétées (créer les frames en Lua).
    pub fn script_files(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for f in &self.files {
            let p = self.dir.join(f);
            if has_ext(f, "lua") {
                out.push(p);
            } else if has_ext(f, "xml") {
                expand_xml(&p, &self.dir, &mut out, &mut seen, 0);
            }
        }
        out
    }
}

fn has_ext(p: &Path, ext: &str) -> bool {
    p.extension()
        .map(|e| e.eq_ignore_ascii_case(ext))
        .unwrap_or(false)
}

/// Valeurs de l'attribut `file` des balises `<Script>` et `<Include>` d'un XML d'interface.
pub fn xml_file_refs(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    // Retire les commentaires.
    let mut clean = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("<!--") {
        clean.push_str(&rest[..i]);
        rest = match rest[i..].find("-->") {
            Some(j) => &rest[i + j + 3..],
            None => "",
        };
    }
    clean.push_str(rest);
    let lower = clean.to_ascii_lowercase();
    let mut pos = 0;
    while let Some(i) = lower[pos..].find('<') {
        let start = pos + i + 1;
        let end = lower[start..].find('>').map(|e| start + e).unwrap_or(lower.len());
        let tag = &clean[start..end];
        let tag_lower = &lower[start..end];
        if tag_lower.starts_with("script") || tag_lower.starts_with("include") {
            if let Some(a) = tag_lower.find("file") {
                let after = &tag[a + 4..];
                let after = after.trim_start().strip_prefix('=').unwrap_or(after).trim_start();
                if let Some(q) = after.chars().next().filter(|c| *c == '"' || *c == '\'') {
                    if let Some(close) = after[1..].find(q) {
                        out.push(after[1..1 + close].to_string());
                    }
                }
            }
        }
        pos = end.max(start);
    }
    out
}

fn expand_xml(
    xml: &Path,
    root: &Path,
    out: &mut Vec<PathBuf>,
    seen: &mut std::collections::HashSet<PathBuf>,
    depth: usize,
) {
    if depth > 16 || !seen.insert(xml.to_path_buf()) {
        return;
    }
    let Ok(text) = std::fs::read_to_string(xml) else {
        log::warn!("addons : {} illisible", xml.display());
        return;
    };
    let base = xml.parent().unwrap_or(root);
    for r in xml_file_refs(&text) {
        let Some(rel) = crate::manager::safe_relative(&r) else {
            log::warn!("addons : chemin refusé dans {} : {r}", xml.display());
            continue;
        };
        let p = base.join(rel);
        if has_ext(&p, "lua") {
            out.push(p);
        } else if has_ext(&p, "xml") {
            expand_xml(&p, root, out, seen, depth + 1);
        }
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
