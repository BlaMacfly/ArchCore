//! Protocole réseau du client ArchLord 64 bits, reconstruit par rétro-ingénierie de l'exécutable
//! (Ghidra via REA) et validé avec le vrai client contre l'émulateur ArchCore.
//!
//! * [`packet`] : codec « AuPacket » (en-tête de 19 octets, drapeaux de champs, mini-paquets).
//! * [`schema`] : types de paquets, schémas de champs et codes d'opération connus.
//! * [`framing`] : trames chiffrées `A1/B1 … AF/BF` (Blowfish ECB) et découpage du flux TCP.
//! * [`handshake`] : poignée de main `AgpmStartupEncryption` (type 0x48), côté client et serveur.

pub mod framing;
pub mod handshake;
pub mod packet;
pub mod schema;

pub use framing::{CryptoSession, Direction, Frame, FrameSplitter};
pub use packet::{
    build_mini, build_packet, decode_mini, decode_packet, frame_mini, parse_header, Header, Packet,
    Value, GUARD_HEAD, GUARD_TAIL, HEADER_SIZE,
};
pub use schema::{Field, FieldType, Schema};
