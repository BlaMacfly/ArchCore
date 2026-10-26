//! `archcore sim reconnect` : reconnexion automatique (AGPMRECONNECT 0x63), comme AgcmReconnect d'Archonia.
//!
//! A reçoit sa clé à l'entrée en jeu ; B, à côté de lui, le voit. La connexion de A est coupée : B ne le
//! voit pas partir. Une nouvelle connexion au monde présente une mauvaise clé (fermée), puis la bonne : la
//! partie reprend (nouvelle clé, accord 1, vue renvoyée) et une commande passe sur la nouvelle connexion.
//! Coupé une seconde fois, A n'est pas repris : après 60 s, B le voit quitter le monde et la reprise est
//! refusée.

use super::{enter_world, step, Recv, TestClient};
use archcore_net::schema::{self, chat_type};
use archcore_net::{Fields, Vec3};
use std::time::Duration;

const STOP: i64 = 0x04;

/// Nouvelle connexion au monde (chiffrement établi), sans passer par le serveur de connexion.
async fn world_link(cfg: &crate::config::Config, label: &str) -> anyhow::Result<TestClient> {
    let mut c = TestClient::connect(label, &format!("127.0.0.1:{}", cfg.world.port)).await?;
    c.handshake().await?;
    Ok(c)
}

/// Demande de reprise (op 1) telle que l'envoie Archonia (FUN_1404b8100).
fn request(cid: u32, name: &str, account: &str, password: &str, key: i64) -> Fields {
    let r = &schema::RECONNECT;
    Fields::new(r).with(0, 1).with(1, cid).with(2, name).with(3, account).with(4, password).with(5, key).with(6, 0)
}

pub async fn run(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let (r, c, ch, mv) = (&schema::RECONNECT, &schema::CHARACTER, &schema::CHATTING, &schema::OPT_CHAR_MOVE);
    let removed = |cid: u32| move |q: &Recv| q.is(c, Some(3)) && q.int(1) == Some(cid as i64);

    step(&format!("{ca} entre en jeu : clé de reconnexion (op 2)"));
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    let key = a.world.expect(r, Some(2)).await?;
    let k1 = key.int(5).unwrap_or(0);
    anyhow::ensure!(k1 != 0 && key.int(6) == Some(0) && key.int(1) == Some(a.cid as i64), "clé {k1}, accord {:?}, CID {:?}", key.int(6), key.int(1));
    println!("  ✓ clé {k1}, accord 0");

    step(&format!("{cb} entre en jeu à côté de {ca}"));
    let mut b = enter_world(&cfg, ub, pb, cb, "B-").await?;
    let spot = Vec3 { x: b.start.x + 200.0, ..b.start };
    a.world.send(mv, Fields::new(mv).with(0, a.cid).with(1, spot).with(2, spot).with(5, STOP).with(6, 0)).await?;
    b.world.expect_where(|q| q.is(c, Some(0)) && q.int(1) == Some(a.cid as i64), 5000, &format!("{cb} voit {ca}")).await?;
    println!("  ✓ {cb} voit {ca}");

    step(&format!("coupure de {ca} : il reste dans le monde"));
    let cid = a.cid;
    drop(a);
    anyhow::ensure!(b.world.expect_where(removed(cid), 2000, "retrait").await.is_err(), "{ca} a quitté le monde dès la coupure");
    println!("  ✓ {cb} le voit toujours");

    step("mauvaise clé : connexion fermée");
    let mut bad = world_link(&cfg, "A2-").await?;
    bad.send(r, request(cid, ca, ua, pa, k1 ^ 0x5555)).await?;
    anyhow::ensure!(bad.expect(r, Some(2)).await.is_err(), "reprise accordée avec une mauvaise clé");
    println!("  ✓ refusée");

    step("bonne clé : la partie reprend");
    let mut a2 = world_link(&cfg, "A3-").await?;
    a2.send(r, request(cid, ca, ua, pa, k1)).await?;
    let ok = a2.expect(r, Some(2)).await?;
    let k2 = ok.int(5).unwrap_or(0);
    anyhow::ensure!(ok.int(6) == Some(1) && k2 != 0 && k2 != k1, "accord {:?}, nouvelle clé {k2}", ok.int(6));
    a2.expect_where(|q| q.is(mv, None) && q.int(0) == Some(cid as i64), 3000, "position renvoyée").await?;
    a2.expect_where(|q| q.is(&schema::OPT_VIEW, Some(1)), 3000, "PNJ renvoyés").await?;
    a2.expect_where(|q| q.is(c, Some(0)) && q.int(1) == Some(b.cid as i64), 3000, &format!("{cb} renvoyé")).await?;
    println!("  ✓ accord 1, nouvelle clé {k2}, position, PNJ et {cb} renvoyés");
    a2.send(ch, Fields::new(ch).with(0, 0).with(1, cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252("/GetExp"))).await?;
    let reply = a2.expect_where(|q| q.ptype == ch.id && q.int(2) == Some(chat_type::SYSTEM), 3000, "réponse à /GetExp").await?;
    println!("  ✓ commande sur la nouvelle connexion : {}", reply.fields().text(6).unwrap_or_default());
    anyhow::ensure!(b.world.expect_where(removed(cid), 500, "retrait").await.is_err(), "{ca} retiré pendant la reprise");

    step("seconde coupure sans reprise : départ au bout de 60 s");
    drop(a2);
    let gone = b.world.expect_where(removed(cid), 66_000, &format!("{cb} voit {ca} quitter le monde")).await;
    anyhow::ensure!(gone.is_ok(), "{ca} toujours dans le monde après 66 s");
    println!("  ✓ {ca} a quitté le monde");
    let mut late = world_link(&cfg, "A4-").await?;
    late.send(r, request(cid, ca, ua, pa, k2)).await?;
    anyhow::ensure!(late.expect(r, Some(2)).await.is_err(), "reprise accordée après le délai");
    println!("  ✓ reprise refusée après le délai");
    tokio::time::sleep(Duration::from_millis(200)).await;
    println!("\nreconnexion : OK");
    Ok(())
}
