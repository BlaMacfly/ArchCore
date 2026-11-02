//! Actions sociales (« Emotes », épisode 8) : AGPMCHARACTER (0x02) op 16 `SOCIAL_ANIMATION`.
//!
//! ```text
//!   client → op 16 : 1 son CID, 16 type (U8, 0-16 : ligne de SocialAction.txt − 1, salut … assis)
//!   serveur → op 16 : 1 CID, 16 type, au lanceur et à tous ceux qui le voient
//! ```
//! Le client n'anime rien à l'envoi : l'animation et le son (`INI\SOCIAL_PC.txt`, ligne = type, colonne = TID du
//! personnage) suivent l'op 16 reçue, d'où le renvoi au lanceur. Aucun code de résultat : un refus est silencieux.
//! Les types suivants (SELECT* de l'écran de sélection, GM_*) ne viennent pas d'un joueur et sont refusés.
//! Refusée aussi au mort, à cheval et sous métamorphose (le client pose lui-même trois conditions de ce genre avant
//! d'envoyer ; leur sens exact n'est pas établi : choix prudent d'ArchCore), et plus d'une fois par
//! `Social.MinIntervalMs` (anti-inondation, choix d'ArchCore). Pas d'état gardé.

use super::{WorldCtx, WorldState};
use crate::debug;
use archcore_net::schema;
use archcore_net::{build_packet, Fields, Packet};
use std::time::{Duration, Instant};

/// Opération d'AGPMCHARACTER.
pub const SOCIAL_ANIMATION: i64 = 16;
/// Champ du type d'action.
pub const FIELD_TYPE: usize = 16;
/// Types qu'un joueur peut lancer : 0 salut … 16 assis.
pub const PLAYER_TYPES: i64 = 17;

/// Paquet AGPMCHARACTER op 16 : action sociale `kind` du personnage `cid`.
pub fn packet(cid: u32, kind: u8) -> Vec<u8> {
    let c = &schema::CHARACTER;
    build_packet(c, &Fields::new(c).with(0, SOCIAL_ANIMATION).with(1, cid).with(FIELD_TYPE, kind))
}

/// Type reçu, s'il est de ceux qu'un joueur lance.
pub fn player_kind(p: &Packet) -> Option<u8> {
    p.int(FIELD_TYPE).filter(|k| (0..PLAYER_TYPES).contains(k)).map(|k| k as u8)
}

/// Pourquoi l'action est refusée maintenant, sinon `None`.
pub fn refusal(dead: bool, mounted: bool, morphed: bool, last: Option<Instant>, min_interval: Duration, now: Instant) -> Option<&'static str> {
    if dead {
        Some("mort")
    } else if mounted {
        Some("à cheval")
    } else if morphed {
        Some("métamorphosé")
    } else if last.is_some_and(|t| now < t + min_interval) {
        Some("trop rapproché")
    } else {
        None
    }
}

/// Op 16 reçue : relayée au lanceur et à ses voisins.
pub fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) {
    let Some(player) = st.players.get(&sid).filter(|x| x.in_game) else { return };
    let cid = player.ch.id;
    if p.int(1).is_some_and(|c| c != cid as i64) {
        return;
    }
    let Some(kind) = player_kind(p) else {
        debug!("[social] {} : type {:?} refusé", player.ch.name, p.int(FIELD_TYPE));
        return;
    };
    let now = Instant::now();
    let min = Duration::from_millis(ctx.config.world.social_min_interval_ms);
    let why = refusal(player.dead, super::ride::is_mounted(&player.inventory), player.form.morph.is_some(), player.social_at, min, now);
    if let Some(why) = why {
        debug!("[social] {} : action {kind} refusée ({why})", player.ch.name);
        return;
    }
    debug!("[social] {} : action {kind}", player.ch.name);
    st.players.get_mut(&sid).unwrap().social_at = Some(now);
    super::combat::send_around(st, sid, &packet(cid, kind));
}

#[cfg(test)]
mod tests {
    use super::*;
    use archcore_net::decode_packet;

    #[test]
    fn paquet_et_refus() {
        let c = &schema::CHARACTER;
        let p = decode_packet(c, &packet(42, 5)).unwrap();
        assert_eq!((p.int(0), p.int(1), p.int(16)), (Some(16), Some(42), Some(5)));
        assert_eq!(player_kind(&p), Some(5));
        let p = decode_packet(c, &build_packet(c, &Fields::new(c).with(0, 16).with(1, 42).with(16, 17))).unwrap();
        assert_eq!(player_kind(&p), None, "SELECT1 : écran de sélection");
        let p = decode_packet(c, &build_packet(c, &Fields::new(c).with(0, 16).with(1, 42))).unwrap();
        assert_eq!(player_kind(&p), None, "sans type");
        let now = Instant::now();
        let s = Duration::from_secs(1);
        assert_eq!(refusal(false, false, false, None, s, now), None);
        assert_eq!(refusal(true, false, false, None, s, now), Some("mort"));
        assert_eq!(refusal(false, true, false, None, s, now), Some("à cheval"));
        assert_eq!(refusal(false, false, true, None, s, now), Some("métamorphosé"));
        assert_eq!(refusal(false, false, false, Some(now), s, now + Duration::from_millis(500)), Some("trop rapproché"));
        assert_eq!(refusal(false, false, false, Some(now), s, now + s), None);
    }
}
