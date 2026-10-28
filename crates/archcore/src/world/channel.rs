//! Salons de discussion (AgpmChannel, 0x2C ; fenêtres UI_ChatRoom0..4 d'Archonia, `/join` et `/leave`).
//!
//! Un salon est public, privé (mot de passe de 8 caractères au plus) ou de guilde (créé par le chef, réservé aux
//! membres, un seul par guilde). Limites du client : 5 salons par personnage, 15 membres (100 pour un salon de guilde).
//! Les salons vivent en mémoire : pas de persistance, un personnage quitte les siens en quittant le monde, un salon
//! vide disparaît. Noms comparés sans tenir compte de la casse.
//!
//! Déroulé (`channel_op`, codes `channel_result` de l'op 11) :
//! * création (op 1) : nom pris → 1 ; 5 salons → 4 ; salon de guilde : chef seulement → 5, déjà un → 7 ; sinon
//!   op 3 au créateur ;
//! * entrée (op 3) : inconnu → 2 ; déjà membre → 11 ; guilde d'un autre → 6 ; plein ou 5 salons → 4 ; salon privé :
//!   op 8 (mot de passe demandé) puis op 8 du client, faux → 8 ; une invitation dispense du mot de passe (choix
//!   d'ArchCore). Entrée faite : op 3 à l'entrant (liste des membres, lui compris, toujours présente : le client la lit
//!   sans vérifier), op 5 aux autres ;
//! * message (op 9) : relayé à tous les membres, auteur compris (le client n'affiche rien avant le retour) ;
//! * sortie (op 4) : op 4 au sortant, op 6 aux autres ;
//! * invitation (op 7) : cible en ligne → op 7 chez elle, sinon 3 ; recherche (op 10) : op 10 si en ligne, sinon 3.
//!
//! La règle de région `<Chat>No</Chat>` de regionperculiarity.xml n'est pas appliquée : le fichier d'Archonia n'en
//! contient aucune.

use super::{send_system, WorldState};
use crate::{debug, info, warn};
use archcore_net::schema::{self, channel_kind, channel_op, channel_result};
use archcore_net::{build_packet, Fields, Packet};
use std::collections::HashMap;
use std::time::Instant;

/// Salons par personnage, membres d'un salon (public ou privé, de guilde).
pub const MAX_CHANNELS: usize = 5;
pub const MAX_MEMBERS: usize = 15;
pub const MAX_GUILD_MEMBERS: usize = 100;
/// Longueurs maximales des champs du client : nom, mot de passe, message.
pub const NAME_LEN: usize = 40;
pub const PASSWORD_LEN: usize = 8;
pub const TEXT_LEN: usize = 120;

#[derive(Clone, Debug, PartialEq)]
pub struct Channel {
    pub name: String,
    pub kind: i64,
    pub password: String,
    /// Guilde d'un salon de guilde.
    pub guild: Option<u32>,
    /// Membres (CID, nom), dans l'ordre d'arrivée.
    pub members: Vec<(u32, String)>,
}

impl Channel {
    pub fn capacity(&self) -> usize {
        if self.kind == channel_kind::GUILD { MAX_GUILD_MEMBERS } else { MAX_MEMBERS }
    }
    pub fn has(&self, cid: u32) -> bool {
        self.members.iter().any(|(c, _)| *c == cid)
    }
    /// Liste de l'op 3 : « nom1;nom2;…; » terminée par un zéro (le client ne prend un nom que s'il est suivi de « ; »).
    pub fn member_list(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for (_, name) in &self.members {
            out.extend(archcore_net::text::encode(name));
            out.push(b';');
        }
        out.push(0);
        out
    }
}

/// Salons ouverts et invitations en attente.
#[derive(Default)]
pub struct Channels {
    /// Clé : nom en minuscules.
    pub by_name: HashMap<String, Channel>,
    /// Invitations reçues (CID → clés des salons) : dispensent du mot de passe, une fois.
    pub invites: HashMap<u32, Vec<String>>,
    /// Salon privé dont chaque personnage doit donner le mot de passe (après une op 8 envoyée).
    pub pending: HashMap<u32, String>,
}

impl Channels {
    pub fn count_of(&self, cid: u32) -> usize {
        self.by_name.values().filter(|c| c.has(cid)).count()
    }
    pub fn guild_channel(&self, guild: u32) -> Option<&Channel> {
        self.by_name.values().find(|c| c.guild == Some(guild))
    }
}

pub fn key(name: &str) -> String {
    name.trim().to_lowercase()
}

/// Paquet AgpmChannel : op, CID et champs remplis par `f`.
pub fn packet(op: i64, cid: u32, f: impl FnOnce(Fields) -> Fields) -> Vec<u8> {
    let s = &schema::CHANNEL;
    build_packet(s, &f(Fields::new(s).with(0, op).with(1, cid)))
}

fn send(st: &WorldState, cid: u32, plain: Vec<u8>) {
    if let Some(p) = st.by_cid.get(&cid).and_then(|s| st.players.get(s)) {
        p.send_raw(plain);
    }
}

fn result(st: &WorldState, cid: u32, channel: &str, code: i64) {
    debug!("[salon] résultat {code} pour {cid:#x} ({channel})");
    send(st, cid, packet(channel_op::RESULT, cid, |f| f.with(2, channel).with(6, code)));
}

/// AgpmChannel reçu du client.
pub fn on_packet(st: &mut WorldState, sid: u32, p: &Packet) {
    let Some(player) = st.players.get(&sid) else { return };
    if !player.in_game {
        return;
    }
    let (cid, me) = (player.ch.id, player.ch.name.clone());
    if p.int(1).is_some_and(|c| c != 0 && c != cid as i64) {
        warn!("[salon] #{sid} CID {:?} usurpé", p.int(1));
        return;
    }
    let name = p.text(2).unwrap_or_default().trim().to_string();
    match p.int(0) {
        Some(channel_op::CREATE) => create(st, cid, &me, &name, p.int(5).unwrap_or(channel_kind::PUBLIC), &p.text(3).unwrap_or_default()),
        Some(channel_op::JOIN) => join(st, cid, &me, &name, None),
        Some(channel_op::PASSWORD) => join(st, cid, &me, &name, Some(p.text(3).unwrap_or_default())),
        Some(channel_op::LEAVE) => leave(st, cid, &name),
        Some(channel_op::INVITE) => invite(st, cid, &me, &name, &p.text(4).unwrap_or_default()),
        Some(channel_op::MESSAGE) => message(st, sid, cid, &me, &name, &p.text(7).unwrap_or_default()),
        Some(channel_op::FIND_USER) => {
            let target = p.text(4).unwrap_or_default();
            match st.by_name(target.trim()).and_then(|s| st.players.get(&s)).filter(|t| t.in_game).map(|t| t.ch.name.clone()) {
                Some(found) => send(st, cid, packet(channel_op::FIND_USER, cid, |f| f.with(4, found.as_str()))),
                None => result(st, cid, &name, channel_result::NO_CHARACTER),
            }
        }
        op => debug!("[salon] #{sid} op {op:?} ignorée"),
    }
}

fn create(st: &mut WorldState, cid: u32, me: &str, name: &str, kind: i64, password: &str) {
    if name.is_empty() || name.chars().count() > NAME_LEN || !(channel_kind::PUBLIC..=channel_kind::GUILD).contains(&kind) {
        return result(st, cid, name, channel_result::NOT_EXIST);
    }
    let password = if kind == channel_kind::PRIVATE { password.trim().to_string() } else { String::new() };
    if kind == channel_kind::PRIVATE && (password.is_empty() || password.chars().count() > PASSWORD_LEN) {
        return result(st, cid, name, channel_result::INVALID_PASSWORD);
    }
    if st.channels.by_name.contains_key(&key(name)) {
        return result(st, cid, name, channel_result::ALREADY_EXIST);
    }
    if st.channels.count_of(cid) >= MAX_CHANNELS {
        return result(st, cid, name, channel_result::FULL);
    }
    let mut guild = None;
    if kind == channel_kind::GUILD {
        let gid = st.guilds.of_cid.get(&cid).copied();
        let master = gid.and_then(|g| st.guilds.by_id.get(&g)).and_then(|g| g.master()).is_some_and(|m| m.cid == cid);
        if !master {
            return result(st, cid, name, channel_result::NOT_GUILD_MASTER);
        }
        if gid.is_some_and(|g| st.channels.guild_channel(g).is_some()) {
            return result(st, cid, name, channel_result::GUILD_CHANNEL_EXIST);
        }
        guild = gid;
    }
    let ch = Channel { name: name.to_string(), kind, password, guild, members: vec![(cid, me.to_string())] };
    info!("[salon] {me} crée « {name} » (genre {kind})");
    send(st, cid, entered_packet(&ch, cid));
    st.channels.by_name.insert(key(name), ch);
}

/// Op 3 à un entrant : nom, mot de passe, genre et liste des membres.
fn entered_packet(ch: &Channel, cid: u32) -> Vec<u8> {
    packet(channel_op::JOIN, cid, |f| f.with(2, ch.name.as_str()).with(3, ch.password.as_str()).with(5, ch.kind).with(8, ch.member_list()))
}

/// Entrée dans un salon ; `password` : réponse à l'op 8.
fn join(st: &mut WorldState, cid: u32, me: &str, name: &str, password: Option<String>) {
    let k = key(name);
    let Some(ch) = st.channels.by_name.get(&k) else { return result(st, cid, name, channel_result::NOT_EXIST) };
    if ch.has(cid) {
        return result(st, cid, name, channel_result::ALREADY_JOINED);
    }
    if ch.guild.is_some() && st.guilds.of_cid.get(&cid).copied() != ch.guild {
        return result(st, cid, name, channel_result::NOT_GUILD_MEMBER);
    }
    if ch.members.len() >= ch.capacity() || st.channels.count_of(cid) >= MAX_CHANNELS {
        return result(st, cid, name, channel_result::FULL);
    }
    let invited = st.channels.invites.get(&cid).is_some_and(|l| l.contains(&k));
    if ch.kind == channel_kind::PRIVATE && !invited {
        match password {
            None => {
                st.channels.pending.insert(cid, k);
                let shown = ch.name.clone();
                return send(st, cid, packet(channel_op::PASSWORD, cid, |f| f.with(2, shown.as_str())));
            }
            Some(pw) if pw.trim() != ch.password => {
                info!("[salon] {me} : mauvais mot de passe pour « {} »", ch.name);
                return result(st, cid, name, channel_result::INVALID_PASSWORD);
            }
            Some(_) => {}
        }
    }
    if let Some(list) = st.channels.invites.get_mut(&cid) {
        list.retain(|x| *x != k);
    }
    st.channels.pending.remove(&cid);
    let ch = st.channels.by_name.get_mut(&k).unwrap();
    ch.members.push((cid, me.to_string()));
    let ch = ch.clone();
    info!("[salon] {me} entre dans « {} » ({} membre(s))", ch.name, ch.members.len());
    send(st, cid, entered_packet(&ch, cid));
    for (other, _) in ch.members.iter().filter(|(c, _)| *c != cid) {
        send(st, *other, packet(channel_op::ADD_MEMBER, *other, |f| f.with(2, ch.name.as_str()).with(4, me)));
    }
}

/// Sortie d'un salon : op 4 au sortant, op 6 aux autres ; salon vide supprimé.
fn leave(st: &mut WorldState, cid: u32, name: &str) {
    let k = key(name);
    let Some(ch) = st.channels.by_name.get_mut(&k).filter(|c| c.has(cid)) else { return result(st, cid, name, channel_result::NOT_EXIST) };
    let me = ch.members.iter().find(|(c, _)| *c == cid).map(|(_, n)| n.clone()).unwrap_or_default();
    ch.members.retain(|(c, _)| *c != cid);
    let ch = ch.clone();
    if ch.members.is_empty() {
        st.channels.by_name.remove(&k);
        info!("[salon] « {} » fermé", ch.name);
    }
    send(st, cid, packet(channel_op::LEAVE, cid, |f| f.with(2, ch.name.as_str())));
    for (other, _) in &ch.members {
        send(st, *other, packet(channel_op::REMOVE_MEMBER, *other, |f| f.with(2, ch.name.as_str()).with(4, me.as_str())));
    }
}

fn invite(st: &mut WorldState, cid: u32, me: &str, name: &str, target: &str) {
    let k = key(name);
    let Some(ch) = st.channels.by_name.get(&k).filter(|c| c.has(cid)) else { return result(st, cid, name, channel_result::NOT_EXIST) };
    let (shown, kind) = (ch.name.clone(), ch.kind);
    let Some(tcid) = st.by_name(target.trim()).and_then(|s| st.players.get(&s)).filter(|t| t.in_game).map(|t| t.ch.id) else {
        return result(st, cid, name, channel_result::NO_CHARACTER);
    };
    if ch.has(tcid) {
        return result(st, cid, name, channel_result::ALREADY_JOINED);
    }
    let list = st.channels.invites.entry(tcid).or_default();
    if !list.contains(&k) {
        list.push(k);
    }
    info!("[salon] {me} invite {target} dans « {shown} »");
    send(st, tcid, packet(channel_op::INVITE, tcid, |f| f.with(2, shown.as_str()).with(4, me).with(5, kind)));
}

fn message(st: &mut WorldState, sid: u32, cid: u32, me: &str, name: &str, text: &str) {
    let Some(ch) = st.channels.by_name.get(&key(name)).filter(|c| c.has(cid)) else { return result(st, cid, name, channel_result::NOT_EXIST) };
    if text.trim().is_empty() {
        return;
    }
    if st.chat_bans.get(&cid).is_some_and(|until| *until > Instant::now()) {
        return send_system(st, sid, "Vous ne pouvez pas parler pour le moment.");
    }
    let (shown, members) = (ch.name.clone(), ch.members.clone());
    if !st.players.get_mut(&sid).is_some_and(|p| p.chat.allow_say(Instant::now())) {
        return;
    }
    let text: String = text.chars().take(TEXT_LEN).collect();
    info!("[salon] {me} dans « {shown} » : {text}");
    for (other, _) in members {
        send(st, other, packet(channel_op::MESSAGE, other, |f| f.with(2, shown.as_str()).with(4, me).with(7, text.as_str())));
    }
}

/// Le personnage quitte le monde : il sort de tous ses salons, ses invitations sont oubliées.
pub fn on_leave_world(st: &mut WorldState, cid: u32) {
    let names: Vec<String> = st.channels.by_name.values().filter(|c| c.has(cid)).map(|c| c.name.clone()).collect();
    for name in names {
        leave(st, cid, &name);
    }
    st.channels.invites.remove(&cid);
    st.channels.pending.remove(&cid);
}

#[cfg(test)]
mod tests {
    use super::*;
    use archcore_net::decode_packet;

    #[test]
    fn liste_des_membres() {
        let ch = Channel { name: "Salon".into(), kind: channel_kind::PUBLIC, password: String::new(), guild: None, members: vec![(1, "GmTest".into()), (2, "SimA".into())] };
        assert_eq!(ch.member_list(), b"GmTest;SimA;\0".to_vec());
        assert_eq!((ch.capacity(), Channel { kind: channel_kind::GUILD, ..ch.clone() }.capacity()), (15, 100));
        let p = decode_packet(&schema::CHANNEL, &entered_packet(&ch, 1)).unwrap();
        assert_eq!((p.int(0), p.int(1), p.int(5)), (Some(3), Some(1), Some(1)));
        assert_eq!((p.text(2).as_deref(), p.bytes(8).map(|b| b.to_vec())), (Some("Salon"), Some(b"GmTest;SimA;\0".to_vec())));
    }

    #[test]
    fn liste_jamais_absente() {
        // Un seul membre : le bloc n'est pas vide (un bloc vide serait omis du paquet et le client planterait).
        let ch = Channel { name: "x".into(), kind: channel_kind::PRIVATE, password: "abc".into(), guild: None, members: vec![(1, "A".into())] };
        let p = decode_packet(&schema::CHANNEL, &entered_packet(&ch, 1)).unwrap();
        assert_eq!(p.bytes(8), Some(&b"A;\0"[..]));
        assert_eq!(p.text(3).as_deref(), Some("abc"));
    }

    #[test]
    fn message_et_resultat() {
        let m = decode_packet(&schema::CHANNEL, &packet(channel_op::MESSAGE, 5, |f| f.with(2, "Salon").with(4, "SimA").with(7, "bonjour"))).unwrap();
        assert_eq!((m.int(0), m.text(4).as_deref(), m.text(7).as_deref()), (Some(9), Some("SimA"), Some("bonjour")));
        let r = decode_packet(&schema::CHANNEL, &packet(channel_op::RESULT, 5, |f| f.with(6, channel_result::INVALID_PASSWORD))).unwrap();
        assert_eq!(r.int(6), Some(8));
        assert_eq!(key(" Mon Salon "), "mon salon");
    }
}
