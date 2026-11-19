//! AgpmCasper (0x19, client → serveur) : arrêt du serveur demandé depuis la console de développement de Webzen.
//!
//! Paquet `{op INT8, CID INT32}`, ops 0 à 3, sans fenêtre ni abonné dans les deux clients : les ops que recevrait le
//! client déclenchent des rappels que personne n'écoute. Le seul envoi atteignable est l'**op 3**, sans CID : la
//! fonction de script `KillGameServer1()` que le client de Webzen enregistre (AgcmCharacter 0x5ab390) dans sa console
//! de développement Lua (AgcmUIConsole), avec `ShowMyCID`, `ShowRegionInfo`, `ToggleStrafeMove` ; envoi 0x5a2590 →
//! 0x56a3c0. Archonia garde la fonction d'envoi (0x1404b0ea0) sans plus rien qui l'appelle.
//!
//! Choix d'ArchCore : l'op 3 d'un compte MJ vaut la commande de discussion `/announce_restart 0 10` (même niveau, même
//! journal, réponse en message système) : redémarrage annoncé dans 10 s, qui n'arrête le processus que si
//! `World.ExitOnRestart` ; `/announce_restart 0` l'annule. Le reste (joueur sans droits, ops 0 à 2) est ignoré.

use super::gm;
use super::{send_system, World, WorldState};
use crate::{debug, info};
use archcore_net::Packet;
use std::sync::Arc;

/// « KillGameServer » de la console de développement.
pub const KILL_GAME_SERVER: i64 = 3;

/// Commande de discussion équivalente à l'op 3.
pub const KILL_COMMAND: &str = "/announce_restart 0 10";

pub async fn on_packet(world: &Arc<World>, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some(player) = st.players.get(&sid) else { return Ok(()) };
    let opc = p.int(0).unwrap_or(-1);
    if opc != KILL_GAME_SERVER || player.gm_level <= 0 {
        debug!("[world] #{sid} AGPMCASPER op {opc} de {} ignorée", player.ch.name);
        return Ok(());
    }
    info!("[mj] {} : KillGameServer (AgpmCasper op 3) → {KILL_COMMAND}", player.ch.name);
    if let Some(Some(reply)) = gm::execute_quiet(world, st, sid, KILL_COMMAND).await? {
        send_system(st, sid, &reply);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn commande_equivalente() {
        let (name, args) = super::gm::parse_command(super::KILL_COMMAND).unwrap().unwrap();
        assert_eq!((name.as_str(), args), ("announce_restart", vec!["0".to_string(), "10".to_string()]));
        assert!(super::gm::command(&name).is_some());
    }
}
