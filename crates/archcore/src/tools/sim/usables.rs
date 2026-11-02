//! `archcore sim usables` : consommables sans compétence de l'épisode 8 (`world/consumables.rs`, `bank.rs`, `convert.rs`).
//!
//! GmTest (MJ) part avec Dual Slash au niveau 2 et sans page de banque en plus ; SimA est posé à côté de lui (compétences,
//! pages, signature et argent de GmTest remis à la fin). Return Orb (1171) : AGPMSKILL op 13 sur l'attaque normale
//! refusé (message), sur Dual Slash → op 1, orbe consommée (op 4) ; sans orbe, refus. Secret Safe (3916) refusé sans
//! deuxième page ; `/safekeeper` (4923) ouvre la banque à distance (AGPMEVENTBANK op 1), où la page 2 s'achète (op 22) ;
//! deux Secret Safe ouvrent les pages 3 et 4 (champ 23 = 2 puis 3, message 30), le troisième est refusé (message 29).
//! `/merchant` (4924) : liste d'une boutique (0x32 op 1). Release of Agreement (4877) sur une Bastard Sword +1 à 2 sockets
//! sertis : AGPMITEMCONVERT op 0 (renforcement 1, 2 sockets, rien de serti), objet consommé ; second essai sans effet.
//! Small firecracker (4606) : op 4 chez GmTest et chez SimA, second refusé (op 5). Fortune Manual (4569) et Talisman of
//! Protection (1842) : op 26 allumé puis éteint (op 3, op 23). `/nick` : 17 caractères refusés, « Héros » vu par les deux
//! (AGPMCHARACTER op 1 champ 27), en base, puis « Il faut » sans objet. `/go` : joueur absent refusé, puis GmTest
//! éloigné rejoint SimA (téléportation), recharge refusée.

use super::{character_id, enter_world, gm_place, step, InWorld, Recv};
use archcore_net::schema::{self, chat_type, mini};
use archcore_net::{build_mini, decode_mini, Fields, Vec3};
use std::time::Duration;

const SPOT: Vec3 = Vec3 { x: -476_000.0, y: 4_422.0, z: -61_000.0 };
const FAR: Vec3 = Vec3 { x: -470_000.0, y: 4_422.0, z: -61_000.0 };
const DUAL_SLASH: i64 = 1;
const NORMAL_ATTACK: i64 = 222;
const RETURN_ORB: i64 = 1171;
const SECRET_SAFE: i64 = 3916;
const SAFE_KEEPER: i64 = 4923;
const MERCHANT: i64 = 4924;
const SWORD: i64 = 2;
const RELEASE: i64 = 4877;
const FIRECRACKER: i64 = 4606;
const FORTUNE: i64 = 4569;
const TALISMAN: i64 = 1842;
const SIGNATURE: i64 = 4875;
const GATE: i64 = 4568;

fn say(w: &InWorld, text: &str) -> Fields {
    let ch = &schema::CHATTING;
    Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text))
}

/// Message de discussion (une toutes les 100 ms au plus, chat.rs).
async fn talk(w: &mut InWorld, text: &str) -> anyhow::Result<()> {
    tokio::time::sleep(Duration::from_millis(150)).await;
    w.world.send(&schema::CHATTING, say(w, text)).await
}

/// Message système reçu (type « monde entier ») contenant `part`.
async fn system(w: &mut InWorld, part: &str) -> anyhow::Result<String> {
    let ch = &schema::CHATTING;
    let r = w.world.expect_where(|q| q.ptype == ch.id && q.int(2) == Some(chat_type::WHOLE_WORLD) && q.fields().text(6).is_some_and(|t| t.contains(part)), 3000, part).await?;
    Ok(r.fields().text(6).unwrap_or_default())
}

/// Objet créé par /additem (inventaire 2 ou Cash 51) : son IID.
async fn add_item(w: &mut InWorld, tid: i64) -> anyhow::Result<i64> {
    let it = &schema::ITEM;
    talk(w, &format!("/additem {tid} 1")).await?;
    Ok(w.world.expect_where(|q| q.is(it, None) && q.int(3) == Some(tid) && matches!(q.int(1), Some(2 | 51)), 3000, "objet ajouté").await?.int(2).unwrap_or(0))
}

async fn use_item(w: &mut InWorld, iid: i64) -> anyhow::Result<()> {
    let it = &schema::ITEM;
    w.world.send(it, Fields::new(it).with(0, 3).with(2, iid)).await
}

/// Op 4 de l'objet `tid` pour le joueur `cid`.
async fn used(w: &mut InWorld, tid: i64, cid: i64) -> anyhow::Result<Recv> {
    let it = &schema::ITEM;
    w.world.expect_where(|q| q.is(it, Some(4)) && q.int(3) == Some(tid) && q.int(4) == Some(cid), 3000, "op 4 de l'objet").await
}

/// AGPMITEM op 26 de l'objet `iid` : octet « en cours d'utilisation » du mini Cash.
async fn lit(w: &mut InWorld, iid: i64) -> anyhow::Result<i64> {
    let it = &schema::ITEM;
    let r = w.world.expect_where(|q| q.is(it, Some(26)) && q.int(2) == Some(iid), 3000, "op 26").await?;
    Ok(r.fields().bytes(23).and_then(|b| decode_mini(&mini::ITEM_CASH, b).ok()).and_then(|m| m.int(0)).unwrap_or(-1))
}

pub async fn run(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let db = crate::db::Databases::new(&cfg.database);
    let id = character_id(&db, ca).await?;
    let account: u32 = db.characters.first("SELECT account_id FROM characters WHERE id = ?", (id,)).await?.map(|r| crate::db::col(&r, "account_id")).unwrap_or(0);
    let saved: Vec<(u32, u32, u32)> = db
        .characters
        .rows("SELECT skill_tid, level, exp FROM character_skills WHERE character_id = ?", (id,))
        .await?
        .iter()
        .map(|r| (crate::db::col(r, "skill_tid"), crate::db::col(r, "level"), crate::db::col(r, "exp")))
        .collect();
    let money: i64 = db.characters.first("SELECT money FROM characters WHERE id = ?", (id,)).await?.map(|r| crate::db::col(&r, "money")).unwrap_or(0);
    let pages: i64 = db.characters.first("SELECT extra_slots FROM account_bank WHERE account_id = ?", (account,)).await?.map(|r| crate::db::col(&r, "extra_slots")).unwrap_or(0);
    db.characters.exec("DELETE FROM character_skills WHERE character_id = ?", (id,)).await?;
    db.characters.exec("INSERT INTO character_skills (character_id, skill_tid, level) VALUES (?, ?, 2)", (id, DUAL_SLASH)).await?;
    db.characters.exec("UPDATE account_bank SET extra_slots = 0 WHERE account_id = ?", (account,)).await?;
    // Bastard Sword +1, deux sockets sertis (pierre d'eau, pierre de prière), pour le Release of Agreement.
    let sword = db
        .characters
        .exec("INSERT INTO character_items (character_id, template_id, status, layer, row_idx, col_idx, count, convert_level, socket_count, socket_tids) VALUES (?, ?, 0, 3, 3, 3, 1, 1, 2, '201,463')", (id, SWORD))
        .await? as i64;
    let result = Box::pin(scenario(&cfg, &db, id, sword, (ua, pa, ca), (ub, pb, cb))).await;
    db.characters.exec("DELETE FROM character_skills WHERE character_id = ?", (id,)).await?;
    for (tid, level, exp) in &saved {
        db.characters.exec("INSERT INTO character_skills (character_id, skill_tid, level, exp) VALUES (?, ?, ?, ?)", (id, *tid, *level, *exp)).await?;
    }
    db.characters.exec("UPDATE characters SET money = ?, signature = '' WHERE id = ?", (money, id)).await?;
    db.characters.exec("UPDATE account_bank SET extra_slots = ? WHERE account_id = ?", (pages, account)).await?;
    db.characters.exec("DELETE FROM character_items WHERE id = ?", (sword,)).await?;
    println!("\n  compétences ({}), argent, pages de banque ({pages}) et signature de {ca} remis", saved.len());
    result
}

async fn scenario(cfg: &crate::config::Config, db: &crate::db::Databases, id: u32, sword: i64, a: (&str, &str, &str), b: (&str, &str, &str)) -> anyhow::Result<()> {
    let (k, it, c, ic) = (&schema::SKILL, &schema::ITEM, &schema::CHARACTER, &schema::ITEM_CONVERT);
    let (mv, sm, bk, tr) = (&schema::OPT_CHAR_MOVE, &schema::SYSTEM_MESSAGE, &schema::EVENT_BANK, &schema::EVENT_NPC_TRADE);
    step(&format!("{} et {} entrent en jeu au même endroit", a.2, b.2));
    let mut w = enter_world(cfg, a.0, a.1, a.2, "A-").await?;
    let mut s = enter_world(cfg, b.0, b.1, b.2, "B-").await?;
    gm_place(&mut w, a.2, SPOT).await?;
    gm_place(&mut w, b.2, SPOT).await?;
    for x in [&mut w, &mut s] {
        let cid = x.cid;
        x.world.send(mv, Fields::new(mv).with(0, cid).with(1, SPOT).with(2, SPOT).with(5, 0x04).with(6, 0)).await?;
    }
    let (me, other) = (w.cid as i64, s.cid as i64);
    talk(&mut w, "/recast").await?;

    step("Return Orb : attaque normale refusée, Dual Slash oubliée, plus d'orbe");
    let slash = w.skills.iter().find(|x| x.1 == DUAL_SLASH).map(|x| x.0).ok_or_else(|| anyhow::anyhow!("Dual Slash absente : {:?}", w.skills))?;
    let normal = w.skills.iter().find(|x| x.1 == NORMAL_ATTACK).map(|x| x.0).ok_or_else(|| anyhow::anyhow!("attaque normale absente"))?;
    let orb = add_item(&mut w, RETURN_ORB).await?;
    let base = build_mini(&mini::SKILL_BASE, &Fields::new(&mini::SKILL_BASE).with(0, 1).with(1, me));
    w.world.send(k, Fields::new(k).with(0, 13).with(1, normal).with(2, base.clone())).await?;
    system(&mut w, "ne s'oublie pas").await?;
    w.world.send(k, Fields::new(k).with(0, 13).with(1, slash).with(2, base.clone())).await?;
    w.world.expect_where(|q| q.is(k, Some(1)) && q.int(1) == Some(slash), 3000, "Dual Slash retirée (op 1)").await?;
    w.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(orb), 3000, "orbe consommée").await?;
    used(&mut w, RETURN_ORB, me).await?;
    let left: Option<u32> = db.characters.first("SELECT skill_tid FROM character_skills WHERE character_id = ? AND skill_tid = ?", (id, DUAL_SLASH)).await?.map(|r| crate::db::col(&r, "skill_tid"));
    anyhow::ensure!(left.is_none(), "Dual Slash encore en base");
    w.world.send(k, Fields::new(k).with(0, 13).with(1, slash).with(2, base)).await?;
    system(&mut w, "Impossible d'oublier").await?;
    println!("  ✓ refus sur l'attaque normale ; Dual Slash : op 1, orbe consommée (op 4) ; ensuite refus");

    step("Secret Safe sans deuxième page, /safekeeper, page 2 achetée, pages 3 et 4, plafond");
    let safe1 = add_item(&mut w, SECRET_SAFE).await?;
    use_item(&mut w, safe1).await?;
    system(&mut w, "deuxième page").await?;
    println!("  ✓ refusé sans deuxième page");
    add_item(&mut w, SAFE_KEEPER).await?;
    talk(&mut w, "/safekeeper").await?;
    used(&mut w, SAFE_KEEPER, me).await?;
    w.world.expect_where(|q| q.is(bk, Some(1)), 3000, "banque ouverte à distance (op 1)").await?;
    talk(&mut w, "/money 1500000").await?;
    w.world.expect_where(|q| q.is(c, Some(1)) && q.int(8) == Some(1_500_000), 3000, "argent").await?;
    w.world.send(it, Fields::new(it).with(0, 22).with(4, w.cid)).await?;
    let up = w.world.expect_where(|q| q.is(c, Some(1)) && q.int(23).is_some(), 3000, "page 2 achetée").await?;
    anyhow::ensure!(up.int(23) == Some(1), "pages {:?}", up.int(23));
    println!("  ✓ /safekeeper : banque ouverte à distance, page 2 achetée");
    for (n, safe) in [(2, Some(safe1)), (3, None)] {
        let safe = match safe {
            Some(iid) => iid,
            None => add_item(&mut w, SECRET_SAFE).await?,
        };
        use_item(&mut w, safe).await?;
        w.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(safe), 3000, "Secret Safe consommé").await?;
        let up = w.world.expect_where(|q| q.is(c, Some(1)) && q.int(23).is_some(), 3000, "page en plus").await?;
        anyhow::ensure!(up.int(23) == Some(n), "pages {:?} au lieu de {n}", up.int(23));
        w.world.expect_where(|q| q.is(sm, Some(0)) && q.int(1) == Some(30), 3000, "message 30").await?;
    }
    let safe3 = add_item(&mut w, SECRET_SAFE).await?;
    use_item(&mut w, safe3).await?;
    w.world.expect_where(|q| q.is(sm, Some(0)) && q.int(1) == Some(29), 3000, "message 29").await?;
    let pages: i64 = db.characters.first("SELECT b.extra_slots FROM account_bank b JOIN characters c ON c.account_id = b.account_id WHERE c.id = ?", (id,)).await?.map(|r| crate::db::col(&r, "extra_slots")).unwrap_or(0);
    anyhow::ensure!(pages == 3, "{pages} page(s) en plus en base");
    println!("  ✓ pages 3 et 4 (champ 23 = 2, 3 ; message 30), plafond : message 29 ; 3 pages en plus en base");

    step("/merchant : boutique à distance");
    add_item(&mut w, MERCHANT).await?;
    talk(&mut w, "/merchant").await?;
    used(&mut w, MERCHANT, me).await?;
    let mut listed = 0;
    loop {
        let p = w.world.expect_where(|q| q.ptype == tr.id && q.packet.is_some(), 3000, "liste de la boutique").await?;
        match p.fields().int(1) {
            Some(0) => listed += 1,
            Some(1) => break,
            _ => {}
        }
    }
    anyhow::ensure!(listed > 0, "boutique vide");
    println!("  ✓ {listed} objet(s) de la boutique la plus proche");

    step("Release of Agreement : pierres retirées, sockets et renforcement gardés");
    anyhow::ensure!(w.converts.iter().any(|x| x.0 == sword && x.3 == 2), "épée sertie absente à l'entrée : {:?}", w.converts);
    let release = add_item(&mut w, RELEASE).await?;
    w.world.send(ic, Fields::new(ic).with(0, 12).with(1, me).with(2, sword).with(3, release)).await?;
    w.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(release), 3000, "objet consommé").await?;
    let st = w.world.expect_where(|q| q.is(ic, Some(0)) && q.int(2) == Some(sword), 3000, "état de l'épée (op 0)").await?;
    anyhow::ensure!(st.int(5) == Some(1) && st.int(6) == Some(2) && st.int(7) == Some(0), "renforcement {:?}, sockets {:?}, sertis {:?}", st.int(5), st.int(6), st.int(7));
    let release = add_item(&mut w, RELEASE).await?;
    w.world.send(ic, Fields::new(ic).with(0, 12).with(1, me).with(2, sword).with(3, release)).await?;
    anyhow::ensure!(w.world.expect_where(|q| q.is(ic, Some(0)) && q.int(2) == Some(sword), 800, "").await.is_err(), "épée vide de nouveau libérée");
    println!("  ✓ op 0 (+1, 2 sockets, rien de serti), objet consommé ; épée vide : sans effet");

    step("feu d'artifice vu par SimA, recharge");
    let fire = add_item(&mut w, FIRECRACKER).await?;
    talk(&mut w, &format!("/additem {FIRECRACKER} 1")).await?;
    use_item(&mut w, fire).await?;
    used(&mut w, FIRECRACKER, me).await?;
    used(&mut s, FIRECRACKER, me).await?;
    use_item(&mut w, fire).await?;
    w.world.expect_where(|q| q.is(it, Some(5)) && q.int(3) == Some(FIRECRACKER), 3000, "second feu refusé (op 5)").await?;
    println!("  ✓ op 4 chez GmTest et SimA ; second refusé (op 5)");

    step("Fortune Manual et Talisman of Protection : allumés, éteints");
    let fortune = add_item(&mut w, FORTUNE).await?;
    use_item(&mut w, fortune).await?;
    anyhow::ensure!(lit(&mut w, fortune).await? == 1, "Fortune Manual pas allumé");
    use_item(&mut w, fortune).await?;
    anyhow::ensure!(lit(&mut w, fortune).await? == 0, "Fortune Manual pas éteint");
    let talisman = add_item(&mut w, TALISMAN).await?;
    use_item(&mut w, talisman).await?;
    anyhow::ensure!(lit(&mut w, talisman).await? == 1, "talisman pas allumé");
    w.world.send(it, Fields::new(it).with(0, 23).with(2, talisman)).await?;
    anyhow::ensure!(lit(&mut w, talisman).await? == 0, "talisman pas éteint par l'op 23");
    println!("  ✓ op 26 : 1 puis 0 (op 3 et op 23)");

    step("/nick : refus, signature vue des deux, en base, sans objet");
    add_item(&mut w, SIGNATURE).await?;
    talk(&mut w, "/nick abcdefghijklmnopq").await?;
    system(&mut w, "impossible").await?;
    talk(&mut w, "/nick Héros").await?;
    used(&mut w, SIGNATURE, me).await?;
    let mine = w.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(me) && q.fields().text(27).is_some(), 3000, "signature (soi)").await?;
    let seen = s.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(me) && q.fields().text(27).is_some(), 3000, "signature (SimA)").await?;
    anyhow::ensure!(mine.fields().text(27).as_deref() == Some("Héros") && seen.fields().text(27).as_deref() == Some("Héros"), "signature {:?} / {:?}", mine.fields().text(27), seen.fields().text(27));
    let saved: String = db.characters.first("SELECT signature FROM characters WHERE id = ?", (id,)).await?.map(|r| crate::db::col(&r, "signature")).unwrap_or_default();
    anyhow::ensure!(saved == "Héros", "signature en base {saved:?}");
    talk(&mut w, "/nick Autre").await?;
    system(&mut w, "Il faut").await?;
    println!("  ✓ 17 caractères refusés ; « Héros » (champ 27) chez les deux, en base ; sans objet : « Il faut »");

    step("/go : absent refusé, GmTest rejoint SimA, recharge");
    add_item(&mut w, GATE).await?;
    talk(&mut w, "/go Personne").await?;
    system(&mut w, "pas en jeu").await?;
    gm_place(&mut w, a.2, FAR).await?;
    talk(&mut w, &format!("/go {}", b.2)).await?;
    used(&mut w, GATE, me).await?;
    let m = w.world.expect_where(|q| q.is(mv, None) && q.int(0) == Some(me) && q.fields().vec3(2).is_some_and(|p| (p.x - SPOT.x).abs() < 50.0), 3000, "téléporté près de SimA").await?;
    let at = m.fields().vec3(2).unwrap_or_default();
    add_item(&mut w, GATE).await?;
    talk(&mut w, &format!("/go {}", b.2)).await?;
    system(&mut w, "pas encore prêt").await?;
    println!("  ✓ « pas en jeu » ; arrivé en {:.0}, {:.0} près de SimA ({other:#x}) ; recharge", at.x, at.z);
    println!("\nobjets sans compétence : OK");
    Ok(())
}
