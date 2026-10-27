//! `archcore sim pk` : combat entre joueurs (AgpmPvP 0x4A).
//!
//! Le MJ passe niveau 40 et met l'autre joueur au niveau 20, puis tous deux vont au nord d'Anchorville (région 0,
//! zone libre). Une attaque sans Ctrl est refusée ; une attaque forcée les rend ennemis (op 2 des deux côtés,
//! message 3) et fait du MJ un premier attaquant (champ 12 = 1). Il frappe jusqu'à la mort : victoire et défaite
//! (op 0, messages 12 et 5), inimitié levée (op 4, message 4) et 20 points de meurtrier (victime innocente de plus
//! de 4 niveaux en dessous, attaquée en premier). Les niveaux sont remis à la fin ; les points restent en base
//! (`isole.sh` les efface).

use super::{enter_world, step, Recv};
use archcore_net::schema::{self, chat_type, mini};
use archcore_net::{decode_mini, Fields, Vec3};

/// Nord d'Anchorville, région 0 (libre).
const SPOT: Vec3 = Vec3 { x: -466_468.0, y: 4_700.0, z: -50_010.0 };
const STOP: i64 = 0x04;

fn murderer_points(bytes: &[u8]) -> Option<i64> {
    let f = decode_mini(&mini::CHAR_FACTORS, bytes).ok()?;
    decode_mini(&mini::FACTOR_STATUS, f.bytes(1)?).ok()?.int(11)
}

pub async fn run(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let (mv, act, ch, pvp, c) = (&schema::OPT_CHAR_MOVE, &schema::OPT_CHAR_ACTION, &schema::CHATTING, &schema::PVP, &schema::CHARACTER);
    let say = |cid: u32, text: &str| Fields::new(ch).with(0, 0).with(1, cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text));
    let place = |cid: u32| Fields::new(mv).with(0, cid).with(1, SPOT).with(2, SPOT).with(5, STOP).with(6, 0);

    step(&format!("{ca} (niveau 40) et {cb} (niveau 20) au nord d'Anchorville"));
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    let mut b = enter_world(&cfg, ub, pb, cb, "B-").await?;
    let (acid, bcid) = (a.cid, b.cid);
    let (level_a, level_b) = (a.level, b.level);
    for cmd in ["/level 40".to_string(), format!("/level 20 \"{cb}\"")] {
        a.world.send(ch, say(acid, &cmd)).await?;
        a.world.expect_where(|q| q.ptype == ch.id, 5000, "réponse du MJ").await?;
    }
    a.world.send(mv, place(acid)).await?;
    b.world.send(mv, place(bcid)).await?;
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;

    step(&format!("{ca} attaque {cb} sans Ctrl : refusé"));
    a.world.send(act, Fields::new(act).with(0, acid).with(1, bcid).with(6, 0)).await?;
    anyhow::ensure!(a.world.expect_where(|q| q.is(pvp, Some(2)), 1500, "inimitié").await.is_err(), "inimitié créée sans attaque forcée");
    println!("  ✓ pas d'inimitié");

    step(&format!("{ca} force l'attaque (Ctrl)"));
    a.world.send(act, Fields::new(act).with(0, acid).with(1, bcid).with(6, 1)).await?;
    let add = a.world.expect_where(|q| q.is(pvp, Some(2)), 8000, "op 2 chez l'agresseur").await?;
    anyhow::ensure!(add.int(2) == Some(bcid as i64) && add.int(5) == Some(2), "op 2 : cible {:?}, mode {:?}", add.int(2), add.int(5));
    let add_b = b.world.expect_where(|q| q.is(pvp, Some(2)), 3000, "op 2 chez la victime").await?;
    anyhow::ensure!(add_b.int(2) == Some(acid as i64), "op 2 chez {cb} : {:?}", add_b.int(2));
    println!("  ✓ ennemis l'un de l'autre, zone libre (mode 2)");
    let flag = a.world.expect_where(|q: &Recv| q.is(c, Some(1)) && q.int(1) == Some(acid as i64) && q.int(12).is_some(), 3000, "statut criminel").await?;
    anyhow::ensure!(flag.int(12) == Some(1) && flag.int(25).is_some_and(|s| s > 1100), "champ 12 {:?}, 25 {:?}", flag.int(12), flag.int(25));
    println!("  ✓ {ca} premier attaquant (champ 12 = 1, {} s)", flag.int(25).unwrap_or(0));

    step(&format!("{ca} frappe jusqu'à la mort de {cb}"));
    let kill = a.world.expect_where(|q| q.is(pvp, Some(18)) && q.int(1) == Some(12), 90_000, "message 12 (victoire)").await?;
    anyhow::ensure!(kill.fields().text(8).as_deref() == Some(cb), "vaincu annoncé : {:?}", kill.fields().text(8));
    let dead = b.world.expect_where(|q| q.is(pvp, Some(18)) && q.int(1) == Some(5), 5000, "message 5 (défaite)").await?;
    anyhow::ensure!(dead.fields().text(8).as_deref() == Some(ca), "vainqueur annoncé : {:?}", dead.fields().text(8));
    println!("  ✓ messages 12 et 5");
    let points = a.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(acid as i64) && q.fields().bytes(7).and_then(murderer_points).is_some_and(|p| p > 0), 5000, "points de meurtrier").await?;
    let points = points.fields().bytes(7).and_then(murderer_points).unwrap_or(0);
    anyhow::ensure!(points >= 20, "points de meurtrier : {points}");
    println!("  ✓ {ca} : {points} point(s) de meurtrier");

    step("remise des niveaux");
    for cmd in [format!("/level {level_a}"), format!("/level {} \"{cb}\"", level_b.max(1))] {
        a.world.send(ch, say(acid, &cmd)).await?;
        let _ = a.world.expect_where(|q| q.ptype == ch.id, 5000, "réponse du MJ").await;
    }
    println!("\ncombat entre joueurs : OK");
    Ok(())
}
