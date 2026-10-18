//! Framing réseau ArchLord : encapsulation chiffrée et découpage des trames TCP.
//!
//! Trame chiffrée (après le handshake AGPMSTARTUPENCRYPTION), relevée dans le client
//! (`AuPacket::Encrypt`) :
//! ```text
//!   0x00  u8   0xA1 (serveur → client) ou 0xB1 (client → serveur)
//!   0x01  u32  taille totale de la trame (en-tête 9 + données + pied 1)
//!   0x05  u32  compteur de paquets
//!   0x09  données : paquet en clair (0xD6 … 0x6B) complété de zéros à un multiple de 8 puis
//!         chiffré Blowfish ECB (clé du serveur pour 0xA1, clé du client pour 0xB1)
//!   fin   u8   0xAF (serveur) ou 0xBF (client)
//! ```
//! Avant le handshake, les paquets circulent en clair (0xD6 … 0x6B).

use crate::blowfish::Blowfish;
use crate::packet::{GUARD_HEAD, GUARD_TAIL, HEADER_SIZE};
use crate::Error;

pub const FRAME_SERVER: u8 = 0xa1;
pub const FRAME_CLIENT: u8 = 0xb1;
pub const FRAME_TAIL_SERVER: u8 = 0xaf;
pub const FRAME_TAIL_CLIENT: u8 = 0xbf;
pub const FRAME_HEADER: usize = 9;
pub const FRAME_OVERHEAD: usize = 10;

#[derive(Default)]
pub struct CryptoSession {
    /// Blowfish avec la clé annoncée par le serveur.
    pub server_cipher: Option<Blowfish>,
    /// Blowfish avec la clé envoyée (chiffrée) par le client.
    pub client_cipher: Option<Blowfish>,
    pub counter: u32,
    /// Vrai une fois CRYPT_COMPLETE envoyé.
    pub ready: bool,
}

impl CryptoSession {
    pub fn set_server_key(&mut self, key: &[u8]) -> Result<(), Error> {
        self.server_cipher = Some(Blowfish::new(key)?);
        Ok(())
    }

    pub fn set_client_key(&mut self, key: &[u8]) -> Result<(), Error> {
        self.client_cipher = Some(Blowfish::new(key)?);
        Ok(())
    }

    /// Chiffre un paquet en clair en une trame 0xA1.
    pub fn encode_frame(&mut self, plain: &[u8]) -> Result<Vec<u8>, Error> {
        let cipher = self.server_cipher.as_ref().ok_or_else(|| Error::Invalid("clé serveur absente".into()))?;
        if plain.first() != Some(&GUARD_HEAD) || plain.len() < HEADER_SIZE + 3 {
            return Err(Error::Invalid("paquet en clair invalide".into()));
        }
        let padded = plain.len().div_ceil(8) * 8;
        let mut frame = vec![0u8; FRAME_OVERHEAD + padded];
        frame[FRAME_HEADER..FRAME_HEADER + plain.len()].copy_from_slice(plain);
        cipher.encrypt_le(&mut frame[FRAME_HEADER..FRAME_HEADER + padded])?;
        self.counter = self.counter.wrapping_add(1);
        let len = frame.len() as u32;
        frame[0] = FRAME_SERVER;
        frame[1..5].copy_from_slice(&len.to_le_bytes());
        frame[5..9].copy_from_slice(&self.counter.to_le_bytes());
        *frame.last_mut().unwrap() = FRAME_TAIL_SERVER;
        Ok(frame)
    }

    /// Déchiffre une trame 0xB1/0xA1 complète et rend le paquet en clair.
    pub fn decode_frame(&self, frame: &[u8]) -> Result<Vec<u8>, Error> {
        let from_server = frame[0] == FRAME_SERVER;
        let cipher = if from_server { &self.server_cipher } else { &self.client_cipher };
        let cipher = cipher.as_ref().ok_or_else(|| Error::Invalid("clé absente pour déchiffrer".into()))?;
        let tail = if from_server { FRAME_TAIL_SERVER } else { FRAME_TAIL_CLIENT };
        if frame.len() < FRAME_OVERHEAD || *frame.last().unwrap() != tail {
            return Err(Error::Invalid("pied de trame invalide".into()));
        }
        let mut body = frame[FRAME_HEADER..frame.len() - 1].to_vec();
        if body.len() % 8 != 0 {
            return Err(Error::Invalid("corps chiffré non aligné sur 8".into()));
        }
        cipher.decrypt_le(&mut body)?;
        if body.first() != Some(&GUARD_HEAD) {
            return Err(Error::Invalid("déchiffrement incorrect (pas de 0xD6)".into()));
        }
        let size = u32::from_le_bytes(body[1..5].try_into().unwrap()) as usize;
        if size < HEADER_SIZE + 2 || size > body.len() {
            return Err(Error::Invalid(format!("taille de paquet incohérente {size}")));
        }
        if body[size - 1] != GUARD_TAIL {
            return Err(Error::Invalid("pied de paquet invalide après déchiffrement".into()));
        }
        body.truncate(size);
        Ok(body)
    }
}

/// Une trame complète : paquet en clair, ou trame chiffrée entière.
#[derive(Debug, PartialEq, Eq)]
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
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<Frame>, Error> {
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
                        return Err(Error::Invalid(format!("paquet en clair trop court ({size})")));
                    }
                    false
                }
                FRAME_CLIENT | FRAME_SERVER => {
                    if size < FRAME_OVERHEAD + 24 {
                        return Err(Error::Invalid(format!("trame chiffrée trop courte ({size})")));
                    }
                    true
                }
                other => return Err(Error::Invalid(format!("octet de garde inconnu 0x{other:x}"))),
            };
            if self.pending.len() < size {
                break;
            }
            let data: Vec<u8> = self.pending.drain(..size).collect();
            out.push(Frame { encrypted, data });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::{build_packet, Fields};
    use crate::schema;

    #[test]
    fn aller_retour_chiffre() {
        let key = [7u8; 32];
        let mut server = CryptoSession::default();
        server.set_server_key(&key).unwrap();
        let plain = build_packet(&schema::TIMER, &Fields::new(&schema::TIMER).with(0, 6u8).with(1, 123456u64));
        let frame = server.encode_frame(&plain).unwrap();
        assert_eq!(frame[0], FRAME_SERVER);
        assert_eq!(frame.len() % 8, 2);
        assert_eq!(server.decode_frame(&frame).unwrap(), plain);
    }

    #[test]
    fn decoupage_incremental() {
        let p = build_packet(&schema::TIMER, &Fields::new(&schema::TIMER).with(0, 1u8));
        let mut s = FrameSplitter::default();
        let mut stream = p.clone();
        stream.extend_from_slice(&p);
        assert!(s.push(&stream[..10]).unwrap().is_empty());
        let frames = s.push(&stream[10..]).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].data, p);
    }
}
