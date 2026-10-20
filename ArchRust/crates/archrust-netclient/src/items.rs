//! Objets de l'inventaire : déplacer, réunir et séparer des piles, porter et ranger l'équipement,
//! utiliser (potions, parchemins), détruire, revenir au point d'un parchemin aller-retour.
//!
//! Dialogue validé avec ArchCore (scénarios `sim equip`, `sim bag`, `sim potion`, `sim scroll`) :
//!
//! ```text
//! AGPMITEM op 2 statut 3 (champ 2 = IID)                        porter un objet de l'inventaire
//! AGPMITEM op 2 statut 2, champ 7 = mini [sac, ligne, colonne]  ranger un objet porté (sans case :
//!                                                               au choix du serveur), déplacer dans
//!                                                               l'inventaire, réunir deux piles
//!   ← op 2 (statut 3 sans case, ou statut 2 avec sa case), op 1 pour une pile absorbée,
//!     AGPMCHARACTER op 1 (facteurs) ; un refus donne un message système et remet l'objet en place
//! AGPMITEM op 3 (champ 2 = IID)                                  utiliser (potion, parchemin)
//!   ← op 2 (nouvelle quantité) ou op 1 (dernier exemplaire), op 4 (champ 3 TID, 18 délai en ms)
//! AGPMITEM op 17 (2 IID, 5 quantité, 7 case libre)               séparer une pile ← op 0 + op 2
//! AGPMITEM op 21 (champ 2 = IID)                                 détruire ← op 1
//! AGPMITEM op 9 (champ 4 = CID) / op 10                          retour d'aller-retour possible / fini
//! AGPMITEM op 6                                                  revenir au point mémorisé
//! ```

use archrust_protocol::packet::{build_mini, sparse, Value};
use archrust_protocol::schema::{main, mini, Schema};

/// Opérations AgpmItem de l'inventaire.
pub mod op {
    pub const UPDATE: u8 = 2;
    pub const USE: u8 = 3;
    /// Confirmation d'utilisation (serveur → client).
    pub const USE_RESULT: u8 = 4;
    /// Retour au point mémorisé par un parchemin aller-retour (client → serveur).
    pub const USE_RETURN: u8 = 6;
    /// Point de retour mémorisé (serveur → client).
    pub const RETURN_ENABLED: u8 = 9;
    /// Retour désactivé (serveur → client).
    pub const RETURN_DISABLED: u8 = 10;
    pub const SPLIT: u8 = 17;
    pub const DESTROY: u8 = 21;
}

/// Statuts AgpmItem d'un objet du joueur.
pub mod status {
    pub const INVENTORY: i8 = 2;
    pub const EQUIP: i8 = 3;
}

/// Case de l'inventaire : sac (0 à 4), ligne, colonne.
pub type Grid = [i16; 3];

/// Demandes d'objets du moteur.
#[derive(Debug, Clone, PartialEq)]
pub enum ItemCommand {
    /// Porter un objet de l'inventaire.
    Equip(i32),
    /// Ranger un objet porté, dans la case donnée ou à la place choisie par le serveur.
    Unequip { iid: i32, grid: Option<Grid> },
    /// Déplacer un objet de l'inventaire (sur une pile du même objet : les réunir).
    Move { iid: i32, grid: Grid },
    /// Utiliser un objet (potion, parchemin).
    Use(i32),
    /// Séparer `count` exemplaires d'une pile vers une case libre.
    Split { iid: i32, count: i32, grid: Grid },
    /// Détruire un objet de l'inventaire.
    Destroy(i32),
    /// Revenir au point mémorisé par un parchemin aller-retour.
    Return,
}

fn grid_value(grid: Grid) -> Value {
    let values: Vec<Option<Value>> = grid.iter().map(|v| Some(Value::I16(*v))).collect();
    Value::Packet(build_mini(mini::ITEM_GRID, &values).unwrap_or_default())
}

/// Paquet AGPMITEM d'une demande d'objet pour le personnage `me`.
pub fn command_packet(cmd: &ItemCommand, me: i32) -> (&'static Schema, Vec<Option<Value>>) {
    let s = main::ITEM;
    let values = match cmd {
        ItemCommand::Equip(iid) => sparse(
            s,
            &[(0, Value::U8(op::UPDATE)), (1, Value::I8(status::EQUIP)), (2, Value::I32(*iid)), (4, Value::I32(me))],
        ),
        ItemCommand::Unequip { iid, grid } => {
            let mut v = vec![(0, Value::U8(op::UPDATE)), (1, Value::I8(status::INVENTORY)), (2, Value::I32(*iid)), (4, Value::I32(me))];
            if let Some(g) = grid {
                v.push((7, grid_value(*g)));
            }
            sparse(s, &v)
        }
        ItemCommand::Move { iid, grid } => sparse(
            s,
            &[
                (0, Value::U8(op::UPDATE)),
                (1, Value::I8(status::INVENTORY)),
                (2, Value::I32(*iid)),
                (4, Value::I32(me)),
                (7, grid_value(*grid)),
            ],
        ),
        ItemCommand::Use(iid) => sparse(s, &[(0, Value::U8(op::USE)), (2, Value::I32(*iid)), (4, Value::I32(me))]),
        ItemCommand::Split { iid, count, grid } => sparse(
            s,
            &[
                (0, Value::U8(op::SPLIT)),
                (2, Value::I32(*iid)),
                (4, Value::I32(me)),
                (5, Value::I32(*count)),
                (7, grid_value(*grid)),
            ],
        ),
        ItemCommand::Destroy(iid) => sparse(s, &[(0, Value::U8(op::DESTROY)), (2, Value::I32(*iid)), (4, Value::I32(me))]),
        ItemCommand::Return => sparse(s, &[(0, Value::U8(op::USE_RETURN)), (4, Value::I32(me))]),
    };
    (s, values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use archrust_protocol::packet::decode_mini;

    fn int(values: &[Option<Value>], i: usize) -> Option<i64> {
        values.get(i).and_then(Option::as_ref).and_then(Value::as_i64)
    }

    #[test]
    fn demandes_d_objets() {
        let (_, v) = command_packet(&ItemCommand::Equip(42), 7);
        assert_eq!((int(&v, 0), int(&v, 1), int(&v, 2), int(&v, 4)), (Some(2), Some(3), Some(42), Some(7)));

        let (_, v) = command_packet(&ItemCommand::Move { iid: 42, grid: [1, 2, 3] }, 7);
        assert_eq!(int(&v, 1), Some(2));
        let Some(Value::Packet(bytes)) = v[7].as_ref() else { panic!("case absente") };
        let (_, g) = decode_mini(mini::ITEM_GRID, bytes).unwrap();
        assert_eq!((int(&g, 0), int(&g, 1), int(&g, 2)), (Some(1), Some(2), Some(3)));

        let (_, v) = command_packet(&ItemCommand::Unequip { iid: 42, grid: None }, 7);
        assert!(v[7].is_none(), "sans case : place choisie par le serveur");

        let (_, v) = command_packet(&ItemCommand::Split { iid: 42, count: 5, grid: [0, 4, 4] }, 7);
        assert_eq!((int(&v, 0), int(&v, 5)), (Some(17), Some(5)));
        assert_eq!(int(&command_packet(&ItemCommand::Use(42), 7).1, 0), Some(3));
        assert_eq!(int(&command_packet(&ItemCommand::Destroy(42), 7).1, 0), Some(21));
        assert_eq!(int(&command_packet(&ItemCommand::Return, 7).1, 0), Some(6));
    }
}
