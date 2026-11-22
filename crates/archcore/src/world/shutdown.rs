//! Arrêt propre du serveur monde : Ctrl+C (ce qu'envoie le lanceur ArchHome), fermeture de la console Windows, ou fin
//! du compte à rebours d'un redémarrage (`/announce_restart`, console `server shutdown|restart`) avec
//! `World.ExitOnRestart`.
//!
//! Dans l'ordre : plus aucune entrée en jeu ; annonce « Arrêt du serveur. » ; une seconde pour que les clients la
//! reçoivent ; chaque joueur quitte le monde comme à une déconnexion (échange annulé, groupe, guilde, salons, amis,
//! invocations, position enregistrée), joueurs en attente de reconnexion compris, puis sa connexion est fermée ;
//! taxes des châteaux en attente enregistrées ; journaux encore en vol attendus quelques secondes. Le serveur ferme
//! ensuite ses bases et quitte. Un second Ctrl+C pendant l'arrêt quitte aussitôt.

use super::World;
use crate::{info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

/// Délai laissé aux clients pour recevoir l'annonce avant la fermeture des connexions.
const NOTICE_DELAY: Duration = Duration::from_secs(1);
/// Attente des journaux encore en vol.
const JOURNAL_WAIT: Duration = Duration::from_secs(5);

static REQUESTED: AtomicBool = AtomicBool::new(false);

fn notify() -> &'static Notify {
    static N: std::sync::OnceLock<Notify> = std::sync::OnceLock::new();
    N.get_or_init(Notify::new)
}

/// Demande l'arrêt (fin d'un redémarrage annoncé) : le serveur le fait comme pour un Ctrl+C.
pub fn request() {
    REQUESTED.store(true, Ordering::SeqCst);
    notify().notify_one();
}

/// Attend une demande d'arrêt par [`request`].
pub async fn requested() {
    if REQUESTED.load(Ordering::SeqCst) {
        return;
    }
    notify().notified().await;
}

/// Fait quitter le monde à tous les joueurs et enregistre ce qui attend encore de l'être. Renvoie le nombre de joueurs
/// sortis.
pub async fn run(world: &Arc<World>) -> usize {
    let anyone = {
        let mut st = world.state.lock().await;
        st.closing = true;
        if let Some(r) = st.restart.take() {
            r.task.abort();
        }
        if !st.players.is_empty() {
            super::broadcast_notice(&st, "Arrêt du serveur.");
        }
        !st.players.is_empty()
    };
    if anyone {
        tokio::time::sleep(NOTICE_DELAY).await;
    }
    let mut st = world.state.lock().await;
    let sids = st.sids();
    for &sid in &sids {
        let sess = st.players.get(&sid).map(|p| p.sess.clone());
        super::remove_player(&world.ctx, &mut st, sid).await;
        if let Some(sess) = sess {
            st.evicted.insert(sid);
            sess.close();
        }
    }
    if let Err(e) = super::tax::flush(&world.ctx, &mut st).await {
        warn!("[arrêt] taxes des châteaux : {e:#}");
    }
    drop(st);
    if !crate::journal::drain(JOURNAL_WAIT).await {
        warn!("[arrêt] journaux encore en cours d'écriture après {} s", JOURNAL_WAIT.as_secs());
    }
    info!("[arrêt] {} joueur(s) sorti(s) du monde, données enregistrées", sids.len());
    sids.len()
}
