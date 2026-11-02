//! Scénarios des règles proposées d'après les tables de l'épisode 8 (`world/ammo.rs`, `world/requirement.rs`,
//! `world/lifetime.rs`).
//!
//! ```text
//!   archcore sim ammo     [compte] [mdp] [perso]   munitions et arme exigée, SimArcher (archer humain du compte MJ
//!                                                  simarcher, créés au besoin) : Short Bow porté ; sans flèche, coup refusé (résultat 2)
//!                                                  et Dual Shot refusée (op 7, action 12) ; Blade Sting (épée exigée)
//!                                                  refusée (action 8) ; avec 10 flèches, un coup en coûte une (op 2) et
//!                                                  Dual Shot deux
//!   archcore sim lifetime [compte] [mdp] [perso]   objets à durée, GmTest : Talisman of Awakening <1 Day> utilisé (op 26 :
//!                                                  en cours, date ≈ maintenant + 1 jour ; pas consommé), <30 Minutes>
//!                                                  (reste 1 800 000 ms) ; date passée en base, nouvelle entrée : message
//!                                                  système 5 puis l'objet part (op 1) ; reste relu à l'entrée (op 0)
//!   archcore sim deathstrike [compte] [mdp] [perso] Death Strike (passive 74 au niveau 20 : 90 %), GmTest : un Ver vert
//!                                                  tombe d'un coup, résultat 13 (AGPMOPTIMIZEDCHARACTION)
//! ```

use super::{character_id, enter_world, step, InWorld, Recv};
use archcore_net::schema::{self, chat_type, mini};
use archcore_net::{decode_mini, Fields};
use std::time::Duration;

/// Short Bow (archer, niveau 1), flèches, Dual Shot (2 flèches au niveau 1), Blade Sting (épée exigée).
const BOW: i64 = 1;
const ARROW: i64 = 194;
const DUAL_SHOT: i64 = 40;
const BLADE_STING: i64 = 223;
/// Modèle de l'archer humain.
const ARCHER_TID: &str = "1";
/// Talisman of Awakening <1 Day> (temps réel, 1440 min) et <30 Minutes> (temps de jeu).
const TALISMAN_DAY: i64 = 3436;
const TALISMAN_30: i64 = 1844;

fn say(w: &InWorld, text: &str) -> Fields {
    let ch = &schema::CHATTING;
    Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text))
}

/// Commande MJ (une toutes les 100 ms au plus, chat.rs), sa réponse attendue.
async fn gm(w: &mut InWorld, text: &str) -> anyhow::Result<()> {
    tokio::time::sleep(Duration::from_millis(150)).await;
    w.world.send(&schema::CHATTING, say(w, text)).await?;
    let ch = &schema::CHATTING;
    w.world.expect_where(|q| q.ptype == ch.id && q.int(2) == Some(chat_type::WHOLE_WORLD), 3000, text).await?;
    Ok(())
}

/// Objet créé par /additem (inventaire ou inventaire Cash) : son IID.
async fn add_item(w: &mut InWorld, tid: i64, count: i64) -> anyhow::Result<i64> {
    let it = &schema::ITEM;
    tokio::time::sleep(Duration::from_millis(150)).await;
    w.world.send(&schema::CHATTING, say(w, &format!("/additem {tid} {count}"))).await?;
    Ok(w.world.expect_where(|q| q.is(it, None) && q.int(3) == Some(tid) && matches!(q.int(1), Some(2 | 51)), 3000, "objet ajouté").await?.int(2).unwrap_or(0))
}

/// Type d'action du mini SKILL_ACTION d'un AGPMSKILL op 7.
fn action_of(q: &Recv) -> Option<i64> {
    q.fields().bytes(6).and_then(|b| decode_mini(&mini::SKILL_ACTION, b).ok()).and_then(|a| a.int(0))
}

/// Mini Cash (champ 23) d'un paquet AGPMITEM : (en cours, reste, date).
fn cash_of(q: &Recv) -> Option<(i64, i64, i64)> {
    let m = decode_mini(&mini::ITEM_CASH, q.fields().bytes(23)?).ok()?;
    Some((m.int(0).unwrap_or(0), m.int(1).unwrap_or(0), m.int(2).unwrap_or(0)))
}

fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub async fn ammo(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let db = crate::db::Databases::new(&cfg.database);
    let (act, k, it) = (&schema::OPT_CHAR_ACTION, &schema::SKILL, &schema::ITEM);
    step(&format!("{char_name} (archer) entre en jeu, arc porté, sans flèche"));
    // Compte MJ (mot de passe identique au nom) et archer humain (modèle 1), créés au besoin.
    let hash = crate::login::sha256_hex(&crate::tools::to_cp1252(pass));
    db.auth
        .exec("INSERT INTO account (username, password_hash, gm_level) VALUES (?, ?, 3) ON DUPLICATE KEY UPDATE password_hash = VALUES(password_hash), gm_level = 3", (user, hash.as_str()))
        .await?;
    std::env::set_var("ARCHCORE_SIM_CREATE_TID", ARCHER_TID);
    let w = enter_world(&cfg, user, pass, char_name, "").await?;
    std::env::remove_var("ARCHCORE_SIM_CREATE_TID");
    drop(w);
    let id = character_id(&db, char_name).await?;
    db.characters.exec("DELETE FROM character_items WHERE character_id = ? AND template_id IN (194, 238)", (id,)).await?;
    for tid in [DUAL_SHOT, BLADE_STING] {
        db.characters.exec("INSERT INTO character_skills (character_id, skill_tid, level) VALUES (?, ?, 1) ON DUPLICATE KEY UPDATE level = 1", (id, tid)).await?;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let me = w.cid as i64;
    gm(&mut w, "/level 10").await?;
    gm(&mut w, "/invincible 1").await?;
    gm(&mut w, "/recast").await?;
    super::hold_item(&mut w, BOW).await?;
    let skill = |w: &InWorld, tid: i64| w.skills.iter().find(|s| s.1 == tid).map(|s| s.0).ok_or_else(|| anyhow::anyhow!("compétence {tid} absente"));
    let (dual, sting) = (skill(&w, DUAL_SHOT)?, skill(&w, BLADE_STING)?);
    let worm = super::competences::worms(&mut w).await?[0];

    step("coup sans flèche : résultat 2");
    w.world.send(act, Fields::new(act).with(0, w.cid).with(1, worm).with(6, 1)).await?;
    let refused = w.world.expect_where(|q| q.is(act, None) && q.int(0) == Some(me) && q.int(1) == Some(worm), 20_000, "coup").await?;
    anyhow::ensure!(refused.int(2) == Some(2), "résultat {:?} (attendu 2, pas de flèche)", refused.int(2));
    println!("  ✓ résultat 2 : « You do not have any arrows »");

    step("Dual Shot sans flèche : action 12 ; Blade Sting à l'arc : action 8");
    w.world.send(k, super::cast_fields(dual, worm)).await?;
    let r = w.world.expect_where(|q| q.is(k, Some(7)) && q.int(1) == Some(dual), 5000, "refus de Dual Shot").await?;
    anyhow::ensure!(action_of(&r) == Some(12), "Dual Shot : action {:?} (attendu 12)", action_of(&r));
    println!("  ✓ Dual Shot refusée (action 12, pas assez de flèches)");
    w.world.send(k, super::cast_fields(sting, worm)).await?;
    let r = w.world.expect_where(|q| q.is(k, Some(7)) && q.int(1) == Some(sting), 5000, "refus de Blade Sting").await?;
    anyhow::ensure!(action_of(&r) == Some(8), "Blade Sting : action {:?} (attendu 8)", action_of(&r));
    println!("  ✓ Blade Sting refusée (action 8, arme exigée : une épée)");

    step("10 flèches : un coup en coûte une, Dual Shot deux");
    let arrows = add_item(&mut w, ARROW, 10).await?;
    w.world.send(act, Fields::new(act).with(0, w.cid).with(1, worm).with(6, 1)).await?;
    let hit = w.world.expect_where(|q| q.is(act, None) && q.int(0) == Some(me) && q.int(1) == Some(worm), 20_000, "coup avec flèche").await?;
    anyhow::ensure!(hit.int(2) != Some(2), "coup encore refusé avec des flèches");
    let left = w.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(arrows), 3000, "flèche consommée").await?;
    anyhow::ensure!(left.int(5) == Some(9), "flèches restantes {:?} (attendu 9)", left.int(5));
    println!("  ✓ coup porté (résultat {:?}), 9 flèches", hit.int(2));
    // Plus d'attaque normale : la compétence seule.
    let mv = &schema::OPT_CHAR_MOVE;
    let at = w.start;
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, at).with(2, at).with(5, 0x04).with(6, 0)).await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let before = count_after(&mut w, arrows, 500).await.unwrap_or(9);
    w.world.send(k, super::cast_fields(dual, worm)).await?;
    w.world.expect_where(|q| q.is(k, Some(7)) && q.int(1) == Some(dual) && action_of(q) == Some(1), 8000, "Dual Shot lancée").await?;
    let mut after = before;
    for _ in 0..4 {
        match count_after(&mut w, arrows, 4000).await {
            Some(n) => after = n,
            None => break,
        }
        if before - after >= 2 {
            break;
        }
    }
    anyhow::ensure!(before - after >= 2, "Dual Shot : {before} → {after} flèches (attendu 2 de moins au moins)");
    println!("  ✓ Dual Shot lancée : {before} → {after} flèches");
    println!("\nmunitions : OK");
    Ok(())
}

pub async fn death_strike(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    const DEATH_STRIKE: i64 = 74;
    let cfg = crate::config::load()?;
    let db = crate::db::Databases::new(&cfg.database);
    let act = &schema::OPT_CHAR_ACTION;
    let id = character_id(&db, char_name).await?;
    db.characters.exec("INSERT INTO character_skills (character_id, skill_tid, level) VALUES (?, ?, 20) ON DUPLICATE KEY UPDATE level = 20", (id, DEATH_STRIKE)).await?;
    step(&format!("{char_name} entre en jeu avec Death Strike niveau 20 (90 %)"));
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let me = w.cid as i64;
    super::hold_weapon(&mut w).await?;
    gm(&mut w, "/invincible 1").await?;
    step("coups normaux sur les Vers verts : résultat 13");
    let r = super::competences::hunt(&mut w, |q| q.is(act, None) && q.int(0) == Some(me) && q.int(2) == Some(13), 90, "Death Strike").await;
    db.characters.exec("DELETE FROM character_skills WHERE character_id = ? AND skill_tid = ?", (id, DEATH_STRIKE)).await?;
    let r = r?;
    println!("  ✓ résultat 13 sur {:#x}", r.int(1).unwrap_or(0));
    gm(&mut w, "/invincible 0").await?;
    println!("
Death Strike : OK");
    Ok(())
}

/// Prochaine quantité annoncée de la pile `iid` (AGPMITEM op 2), sous `ms`.
async fn count_after(w: &mut InWorld, iid: i64, ms: u64) -> Option<i64> {
    let it = &schema::ITEM;
    w.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(iid), ms, "flèches").await.ok().and_then(|q| q.int(5))
}

pub async fn lifetime(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let db = crate::db::Databases::new(&cfg.database);
    let it = &schema::ITEM;
    let id = character_id(&db, char_name).await?;
    db.characters.exec("DELETE FROM character_items WHERE character_id = ? AND template_id IN (?, ?)", (id, TALISMAN_DAY, TALISMAN_30)).await?;
    step(&format!("{char_name} entre en jeu"));
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    gm(&mut w, "/recast").await?;

    step("Talisman of Awakening <1 Day> : date fixée, objet gardé");
    let day = add_item(&mut w, TALISMAN_DAY, 1).await?;
    tokio::time::sleep(Duration::from_millis(300)).await;
    w.world.send(it, Fields::new(it).with(0, 3).with(2, day)).await?;
    let q = w.world.expect_where(|q| q.is(it, Some(26)) && q.int(2) == Some(day), 3000, "op 26 du talisman").await?;
    let (in_use, _, expire) = cash_of(&q).unwrap_or_default();
    let want = unix_now() + 1440 * 60;
    anyhow::ensure!(in_use == 1 && (expire - want).abs() <= 120, "mini Cash ({in_use}, date {expire}) : attendu en cours, date ≈ {want}");
    anyhow::ensure!(w.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(day), 1500, "retrait").await.is_err(), "le talisman a été consommé à l'usage");
    println!("  ✓ en cours d'utilisation, expire à {expire} (dans un jour), pas consommé");

    step("Talisman of Awakening <30 Minutes> : temps de jeu");
    let half = add_item(&mut w, TALISMAN_30, 1).await?;
    tokio::time::sleep(Duration::from_millis(300)).await;
    gm(&mut w, "/recast").await?;
    w.world.send(it, Fields::new(it).with(0, 3).with(2, half)).await?;
    let q = w.world.expect_where(|q| q.is(it, Some(26)) && q.int(2) == Some(half), 3000, "op 26 du talisman de 30 min").await?;
    let (in_use, remain, _) = cash_of(&q).unwrap_or_default();
    anyhow::ensure!(in_use == 1 && remain == 1_800_000, "mini Cash ({in_use}, reste {remain}) : attendu en cours, 1 800 000 ms");
    println!("  ✓ reste 1 800 000 ms");

    step("date passée en base, nouvelle entrée : message système 5, objet retiré");
    drop(w);
    tokio::time::sleep(Duration::from_millis(800)).await;
    db.characters.exec("UPDATE character_items SET expire_at = ? WHERE id = ?", (unix_now() - 5, day)).await?;
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let sm = &schema::SYSTEM_MESSAGE;
    let msg = w.world.expect_where(|q| q.is(sm, Some(0)) && q.int(1) == Some(5), 15_000, "message système 5").await?;
    println!("  ✓ message système 5 : {}", msg.fields().text(4).unwrap_or_default());
    w.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(day), 3000, "talisman retiré").await?;
    println!("  ✓ talisman d'un jour retiré");
    let kept: i64 = db.characters.first("SELECT remain_ms FROM character_items WHERE id = ?", (half,)).await?.map(|r| crate::db::col(&r, "remain_ms")).unwrap_or(0);
    anyhow::ensure!((1_700_000..=1_800_000).contains(&kept), "reste enregistré du talisman de 30 min : {kept}");
    println!("  ✓ talisman de 30 min : {kept} ms enregistrés");
    db.characters.exec("DELETE FROM character_items WHERE character_id = ? AND template_id IN (?, ?)", (id, TALISMAN_DAY, TALISMAN_30)).await?;
    println!("\nobjets à durée : OK");
    Ok(())
}
