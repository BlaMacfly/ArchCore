//! Extensions ArchRust ↔ ArchCore : négociation de capacités (HELLO / HELLO_ACK).
//!
//! Le client ArchRust envoie un paquet 0xAC `HELLO` juste après la poignée de main chiffrée, sur chaque
//! connexion (login, monde). Le serveur répond `HELLO_ACK` avec l'intersection des capacités demandées et
//! de celles qu'il gère, et ses limites. Une connexion sans `HELLO` (client d'origine) ne reçoit jamais de
//! paquet 0xAC. Spécification complète : page « Extensions ArchRust » du wiki.

use archcore_net::schema::{self, ext_op};
use archcore_net::{Fields, Packet};
use std::collections::BTreeMap;

/// Version du protocole d'extension gérée par ce serveur.
pub const VERSION_MAJOR: i64 = 1;
pub const VERSION_MINOR: i64 = 0;

/// Capacités gérées par ce serveur, avec leur version.
pub const SUPPORTED: &[(&str, u32)] = &[("addon-msg", 1)];

/// Limites du canal de messages d'AddOn (version 1).
pub const MAX_PAYLOAD: usize = 255;
pub const RATE_BURST: f64 = 20.0;
pub const RATE_PER_SEC: f64 = 10.0;

pub const ADDON_MSG: &str = "addon-msg";

/// Accord conclu sur une connexion.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Agreement {
    /// Logiciel du client (`ArchRust 0.3.0`).
    pub software: String,
    /// Capacités accordées, avec leur version.
    pub capabilities: BTreeMap<String, u32>,
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

/// Réponse `HELLO_ACK` à une demande `HELLO` ; rend aussi l'accord conclu.
pub fn hello(packet: &Packet) -> (Agreement, Fields) {
    let requested = parse_capabilities(&packet.text(5).unwrap_or_default());
    let granted = negotiate(packet.int(2).unwrap_or(0), &requested);
    let software = String::from_utf8_lossy(packet.bytes(4).unwrap_or_default()).chars().take(64).collect();
    let s = &schema::ARCHCORE_EXT;
    let ack = Fields::new(s)
        .with(0, ext_op::HELLO_ACK)
        .with(2, VERSION_MAJOR)
        .with(3, VERSION_MINOR)
        .with(4, format!("ArchCore {}", env!("CARGO_PKG_VERSION")).into_bytes())
        .with(5, format_capabilities(&granted).into_bytes())
        .with(6, MAX_PAYLOAD)
        .with(7, RATE_BURST as i64)
        .with(8, RATE_PER_SEC as i64);
    (Agreement { software, capabilities: granted }, ack)
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
        let req = parse_capabilities("addon-msg/3;saved-vars/1");
        assert_eq!(format_capabilities(&negotiate(1, &req)), "addon-msg/1");
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
        let ack = decode_packet(s, &build_packet(s, &ack)).unwrap();
        assert_eq!(ack.int(0), Some(ext_op::HELLO_ACK));
        assert_eq!(ack.text(5).as_deref(), Some("addon-msg/1"));
        assert_eq!((ack.int(6), ack.int(7), ack.int(8)), (Some(255), Some(20), Some(10)));
    }
}
