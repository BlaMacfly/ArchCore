//! Drapeaux d'options du personnage (AgpmCharacter op 0x11, champ 22) : refus d'échange, d'invitation de
//! groupe, de guilde… cochés dans l'onglet « Fonction » des options. Le client les envoie à l'entrée en jeu
//! et à chaque case ; le serveur les garde (`characters.option_flag`), les renvoie par la même op, et arrête
//! à la source ce qui vise un joueur qui refuse. L'auteur reçoit le message d'Archonia : invitation de groupe,
//! op 14 de gestion (« Option_Party_Off_User ») ; invitation de guilde, message système 42
//! (« Option_Guild_Off_User ») ; demande d'échange, op 25 (« Option_Trade_Off_User »).

use super::WorldState;
use crate::info;
use archcore_net::{schema, Fields, Packet};

/// Opération AgpmCharacter « drapeaux d'options ».
pub const UPDATE_OPTION_FLAG: i64 = 0x11;

/// Bits du champ 22 appliqués par le serveur. Les autres attendent leur système : 0x04 combat de guilde,
/// 0x08 combat, 0x40 alliance ou hostilité de guilde.
pub const REFUSE_TRADE: u32 = 0x01;
pub const REFUSE_PARTY_IN: u32 = 0x02;
pub const REFUSE_GUILD_IN: u32 = 0x10;
/// Demandes d'ami (« OP_RefuseBuddy ») : non relayées, le demandeur reçoit un refus (AGPMBUDDY op 3, buddy.rs).
pub const REFUSE_BUDDY: u32 = 0x80;

/// Le joueur `sid` refuse-t-il ce que `bit` désigne ?
pub fn refuses(st: &WorldState, sid: u32, bit: u32) -> bool {
    st.players.get(&sid).is_some_and(|p| p.ch.option_flag & bit != 0)
}

/// Op 0x11 vers le client : drapeaux du personnage.
pub fn flags_fields(cid: u32, flags: u32) -> Fields {
    let c = &schema::CHARACTER;
    Fields::new(c).with(0, UPDATE_OPTION_FLAG).with(1, cid).with(22, flags as i64)
}

/// Op 0x11 reçue : drapeaux gardés et renvoyés au client.
pub async fn on_packet(ctx: &super::WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some(flags) = p.int(22).map(|f| f as u32) else { return Ok(()) };
    let Some(player) = st.players.get_mut(&sid) else { return Ok(()) };
    if p.int(1).is_some_and(|c| c != player.ch.id as i64) {
        return Ok(());
    }
    let changed = player.ch.option_flag != flags;
    player.ch.option_flag = flags;
    let (cid, name) = (player.ch.id, player.ch.name.clone());
    player.send(&schema::CHARACTER, flags_fields(cid, flags));
    if changed {
        info!("[options] {name} : drapeaux 0x{flags:x}");
        ctx.db.characters.exec("UPDATE characters SET option_flag = ? WHERE id = ?", (flags, cid)).await?;
    }
    Ok(())
}
