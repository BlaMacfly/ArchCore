//! `archcore sim questgather` : ramassage d'objets de quête sur les objets du monde (AGPMQUEST op 11).
//!
//! GmTest (chevalier humain) passe niveau 58, « Protection silencieuse » (2829) terminée en base. « Permission »
//! (2830, donnée et reprise par la Voix antique de Deribelle) demande quatre certificats, un par objet du monde
//! (colonnes `ObjectA` à `ObjectD`). Ramassage refusé hors quête, puis de loin ; accepté près de chaque objet (objet
//! remis, progression op 10, objectifs remplis au quatrième) ; refusé pour un objet déjà détenu et pour un autre objet
//! que celui du rang ; quête rendue à la Voix (certificats repris).

use super::{character_id, enter_world, step, InWorld, Recv};
use archcore_net::schema::{self, chat_type, mini};
use archcore_net::{build_mini, decode_mini, Fields};

/// Voix antique de Deribelle : numéro, position (x, z).
const VOICE: (i64, f64, f64) = (178_520_354, 267_600.562_5, -22_785.878_906);
const PREVIOUS: i64 = 2829;
const PERMISSION: i64 = 2830;
/// Rang (A = 0 … D = 3), objet du monde, position (x, z), certificat remis.
const GATHERS: [(i64, i64, f64, f64, i64); 4] = [
    (0, 178_520_252, 261_400.0, -37_000.0, 4830),
    (1, 178_520_255, 261_400.0, -49_400.0, 4831),
    (2, 178_520_254, 273_800.0, -49_400.0, 4832),
    (3, 178_520_253, 273_800.0, -37_000.0, 4833),
];

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

/// Op 11 telle que le client l'envoie au bout de la barre de 5 s (FUN_14033ef80).
async fn gather(w: &mut InWorld, object: i64, rank: i64) -> anyhow::Result<()> {
    let qs = &schema::QUEST;
    w.world.send(qs, Fields::new(qs).with(0, 11).with(1, w.cid).with(2, base(object)).with(4, PERMISSION).with(5, rank)).await
}

/// Aucun objet remis dans la seconde.
async fn nothing(w: &mut InWorld, what: &str) -> anyhow::Result<()> {
    let it = &schema::ITEM;
    anyhow::ensure!(w.world.expect_where(|q| q.is(it, Some(0)), 1000, "rien").await.is_err(), "objet remis : {what}");
    Ok(())
}

pub async fn run(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let (qs, it) = (&schema::QUEST, &schema::ITEM);
    let result = |op: i64, tid: i64| move |q: &Recv| q.is(&schema::QUEST, Some(op)) && q.int(4) == Some(tid);
    let db = crate::db::Databases::new(&cfg.database);
    anyhow::ensure!(db.world.first("SELECT tid FROM quest_template WHERE tid = ?", (PERMISSION,)).await?.is_some(), "quête {PERMISSION} non importée (`archcore import … --webzen`)");
    let char_id = character_id(&db, char_name).await?;

    step(&format!("{char_name} : niveau 58, « Protection silencieuse » ({PREVIOUS}) terminée"));
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let level = w.level.max(1);
    for tid in [PREVIOUS, PERMISSION] {
        gm(&mut w, &format!("/questreset {tid}"), "").await?;
    }
    gm(&mut w, "/level 58", "").await?;
    drop(w);
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    db.characters.exec("REPLACE INTO character_quests (character_id, quest_tid, status, count_a, count_b, journal) VALUES (?, ?, 1, 0, 0, 0)", (char_id, PREVIOUS)).await?;
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let me = w.cid as i64;
    let (rank, object, ox, oz, _) = GATHERS[0];

    step("ramassage hors quête : rien");
    gm(&mut w, &format!("/move {},{}", ox - 300.0, oz), "porté en").await?;
    gather(&mut w, object, rank).await?;
    nothing(&mut w, "hors quête").await?;
    println!("  ✓ rien sans la quête");

    step("« Permission » prise auprès de la Voix antique");
    gm(&mut w, &format!("/move {},{}", VOICE.1 - 300.0, VOICE.2), "porté en").await?;
    w.world.send(qs, Fields::new(qs).with(0, 0).with(1, me).with(2, base(VOICE.0)).with(4, PERMISSION)).await?;
    anyhow::ensure!(w.world.expect_where(result(1, PERMISSION), 3000, "acceptation").await?.int(6) == Some(1), "prise refusée");
    // Progression envoyée à la prise (objets déjà réunis) : aucun certificat encore.
    w.world.expect_where(result(10, PERMISSION), 3000, "progression à la prise").await?;
    println!("  ✓ prise");

    step("ramassage de loin : rien");
    gm(&mut w, &format!("/move {},{}", ox - 3000.0, oz), "porté en").await?;
    gather(&mut w, object, rank).await?;
    nothing(&mut w, "à 3000 unités").await?;
    println!("  ✓ rien à 3000 unités");

    step("un certificat par objet du monde, objectifs remplis au quatrième");
    for (i, (rank, object, x, z, item)) in GATHERS.into_iter().enumerate() {
        gm(&mut w, &format!("/move {},{}", x - 300.0, z), "porté en").await?;
        gather(&mut w, object, rank).await?;
        w.world.expect_where(|q| q.is(it, Some(0)) && q.int(3) == Some(item), 3000, "certificat remis").await?;
        let p = w.world.expect_where(result(10, PERMISSION), 3000, "progression").await?;
        let info = decode_mini(&mini::QUEST_INFO, p.fields().bytes(3).unwrap_or_default())?;
        let done = info.int(3) == Some(1);
        anyhow::ensure!(done == (i == GATHERS.len() - 1), "objectifs remplis : {done} après {} certificat(s)", i + 1);
        println!("  ✓ rang {rank} : objet {item} remis sur l'objet {object}{}", if done { ", objectifs remplis" } else { "" });
    }

    step("refus : objet déjà détenu, autre objet que celui du rang");
    gather(&mut w, GATHERS[3].1, GATHERS[3].0).await?;
    nothing(&mut w, "déjà détenu").await?;
    gather(&mut w, GATHERS[3].1, GATHERS[0].0).await?;
    nothing(&mut w, "objet d'un autre rang").await?;
    println!("  ✓ rien (déjà détenu ; pas l'objet du rang A)");

    step("« Permission » rendue à la Voix : certificats repris");
    gm(&mut w, &format!("/move {},{}", VOICE.1 - 300.0, VOICE.2), "porté en").await?;
    w.world.send(qs, Fields::new(qs).with(0, 2).with(1, me).with(2, base(VOICE.0)).with(4, PERMISSION).with(7, -1)).await?;
    anyhow::ensure!(w.world.expect_where(result(3, PERMISSION), 3000, "fin").await?.int(6) == Some(1), "rendu refusé");
    let left: i64 = db
        .characters
        .first("SELECT COUNT(*) AS n FROM character_items WHERE character_id = ? AND template_id IN (4830, 4831, 4832, 4833)", (char_id,))
        .await?
        .map(|r| crate::db::col(&r, "n"))
        .unwrap_or(-1);
    anyhow::ensure!(left == 0, "{left} certificat(s) encore dans l'inventaire");
    println!("  ✓ rendue, certificats repris");

    // Remise en état.
    for tid in [PREVIOUS, PERMISSION] {
        gm(&mut w, &format!("/questreset {tid}"), "").await?;
    }
    gm(&mut w, &format!("/level {level}"), "").await?;
    db.close().await;
    println!("\nramassage des quêtes : OK");
    Ok(())
}
