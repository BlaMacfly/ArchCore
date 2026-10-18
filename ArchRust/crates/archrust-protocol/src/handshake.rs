//! Poignée de main `AgpmStartupEncryption` (type 0x48), commune au login et au monde.
//!
//! ```text
//! client  → REQUEST (0)               en clair
//! serveur → SET_ALGO (6)              en clair, bloc vide
//! serveur → PUBLIC_KEY (1) + clé      en clair, 32 octets aléatoires : clé Blowfish « serveur »
//! client  → PRIVATE_KEY (2) + clé     en clair, 32 octets chiffrés Blowfish avec la clé serveur
//! serveur → COMPLETE (3)              chiffré ; tout le reste de la session est chiffré
//! ```
//!
//! Ce module fournit les deux rôles sous forme de machines à états sans E/S : l'appelant fait
//! circuler les octets.

use crate::framing::{CryptoSession, Direction};
use crate::packet::{build_packet, decode_packet, sparse, Packet, Value};
use crate::schema::crypt_op;
use crate::schema::main::STARTUP_ENCRYPTION;
use rand::RngCore;

#[derive(Debug, thiserror::Error)]
pub enum HandshakeError {
    #[error("opération de chiffrement inattendue {0}")]
    Unexpected(i64),
    #[error("clé invalide ({0} octets, 32 attendus)")]
    BadKey(usize),
    #[error("paquet : {0}")]
    Packet(#[from] crate::packet::PacketError),
}

fn random_key() -> [u8; 32] {
    let mut key = [0u8; 32];
    rand::rng().fill_bytes(&mut key);
    key
}

fn crypt_packet(op: i8, block: Option<Vec<u8>>) -> Vec<u8> {
    let mut pairs = vec![(0, Value::I8(op))];
    if let Some(b) = block {
        pairs.push((1, Value::Block(b)));
    }
    build_packet(STARTUP_ENCRYPTION, &sparse(STARTUP_ENCRYPTION, &pairs))
        .expect("schéma statique valide")
}

/// Sortie d'une étape : paquets à envoyer, en clair ou chiffrés.
#[derive(Debug, Default)]
pub struct Outgoing {
    pub plain: Vec<Vec<u8>>,
    pub encrypted: Vec<Vec<u8>>,
}

/// Rôle client de la poignée de main.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientState {
    Idle,
    WaitingPublicKey,
    WaitingComplete,
    Done,
}

pub struct ClientHandshake {
    pub state: ClientState,
    client_key: [u8; 32],
}

impl Default for ClientHandshake {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientHandshake {
    pub fn new() -> Self {
        Self {
            state: ClientState::Idle,
            client_key: random_key(),
        }
    }

    /// Premier paquet à envoyer (en clair).
    pub fn start(&mut self) -> Vec<u8> {
        self.state = ClientState::WaitingPublicKey;
        crypt_packet(crypt_op::REQUEST, None)
    }

    /// Traite un paquet 0x48 reçu (déjà en clair). Rend les paquets à renvoyer.
    pub fn on_packet(
        &mut self,
        crypto: &mut CryptoSession,
        plain: &[u8],
    ) -> Result<Outgoing, HandshakeError> {
        let pkt: Packet = decode_packet(STARTUP_ENCRYPTION, plain)?;
        let op = pkt.op().unwrap_or(-1);
        let mut out = Outgoing::default();
        match op as i8 {
            crypt_op::SET_ALGO => {}
            crypt_op::PUBLIC_KEY => {
                let key = pkt
                    .get(1)
                    .and_then(Value::as_bytes)
                    .ok_or(HandshakeError::BadKey(0))?;
                if key.len() != 32 {
                    return Err(HandshakeError::BadKey(key.len()));
                }
                crypto.set_server_key(key);
                crypto.set_client_key(&self.client_key);
                let mut enc = self.client_key.to_vec();
                crypto.server_cipher().unwrap().encrypt_buffer(&mut enc);
                out.plain
                    .push(crypt_packet(crypt_op::PRIVATE_KEY, Some(enc)));
                self.state = ClientState::WaitingComplete;
            }
            crypt_op::COMPLETE => {
                crypto.ready = true;
                self.state = ClientState::Done;
            }
            other => return Err(HandshakeError::Unexpected(other as i64)),
        }
        Ok(out)
    }
}

/// Rôle serveur de la poignée de main (utile aux outils et aux tests).
#[derive(Default)]
pub struct ServerHandshake {
    pub done: bool,
}

impl ServerHandshake {
    pub fn on_packet(
        &mut self,
        crypto: &mut CryptoSession,
        plain: &[u8],
    ) -> Result<Outgoing, HandshakeError> {
        let pkt = decode_packet(STARTUP_ENCRYPTION, plain)?;
        let op = pkt.op().unwrap_or(-1);
        let mut out = Outgoing::default();
        match op as i8 {
            crypt_op::REQUEST => {
                out.plain
                    .push(crypt_packet(crypt_op::SET_ALGO, Some(Vec::new())));
                let server_key = random_key();
                crypto.set_server_key(&server_key);
                out.plain.push(crypt_packet(
                    crypt_op::PUBLIC_KEY,
                    Some(server_key.to_vec()),
                ));
            }
            crypt_op::PRIVATE_KEY => {
                let block = pkt
                    .get(1)
                    .and_then(Value::as_bytes)
                    .ok_or(HandshakeError::BadKey(0))?;
                if block.len() != 32 {
                    return Err(HandshakeError::BadKey(block.len()));
                }
                let mut client_key = block.to_vec();
                crypto
                    .server_cipher()
                    .ok_or(HandshakeError::BadKey(0))?
                    .decrypt_buffer(&mut client_key);
                crypto.set_client_key(&client_key);
                crypto.ready = true;
                let complete = crypt_packet(crypt_op::COMPLETE, None);
                out.encrypted.push(
                    crypto
                        .encode_frame(&complete, Direction::ServerToClient)
                        .expect("clé serveur posée"),
                );
                self.done = true;
            }
            other => return Err(HandshakeError::Unexpected(other as i64)),
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_handshake() {
        let mut client = ClientHandshake::new();
        let mut server = ServerHandshake::default();
        let mut ccrypto = CryptoSession::new();
        let mut scrypto = CryptoSession::new();

        let req = client.start();
        let s1 = server.on_packet(&mut scrypto, &req).unwrap();
        assert_eq!(s1.plain.len(), 2);
        let mut to_server = Vec::new();
        for p in &s1.plain {
            let o = client.on_packet(&mut ccrypto, p).unwrap();
            to_server.extend(o.plain);
        }
        assert_eq!(to_server.len(), 1);
        let s2 = server.on_packet(&mut scrypto, &to_server[0]).unwrap();
        assert!(server.done);
        assert_eq!(s2.encrypted.len(), 1);
        let plain = ccrypto.decode_frame(&s2.encrypted[0]).unwrap();
        client.on_packet(&mut ccrypto, &plain).unwrap();
        assert_eq!(client.state, ClientState::Done);
        assert!(ccrypto.ready);

        // Les deux côtés partagent maintenant les mêmes clés.
        let msg = crypt_packet(crypt_op::COMPLETE, None);
        let f = ccrypto
            .encode_frame(&msg, Direction::ClientToServer)
            .unwrap();
        assert_eq!(scrypto.decode_frame(&f).unwrap(), msg);
    }
}
