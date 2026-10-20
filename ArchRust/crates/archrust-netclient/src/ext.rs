//! Extensions ArchRust ↔ ArchCore (paquet `0xAC`, schéma `ARCHCORE_EXT`), version 1.1 de la
//! spécification (page « Extensions ArchRust » du wiki d'ArchCore, qui fait foi).
//!
//! Sur chaque connexion (login et monde), juste après la poignée de main `0x48`, le client envoie
//! `HELLO` (version 1.1, logiciel, capacités demandées, langue souhaitée). Le serveur répond
//! `HELLO_ACK` avec les capacités accordées, ses limites, la langue qu'il applique et celles qu'il
//! propose. Sans réponse dans les 3 secondes, le serveur est considéré sans extensions ; le client
//! n'attend pas pour autant : la réponse est interceptée au passage par [`crate::Connection`].
//!
//! Capacité `language/1` : `SET_LANGUAGE` change la langue en cours de connexion (`LANGUAGE_ACK`),
//! `TEXT_REQUEST` demande des textes localisés par sorte et identifiants (`TEXT_REPLY`).
//!
//! Le schéma et les constantes viennent de la crate `archcore-net` du serveur (source unique).

use archcore_net::ext_text::{decode_language_list, decode_texts, encode_ids};
use archcore_net::schema::{self, ext_channel, ext_op, ext_result};
use archcore_net::{build_packet, decode_packet, Fields};
use std::collections::BTreeMap;
use std::time::Duration;

pub use archcore_net::ext_text;
pub use archcore_net::schema::ext_channel as channel;
pub use archcore_net::schema::ext_result as result;
pub use archcore_net::schema::ext_text_kind as text_kind;

/// Type de paquet des extensions.
pub const PACKET_TYPE: u8 = schema::ptype::ARCHCORE_EXT;
/// Version du protocole d'extension parlée par ArchRust.
pub const VERSION_MAJOR: u16 = 1;
pub const VERSION_MINOR: u16 = 1;
/// Capacités demandées au serveur.
pub const REQUESTED: &str = "addon-msg/1;language/1";
/// Capacité du canal de messages d'AddOn.
pub const ADDON_MSG: &str = "addon-msg";
/// Capacité de la langue et des textes localisés.
pub const LANGUAGE: &str = "language";
/// Délai au-delà duquel un `HELLO_ACK` n'est plus accepté.
pub const HELLO_TIMEOUT: Duration = Duration::from_secs(3);

/// Nom du logiciel annoncé (`ArchRust x.y.z`, 64 octets au plus).
pub fn software() -> String {
    format!("ArchRust {}", env!("CARGO_PKG_VERSION"))
}

/// Accord conclu avec le serveur (`HELLO_ACK`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Extensions {
    /// Logiciel du serveur (`ArchCore 0.2.0`).
    pub software: String,
    pub major: u16,
    pub minor: u16,
    /// Capacités accordées : nom → version.
    pub capabilities: BTreeMap<String, u32>,
    /// Charge maximale d'un message d'AddOn (octets).
    pub max_payload: u16,
    /// Limite de débit : messages permis d'un coup, puis rendus par seconde.
    pub rate_burst: u16,
    pub rate_per_sec: u16,
    /// Langue appliquée par le serveur (capacité `language`), `None` sans la capacité.
    pub language: Option<String>,
    /// Langues proposées par le serveur, dans son ordre.
    pub languages: Vec<String>,
}

impl Extensions {
    pub fn has(&self, capability: &str) -> bool {
        self.capabilities.contains_key(capability)
    }
}

/// Lit une liste « nom/version;… » ; les entrées mal formées sont ignorées.
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

/// Paquet `HELLO` en clair, avec la langue souhaitée (champ 16) si elle est connue.
pub fn hello_packet(language: Option<&str>) -> Vec<u8> {
    let s = &schema::ARCHCORE_EXT;
    let mut f = Fields::new(s)
        .with(0, ext_op::HELLO)
        .with(2, VERSION_MAJOR)
        .with(3, VERSION_MINOR)
        .with(4, software().into_bytes())
        .with(5, REQUESTED.as_bytes().to_vec());
    if let Some(l) = language.and_then(ext_text::normalize_language) {
        f = f.with(16, l.into_bytes());
    }
    build_packet(s, &f)
}

/// Paquet `SET_LANGUAGE` en clair (langue normalisée ; `None` si le code est mal formé).
pub fn set_language_packet(request_id: u32, language: &str) -> Option<Vec<u8>> {
    let l = ext_text::normalize_language(language)?;
    let s = &schema::ARCHCORE_EXT;
    Some(build_packet(s, &Fields::new(s).with(0, ext_op::SET_LANGUAGE).with(1, request_id).with(16, l.into_bytes())))
}

/// Paquet `TEXT_REQUEST` en clair : sorte (`text_kind`) et identifiants (1 à `text_kind::MAX_IDS`).
pub fn text_request_packet(request_id: u32, kind: &str, ids: &[u32]) -> Option<Vec<u8>> {
    if !text_kind::ALL.contains(&kind) || ids.is_empty() || ids.len() > text_kind::MAX_IDS {
        return None;
    }
    let s = &schema::ARCHCORE_EXT;
    Some(build_packet(
        s,
        &Fields::new(s)
            .with(0, ext_op::TEXT_REQUEST)
            .with(1, request_id)
            .with(18, kind.as_bytes().to_vec())
            .with(19, encode_ids(ids)),
    ))
}

/// Message d'extension reçu (hors `HELLO_ACK`, géré par la connexion).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtMessage {
    HelloAck(Extensions),
    /// `ADDON_RECV` : message d'AddOn relayé par le serveur (émetteur renseigné par lui).
    AddonRecv {
        prefix: String,
        channel: u8,
        /// Destinataire, pour un murmure.
        target: Option<String>,
        sender_cid: i32,
        sender_name: String,
        payload: Vec<u8>,
    },
    /// `RESULT` : refus d'un envoi ou d'une demande (`request_id` de la demande, code `ext_result`).
    Result { request_id: u32, result: u8 },
    /// `LANGUAGE_ACK` : langue appliquée par le serveur et langues proposées.
    LanguageAck { request_id: u32, language: String, languages: Vec<String> },
    /// `TEXT_REPLY` : textes localisés, dans l'ordre demandé (chaîne vide : inconnu du serveur).
    Texts { request_id: u32, language: String, kind: String, texts: Vec<(u32, String)> },
    /// Opération inconnue (ignorée et journalisée, comme le veut la spécification).
    Unknown(i64),
}

/// Décode un paquet `0xAC` en clair.
pub fn decode(raw: &[u8]) -> Option<ExtMessage> {
    let p = match decode_packet(&schema::ARCHCORE_EXT, raw) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("extension 0xAC illisible : {e}");
            return None;
        }
    };
    let text = |i: usize| {
        p.bytes(i)
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .unwrap_or_default()
    };
    let name = |i: usize| p.bytes(i).map(archcore_net::text::decode);
    let request_id = p.int(1).unwrap_or(0) as u32;
    Some(match p.int(0).unwrap_or(-1) {
        ext_op::HELLO_ACK => ExtMessage::HelloAck(Extensions {
            software: text(4),
            major: p.int(2).unwrap_or(0) as u16,
            minor: p.int(3).unwrap_or(0) as u16,
            capabilities: parse_capabilities(&text(5)),
            max_payload: p.int(6).unwrap_or(0) as u16,
            rate_burst: p.int(7).unwrap_or(0) as u16,
            rate_per_sec: p.int(8).unwrap_or(0) as u16,
            language: p.bytes(16).and_then(|b| ext_text::normalize_language(&String::from_utf8_lossy(b))),
            languages: decode_language_list(&text(17)),
        }),
        ext_op::ADDON_RECV => ExtMessage::AddonRecv {
            prefix: text(9),
            channel: p.int(10).unwrap_or(0) as u8,
            target: name(11).filter(|t| !t.is_empty()),
            sender_cid: p.int(12).unwrap_or(0) as i32,
            sender_name: name(13).unwrap_or_default(),
            payload: p.bytes(14).map(<[u8]>::to_vec).unwrap_or_default(),
        },
        ext_op::RESULT => ExtMessage::Result {
            request_id,
            result: p.int(15).unwrap_or(0) as u8,
        },
        ext_op::LANGUAGE_ACK => ExtMessage::LanguageAck {
            request_id,
            language: text(16),
            languages: decode_language_list(&text(17)),
        },
        ext_op::TEXT_REPLY => ExtMessage::Texts {
            request_id,
            language: text(16),
            kind: text(18),
            texts: decode_texts(p.bytes(20).unwrap_or_default()),
        },
        other => ExtMessage::Unknown(other),
    })
}

/// Paquet `ADDON_SEND` en clair. `target` (nom de personnage, Windows-1252) pour un murmure.
pub fn addon_send_packet(
    request_id: u32,
    prefix: &str,
    channel: u8,
    target: Option<&str>,
    payload: &[u8],
) -> Vec<u8> {
    let s = &schema::ARCHCORE_EXT;
    let mut f = Fields::new(s)
        .with(0, ext_op::ADDON_SEND)
        .with(9, prefix.as_bytes().to_vec())
        .with(10, channel);
    if request_id != 0 {
        f = f.with(1, request_id);
    }
    if let Some(t) = target.filter(|t| !t.is_empty()) {
        f = f.with(11, archcore_net::text::encode(t));
    }
    if !payload.is_empty() {
        f = f.with(14, payload.to_vec());
    }
    build_packet(s, &f)
}

/// Nom d'une portée (`WHISPER`, `SAY`…), comme les `chatType` de WoW.
pub fn channel_name(c: u8) -> &'static str {
    match c as i64 {
        ext_channel::WHISPER => "WHISPER",
        ext_channel::SAY => "SAY",
        ext_channel::PARTY => "PARTY",
        ext_channel::GUILD => "GUILD",
        ext_channel::RAID => "RAID",
        ext_channel::CHANNEL => "CHANNEL",
        ext_channel::SERVER => "SERVER",
        _ => "UNKNOWN",
    }
}

/// Portée d'après un `chatType` de WoW (insensible à la casse).
pub fn channel_from_name(name: &str) -> Option<u8> {
    Some(match name.to_ascii_uppercase().as_str() {
        "WHISPER" => ext_channel::WHISPER,
        "SAY" => ext_channel::SAY,
        "PARTY" => ext_channel::PARTY,
        "GUILD" | "OFFICER" => ext_channel::GUILD,
        "RAID" => ext_channel::RAID,
        "CHANNEL" => ext_channel::CHANNEL,
        "SERVER" => ext_channel::SERVER,
        _ => return None,
    } as u8)
}

/// Nom d'un code de résultat (`THROTTLED`…).
pub fn result_name(r: u8) -> &'static str {
    match r as i64 {
        ext_result::OK => "OK",
        ext_result::INVALID_PREFIX => "INVALID_PREFIX",
        ext_result::MESSAGE_TOO_LONG => "MESSAGE_TOO_LONG",
        ext_result::THROTTLED => "THROTTLED",
        ext_result::TARGET_OFFLINE => "TARGET_OFFLINE",
        ext_result::NOT_AVAILABLE => "NOT_AVAILABLE",
        ext_result::NOT_IN_GROUP => "NOT_IN_GROUP",
        ext_result::NOT_ALLOWED => "NOT_ALLOWED",
        ext_result::TARGET_NO_EXTENSION => "TARGET_NO_EXTENSION",
        ext_result::INVALID_REQUEST => "INVALID_REQUEST",
        _ => "UNKNOWN",
    }
}

/// Préfixe valide selon la spécification : 1 à 16 octets ASCII de `0x21` à `0x7E`.
pub fn valid_prefix(prefix: &str) -> bool {
    (1..=16).contains(&prefix.len()) && prefix.bytes().all(|b| (0x21..=0x7e).contains(&b))
}

/// Préfixes réservés au serveur.
pub fn reserved_prefix(prefix: &str) -> bool {
    prefix.starts_with("AC.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use archcore_net::schema::ARCHCORE_EXT;

    #[test]
    fn hello_conforme() {
        let p = decode_packet(&ARCHCORE_EXT, &hello_packet(None)).unwrap();
        assert_eq!(p.header.ptype, 0xac);
        assert_eq!(p.int(0), Some(ext_op::HELLO));
        assert_eq!(p.int(2), Some(1));
        assert_eq!(p.int(3), Some(1));
        assert!(p.text(4).unwrap().starts_with("ArchRust "));
        assert_eq!(p.text(5).as_deref(), Some("addon-msg/1;language/1"));
        // Champs non prévus pour HELLO absents.
        assert!(p.get(1).is_none() && p.get(9).is_none() && p.get(14).is_none() && p.get(16).is_none());
        // Langue souhaitée, normalisée ; un code mal formé n'est pas envoyé.
        let p = decode_packet(&ARCHCORE_EXT, &hello_packet(Some("FR-ca"))).unwrap();
        assert_eq!(p.text(16).as_deref(), Some("fr-CA"));
        let p = decode_packet(&ARCHCORE_EXT, &hello_packet(Some("français"))).unwrap();
        assert!(p.get(16).is_none());
    }

    #[test]
    fn ack_recv_result() {
        let s = &ARCHCORE_EXT;
        let ack = build_packet(
            s,
            &Fields::new(s)
                .with(0, ext_op::HELLO_ACK)
                .with(2, 1)
                .with(3, 0)
                .with(4, b"ArchCore 0.2.0".to_vec())
                .with(5, b"addon-msg/1;bad;x/0".to_vec())
                .with(6, 255)
                .with(7, 20)
                .with(8, 10),
        );
        let Some(ExtMessage::HelloAck(e)) = decode(&ack) else {
            panic!("HELLO_ACK attendu")
        };
        assert_eq!(e.software, "ArchCore 0.2.0");
        assert!(e.has(ADDON_MSG));
        assert_eq!(e.capabilities.len(), 1);
        assert_eq!((e.max_payload, e.rate_burst, e.rate_per_sec), (255, 20, 10));
        assert!(e.language.is_none() && e.languages.is_empty(), "serveur 1.0 : pas de langue");

        let recv = build_packet(
            s,
            &Fields::new(s)
                .with(0, ext_op::ADDON_RECV)
                .with(9, b"MonAddOn".to_vec())
                .with(10, ext_channel::WHISPER)
                .with(11, archcore_net::text::encode("Hélène"))
                .with(12, 42)
                .with(13, archcore_net::text::encode("Éric"))
                .with(14, vec![0u8, 1, 255, b'^']),
        );
        assert_eq!(
            decode(&recv),
            Some(ExtMessage::AddonRecv {
                prefix: "MonAddOn".into(),
                channel: 1,
                target: Some("Hélène".into()),
                sender_cid: 42,
                sender_name: "Éric".into(),
                payload: vec![0, 1, 255, b'^'],
            })
        );

        let res = build_packet(
            s,
            &Fields::new(s)
                .with(0, ext_op::RESULT)
                .with(1, 7u32)
                .with(15, ext_result::THROTTLED),
        );
        assert_eq!(
            decode(&res),
            Some(ExtMessage::Result {
                request_id: 7,
                result: 3
            })
        );
        assert_eq!(result_name(3), "THROTTLED");
        assert_eq!(result_name(9), "INVALID_REQUEST");
    }

    #[test]
    fn langue() {
        let s = &ARCHCORE_EXT;
        let ack = build_packet(
            s,
            &Fields::new(s)
                .with(0, ext_op::HELLO_ACK)
                .with(2, 1)
                .with(3, 1)
                .with(4, b"ArchCore 0.2.0".to_vec())
                .with(5, b"addon-msg/1;language/1".to_vec())
                .with(6, 255)
                .with(7, 20)
                .with(8, 10)
                .with(16, b"fr".to_vec())
                .with(17, b"en;fr;de".to_vec()),
        );
        let Some(ExtMessage::HelloAck(e)) = decode(&ack) else {
            panic!("HELLO_ACK attendu")
        };
        assert!(e.has(LANGUAGE));
        assert_eq!(e.language.as_deref(), Some("fr"));
        assert_eq!(e.languages, vec!["en", "fr", "de"]);

        let set = set_language_packet(5, "de_de").unwrap();
        let p = decode_packet(s, &set).unwrap();
        assert_eq!((p.int(0), p.int(1)), (Some(ext_op::SET_LANGUAGE), Some(5)));
        assert_eq!(p.text(16).as_deref(), Some("de-DE"));
        assert!(set_language_packet(5, "n'importe quoi").is_none());

        let lack = build_packet(
            s,
            &Fields::new(s)
                .with(0, ext_op::LANGUAGE_ACK)
                .with(1, 5u32)
                .with(16, b"de".to_vec())
                .with(17, b"en;fr;de".to_vec()),
        );
        assert_eq!(
            decode(&lack),
            Some(ExtMessage::LanguageAck {
                request_id: 5,
                language: "de".into(),
                languages: vec!["en".into(), "fr".into(), "de".into()]
            })
        );

        let req = text_request_packet(6, text_kind::QUEST_NAME, &[100, 101]).unwrap();
        let p = decode_packet(s, &req).unwrap();
        assert_eq!((p.int(0), p.int(1)), (Some(ext_op::TEXT_REQUEST), Some(6)));
        assert_eq!(p.text(18).as_deref(), Some("quest_name"));
        assert_eq!(ext_text::decode_ids(p.bytes(19).unwrap()), vec![100, 101]);
        assert!(text_request_packet(6, "bidule", &[1]).is_none());
        assert!(text_request_packet(6, text_kind::QUEST_NAME, &[]).is_none());
        assert!(text_request_packet(6, text_kind::QUEST_NAME, &vec![1; 65]).is_none());

        let reply = build_packet(
            s,
            &Fields::new(s)
                .with(0, ext_op::TEXT_REPLY)
                .with(1, 6u32)
                .with(16, b"fr".to_vec())
                .with(18, b"quest_name".to_vec())
                .with(20, ext_text::encode_texts([(100, "Premiers pas"), (101, "")])),
        );
        assert_eq!(
            decode(&reply),
            Some(ExtMessage::Texts {
                request_id: 6,
                language: "fr".into(),
                kind: "quest_name".into(),
                texts: vec![(100, "Premiers pas".into()), (101, String::new())]
            })
        );
    }

    #[test]
    fn envoi_et_validation() {
        let raw = addon_send_packet(9, "Pfx", 1, Some("Élise"), b"");
        let p = decode_packet(&ARCHCORE_EXT, &raw).unwrap();
        assert_eq!(p.int(0), Some(ext_op::ADDON_SEND));
        assert_eq!(p.int(1), Some(9));
        assert_eq!(p.text(9).as_deref(), Some("Pfx"));
        assert_eq!(p.bytes(11), Some(&[0xc9, b'l', b'i', b's', b'e'][..]));
        assert!(p.get(14).is_none(), "charge vide = champ absent");
        assert!(valid_prefix("ArchAce"));
        assert!(!valid_prefix(""));
        assert!(!valid_prefix("avec espace"));
        assert!(!valid_prefix("x".repeat(17).as_str()));
        assert!(reserved_prefix("AC.serveur"));
        assert_eq!(channel_from_name("whisper"), Some(1));
        assert_eq!(channel_from_name("INSTANCE_CHAT"), None);
        assert_eq!(channel_name(2), "SAY");
    }
}
