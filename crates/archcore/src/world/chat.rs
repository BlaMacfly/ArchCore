//! Discussion (AGPMCHATTING, 0x0F) et données MJ (AGPMADMIN, 0x12).
//!
//! Le client envoie un message sans zéro final. Si le joueur a choisi une couleur de discussion, il
//! ajoute « \x01RRGGBB » à la fin du message : ce suffixe est retiré avant d'interpréter le texte, puis
//! remis sur le message relayé.

use super::{gm, send_system, World, WorldState};
use crate::info;
use archcore_net::schema::{self, chat_type, mini};
use archcore_net::{build_mini, build_packet, Fields, Packet};
use std::sync::Arc;
use std::time::Instant;

/// Taille maximale d'un message accepté par le client à la réception.
pub const CHAT_MAX_BYTES: usize = 512;

fn strip_nuls(raw: &[u8]) -> &[u8] {
    let end = raw.iter().rposition(|&b| b != 0).map(|i| i + 1).unwrap_or(0);
    &raw[..end]
}

fn color_suffix(raw: &[u8]) -> Option<usize> {
    if raw.len() >= 7 && raw[raw.len() - 7] == 1 && raw[raw.len() - 6..].iter().all(|b| b.is_ascii_hexdigit()) {
        Some(raw.len() - 7)
    } else {
        None
    }
}

/// Texte d'un message reçu : sans zéro final ni suffixe de couleur.
pub fn chat_text(block: &[u8]) -> String {
    let raw = strip_nuls(block);
    let raw = color_suffix(raw).map(|i| &raw[..i]).unwrap_or(raw);
    archcore_net::text::decode(raw)
}

/// Couleur de discussion choisie par le joueur (suffixe « \x01RRGGBB »), ou `None`.
pub fn chat_color(block: &[u8]) -> Option<Vec<u8>> {
    let raw = strip_nuls(block);
    color_suffix(raw).map(|i| raw[i..].to_vec())
}

/// Paquet AGPMCHATTING op 0. Émetteur facultatif pour les messages système.
pub fn chat_packet(kind: i64, sender_cid: Option<u32>, sender_name: Option<&str>, target_name: Option<&str>, text: &str) -> Vec<u8> {
    chat_packet_bytes(kind, sender_cid, sender_name, target_name, archcore_net::text::encode(text))
}

pub fn chat_packet_bytes(kind: i64, sender_cid: Option<u32>, sender_name: Option<&str>, target_name: Option<&str>, mut data: Vec<u8>) -> Vec<u8> {
    data.truncate(CHAT_MAX_BYTES);
    let s = &schema::CHATTING;
    build_packet(
        s,
        &Fields::new(s).with(0, 0).with_opt(1, sender_cid).with(2, kind).with_opt(4, sender_name).with_opt(5, target_name).with(6, data),
    )
}

/// Paquet AGPMADMIN op 0 qui donne au client le niveau MJ d'un personnage (mini ADMIN_CHAR op 1).
pub fn admin_level_packet(cid: u32, name: &str, level: i64, account: &str) -> Vec<u8> {
    let m = &mini::ADMIN_CHAR;
    let info = build_mini(m, &Fields::new(m).with(0, 1).with(1, cid).with(2, name).with(3, level).with(5, account));
    let s = &schema::ADMIN;
    build_packet(s, &Fields::new(s).with(0, 0).with(1, cid).with(2, info))
}

pub async fn on_chatting(world: &Arc<World>, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some(player) = st.players.get(&sid) else { return Ok(()) };
    if p.int(0) != Some(0) {
        crate::debug!("[world] #{sid} AGPMCHATTING op {:?} ignoré", p.int(0));
        return Ok(());
    }
    let kind = p.int(2).unwrap_or(chat_type::NORMAL);
    let block = p.bytes(6).unwrap_or_default().to_vec();
    let text = chat_text(&block);
    if text.is_empty() {
        return Ok(());
    }
    let (cid, name) = (player.ch.id, player.ch.name.clone());
    info!("[chat] {name} (type {kind}) : {text}");

    if text.starts_with('/') && gm::execute(world, st, sid, &text).await? {
        return Ok(());
    }
    if let Some(until) = st.chat_bans.get(&cid).copied() {
        let now = Instant::now();
        if until > now {
            let left = (until - now).as_secs_f64().ceil() as i64;
            send_system(st, sid, &format!("Vous ne pouvez pas parler encore {}.", gm::format_duration(left)));
            return Ok(());
        }
    }
    let mut colored = archcore_net::text::encode(&text);
    colored.extend(chat_color(&block).unwrap_or_default());
    if kind == chat_type::WHISPER {
        let target_name = p.text(5).unwrap_or_default();
        let Some(tsid) = st.by_name(&target_name) else {
            send_system(st, sid, &format!("{target_name} n'est pas connecté."));
            return Ok(());
        };
        let target = &st.players[&tsid];
        let pkt = chat_packet_bytes(kind, Some(cid), Some(&name), Some(&target.ch.name), colored);
        target.send_raw(pkt.clone());
        if tsid != sid {
            st.players[&sid].send_raw(pkt);
        }
        return Ok(());
    }
    // Discussion de guilde : aux membres en ligne ; de groupe : aux membres du groupe.
    if kind == chat_type::GUILD {
        let pkt = chat_packet_bytes(kind, Some(cid), Some(&name), None, colored);
        if !super::guild::chat(st, cid, &pkt) {
            send_system(st, sid, "Vous n'appartenez à aucune guilde.");
        }
        return Ok(());
    }
    if kind == chat_type::PARTY {
        let Some(members) = st.parties.party_of(cid).map(|p| p.members.clone()) else {
            send_system(st, sid, "Vous n'appartenez à aucun groupe.");
            return Ok(());
        };
        let pkt = chat_packet_bytes(kind, Some(cid), Some(&name), None, colored);
        for m in members {
            if let Some(p) = st.by_cid.get(&m).and_then(|s| st.players.get(s)) {
                p.send_raw(pkt.clone());
            }
        }
        return Ok(());
    }
    // Discussion normale : tous les joueurs à portée de vue, émetteur compris (le client n'affiche son
    // propre message qu'au retour du serveur).
    let pkt = chat_packet_bytes(chat_type::NORMAL, Some(cid), Some(&name), None, colored);
    let me = st.players[&sid].ch.pos();
    let r2 = st.view_radius * st.view_radius;
    for other in st.players.values() {
        let o = other.ch.pos();
        let (dx, dz) = ((o.x - me.x) as f64, (o.z - me.z) as f64);
        if dx * dx + dz * dz <= r2 {
            other.send_raw(pkt.clone());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suffixe_de_couleur() {
        let raw = b"bonjour\x01FF8800\0".to_vec();
        assert_eq!(chat_text(&raw), "bonjour");
        assert_eq!(chat_color(&raw), Some(b"\x01FF8800".to_vec()));
        assert_eq!(chat_text(b"/SetExp 2"), "/SetExp 2");
        assert_eq!(chat_color(b"salut"), None);
    }
}
