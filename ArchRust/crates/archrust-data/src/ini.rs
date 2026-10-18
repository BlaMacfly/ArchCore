//! Tables `ini` du client.
//!
//! Chaque table (`charactertemplate.ini`, `npc.ini`, `itemtemplateall.ini`…) et le fichier de
//! serveurs sont du texte en CP949 chiffré RC4 avec la clé CryptoAPI « 1111 ». Une variante en
//! clair `…Test.ini` est lue en priorité par le client pour le fichier de serveurs.
//!
//! Le texte suit la forme INI classique : des sections `[id]` et des lignes `clé=valeur`.
//! La même clé peut apparaître plusieurs fois dans une section ; l'ordre est conservé.

use archrust_crypto::{capi_crypt, KEY_INI};
use std::collections::HashMap;

/// Page de code du texte des tables. Les tables d'origine (`ini/`) sont en CP949 ; les dossiers de
/// langue occidentaux (`ini_fr`, `ini_de`, `ini_es`) sont en Windows-1252 (accents vérifiés en jeu).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CodePage {
    #[default]
    Cp949,
    Windows1252,
}

impl CodePage {
    fn encoding(self) -> &'static encoding_rs::Encoding {
        match self {
            CodePage::Cp949 => encoding_rs::EUC_KR,
            CodePage::Windows1252 => encoding_rs::WINDOWS_1252,
        }
    }
    /// Page de code d'un dossier de tables d'après son nom (`ini`, `ini_fr`, `ini_ko`…).
    pub fn for_dir(dir_name: &str) -> CodePage {
        match dir_name.to_ascii_lowercase().as_str() {
            "ini_fr" | "ini_de" | "ini_es" | "ini_en" | "ini_it" | "ini_pt" => {
                CodePage::Windows1252
            }
            _ => CodePage::Cp949,
        }
    }
}

/// Déchiffre une table `ini` et la convertit de CP949 en UTF-8.
pub fn decrypt_ini(raw: &[u8]) -> String {
    decrypt_ini_with(raw, CodePage::Cp949)
}

/// Déchiffre une table `ini` et la convertit depuis la page de code donnée.
pub fn decrypt_ini_with(raw: &[u8], cp: CodePage) -> String {
    let plain = capi_crypt(KEY_INI, raw);
    decode_text(&plain, cp)
}

/// Convertit un texte de la page de code donnée en UTF-8 (octets invalides → U+FFFD).
pub fn decode_text(bytes: &[u8], cp: CodePage) -> String {
    let (text, _, _) = cp.encoding().decode(bytes);
    text.into_owned()
}

/// Encode en page de code (pour réécrire une table ou un paquet).
pub fn encode_text(text: &str, cp: CodePage) -> Vec<u8> {
    let (bytes, _, _) = cp.encoding().encode(text);
    bytes.into_owned()
}

/// Convertit du CP949 en UTF-8.
pub fn decode_cp949(bytes: &[u8]) -> String {
    decode_text(bytes, CodePage::Cp949)
}

/// Encode en CP949.
pub fn encode_cp949(text: &str) -> Vec<u8> {
    encode_text(text, CodePage::Cp949)
}

/// Section d'une table : liste ordonnée de `(clé, valeur)`.
#[derive(Debug, Default, Clone)]
pub struct IniSection {
    pub name: String,
    pub entries: Vec<(String, String)>,
}

impl IniSection {
    /// Première valeur de `key`.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }
    /// Toutes les valeurs de `key`, dans l'ordre.
    pub fn get_all<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.entries
            .iter()
            .filter(move |(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }
    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.get(key)?.trim().parse().ok()
    }
    pub fn get_f32(&self, key: &str) -> Option<f32> {
        self.get(key)?.trim().parse().ok()
    }
}

/// Table `ini` analysée.
#[derive(Debug, Default, Clone)]
pub struct IniTable {
    pub sections: Vec<IniSection>,
    index: HashMap<String, usize>,
}

impl IniTable {
    /// Analyse un texte INI déjà déchiffré.
    pub fn parse(text: &str) -> Self {
        let mut table = IniTable::default();
        let mut current: Option<IniSection> = None;
        for raw_line in text.lines() {
            let line = raw_line.trim_end_matches('\r').trim();
            if line.is_empty()
                || line.starts_with(';')
                || line.starts_with('#')
                || line.starts_with("//")
            {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                if let Some(sec) = current.take() {
                    table.push(sec);
                }
                current = Some(IniSection {
                    name: name.trim().to_string(),
                    entries: Vec::new(),
                });
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let sec = current.get_or_insert_with(IniSection::default);
            sec.entries
                .push((k.trim().to_string(), v.trim().to_string()));
        }
        if let Some(sec) = current.take() {
            table.push(sec);
        }
        table
    }

    /// Déchiffre puis analyse une table chiffrée du client.
    pub fn from_encrypted(raw: &[u8]) -> Self {
        Self::parse(&decrypt_ini(raw))
    }

    fn push(&mut self, sec: IniSection) {
        self.index
            .entry(sec.name.to_ascii_lowercase())
            .or_insert(self.sections.len());
        self.sections.push(sec);
    }

    /// Section par nom (insensible à la casse) ; en cas de doublon, la première.
    pub fn section(&self, name: &str) -> Option<&IniSection> {
        self.index
            .get(&name.to_ascii_lowercase())
            .map(|&i| &self.sections[i])
    }

    pub fn len(&self) -> usize {
        self.sections.len()
    }
    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }
}

/// Entrée du fichier de serveurs (`Archonia.ini` / `ArchoniaTest.ini`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerEntry {
    pub name: String,
    pub address: String,
    pub port: u16,
}

/// Analyse un fichier de serveurs en clair. Les lignes `Nom=adresse:port` ou `[Nom] IP=… Port=…`
/// sont acceptées ; on reste tolérant car la forme exacte varie selon le lanceur.
pub fn parse_server_list(text: &str) -> Vec<ServerEntry> {
    let table = IniTable::parse(text);
    let mut out = Vec::new();
    for sec in &table.sections {
        if let (Some(ip), Some(port)) = (
            sec.get("IP").or_else(|| sec.get("Address")),
            sec.get_i64("Port"),
        ) {
            out.push(ServerEntry {
                name: sec.name.clone(),
                address: ip.to_string(),
                port: port as u16,
            });
            continue;
        }
        for (k, v) in &sec.entries {
            if let Some((addr, port)) = v.rsplit_once(':') {
                if let Ok(port) = port.trim().parse::<u16>() {
                    out.push(ServerEntry {
                        name: k.clone(),
                        address: addr.trim().to_string(),
                        port,
                    });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sections_and_duplicates() {
        let t = IniTable::parse("[1]\nName=Humain\nSkill=10\nSkill=11\n\n[2]\nName=Orc\n");
        assert_eq!(t.len(), 2);
        let s = t.section("1").unwrap();
        assert_eq!(s.get("name"), Some("Humain"));
        assert_eq!(s.get_all("Skill").collect::<Vec<_>>(), vec!["10", "11"]);
        assert_eq!(t.section("2").unwrap().get_i64("Skill"), None);
    }

    #[test]
    fn server_list_forms() {
        let v = parse_server_list("Local=127.0.0.1:11002\n");
        assert_eq!(
            v,
            vec![ServerEntry {
                name: "Local".into(),
                address: "127.0.0.1".into(),
                port: 11002
            }]
        );
        let v = parse_server_list("[Prod]\nIP=5.135.137.163\nPort=11002\n");
        assert_eq!(v[0].port, 11002);
    }

    #[test]
    fn codepage_roundtrips() {
        let ko = "ArchLord 한글";
        assert_eq!(decode_cp949(&encode_cp949(ko)), ko);
        let fr = "Créer un personnage";
        assert_eq!(
            decode_text(
                &encode_text(fr, CodePage::Windows1252),
                CodePage::Windows1252
            ),
            fr
        );
        assert_eq!(CodePage::for_dir("ini_fr"), CodePage::Windows1252);
        assert_eq!(CodePage::for_dir("ini"), CodePage::Cp949);
    }
}
