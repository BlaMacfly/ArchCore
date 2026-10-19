//! Extensions ArchRust ↔ ArchCore : négociation de capacités (HELLO / HELLO_ACK) et paquets communs aux
//! deux serveurs (version 1.1 de la spécification).
//!
//! Le client ArchRust envoie un paquet 0xAC `HELLO` juste après la poignée de main chiffrée, sur chaque
//! connexion (login, monde). Le serveur répond `HELLO_ACK` avec l'intersection des capacités demandées et
//! de celles qu'il gère, et ses limites. Une connexion sans `HELLO` (client d'origine) ne reçoit jamais de
//! paquet 0xAC. Avec la capacité `language/1`, le client peut joindre sa langue au `HELLO` (champ 16) et la
//! changer ensuite (`SET_LANGUAGE`) ; le serveur répond la langue qu'il applique et celles qu'il propose.
//! Spécification complète : page « Extensions ArchRust » du wiki.

use archcore_net::ext_text::normalize_language;
use archcore_net::schema::{self, ext_op};
use archcore_net::{Fields, Packet};
use std::collections::BTreeMap;

/// Version du protocole d'extension gérée par ce serveur.
pub const VERSION_MAJOR: i64 = 1;
pub const VERSION_MINOR: i64 = 1;

/// Capacités gérées par ce serveur, avec leur version.
pub const SUPPORTED: &[(&str, u32)] = &[("addon-msg", 1), ("language", 1)];

/// Limites du canal de messages d'AddOn (version 1).
pub const MAX_PAYLOAD: usize = 255;
pub const RATE_BURST: f64 = 20.0;
pub const RATE_PER_SEC: f64 = 10.0;

pub const ADDON_MSG: &str = "addon-msg";
pub const LANGUAGE: &str = "language";

/// Accord conclu sur une connexion.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Agreement {
    /// Logiciel du client (`ArchRust 0.3.0`).
    pub software: String,
    /// Capacités accordées, avec leur version.
    pub capabilities: BTreeMap<String, u32>,
    /// Langue souhaitée par le client (champ 16 du `HELLO` ou du dernier `SET_LANGUAGE`), normalisée.
    /// La langue réellement appliquée en découle par `locale::Summary::pick`.
    pub language: Option<String>,
}

impl Agreement {
    pub fn has(&self, name: &str) -> bool {
        self.capabilities.contains_key(name)
    }
}

/// Lit une liste « nom/version;nom/version ». Les entrées mal formées sont ignorées.
pub fn parse_capabilities(raw: &str) -> BTreeMap<String, u32> {
    raw.split(';')
        .filter_map(|item| {
            let (name, version) = item.trim().split_once('/')?;
            let name = name.trim();
            let version: u32 = version.trim().parse().ok()?;
            (!name.is_empty() && version > 0).then(|| (name.to_string(), version))
        })
        .collect()
}

pub fn format_capabilities(caps: &BTreeMap<String, u32>) -> String {
    caps.iter().map(|(n, v)| format!("{n}/{v}")).collect::<Vec<_>>().join(";")
}

/// Capacités accordées : intersection, à la version la plus haute connue des deux côtés.
/// Une version majeure différente rend les deux côtés incompatibles (aucune capacité).
pub fn negotiate(client_major: i64, requested: &BTreeMap<String, u32>) -> BTreeMap<String, u32> {
    if client_major != VERSION_MAJOR {
        return BTreeMap::new();
    }
    SUPPORTED
        .iter()
        .filter_map(|(name, ours)| requested.get(*name).map(|theirs| (name.to_string(), (*theirs).min(*ours))))
        .collect()
}

/// Champ 16 (langue, ASCII) d'un paquet d'extension, normalisé.
pub fn language_field(packet: &Packet) -> Option<String> {
    packet.bytes(16).and_then(|b| normalize_language(&String::from_utf8_lossy(b)))
}

/// Réponse `HELLO_ACK` à une demande `HELLO` ; rend aussi l'accord conclu.
pub fn hello(packet: &Packet) -> (Agreement, Fields) {
    let requested = parse_capabilities(&packet.text(5).unwrap_or_default());
    let granted = negotiate(packet.int(2).unwrap_or(0), &requested);
    let software = String::from_utf8_lossy(packet.bytes(4).unwrap_or_default()).chars().take(64).collect();
    let language = if granted.contains_key(LANGUAGE) { language_field(packet) } else { None };
    let s = &schema::ARCHCORE_EXT;
    let mut ack = Fields::new(s)
        .with(0, ext_op::HELLO_ACK)
        .with(2, VERSION_MAJOR)
        .with(3, VERSION_MINOR)
        .with(4, format!("ArchCore {}", env!("CARGO_PKG_VERSION")).into_bytes())
        .with(5, format_capabilities(&granted).into_bytes())
        .with(6, MAX_PAYLOAD)
        .with(7, RATE_BURST as i64)
        .with(8, RATE_PER_SEC as i64);
    if granted.contains_key(LANGUAGE) {
        let summary = crate::locale::summary();
        ack = ack.with(16, summary.pick(language.as_deref()).into_bytes()).with(17, summary.available_list().into_bytes());
    }
    (Agreement { software, capabilities: granted, language }, ack)
}

/// `LANGUAGE_ACK` : langue appliquée (16) et langues proposées (17), en réponse à la demande `request_id`.
pub fn language_ack(request_id: i64, applied: &str, available: &str) -> Fields {
    let s = &schema::ARCHCORE_EXT;
    Fields::new(s)
        .with(0, ext_op::LANGUAGE_ACK)
        .with(1, request_id)
        .with(16, applied.as_bytes().to_vec())
        .with(17, available.as_bytes().to_vec())
}

/// `RESULT` d'erreur pour la demande `request_id`.
pub fn result(request_id: i64, code: i64) -> Fields {
    let s = &schema::ARCHCORE_EXT;
    Fields::new(s).with(0, ext_op::RESULT).with(1, request_id).with(15, code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use archcore_net::{build_packet, decode_packet};

    #[test]
    fn lecture_des_capacites() {
        let c = parse_capabilities("addon-msg/1; saved-vars/2;bad;x/0;/3;y/z");
        assert_eq!(c.len(), 2);
        assert_eq!(c["addon-msg"], 1);
        assert_eq!(c["saved-vars"], 2);
        assert_eq!(format_capabilities(&c), "addon-msg/1;saved-vars/2");
    }

    #[test]
    fn intersection_et_version() {
        let req = parse_capabilities("addon-msg/3;saved-vars/1;language/2");
        assert_eq!(format_capabilities(&negotiate(1, &req)), "addon-msg/1;language/1");
        assert!(negotiate(2, &req).is_empty());
        assert!(negotiate(1, &parse_capabilities("saved-vars/1")).is_empty());
    }

    #[test]
    fn reponse_hello_ack() {
        let s = &schema::ARCHCORE_EXT;
        let hello_pkt = build_packet(
            s,
            &Fields::new(s).with(0, ext_op::HELLO).with(2, 1).with(3, 0).with(4, b"ArchRust 0.3.0".to_vec()).with(5, b"addon-msg/1;saved-vars/1".to_vec()),
        );
        let (agreement, ack) = hello(&decode_packet(s, &hello_pkt).unwrap());
        assert_eq!(agreement.software, "ArchRust 0.3.0");
        assert!(agreement.has(ADDON_MSG));
        assert!(!agreement.has("saved-vars"));
        assert!(agreement.language.is_none());
        let ack = decode_packet(s, &build_packet(s, &ack)).unwrap();
        assert_eq!(ack.int(0), Some(ext_op::HELLO_ACK));
        assert_eq!(ack.int(3), Some(VERSION_MINOR));
        assert_eq!(ack.text(5).as_deref(), Some("addon-msg/1"));
        assert_eq!((ack.int(6), ack.int(7), ack.int(8)), (Some(255), Some(20), Some(10)));
        // Sans la capacité « language », pas de champs 16 et 17 (client 1.0 : champs réservés absents).
        assert!(ack.get(16).is_none() && ack.get(17).is_none());
    }

    #[test]
    fn hello_avec_langue() {
        let s = &schema::ARCHCORE_EXT;
        let hello_pkt = build_packet(
            s,
            &Fields::new(s).with(0, ext_op::HELLO).with(2, 1).with(3, 1).with(4, b"ArchRust 0.4.0".to_vec()).with(5, b"addon-msg/1;language/1".to_vec()).with(16, b"FR-ca".to_vec()),
        );
        let (agreement, ack) = hello(&decode_packet(s, &hello_pkt).unwrap());
        assert!(agreement.has(LANGUAGE));
        assert_eq!(agreement.language.as_deref(), Some("fr-CA"), "souhait normalisé");
        let ack = decode_packet(s, &build_packet(s, &ack)).unwrap();
        assert_eq!(ack.text(5).as_deref(), Some("addon-msg/1;language/1"));
        // Sans résumé installé (tests), le serveur ne propose que l'anglais : langue appliquée « en ».
        assert_eq!(ack.text(16).as_deref(), Some("en"));
        assert_eq!(ack.text(17).as_deref(), Some("en"));
        // Langue mal formée : pas de souhait retenu.
        let bad = build_packet(s, &Fields::new(s).with(0, ext_op::HELLO).with(2, 1).with(5, b"language/1".to_vec()).with(16, b"fran\xe7ais".to_vec()));
        assert!(hello(&decode_packet(s, &bad).unwrap()).0.language.is_none());
    }

    #[test]
    fn paquets_de_langue() {
        let s = &schema::ARCHCORE_EXT;
        let ack = decode_packet(s, &build_packet(s, &language_ack(7, "fr", "fr;en"))).unwrap();
        assert_eq!((ack.int(0), ack.int(1)), (Some(ext_op::LANGUAGE_ACK), Some(7)));
        assert_eq!((ack.text(16).as_deref(), ack.text(17).as_deref()), (Some("fr"), Some("fr;en")));
        let r = decode_packet(s, &build_packet(s, &result(3, 9))).unwrap();
        assert_eq!((r.int(0), r.int(1), r.int(15)), (Some(ext_op::RESULT), Some(3), Some(9)));
        let set = build_packet(s, &Fields::new(s).with(0, ext_op::SET_LANGUAGE).with(16, b"de_DE".to_vec()));
        assert_eq!(language_field(&decode_packet(s, &set).unwrap()).as_deref(), Some("de-DE"));
    }
}
