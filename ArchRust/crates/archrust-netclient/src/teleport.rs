//! Portails de téléportation (AGPMEVENTTELEPORT, 0x31).
//!
//! Un portail est un objet du décor (`obj*.ini`) qui porte la fonction 8 et un `PointName` de
//! `TeleportPoint.ini`. Échange validé avec ArchCore (`sim portal`) :
//!
//! ```text
//! op 4 (1 EVENT_BASE : type 1 objet, identifiant = section de l'objet, fonction 8 ; 2 CID)
//!     → op 5 : accord (renvoie l'EVENT_BASE) ; de loin, le serveur fait d'abord courir le joueur
//! op 0 (1 EVENT_BASE du départ, 2 CID, 3 nom du point d'arrivée en CP949)
//!     → op 1 (2 CID, 4 position d'arrivée), déplacement, op 8 (chargement fini)
//! op 7 (1 EVENT_BASE, 2 CID) : « Retour » depuis un point de zone dangereuse (Archonia
//!     FUN_1402e7f90 ; ArchCore ne le traite pas encore)
//! ```

use archrust_protocol::packet::{build_mini, decode_mini, sparse, Value};
use archrust_protocol::schema::{main, mini, Schema};

/// Fonction d'événement « portail » (`Function=8` des objets).
pub const FUNCTION_TELEPORT: i32 = 8;
/// Type de source « objet du décor » d'une désignation d'événement.
const BASE_TYPE_OBJECT: i8 = 1;

pub mod op {
    pub const TELEPORT_POINT: i8 = 0;
    pub const TELEPORT_START: i8 = 1;
    pub const TELEPORT_RETURN: i8 = 7;
    pub const REQUEST: i8 = 4;
    pub const GRANTED: i8 = 5;
    pub const LOADING: i8 = 8;
}

/// Mini EVENT_BASE désignant le portail porté par l'objet `object`.
pub fn event_base(object: u32) -> Value {
    let s = mini::EVENT_BASE;
    let values = sparse(s, &[(0, Value::I8(BASE_TYPE_OBJECT)), (1, Value::I32(object as i32)), (3, Value::I32(FUNCTION_TELEPORT))]);
    Value::Packet(build_mini(s, &values).unwrap_or_default())
}

/// Demandes au portail.
#[derive(Debug, Clone, PartialEq)]
pub enum TeleportCommand {
    /// Le joueur s'adresse au portail porté par l'objet.
    Request(u32),
    /// Destination choisie depuis le portail `object`.
    Go { object: u32, point: String },
    /// « Retour » (point de départ en zone dangereuse).
    Return(u32),
}

/// Paquet d'une demande au portail pour le personnage `me`.
pub fn command_packet(cmd: &TeleportCommand, me: i32) -> (&'static Schema, Vec<Option<Value>>) {
    let s = main::EVENT_TELEPORT;
    let values = match cmd {
        TeleportCommand::Request(object) => sparse(s, &[(0, Value::I8(op::REQUEST)), (1, event_base(*object)), (2, Value::I32(me))]),
        TeleportCommand::Go { object, point } => {
            let name = encoding_rs::EUC_KR.encode(point).0.into_owned();
            sparse(s, &[(0, Value::I8(op::TELEPORT_POINT)), (1, event_base(*object)), (2, Value::I32(me)), (3, Value::Str(name))])
        }
        TeleportCommand::Return(object) => sparse(s, &[(0, Value::I8(op::TELEPORT_RETURN)), (1, event_base(*object)), (2, Value::I32(me))]),
    };
    (s, values)
}

/// Réponses du portail.
#[derive(Debug, Clone, PartialEq)]
pub enum TeleportEvent {
    /// Accord (op 5) : la liste des destinations du portail `object` peut s'ouvrir.
    Granted { object: u32 },
    /// Départ (op 1) : position d'arrivée.
    Start { cid: i32, position: Option<[f32; 3]> },
    /// Chargement terminé (op 8).
    Loaded { cid: i32 },
}

fn int(values: &[Option<Value>], i: usize) -> Option<i64> {
    values.get(i).and_then(Option::as_ref).and_then(Value::as_i64)
}

/// Traduit un paquet 0x31, sinon `None`.
pub fn translate(packet_type: u8, values: &[Option<Value>]) -> Option<TeleportEvent> {
    if packet_type != archrust_protocol::schema::packet_type::EVENT_TELEPORT {
        return None;
    }
    let cid = int(values, 2).unwrap_or(0) as i32;
    Some(match int(values, 0)? as i8 {
        op::GRANTED => {
            let base = values.get(1).and_then(Option::as_ref).and_then(Value::as_bytes).and_then(|b| decode_mini(mini::EVENT_BASE, b).ok());
            TeleportEvent::Granted { object: base.and_then(|(_, v)| int(&v, 1)).unwrap_or(0) as u32 }
        }
        op::TELEPORT_START => TeleportEvent::Start { cid, position: values.get(4).and_then(Option::as_ref).and_then(Value::as_vec3) },
        op::LOADING => TeleportEvent::Loaded { cid },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demande_et_accord() {
        let (s, v) = command_packet(&TeleportCommand::Request(132_646_586), 7);
        assert_eq!(s.id, 0x31);
        assert_eq!((int(&v, 0), int(&v, 2)), (Some(4), Some(7)));
        let base = v[1].as_ref().and_then(Value::as_bytes).unwrap();
        let (_, b) = decode_mini(mini::EVENT_BASE, base).unwrap();
        assert_eq!((int(&b, 0), int(&b, 1), int(&b, 3)), (Some(1), Some(132_646_586), Some(8)));

        let (_, v) = command_packet(&TeleportCommand::Go { object: 1, point: "정찰자의 터".into() }, 7);
        assert_eq!(v[3].as_ref().and_then(Value::as_bytes).map(<[u8]>::to_vec), Some(encoding_rs::EUC_KR.encode("정찰자의 터").0.into_owned()));

        let ok = sparse(main::EVENT_TELEPORT, &[(0, Value::I8(5)), (1, event_base(132_646_586)), (2, Value::I32(7))]);
        assert_eq!(translate(0x31, &ok), Some(TeleportEvent::Granted { object: 132_646_586 }));
        let start = sparse(main::EVENT_TELEPORT, &[(0, Value::I8(1)), (2, Value::I32(7)), (4, Value::Vec3([1.0, 2.0, 3.0]))]);
        assert_eq!(translate(0x31, &start), Some(TeleportEvent::Start { cid: 7, position: Some([1.0, 2.0, 3.0]) }));
    }
}
