//! `archcore sim ride` : montures (objet de partie 14, AgpmRide 0x4D).
//!
//! SimDepart (compte MJ gmtest, déjà plein : 3 personnages) passe niveau 40 et reçoit un White Horse (TID 3002 : humain, niveau 35,
//! Run_Buff 10 500). SimA le regarde. Monter : objet porté, course 10 500 dans les facteurs, AgpmRide op 2 chez lui et
//! chez SimA. Descendre : op 4, course d'origine. Refus en combat : il frappe un Ver vert, la monte est refusée
//! (AgpmSystemMessage code 24, l'objet reste dans l'inventaire) ; 11 s plus tard, au village, elle passe. Reconnexion à cheval :
//! champ 18 du personnage (bit 1) et op 2 à l'entrée. Mort (/kill) : descente forcée (op 4).

use super::{enter_world, factors_run_speed, step, InWorld, SeenMonster};
use archcore_net::schema::{self, chat_type, mini};
use archcore_net::{build_mini, decode_mini, Fields, Vec3};
use std::time::Duration;

const HORSE: i64 = 3002;
const RUN_BUFF: i64 = 10_500;
const HUNT: Vec3 = Vec3 { x: -466_468.0, y: 4_700.0, z: -50_010.0 };
const STOP: i64 = 0x04;

fn say(w: &InWorld, text: &str) -> Fields {
    let ch = &schema::CHATTING;
    Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text))
}

async fn gm(w: &mut InWorld, text: &str, expect: &str) -> anyhow::Result<()> {
    w.world.send(&schema::CHATTING, say(w, text)).await?;
    let ch = &schema::CHATTING;
    w.world.expect_where(|q| q.ptype == ch.id && q.fields().text(6).is_some_and(|t| t.contains(expect)), 3000, text).await?;
    Ok(())
}

async fn move_to(w: &mut InWorld, at: Vec3) -> anyhow::Result<()> {
    let mv = &schema::OPT_CHAR_MOVE;
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, at).with(2, at).with(5, STOP).with(6, 0)).await
}

/// Monte : objet porté, facteurs (course rendue), AgpmRide op 2.
async fn mount(w: &mut InWorld, iid: i64) -> anyhow::Result<i64> {
    let (it, c, r, me) = (&schema::ITEM, &schema::CHARACTER, &schema::RIDE, w.cid as i64);
    w.world.send(it, Fields::new(it).with(0, 2).with(1, 3).with(2, iid)).await?;
    w.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(iid) && q.int(1) == Some(3), 3000, "monture portée").await?;
    let f = w.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(me) && q.fields().bytes(7).is_some(), 3000, "facteurs").await?;
    let speed = f.fields().bytes(7).and_then(factors_run_speed).unwrap_or(0);
    let ack = w.world.expect_where(|q| q.is(r, Some(2)), 3000, "AgpmRide op 2").await?;
    anyhow::ensure!(ack.int(1) == Some(me) && ack.int(2) == Some(iid), "op 2 : cavalier {:?}, objet {:?}", ack.int(1), ack.int(2));
    Ok(speed)
}

/// Descend : objet rangé, facteurs, AgpmRide op 4.
async fn dismount(w: &mut InWorld, iid: i64) -> anyhow::Result<i64> {
    let (it, c, r, me) = (&schema::ITEM, &schema::CHARACTER, &schema::RIDE, w.cid as i64);
    let any = &mini::ITEM_GRID;
    let grid = build_mini(any, &Fields::new(any).with(0, -1).with(1, -1).with(2, -1));
    w.world.send(it, Fields::new(it).with(0, 2).with(1, 2).with(2, iid).with(7, grid)).await?;
    w.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(iid) && q.int(1) == Some(2), 3000, "monture rangée").await?;
    let f = w.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(me) && q.fields().bytes(7).is_some(), 3000, "facteurs").await?;
    let speed = f.fields().bytes(7).and_then(factors_run_speed).unwrap_or(0);
    w.world.expect_where(|q| q.is(r, Some(4)) && q.int(1) == Some(me), 3000, "AgpmRide op 4").await?;
    Ok(speed)
}

pub async fn run(user: &str, pass: &str, char_name: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let (it, r, act, c) = (&schema::ITEM, &schema::RIDE, &schema::OPT_CHAR_ACTION, &schema::CHARACTER);
    step(&format!("{cb}, témoin, et {char_name} entrent en jeu"));
    let mut witness = enter_world(&cfg, ub, pb, cb, "B-").await?;
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let me = w.cid as i64;
    let spot = witness.start;
    move_to(&mut w, spot).await?;
    move_to(&mut witness, spot).await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    gm(&mut w, "/level 40", "niveau 40").await?;
    gm(&mut w, "/invincible 1", "Invincibilité").await?;
    w.world.send(&schema::CHATTING, say(&w, &format!("/additem {HORSE} 1"))).await?;
    let horse = w.world.expect_where(|q| q.is(it, Some(0)) && q.int(3) == Some(HORSE) && q.int(1) == Some(2), 3000, "cheval").await?.int(2).unwrap_or(0);
    w.world.send(&schema::CHATTING, say(&w, "/speed 1")).await?;
    let base = w.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(me) && q.fields().bytes(7).is_some(), 3000, "facteurs").await?;
    let base = base.fields().bytes(7).and_then(factors_run_speed).unwrap_or(0);
    println!("  ✓ White Horse reçu ({horse}), course à pied {base}");

    step("monter");
    let speed = mount(&mut w, horse).await?;
    anyhow::ensure!(speed == RUN_BUFF, "course à cheval {speed}");
    println!("  ✓ monté : course {speed} (Run_Buff), AgpmRide op 2");
    let seen = witness.world.expect_where(|q| q.is(r, Some(2)) && q.int(1) == Some(me), 3000, "op 2 chez le témoin").await?;
    println!("  ✓ {cb} voit {char_name} monter (objet {})", seen.int(2).unwrap_or(0));

    step("reconnexion à cheval");
    drop(w);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    anyhow::ensure!(w.transform & 0x02 != 0, "champ 18 = {}", w.transform);
    anyhow::ensure!(w.rides.contains(&(2, horse)), "AgpmRide à l'entrée : {:?}", w.rides);
    println!("  ✓ champ 18 = {} (monté) et op 2 à l'entrée", w.transform);

    step("descendre");
    let speed = dismount(&mut w, horse).await?;
    anyhow::ensure!(speed == base, "course {speed} au lieu de {base}");
    println!("  ✓ descendu : course {speed}, AgpmRide op 4");

    step("refus en combat");
    move_to(&mut w, HUNT).await?;
    let mut seen = w.monsters.clone();
    let ov = &schema::OPT_VIEW;
    while let Ok(p) = w.world.expect_where(|q| q.is(ov, Some(1)), 1500, "monstres").await {
        if let Ok(view) = decode_mini(&mini::OPT_VIEW_CHAR, p.fields().bytes(1).unwrap_or_default()) {
            if view.int(0).unwrap_or(0) >= crate::world::monsters::MONSTER_CID_BASE as i64 {
                seen.push(SeenMonster::from_view(&view));
            }
        }
    }
    let dist = |p: Vec3| (((p.x - HUNT.x) as f64).powi(2) + ((p.z - HUNT.z) as f64).powi(2)).sqrt();
    let target = seen.iter().filter(|m| m.hp > 0 && m.name == "Green Worm").min_by_key(|m| dist(m.pos) as i64).cloned().ok_or_else(|| anyhow::anyhow!("aucun Ver vert"))?;
    w.world.send(act, Fields::new(act).with(0, w.cid).with(1, target.cid).with(6, 1)).await?;
    let tcid = target.cid as i64;
    w.world.expect_where(|q| q.is(act, None) && q.int(0) == Some(me) && q.int(1) == Some(tcid), 15_000, "coup porté").await?;
    // Retour au village, loin du ver (qui sinon, en frappant, prolongerait l'état de combat).
    move_to(&mut w, spot).await?;
    w.world.send(it, Fields::new(it).with(0, 2).with(1, 3).with(2, horse)).await?;
    let sm = &schema::SYSTEM_MESSAGE;
    let no = w.world.expect_where(|q| q.is(sm, None), 3000, "refus en combat").await?;
    anyhow::ensure!(no.int(1) == Some(24), "code {:?}", no.int(1));
    w.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(horse) && q.int(1) == Some(2), 3000, "cheval rendu à l'inventaire").await?;
    println!("  ✓ montée refusée en combat (AgpmSystemMessage code 24), cheval resté dans l'inventaire");
    tokio::time::sleep(Duration::from_secs(11)).await;
    let speed = mount(&mut w, horse).await?;
    anyhow::ensure!(speed == RUN_BUFF, "course à cheval {speed}");
    println!("  ✓ 11 s plus tard, hors combat : monté");

    step("mort à cheval : descente forcée");
    gm(&mut w, "/invincible 0", "Invincibilité").await?;
    w.world.send(&schema::CHATTING, say(&w, "/kill")).await?;
    w.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(me) && q.int(11) == Some(2), 3000, "mort").await?;
    w.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(horse) && q.int(1) == Some(2), 3000, "cheval rangé").await?;
    w.world.expect_where(|q| q.is(r, Some(4)) && q.int(1) == Some(me), 3000, "AgpmRide op 4").await?;
    println!("  ✓ descendu de force à la mort");
    w.world.send(c, Fields::new(c).with(0, 9).with(1, w.cid)).await?;
    w.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(me) && q.int(11) == Some(0), 5000, "résurrection").await?;

    // Remise en état : cheval détruit.
    w.world.send(it, Fields::new(it).with(0, 21).with(2, horse)).await?;
    w.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(horse), 3000, "cheval détruit").await?;
    println!("\nmontures : OK");
    Ok(())
}
