//! Guildes : paquets AGPMGUILD (0x15) et AGPMEVENTGUILD (0x39), état de la guilde côté client.
//!
//! ```text
//! AGPMGUILD (drapeaux 4) : 0 op, 1 CID (message système : code), 2 nom de la guilde (texte 1),
//!   3 chef ou joueur (texte 2), 4 8 × INT32 (nombre), 6 date de création, 7 taille, 9 mot de passe,
//!   10 annonce (CP1252), 16 membre (mini GUILD_MEMBER : nom, rang, adhésion, niveau, TID, en ligne)
//!   envoyé : 0 création (2, 9), 1 invitation (2, 16 = l'invité), 2 refus (2), 3 adhésion (2) ou accord
//!            d'une demande par le chef (16), 4 demande de départ, 29 départ accepté (16), 5 exclusion
//!            (16), 6 dissolution (9), 9 annonce (10)
//!   reçu   : 0 la guilde, 1 invitation (2 guilde, 3 inviteur), 3 un membre, 4 départ et 5 exclusion
//!            (16 = le partant), 6 dissolution, 8 lien CID ↔ guilde (1, 2, 16), 9 annonce, 10 message
//!            système (1 code, 2 texte, 4 nombre)
//! AGPMEVENTGUILD (drapeaux 1) : op 0 (1 EVENT_BASE du gestionnaire, fonction 23 ; 2 CID) → op 1 accord
//! ```
//!
//! À l'entrée en jeu, un membre reçoit sa guilde (op 0), chaque membre (op 3) puis son lien (op 8).
//! Rangs : 1 membre, 2 demande d'adhésion, 3 demande de départ, 10 chef, 11 second.

use archrust_protocol::packet::{build_mini, decode_mini, sparse, Value};
use archrust_protocol::schema::{main, mini, packet_type, Schema};
use std::collections::HashMap;

/// Fonction d'événement « gestionnaire de guilde » des PNJ.
pub const FUNCTION_GUILD: i32 = 23;
/// Type de source « personnage » d'une désignation d'événement.
const BASE_TYPE_CHARACTER: i8 = 3;

/// Opérations d'AGPMGUILD.
pub mod op {
    pub const CREATE: i8 = 0;
    pub const JOIN_REQUEST: i8 = 1;
    pub const JOIN_REJECT: i8 = 2;
    pub const JOIN: i8 = 3;
    pub const LEAVE: i8 = 4;
    pub const FORCED_LEAVE: i8 = 5;
    pub const DESTROY: i8 = 6;
    pub const CHAR_DATA: i8 = 8;
    pub const UPDATE_NOTICE: i8 = 9;
    pub const SYSTEM_MESSAGE: i8 = 10;
    pub const LEAVE_ALLOW: i8 = 29;
}

/// Rangs d'un membre.
pub mod rank {
    pub const NORMAL: i32 = 1;
    pub const JOIN_REQUEST: i32 = 2;
    pub const LEAVE_REQUEST: i32 = 3;
    pub const MASTER: i32 = 10;
    pub const SUBMASTER: i32 = 11;
}

/// Codes des messages système (op 10) envoyés par ArchCore.
pub mod code {
    pub const EXIST_GUILD_ID: i32 = 0;
    pub const NEED_MORE_LEVEL: i32 = 1;
    pub const NEED_MORE_MONEY: i32 = 2;
    pub const USE_SPECIAL_CHAR: i32 = 4;
    pub const ALREADY_MEMBER: i32 = 5;
    pub const MAX_MEMBER: i32 = 6;
    pub const JOIN_FAIL: i32 = 7;
    pub const LEAVE_FAIL: i32 = 8;
    pub const NOT_MASTER: i32 = 9;
    pub const INVALID_PASSWORD: i32 = 10;
    pub const CREATE_COMPLETE: i32 = 11;
    pub const DESTROY: i32 = 12;
    pub const JOIN: i32 = 13;
    pub const JOIN_REJECT: i32 = 14;
    pub const LEAVE: i32 = 15;
    pub const FORCED_LEAVE: i32 = 16;
    pub const ALREADY_MEMBER2: i32 = 17;
    pub const JOIN_REQUEST_SELF: i32 = 48;
    pub const LEAVE_REQUEST_SELF: i32 = 50;
}

/// Opérations d'AGPMEVENTGUILD.
pub mod event_op {
    pub const REQUEST: i8 = 0;
    pub const GRANT: i8 = 1;
}

/// Membre d'une guilde (mini GUILD_MEMBER).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GuildMember {
    pub name: String,
    pub rank: i32,
    /// Date d'adhésion (secondes depuis 1970).
    pub joined_at: i64,
    pub level: i64,
    /// Modèle du personnage (race et classe).
    pub tid: i32,
    pub online: bool,
}

impl GuildMember {
    pub fn leads(&self) -> bool {
        matches!(self.rank, rank::MASTER | rank::SUBMASTER)
    }

    /// Membre à part entière (ni demande d'adhésion).
    pub fn active(&self) -> bool {
        self.rank != rank::JOIN_REQUEST
    }
}

/// Mini GUILD_MEMBER réduit au nom (invitation, départ, exclusion).
fn member_named(name: &str) -> Value {
    let s = mini::GUILD_MEMBER;
    Value::Packet(build_mini(s, &sparse(s, &[(0, Value::str(name.as_bytes()))])).unwrap_or_default())
}

fn int(values: &[Option<Value>], i: usize) -> Option<i64> {
    values.get(i).and_then(Option::as_ref).and_then(Value::as_i64)
}

fn text(values: &[Option<Value>], i: usize) -> String {
    values.get(i).and_then(Option::as_ref).and_then(Value::as_text).unwrap_or_default().trim().to_string()
}

/// Texte CP1252 d'un bloc (annonce), jusqu'au premier octet nul.
fn cp1252(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    encoding_rs::WINDOWS_1252.decode(&bytes[..end]).0.into_owned()
}

/// Encode un texte saisi en CP1252 (caractère inconnu : `?`).
pub fn to_cp1252(s: &str) -> Vec<u8> {
    let (bytes, _, errors) = encoding_rs::WINDOWS_1252.encode(s);
    if !errors {
        return bytes.into_owned();
    }
    s.chars()
        .map(|c| {
            let mut buf = [0u8; 4];
            let (b, _, err) = encoding_rs::WINDOWS_1252.encode(c.encode_utf8(&mut buf));
            if err || b.len() != 1 { b'?' } else { b[0] }
        })
        .collect()
}

fn decode_member(values: &[Option<Value>]) -> Option<GuildMember> {
    let b = values.get(16).and_then(Option::as_ref).and_then(Value::as_bytes)?;
    let (_, m) = decode_mini(mini::GUILD_MEMBER, b).ok()?;
    let name = text(&m, 0);
    if name.is_empty() {
        return None;
    }
    Some(GuildMember {
        name,
        rank: int(&m, 1).unwrap_or(0) as i32,
        joined_at: int(&m, 2).unwrap_or(0),
        level: int(&m, 3).unwrap_or(0),
        tid: int(&m, 4).unwrap_or(0) as i32,
        online: int(&m, 5).unwrap_or(0) != 0,
    })
}

/// Fiche d'une guilde (op 0).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GuildInfo {
    pub name: String,
    pub master: String,
    pub created_at: i64,
    pub max_members: i32,
    pub notice: String,
}

/// Paquets de guilde reçus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuildEvent {
    /// La guilde du joueur (création, entrée en jeu, adhésion).
    Guild(GuildInfo),
    /// Invitation reçue : guilde et inviteur.
    Invited { guild: String, from: String },
    /// Membre ajouté ou mis à jour.
    Member { guild: String, member: GuildMember },
    /// Départ (`forced` : exclusion) d'un membre, joueur compris.
    Left { guild: String, name: String, forced: bool },
    Destroyed { guild: String },
    /// Lien d'un personnage à sa guilde (le joueur ou un autre en vue).
    Linked { cid: i32, guild: String, member: Option<GuildMember> },
    Notice { guild: String, text: String },
    /// Message système : code, textes, nombre.
    System { code: i32, text: String, text2: String, number: i64 },
    /// Accord d'un gestionnaire de guilde (AGPMEVENTGUILD op 1) : CID du PNJ.
    ManagerGranted { npc: i32 },
}

/// Traduit un paquet 0x15 ou 0x39, sinon `None`.
pub fn translate(packet_type: u8, values: &[Option<Value>]) -> Option<GuildEvent> {
    if packet_type == packet_type::EVENT_GUILD {
        if int(values, 0)? as i8 != event_op::GRANT {
            return None;
        }
        let base = values.get(1).and_then(Option::as_ref).and_then(Value::as_bytes).and_then(|b| decode_mini(mini::EVENT_BASE, b).ok());
        return Some(GuildEvent::ManagerGranted { npc: base.and_then(|(_, v)| int(&v, 1)).unwrap_or(0) as i32 });
    }
    if packet_type != packet_type::GUILD {
        return None;
    }
    let guild = text(values, 2);
    Some(match int(values, 0)? as i8 {
        op::CREATE => GuildEvent::Guild(GuildInfo {
            name: guild,
            master: text(values, 3),
            created_at: int(values, 6).unwrap_or(0),
            max_members: int(values, 7).unwrap_or(0) as i32,
            notice: values.get(10).and_then(Option::as_ref).and_then(Value::as_bytes).map(cp1252).unwrap_or_default(),
        }),
        op::JOIN_REQUEST => GuildEvent::Invited { guild, from: text(values, 3) },
        op::JOIN => GuildEvent::Member { guild, member: decode_member(values)? },
        o @ (op::LEAVE | op::FORCED_LEAVE) => GuildEvent::Left { guild, name: decode_member(values)?.name, forced: o == op::FORCED_LEAVE },
        op::DESTROY => GuildEvent::Destroyed { guild },
        op::CHAR_DATA => GuildEvent::Linked { cid: int(values, 1)? as i32, guild, member: decode_member(values) },
        op::UPDATE_NOTICE => GuildEvent::Notice {
            guild,
            text: values.get(10).and_then(Option::as_ref).and_then(Value::as_bytes).map(cp1252).unwrap_or_default(),
        },
        op::SYSTEM_MESSAGE => {
            let number = match values.get(4).and_then(Option::as_ref) {
                Some(Value::Ints(v)) => v.first().copied().unwrap_or(0),
                Some(v) => v.as_i64().unwrap_or(0),
                None => 0,
            };
            GuildEvent::System { code: int(values, 1).unwrap_or(-1) as i32, text: guild, text2: text(values, 3), number }
        }
        _ => return None,
    })
}

/// Demandes du joueur concernant les guildes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuildCommand {
    /// S'adresse au gestionnaire de guilde (PNJ).
    TalkToManager(i32),
    Create { name: String, password: String },
    /// Invite un joueur par son nom (chef ou second).
    Invite { guild: String, name: String },
    /// Refuse l'invitation de cette guilde.
    Reject { guild: String },
    /// Rejoint cette guilde (invitation acceptée) ou demande à la rejoindre.
    Join { guild: String },
    /// Le chef accepte la demande d'adhésion de ce membre.
    AllowJoin { guild: String, name: String },
    /// Demande de départ (le joueur se nomme lui-même).
    Leave { guild: String, name: String },
    /// Le chef accepte la demande de départ de ce membre.
    AllowLeave { guild: String, name: String },
    Kick { guild: String, name: String },
    Destroy { guild: String, password: String },
    Notice { guild: String, text: String },
}

/// Mini EVENT_BASE désignant le gestionnaire de guilde `npc`.
pub fn event_base(npc: i32) -> Value {
    let s = mini::EVENT_BASE;
    let values = sparse(s, &[(0, Value::I8(BASE_TYPE_CHARACTER)), (1, Value::I32(npc)), (3, Value::I32(FUNCTION_GUILD))]);
    Value::Packet(build_mini(s, &values).unwrap_or_default())
}

/// Paquet à envoyer pour une demande ; `me` = CID du joueur.
pub fn command_packet(cmd: &GuildCommand, me: i32) -> (&'static Schema, Vec<Option<Value>>) {
    let s = main::GUILD;
    let packet = |o: i8, guild: &str, extra: Vec<(usize, Value)>| {
        let mut pairs = vec![(0, Value::I8(o)), (1, Value::I32(me))];
        if !guild.is_empty() {
            pairs.push((2, Value::str(guild.as_bytes())));
        }
        pairs.extend(extra);
        (s, sparse(s, &pairs))
    };
    match cmd {
        GuildCommand::TalkToManager(npc) => {
            let e = main::EVENT_GUILD;
            (e, sparse(e, &[(0, Value::I8(event_op::REQUEST)), (1, event_base(*npc)), (2, Value::I32(me))]))
        }
        GuildCommand::Create { name, password } => packet(op::CREATE, name, vec![(9, Value::str(password.as_bytes()))]),
        GuildCommand::Invite { guild, name } => packet(op::JOIN_REQUEST, guild, vec![(16, member_named(name))]),
        GuildCommand::Reject { guild } => packet(op::JOIN_REJECT, guild, vec![]),
        GuildCommand::Join { guild } => packet(op::JOIN, guild, vec![]),
        GuildCommand::AllowJoin { guild, name } => packet(op::JOIN, guild, vec![(16, member_named(name))]),
        GuildCommand::Leave { guild, name } => packet(op::LEAVE, guild, vec![(16, member_named(name))]),
        GuildCommand::AllowLeave { guild, name } => packet(op::LEAVE_ALLOW, guild, vec![(16, member_named(name))]),
        GuildCommand::Kick { guild, name } => packet(op::FORCED_LEAVE, guild, vec![(16, member_named(name))]),
        GuildCommand::Destroy { guild, password } => packet(op::DESTROY, guild, vec![(9, Value::str(password.as_bytes()))]),
        GuildCommand::Notice { guild, text } => packet(op::UPDATE_NOTICE, guild, vec![(10, Value::Block(to_cp1252(text)))]),
    }
}

/// Invitation reçue, en attente de réponse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuildInvitation {
    pub guild: String,
    pub from: String,
}

/// Ce qui a changé, pour l'interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuildNotice {
    /// Le joueur a (de nouveau) une guilde.
    Joined,
    /// Le joueur n'a plus de guilde (départ, exclusion, dissolution).
    Left,
    Roster,
    NoticeChanged,
    Invited(GuildInvitation),
    /// Un membre se connecte (`true`) ou se déconnecte.
    MemberOnline(String, bool),
}

/// État de la guilde du joueur et des liens connus des autres personnages.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GuildState {
    pub info: Option<GuildInfo>,
    /// Membres dans l'ordre d'arrivée.
    pub members: Vec<GuildMember>,
    pub invitation: Option<GuildInvitation>,
    /// Guilde de chaque personnage connu (CID → nom de la guilde).
    pub links: HashMap<i32, String>,
}

impl GuildState {
    pub fn in_guild(&self) -> bool {
        self.info.is_some()
    }

    pub fn name(&self) -> &str {
        self.info.as_ref().map_or("", |i| i.name.as_str())
    }

    pub fn member(&self, name: &str) -> Option<&GuildMember> {
        self.members.iter().find(|m| m.name.eq_ignore_ascii_case(name.trim()))
    }

    /// Membres à part entière en ligne, et en tout.
    pub fn counts(&self) -> (usize, usize) {
        let active = self.members.iter().filter(|m| m.active());
        (active.clone().filter(|m| m.online).count(), active.count())
    }

    fn clear_guild(&mut self) -> bool {
        let was = self.in_guild();
        self.info = None;
        self.members.clear();
        was
    }

    /// Applique un paquet reçu ; `me` = nom du joueur, `my_cid` = son CID.
    pub fn apply(&mut self, ev: &GuildEvent, me: &str, my_cid: i32) -> Option<GuildNotice> {
        let mine = |g: &str, s: &Self| s.info.as_ref().is_some_and(|i| i.name.eq_ignore_ascii_case(g));
        match ev {
            GuildEvent::Guild(info) => {
                let fresh = !mine(&info.name, self);
                if fresh {
                    self.members.clear();
                }
                self.info = Some(info.clone());
                self.invitation = None;
                self.links.insert(my_cid, info.name.clone());
                fresh.then_some(GuildNotice::Joined)
            }
            GuildEvent::Invited { guild, from } => {
                let i = GuildInvitation { guild: guild.clone(), from: from.clone() };
                self.invitation = Some(i.clone());
                Some(GuildNotice::Invited(i))
            }
            GuildEvent::Member { guild, member } => {
                if !mine(guild, self) {
                    return None;
                }
                match self.members.iter_mut().find(|m| m.name.eq_ignore_ascii_case(&member.name)) {
                    Some(m) => *m = member.clone(),
                    None => self.members.push(member.clone()),
                }
                Some(GuildNotice::Roster)
            }
            GuildEvent::Left { guild, name, .. } => {
                if !mine(guild, self) {
                    return None;
                }
                if name.eq_ignore_ascii_case(me) {
                    self.links.remove(&my_cid);
                    return self.clear_guild().then_some(GuildNotice::Left);
                }
                self.members.retain(|m| !m.name.eq_ignore_ascii_case(name));
                Some(GuildNotice::Roster)
            }
            GuildEvent::Destroyed { guild } => {
                if !mine(guild, self) {
                    return None;
                }
                self.links.retain(|_, g| !g.eq_ignore_ascii_case(guild));
                self.clear_guild().then_some(GuildNotice::Left)
            }
            GuildEvent::Linked { cid, guild, member } => {
                self.links.insert(*cid, guild.clone());
                let m = member.as_ref().filter(|_| mine(guild, self))?;
                let was = self.member(&m.name).map(|x| x.online);
                match self.members.iter_mut().find(|x| x.name.eq_ignore_ascii_case(&m.name)) {
                    Some(x) => *x = m.clone(),
                    None => self.members.push(m.clone()),
                }
                match was {
                    Some(o) if o != m.online && !m.name.eq_ignore_ascii_case(me) => Some(GuildNotice::MemberOnline(m.name.clone(), m.online)),
                    _ => Some(GuildNotice::Roster),
                }
            }
            GuildEvent::Notice { guild, text } => {
                let info = self.info.as_mut().filter(|i| i.name.eq_ignore_ascii_case(guild))?;
                info.notice = text.clone();
                Some(GuildNotice::NoticeChanged)
            }
            GuildEvent::System { .. } | GuildEvent::ManagerGranted { .. } => None,
        }
    }

    /// Un personnage quitte la vue : son lien est oublié (sauf le joueur).
    pub fn forget(&mut self, cid: i32, my_cid: i32) {
        if cid != my_cid {
            self.links.remove(&cid);
        }
    }

    /// Déconnexion : plus rien.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(name: &str, r: i32, online: bool) -> Value {
        let s = mini::GUILD_MEMBER;
        let v = sparse(s, &[(0, Value::str(name)), (1, Value::I32(r)), (3, Value::U64(20)), (4, Value::I32(96)), (5, Value::I8(online as i8))]);
        Value::Packet(build_mini(s, &v).unwrap())
    }

    fn guild(pairs: &[(usize, Value)]) -> GuildEvent {
        translate(0x15, &sparse(main::GUILD, pairs)).unwrap()
    }

    #[test]
    fn entree_en_jeu_et_membres() {
        let mut s = GuildState::default();
        let g = guild(&[(0, Value::I8(0)), (2, Value::str("SimGuild")), (3, Value::str("GmTest")), (7, Value::I32(50)), (10, Value::Block(b"Bienvenue \xe0 tous".to_vec()))]);
        assert_eq!(s.apply(&g, "GmTest", 7), Some(GuildNotice::Joined));
        assert_eq!(s.info.as_ref().unwrap().notice, "Bienvenue à tous");
        let m = guild(&[(0, Value::I8(3)), (2, Value::str("SimGuild")), (16, member("GmTest", rank::MASTER, true))]);
        assert_eq!(s.apply(&m, "GmTest", 7), Some(GuildNotice::Roster));
        let m = guild(&[(0, Value::I8(3)), (2, Value::str("SimGuild")), (16, member("SimA", rank::NORMAL, false))]);
        s.apply(&m, "GmTest", 7);
        assert_eq!(s.counts(), (1, 2));
        // SimA se connecte : lien op 8 avec le membre en ligne.
        let l = guild(&[(0, Value::I8(8)), (1, Value::I32(9)), (2, Value::str("SimGuild")), (16, member("SimA", rank::NORMAL, true))]);
        assert_eq!(s.apply(&l, "GmTest", 7), Some(GuildNotice::MemberOnline("SimA".into(), true)));
        assert_eq!(s.links.get(&9).map(String::as_str), Some("SimGuild"));
        // Exclusion de SimA, puis dissolution.
        let k = guild(&[(0, Value::I8(5)), (2, Value::str("SimGuild")), (16, member("SimA", 0, false))]);
        assert_eq!(s.apply(&k, "GmTest", 7), Some(GuildNotice::Roster));
        assert_eq!(s.members.len(), 1);
        let d = guild(&[(0, Value::I8(6)), (2, Value::str("SimGuild"))]);
        assert_eq!(s.apply(&d, "GmTest", 7), Some(GuildNotice::Left));
        assert!(!s.in_guild() && s.links.is_empty());
    }

    #[test]
    fn invitation_et_message_systeme() {
        let mut s = GuildState::default();
        let i = guild(&[(0, Value::I8(1)), (2, Value::str("SimGuild")), (3, Value::str("GmTest")), (16, member("SimA", 0, false))]);
        assert_eq!(s.apply(&i, "SimA", 9), Some(GuildNotice::Invited(GuildInvitation { guild: "SimGuild".into(), from: "GmTest".into() })));
        let sys = guild(&[(0, Value::I8(10)), (1, Value::I32(2)), (4, Value::Ints(vec![10_000, 0, 0, 0, 0, 0, 0, 0]))]);
        assert_eq!(sys, GuildEvent::System { code: 2, text: String::new(), text2: String::new(), number: 10_000 });
    }

    #[test]
    fn paquets_envoyes() {
        let (s, v) = command_packet(&GuildCommand::Invite { guild: "SimGuild".into(), name: "SimA".into() }, 7);
        assert_eq!(s.id, 0x15);
        assert_eq!((int(&v, 0), int(&v, 1), text(&v, 2)), (Some(1), Some(7), "SimGuild".to_string()));
        let b = v[16].as_ref().and_then(Value::as_bytes).unwrap();
        assert_eq!(text(&decode_mini(mini::GUILD_MEMBER, b).unwrap().1, 0), "SimA");
        let (_, v) = command_packet(&GuildCommand::Notice { guild: "G".into(), text: "Rendez-vous à Anchorville".into() }, 7);
        assert_eq!(v[10].as_ref().and_then(Value::as_bytes).map(cp1252).as_deref(), Some("Rendez-vous à Anchorville"));
        let (s, v) = command_packet(&GuildCommand::TalkToManager(0x4000_0010), 7);
        assert_eq!((s.id, int(&v, 0), int(&v, 2)), (0x39, Some(0), Some(7)));
        let ok = sparse(main::EVENT_GUILD, &[(0, Value::I8(1)), (1, event_base(0x4000_0010)), (2, Value::I32(7))]);
        assert_eq!(translate(0x39, &ok), Some(GuildEvent::ManagerGranted { npc: 0x4000_0010 }));
    }
}
