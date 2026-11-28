//! Verrou des objets (fenêtre `UI_ItemLock` d'Archonia, bouton cadenas `CONTROL_LockButton` de l'inventaire) : ajout
//! d'Archonia gardé (absent du client de Webzen).
//!
//! ```text
//!   Client → serveur : AGPMITEM op 36, IID (champ 2), CID (4), champ 19 = 0x2000 (Archonia FUN_1407c1a10 puis
//!     FUN_1401ca610, « Confirm » de la fenêtre) pour un objet de l'inventaire (2), du sac du familier (13), de
//!     l'inventaire étendu (15) ou de l'inventaire Cash (51). Chaque demande verrouille ou déverrouille.
//!   Serveur → client : op 36, IID, CID, champ 19 = drapeaux de l'objet (0x2000 verrouillé, 0 sinon) ; à l'entrée en
//!     jeu, après les objets, une op 36 par objet verrouillé.
//! ```
//! Le client montre « Item_Locked_Type » (« Locked ») dans l'info-bulle d'un objet au bit 0x2000. Un objet verrouillé
//! ne se vend pas au marchand (« Sell All » le laisse : texte de la confirmation d'Archonia), ne se détruit pas, ne
//! s'échange pas, ne se met pas en vente et ne part pas par courrier (règles d'ArchCore, déduites de ce texte). En
//! base : `character_items.locked`. Choix d'ArchCore : le client d'Archonia demande peut-être un ensemble de drapeaux ;
//! ArchCore traite la demande comme une bascule du seul verrou et renvoie les drapeaux complets.

use super::items::{item_op, status_flag_packet};
use super::{send_system, WorldCtx, WorldState};
use crate::{debug, info};
use archcore_net::Packet;

/// L'objet `iid` du joueur `sid` est verrouillé.
pub fn is_locked(st: &WorldState, sid: u32, iid: u32) -> bool {
    st.players.get(&sid).is_some_and(|p| p.inventory.locked.contains(&iid))
}

/// Refus d'une action sur un objet verrouillé : message au joueur, vrai si refusé.
pub fn refuse_locked(st: &WorldState, sid: u32, iid: u32, what: &str) -> bool {
    if !is_locked(st, sid, iid) {
        return false;
    }
    send_system(st, sid, &format!("Objet verrouillé : {what} impossible (déverrouillez-le d'abord avec le cadenas de l'inventaire)."));
    true
}

/// Op 36 à l'entrée en jeu : les objets verrouillés.
pub fn on_enter(st: &WorldState, sid: u32) {
    let Some(p) = st.players.get(&sid) else { return };
    for iid in &p.inventory.locked {
        p.send_raw(status_flag_packet(*iid, p.ch.id, true));
    }
}

/// AGPMITEM op 36 : verrouille ou déverrouille un objet de l'inventaire, du sac du familier, de l'inventaire étendu
/// ou de l'inventaire Cash.
pub async fn on_request(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    debug_assert_eq!(p.int(0), Some(item_op::UPDATE_STATUS_FLAG));
    let iid = p.int(2).unwrap_or(0) as u32;
    let Some(player) = st.players.get_mut(&sid) else { return Ok(()) };
    let Some(item) = player.inventory.get_usable(iid).cloned() else {
        debug!("[verrou] {} : objet {iid} hors de l'inventaire, ignoré", player.ch.name);
        return Ok(());
    };
    let locked = !player.inventory.locked.contains(&iid);
    ctx.db.characters.exec("UPDATE character_items SET locked = ? WHERE id = ?", (i64::from(locked), iid)).await?;
    if locked {
        player.inventory.locked.insert(iid);
    } else {
        player.inventory.locked.remove(&iid);
    }
    player.send_raw(status_flag_packet(iid, player.ch.id, locked));
    let label = ctx.catalog.get(item.tid).map(|d| d.name.clone()).unwrap_or_default();
    info!("[verrou] {} {} {label} ({iid})", player.ch.name, if locked { "verrouille" } else { "déverrouille" });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use archcore_net::{decode_packet, schema};

    #[test]
    fn paquet_du_verrou() {
        let p = decode_packet(&schema::ITEM, &status_flag_packet(77, 9, true)).unwrap();
        assert_eq!((p.int(0), p.int(2), p.int(4), p.int(19)), (Some(36), Some(77), Some(9), Some(0x2000)));
        let p = decode_packet(&schema::ITEM, &status_flag_packet(77, 9, false)).unwrap();
        assert_eq!(p.int(19), Some(0));
    }
}
