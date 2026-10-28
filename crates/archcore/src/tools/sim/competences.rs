//! Scénarios des compétences et des objets : points héroïques et charisme, partage des dégâts, purges, défense
//! ignorée, runes à compétence, options à déclenchement.
//!
//! ```text
//!   archcore sim heroic  [compte] [mdp] [perso]       points héroïques (niveaux 89, 100, 120) et charisme (/charisma)
//!   archcore sim divide  [A… B…]                      Martyr's Mark de GmTest : il prend une part des coups reçus par SimA
//!   archcore sim dispel  [compte] [mdp] [perso]       Antimagic retire de Blade Sting d'un Ver vert (AGPMSKILL op 5)
//!   archcore sim ignore  [compte] [mdp] [perso]       Stunner : la part d'eau d'un coup passe de 3 à 4 contre un ver résistant à 50 %
//!   archcore sim rune    [compte] [mdp] [perso]       pierre de prière de glace sertie : Cold Prayer Stone part sur un coup (op 8, puis op 4)
//!   archcore sim trigger [compte] [mdp] [perso]       option « 28 % d'étourdir 3 s » : un Ver vert étourdi (champ 17)
//! ```
//! Comptes MJ requis (GmTest). Les compétences et les objets sont préparés en base (personnage hors jeu), puis
//! retirés à la fin.

use super::{cast_fields, character_id, enter_world, factors_hp, o, skill_owner, step, InWorld, Recv};
use archcore_net::schema::{self, chat_type, mini};
use crate::config::Config;
use archcore_net::{decode_mini, Fields, Vec3};
use std::future::Future;
use std::pin::Pin;
use std::collections::HashSet;
use std::time::Duration;

/// Coin des Vers verts au nord d'Anchorville (résistances élémentaires 50 %).
const HUNT: Vec3 = Vec3 { x: -466_468.0, y: 4_700.0, z: -50_010.0 };
const STOP: i64 = 0x04;
/// Short Sword (guerrier, 11 à 13).
const SWORD: i64 = 11;

fn say(cid: u32, text: &str) -> Fields {
    let ch = &schema::CHATTING;
    Fields::new(ch).with(0, 0).with(1, cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text))
}

fn system(q: &Recv) -> bool {
    q.ptype == schema::CHATTING.id && q.int(2) == Some(chat_type::WHOLE_WORLD)
}

/// Commande MJ, attend la réponse du serveur.
async fn gm(w: &mut InWorld, text: &str) -> anyhow::Result<String> {
    w.world.send(&schema::CHATTING, say(w.cid, text)).await?;
    let r = w.world.expect_where(system, 3000, text).await?;
    Ok(r.fields().text(6).unwrap_or_default())
}

/// Valeur `i` d'un sous-paquet de facteurs (`part` : 1 statut, 3 points, 11 « dirt »).
fn factor(bytes: &[u8], part: usize, i: usize) -> Option<i64> {
    let f = decode_mini(&mini::CHAR_FACTORS, bytes).ok()?;
    let schema = match part {
        1 => &mini::FACTOR_STATUS,
        3 => &mini::FACTOR_POINT,
        11 => &mini::FACTOR_DIRT,
        _ => return None,
    };
    decode_mini(schema, f.bytes(part)?).ok()?.int(i)
}

/// Dégâts de nature `i` d'un résultat (sous-paquet 0 des facteurs, points, en négatif).
fn shown_damage(bytes: &[u8], i: usize) -> Option<i64> {
    let f = decode_mini(&mini::CHAR_FACTORS, bytes).ok()?;
    let result = decode_mini(&mini::CHAR_FACTORS, f.bytes(0)?).ok()?;
    decode_mini(&mini::FACTOR_POINT, result.bytes(3)?).ok()?.int(i).map(|d| -d)
}

/// Mes facteurs (AGPMCHARACTER op 1, champ 7).
fn mine(q: &Recv, me: i64) -> bool {
    q.is(&schema::CHARACTER, Some(1)) && q.int(1) == Some(me) && q.fields().bytes(7).is_some()
}

/// Va chez les Vers verts ; rend les vers vivants en vue, les plus valides d'abord.
async fn worms(w: &mut InWorld) -> anyhow::Result<Vec<i64>> {
    let mv = &schema::OPT_CHAR_MOVE;
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, HUNT).with(2, HUNT).with(5, STOP).with(6, 0)).await?;
    let near = |p: Vec3| (((p.x - HUNT.x) as f64).powi(2) + ((p.z - HUNT.z) as f64).powi(2)).sqrt() <= 3000.0;
    let mut out: Vec<(i64, i64)> = w.monsters.iter().filter(|m| m.name == "Green Worm" && m.hp > 0 && near(m.pos)).map(|m| (m.cid as i64, m.hp)).collect();
    while let Ok(p) = w.world.expect_where(|q| q.is(&schema::OPT_VIEW, Some(1)), 1500, "monstres").await {
        if let Ok(view) = decode_mini(&mini::OPT_VIEW_CHAR, p.fields().bytes(1).unwrap_or_default()) {
            let hp = view.bytes(5).and_then(factors_hp).unwrap_or(0);
            if view.text(2).as_deref() == Some("Green Worm") && hp > 0 {
                out.retain(|(c, _)| Some(*c) != view.int(0));
                out.extend(view.int(0).map(|c| (c, hp)));
            }
        }
    }
    out.sort_by_key(|(_, hp)| -hp);
    anyhow::ensure!(!out.is_empty(), "aucun Ver vert en vue");
    Ok(out.into_iter().map(|(c, _)| c).collect())
}

/// Frappe les vers les uns après les autres jusqu'à recevoir un paquet qui vérifie `pred` (`secs` au plus).
async fn hunt(w: &mut InWorld, pred: impl Fn(&Recv) -> bool, secs: u64, what: &str) -> anyhow::Result<Recv> {
    let (act, c) = (&schema::OPT_CHAR_ACTION, &schema::CHARACTER);
    let mut dead: HashSet<i64> = HashSet::new();
    let t0 = std::time::Instant::now();
    loop {
        anyhow::ensure!(t0.elapsed() < Duration::from_secs(secs), "{what} : rien en {secs} s");
        let list = worms(w).await?;
        // Tous frappés à mort : réapparus depuis (même CID), on recommence.
        if list.iter().all(|t| dead.contains(t)) {
            dead.clear();
        }
        let worm = list.into_iter().find(|t| !dead.contains(t)).unwrap_or_default();
        w.world.send(act, Fields::new(act).with(0, w.cid).with(1, worm).with(6, 1)).await?;
        // Jusqu'à la mort du ver (ou 15 s sans rien : un autre ver).
        let Ok(q) = w.world.expect_where(|q| pred(q) || (q.is(c, Some(1)) && q.int(1) == Some(worm) && q.int(11) == Some(2)), 15_000, what).await else { continue };
        if pred(&q) {
            return Ok(q);
        }
        dead.insert(worm);
    }
}

/// Arme prête en base : une Short Sword avec ces sockets et ces options, portée à la prochaine entrée.
async fn armed(db: &crate::db::Databases, w: &mut InWorld, sockets: &str, options: &str) -> anyhow::Result<i64> {
    let it = &schema::ITEM;
    w.world.send(&schema::CHATTING, say(w.cid, &format!("/additem {SWORD} 1"))).await?;
    let added = w.world.expect_where(|q| q.is(it, Some(0)) && q.int(3) == Some(SWORD) && q.int(1) == Some(2), 3000, "épée ajoutée").await?;
    let iid = added.int(2).unwrap_or(0);
    let filled = sockets.split(',').filter(|s| !s.is_empty()).count();
    db.characters
        .exec("UPDATE character_items SET socket_count = ?, socket_tids = ?, option_tids = ? WHERE id = ?", (filled.max(1) as i64, sockets.to_string(), options.to_string(), iid))
        .await?;
    Ok(iid)
}

/// Porte l'objet `iid` (AGPMITEM op 2, statut 3).
async fn wear(w: &mut InWorld, iid: i64) -> anyhow::Result<()> {
    let it = &schema::ITEM;
    w.world.send(it, Fields::new(it).with(0, 2).with(1, 3).with(2, iid)).await?;
    w.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(iid) && q.int(1) == Some(3), 3000, "objet porté").await?;
    Ok(())
}

/// Range l'objet porté `iid` dans l'inventaire, puis le détruit.
async fn destroy(w: &mut InWorld, iid: i64) -> anyhow::Result<()> {
    let (it, grid) = (&schema::ITEM, &mini::ITEM_GRID);
    let any = archcore_net::build_mini(grid, &Fields::new(grid).with(0, -1).with(1, -1).with(2, -1));
    w.world.send(it, Fields::new(it).with(0, 2).with(1, 2).with(2, iid).with(7, any)).await?;
    w.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(iid) && q.int(1) == Some(2), 3000, "objet rangé").await?;
    w.world.send(it, Fields::new(it).with(0, 21).with(2, iid)).await?;
    w.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(iid), 3000, "objet détruit").await?;
    Ok(())
}

/// Entrée en jeu derrière une fonction ordinaire, jamais en ligne (gros futur, comme dans `lang.rs`).
#[inline(never)]
fn enter<'a>(cfg: &'a Config, user: &'a str, pass: &'a str, name: &'a str, label: &'a str) -> Pin<Box<dyn Future<Output = anyhow::Result<InWorld>> + 'a>> {
    Box::pin(enter_world(cfg, user, pass, name, label))
}

/// Points héroïques (facteurs « dirt », indice 7) aux niveaux 89, 100 et 120 ; charisme (statut 4) gagné par
/// /charisma, au taux du serveur.
pub async fn heroic(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    step(&format!("{char_name} entre en jeu"));
    let mut w = enter(&cfg, user, pass, char_name, "").await?;
    let me = w.cid as i64;
    let level = w.level.max(1);
    for (lv, at_least) in [(89, 0), (100, 0), (120, 0)] {
        w.world.send(&schema::CHATTING, say(w.cid, &format!("/level {lv}"))).await?;
        let f = w.world.expect_where(|q| mine(q, me) && q.fields().bytes(7).and_then(|b| factor(b, 1, 7)) == Some(lv), 3000, "facteurs du niveau").await?;
        let heroic = f.fields().bytes(7).and_then(|b| factor(b, 11, 7)).unwrap_or(-1);
        let expected = crate::world::skills::heroic_points_for_level(lv as i32) as i64;
        anyhow::ensure!(heroic >= at_least && heroic <= expected, "niveau {lv} : {heroic} point(s) héroïque(s), {expected} au plus");
        println!("  ✓ niveau {lv} : {heroic} point(s) héroïque(s) (gagnés : {expected}, moins ceux dépensés)");
        if lv == 89 {
            anyhow::ensure!(heroic == 0, "points héroïques avant le niveau 90 : {heroic}");
        }
    }

    step("charisme : /charisma 300");
    let before = {
        w.world.send(&schema::CHATTING, say(w.cid, "/charisma 300")).await?;
        let f = w.world.expect_where(|q| mine(q, me), 3000, "facteurs").await?;
        f.fields().bytes(7).and_then(|b| factor(b, 1, 4)).unwrap_or(-1)
    };
    let text = w.world.expect_where(system, 3000, "réponse").await?.fields().text(6).unwrap_or_default();
    anyhow::ensure!(before >= 300, "charisme {before} après le gain");
    println!("  ✓ charisme {before} (CHA brut, statut 4) : {text}");

    step("reconnexion : charisme relu");
    drop(w);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut w = enter(&cfg, user, pass, char_name, "").await?;
    w.world.send(&schema::CHATTING, say(w.cid, &format!("/level {level}"))).await?;
    let f = w.world.expect_where(|q| mine(q, me), 3000, "facteurs").await?;
    let again = f.fields().bytes(7).and_then(|b| factor(b, 1, 4)).unwrap_or(-1);
    anyhow::ensure!(again == before, "charisme relu {again}, attendu {before}");
    println!("  ✓ {again} après reconnexion ; niveau {level} rétabli");
    println!("\npoints héroïques et charisme : OK");
    Ok(())
}

/// Partage des dégâts : GmTest (Martyr's Mark niveau 7, 40 % des dégâts physiques) et SimA en groupe ; un Ver
/// vert frappe SimA, GmTest perd des PV (AGPMCHARACTER op 1, facteurs du résultat).
pub async fn divide(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    const MARTYR: i64 = 608;
    let cfg = crate::config::load()?;
    let (k, mv, pm, act, c) = (&schema::SKILL, &schema::OPT_CHAR_MOVE, &schema::PARTY_MANAGEMENT, &schema::OPT_CHAR_ACTION, &schema::CHARACTER);
    step(&format!("préparation : {ca} connaît Martyr's Mark niveau 7"));
    let db = crate::db::Databases::new(&cfg.database);
    let id = character_id(&db, ca).await?;
    db.characters.exec("INSERT INTO character_skills (character_id, skill_tid, level) VALUES (?, ?, 7) ON DUPLICATE KEY UPDATE level = 7", (id, MARTYR)).await?;

    step(&format!("{ca} et {cb} entrent en jeu et forment un groupe"));
    let mut a = enter(&cfg, ua, pa, ca, "A-").await?;
    let mut b = enter(&cfg, ub, pb, cb, "B-").await?;
    for w in [&mut a, &mut b] {
        let cid = w.cid;
        w.world.send(mv, Fields::new(mv).with(0, cid).with(1, HUNT).with(2, HUNT).with(5, STOP).with(6, 0)).await?;
    }
    let (acid, bcid) = (a.cid as i64, b.cid as i64);
    let martyr = a.skills.iter().find(|s| s.1 == MARTYR).map(|s| s.0).ok_or_else(|| anyhow::anyhow!("Martyr's Mark absente"))?;
    a.world.send(pm, Fields::new(pm).with(0, 0).with(1, acid).with(2, bcid)).await?;
    b.world.expect_where(|q| q.is(pm, Some(0)), 3000, "invitation").await?;
    b.world.send(pm, Fields::new(pm).with(0, 2).with(1, bcid).with(2, acid)).await?;
    a.world.expect_where(|q| q.is(pm, Some(2)), 3000, "groupe formé").await?;
    let level = a.level.max(1);
    gm(&mut a, "/level 71").await?;
    gm(&mut a, "/invincible 1").await?;
    gm(&mut a, "/recast").await?;

    step(&format!("{ca} lance Martyr's Mark"));
    let on = |q: &Recv, who: i64| q.is(k, Some(4)) && q.int(1) == Some(martyr) && skill_owner(q, 2) == Some(who);
    a.world.send(k, cast_fields(martyr, acid)).await?;
    a.world.expect_where(|q| on(q, acid), 8000, "effet sur le lanceur").await?;
    b.world.expect_where(|q| on(q, bcid), 3000, "effet sur le membre").await?;
    println!("  ✓ effet posé sur {ca} et sur {cb}");

    step(&format!("un Ver vert frappe {cb}"));
    let worm = worms(&mut b).await?[0];
    b.world.send(act, Fields::new(act).with(0, b.cid).with(1, worm).with(6, 1)).await?;
    b.world.expect_where(|q| q.is(act, None) && q.int(0) == Some(bcid), 15_000, "premier coup").await?;
    let here = b.monsters.iter().find(|m| m.cid as i64 == worm).map(|m| m.pos).unwrap_or(HUNT);
    b.world.send(mv, Fields::new(mv).with(0, b.cid).with(1, here).with(2, here).with(5, STOP).with(6, 0)).await?;
    let hit = b.world.expect_where(|q| q.is(act, None) && q.int(0) == Some(worm) && q.int(1) == Some(bcid) && q.int(2) == Some(5), 60_000, "coup du ver").await?;
    let kept = hit.fields().bytes(3).and_then(|f| shown_damage(f, 9)).unwrap_or(0);
    let taken = a.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(acid) && q.fields().bytes(7).and_then(|f| shown_damage(f, 9)).unwrap_or(0) > 0, 3000, "part prise par le lanceur").await?;
    let share = taken.fields().bytes(7).and_then(|f| shown_damage(f, 9)).unwrap_or(0);
    anyhow::ensure!(share == (kept + share) * 40 / 100, "{ca} prend {share} sur {} (attendu 40 %)", kept + share);
    println!("  ✓ {cb} reçoit {kept}, {ca} prend {share} ({} PV)", o(taken.fields().bytes(7).and_then(factors_hp)));

    // Remise en état.
    a.world.send(pm, Fields::new(pm).with(0, 3).with(1, acid)).await?;
    gm(&mut a, "/invincible 0").await?;
    gm(&mut a, &format!("/level {level}")).await?;
    db.characters.exec("DELETE FROM character_skills WHERE character_id = ? AND skill_tid = ?", (id, MARTYR)).await?;
    db.close().await;
    println!("\npartage des dégâts : OK");
    Ok(())
}

/// Purge : Blade Sting affaiblit un Ver vert (AGPMSKILL op 4), Antimagic le lui retire (op 5).
pub async fn dispel(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    const BLADE_STING: i64 = 223;
    const ANTIMAGIC: i64 = 606;
    let cfg = crate::config::load()?;
    let k = &schema::SKILL;
    step("préparation : Blade Sting et Antimagic en base");
    let db = crate::db::Databases::new(&cfg.database);
    let id = character_id(&db, char_name).await?;
    // Blade Sting niveau 10 : 33 s d'affaiblissement, le temps d'approcher et de lancer Antimagic.
    for (tid, level) in [(BLADE_STING, 10), (ANTIMAGIC, 1)] {
        db.characters.exec("INSERT INTO character_skills (character_id, skill_tid, level) VALUES (?, ?, ?) ON DUPLICATE KEY UPDATE level = ?", (id, tid, level, level)).await?;
    }
    step(&format!("{char_name} entre en jeu"));
    let mut w = enter(&cfg, user, pass, char_name, "").await?;
    let level = w.level.max(1);
    let ids: Vec<i64> = [BLADE_STING, ANTIMAGIC].iter().map(|t| w.skills.iter().find(|s| s.1 == *t).map(|s| s.0).unwrap_or(0)).collect();
    anyhow::ensure!(ids.iter().all(|i| *i != 0), "compétences absentes : {:?}", w.skills);
    // Niveau 30 : assez de PM pour Blade Sting niveau 10 (85), qui blesse sans tuer d'un coup.
    gm(&mut w, "/level 30").await?;
    gm(&mut w, "/invincible 1").await?;
    gm(&mut w, "/recast").await?;
    let worm = worms(&mut w).await?[0];

    step("Blade Sting sur un Ver vert");
    w.world.send(k, cast_fields(ids[0], worm)).await?;
    w.world.expect_where(|q| q.is(k, Some(4)) && q.int(1) == Some(ids[0]) && skill_owner(q, 2) == Some(worm), 30_000, "affaiblissement posé").await?;
    println!("  ✓ affaiblissement posé sur {worm:#x}");
    // Un déplacement arrête l'attaque automatique ; niveau 100 pour Antimagic (500 PM ; 598 au niveau 100).
    let mv = &schema::OPT_CHAR_MOVE;
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, HUNT).with(2, HUNT).with(5, STOP).with(6, 0)).await?;
    gm(&mut w, "/level 100").await?;

    step("Antimagic sur le même ver");
    w.world.send(k, cast_fields(ids[1], worm)).await?;
    w.world.expect_where(|q| q.is(k, Some(8)) && q.int(1) == Some(ids[1]), 30_000, "Antimagic").await?;
    w.world.expect_where(|q| q.is(k, Some(5)) && q.int(1) == Some(ids[0]) && skill_owner(q, 2) == Some(worm), 1500, "affaiblissement retiré").await?;
    println!("  ✓ Blade Sting retiré par la purge, sans attendre sa fin (33 s)");

    gm(&mut w, "/invincible 0").await?;
    gm(&mut w, &format!("/level {level}")).await?;
    db.characters.exec("DELETE FROM character_skills WHERE character_id = ? AND skill_tid IN (?, ?)", (id, BLADE_STING, ANTIMAGIC)).await?;
    db.close().await;
    println!("\npurges : OK");
    Ok(())
}

/// Défense ignorée : épée avec une pierre d'eau (3 à 3), Ver vert résistant à 50 % : la part d'eau d'un coup vaut
/// 3 ; sous Stunner niveau 20 (48 % des résistances ignorées), 4.
pub async fn ignore(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    const WATER_STONE: &str = "201";
    const STUNNER: i64 = 167;
    const WATER: usize = 11;
    let cfg = crate::config::load()?;
    let (k, act) = (&schema::SKILL, &schema::OPT_CHAR_ACTION);
    let db = crate::db::Databases::new(&cfg.database);
    let id = character_id(&db, char_name).await?;
    db.characters.exec("INSERT INTO character_skills (character_id, skill_tid, level) VALUES (?, ?, 20) ON DUPLICATE KEY UPDATE level = 20", (id, STUNNER)).await?;
    step(&format!("{char_name} entre en jeu ; épée et pierre d'eau"));
    let mut w = enter(&cfg, user, pass, char_name, "").await?;
    let iid = armed(&db, &mut w, WATER_STONE, "").await?;
    drop(w);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut w = enter(&cfg, user, pass, char_name, "").await?;
    let me = w.cid as i64;
    let level = w.level.max(1);
    wear(&mut w, iid).await?;
    gm(&mut w, "/level 30").await?;
    gm(&mut w, "/invincible 1").await?;
    let water = |q: &Recv| q.is(act, None) && q.int(0) == Some(me) && q.fields().bytes(3).and_then(|f| shown_damage(f, WATER)).unwrap_or(0) > 0;

    step("coups sans Stunner");
    let q = hunt(&mut w, water, 120, "part d'eau").await?;
    let before = q.fields().bytes(3).and_then(|f| shown_damage(f, WATER)).unwrap_or(0);
    anyhow::ensure!(before == 3, "part d'eau {before} (attendu 2 × 3 × 50 %)");
    println!("  ✓ part d'eau {before}");

    step("Stunner niveau 20");
    let stunner = w.skills.iter().find(|s| s.1 == STUNNER).map(|s| s.0).ok_or_else(|| anyhow::anyhow!("Stunner absente"))?;
    // Niveau 60 : assez de PM (150) ; la part d'eau ne dépend que de la pierre.
    gm(&mut w, "/level 60").await?;
    gm(&mut w, "/recast").await?;
    w.world.send(k, cast_fields(stunner, me)).await?;
    w.world.expect_where(|q| q.is(k, Some(4)) && q.int(1) == Some(stunner), 10_000, "effet").await?;
    let q = hunt(&mut w, water, 120, "part d'eau").await?;
    let after = q.fields().bytes(3).and_then(|f| shown_damage(f, WATER)).unwrap_or(0);
    anyhow::ensure!(after == 4, "part d'eau {after} sous Stunner (attendu 2 × 3 × 74 % = 4)");
    println!("  ✓ part d'eau {after} : résistance 50 % → 26 %");

    destroy(&mut w, iid).await?;
    gm(&mut w, "/invincible 0").await?;
    gm(&mut w, &format!("/level {level}")).await?;
    db.characters.exec("DELETE FROM character_skills WHERE character_id = ? AND skill_tid = ?", (id, STUNNER)).await?;
    db.close().await;
    println!("\ndéfense ignorée : OK");
    Ok(())
}

/// Rune à compétence : Cold Prayer Stone<Lv1> (TID 588 : « Cold Prayer Stone <Small> », 4 %) sertie ; sur un coup,
/// AGPMSKILL op 8 (TID 275, dégâts de glace) puis op 4 (ralentissement) sur le ver.
pub async fn rune(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    const COLD: i64 = 275;
    const ICE: usize = 17;
    let cfg = crate::config::load()?;
    let k = &schema::SKILL;
    let db = crate::db::Databases::new(&cfg.database);
    step(&format!("{char_name} entre en jeu ; épée et pierre de prière de glace"));
    let mut w = enter(&cfg, user, pass, char_name, "").await?;
    let iid = armed(&db, &mut w, "588", "").await?;
    drop(w);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut w = enter(&cfg, user, pass, char_name, "").await?;
    let me = w.cid as i64;
    let level = w.level.max(1);
    wear(&mut w, iid).await?;
    gm(&mut w, "/level 1").await?;
    gm(&mut w, "/invincible 1").await?;

    step("frapper jusqu'au déclenchement (4 % par coup)");
    let proc = hunt(&mut w, |q| q.is(k, Some(8)) && q.int(4) == Some(COLD) && skill_owner(q, 2) == Some(me), 300, "compétence de la rune").await?;
    let act = decode_mini(&mini::SKILL_ACTION, proc.fields().bytes(6).unwrap_or_default())?;
    let target = decode_mini(&mini::SKILL_BASE, act.bytes(1).unwrap_or_default())?.int(1).unwrap_or(0);
    let ice = act.bytes(2).and_then(|f| shown_damage(f, ICE)).unwrap_or(0);
    anyhow::ensure!(ice > 0, "pas de dégâts de glace");
    println!("  ✓ Cold Prayer Stone <Small> sur {target:#x} : {ice} de glace");
    let slow = w.world.expect_where(|q| q.is(k, Some(4)) && skill_owner(q, 2) == Some(target), 1500, "ralentissement").await;
    match slow {
        Ok(_) => println!("  ✓ ralentissement posé (AGPMSKILL op 4)"),
        Err(_) => anyhow::bail!("pas d'effet posé sur {target:#x}"),
    }

    destroy(&mut w, iid).await?;
    gm(&mut w, "/invincible 0").await?;
    gm(&mut w, &format!("/level {level}")).await?;
    db.close().await;
    println!("\nrunes à compétence : OK");
    Ok(())
}

/// Option à déclenchement : épée avec l'option 5658 (« 28 % d'étourdir 3 s ») ; un Ver vert passe étourdi
/// (AGPMCHARACTER op 1, champ 17, bit 1), puis l'état retombe.
pub async fn trigger(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let c = &schema::CHARACTER;
    let db = crate::db::Databases::new(&cfg.database);
    step(&format!("{char_name} entre en jeu ; épée à l'option d'étourdissement"));
    let mut w = enter(&cfg, user, pass, char_name, "").await?;
    let iid = armed(&db, &mut w, "", "5658").await?;
    drop(w);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut w = enter(&cfg, user, pass, char_name, "").await?;
    let level = w.level.max(1);
    wear(&mut w, iid).await?;
    gm(&mut w, "/level 1").await?;
    gm(&mut w, "/invincible 1").await?;

    step("frapper jusqu'à l'étourdissement");
    let stunned = hunt(&mut w, |q| q.is(c, Some(1)) && q.int(17).is_some_and(|s| s & 1 != 0), 180, "étourdissement").await?;
    let worm = stunned.int(1).unwrap_or(0);
    println!("  ✓ {worm:#x} étourdi (état {:#x})", stunned.int(17).unwrap_or(0));
    // Un déplacement arrête l'attaque : plus de coup, plus de nouvel étourdissement.
    let mv = &schema::OPT_CHAR_MOVE;
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, HUNT).with(2, HUNT).with(5, STOP).with(6, 0)).await?;
    let t0 = std::time::Instant::now();
    w.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(worm) && q.int(17).is_some_and(|s| s & 1 == 0), 8000, "fin de l'étourdissement").await?;
    println!("  ✓ état levé au bout de {:.1} s", t0.elapsed().as_secs_f64());

    destroy(&mut w, iid).await?;
    gm(&mut w, "/invincible 0").await?;
    gm(&mut w, &format!("/level {level}")).await?;
    db.close().await;
    println!("\noptions à déclenchement : OK");
    Ok(())
}
