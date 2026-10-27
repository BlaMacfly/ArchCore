//! `archcore sim product` : production (AgpmProduct 0x17, AgpmEventProduct 0x3A).
//!
//! Le MJ repart sans compétence ni recette de production (effacées en base), passe niveau 20 et reçoit de l'or, le
//! couteau de récolte (654) et les ustensiles de cuisine (655). Chez Kauman (Adventurer, Anchorville), il apprend
//! Collection Skill (op 2 → AGPMSKILL op 0 et op 3 code 0 ; une seconde fois : code 2). Au nord du village, il tue des
//! Vers verts (récoltables, ensemble 46) et récolte leur cadavre : lancement (AGPMSKILL op 7, action 1), op 2 → op 3
//! (objet, expérience, facteur « dirt » 6) ; le même cadavre ne se récolte pas deux fois (op 3 code 7, lancement :
//! action 17). Chez Lucy (Cook), il apprend Cooking, achète Broiled Meat of Vitality (644 ; avant Cooking : code 5),
//! fabrique sans matériaux (code 6) puis avec (op 0 → op 3, matériaux et recette consommés). À la reconnexion, l'op 1
//! rend la recette achetée. Compétences et recettes de production sont effacées à la fin.

use super::{cast_fields, character_id, enter_world, step, InWorld, Recv, SeenMonster, TestClient};
use archcore_net::schema::{self, chat_type, mini};
use archcore_net::{build_mini, decode_mini, Fields, Vec3};

/// PNJ de production d'Anchorville (`npc_spawn.id` 15 et 14).
const KAUMAN: i64 = 0x4000_0000 + 15;
const LUCY: i64 = 0x4000_0000 + 14;
/// Vers verts au nord d'Anchorville.
const HUNT: Vec3 = Vec3 { x: -466_468.0, y: 4_700.0, z: -50_010.0 };
const STOP: i64 = 0x04;
const COLLECTION: i64 = 663;
const COOKING: i64 = 203;
const GREEN_WORM: i64 = 450;
/// Ensemble 46 (Ver vert) aux niveaux 1 et 2 : Organ, Skin, Bug Meat.
const SET_46: [i64; 3] = [4972, 4927, 4960];
const BROILED_MEAT: i64 = 644;
const SPIDER_POTION: i64 = 406;
const SALTY_SEASONING: i64 = 623;

/// Mini EVENT_BASE de la fonction 24 d'un PNJ.
fn product_event(npc: i64) -> Vec<u8> {
    let eb = &mini::EVENT_BASE;
    build_mini(eb, &Fields::new(eb).with(0, 3).with(1, npc).with(3, 24))
}

/// Expérience cumulée (facteur « dirt » 6) d'un AGPMSKILL op 0 ou 2.
fn skill_exp(q: &Recv) -> Option<i64> {
    let f = decode_mini(&mini::CHAR_FACTORS, q.fields().bytes(3)?).ok()?;
    decode_mini(&mini::FACTOR_DIRT, f.bytes(11)?).ok()?.int(6)
}

/// Paquets reçus jusqu'au premier qui satisfait `last`, compris.
async fn until(w: &mut TestClient, last: impl Fn(&Recv) -> bool, ms: u64, what: &str) -> anyhow::Result<Vec<Recv>> {
    let mut got = Vec::new();
    loop {
        let q = w.expect_where(|_| true, ms, what).await?;
        let end = last(&q);
        got.push(q);
        if end {
            return Ok(got);
        }
    }
}

/// Commande MJ ; rend les paquets reçus jusqu'à sa réponse.
async fn gm(w: &mut InWorld, text: &str) -> anyhow::Result<Vec<Recv>> {
    let ch = &schema::CHATTING;
    w.world.send(ch, Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text))).await?;
    until(&mut w.world, |q| q.ptype == ch.id, 5000, &format!("réponse à {text}")).await
}

/// Demande au PNJ (op 2 ou 4) ; rend le code de sa réponse (op 3 ou 5, champ 4) et les paquets reçus jusque-là.
async fn npc_request(w: &mut InWorld, f: Fields, op: i64) -> anyhow::Result<(i64, Vec<Recv>)> {
    let ep = &schema::EVENT_PRODUCT;
    w.world.send(ep, f).await?;
    let got = until(&mut w.world, |q| q.is(ep, Some(op + 1)), 3000, &format!("op {}", op + 1)).await?;
    Ok((got.last().unwrap().int(4).unwrap_or(-1), got))
}

/// Objet reçu (AGPMITEM op 0 ou 2) de ce TID : IID.
fn item_iid(got: &[Recv], tid: i64) -> Option<i64> {
    got.iter().find(|q| q.ptype == schema::ITEM.id && matches!(q.int(0), Some(0 | 2)) && q.int(3) == Some(tid)).and_then(|q| q.int(2))
}

pub async fn run(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let (k, pr, ep, c, act, mv, ov, it) =
        (&schema::SKILL, &schema::PRODUCT, &schema::EVENT_PRODUCT, &schema::CHARACTER, &schema::OPT_CHAR_ACTION, &schema::OPT_CHAR_MOVE, &schema::OPT_VIEW, &schema::ITEM);

    step(&format!("préparation : {char_name} sans compétence ni recette de production"));
    let db = crate::db::Databases::new(&cfg.database);
    let id = character_id(&db, char_name).await?;
    let wipe = || async {
        db.characters.exec("DELETE FROM character_skills WHERE character_id = ? AND skill_tid IN (663, 203, 285)", (id,)).await?;
        db.characters.exec("DELETE FROM character_recipes WHERE character_id = ?", (id,)).await
    };
    wipe().await?;
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let me = w.cid as i64;
    let info = w.world.expect(pr, Some(1)).await?;
    anyhow::ensure!(info.int(1) == Some(me) && info.fields().bytes(7).is_none_or(|l| l.is_empty()), "op 1 : {:?}", info.fields().bytes(7));
    println!("  ✓ op 1 à l'entrée : aucune recette");
    let level = w.level.max(1);
    for cmd in ["/level 20", "/money 100000", "/invincible 1", "/additem 654 1", "/additem 655 1"] {
        gm(&mut w, cmd).await?;
    }

    step("Kauman (Adventurer) : Collection Skill");
    gm(&mut w, "/move -473400,-64100").await?;
    let ev = product_event(KAUMAN);
    let req = |op: i64, a: Option<i64>| Fields::new(ep).with(0, op).with(1, ev.clone()).with(2, me).with_opt(3, a);
    w.world.send(ep, req(0, None)).await?;
    w.world.expect_where(|q| q.is(ep, Some(1)), 3000, "accord de Kauman (op 1)").await?;
    println!("  ✓ fenêtre accordée (op 1)");
    w.world.send(ep, req(2, Some(COOKING))).await?;
    let r = w.world.expect_where(|q| q.is(ep, Some(3)), 3000, "op 3").await?;
    anyhow::ensure!(r.int(3) == Some(COOKING) && r.int(4) == Some(1), "Cooking chez Kauman : a {:?}, b {:?}", r.int(3), r.int(4));
    println!("  ✓ Cooking refusée chez l'Adventurer (code 1)");
    w.world.send(ep, req(2, Some(COLLECTION))).await?;
    let got = until(&mut w.world, |q| q.is(ep, Some(3)), 3000, "op 3").await?;
    anyhow::ensure!(got.last().unwrap().int(4) == Some(0), "Collection Skill : code {:?}", got.last().unwrap().int(4));
    let add = got.iter().find(|q| q.is(k, Some(0)) && q.int(4) == Some(COLLECTION)).ok_or_else(|| anyhow::anyhow!("AGPMSKILL op 0 de Collection Skill absent"))?;
    let collect = add.int(1).unwrap_or(0);
    anyhow::ensure!(skill_exp(add) == Some(0), "expérience de départ {:?}", skill_exp(add));
    let money = got.iter().rev().find(|q| q.is(c, Some(1)) && q.int(8).is_some()).and_then(|q| q.int(8));
    anyhow::ensure!(money == Some(100_000 - 300), "or après l'apprentissage : {money:?}");
    println!("  ✓ apprise (identifiant {collect}), 300 or débités");
    w.world.send(ep, req(2, Some(COLLECTION))).await?;
    let r = w.world.expect_where(|q| q.is(ep, Some(3)), 3000, "op 3").await?;
    anyhow::ensure!(r.int(4) == Some(2), "seconde demande : code {:?}", r.int(4));
    println!("  ✓ seconde demande : déjà apprise (code 2)");

    step("Vers verts : tuer et récolter");
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, HUNT).with(2, HUNT).with(5, STOP).with(6, 0)).await?;
    let mut seen: Vec<SeenMonster> = w.monsters.clone();
    while let Ok(p) = w.world.expect_where(|q| q.is(ov, Some(1)), 1500, "monstres").await {
        if let Ok(view) = decode_mini(&mini::OPT_VIEW_CHAR, p.fields().bytes(1).unwrap_or_default()) {
            if view.int(0).unwrap_or(0) >= crate::world::monsters::MONSTER_CID_BASE as i64 {
                seen.push(SeenMonster::from_view(&view));
            }
        }
    }
    let dist = |p: Vec3| (((p.x - HUNT.x) as f64).powi(2) + ((p.z - HUNT.z) as f64).powi(2)).sqrt();
    let mut worms: Vec<SeenMonster> = seen.into_iter().filter(|m| m.tid == GREEN_WORM && m.hp > 0).collect();
    worms.sort_by_key(|m| dist(m.pos) as i64);
    worms.dedup_by_key(|m| m.cid);
    anyhow::ensure!(!worms.is_empty(), "aucun Ver vert en vue");
    let mut harvested = None;
    for (n, worm) in worms.iter().take(6).enumerate() {
        let tcid = worm.cid as i64;
        w.world.send(act, Fields::new(act).with(0, w.cid).with(1, worm.cid).with(6, 1)).await?;
        w.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(tcid) && q.int(11) == Some(2), 60_000, "mort du Ver vert").await?;
        println!("  Ver vert {tcid:#x} tué");
        w.world.send(k, cast_fields(collect, tcid)).await?;
        let cast = w.world.expect_where(|q| q.is(k, Some(7)) && q.int(1) == Some(collect), 15_000, "lancement de la récolte").await?;
        let action = decode_mini(&mini::SKILL_ACTION, cast.fields().bytes(6).unwrap_or_default())?;
        anyhow::ensure!(action.int(0) == Some(1), "lancement : action {:?}", action.int(0));
        w.world.send(pr, Fields::new(pr).with(0, 2).with(1, me).with(2, collect).with(3, tcid)).await?;
        let got = until(&mut w.world, |q| q.is(pr, Some(3)), 5000, "résultat de la récolte (op 3)").await?;
        let r = got.last().unwrap();
        match r.int(5) {
            Some(0) => {
                let tid = r.int(3).unwrap_or(0);
                anyhow::ensure!(SET_46.contains(&tid) && r.int(4) == Some(1), "objet récolté {tid} × {:?}", r.int(4));
                anyhow::ensure!(item_iid(&got, tid).is_some(), "objet {tid} absent de l'inventaire");
                let gain = r.int(6).unwrap_or(0);
                let update = got.iter().find(|q| q.is(k, Some(2)) && q.int(1) == Some(collect)).ok_or_else(|| anyhow::anyhow!("AGPMSKILL op 2 absent"))?;
                anyhow::ensure!((5..=7).contains(&gain) && skill_exp(update) == Some(gain), "expérience {gain}, facteur « dirt » 6 {:?}", skill_exp(update));
                println!("  ✓ récolte réussie : objet {tid}, +{gain} d'expérience (facteur « dirt » 6 = {gain})");
                harvested = Some(tid);
            }
            Some(1) => println!("  · récolte manquée (code 1)"),
            other => anyhow::bail!("récolte : code {other:?}"),
        }
        if n == 0 {
            w.world.send(pr, Fields::new(pr).with(0, 2).with(1, me).with(2, collect).with(3, tcid)).await?;
            let again = w.world.expect_where(|q| q.is(pr, Some(3)), 3000, "seconde récolte").await?;
            anyhow::ensure!(again.int(5) == Some(7), "seconde récolte : code {:?}", again.int(5));
            // Le premier lancement se termine au tour suivant du monde (le client attend, lui, 5,4 s).
            tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
            w.world.send(k, cast_fields(collect, tcid)).await?;
            let refused = w.world.expect_where(|q| q.is(k, Some(7)) && q.int(1) == Some(collect), 3000, "lancement refusé").await?;
            let action = decode_mini(&mini::SKILL_ACTION, refused.fields().bytes(6).unwrap_or_default())?;
            anyhow::ensure!(action.int(0) == Some(17), "relance sur le cadavre : action {:?}", action.int(0));
            println!("  ✓ cadavre déjà récolté : op 3 code 7, lancement refusé (action 17)");
        }
        if harvested.is_some() {
            break;
        }
    }
    anyhow::ensure!(harvested.is_some(), "aucune récolte réussie sur {} Vers verts", worms.len().min(6));

    step("Lucy (Cook) : Cooking et recette");
    gm(&mut w, "/move -476300,-69900").await?;
    let ev = product_event(LUCY);
    let req = |op: i64, a: Option<i64>| Fields::new(ep).with(0, op).with(1, ev.clone()).with(2, me).with_opt(3, a);
    w.world.send(ep, req(0, None)).await?;
    w.world.expect_where(|q| q.is(ep, Some(1)), 3000, "accord de Lucy (op 1)").await?;
    let (b, _) = npc_request(&mut w, req(4, Some(BROILED_MEAT)), 4).await?;
    anyhow::ensure!(b == 5, "recette avant Cooking : code {b}");
    println!("  ✓ recette refusée sans Cooking (code 5)");
    let (b, got) = npc_request(&mut w, req(2, Some(COOKING)), 2).await?;
    let cooking = got.iter().find(|q| q.is(k, Some(0)) && q.int(4) == Some(COOKING)).and_then(|q| q.int(1));
    anyhow::ensure!(b == 0 && cooking.is_some(), "Cooking : code {b}");
    let cooking = cooking.unwrap();
    println!("  ✓ Cooking apprise (identifiant {cooking})");
    let (b, _) = npc_request(&mut w, req(4, Some(BROILED_MEAT)), 4).await?;
    anyhow::ensure!(b == 0, "achat de la recette : code {b}");
    let (b, _) = npc_request(&mut w, req(4, Some(BROILED_MEAT)), 4).await?;
    anyhow::ensure!(b == 2, "second achat : code {b}");
    let (b, _) = npc_request(&mut w, req(4, Some(SPIDER_POTION)), 4).await?;
    anyhow::ensure!(b == 1, "recette d'alchimie chez la cuisinière : code {b}");
    println!("  ✓ Broiled Meat of Vitality achetée (code 0), puis déjà connue (2) ; potion d'alchimie refusée (1)");

    step("fabrication");
    let compose = |iid: i64| Fields::new(pr).with(0, 0).with(1, me).with(2, cooking).with(3, BROILED_MEAT).with(4, iid);
    w.world.send(pr, compose(0)).await?;
    let r = w.world.expect_where(|q| q.is(pr, Some(3)), 3000, "fabrication sans matériaux").await?;
    anyhow::ensure!(r.int(5) == Some(6), "sans matériaux : code {:?}", r.int(5));
    println!("  ✓ sans matériaux : code 6");
    for cmd in ["/additem 4960 12", "/additem 4963 4", "/additem 641 4"] {
        gm(&mut w, cmd).await?;
    }
    let seasoning = item_iid(&gm(&mut w, &format!("/additem {SALTY_SEASONING} 1")).await?, SALTY_SEASONING).ok_or_else(|| anyhow::anyhow!("assaisonnement non reçu"))?;
    let mut made = false;
    for attempt in 0..4 {
        let iid = if attempt == 0 { seasoning } else { 0 };
        w.world.send(pr, compose(iid)).await?;
        let got = until(&mut w.world, |q| q.is(pr, Some(3)), 5000, "résultat de la fabrication").await?;
        let r = got.last().unwrap();
        let used = got.iter().filter(|q| q.ptype == it.id && matches!(q.int(0), Some(1 | 2))).count();
        anyhow::ensure!(used >= 3, "matériaux non consommés ({used} mise(s) à jour)");
        if iid != 0 {
            anyhow::ensure!(got.iter().any(|q| q.is(it, Some(1)) && q.int(2) == Some(iid)), "assaisonnement non consommé");
            println!("  ✓ assaisonnement {iid:#x} consommé avec la recette");
        }
        match r.int(5) {
            Some(0) => {
                anyhow::ensure!(r.int(3) == Some(BROILED_MEAT) && item_iid(&got, BROILED_MEAT).is_some(), "produit {:?}", r.int(3));
                let update = got.iter().find(|q| q.is(k, Some(2)) && q.int(1) == Some(cooking));
                anyhow::ensure!(r.int(6).is_some_and(|e| e > 0) && update.and_then(skill_exp) == r.int(6), "expérience {:?}", r.int(6));
                println!("  ✓ fabrication réussie (essai {}) : Broiled Meat of Vitality, +{} d'expérience", attempt + 1, r.int(6).unwrap_or(0));
                made = true;
                break;
            }
            Some(1) => println!("  · fabrication manquée (code 1), matériaux perdus"),
            other => anyhow::bail!("fabrication : code {other:?}"),
        }
    }
    anyhow::ensure!(made, "aucune fabrication réussie en 4 essais");

    step("reconnexion : recettes et compétences relues");
    drop(w);
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let info = w.world.expect(pr, Some(1)).await?;
    let list: Vec<i32> = info.fields().bytes(7).unwrap_or_default().chunks(4).map(|b| i32::from_le_bytes(b.try_into().unwrap_or([0; 4]))).collect();
    anyhow::ensure!(list == vec![BROILED_MEAT as i32], "op 1 : {list:?}");
    anyhow::ensure!(w.skills.iter().any(|s| s.1 == COLLECTION) && w.skills.iter().any(|s| s.1 == COOKING), "compétences : {:?}", w.skills);
    println!("  ✓ op 1 : {list:?} ; Collection Skill et Cooking connues");
    for cmd in [format!("/level {level}"), "/invincible 0".into(), "/move -476000,-61000".into()] {
        gm(&mut w, &cmd).await?;
    }
    drop(w);
    wipe().await?;
    println!("\nproduction : OK");
    Ok(())
}
