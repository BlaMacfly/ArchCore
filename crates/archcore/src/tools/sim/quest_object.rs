//! `archcore sim questobject` : quêtes données et reprises par un objet du monde (fonction 27 d'un obj*.ini, table
//! `quest_object`).
//!
//! GmTest (chevalier humain) passe niveau 58 ; « Permission » (2830) terminée en base. Voix antique de Deribelle
//! (objet 178520354, son propre groupe : il donne et reprend « Héritage », 2831, et donne « La puissance des
//! certificats », 2832, que reprend le forgeron Torune) : fenêtre demandée de loin (rien), à 300 unités (accord,
//! EVENT_BASE renvoyé) ; 2831 refusée de loin, prise auprès de l'objet, rendue sans l'objectif (refus), puis avec
//! (Harn tué, compté en base entre deux connexions) ; 2832 prise auprès de l'objet, refusée au rendu à l'objet (le
//! forgeron la reprend), puis abandonnée.

use super::{character_id, enter_world, step, InWorld, Recv};
use archcore_net::schema::{self, chat_type, mini};
use archcore_net::{build_mini, decode_mini, Fields};

/// Voix antique de Deribelle : numéro, position (x, z).
const VOICE: (i64, f64, f64) = (178_520_354, 267_600.562_5, -22_785.878_906);
/// Quête précédente (terminée en base), quête rendue à l'objet, quête donnée par l'objet et rendue à un PNJ.
const PERMISSION: i64 = 2830;
const HERITAGE: i64 = 2831;
const CERTIFICATES: i64 = 2832;

fn say(w: &InWorld, text: &str) -> Fields {
    let ch = &schema::CHATTING;
    Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text))
}

/// Commande MJ et sa réponse (message système).
async fn gm(w: &mut InWorld, text: &str, expect: &str) -> anyhow::Result<()> {
    w.world.send(&schema::CHATTING, say(w, text)).await?;
    let ch = &schema::CHATTING;
    w.world.expect_where(|q| q.ptype == ch.id && q.fields().text(6).is_some_and(|t| t.contains(expect)), 5000, text).await?;
    Ok(())
}

/// EVENT_BASE d'un objet du monde, comme le client : { 1, numéro, -, 27 }.
fn base(object: i64) -> Vec<u8> {
    let eb = &mini::EVENT_BASE;
    build_mini(eb, &Fields::new(eb).with(0, 1).with(1, object).with(3, 27))
}

pub async fn run(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let (eq, qs) = (&schema::EVENT_QUEST, &schema::QUEST);
    let result = |op: i64, tid: i64| move |q: &Recv| q.is(&schema::QUEST, Some(op)) && q.int(4) == Some(tid);
    let db = crate::db::Databases::new(&cfg.database);
    let row = db.world.first("SELECT quest_group FROM quest_object WHERE object_id = ?", (VOICE.0,)).await?;
    anyhow::ensure!(row.is_some(), "objet {} absent de quest_object : `archcore db-init` puis `archcore import … --webzen`", VOICE.0);
    for tid in [HERITAGE, CERTIFICATES] {
        anyhow::ensure!(db.world.first("SELECT tid FROM quest_template WHERE tid = ?", (tid,)).await?.is_some(), "quête {tid} non importée (quêtes des objets écartées ?)");
    }
    let char_id = character_id(&db, char_name).await?;

    step(&format!("{char_name} : niveau 58, « Permission » ({PERMISSION}) terminée"));
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let level = w.level.max(1);
    for tid in [PERMISSION, HERITAGE, CERTIFICATES] {
        gm(&mut w, &format!("/questreset {tid}"), "").await?;
    }
    gm(&mut w, "/level 58", "").await?;
    drop(w);
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    db.characters.exec("REPLACE INTO character_quests (character_id, quest_tid, status, count_a, count_b, journal) VALUES (?, ?, 1, 0, 0, 0)", (char_id, PERMISSION)).await?;
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let me = w.cid as i64;
    anyhow::ensure!(w.quests_done.contains(&(PERMISSION as u32)), "{PERMISSION} absente des quêtes terminées");
    let window = |w: &InWorld| Fields::new(eq).with(0, 0).with(1, base(VOICE.0)).with(2, w.cid).with(3, 250);
    let ask = |op: i64, tid: i64| Fields::new(qs).with(0, op).with(1, me).with(2, base(VOICE.0)).with(4, tid);

    step("fenêtre de l'objet : de loin rien, à 300 unités accordée");
    gm(&mut w, &format!("/move {},{}", VOICE.1 - 3000.0, VOICE.2), "porté en").await?;
    w.world.send(eq, window(&w)).await?;
    anyhow::ensure!(w.world.expect_where(|q| q.is(eq, Some(0)), 1000, "refus").await.is_err(), "fenêtre ouverte à 3000 unités");
    println!("  ✓ à 3000 unités : rien");
    step("« Héritage » refusée de loin (quête précédente terminée : seule la distance compte)");
    w.world.send(qs, ask(0, HERITAGE)).await?;
    anyhow::ensure!(w.world.expect_where(result(1, HERITAGE), 3000, "refus").await?.int(6) == Some(0), "prise de loin acceptée");
    println!("  ✓ refusée (trop loin de l'objet)");
    gm(&mut w, &format!("/move {},{}", VOICE.1 - 300.0, VOICE.2), "porté en").await?;
    w.world.send(eq, window(&w)).await?;
    let r = w.world.expect_where(|q| q.is(eq, Some(0)), 3000, "accord").await?;
    let back = decode_mini(&mini::EVENT_BASE, r.fields().bytes(1).unwrap_or_default())?;
    anyhow::ensure!((back.int(0), back.int(1), back.int(3)) == (Some(1), Some(VOICE.0), Some(27)), "EVENT_BASE renvoyé : {:?}", (back.int(0), back.int(1), back.int(3)));
    println!("  ✓ accord (EVENT_BASE {{1, {}, -, 27}} renvoyé)", VOICE.0);

    step("« Héritage » prise auprès de l'objet, rendue sans l'objectif : refus");
    w.world.send(qs, ask(0, HERITAGE)).await?;
    anyhow::ensure!(w.world.expect_where(result(1, HERITAGE), 3000, "acceptation").await?.int(6) == Some(1), "prise refusée");
    w.world.send(qs, ask(2, HERITAGE)).await?;
    anyhow::ensure!(w.world.expect_where(result(3, HERITAGE), 3000, "refus").await?.int(6) == Some(0), "rendue sans l'objectif");
    println!("  ✓ prise ; rendu refusé (Harn non tué)");

    step("Harn compté en base, rendu à l'objet");
    drop(w);
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    db.characters.exec("UPDATE character_quests SET count_a = 1 WHERE character_id = ? AND quest_tid = ?", (char_id, HERITAGE)).await?;
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    gm(&mut w, &format!("/move {},{}", VOICE.1 - 300.0, VOICE.2), "porté en").await?;
    w.world.send(qs, ask(2, HERITAGE)).await?;
    anyhow::ensure!(w.world.expect_where(result(3, HERITAGE), 3000, "fin").await?.int(6) == Some(1), "rendu refusé");
    println!("  ✓ « Héritage » rendue à l'objet");

    step("« La puissance des certificats » prise auprès de l'objet, puis abandonnée");
    w.world.send(qs, ask(0, CERTIFICATES)).await?;
    anyhow::ensure!(w.world.expect_where(result(1, CERTIFICATES), 3000, "acceptation").await?.int(6) == Some(1), "prise refusée");
    w.world.send(qs, ask(2, CERTIFICATES)).await?;
    anyhow::ensure!(w.world.expect_where(result(3, CERTIFICATES), 3000, "refus").await?.int(6) == Some(0), "rendue à l'objet au lieu du forgeron");
    w.world.send(qs, Fields::new(qs).with(0, 6).with(1, me).with(4, CERTIFICATES)).await?;
    anyhow::ensure!(w.world.expect_where(result(7, CERTIFICATES), 3000, "abandon").await?.int(6) == Some(1), "abandon refusé");
    println!("  ✓ prise ; rendu à l'objet refusé (le forgeron la reprend) ; abandonnée");

    // Remise en état.
    for tid in [PERMISSION, HERITAGE, CERTIFICATES] {
        gm(&mut w, &format!("/questreset {tid}"), "").await?;
    }
    gm(&mut w, &format!("/level {level}"), "").await?;
    db.close().await;
    println!("\nquêtes des objets : OK");
    Ok(())
}
