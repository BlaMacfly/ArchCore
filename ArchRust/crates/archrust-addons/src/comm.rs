//! Messages d'AddOn entre joueurs, par les extensions ArchRust (capacité `addon-msg/1`, paquet
//! `0xAC`) : API `C_ChatInfo.*` façon WoW, `C_ArchCore.*`, file d'envoi lissée selon les limites
//! annoncées par le serveur (seau à jetons, à la manière de ChatThrottleLib), et événements
//! `CHAT_MSG_ADDON`, `ARCHCORE_EXTENSIONS_READY`, `ARCHCORE_ADDON_MESSAGE_FAILED`.
//!
//! Spécification : page « Extensions ArchRust » du wiki d'ArchCore (section 7 pour l'API Lua).

use crate::manager::{fire, HostCommand, Shared};
use mlua::{Lua, MultiValue, Table, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Accord d'extensions conclu avec le serveur monde (`HELLO_ACK`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExtInfo {
    pub software: String,
    pub major: u16,
    pub minor: u16,
    pub capabilities: BTreeMap<String, u32>,
    pub max_payload: u16,
    pub rate_burst: u16,
    pub rate_per_sec: u16,
    /// Langue appliquée par le serveur (capacité `language`), `None` sans la capacité.
    pub language: Option<String>,
    /// Langues proposées par le serveur.
    pub languages: Vec<String>,
}

impl ExtInfo {
    pub fn has(&self, capability: &str) -> bool {
        self.capabilities.contains_key(capability)
    }
}

/// Préfixes enregistrés au plus (comme WoW).
pub const MAX_PREFIXES: usize = 128;
/// Messages en attente au plus : au-delà, `SendAddonMessage` rend `AddonMessageThrottle`.
pub const MAX_QUEUE: usize = 256;

/// Codes de `Enum.SendAddonMessageResult` (valeurs de WoW).
pub mod send_result {
    pub const SUCCESS: i64 = 0;
    pub const INVALID_PREFIX: i64 = 1;
    pub const INVALID_MESSAGE: i64 = 2;
    pub const ADDON_MESSAGE_THROTTLE: i64 = 3;
    pub const INVALID_CHAT_TYPE: i64 = 4;
    pub const NOT_IN_GROUP: i64 = 5;
    pub const TARGET_REQUIRED: i64 = 6;
    pub const INVALID_CHANNEL: i64 = 7;
    pub const CHANNEL_THROTTLE: i64 = 8;
    pub const GENERAL_ERROR: i64 = 9;
    pub const NOT_IN_GUILD: i64 = 10;
    pub const ADDON_MESSAGE_LOCKDOWN: i64 = 11;
    pub const TARGET_OFFLINE: i64 = 12;
}

/// Portées du protocole (`ext_channel` d'ArchCore).
pub fn channel_from_name(name: &str) -> Option<u8> {
    Some(match name.to_ascii_uppercase().as_str() {
        "WHISPER" => 1,
        "SAY" => 2,
        "PARTY" => 3,
        "GUILD" | "OFFICER" => 4,
        "RAID" => 5,
        "CHANNEL" => 6,
        "SERVER" => 7,
        _ => return None,
    })
}

pub fn channel_name(c: u8) -> &'static str {
    match c {
        1 => "WHISPER",
        2 => "SAY",
        3 => "PARTY",
        4 => "GUILD",
        5 => "RAID",
        6 => "CHANNEL",
        7 => "SERVER",
        _ => "UNKNOWN",
    }
}

/// Noms des codes `RESULT` du serveur (`ext_result`).
pub const SERVER_RESULTS: [&str; 10] = [
    "OK",
    "INVALID_PREFIX",
    "MESSAGE_TOO_LONG",
    "THROTTLED",
    "TARGET_OFFLINE",
    "NOT_AVAILABLE",
    "NOT_IN_GROUP",
    "NOT_ALLOWED",
    "TARGET_NO_EXTENSION",
    "INVALID_REQUEST",
];

/// Sortes de textes localisés acceptées par le serveur (`ext_text_kind` d'archcore-net).
pub const TEXT_KINDS: &[&str] = &[
    "npc_name",
    "monster_name",
    "item_name",
    "item_desc",
    "quest_name",
    "quest_desc",
    "skill_name",
    "region_name",
    "npc_dialog",
];
/// Identifiants au plus par demande de textes.
pub const MAX_TEXT_IDS: usize = 64;

/// `C_ArchCore.RequestTexts(kind, ids)` : met en file une demande de textes localisés ; rend son
/// numéro, ou `None` sans la capacité `language` ou si la demande est mal formée. La réponse arrive
/// par `ARCHCORE_TEXTS(requestId, language, kind, textes)`.
pub fn request_texts(shared: &Shared, kind: &str, ids: Vec<u32>) -> Option<u32> {
    if !shared.borrow().ext.as_ref().is_some_and(|e| e.has("language")) {
        return None;
    }
    if !TEXT_KINDS.contains(&kind) || ids.is_empty() || ids.len() > MAX_TEXT_IDS {
        return None;
    }
    let mut s = shared.borrow_mut();
    let request_id = s.comm.new_request(&format!("<texts:{kind}>"));
    s.commands.push(HostCommand::RequestTexts {
        request_id,
        kind: kind.to_string(),
        ids,
    });
    Some(request_id)
}

pub fn valid_prefix(prefix: &[u8]) -> bool {
    (1..=16).contains(&prefix.len()) && prefix.iter().all(|b| (0x21..=0x7e).contains(b))
}

/// Message en attente d'envoi.
#[derive(Debug, Clone, PartialEq)]
pub struct Outgoing {
    pub request_id: u32,
    pub prefix: String,
    pub channel: u8,
    pub target: Option<String>,
    pub payload: Vec<u8>,
}

/// État du canal de messages d'AddOn.
#[derive(Debug, Default)]
pub struct CommState {
    pub prefixes: BTreeSet<String>,
    pub queue: VecDeque<Outgoing>,
    tokens: f64,
    last_refill: f64,
    next_request: u32,
    /// Dernières demandes : numéro → préfixe (pour `ARCHCORE_ADDON_MESSAGE_FAILED`).
    pending: VecDeque<(u32, String)>,
    pub sent: u64,
}

impl CommState {
    /// Capacité et débit du seau côté client : un peu sous les limites du serveur, pour ne pas
    /// provoquer `THROTTLED` à cause des écarts de temps entre les deux machines.
    fn bucket(ext: &ExtInfo) -> (f64, f64) {
        let cap = (ext.rate_burst as f64 - 1.0).max(1.0);
        let rate = (ext.rate_per_sec as f64 * 0.9).max(0.5);
        (cap, rate)
    }

    /// Nouvel accord : seau plein.
    pub fn reset(&mut self, ext: &ExtInfo, now: f64) {
        self.tokens = Self::bucket(ext).0;
        self.last_refill = now;
    }

    fn refill(&mut self, ext: &ExtInfo, now: f64) {
        let (cap, rate) = Self::bucket(ext);
        let dt = (now - self.last_refill).max(0.0);
        self.tokens = (self.tokens + dt * rate).min(cap);
        self.last_refill = now;
    }

    pub fn new_request(&mut self, prefix: &str) -> u32 {
        self.next_request = self.next_request.wrapping_add(1).max(1);
        self.pending.push_back((self.next_request, prefix.to_string()));
        if self.pending.len() > 512 {
            self.pending.pop_front();
        }
        self.next_request
    }

    pub fn prefix_of(&self, request_id: u32) -> Option<String> {
        self.pending
            .iter()
            .rev()
            .find(|(id, _)| *id == request_id)
            .map(|(_, p)| p.clone())
    }

    /// Messages à envoyer maintenant (autant que de jetons).
    pub fn drain(&mut self, ext: &ExtInfo, now: f64) -> Vec<Outgoing> {
        self.refill(ext, now);
        let mut out = Vec::new();
        while self.tokens >= 1.0 {
            let Some(m) = self.queue.pop_front() else {
                break;
            };
            self.tokens -= 1.0;
            self.sent += 1;
            out.push(m);
        }
        out
    }
}

/// Envoie ce que le seau permet (appelé à chaque image et après chaque `SendAddonMessage`).
pub fn flush(shared: &Shared) {
    let mut s = shared.borrow_mut();
    let Some(ext) = s.ext.clone() else { return };
    let now = s.host.time;
    let out = s.comm.drain(&ext, now);
    for m in out {
        s.commands.push(HostCommand::AddonSend {
            request_id: m.request_id,
            prefix: m.prefix,
            channel: m.channel,
            target: m.target,
            payload: m.payload,
        });
    }
}

fn lua_bytes(v: &Value) -> Option<Vec<u8>> {
    match v {
        Value::String(s) => Some(s.as_bytes().to_vec()),
        Value::Integer(i) => Some(i.to_string().into_bytes()),
        Value::Number(n) => Some(n.to_string().into_bytes()),
        Value::Nil => Some(Vec::new()),
        _ => None,
    }
}

/// Validation et mise en file d'un message (`C_ChatInfo.SendAddonMessage`).
pub fn send(shared: &Shared, prefix: &Value, text: &Value, chat_type: &Value, target: &Value) -> i64 {
    use send_result::*;
    let ext = shared.borrow().ext.clone();
    let Some(ext) = ext.filter(|e| e.has("addon-msg")) else {
        return GENERAL_ERROR;
    };
    let Some(prefix) = lua_bytes(prefix).filter(|p| valid_prefix(p) && !p.starts_with(b"AC."))
    else {
        return INVALID_PREFIX;
    };
    let prefix = String::from_utf8_lossy(&prefix).into_owned();
    let Some(payload) = lua_bytes(text) else {
        return INVALID_MESSAGE;
    };
    let chat = match chat_type {
        Value::String(s) => s.to_str().map(|s| s.to_string()).unwrap_or_default(),
        Value::Nil => "PARTY".to_string(),
        _ => String::new(),
    };
    let Some(channel) = channel_from_name(&chat) else {
        return INVALID_CHAT_TYPE;
    };
    if payload.len() > ext.max_payload as usize {
        return INVALID_MESSAGE;
    }
    let target = match target {
        Value::String(s) => s
            .to_str()
            .ok()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty()),
        Value::Integer(i) => Some(i.to_string()),
        _ => None,
    };
    if channel == 1 && target.is_none() {
        return TARGET_REQUIRED;
    }
    if channel == 6 && target.is_none() {
        return INVALID_CHANNEL;
    }
    {
        let mut s = shared.borrow_mut();
        if s.comm.queue.len() >= MAX_QUEUE {
            return ADDON_MESSAGE_THROTTLE;
        }
        let request_id = s.comm.new_request(&prefix);
        s.comm.queue.push_back(Outgoing {
            request_id,
            prefix,
            channel,
            target: if channel == 1 || channel == 6 { target } else { None },
            payload,
        });
    }
    flush(shared);
    SUCCESS
}

/// Message reçu : `CHAT_MSG_ADDON(prefix, text, channel, sender, target, 0, 0, "", 0, senderCID)`,
/// seulement pour les préfixes enregistrés.
pub fn receive(
    lua: &Lua,
    shared: &Shared,
    prefix: &str,
    channel: u8,
    target: Option<&str>,
    sender: &str,
    sender_cid: i32,
    payload: &[u8],
) -> bool {
    if !shared.borrow().comm.prefixes.contains(prefix) {
        return false;
    }
    let args = (|| -> mlua::Result<MultiValue> {
        Ok(MultiValue::from_iter([
            Value::String(lua.create_string(prefix)?),
            Value::String(lua.create_string(payload)?),
            Value::String(lua.create_string(channel_name(channel))?),
            Value::String(lua.create_string(sender)?),
            Value::String(lua.create_string(target.unwrap_or(""))?),
            Value::Integer(0),
            Value::Integer(0),
            Value::String(lua.create_string("")?),
            Value::Integer(0),
            Value::Integer(sender_cid as i64),
        ]))
    })();
    match args {
        Ok(a) => {
            fire(lua, shared, "CHAT_MSG_ADDON", a);
            true
        }
        Err(e) => {
            log::error!("CHAT_MSG_ADDON : {e}");
            false
        }
    }
}

/// Refus du serveur : `ARCHCORE_ADDON_MESSAGE_FAILED(prefix, result, resultName, requestId)`.
pub fn failed(lua: &Lua, shared: &Shared, request_id: u32, result: u8) {
    let prefix = shared.borrow().comm.prefix_of(request_id).unwrap_or_default();
    let name = SERVER_RESULTS.get(result as usize).copied().unwrap_or("UNKNOWN");
    log::info!("message d'AddOn {prefix} refusé : {name} (demande {request_id})");
    let args = (|| -> mlua::Result<MultiValue> {
        Ok(MultiValue::from_iter([
            Value::String(lua.create_string(&prefix)?),
            Value::Integer(result as i64),
            Value::String(lua.create_string(name)?),
            Value::Integer(request_id as i64),
        ]))
    })();
    if let Ok(a) = args {
        fire(lua, shared, "ARCHCORE_ADDON_MESSAGE_FAILED", a);
    }
}

fn enum_table(lua: &Lua, pairs: &[(&str, i64)]) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    for (k, v) in pairs {
        t.set(*k, *v)?;
    }
    Ok(t)
}

/// Installe `C_ChatInfo`, `C_ArchCore` et les énumérations.
pub fn install(lua: &Lua, shared: Shared) -> mlua::Result<()> {
    let g = lua.globals();
    let en: Table = match g.get::<Value>("Enum")? {
        Value::Table(t) => t,
        _ => {
            let t = lua.create_table()?;
            g.set("Enum", t.clone())?;
            t
        }
    };
    use send_result::*;
    en.set(
        "SendAddonMessageResult",
        enum_table(
            lua,
            &[
                ("Success", SUCCESS),
                ("InvalidPrefix", INVALID_PREFIX),
                ("InvalidMessage", INVALID_MESSAGE),
                ("AddonMessageThrottle", ADDON_MESSAGE_THROTTLE),
                ("InvalidChatType", INVALID_CHAT_TYPE),
                ("NotInGroup", NOT_IN_GROUP),
                ("TargetRequired", TARGET_REQUIRED),
                ("InvalidChannel", INVALID_CHANNEL),
                ("ChannelThrottle", CHANNEL_THROTTLE),
                ("GeneralError", GENERAL_ERROR),
                ("NotInGuild", NOT_IN_GUILD),
                ("AddOnMessageLockdown", ADDON_MESSAGE_LOCKDOWN),
                ("TargetOffline", TARGET_OFFLINE),
            ],
        )?,
    )?;
    en.set(
        "RegisterAddonMessagePrefixResult",
        enum_table(
            lua,
            &[
                ("Success", 0),
                ("DuplicatePrefix", 1),
                ("InvalidPrefix", 2),
                ("MaxPrefixes", 3),
            ],
        )?,
    )?;
    let pairs: Vec<(&str, i64)> = SERVER_RESULTS
        .iter()
        .enumerate()
        .map(|(i, n)| (*n, i as i64))
        .collect();
    en.set("ArchCoreResult", enum_table(lua, &pairs)?)?;

    let ci: Table = match g.get::<Value>("C_ChatInfo")? {
        Value::Table(t) => t,
        _ => lua.create_table()?,
    };
    {
        let s = shared.clone();
        ci.set(
            "RegisterAddonMessagePrefix",
            lua.create_function(move |_, prefix: Value| {
                let Some(p) = lua_bytes(&prefix).filter(|p| valid_prefix(p)) else {
                    return Ok((false, 2));
                };
                let p = String::from_utf8_lossy(&p).into_owned();
                let mut st = s.borrow_mut();
                if st.comm.prefixes.contains(&p) {
                    return Ok((true, 1));
                }
                if st.comm.prefixes.len() >= MAX_PREFIXES {
                    return Ok((false, 3));
                }
                st.comm.prefixes.insert(p);
                Ok((true, 0))
            })?,
        )?;
        let s = shared.clone();
        ci.set(
            "IsAddonMessagePrefixRegistered",
            lua.create_function(move |_, prefix: String| {
                Ok(s.borrow().comm.prefixes.contains(&prefix))
            })?,
        )?;
        let s = shared.clone();
        ci.set(
            "GetRegisteredAddonMessagePrefixes",
            lua.create_function(move |_, ()| {
                Ok(s.borrow().comm.prefixes.iter().cloned().collect::<Vec<_>>())
            })?,
        )?;
        let s = shared.clone();
        ci.set(
            "SendAddonMessage",
            lua.create_function(
                move |_, (prefix, text, chat, target): (Value, Value, Value, Value)| {
                    Ok(send(&s, &prefix, &text, &chat, &target))
                },
            )?,
        )?;
        ci.set("SendAddonMessageLogged", ci.get::<Value>("SendAddonMessage")?)?;
    }
    g.set("C_ChatInfo", ci.clone())?;
    // Noms d'avant WoW 8.0, encore employés par de vieux AddOns.
    g.set(
        "RegisterAddonMessagePrefix",
        ci.get::<Value>("RegisterAddonMessagePrefix")?,
    )?;
    g.set("SendAddonMessage", ci.get::<Value>("SendAddonMessage")?)?;

    let ac = lua.create_table()?;
    {
        let s = shared.clone();
        ac.set(
            "IsAvailable",
            lua.create_function(move |_, ()| Ok(s.borrow().ext.is_some()))?,
        )?;
        let s = shared.clone();
        ac.set(
            "HasCapability",
            lua.create_function(move |_, name: String| {
                Ok(s.borrow().ext.as_ref().and_then(|e| e.capabilities.get(&name).copied()))
            })?,
        )?;
        let s = shared.clone();
        ac.set(
            "GetCapabilities",
            lua.create_function(move |lua, ()| {
                let t = lua.create_table()?;
                if let Some(e) = &s.borrow().ext {
                    for (k, v) in &e.capabilities {
                        t.set(k.as_str(), *v)?;
                    }
                }
                Ok(t)
            })?,
        )?;
        let s = shared.clone();
        ac.set(
            "GetLimits",
            lua.create_function(move |lua, ()| {
                let st = s.borrow();
                let Some(e) = &st.ext else {
                    return Ok(Value::Nil);
                };
                let t = lua.create_table()?;
                t.set("maxPayload", e.max_payload)?;
                t.set("burst", e.rate_burst)?;
                t.set("perSecond", e.rate_per_sec)?;
                Ok(Value::Table(t))
            })?,
        )?;
        let s = shared.clone();
        ac.set(
            "GetServerSoftware",
            lua.create_function(move |_, ()| {
                Ok(match &s.borrow().ext {
                    Some(e) => (Some(e.software.clone()), Some(e.major), Some(e.minor)),
                    None => (None, None, None),
                })
            })?,
        )?;
        let s = shared.clone();
        ac.set(
            "GetLanguage",
            lua.create_function(move |_, ()| {
                Ok(s.borrow().ext.as_ref().and_then(|e| e.language.clone()))
            })?,
        )?;
        let s = shared.clone();
        ac.set(
            "GetLanguages",
            lua.create_function(move |lua, ()| {
                let t = lua.create_table()?;
                if let Some(e) = &s.borrow().ext {
                    for (i, l) in e.languages.iter().enumerate() {
                        t.set(i + 1, l.as_str())?;
                    }
                }
                Ok(t)
            })?,
        )?;
        let s = shared.clone();
        ac.set(
            "RequestTexts",
            lua.create_function(move |_, (kind, ids): (String, Table)| {
                let mut v: Vec<u32> = Vec::new();
                for id in ids.sequence_values::<i64>() {
                    let id = id?;
                    if (0..=u32::MAX as i64).contains(&id) {
                        v.push(id as u32);
                    }
                }
                Ok(request_texts(&s, &kind, v))
            })?,
        )?;
        let s = shared.clone();
        ac.set(
            "GetPendingMessages",
            lua.create_function(move |_, ()| Ok(s.borrow().comm.queue.len()))?,
        )?;
        let s = shared.clone();
        ac.set(
            "GetSentMessages",
            lua.create_function(move |_, ()| Ok(s.borrow().comm.sent))?,
        )?;
    }
    g.set("C_ArchCore", ac)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ext() -> ExtInfo {
        ExtInfo {
            software: "ArchCore 0.2.0".into(),
            major: 1,
            minor: 0,
            capabilities: [("addon-msg".to_string(), 1)].into_iter().collect(),
            max_payload: 255,
            rate_burst: 20,
            rate_per_sec: 10,
            language: None,
            languages: Vec::new(),
        }
    }

    fn msg(i: u32) -> Outgoing {
        Outgoing {
            request_id: i,
            prefix: "T".into(),
            channel: 2,
            target: None,
            payload: vec![],
        }
    }

    #[test]
    fn seau_a_jetons() {
        let e = ext();
        let mut c = CommState::default();
        c.reset(&e, 0.0);
        for i in 0..30 {
            c.queue.push_back(msg(i));
        }
        // Rafale : 19 d'un coup (un jeton de marge sous les 20 du serveur).
        assert_eq!(c.drain(&e, 0.0).len(), 19);
        // Puis 9 par seconde (10 × 0,9).
        assert_eq!(c.drain(&e, 1.0).len(), 9);
        assert_eq!(c.drain(&e, 1.2).len(), 1);
        assert_eq!(c.queue.len(), 1);
        // Sur 10 s, jamais plus que ce que le serveur accepte (20 + 10 × 10 = 120).
        let mut c = CommState::default();
        c.reset(&e, 0.0);
        let mut sent = 0;
        let mut t = 0.0;
        while t <= 10.0 {
            for i in 0..5 {
                c.queue.push_back(msg(i));
            }
            sent += c.drain(&e, t).len();
            t += 0.016;
        }
        assert!(sent <= 120, "{sent}");
        assert!(sent >= 100, "{sent}");
    }

    #[test]
    fn prefixes_et_resultats() {
        assert!(valid_prefix(b"ArchAce"));
        assert!(!valid_prefix(b""));
        assert!(!valid_prefix(b"a b"));
        assert!(!valid_prefix(&[b'x'; 17]));
        assert_eq!(channel_from_name("whisper"), Some(1));
        assert_eq!(channel_name(2), "SAY");
        assert_eq!(SERVER_RESULTS[3], "THROTTLED");
        let mut c = CommState::default();
        let id = c.new_request("Pfx");
        assert_eq!(c.prefix_of(id).as_deref(), Some("Pfx"));
    }
}
