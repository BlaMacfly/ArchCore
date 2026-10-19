//! Protocole réseau ArchLord (client 64 bits), tel que reconstruit pour ArchCore :
//! paquets AuPacket pilotés par schémas, Blowfish du jeu, trames chiffrées, RC4 CryptoAPI.

pub mod blowfish;
mod blowfish_tables;
pub mod ext_text;
pub mod framing;
pub mod packet;
pub mod rc4;
pub mod schema;
pub mod text;
#[cfg(test)]
mod test_vectors;

pub use packet::{
    build_mini, build_packet, decode_mini, decode_packet, hex_dump, parse_header, Field, Fields, Header, Packet, Schema,
    Ty, Value, Vec3,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use schema::{mini, *};

    #[test]
    fn mini_dans_paquet_aller_retour() {
        let base = build_mini(&mini::EVENT_BASE, &Fields::new(&mini::EVENT_BASE).with(0, 3u8).with(1, 0x4000_0003u32).with(3, 21));
        let p = build_packet(&EVENT_NPC_DIALOG, &Fields::new(&EVENT_NPC_DIALOG).with(0, 1u8).with(1, base).with(2, 7u32));
        let d = decode_packet(&EVENT_NPC_DIALOG, &p).unwrap();
        assert_eq!(d.int(0), Some(1));
        assert_eq!(d.int(2), Some(7));
        let b = decode_mini(&mini::EVENT_BASE, d.bytes(1).unwrap()).unwrap();
        assert_eq!(b.int(1), Some(0x4000_0003));
        assert_eq!(b.int(3), Some(21));
        assert_eq!(b.get(2), None);
    }

    #[test]
    fn paquet_objet_identique_a_node() {
        // Octets produits par le serveur Node pour un ajout d'objet (IID 3, TID 190, 5 exemplaires).
        let grid = build_mini(&mini::ITEM_GRID, &Fields::new(&mini::ITEM_GRID).with(0, 0).with(1, 0).with(2, 0));
        let p = build_packet(
            &ITEM,
            &Fields::new(&ITEM).with(0, 0).with(1, 2).with(2, 3).with(3, 190).with(4, 1).with(5, 5).with(7, grid),
        );
        let expected = "d6350000000400000000000000000000000000bf000000000203000000be000000010000000500000007000000070000000000006b";
        assert_eq!(p.iter().map(|b| format!("{b:02x}")).collect::<String>(), expected);
    }

    #[test]
    fn bloc_memoire_vide_sans_drapeau() {
        let p = build_packet(&STARTUP_ENCRYPTION, &Fields::new(&STARTUP_ENCRYPTION).with(0, 6).with(1, Vec::<u8>::new()));
        let d = decode_packet(&STARTUP_ENCRYPTION, &p).unwrap();
        assert_eq!(d.flags, 1);
    }

    #[test]
    fn bloc_memoire_vide_suivi_d_un_champ() {
        // Le bloc vide (champ 7) ne doit rien écrire : le champ 8 se relit intact.
        let p = build_packet(&OPT_VIEW, &Fields::new(&OPT_VIEW).with(0, 2).with(7, Vec::<u8>::new()).with(8, vec![1u8, 2, 3]));
        let d = decode_packet(&OPT_VIEW, &p).unwrap();
        assert_eq!((d.bytes(7), d.bytes(8)), (None, Some(&[1u8, 2, 3][..])));
    }
}
