//! `archcore sim customize` : changement d'apparence chez Gladiol (AgpmEventCharCustomize 0x43, fonction 29).
//!
//! GmTest (MJ) et SimA chez Gladiol, coiffeur d'Anchorville. Ouverture (op 0 → op 1, événement et CID renvoyés).
//! Achat de la coiffure 4 (« Top Volume 1 », 1000 pièces, Knight) : refusé faute d'or (message système), puis accepté :
//! argent débité (champ 8), coiffure 21 = 4 chez GmTest et chez SimA qui le voit (AGPMCHARACTER op 1). La même
//! coiffure une seconde fois est refusée ; la coiffure 1 (gratuite) remet l'apparence d'origine.

use super::{enter_world, step, InWorld, Recv};
use archcore_net::schema::{self, chat_type, customize_op, mini};
use archcore_net::{build_mini, decode_mini, Fields, Vec3};
use std::time::Duration;

const GLADIOL: i64 = 0x4000_0000 + 222;
const SALON: Vec3 = Vec3 { x: -474_100.0, y: 4_713.0, z: -66_100.0 };
const HAIR: i64 = 4;
const PRICE: i64 = 1000;

fn say(w: &InWorld, text: &str) -> Fields {
    let ch = &schema::CHATTING;
    Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text))
}

async fn gm(w: &mut InWorld, text: &str) -> anyhow::Result<()> {
    w.world.send(&schema::CHATTING, say(w, text)).await?;
    w.world.expect_where(|q| q.ptype == schema::CHATTING.id, 3000, text).await?;
    Ok(())
}

fn buy(w: &InWorld, face: i64, hair: i64) -> Fields {
    let (s, eb) = (&schema::EVENT_CHAR_CUSTOMIZE, &mini::EVENT_BASE);
    let event = build_mini(eb, &Fields::new(eb).with(0, 3).with(1, GLADIOL).with(3, 29));
    Fields::new(s).with(0, customize_op::BUY).with(1, event).with(2, w.cid).with(3, face).with(4, hair)
}

async fn refused(w: &mut InWorld, expect: &'static str) -> anyhow::Result<()> {
    let ch = &schema::CHATTING;
    let r = w.world.expect_where(|q| q.ptype == ch.id && q.int(2) == Some(chat_type::WHOLE_WORLD), 3000, expect).await?;
    let got = r.fields().text(6).unwrap_or_default();
    anyhow::ensure!(got.contains(expect), "refus attendu ({expect}) : {got:?}");
    println!("  ✓ refus : « {got} »");
    Ok(())
}

pub async fn run(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let (mv, s, c) = (&schema::OPT_CHAR_MOVE, &schema::EVENT_CHAR_CUSTOMIZE, &schema::CHARACTER);
    let place = |cid: u32| Fields::new(mv).with(0, cid).with(1, SALON).with(2, SALON).with(5, 0x04).with(6, 0);

    step(&format!("{ca} (MJ) et {cb} chez Gladiol"));
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    let mut b = enter_world(&cfg, ub, pb, cb, "B-").await?;
    let acid = a.cid as i64;
    a.world.send(mv, place(a.cid)).await?;
    // B n'a pas de droits MJ : A (MJ) le pose, le serveur refusant qu'il se téléporte lui-même.
    super::gm_place(&mut a, cb, SALON).await?;
    b.world.send(mv, place(b.cid)).await?;
    tokio::time::sleep(Duration::from_millis(800)).await;
    gm(&mut a, "/level 20").await?;

    step("ouverture de la fenêtre (op 0 → op 1)");
    let eb = &mini::EVENT_BASE;
    let event = build_mini(eb, &Fields::new(eb).with(0, 3).with(1, GLADIOL).with(3, 29));
    a.world.send(s, Fields::new(s).with(0, customize_op::REQUEST).with(1, event).with(2, a.cid)).await?;
    let grant = a.world.expect_where(|q| q.is(s, Some(customize_op::GRANT)), 3000, "accord (op 1)").await?;
    let base = decode_mini(eb, grant.fields().bytes(1).unwrap_or_default())?;
    anyhow::ensure!(grant.int(2) == Some(acid) && base.int(1) == Some(GLADIOL) && base.int(3) == Some(29), "op 1 : CID {:?}, PNJ {:?}", grant.int(2), base.int(1));
    println!("  ✓ op 1 : événement de Gladiol (fonction 29) et CID renvoyés");

    step(&format!("coiffure {HAIR} sans assez d'or"));
    gm(&mut a, "/money 500").await?;
    a.world.send(s, buy(&a, -1, HAIR)).await?;
    refused(&mut a, "pièce(s) d'or").await?;

    step(&format!("coiffure {HAIR} pour {PRICE} pièces"));
    gm(&mut a, "/money 5000").await?;
    a.world.send(s, buy(&a, -1, HAIR)).await?;
    let money = a.world.expect_where(|q: &Recv| q.is(c, Some(1)) && q.int(8).is_some(), 3000, "argent débité").await?;
    anyhow::ensure!(money.int(8) == Some(5000 - PRICE), "argent {:?}", money.int(8));
    let is_hair = |q: &Recv| q.is(c, Some(1)) && q.int(1) == Some(acid) && q.int(21).is_some();
    let mine = a.world.expect_where(is_hair, 3000, "nouvelle coiffure chez A").await?;
    anyhow::ensure!(mine.int(21) == Some(HAIR) && mine.int(20).is_none(), "champs 20 {:?}, 21 {:?}", mine.int(20), mine.int(21));
    let seen = b.world.expect_where(is_hair, 3000, "nouvelle coiffure chez B").await?;
    anyhow::ensure!(seen.int(21) == Some(HAIR), "chez {cb} : {:?}", seen.int(21));
    println!("  ✓ {} pièces restantes ; champ 21 = {HAIR} chez {ca} et chez {cb}", 5000 - PRICE);

    step("même coiffure une seconde fois : refusée ; coiffure 1 (gratuite) remise");
    tokio::time::sleep(Duration::from_millis(200)).await;
    a.world.send(s, buy(&a, -1, HAIR)).await?;
    refused(&mut a, "différents").await?;
    a.world.send(s, buy(&a, -1, 1)).await?;
    a.world.expect_where(|q| is_hair(q) && q.int(21) == Some(1), 3000, "coiffure 1").await?;
    println!("  ✓ coiffure d'origine remise");

    println!("\napparence : OK");
    Ok(())
}
