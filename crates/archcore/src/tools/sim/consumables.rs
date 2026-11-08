//! `archcore sim consumables` : consommables à compétence, orbe de résurrection, oubli des compétences
//! (`world/consumables.rs`).
//!
//! GmTest (MJ) part avec Dual Slash au niveau 2 seulement (ses compétences sont remises à la fin) et une chaîne
//! « skill init » vide. Scorching Stew (TID 111) : effet 459 (AGPMSKILL op 4, 300 s), op 4 de l'objet avec la recharge de
//! la compétence (420 000 ms), second ragoût refusé (op 5). Janus Talisman of Growth LV1 (50101) : effet 1801 de 30 min
//! (durée de l'objet, objet à date d'expiration : op 26, pas consommé). Reverse Orb (1054) : refusé vivant (op 5) ; mort (/kill), il ressuscite sur place (op 1 champ 11 = 0,
//! op 4, op 18 de 1 800 000 ms) ; mort de nouveau, le second orbe est refusé (op 18 puis op 27 code 3) et GmTest rentre
//! au village (op 9). Maître des compétences (0x3B op 8, sans événement) : Dual Slash retirée (AGPMSKILL op 1), 560 pièces
//! rendues (80 + 480), op 23 (« 1 »), op 9 résultat 0 ; une seconde demande, puis une autre après reconnexion (chaîne
//! relue en base) → résultat 1. Gem of origin (4876) : Dual Slash, remise en base, de nouveau retirée (op 1), op 4.

use super::{character_id, enter_world, step, InWorld, Recv};
use archcore_net::schema::{self, chat_type, mini};
use archcore_net::{decode_mini, Fields, Value};
use std::time::Duration;

const STEW: i64 = 111;
const FIRE_DEFENSE: i64 = 459;
const TALISMAN: i64 = 50101;
const GROWTH: i64 = 1801;
const REVERSE_ORB: i64 = 1054;
const GEM_OF_ORIGIN: i64 = 4876;
const DUAL_SLASH: i64 = 1;
/// Dual Slash niveau 2 : 80 pièces (niveau 1) + 480 (amélioration).
const REFUND: i64 = 560;
const ORB_REUSE: i64 = 1_800_000;

fn say(w: &InWorld, text: &str) -> Fields {
    let ch = &schema::CHATTING;
    Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text))
}

/// Commande MJ (une toutes les 100 ms au plus, chat.rs).
async fn gm(w: &mut InWorld, text: &str) -> anyhow::Result<()> {
    tokio::time::sleep(Duration::from_millis(150)).await;
    w.world.send(&schema::CHATTING, say(w, text)).await
}

/// Objet créé par /additem : son IID.
async fn add_item(w: &mut InWorld, tid: i64, count: i64) -> anyhow::Result<i64> {
    let it = &schema::ITEM;
    gm(w, &format!("/additem {tid} {count}")).await?;
    // Inventaire (2) ou inventaire Cash (51) pour un objet Cash (talisman, Gem of origin).
    Ok(w.world.expect_where(|q| q.is(it, None) && q.int(3) == Some(tid) && matches!(q.int(1), Some(2 | 51)), 3000, "objet ajouté").await?.int(2).unwrap_or(0))
}

async fn use_item(w: &mut InWorld, iid: i64) -> anyhow::Result<()> {
    let it = &schema::ITEM;
    w.world.send(it, Fields::new(it).with(0, 3).with(2, iid)).await
}

/// Effet d'un AGPMSKILL op 4 (champ 8, premier mini SKILL_BUFF) : (TID, durée).
fn buff_of(q: &Recv) -> Option<(i64, i64)> {
    let first = q.fields().list(8)?.first()?;
    let Value::Bytes(b) = first else { return None };
    let buff = decode_mini(&mini::SKILL_BUFF, b).ok()?;
    Some((buff.int(0)?, buff.int(1).unwrap_or(0)))
}

/// Effet posé sur soi (AGPMSKILL op 4) : sa durée.
async fn effect_on(w: &mut InWorld, skill: i64) -> anyhow::Result<i64> {
    let (k, me) = (&schema::SKILL, w.cid as i64);
    let on = w.world.expect_where(|q| q.is(k, Some(4)) && super::skill_owner(q, 2) == Some(me) && buff_of(q).is_some_and(|b| b.0 == skill), 3000, "effet posé").await?;
    Ok(buff_of(&on).map(|b| b.1).unwrap_or(0))
}

/// Op 4 de l'objet : recharge du champ 18.
async fn used(w: &mut InWorld, item: i64) -> anyhow::Result<i64> {
    let (it, me) = (&schema::ITEM, w.cid as i64);
    Ok(w.world.expect_where(|q| q.is(it, Some(4)) && q.int(3) == Some(item) && q.int(4) == Some(me), 3000, "op 4 de l'objet").await?.int(18).unwrap_or(-1))
}

/// Statut d'action du joueur (AGPMCHARACTER op 1, champ 11).
async fn action_status(w: &mut InWorld, status: i64, what: &str) -> anyhow::Result<()> {
    let (c, me) = (&schema::CHARACTER, w.cid as i64);
    w.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(me) && q.int(11) == Some(status), 3000, what).await?;
    Ok(())
}

/// Oubli chez le maître : 0x3B op 8, champ 2 = CID, sans événement.
async fn master_initialize(w: &mut InWorld) -> anyhow::Result<()> {
    let sm = &schema::EVENT_SKILL_MASTER;
    w.world.send(sm, Fields::new(sm).with(0, 8).with(2, w.cid)).await
}

async fn init_result(w: &mut InWorld) -> anyhow::Result<i64> {
    let sm = &schema::EVENT_SKILL_MASTER;
    Ok(w.world.expect_where(|q| q.is(sm, Some(9)), 3000, "résultat de l'oubli (op 9)").await?.int(5).unwrap_or(-1))
}

pub async fn run(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let db = crate::db::Databases::new(&cfg.database);
    let id = character_id(&db, char_name).await?;
    // Compétences et argent de départ, remis à la fin quoi qu'il arrive.
    let saved: Vec<(u32, u32, u32)> = db
        .characters
        .rows("SELECT skill_tid, level, exp FROM character_skills WHERE character_id = ?", (id,))
        .await?
        .iter()
        .map(|r| (crate::db::col(r, "skill_tid"), crate::db::col(r, "level"), crate::db::col(r, "exp")))
        .collect();
    let money: i64 = db.characters.first("SELECT money FROM characters WHERE id = ?", (id,)).await?.map(|r| crate::db::col(&r, "money")).unwrap_or(0);
    db.characters.exec("DELETE FROM character_skills WHERE character_id = ?", (id,)).await?;
    db.characters.exec("INSERT INTO character_skills (character_id, skill_tid, level) VALUES (?, ?, 2)", (id, DUAL_SLASH)).await?;
    db.characters.exec("UPDATE characters SET skill_init = '' WHERE id = ?", (id,)).await?;
    let result = Box::pin(scenario(&cfg, &db, id, user, pass, char_name)).await;
    db.characters.exec("DELETE FROM character_skills WHERE character_id = ?", (id,)).await?;
    for (tid, level, exp) in &saved {
        db.characters.exec("INSERT INTO character_skills (character_id, skill_tid, level, exp) VALUES (?, ?, ?, ?)", (id, *tid, *level, *exp)).await?;
    }
    db.characters.exec("UPDATE characters SET skill_init = '', money = ? WHERE id = ?", (money, id)).await?;
    println!("\n  compétences ({}) et argent de {char_name} remis", saved.len());
    result
}

async fn scenario(cfg: &crate::config::Config, db: &crate::db::Databases, id: u32, user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    let (k, it, c, me_of) = (&schema::SKILL, &schema::ITEM, &schema::CHARACTER, |w: &InWorld| w.cid as i64);
    step(&format!("{char_name} entre en jeu avec Dual Slash niveau 2"));
    let mut w = enter_world(cfg, user, pass, char_name, "").await?;
    let me = me_of(&w);
    let slash = w.skills.iter().find(|s| s.1 == DUAL_SLASH).map(|s| s.0).ok_or_else(|| anyhow::anyhow!("Dual Slash absente : {:?}", w.skills))?;
    let start_money: i64 = db.characters.first("SELECT money FROM characters WHERE id = ?", (id,)).await?.map(|r| crate::db::col(&r, "money")).unwrap_or(0);
    gm(&mut w, "/recast").await?;

    step("Scorching Stew : effet Fire Defense, recharge de la compétence, second ragoût refusé");
    let stew = add_item(&mut w, STEW, 2).await?;
    use_item(&mut w, stew).await?;
    let duration = effect_on(&mut w, FIRE_DEFENSE).await?;
    let reuse = used(&mut w, STEW).await?;
    anyhow::ensure!(duration == 300_000 && reuse == 420_000, "effet {duration} ms, recharge {reuse} ms");
    println!("  ✓ effet {FIRE_DEFENSE} ({} s), op 4 recharge {reuse} ms", duration / 1000);
    use_item(&mut w, stew).await?;
    w.world.expect_where(|q| q.is(it, Some(5)) && q.int(3) == Some(STEW) && q.int(4) == Some(me), 3000, "second ragoût refusé (op 5)").await?;
    println!("  ✓ second ragoût refusé (op 5)");

    step("Janus Talisman of Growth LV1 : effet de 30 min");
    let talisman = add_item(&mut w, TALISMAN, 1).await?;
    use_item(&mut w, talisman).await?;
    // Objet Cash à date d'expiration (« ExpireTime » 30, lifetime.rs) : en cours d'utilisation (op 26, avant l'effet),
    // pas consommé.
    w.world.expect_where(|q| q.is(it, Some(26)) && q.int(2) == Some(talisman), 3000, "talisman en cours d'utilisation (op 26)").await?;
    let duration = effect_on(&mut w, GROWTH).await?;
    anyhow::ensure!(duration == 1_800_000, "effet du talisman {duration} ms");
    used(&mut w, TALISMAN).await?;
    println!("  ✓ effet {GROWTH} (30 min), talisman en cours d'utilisation jusqu'à sa date (op 26)");

    step("Reverse Orb : refusé vivant, résurrection sur place, recharge");
    let orb = add_item(&mut w, REVERSE_ORB, 2).await?;
    use_item(&mut w, orb).await?;
    w.world.expect_where(|q| q.is(it, Some(5)) && q.int(3) == Some(REVERSE_ORB), 3000, "orbe refusé vivant (op 5)").await?;
    println!("  ✓ vivant : op 5");
    gm(&mut w, "/kill").await?;
    action_status(&mut w, 2, "mort").await?;
    use_item(&mut w, orb).await?;
    action_status(&mut w, 0, "ressuscité sur place").await?;
    let used = w.world.expect_where(|q| q.is(it, Some(4)) && q.int(3) == Some(REVERSE_ORB) && q.int(4) == Some(me), 3000, "op 4 de l'orbe").await?;
    let reuse = w.world.expect_where(|q| q.is(it, Some(18)) && q.int(4) == Some(me), 3000, "op 18").await?;
    anyhow::ensure!(used.int(18) == Some(ORB_REUSE) && reuse.int(18) == Some(ORB_REUSE), "recharges {:?} / {:?}", used.int(18), reuse.int(18));
    println!("  ✓ ressuscité sur place : op 1 (statut 0), op 4 et op 18 ({ORB_REUSE} ms)");
    gm(&mut w, "/kill").await?;
    action_status(&mut w, 2, "mort de nouveau").await?;
    use_item(&mut w, orb).await?;
    let left = w.world.expect_where(|q| q.is(it, Some(18)) && q.int(4) == Some(me), 3000, "op 18 du refus").await?.int(18).unwrap_or(0);
    let refused = w.world.expect_where(|q| q.is(it, Some(27)) && q.int(4) == Some(me), 3000, "op 27").await?;
    anyhow::ensure!(refused.int(1) == Some(3) && left > 0 && left <= ORB_REUSE, "code {:?}, reste {left}", refused.int(1));
    println!("  ✓ second orbe pendant la recharge : op 18 ({left} ms), op 27 code 3");
    w.world.send(c, Fields::new(c).with(0, 9).with(1, w.cid)).await?;
    action_status(&mut w, 0, "ressuscité au village").await?;
    println!("  ✓ résurrection au village (op 9)");

    step("maître des compétences : oubli, or rendu, une seule fois");
    master_initialize(&mut w).await?;
    w.world.expect_where(|q| q.is(k, Some(1)) && q.int(1) == Some(slash), 3000, "Dual Slash retirée (AGPMSKILL op 1)").await?;
    let paid = w.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(me) && q.int(8).is_some(), 3000, "argent rendu").await?.int(8).unwrap_or(0);
    let init = w.world.expect_where(|q| q.is(c, Some(23)) && q.int(1) == Some(me), 3000, "op 23").await?;
    anyhow::ensure!(init.fields().text(19).as_deref() == Some("1"), "chaîne skill init {:?}", init.fields().text(19));
    let r = init_result(&mut w).await?;
    anyhow::ensure!(r == 0 && paid == start_money + REFUND, "résultat {r}, argent {paid} (avant {start_money})");
    let left: Option<u32> = db.characters.first("SELECT skill_tid FROM character_skills WHERE character_id = ? AND skill_tid = ?", (id, DUAL_SLASH)).await?.map(|r| crate::db::col(&r, "skill_tid"));
    anyhow::ensure!(left.is_none(), "Dual Slash encore en base");
    println!("  ✓ op 1, argent {start_money} → {paid} (+{REFUND}), op 23 « 1 », op 9 résultat 0");
    master_initialize(&mut w).await?;
    anyhow::ensure!(init_result(&mut w).await? == 1, "seconde demande acceptée");
    println!("  ✓ seconde demande : résultat 1");

    step("boîtes de loterie et clés");
    let sys = |code: i64| move |q: &Recv| q.ptype == schema::SYSTEM_MESSAGE.id && q.int(1) == Some(code);
    let chest = add_item(&mut w, 3616, 1).await?; // Gold Treasure Chest
    use_item(&mut w, chest).await?;
    w.world.expect_where(sys(2), 3000, "clé d'or exigée (message 2)").await?;
    let key = add_item(&mut w, 1838, 1).await?; // Gold Key
    use_item(&mut w, key).await?;
    w.world.expect_where(sys(1), 3000, "clé seule (message 1)").await?;
    use_item(&mut w, chest).await?;
    let got = w.world.expect_where(sys(0), 5000, "boîte ouverte (message 0)").await?;
    anyhow::ensure!(got.int(2).unwrap_or(0) > 0 && got.fields().text(4).as_deref() == Some("Gold Treasure Chest"), "message {:?} {:?}", got.int(2), got.fields().text(4));
    println!("  ✓ refus sans clé, message de la clé, {} × {} tirés", got.int(2).unwrap_or(0), got.fields().text(5).unwrap_or_default());
    let left: Vec<u32> = db.characters.rows("SELECT template_id FROM character_items WHERE character_id = ? AND template_id IN (3616, 1838)", (id,)).await?.iter().map(|r| crate::db::col(r, "template_id")).collect();
    anyhow::ensure!(left.is_empty(), "boîte ou clé encore en base : {left:?}");
    println!("  ✓ boîte et clé consommées");

    step("costume par ensemble : Wedding Dress");
    let dress = add_item(&mut w, 6446, 1).await?;
    use_item(&mut w, dress).await?;
    let piece = w.world.expect_where(|q| q.is(it, Some(0)) && q.int(3) == Some(5451) && q.int(1) == Some(3), 5000, "pièce du costume équipée").await?;
    let piece = piece.int(2).unwrap_or(0);
    println!("  ✓ pièce 5451 équipée ({piece:#x})");
    use_item(&mut w, dress).await?;
    w.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(piece), 5000, "pièce retirée (op 1)").await?;
    println!("  ✓ objet réutilisé : pièce retirée");

    step("reconnexion : chaîne relue, Gem of origin");
    db.characters.exec("INSERT INTO character_skills (character_id, skill_tid, level) VALUES (?, ?, 1)", (id, DUAL_SLASH)).await?;
    drop(w);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut w = enter_world(cfg, user, pass, char_name, "").await?;
    let me = me_of(&w);
    let slash = w.skills.iter().find(|s| s.1 == DUAL_SLASH).map(|s| s.0).ok_or_else(|| anyhow::anyhow!("Dual Slash absente après reconnexion"))?;
    master_initialize(&mut w).await?;
    anyhow::ensure!(init_result(&mut w).await? == 1, "oubli chez le maître permis après reconnexion");
    println!("  ✓ chaîne « 1 » relue : maître refusé (résultat 1)");
    let gem = add_item(&mut w, GEM_OF_ORIGIN, 1).await?;
    use_item(&mut w, gem).await?;
    w.world.expect_where(|q| q.is(k, Some(1)) && q.int(1) == Some(slash), 3000, "Dual Slash retirée par la gemme").await?;
    w.world.expect_where(|q| q.is(it, Some(4)) && q.int(3) == Some(GEM_OF_ORIGIN) && q.int(4) == Some(me), 3000, "op 4 de la gemme").await?;
    println!("  ✓ Gem of origin : op 1, op 4");
    println!("\nconsommables : OK");
    Ok(())
}
