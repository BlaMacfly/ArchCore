//! `archcore sim recall` : rappel de groupe (`world/party.rs`) ; `archcore sim social` : actions sociales
//! (`world/social.rs`).
//!
//! Rappel, GmTest (MJ) + SimA : Bead of Summoning (2756) sans groupe refusé (AGPMITEM op 5) ; groupe formé, GmTest
//! s'éloigne (/move) et rappelle : op 4 (TID, CID) au lanceur, gestion op 15 (lanceur, nom) à SimA, qui accepte (0x07 op 5
//! puis op 16) : téléporté à côté de GmTest, op 16 (SimA) au lanceur ; second rappel refusé par SimA (op 17 au lanceur) ;
//! acceptation sans demande ignorée ; Summoning Orb (1174, Cash, 300 s de recharge) : second usage refusé (op 5).
//! Social, GmTest + SimA côte à côte : SimA salue (type 5) : op 16 chez les deux ; type 17 (écran de sélection) refusé ;
//! action trop rapprochée ignorée, puis permise après `Social.MinIntervalMs`.

use super::{enter_world, start_at, step, InWorld, Recv};
use archcore_net::schema::{self, chat_type};
use archcore_net::{Fields, Vec3};
use std::time::Duration;

const SPOT: Vec3 = Vec3 { x: -476_000.0, y: 4_422.0, z: -61_000.0 };
/// Ailleurs dans Anchorville, à plus de 10 000 unités.
const FAR: Vec3 = Vec3 { x: -466_000.0, y: 4_422.0, z: -51_500.0 };
const BEAD: i64 = 2756;
const ORB: i64 = 1174;
const STOP: i64 = 0x04;

fn say(w: &InWorld, text: &str) -> Fields {
    let ch = &schema::CHATTING;
    Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text))
}

/// Commande MJ (une toutes les 100 ms au plus, chat.rs).
async fn gm(w: &mut InWorld, text: &str) -> anyhow::Result<()> {
    tokio::time::sleep(Duration::from_millis(150)).await;
    w.world.send(&schema::CHATTING, say(w, text)).await
}

/// Objet créé par /additem : son IID (inventaire ou inventaire Cash).
async fn add_item(w: &mut InWorld, tid: i64, count: i64) -> anyhow::Result<i64> {
    let it = &schema::ITEM;
    gm(w, &format!("/additem {tid} {count}")).await?;
    Ok(w.world.expect_where(|q| q.is(it, None) && q.int(3) == Some(tid) && matches!(q.int(1), Some(2 | 51)), 3000, "objet ajouté").await?.int(2).unwrap_or(0))
}

async fn use_item(w: &mut InWorld, iid: i64) -> anyhow::Result<()> {
    let it = &schema::ITEM;
    w.world.send(it, Fields::new(it).with(0, 3).with(2, iid)).await
}

/// Les deux joueurs à `SPOT`, arrêtés.
async fn enter_both(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<(InWorld, InWorld)> {
    let cfg = crate::config::load()?;
    start_at(&cfg, ca, SPOT).await?;
    start_at(&cfg, cb, SPOT).await?;
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    let mut b = enter_world(&cfg, ub, pb, cb, "B-").await?;
    let mv = &schema::OPT_CHAR_MOVE;
    for w in [&mut a, &mut b] {
        let cid = w.cid;
        w.world.send(mv, Fields::new(mv).with(0, cid).with(1, SPOT).with(2, SPOT).with(5, STOP).with(6, 0)).await?;
    }
    Ok((a, b))
}

pub async fn run(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    let (pm, it) = (&schema::PARTY_MANAGEMENT, &schema::ITEM);
    let is_m = |op: i64| move |q: &Recv| q.is(pm, Some(op));
    step(&format!("{ca} et {cb} entrent en jeu"));
    let (mut a, mut b) = enter_both(ua, pa, ca, ub, pb, cb).await?;
    let (acid, bcid) = (a.cid as i64, b.cid as i64);
    gm(&mut a, "/recast").await?;

    step("sans groupe : rappel refusé");
    let bead = add_item(&mut a, BEAD, 3).await?;
    use_item(&mut a, bead).await?;
    a.world.expect_where(|q| q.is(it, Some(5)) && q.int(3) == Some(BEAD) && q.int(4) == Some(acid), 3000, "refus sans groupe (op 5)").await?;
    println!("  ✓ op 5 (aucun autre membre)");

    step(&format!("{ca} invite {cb}, qui accepte"));
    a.world.send(pm, Fields::new(pm).with(0, 0).with(1, acid).with(2, bcid)).await?;
    b.world.expect_where(is_m(0), 3000, "invitation reçue").await?;
    b.world.send(pm, Fields::new(pm).with(0, 2).with(1, bcid).with(2, acid)).await?;
    a.world.expect_where(is_m(2), 3000, "acceptation annoncée").await?;
    println!("  ✓ groupe formé");

    step(&format!("{ca} s'éloigne et rappelle son groupe ; {cb} accepte"));
    gm(&mut a, &format!("/move {},{}", FAR.x, FAR.z)).await?;
    a.world.expect_where(|q| q.is(&schema::OPT_CHAR_MOVE, None) && q.int(0) == Some(acid), 3000, "téléportation du lanceur").await?;
    use_item(&mut a, bead).await?;
    let ok = a.world.expect_where(|q| q.is(it, Some(4)) && q.int(3) == Some(BEAD) && q.int(4) == Some(acid), 3000, "op 4 de l'orbe").await?;
    println!("  ✓ {ca} : op 4 (TID {BEAD}, recharge {:?} ms)", ok.int(18));
    let req = b.world.expect_where(is_m(15), 3000, "demande de rappel (op 15)").await?;
    anyhow::ensure!(req.int(1) == Some(acid) && req.fields().text(3).as_deref() == Some(ca), "op 15 : lanceur {:?}, nom {:?}", req.int(1), req.fields().text(3));
    println!("  ✓ {cb} reçoit l'op 15 (lanceur {ca})");
    let pt = &schema::PRIVATE_TRADE;
    b.world.send(pt, Fields::new(pt).with(0, 5).with(1, bcid)).await?;
    b.world.send(pm, Fields::new(pm).with(0, 16).with(1, bcid).with(2, acid)).await?;
    let mv = &schema::OPT_CHAR_MOVE;
    let landed = b.world.expect_where(|q| q.is(mv, None) && q.int(0) == Some(bcid), 3000, "téléportation du membre").await?;
    let pos = landed.fields().vec3(1).unwrap_or_default();
    anyhow::ensure!((pos.x - FAR.x).abs() < 500.0 && (pos.z - FAR.z).abs() < 500.0, "{cb} posé en {:.0}, {:.0}", pos.x, pos.z);
    let told = a.world.expect_where(is_m(16), 3000, "acceptation au lanceur (op 16)").await?;
    anyhow::ensure!(told.int(1) == Some(bcid), "op 16 : membre {:?}", told.int(1));
    println!("  ✓ {cb} posé en {:.0}, {:.0} ; {ca} reçoit l'op 16", pos.x, pos.z);

    step(&format!("second rappel : {cb} refuse"));
    use_item(&mut a, bead).await?;
    b.world.expect_where(is_m(15), 3000, "seconde demande").await?;
    b.world.send(pm, Fields::new(pm).with(0, 17).with(1, bcid).with(2, acid)).await?;
    let no = a.world.expect_where(is_m(17), 3000, "refus au lanceur (op 17)").await?;
    anyhow::ensure!(no.int(1) == Some(bcid), "op 17 : membre {:?}", no.int(1));
    println!("  ✓ {ca} reçoit l'op 17");
    b.world.send(pm, Fields::new(pm).with(0, 16).with(1, bcid).with(2, acid)).await?;
    anyhow::ensure!(a.world.expect_where(is_m(16), 800, "acceptation sans demande").await.is_err(), "acceptation sans demande relayée");
    println!("  ✓ acceptation sans demande ignorée");

    step("Summoning Orb : recharge de 300 s");
    let orb = add_item(&mut a, ORB, 2).await?;
    use_item(&mut a, orb).await?;
    let used = a.world.expect_where(|q| q.is(it, Some(4)) && q.int(3) == Some(ORB) && q.int(4) == Some(acid), 3000, "op 4 du Summoning Orb").await?;
    anyhow::ensure!(used.int(18) == Some(300_000), "recharge {:?}", used.int(18));
    b.world.expect_where(is_m(15), 3000, "demande du Summoning Orb").await?;
    use_item(&mut a, orb).await?;
    a.world.expect_where(|q| q.is(it, Some(5)) && q.int(3) == Some(ORB), 3000, "second orbe refusé (op 5)").await?;
    println!("  ✓ op 4 (300 000 ms), second orbe refusé (op 5)");
    b.world.send(pm, Fields::new(pm).with(0, 3).with(1, bcid)).await?;
    gm(&mut a, "/recast").await?;
    println!("\nrappel de groupe : OK");
    Ok(())
}

pub async fn social(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    let c = &schema::CHARACTER;
    step(&format!("{ca} et {cb} entrent en jeu côte à côte"));
    let (mut a, mut b) = enter_both(ua, pa, ca, ub, pb, cb).await?;
    let bcid = b.cid as i64;
    let emote = |kind: i64| Fields::new(c).with(0, 16).with(1, bcid).with(16, kind);
    let is_social = move |kind: i64| move |q: &Recv| q.is(c, Some(16)) && q.int(1) == Some(bcid) && q.int(16) == Some(kind);
    // Les deux se voient (entrée dans la vue).
    tokio::time::sleep(Duration::from_millis(800)).await;

    step(&format!("{cb} danse (type 5)"));
    b.world.send(c, emote(5)).await?;
    b.world.expect_where(is_social(5), 3000, "action renvoyée au lanceur").await?;
    a.world.expect_where(is_social(5), 3000, "action vue par le voisin").await?;
    println!("  ✓ op 16 (type 5) chez {cb} et chez {ca}");

    step("refus : type 17 (écran de sélection), action trop rapprochée");
    tokio::time::sleep(Duration::from_millis(1100)).await;
    b.world.send(c, emote(17)).await?;
    anyhow::ensure!(a.world.expect_where(is_social(17), 800, "type 17").await.is_err(), "type 17 relayé");
    b.world.send(c, emote(0)).await?;
    b.world.expect_where(is_social(0), 3000, "salut").await?;
    b.world.send(c, emote(1)).await?;
    anyhow::ensure!(b.world.expect_where(is_social(1), 600, "action rapprochée").await.is_err(), "action trop rapprochée relayée");
    tokio::time::sleep(Duration::from_millis(600)).await;
    b.world.send(c, emote(16)).await?;
    a.world.expect_where(is_social(16), 3000, "assis, après le délai").await?;
    println!("  ✓ type 17 ignoré, seconde action sous 1 s ignorée, permise ensuite");
    drop(a);
    println!("\nactions sociales : OK");
    Ok(())
}
