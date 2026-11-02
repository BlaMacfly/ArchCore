//! Retour à la sélection des personnages (AgpmReturnToLogin, 0x2F, drapeaux 1) : bouton « Select Character » de
//! `UI_EXIT`. Champs : 0 opération, 1 CID, 2 compte (49), 3 groupe de serveurs (32), 4 adresse du serveur de connexion
//! (25), 5 clé.
//!
//! ```text
//!   client → monde op 1 (1 CID)              le client a vérifié lui-même le combat (5 s) et l'arène
//!   monde → client op 5 (1 CID, 4 adresse,   clé à usage unique (`account.return_key`, `ReturnToLogin.KeySeconds`) ;
//!                        5 clé)              le personnage quitte le monde et la connexion se ferme 2 s plus tard
//!            ou op 2                         refus (en combat, mort)
//!   client → connexion op 6 (2 compte, 5 clé) une fois la connexion chiffrée, au lieu de la demande de clé de hachage
//!   connexion → client AGPMLOGIN op 0         clé de hachage ; le client se reconnecte avec l'identifiant et le mot de
//!                                            passe qu'il a gardés (AGPMLOGIN op 1)
//!   connexion → client op 7                   après la connexion réussie : le client redemande la liste de ses
//!                                            personnages (AGPMLOGIN op 3) et revient à `UI_LG_CHARSELECT`
//! ```
//! Relevé dans Archonia : réception FUN_1402cf6d0 (op 2 : rappel d'échec, op 5 : clé gardée puis connexion à
//! l'adresse du champ 4, vide : l'adresse du serveur de connexion déjà connue, op 7 : AGPMLOGIN op 3 du monde choisi),
//! envoi de l'op 6 par FUN_140686fe0 dès que la connexion au serveur de connexion est chiffrée et qu'une clé est gardée.
//! Les refus du monde (en combat, mort) sont des choix d'ArchCore.

use super::{WorldCtx, WorldState};
use crate::{info, warn};
use archcore_net::schema;
use archcore_net::{build_packet, Fields, Packet};
use rand::Rng;
use std::time::{Duration, Instant};

pub mod op {
    pub const REQUEST: i64 = 1;
    pub const REQUEST_FAILED: i64 = 2;
    pub const SEND_KEY_ADDR: i64 = 5;
    pub const RECONNECT_LOGIN_SERVER: i64 = 6;
    pub const END_PROCESS: i64 = 7;
}

/// Délai entre la clé envoyée et la fermeture de la connexion au monde.
pub const CLOSE_AFTER: Duration = Duration::from_secs(2);

/// Op 5 : clé et adresse du serveur de connexion (vide : non envoyée, le client reprend la sienne).
pub fn key_packet(cid: u32, address: &str, key: u32) -> Vec<u8> {
    let s = &schema::RETURN_TO_LOGIN;
    let mut f = Fields::new(s).with(0, op::SEND_KEY_ADDR).with(1, cid).with(5, key);
    if !address.is_empty() {
        f = f.with(4, address);
    }
    build_packet(s, &f)
}

/// Op 2 : retour refusé.
pub fn failed_packet(cid: u32) -> Vec<u8> {
    let s = &schema::RETURN_TO_LOGIN;
    build_packet(s, &Fields::new(s).with(0, op::REQUEST_FAILED).with(1, cid))
}

/// Op 7 : fin du retour, envoyée par le serveur de connexion après la connexion réussie.
pub fn end_packet() -> Vec<u8> {
    let s = &schema::RETURN_TO_LOGIN;
    build_packet(s, &Fields::new(s).with(0, op::END_PROCESS))
}

/// Paquet 0x2F reçu par le monde.
pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some(player) = st.players.get(&sid).filter(|x| x.in_game) else { return Ok(()) };
    let (cid, name, account) = (player.ch.id, player.ch.name.clone(), player.account.id);
    if p.int(0) != Some(op::REQUEST) || p.int(1).is_some_and(|c| c != cid as i64) {
        warn!("[retour] #{sid} AgpmReturnToLogin op {:?} inattendu", p.int(0));
        return Ok(());
    }
    let why = if player.dead {
        Some("mort")
    } else if Instant::now() < player.timers.combat_until {
        Some("en combat")
    } else {
        None
    };
    if let Some(why) = why {
        info!("[retour] {name} : retour à la sélection refusé ({why})");
        player.send_raw(failed_packet(cid));
        return Ok(());
    }
    let w = &ctx.config.world;
    let key: u32 = rand::thread_rng().gen_range(1..i32::MAX as u32);
    ctx.db
        .auth
        .exec("UPDATE account SET return_key = ?, return_key_until = NOW() + INTERVAL ? SECOND WHERE id = ?", (key, w.return_login_key_secs, account))
        .await?;
    let sess = player.sess.clone();
    player.send_raw(key_packet(cid, &w.return_login_address, key));
    info!("[retour] {name} retourne à la sélection des personnages");
    // Le personnage quitte le monde tout de suite (position enregistrée) ; les derniers paquets de la connexion sont
    // ignorés jusqu'à sa fermeture.
    super::remove_player(ctx, st, sid).await;
    st.evicted.insert(sid);
    tokio::spawn(async move {
        tokio::time::sleep(CLOSE_AFTER).await;
        sess.close();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use archcore_net::decode_packet;

    #[test]
    fn paquets_du_retour() {
        let s = &schema::RETURN_TO_LOGIN;
        let p = decode_packet(s, &key_packet(42, "", 123_456)).unwrap();
        assert_eq!((p.int(0), p.int(1), p.int(5)), (Some(5), Some(42), Some(123_456)));
        assert!(p.get(4).is_none(), "adresse vide : non envoyée");
        let p = decode_packet(s, &key_packet(42, "127.0.0.1:11002", 7)).unwrap();
        assert_eq!(p.text(4).as_deref(), Some("127.0.0.1:11002"));
        let p = decode_packet(s, &failed_packet(42)).unwrap();
        assert_eq!((p.int(0), p.int(1)), (Some(2), Some(42)));
        let p = decode_packet(s, &end_packet()).unwrap();
        assert_eq!((p.int(0), p.int(1)), (Some(7), None));
    }
}
