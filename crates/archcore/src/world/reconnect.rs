//! Reconnexion automatique (AGPMRECONNECT, 0x63), comme AgcmReconnect d'Archonia.
//!
//! ```text
//!   Entrée en jeu       ← op 2 (CID, clé de reconnexion au champ 5, accord 0) : le client garde la clé.
//!   Coupure             le client affiche UI_ReconnectHold (« ReconnectingServerShow ») et retente toutes
//!                       les 5 s, pendant 60 s, la connexion au serveur de jeu ; le personnage reste dans le
//!                       monde pendant ce délai (`WINDOW`).
//!   Nouvelle connexion  client → op 1 (CID, nom, compte, mot de passe, clé) au lieu de l'entrée normale
//!                       ← op 2 (CID, nouvelle clé, accord 1) : le client reprend la partie sur la nouvelle
//!                       connexion (FUN_14063bd10) ; le serveur lui renvoie ce qu'il voit.
//!   60 s sans reprise   le personnage quitte le monde (position enregistrée) comme à une déconnexion.
//! ```
//! La connexion qui reprend un personnage prend son identifiant de session : le reste du serveur ne voit
//! pas le changement. ArchCore vérifie la clé (unique, tirée à chaque entrée et à chaque reprise), le CID, le
//! nom et le compte ; le mot de passe envoyé par le client n'est pas contrôlé.

use super::items::item_remove;
use super::{character_remove_packet, send_nearby_npcs, Player, WorldCtx, WorldState};
use crate::character::factors_mini;
use crate::net::SessionHandle;
use crate::{info, warn};
use archcore_net::{schema, Fields, Packet};
use rand::Rng;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Délai pendant lequel un personnage déconnecté attend sa reconnexion (celui du client : 60 s).
pub const WINDOW: Duration = Duration::from_secs(60);

pub mod op {
    pub const REQUEST: i64 = 1;
    pub const RESULT: i64 = 2;
}

/// Lien d'un joueur avec sa connexion.
#[derive(Clone, Copy, Debug, Default)]
pub struct Link {
    /// Clé attendue d'une reconnexion (0 : aucune annoncée).
    pub key: i32,
    /// Déconnecté, en attente de reconnexion jusqu'à cet instant.
    pub detached_until: Option<Instant>,
}

fn new_key() -> i32 {
    rand::thread_rng().gen_range(1..i32::MAX)
}

fn result(cid: u32, key: i32, accepted: bool) -> Fields {
    let s = &schema::RECONNECT;
    Fields::new(s).with(0, op::RESULT).with(1, cid).with(5, key).with(6, accepted as i64)
}

/// Fin de l'entrée en jeu : la clé de reconnexion est annoncée au client.
pub fn on_enter(st: &mut WorldState, sid: u32) {
    let Some(p) = st.players.get_mut(&sid) else { return };
    p.link = Link { key: new_key(), detached_until: None };
    p.send(&schema::RECONNECT, result(p.ch.id, p.link.key, false));
}

/// Connexion perdue : un joueur en jeu qui a reçu sa clé reste dans le monde, immobile, en attendant sa
/// reconnexion. Rend `false` s'il doit quitter le monde tout de suite.
pub fn detach(st: &mut WorldState, sid: u32, now: Instant) -> bool {
    let Some(p) = st.players.get_mut(&sid) else { return false };
    if !p.in_game || p.link.key == 0 {
        return false;
    }
    p.link.detached_until = Some(now + WINDOW);
    p.attack = None;
    p.portal = None;
    let (name, moving) = (p.ch.name.clone(), p.dest.is_some());
    super::trade::cancel(st, sid);
    if moving {
        super::presence::stop(st, sid);
    }
    info!("[reconnexion] {name} déconnecté : attend sa reconnexion {} s", WINDOW.as_secs());
    true
}

/// Joueurs dont le délai de reconnexion est écoulé.
pub fn expired(st: &WorldState, now: Instant) -> Vec<u32> {
    st.players.iter().filter(|(_, p)| p.link.detached_until.is_some_and(|t| t <= now)).map(|(sid, _)| *sid).collect()
}

/// Paquet 0x63 reçu. Pour une demande de reconnexion valable, rend la session du joueur repris : la
/// connexion la prend à son compte. Une demande refusée ferme la connexion.
pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, s: &SessionHandle, p: &Packet) -> anyhow::Result<Option<u32>> {
    if p.int(0) != Some(op::REQUEST) {
        crate::debug!("[reconnexion] #{} AGPMRECONNECT op {:?} (ignoré)", s.id(), p.int(0));
        return Ok(None);
    }
    let cid = p.int(1).unwrap_or(0) as u32;
    let (name, account, key) = (p.text(2).unwrap_or_default(), p.text(3).unwrap_or_default(), p.int(5).unwrap_or(0) as i32);
    let sid = st.by_cid.get(&cid).copied();
    let Some((sid, player)) = sid.and_then(|sid| st.players.get(&sid).map(|pl| (sid, pl))) else {
        warn!("[reconnexion] #{} personnage {cid} ({name:?}) absent du monde", s.id());
        s.close();
        return Ok(None);
    };
    if key == 0 || key != player.link.key || !player.ch.name.eq_ignore_ascii_case(name.trim()) || !player.account.username.eq_ignore_ascii_case(account.trim()) {
        warn!("[reconnexion] #{} reprise de {} refusée (clé, nom ou compte)", s.id(), player.ch.name);
        s.close();
        return Ok(None);
    }
    // Ancienne connexion encore ouverte (coupure pas encore vue) : elle est fermée, ses paquets ignorés.
    let old = player.sess.clone();
    if old.id() != s.id() {
        old.close();
    }
    let p = st.players.get_mut(&sid).unwrap();
    p.sess = s.clone();
    p.link = Link { key: new_key(), detached_until: None };
    p.send(&schema::RECONNECT, result(cid, p.link.key, true));
    info!("[reconnexion] #{} reprend {} (session #{sid})", s.id(), p.ch.name);
    resync(ctx, st, sid);
    Ok(Some(sid))
}

/// Renvoie au client repris ce qu'il voit : tout ce qu'il avait est retiré puis montré de nouveau (il a pu
/// manquer des apparitions et des départs pendant la coupure), avec sa position et ses facteurs.
pub fn resync(ctx: &WorldCtx, st: &mut WorldState, sid: u32) {
    let Some(p) = st.players.get_mut(&sid) else { return };
    let cids: Vec<u32> = p.seen_npcs.drain().chain(p.seen_monsters.drain()).chain(p.seen_players.drain()).collect();
    for cid in cids {
        p.send_raw(character_remove_packet(cid));
    }
    for iid in p.seen_items.drain().collect::<Vec<_>>() {
        p.send_raw(item_remove(iid, None));
    }
    let pos = p.ch.pos();
    send_own(p);
    send_nearby_npcs(ctx, st, sid, pos);
    super::presence::refresh(st, sid);
}

/// Position (arrêt synchronisé) et facteurs du joueur, à lui-même.
fn send_own(p: &Player) {
    let pos = p.ch.pos();
    let mv = &schema::OPT_CHAR_MOVE;
    p.send(mv, Fields::new(mv).with(0, p.ch.id).with(1, pos).with(2, pos).with(5, super::MOVE_STOP_SYNC).with(6, p.direction));
    let c = &schema::CHARACTER;
    p.send(c, Fields::new(c).with(0, 1).with(1, p.ch.id).with(7, factors_mini(&p.factors)));
}

/// Fait quitter le monde aux joueurs dont le délai de reconnexion est écoulé (toutes les secondes).
pub fn spawn_ticker(world: Arc<super::World>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tick.tick().await;
            let mut st = world.state.lock().await;
            for sid in expired(&st, Instant::now()) {
                if let Some(p) = st.players.get(&sid) {
                    info!("[reconnexion] {} ne s'est pas reconnecté : il quitte le monde", p.ch.name);
                }
                super::remove_player(&world.ctx, &mut st, sid).await;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reponse_du_serveur() {
        let f = result(7, 1234, true);
        assert_eq!((f.int(0), f.int(1), f.int(5), f.int(6)), (Some(op::RESULT), Some(7), Some(1234), Some(1)));
        let k = new_key();
        assert!(k > 0);
    }
}
