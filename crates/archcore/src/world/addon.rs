//! Canal de messages d'AddOn (capacité `addon-msg/1`, paquet 0xAC `ADDON_SEND` / `ADDON_RECV`).
//!
//! Contrôles, dans l'ordre de la spécification : connexion avec la capacité et joueur en jeu (sinon le
//! paquet est ignoré), interdiction de discussion, préfixe, taille de la charge, limite de débit, portée et
//! destinataire. Le serveur remplit lui-même l'émetteur, ne lit ni ne journalise la charge, et n'envoie un
//! `RESULT` qu'en cas d'erreur.

use super::{WorldCtx, WorldState};
use crate::ext::{ADDON_MSG, MAX_PAYLOAD, RATE_BURST, RATE_PER_SEC};
use crate::{debug, warn};
use archcore_net::schema::{self, ext_channel, ext_op, ext_result};
use archcore_net::{Fields, Packet};
use std::time::Instant;

/// Préfixe réservé aux messages du serveur.
pub const RESERVED_PREFIX: &[u8] = b"AC.";

/// Seau à jetons d'un joueur : `RATE_BURST` messages d'un coup, `RATE_PER_SEC` rendus par seconde.
#[derive(Clone, Debug)]
pub struct TokenBucket {
    tokens: f64,
    last: Instant,
    /// Refus consécutifs, pour signaler les abus.
    pub refused: u32,
}

impl Default for TokenBucket {
    fn default() -> Self {
        TokenBucket { tokens: RATE_BURST, last: Instant::now(), refused: 0 }
    }
}

impl TokenBucket {
    /// Prend un jeton s'il y en a un.
    pub fn take(&mut self, now: Instant) -> bool {
        let elapsed = now.saturating_duration_since(self.last).as_secs_f64();
        self.tokens = (self.tokens + elapsed * RATE_PER_SEC).min(RATE_BURST);
        self.last = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            self.refused = 0;
            true
        } else {
            self.refused += 1;
            false
        }
    }
}

/// Préfixe valide : 1 à 16 octets ASCII imprimables, sans espace.
pub fn valid_prefix(p: &[u8]) -> bool {
    (1..=16).contains(&p.len()) && p.iter().all(|b| (0x21..=0x7e).contains(b))
}

pub fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) {
    match p.int(0).unwrap_or(-1) {
        ext_op::ADDON_SEND => send(ctx, st, sid, p),
        op => warn!("[addon] #{sid} opération d'extension {op} inattendue du client, ignorée"),
    }
}

fn fail(st: &WorldState, sid: u32, request_id: i64, code: i64) {
    if let Some(player) = st.players.get(&sid) {
        let s = &schema::ARCHCORE_EXT;
        player.send(s, Fields::new(s).with(0, ext_op::RESULT).with(1, request_id).with(15, code));
    }
}

fn recv_fields(prefix: &[u8], channel: i64, target: Option<&str>, cid: u32, name: &str, payload: &[u8]) -> Fields {
    let s = &schema::ARCHCORE_EXT;
    let f = Fields::new(s)
        .with(0, ext_op::ADDON_RECV)
        .with(9, prefix.to_vec())
        .with(10, channel)
        .with_opt(11, target)
        .with(12, cid)
        .with(13, name);
    if payload.is_empty() { f } else { f.with(14, payload.to_vec()) }
}

fn send(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) {
    let Some(player) = st.players.get(&sid) else { return };
    if !player.in_game || !player.sess.has_capability(ADDON_MSG) {
        debug!("[addon] #{sid} message hors jeu ou sans la capacité {ADDON_MSG}, ignoré");
        return;
    }
    let request_id = p.int(1).unwrap_or(0);
    let (cid, name) = (player.ch.id, player.ch.name.clone());
    let now = Instant::now();
    if st.chat_bans.get(&cid).is_some_and(|until| *until > now) {
        return fail(st, sid, request_id, ext_result::NOT_ALLOWED);
    }
    let prefix = p.bytes(9).unwrap_or_default().to_vec();
    if !valid_prefix(&prefix) {
        return fail(st, sid, request_id, ext_result::INVALID_PREFIX);
    }
    if prefix.starts_with(RESERVED_PREFIX) {
        return fail(st, sid, request_id, ext_result::NOT_ALLOWED);
    }
    let payload = p.bytes(14).unwrap_or_default().to_vec();
    if payload.len() > MAX_PAYLOAD {
        return fail(st, sid, request_id, ext_result::MESSAGE_TOO_LONG);
    }
    let player = st.players.get_mut(&sid).unwrap();
    if !player.addon_bucket.take(now) {
        let refused = player.addon_bucket.refused;
        if refused == 1 || refused % 50 == 0 {
            warn!("[addon] {name} dépasse la limite de débit des messages d'AddOn ({refused} refus d'affilée)");
        }
        return fail(st, sid, request_id, ext_result::THROTTLED);
    }
    let channel = p.int(10).unwrap_or(0);
    match channel {
        ext_channel::WHISPER => {
            let target_name = p.text(11).unwrap_or_default();
            let Some(tsid) = st.by_name(&target_name) else {
                return fail(st, sid, request_id, ext_result::TARGET_OFFLINE);
            };
            let target = &st.players[&tsid];
            if !target.in_game {
                return fail(st, sid, request_id, ext_result::TARGET_OFFLINE);
            }
            if !target.sess.has_capability(ADDON_MSG) {
                return fail(st, sid, request_id, ext_result::TARGET_NO_EXTENSION);
            }
            let exact = target.ch.name.clone();
            target.send(&schema::ARCHCORE_EXT, recv_fields(&prefix, channel, Some(&exact), cid, &name, &payload));
        }
        ext_channel::SAY => {
            // Même rayon que la discussion normale ; l'émetteur reçoit aussi son message.
            let me = st.players[&sid].ch.pos();
            let r2 = ctx.view_radius * ctx.view_radius;
            let fields = recv_fields(&prefix, channel, None, cid, &name, &payload);
            for other in st.players.values() {
                if !other.in_game || !other.sess.has_capability(ADDON_MSG) {
                    continue;
                }
                let o = other.ch.pos();
                let (dx, dz) = ((o.x - me.x) as f64, (o.z - me.z) as f64);
                if dx * dx + dz * dz <= r2 {
                    other.send(&schema::ARCHCORE_EXT, fields.clone());
                }
            }
        }
        // Groupes, guildes, raids, canaux nommés et messages au serveur : pas encore gérés.
        _ => fail(st, sid, request_id, ext_result::NOT_AVAILABLE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn prefixes() {
        assert!(valid_prefix(b"DBM"));
        assert!(valid_prefix(b"0123456789ABCDEF"));
        assert!(!valid_prefix(b""));
        assert!(!valid_prefix(b"0123456789ABCDEFG"));
        assert!(!valid_prefix(b"a b"));
        assert!(!valid_prefix(&[0x7f]));
        assert!(valid_prefix(b"AC.Test") && b"AC.Test".starts_with(RESERVED_PREFIX));
    }

    #[test]
    fn seau_a_jetons() {
        let t0 = Instant::now();
        let mut b = TokenBucket { tokens: RATE_BURST, last: t0, refused: 0 };
        for _ in 0..20 {
            assert!(b.take(t0));
        }
        assert!(!b.take(t0));
        assert_eq!(b.refused, 1);
        // 0,1 s plus tard : un jeton est revenu (10 par seconde).
        assert!(b.take(t0 + Duration::from_millis(100)));
        assert!(!b.take(t0 + Duration::from_millis(100)));
        // Le seau ne dépasse jamais sa capacité.
        let later = t0 + Duration::from_secs(60);
        let n = (0..100).take_while(|_| b.take(later)).count();
        assert_eq!(n, 20);
    }
}
