//! `realmlist.wtf` : adresse du serveur de connexion, à la manière de World of Warcraft.
//!
//! ```text
//! # Serveur de connexion ArchCore
//! set realmlist 127.0.0.1
//! set realmlist "mon.serveur.fr:11002"
//! ```
//!
//! Une ligne `set realmlist <hôte>[:<port>]` (guillemets facultatifs, casse indifférente) ; port
//! 11002 par défaut ; lignes vides et commentaires `#`, `;` ou `//` ignorés ; la dernière ligne
//! `realmlist` l'emporte. Le fichier est cherché à côté de l'exécutable, puis dans le dossier
//! courant, puis dans le dossier `Game` du client. Il préremplit l'écran de connexion et sert de
//! serveur à `--connect` quand celui-ci ne précise pas d'hôte.

use std::path::{Path, PathBuf};

/// Nom du fichier.
pub const FILE_NAME: &str = "realmlist.wtf";
/// Port du serveur de connexion ArchLord quand le fichier n'en donne pas.
pub const DEFAULT_PORT: u16 = 11002;

/// Serveur lu dans un `realmlist.wtf`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Realmlist {
    pub host: String,
    pub port: u16,
    /// Fichier d'où vient l'adresse.
    pub source: PathBuf,
}

/// Hôte et port d'un texte `realmlist.wtf`, ou `None` s'il n'a pas de ligne `set realmlist`.
pub fn parse(text: &str) -> Option<(String, u16)> {
    let mut found = None;
    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty()
            || line.starts_with('#')
            || line.starts_with(';')
            || line.starts_with("//")
        {
            continue;
        }
        let mut words = line.splitn(3, char::is_whitespace);
        let (Some(set), Some(key), Some(value)) = (words.next(), words.next(), words.next()) else {
            continue;
        };
        if !set.eq_ignore_ascii_case("set") || !key.eq_ignore_ascii_case("realmlist") {
            continue;
        }
        let value = value.trim().trim_matches('"').trim();
        if value.is_empty() {
            continue;
        }
        // `hôte:port` ; une adresse IPv6 entre crochets garde ses `:` (`[::1]:11002`).
        let (host, port) = match value.rsplit_once(':') {
            Some((h, p)) if !h.is_empty() && (!h.contains(':') || h.ends_with(']')) => {
                match p.parse::<u16>() {
                    Ok(port) => (h, port),
                    Err(_) => continue,
                }
            }
            _ => (value, DEFAULT_PORT),
        };
        found = Some((host.trim_matches(['[', ']']).to_string(), port));
    }
    found
}

/// Dossiers où chercher `realmlist.wtf`, dans l'ordre : exécutable, dossier courant, client.
pub fn search_dirs(game_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        dirs.push(dir);
    }
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd);
    }
    if let Some(g) = game_dir {
        dirs.push(g.to_path_buf());
    }
    dirs
}

/// Premier `realmlist.wtf` lisible et valide parmi `dirs`.
pub fn find(dirs: &[PathBuf]) -> Option<Realmlist> {
    for dir in dirs {
        let path = dir.join(FILE_NAME);
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        match parse(&String::from_utf8_lossy(&bytes)) {
            Some((host, port)) => {
                return Some(Realmlist {
                    host,
                    port,
                    source: path,
                })
            }
            None => log::warn!("{} : aucune ligne « set realmlist »", path.display()),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_wow_style_lines() {
        assert_eq!(
            parse("set realmlist 127.0.0.1\n"),
            Some(("127.0.0.1".into(), 11002))
        );
        assert_eq!(
            parse("# commentaire\nSET realmlist \"logon.exemple.fr:12000\"\r\n"),
            Some(("logon.exemple.fr".into(), 12000))
        );
        assert_eq!(
            parse("set realmlist a.fr\nset realmlist b.fr:1\n"),
            Some(("b.fr".into(), 1))
        );
        assert_eq!(
            parse("set realmlist [::1]:11002"),
            Some(("::1".into(), 11002))
        );
        assert_eq!(parse("set realmlist ::1"), Some(("::1".into(), 11002)));
        assert_eq!(parse("set realmlist hote:abc"), None);
        assert_eq!(parse("set patchlist x\n// rien"), None);
    }
}
