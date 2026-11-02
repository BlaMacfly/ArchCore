//! `archcore sim gold` : or jeté au sol (AGPMITEM op 15) et ramassé par un autre joueur (op 11, op 12).
//!
//! GmTest (MJ) et SimA au nord d'Anchorville. GmTest reçoit 1000 pièces ; jeter 0 pièce, ou plus qu'il n'en porte,
//! est refusé sans réponse ; il en jette 300 : son or passe à 700 (AGPMCHARACTER op 1, champ 8), et la pile d'or
//! (TID 268, 300) tombe à ses pieds sans propriétaire (AGPMITEM op 0, champ 4 absent), vue des deux. SimA la ramasse
//! aussitôt (op 12 réussi, son or monte de 300).

use super::{enter_world, step, InWorld, Recv};
use archcore_net::schema::{self, chat_type};
use archcore_net::{Fields, Vec3};
use std::time::Duration;

/// Nord d'Anchorville, région 0.
const SPOT: Vec3 = Vec3 { x: -466_468.0, y: 4_700.0, z: -50_010.0 };
const GOLD: i64 = 268;

fn say(w: &InWorld, text: &str) -> Fields {
    let ch = &schema::CHATTING;
    Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text))
}

/// AGPMITEM op 15 : jeter `amount` pièces.
fn drop_money(w: &InWorld, amount: i64) -> Fields {
    let it = &schema::ITEM;
    Fields::new(it).with(0, 15).with(4, w.cid).with(5, amount)
}

/// Argent porté annoncé (AGPMCHARACTER op 1, champ 8) pour le personnage `cid`.
fn money_of(cid: u32) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| q.is(&schema::CHARACTER, Some(1)) && q.int(1) == Some(cid as i64) && q.int(8).is_some()
}

pub async fn run(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let (mv, ch, it) = (&schema::OPT_CHAR_MOVE, &schema::CHATTING, &schema::ITEM);
    let place = |cid: u32| Fields::new(mv).with(0, cid).with(1, SPOT).with(2, SPOT).with(5, 0x04).with(6, 0);

    step(&format!("{ca} (MJ) et {cb} au nord d'Anchorville ; {ca} reçoit 1000 pièces"));
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    let mut b = enter_world(&cfg, ub, pb, cb, "B-").await?;
    a.world.send(mv, place(a.cid)).await?;
    // B n'a pas de droits MJ : A (MJ) le pose, le serveur refusant qu'il se téléporte lui-même.
    super::gm_place(&mut a, cb, SPOT).await?;
    b.world.send(mv, place(b.cid)).await?;
    tokio::time::sleep(Duration::from_millis(800)).await;
    a.world.send(ch, say(&a, "/money 1000")).await?;
    let m = a.world.expect_where(money_of(a.cid), 3000, "argent").await?;
    anyhow::ensure!(m.int(8) == Some(1000), "argent {:?}", m.int(8));
    println!("  ✓ 1000 pièces");

    step("refus : 0 pièce, plus que l'or porté");
    let ground = |q: &Recv| q.is(&schema::ITEM, Some(0)) && q.int(1) == Some(1) && q.int(3) == Some(GOLD);
    for amount in [0, 5000] {
        a.world.send(it, drop_money(&a, amount)).await?;
        anyhow::ensure!(a.world.expect_where(ground, 800, "or au sol").await.is_err(), "{amount} pièce(s) jetée(s)");
    }
    println!("  ✓ aucun or au sol");

    step(&format!("{ca} jette 300 pièces"));
    a.world.send(it, drop_money(&a, 300)).await?;
    let m = a.world.expect_where(money_of(a.cid), 3000, "argent après le jet").await?;
    anyhow::ensure!(m.int(8) == Some(700), "argent {:?}", m.int(8));
    let seen_a = a.world.expect_where(ground, 3000, "or au sol chez A").await?;
    anyhow::ensure!(seen_a.int(5) == Some(300) && seen_a.int(4).is_none(), "pile : {:?} pièce(s), propriétaire {:?}", seen_a.int(5), seen_a.int(4));
    let seen_b = b.world.expect_where(ground, 3000, "or au sol chez B").await?;
    let iid = seen_b.int(2).unwrap_or(0);
    anyhow::ensure!(seen_a.int(2) == Some(iid), "objets différents");
    println!("  ✓ 700 pièces restantes ; pile de 300 sans propriétaire ({iid:#x}) vue des deux");

    step(&format!("{cb} la ramasse"));
    b.world.send(it, Fields::new(it).with(0, 11).with(2, iid)).await?;
    let r = b.world.expect_where(|q| q.is(&schema::ITEM, Some(12)), 3000, "résultat du ramassage").await?;
    anyhow::ensure!(r.int(1) == Some(0) && r.int(5) == Some(300), "ramassage : résultat {:?}, quantité {:?}", r.int(1), r.int(5));
    b.world.expect_where(money_of(b.cid), 3000, "argent de B").await?;
    a.world.expect_where(|q| q.is(&schema::ITEM, Some(1)) && q.int(2) == Some(iid), 3000, "pile retirée chez A").await?;
    println!("  ✓ op 12 réussi, or crédité à {cb}, pile retirée chez {ca}");

    println!("\nor jeté : OK");
    Ok(())
}
