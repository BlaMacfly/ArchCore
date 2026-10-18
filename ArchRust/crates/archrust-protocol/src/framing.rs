//! Trames réseau ArchLord : encapsulation chiffrée et découpage du flux TCP.
//!
//! Trame chiffrée (après la poignée de main), relevée dans `AuPacket::Encrypt` :
//!
//! | Octets | Contenu |
//! |--------|---------|
//! | 0      | `0xA1` (serveur → client) ou `0xB1` (client → serveur) |
//! | 1–4    | taille totale de la trame (u32) |
//! | 5–8    | compteur de paquets (u32) |
//! | 9…     | paquet en clair complété de zéros à un multiple de 8, chiffré Blowfish ECB |
//! | fin    | `0xAF` (serveur) ou `0xBF` (client) |
//!
//! Le serveur chiffre avec **sa** clé (annoncée en `PUBLIC_KEY`), le client avec la sienne (envoyée
//! chiffrée en `PRIVATE_KEY`). Avant la poignée de main, les paquets circulent en clair.

use crate::packet::{parse_header, GUARD_HEAD, GUARD_TAIL, HEADER_SIZE};
use archrust_crypto::BlowfishLe;

pub const FRAME_SERVER: u8 = 0xa1;
pub const FRAME_CLIENT: u8 = 0xb1;
pub const FRAME_TAIL_SERVER: u8 = 0xaf;
pub const FRAME_TAIL_CLIENT: u8 = 0xbf;
pub const FRAME_HEADER: usize = 9;
pub const FRAME_OVERHEAD: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Trames émises par le serveur (`A1 … AF`).
    ServerToClient,
    /// Trames émises par le client (`B1 … BF`).
    ClientToServer,
}

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("clé Blowfish absente pour {0:?}")]
    MissingKey(Direction),
    #[error("paquet en clair invalide")]
    BadPlain,
    #[error("pied de trame invalide")]
    BadTail,
    #[error("corps chiffré non aligné sur 8 octets")]
    Unaligned,
    #[error("déchiffrement incorrect (pas de 0xD6)")]
    BadDecrypt,
    #[error("taille de paquet incohérente {0}")]
    BadSize(u32),
    #[error("octet de garde inconnu 0x{0:02x}")]
    BadGuard(u8),
    #[error("trame trop courte ({0})")]
    TooShort(u32),
}

/// État de chiffrement d'une connexion.
#[derive(Default, Clone)]
pub struct CryptoSession {
    server_cipher: Option<BlowfishLe>,
    client_cipher: Option<BlowfishLe>,
    counter: u32,
    /// `true` une fois `COMPLETE` échangé : tout circule chiffré.
    pub ready: bool,
}

impl CryptoSession {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn set_server_key(&mut self, key: &[u8]) {
        self.server_cipher = Some(BlowfishLe::new(key).expect("clé Blowfish de 4 à 56 octets"));
    }
    pub fn set_client_key(&mut self, key: &[u8]) {
        self.client_cipher = Some(BlowfishLe::new(key).expect("clé Blowfish de 4 à 56 octets"));
    }
    pub fn server_cipher(&self) -> Option<&BlowfishLe> {
        self.server_cipher.as_ref()
    }
    pub fn client_cipher(&self) -> Option<&BlowfishLe> {
        self.client_cipher.as_ref()
    }

    fn cipher_for(&self, dir: Direction) -> Result<&BlowfishLe, FrameError> {
        match dir {
            Direction::ServerToClient => self.server_cipher.as_ref(),
            Direction::ClientToServer => self.client_cipher.as_ref(),
        }
        .ok_or(FrameError::MissingKey(dir))
    }

    /// Chiffre un paquet en clair en une trame dans la direction donnée.
    pub fn encode_frame(&mut self, plain: &[u8], dir: Direction) -> Result<Vec<u8>, FrameError> {
        let cipher = self.cipher_for(dir)?;
        if plain.first() != Some(&GUARD_HEAD) || plain.len() < HEADER_SIZE + 3 {
            return Err(FrameError::BadPlain);
        }
        let padded = (plain.len() + 7) & !7;
        let mut frame = vec![0u8; FRAME_OVERHEAD + padded];
        frame[FRAME_HEADER..FRAME_HEADER + plain.len()].copy_from_slice(plain);
        cipher.encrypt_buffer(&mut frame[FRAME_HEADER..FRAME_HEADER + padded]);
        let (head, tail) = match dir {
            Direction::ServerToClient => (FRAME_SERVER, FRAME_TAIL_SERVER),
            Direction::ClientToServer => (FRAME_CLIENT, FRAME_TAIL_CLIENT),
        };
        frame[0] = head;
        let len = frame.len() as u32;
        frame[1..5].copy_from_slice(&len.to_le_bytes());
        self.counter = self.counter.wrapping_add(1);
        frame[5..9].copy_from_slice(&self.counter.to_le_bytes());
        let last = frame.len() - 1;
        frame[last] = tail;
        Ok(frame)
    }

    /// Déchiffre une trame complète (`A1`/`B1`) et rend le paquet en clair.
    pub fn decode_frame(&self, frame: &[u8]) -> Result<Vec<u8>, FrameError> {
        let dir = match frame.first() {
            Some(&FRAME_SERVER) => Direction::ServerToClient,
            Some(&FRAME_CLIENT) => Direction::ClientToServer,
            Some(&b) => return Err(FrameError::BadGuard(b)),
            None => return Err(FrameError::TooShort(0)),
        };
        let cipher = self.cipher_for(dir)?;
        let expected_tail = match dir {
            Direction::ServerToClient => FRAME_TAIL_SERVER,
            Direction::ClientToServer => FRAME_TAIL_CLIENT,
        };
        if frame.len() < FRAME_OVERHEAD + 8 {
            return Err(FrameError::TooShort(frame.len() as u32));
        }
        if frame[frame.len() - 1] != expected_tail {
            return Err(FrameError::BadTail);
        }
        let mut body = frame[FRAME_HEADER..frame.len() - 1].to_vec();
        if body.len() % 8 != 0 {
            return Err(FrameError::Unaligned);
        }
        cipher.decrypt_buffer(&mut body);
        if body[0] != GUARD_HEAD {
            return Err(FrameError::BadDecrypt);
        }
        let size = u32::from_le_bytes(body[1..5].try_into().unwrap());
        if (size as usize) < HEADER_SIZE + 2 || size as usize > body.len() {
            return Err(FrameError::BadSize(size));
        }
        if body[size as usize - 1] != GUARD_TAIL {
            return Err(FrameError::BadTail);
        }
        body.truncate(size as usize);
        Ok(body)
    }
}

/// Trame extraite du flux TCP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub encrypted: bool,
    pub data: Vec<u8>,
}

/// Découpeur incrémental : accumule les octets reçus et rend les trames complètes.
#[derive(Default)]
pub struct FrameSplitter {
    pending: Vec<u8>,
}

impl FrameSplitter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<Frame>, FrameError> {
        self.pending.extend_from_slice(chunk);
        let mut out = Vec::new();
        loop {
            if self.pending.len() < 5 {
                break;
            }
            let head = self.pending[0];
            let size = u32::from_le_bytes(self.pending[1..5].try_into().unwrap()) as usize;
            let encrypted = match head {
                GUARD_HEAD => {
                    if size < HEADER_SIZE + 2 {
                        return Err(FrameError::BadSize(size as u32));
                    }
                    false
                }
                FRAME_CLIENT | FRAME_SERVER => {
                    if size < FRAME_OVERHEAD + 24 {
                        return Err(FrameError::TooShort(size as u32));
                    }
                    true
                }
                b => return Err(FrameError::BadGuard(b)),
            };
            if self.pending.len() < size {
                break;
            }
            let data: Vec<u8> = self.pending.drain(..size).collect();
            out.push(Frame { encrypted, data });
        }
        Ok(out)
    }

    /// Type du paquet en clair, pour les journaux.
    pub fn peek_type(plain: &[u8]) -> Option<u8> {
        parse_header(plain).map(|h| h.packet_type)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::{build_packet, sparse, Value};
    use crate::schema::main::TIMER;

    #[test]
    fn frame_roundtrip_both_directions() {
        let mut server = CryptoSession::new();
        let mut client = CryptoSession::new();
        let skey = [1u8; 32];
        let ckey = [2u8; 32];
        server.set_server_key(&skey);
        server.set_client_key(&ckey);
        client.set_server_key(&skey);
        client.set_client_key(&ckey);

        let plain = build_packet(TIMER, &sparse(TIMER, &[(0, Value::U8(1)), (1, Value::U64(123456))])).unwrap();
        let frame = server.encode_frame(&plain, Direction::ServerToClient).unwrap();
        assert_eq!(frame[0], FRAME_SERVER);
        assert_eq!(*frame.last().unwrap(), FRAME_TAIL_SERVER);
        assert_eq!(client.decode_frame(&frame).unwrap(), plain);

        let frame = client.encode_frame(&plain, Direction::ClientToServer).unwrap();
        assert_eq!(frame[0], FRAME_CLIENT);
        assert_eq!(server.decode_frame(&frame).unwrap(), plain);
    }

    #[test]
    fn splitter_reassembles_partial_chunks() {
        let plain = build_packet(TIMER, &sparse(TIMER, &[(0, Value::U8(1))])).unwrap();
        let mut both = plain.clone();
        both.extend_from_slice(&plain);
        let mut s = FrameSplitter::new();
        let first = s.push(&both[..7]).unwrap();
        assert!(first.is_empty());
        let rest = s.push(&both[7..]).unwrap();
        assert_eq!(rest.len(), 2);
        assert!(!rest[0].encrypted);
        assert_eq!(rest[1].data, plain);
    }
}
