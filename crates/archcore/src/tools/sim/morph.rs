//! `archcore sim morph` : métamorphose par potion (AGPMCHARACTER ops 11 à 13, AGPMITEM ops 4, 19, 27).
//!
//! GmTest (MJ) reçoit trois Spider Potions (TID 406 : forme 272 Arachnoid Drone, dégâts +26 / +28 d'ItemTransformTable)
//! et un Passport (198) ; SimA le regarde. Potion bue : op 4, op 19 (recharge 600 000 ms), facteurs (dégâts majorés), op 11
//! chez lui et chez SimA. Seconde potion refusée (op 27 code 4), Passport refusé (message système). SimA revient en jeu :
//! il reçoit GmTest par l'op 0 (TID d'origine, bit 0 du champ 18) puis l'op 11. Fin de durée : la configuration du
//! simulateur (`Item.TransformMaxDurationMs = 8000`) l'abrège à 8 s → op 12 chez les deux, dégâts d'origine. GmTest revient
//! en jeu (la recharge ne survit pas à la session), boit une potion, l'annule (op 13) → op 12 chez les deux.

use super::{enter_world, factors_combat, step, InWorld, Recv};
use archcore_net::schema::{self, chat_type};
use archcore_net::{Fields, Vec3};
use std::time::Duration;

const SPIDER_POTION: i64 = 406;
const SPIDER: i64 = 272;
const PASSPORT: i64 = 198;
const STOP: i64 = 0x04;

fn say(w: &InWorld, text: &str) -> Fields {
    let ch = &schema::CHATTING;
    Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text))
}

async fn move_to(w: &mut InWorld, at: Vec3) -> anyhow::Result<()> {
    let mv = &schema::OPT_CHAR_MOVE;
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, at).with(2, at).with(5, STOP).with(6, 0)).await
}

/// Objet créé par /additem : son IID.
async fn add_item(w: &mut InWorld, tid: i64, count: i64) -> anyhow::Result<i64> {
    let it = &schema::ITEM;
    w.world.send(&schema::CHATTING, say(w, &format!("/additem {tid} {count}"))).await?;
    Ok(w.world.expect_where(|q| q.is(it, None) && q.int(3) == Some(tid) && q.int(1) == Some(2), 3000, "objet ajouté").await?.int(2).unwrap_or(0))
}

/// Dégâts physiques (min, max) d'un AGPMCHARACTER op 1 de `cid` portant ses facteurs.
async fn damage(w: &mut InWorld, what: &str) -> anyhow::Result<(i64, i64)> {
    let (c, me) = (&schema::CHARACTER, w.cid as i64);
    let f = w.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(me) && q.fields().bytes(7).and_then(factors_combat).is_some(), 3000, what).await?;
    Ok(f.fields().bytes(7).and_then(factors_combat).map(|(d, _)| d).unwrap_or_default())
}

/// Potion bue : op 4 (TID), op 19 (recharge), facteurs, op 11 (forme). Rend les dégâts sous la forme.
async fn drink(w: &mut InWorld, iid: i64) -> anyhow::Result<(i64, i64)> {
    let (it, c, me) = (&schema::ITEM, &schema::CHARACTER, w.cid as i64);
    w.world.send(it, Fields::new(it).with(0, 3).with(2, iid)).await?;
    w.world.expect_where(|q| q.is(it, Some(4)) && q.int(3) == Some(SPIDER_POTION) && q.int(4) == Some(me), 3000, "op 4").await?;
    let reuse = w.world.expect_where(|q| q.is(it, Some(19)) && q.int(4) == Some(me), 3000, "op 19").await?;
    anyhow::ensure!(reuse.int(22) == Some(600_000), "recharge {:?}", reuse.int(22));
    let dmg = damage(w, "facteurs sous la forme").await?;
    let t = w.world.expect_where(|q| q.is(c, Some(11)) && q.int(1) == Some(me), 3000, "op 11").await?;
    anyhow::ensure!(t.int(2) == Some(SPIDER), "forme {:?}", t.int(2));
    Ok(dmg)
}

async fn restored(w: &mut InWorld, cid: i64, timeout_ms: u64, what: &str) -> anyhow::Result<()> {
    let c = &schema::CHARACTER;
    w.world.expect_where(|q| q.is(c, Some(12)) && q.int(1) == Some(cid), timeout_ms, what).await?;
    Ok(())
}

pub async fn run(user: &str, pass: &str, char_name: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let (it, c, ch) = (&schema::ITEM, &schema::CHARACTER, &schema::CHATTING);
    anyhow::ensure!(cfg.world.item_transform_max_ms > 0, "Item.TransformMaxDurationMs doit abréger la métamorphose (configuration du simulateur)");
    step(&format!("{cb}, témoin, et {char_name} entrent en jeu"));
    let mut witness = enter_world(&cfg, ub, pb, cb, "B-").await?;
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let me = w.cid as i64;
    let spot = witness.start;
    move_to(&mut w, spot).await?;
    move_to(&mut witness, spot).await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let potions = add_item(&mut w, SPIDER_POTION, 3).await?;
    let passport = add_item(&mut w, PASSPORT, 1).await?;
    w.world.send(ch, say(&w, "/speed 1")).await?;
    let base = damage(&mut w, "facteurs d'origine").await?;
    println!("  ✓ 3 Spider Potions ({potions}) et un Passport ({passport}) ; dégâts d'origine {base:?}");

    step("potion de métamorphose");
    let dmg = drink(&mut w, potions).await?;
    anyhow::ensure!(dmg == (base.0 + 26, base.1 + 28), "dégâts {dmg:?} au lieu de {base:?} + (26, 28)");
    println!("  ✓ op 4, op 19 (600 000 ms), dégâts {dmg:?} (ItemTransformTable : +26 / +28), op 11 → {SPIDER}");
    let seen = witness.world.expect_where(|q| q.is(c, Some(11)) && q.int(1) == Some(me), 3000, "op 11 chez le témoin").await?;
    println!("  ✓ {cb} voit {char_name} en {}", seen.int(2).unwrap_or(0));

    step("refus sous métamorphose");
    w.world.send(it, Fields::new(it).with(0, 3).with(2, potions)).await?;
    let no = w.world.expect_where(|q| q.is(it, Some(27)) && q.int(4) == Some(me), 3000, "op 27").await?;
    anyhow::ensure!(no.int(1) == Some(4), "code {:?}", no.int(1));
    println!("  ✓ seconde potion refusée (op 27 code 4)");
    w.world.send(it, Fields::new(it).with(0, 3).with(2, passport)).await?;
    let system = |q: &Recv| q.ptype == ch.id && q.int(2) == Some(chat_type::WHOLE_WORLD);
    let msg = w.world.expect_where(|q| system(q) && q.fields().text(6).is_some_and(|t| t.contains("métamorphose")), 3000, "refus du parchemin").await?;
    println!("  ✓ Passport refusé : « {} »", msg.fields().text(6).unwrap_or_default());

    step(&format!("{cb} revient en jeu : op 0 (TID d'origine, bit 0) puis op 11"));
    drop(witness);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut witness = enter_world(&cfg, ub, pb, cb, "B-").await?;
    move_to(&mut witness, spot).await?;
    let add = witness.world.expect_where(|q| q.is(c, Some(0)) && q.int(1) == Some(me), 5000, "op 0 de GmTest").await?;
    anyhow::ensure!(add.int(18).unwrap_or(0) & 0x01 != 0 && add.int(2) != Some(SPIDER), "op 0 : champ 18 {:?}, TID {:?}", add.int(18), add.int(2));
    let t = witness.world.expect_where(|q| q.is(c, Some(11)) && q.int(1) == Some(me), 3000, "op 11 après la vue").await?;
    anyhow::ensure!(t.int(2) == Some(SPIDER), "forme {:?}", t.int(2));
    println!("  ✓ op 0 (TID {}, champ 18 = {}) puis op 11 → {SPIDER}", add.int(2).unwrap_or(0), add.int(18).unwrap_or(0));

    step("fin de durée (8 s par la configuration du simulateur)");
    restored(&mut w, me, 12_000, "op 12 à la fin de la durée").await?;
    let back = damage(&mut w, "facteurs rendus").await?;
    anyhow::ensure!(back == base, "dégâts {back:?} au lieu de {base:?}");
    restored(&mut witness, me, 3000, "op 12 chez le témoin").await?;
    println!("  ✓ op 12 chez les deux, dégâts {back:?}");

    step("annulation (op 13)");
    drop(w);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    move_to(&mut w, spot).await?;
    tokio::time::sleep(Duration::from_millis(300)).await;
    drink(&mut w, potions).await?;
    witness.world.expect_where(|q| q.is(c, Some(11)) && q.int(1) == Some(me), 3000, "op 11 chez le témoin").await?;
    w.world.send(c, Fields::new(c).with(0, 13).with(1, w.cid)).await?;
    restored(&mut w, me, 3000, "op 12 après l'annulation").await?;
    restored(&mut witness, me, 3000, "op 12 chez le témoin").await?;
    println!("  ✓ op 13 → op 12 chez les deux");

    // Remise en état : potion restante et Passport détruits.
    for iid in [potions, passport] {
        w.world.send(it, Fields::new(it).with(0, 21).with(2, iid)).await?;
        w.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(iid), 3000, "objet détruit").await?;
    }
    println!("\nmétamorphose : OK");
    Ok(())
}
