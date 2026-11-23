//! `archcore sim shutdown` : arrêt propre du serveur (world/shutdown.rs). **Le serveur s'arrête** : à lancer contre
//! une instance de test dont `World.ExitOnRestart` vaut 1.
//!
//! GmTest (MJ) et SimA au nord d'Anchorville (`/move` enregistre ce départ). SimA court jusqu'à un point tiré au
//! hasard : sa position n'est pas encore en base (sauvegarde automatique toutes les 60 s). GmTest annonce un redémarrage dans 2 s : les deux
//! reçoivent « Redémarrage du serveur. » puis « Arrêt du serveur. », sans annulation d'échange (aucun en cours), leurs
//! connexions sont fermées, et la base porte
//! la nouvelle position de SimA. Une nouvelle connexion est ensuite refusée (serveur arrêté).

use super::{enter_world, step, Recv};
use archcore_net::schema::{self, chat_type};
use archcore_net::{Fields, Vec3};
use std::time::Duration;

/// Nord d'Anchorville, région 0.
const SPOT: Vec3 = Vec3 { x: -466_468.0, y: 4_700.0, z: -50_010.0 };

fn notice(text: &'static str) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| q.ptype == schema::CHATTING.id && q.fields().text(6).is_some_and(|t| t.contains(text))
}

pub async fn run(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let db = crate::db::Databases::new(&cfg.database);
    let (mv, ch) = (&schema::OPT_CHAR_MOVE, &schema::CHATTING);
    let id_b = super::character_id(&db, cb).await?;

    step(&format!("{ca} (MJ) et {cb} au nord d'Anchorville"));
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    let mut b = enter_world(&cfg, ub, pb, cb, "B-").await?;
    let place = |cid: u32| Fields::new(mv).with(0, cid).with(1, SPOT).with(2, SPOT).with(5, 0x04).with(6, 0);
    a.world.send(mv, place(a.cid)).await?;
    // `/move` du MJ enregistre la position de départ ; la marche qui suit n'est qu'en mémoire.
    super::gm_place(&mut a, cb, SPOT).await?;
    let jitter = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.subsec_millis() % 200) as f32 + 100.0;
    let target = Vec3 { x: SPOT.x + jitter, y: SPOT.y, z: SPOT.z - jitter };
    b.world.send(mv, Fields::new(mv).with(0, b.cid).with(1, SPOT).with(2, target).with(5, 0x10).with(6, 0)).await?;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let before = db.characters.first("SELECT pos_x, pos_z FROM characters WHERE id = ?", (id_b,)).await?.ok_or_else(|| anyhow::anyhow!("{cb} absent"))?;
    let (bx, bz): (f64, f64) = (crate::db::col(&before, "pos_x"), crate::db::col(&before, "pos_z"));
    anyhow::ensure!((bx - target.x as f64).abs() > 1.0 || (bz - target.z as f64).abs() > 1.0, "position déjà en base avant l'arrêt : {bx}, {bz}");
    println!("  ✓ {cb} a couru jusqu'en {:.0}, {:.0} ; en base : {bx:.0}, {bz:.0}", target.x, target.z);

    step("redémarrage annoncé dans 2 s (World.ExitOnRestart = 1)");
    tokio::time::sleep(Duration::from_millis(150)).await;
    a.world.send(ch, Fields::new(ch).with(0, 0).with(1, a.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252("/announce_restart 0 2"))).await?;
    for w in [&mut a, &mut b] {
        w.world.expect_where(notice("Redémarrage du serveur."), 6000, "annonce du redémarrage").await?;
        w.world.expect_where(notice("Arrêt du serveur."), 4000, "annonce de l'arrêt").await?;
    }
    println!("  ✓ « Redémarrage du serveur. » puis « Arrêt du serveur. » chez les deux");
    // Sans échange en cours, pas d'annulation (op 5 d'AGPMPRIVATETRADE) : le client afficherait « The trade has
    // been cancelled ».
    for (w, who) in [(&mut a, ca), (&mut b, cb)] {
        let trade = w.world.expect_where(|q| q.is(&schema::PRIVATE_TRADE, Some(5)), 1500, "annulation d'échange").await;
        anyhow::ensure!(trade.is_err(), "{who} reçoit une annulation d'échange sans échange en cours");
    }
    println!("  ✓ aucune annulation d'échange");
    for (w, who) in [(&mut a, ca), (&mut b, cb)] {
        anyhow::ensure!(w.world.closed_within(5000).await, "connexion de {who} restée ouverte");
    }
    println!("  ✓ connexions fermées");

    step("position enregistrée par l'arrêt");
    let after = db.characters.first("SELECT pos_x, pos_z FROM characters WHERE id = ?", (id_b,)).await?.ok_or_else(|| anyhow::anyhow!("{cb} absent"))?;
    let (x, z): (f64, f64) = (crate::db::col(&after, "pos_x"), crate::db::col(&after, "pos_z"));
    anyhow::ensure!((x - target.x as f64).abs() <= 1.0 && (z - target.z as f64).abs() <= 1.0, "position en base {x}, {z} au lieu de {}, {}", target.x, target.z);
    println!("  ✓ {cb} en {x:.0}, {z:.0}");

    step("serveur arrêté");
    tokio::time::sleep(Duration::from_secs(2)).await;
    anyhow::ensure!(enter_world(&cfg, ua, pa, ca, "C-").await.is_err(), "nouvelle connexion acceptée après l'arrêt");
    println!("  ✓ plus de connexion possible");
    db.close().await;
    Ok(())
}
