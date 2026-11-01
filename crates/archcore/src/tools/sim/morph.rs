//! `archcore sim morph` : métamorphose par potion (AGPMCHARACTER ops 11 à 13, AGPMITEM ops 4, 19, 27).
//!
//! GmTest (MJ) reçoit trois Spider Potions (TID 406 : forme 272 Arachnoid Drone, dégâts +26 / +28 d'ItemTransformTable)
//! et un Passport (198) ; SimA le regarde. Potion bue : op 4, op 19 (recharge 600 000 ms), facteurs (dégâts majorés), op 11
//! chez lui et chez SimA. Seconde potion refusée (op 27 code 4), Passport refusé (message système). SimA revient en jeu :
//! il reçoit GmTest par l'op 0 (TID d'origine, bit 0 du champ 18) puis l'op 11. Fin de durée : la configuration du
//! simulateur (`Item.TransformMaxDurationMs = 8000`) l'abrège à 8 s → op 12 chez les deux, dégâts d'origine. GmTest revient
//! en jeu (la recharge ne survit pas à la session), boit une potion, l'annule (op 13) → op 12 chez les deux.
//! Sous la forme d'araignée : une Short Sword se porte (AGPMITEM op 2 statut 3) puis se range (statut 2), l'échange
//! demandé à SimA est refusé (AGPMPRIVATETRADE op 5, rien chez SimA).
//! Mutation d'un monstre : GmTest (Mutation niveau 1 posée en base) la lance sur un Ver vert → op 11 (CID du ver,
//! forme de la liste du niveau) ; le ver changé en animal n'est pas attaquable (aucun coup), op 12 au bout de 7 s ;
//! une seconde Mutation dans l'immunité qui suit (30 s, `downtime_duration`) ne le change plus.

use super::{cast_fields, character_id, enter_world, factors_combat, step, InWorld, Recv};
use archcore_net::schema::{self, chat_type};
use archcore_net::{Fields, Vec3};
use std::time::Duration;

const SPIDER_POTION: i64 = 406;
const SPIDER: i64 = 272;
const PASSPORT: i64 = 198;
/// Short Sword (guerrier, 11 à 13) ; Mutation (SKILL_SPEC 339) et les formes de son niveau 1.
const SWORD: i64 = 11;
const MUTATION: i64 = 339;
const MUTATION_FORMS: [i64; 10] = [100, 103, 104, 105, 106, 107, 109, 110, 113, 115];
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

/// AGPMITEM op 2 sur l'objet `iid` : statut demandé (3 porter, 2 ranger, case libre), attend la place rendue.
async fn place(w: &mut InWorld, iid: i64, status: i64) -> anyhow::Result<()> {
    let (it, grid) = (&schema::ITEM, &archcore_net::schema::mini::ITEM_GRID);
    let mut f = Fields::new(it).with(0, 2).with(1, status).with(2, iid);
    if status == 2 {
        f = f.with(7, archcore_net::build_mini(grid, &Fields::new(grid).with(0, -1).with(1, -1).with(2, -1)));
    }
    w.world.send(it, f).await?;
    w.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(iid) && q.int(1) == Some(status), 3000, "place de l'objet").await?;
    Ok(())
}

/// Commande MJ, attend la réponse du serveur.
async fn gm(w: &mut InWorld, text: &str) -> anyhow::Result<()> {
    let ch = &schema::CHATTING;
    w.world.send(ch, say(w, text)).await?;
    w.world.expect_where(|q| q.ptype == ch.id && q.int(2) == Some(chat_type::WHOLE_WORLD), 3000, text).await?;
    Ok(())
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
    let sword = add_item(&mut w, SWORD, 1).await?;
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

    step("équipement et échange sous métamorphose");
    place(&mut w, sword, 3).await?;
    place(&mut w, sword, 2).await?;
    println!("  ✓ Short Sword portée (statut 3) puis rangée (statut 2) sous la forme {SPIDER}");
    let tr = &schema::PRIVATE_TRADE;
    w.world.send(tr, Fields::new(tr).with(0, 1).with(1, me).with(2, witness.cid as i64)).await?;
    w.world.expect_where(|q| q.is(tr, Some(5)), 3000, "échange annulé").await?;
    anyhow::ensure!(witness.world.expect_where(|q| q.is(tr, Some(3)), 800, "").await.is_err(), "demande d'échange transmise sous métamorphose");
    println!("  ✓ échange refusé (op 5), rien chez {cb}");

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

    // Remise en état : potion restante, Passport et épée détruits.
    for iid in [potions, passport, sword] {
        w.world.send(it, Fields::new(it).with(0, 21).with(2, iid)).await?;
        w.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(iid), 3000, "objet détruit").await?;
    }
    drop(witness);
    drop(w);
    mutation(&cfg, user, pass, char_name).await?;
    println!("\nmétamorphose : OK");
    Ok(())
}

/// Mutation d'un Ver vert par GmTest : op 11, ver inattaquable, op 12 après 7 s, immunité ensuite.
async fn mutation(cfg: &crate::config::Config, user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    let (k, c, act) = (&schema::SKILL, &schema::CHARACTER, &schema::OPT_CHAR_ACTION);
    step("Mutation d'un monstre");
    let db = crate::db::Databases::new(&cfg.database);
    let id = character_id(&db, char_name).await?;
    db.characters.exec("INSERT INTO character_skills (character_id, skill_tid, level) VALUES (?, ?, 1) ON DUPLICATE KEY UPDATE level = 1", (id, MUTATION)).await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut w = enter_world(cfg, user, pass, char_name, "").await?;
    let (me, level) = (w.cid as i64, w.level.max(1));
    let skill = w.skills.iter().find(|s| s.1 == MUTATION).map(|s| s.0).ok_or_else(|| anyhow::anyhow!("Mutation absente"))?;
    gm(&mut w, "/level 60").await?;
    gm(&mut w, "/invincible 1").await?;
    let worm = super::competences::worms(&mut w).await?[0];
    w.world.send(k, cast_fields(skill, worm)).await?;
    let t = w.world.expect_where(|q| q.is(c, Some(11)) && q.int(1) == Some(worm), 10_000, "op 11 du ver").await?;
    let form = t.int(2).unwrap_or(0);
    anyhow::ensure!(MUTATION_FORMS.contains(&form), "forme {form} hors de la liste du niveau 1");
    println!("  ✓ op 11 : ver {worm:#x} en {form}");
    w.world.send(act, Fields::new(act).with(0, w.cid).with(1, worm).with(6, 1)).await?;
    let hit = w.world.expect_where(|q| q.is(act, None) && q.int(0) == Some(me) && q.int(1) == Some(worm), 2500, "").await;
    anyhow::ensure!(hit.is_err(), "coup porté sur le ver changé en animal");
    restored(&mut w, worm, 9000, "op 12 du ver").await?;
    println!("  ✓ aucun coup sur l'animal ; op 12 à la fin des 7 s");
    gm(&mut w, "/recast").await?;
    w.world.send(k, cast_fields(skill, worm)).await?;
    w.world.expect_where(|q| q.is(k, Some(8)) && q.int(1) == Some(skill), 8000, "second lancement").await?;
    anyhow::ensure!(w.world.expect_where(|q| q.is(c, Some(11)) && q.int(1) == Some(worm), 1500, "").await.is_err(), "Mutation pendant l'immunité");
    println!("  ✓ seconde Mutation lancée, sans effet pendant l'immunité");
    gm(&mut w, "/invincible 0").await?;
    gm(&mut w, &format!("/level {level}")).await?;
    db.characters.exec("DELETE FROM character_skills WHERE character_id = ? AND skill_tid = ?", (id, MUTATION)).await?;
    db.close().await;
    Ok(())
}
