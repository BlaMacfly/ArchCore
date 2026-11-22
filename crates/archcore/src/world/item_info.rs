//! Renseignements sur un objet (AGPMITEM op 31 → op 32) : l'infobulle d'un objet que le client ne détient pas.
//!
//! Le client demande un objet par **op 31** (client → serveur) : champ 2 IID, champ 3 TID, champ 4 CID du demandeur,
//! champ 12 genre de la demande. Deux émetteurs dans Archonia, un seul chez Webzen :
//! - lien d'objet cliqué dans la discussion (genre **8**) : IID de l'objet, TID 0 ; le client garde les objets déjà
//!   reçus et ne redemande pas (Archonia FUN_140511500 → FUN_1407a5f60 → FUN_140649610 → FUN_1401baf00 ; Webzen
//!   0x70c7f6, commandes de `ChatCmd.xml`, → 0x6d4620 → 0x6ebd20 → 0x53ac80) ;
//! - bouton d'objet d'une fenêtre d'Archonia (`Quest_ActiveBox.png`, FUN_14088c530, genre **4**) : IID 0, TID seul.
//!
//! Le serveur répond par **op 32** (serveur → client), que le client fait passer au rappel 106 d'AgpmItem
//! (FUN_1401c5fa0) : l'interface des objets (AgcmUIItem) en garde l'infobulle et l'affiche. L'op 31 renvoyée par le
//! serveur n'aurait aucun effet : son rappel (105) n'a aucun abonné, ni dans Archonia ni chez Webzen. Champs de l'op 32
//! lus par le client (FUN_1401b0e10, cas 32) : 2 IID, 3 TID, 5 quantité, 10 facteurs, 11 facteurs %, 12 genre
//! (renvoyé tel quel), 14 restrictions, 17, 19 drapeaux, 20 options, 21, 23 Cash, 24, 25 : les champs d'un AGPMITEM
//! op 0 sans sa place (6 à 8), sans statut ni propriétaire. Choix d'ArchCore : les champs qu'il envoie déjà à l'op 0
//! (quantité, durabilité entamée, options). Le champ 25 est un ajout d'Archonia (texte `%d,%llu;` × 10 puis
//! `[liste]`, FUN_1401a7fd0 / FUN_1401a81c0, rangé à `AgpdItem+0x534…0x540` et dans deux tableaux de l'état de
//! conversion), pas les sockets d'EP8 : ArchCore ne l'envoie pas. Renforcement, sockets et pierres serties suivent
//! l'op 32 par un AGPMITEMCONVERT op 0, comme après l'op 0 d'un objet : l'op 32 a mis l'objet dans la liste du client
//! (FUN_1401aeba0, FUN_1401af0a0), l'état de conversion s'y rattache par son IID. Demande par TID seul : TID et genre,
//! le client prenant le reste dans son modèle. Objet introuvable (joueur parti, objet vendu) : pas de réponse, comme
//! pour un lien périmé.

use super::items::{durability_factors, InventoryItem};
use super::WorldState;
use crate::debug;
use archcore_net::schema;
use archcore_net::{build_packet, Fields, Packet};

/// Demande de renseignements (client → serveur).
pub const REQUEST_ITEM_INFO: i64 = 31;
/// Renseignements sur un objet (serveur → client).
pub const ITEM_INFO: i64 = 32;

/// Genres de demande du champ 12 (renvoyés tels quels).
pub mod kind {
    /// Lien d'objet de la discussion.
    pub const CHAT_LINK: i64 = 8;
    /// Bouton d'objet d'une fenêtre (TID seul).
    pub const TEMPLATE: i64 = 4;
}

/// Objet `iid` d'un joueur en jeu (inventaire, équipement, inventaire Cash, sac du familier ou costume) et CID de ce
/// joueur.
fn find_item(st: &WorldState, iid: u32) -> Option<(InventoryItem, u32)> {
    st.players.values().filter(|p| p.in_game).find_map(|p| {
        let inv = &p.inventory;
        inv.items
            .get(&iid)
            .or_else(|| inv.cash.get(&iid))
            .or_else(|| inv.sub.get(&iid))
            .or_else(|| inv.equipment.values().find(|i| i.iid == iid))
            .or_else(|| inv.avatar.iter().find(|i| i.iid == iid))
            .map(|i| (i.clone(), p.ch.id))
    })
}

/// AGPMITEM op 32 : l'objet `item` (champs de l'op 0 sans place ni propriétaire), ou le seul modèle `tid`.
pub fn info_packet(item: Option<&InventoryItem>, tid: u32, kind: i64) -> Vec<u8> {
    let s = &schema::ITEM;
    let f = Fields::new(s).with(0, ITEM_INFO).with(12, kind);
    let f = match item {
        Some(i) => {
            let f = f.with(2, i.iid).with(3, i.tid).with(5, i.count);
            let f = f.with_opt(10, (i.durability >= 0).then(|| durability_factors(i.durability)));
            f.with_opt(20, (!i.options.is_empty()).then(|| super::options::options_mini(&i.options)))
        }
        None => f.with(3, tid),
    };
    build_packet(s, &f)
}

/// AGPMITEM op 31 : réponse op 32 au seul demandeur.
pub fn on_request(st: &WorldState, sid: u32, p: &Packet) {
    let Some(player) = st.players.get(&sid) else { return };
    if !player.in_game {
        return;
    }
    let (iid, tid, kind) = (p.int(2).unwrap_or(0).max(0) as u32, p.int(3).unwrap_or(0).max(0) as u32, p.int(12).unwrap_or(0));
    let source = match kind {
        kind::CHAT_LINK => "lien de la discussion",
        kind::TEMPLATE => "bouton d'une fenêtre",
        _ => "genre inconnu",
    };
    debug!("[objets] #{sid} renseignements demandés (IID {iid}, TID {tid}, {source})");
    if iid != 0 {
        let Some((item, owner)) = find_item(st, iid) else {
            debug!("[objets] #{sid} renseignements sur l'objet {iid} introuvable : pas de réponse");
            return;
        };
        player.send_raw(info_packet(Some(&item), item.tid, kind));
        if let Some(conv) = convert_packet(&item, owner) {
            player.send_raw(conv);
        }
    } else if tid != 0 {
        player.send_raw(info_packet(None, tid, kind));
    }
}

/// AGPMITEMCONVERT op 0 qui suit l'op 32 : renforcement, sockets et pierres serties, si l'objet en a.
pub fn convert_packet(item: &InventoryItem, owner: u32) -> Option<Vec<u8>> {
    (!item.conv.is_plain()).then(|| super::convert::state_packet(super::convert::op::ADD, owner, item.iid, &item.conv, None, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use archcore_net::decode_packet;

    fn item() -> InventoryItem {
        InventoryItem { iid: 77, tid: 1234, count: 3, grid: [0, 1, 2], conv: Default::default(), options: vec![12, 34], durability: 40 }
    }

    #[test]
    fn renseignements_sur_un_objet() {
        let p = decode_packet(&schema::ITEM, &info_packet(Some(&item()), 1234, kind::CHAT_LINK)).unwrap();
        assert_eq!((p.int(0), p.int(2), p.int(3), p.int(5), p.int(12)), (Some(32), Some(77), Some(1234), Some(3), Some(8)));
        assert!(p.int(1).is_none() && p.int(4).is_none(), "ni statut ni propriétaire");
        assert!(p.bytes(7).is_none(), "sans case");
        assert!(p.bytes(10).is_some() && p.bytes(20).is_some(), "durabilité entamée et options");
        let mut pleine = item();
        pleine.durability = -1;
        pleine.options.clear();
        let p = decode_packet(&schema::ITEM, &info_packet(Some(&pleine), 1234, kind::CHAT_LINK)).unwrap();
        assert!(p.bytes(10).is_none() && p.bytes(20).is_none());
    }

    #[test]
    fn etat_de_conversion_apres_l_op_32() {
        assert!(convert_packet(&item(), 9).is_none(), "objet sans renforcement ni socket");
        let mut forge = item();
        forge.conv = super::super::convert::ItemConvert { level: 5, sockets: 3, filled: vec![4001, 4002] };
        let p = decode_packet(&schema::ITEM_CONVERT, &convert_packet(&forge, 9).unwrap()).unwrap();
        assert_eq!((p.int(0), p.int(1), p.int(2), p.int(5), p.int(6), p.int(7)), (Some(0), Some(9), Some(77), Some(5), Some(3), Some(2)));
        assert!(p.bytes(9).is_some(), "TID sertis");
    }

    #[test]
    fn renseignements_sur_un_modele() {
        let p = decode_packet(&schema::ITEM, &info_packet(None, 555, kind::TEMPLATE)).unwrap();
        assert_eq!((p.int(0), p.int(2), p.int(3), p.int(12)), (Some(32), None, Some(555), Some(4)));
    }
}
