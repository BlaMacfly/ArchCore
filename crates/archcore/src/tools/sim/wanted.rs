//! `archcore sim wanted` : avis de recherche (AgpmWantedCriminal 0x46, fonction de PNJ 32).
//!
//! Comme `sim pk`, GmTest (MJ, niveau 40) force l'attaque sur SimA (niveau 20, innocent) au nord d'Anchorville et le
//! tue : SimA reçoit l'op 6 avec le nom de son tueur. Mort, il le met à prix : 40 000 pièces sont refusées (prime de
//! base 50 000 au niveau 20, code 1), 60 000 acceptées (code 0) ; GmTest reçoit l'op 7 (recherché) et l'op 5 (rang 1,
//! 60 000) ; une seconde mise à prix est refusée (code 3). Ressuscité, SimA consulte le tableau chez Rembrandt (op 0
//! → op 1, op 3 → op 4 rang 1 puis fin de page). Puis SimA, passé niveau 60, tue GmTest repassé niveau 20 : prime
//! touchée (op 8 genre 4, 60 000), GmTest averti (op 8 genre 5) et retiré des avis (op 7 drapeau 0). Les niveaux sont
//! remis à la fin.

use super::{enter_world, step, InWorld, Recv};
use archcore_net::schema::{self, chat_type, mini, wanted_notice, wanted_op, wanted_result};
use archcore_net::{build_mini, decode_mini, Fields, Vec3};
use std::time::Duration;

/// Nord d'Anchorville, région 0 (libre).
const SPOT: Vec3 = Vec3 { x: -466_468.0, y: 4_700.0, z: -50_010.0 };
/// Rembrandt, chef d'Anchorville (fonction 32).
const REMBRANDT: i64 = 0x4000_0000 + 97;
const NEAR_REMBRANDT: Vec3 = Vec3 { x: -475_700.0, y: 4_618.0, z: -65_000.0 };
const BOUNTY: i64 = 60_000;

fn say(w: &InWorld, text: &str) -> Fields {
    let ch = &schema::CHATTING;
    Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text))
}

async fn gm(w: &mut InWorld, text: &str) -> anyhow::Result<()> {
    w.world.send(&schema::CHATTING, say(w, text)).await?;
    let _ = w.world.expect_where(|q| q.ptype == schema::CHATTING.id, 5000, text).await;
    Ok(())
}

async fn place(w: &mut InWorld, at: Vec3) -> anyhow::Result<()> {
    let mv = &schema::OPT_CHAR_MOVE;
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, at).with(2, at).with(5, 0x04).with(6, 0)).await
}

fn wc(op: i64) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| q.is(&schema::WANTED_CRIMINAL, Some(op))
}

fn mini_of(q: &Recv) -> Fields {
    decode_mini(&mini::WANTED_CRIMINAL, q.fields().bytes(5).unwrap_or_default()).unwrap_or_else(|_| Fields::new(&mini::WANTED_CRIMINAL))
}

/// Mise à prix de `name` pour `amount` : code reçu.
async fn arrange(w: &mut InWorld, name: &str, amount: i64) -> anyhow::Result<i64> {
    let (s, m) = (&schema::WANTED_CRIMINAL, &mini::WANTED_CRIMINAL);
    let offer = build_mini(m, &Fields::new(m).with(0, name).with(1, amount));
    w.world.send(s, Fields::new(s).with(0, wanted_op::ARRANGE).with(1, w.cid).with(5, offer)).await?;
    let r = w.world.expect_where(wc(wanted_op::ARRANGE), 3000, "résultat de la mise à prix").await?;
    anyhow::ensure!(mini_of(&r).text(0).as_deref() == Some(name), "nom renvoyé {:?}", mini_of(&r).text(0));
    Ok(r.int(4).unwrap_or(-1))
}

/// `hunter` force l'attaque sur `prey` et frappe jusqu'à sa mort (message 12 du combat entre joueurs).
async fn kill(hunter: &mut InWorld, prey: u32, what: &str) -> anyhow::Result<()> {
    let act = &schema::OPT_CHAR_ACTION;
    hunter.world.send(act, Fields::new(act).with(0, hunter.cid).with(1, prey).with(6, 1)).await?;
    hunter.world.expect_where(|q| q.is(&schema::PVP, Some(18)) && q.int(1) == Some(12), 90_000, what).await?;
    Ok(())
}

pub async fn run(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let (s, c) = (&schema::WANTED_CRIMINAL, &schema::CHARACTER);

    step(&format!("{ca} (niveau 40) tue {cb} (niveau 20, innocent) au nord d'Anchorville"));
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    let mut b = enter_world(&cfg, ub, pb, cb, "B-").await?;
    let (acid, bcid) = (a.cid, b.cid);
    let (level_a, level_b) = (a.level, b.level);
    gm(&mut a, "/level 40").await?;
    gm(&mut a, &format!("/level 20 \"{cb}\"")).await?;
    place(&mut a, SPOT).await?;
    place(&mut b, SPOT).await?;
    tokio::time::sleep(Duration::from_millis(800)).await;
    kill(&mut a, bcid, "mort de B").await?;
    let killer = b.world.expect_where(wc(wanted_op::KILLER), 5000, "nom du tueur (op 6)").await?;
    anyhow::ensure!(killer.int(1) == Some(bcid as i64) && mini_of(&killer).text(0).as_deref() == Some(ca), "op 6 : CID {:?}, nom {:?}", killer.int(1), mini_of(&killer).text(0));
    println!("  ✓ op 6 : {cb} peut mettre {ca} à prix");

    step(&format!("{cb}, mort, met {ca} à prix"));
    gm(&mut a, &format!("/money {BOUNTY} \"{cb}\"")).await?;
    let code = arrange(&mut b, ca, 40_000).await?;
    anyhow::ensure!(code == wanted_result::NOT_ENOUGH_BASE, "40 000 : code {code}");
    println!("  ✓ 40 000 : code 1 (prime de base 50 000 au niveau 20)");
    let code = arrange(&mut b, ca, BOUNTY).await?;
    anyhow::ensure!(code == wanted_result::SUCCESS, "{BOUNTY} : code {code}");
    println!("  ✓ {BOUNTY} : code 0");
    let flag = a.world.expect_where(wc(wanted_op::WANTED), 3000, "recherché (op 7)").await?;
    anyhow::ensure!(flag.int(4) == Some(1), "op 7 : {:?}", flag.int(4));
    let rank = a.world.expect_where(wc(wanted_op::MY_RANK), 3000, "rang (op 5)").await?;
    anyhow::ensure!(rank.int(3) == Some(1) && mini_of(&rank).int(1) == Some(BOUNTY), "op 5 : rang {:?}, prime {:?}", rank.int(3), mini_of(&rank).int(1));
    println!("  ✓ {ca} : op 7 drapeau 1, op 5 rang 1 prime {BOUNTY}");
    let code = arrange(&mut b, ca, BOUNTY).await?;
    anyhow::ensure!(code == wanted_result::ALREADY_ARRANGED, "seconde mise à prix : code {code}");
    println!("  ✓ seconde mise à prix : code 3");

    step(&format!("{cb} ressuscite et consulte le tableau chez Rembrandt"));
    b.world.send(c, Fields::new(c).with(0, 9).with(1, bcid)).await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    place(&mut b, NEAR_REMBRANDT).await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let eb = &mini::EVENT_BASE;
    let event = build_mini(eb, &Fields::new(eb).with(0, 3).with(1, REMBRANDT).with(3, 32));
    b.world.send(s, Fields::new(s).with(0, wanted_op::REQUEST).with(1, bcid).with(5, event)).await?;
    b.world.expect_where(wc(wanted_op::GRANT), 3000, "tableau accordé (op 1)").await?;
    b.world.send(s, Fields::new(s).with(0, wanted_op::PAGE).with(1, bcid).with(2, 0)).await?;
    let line = b.world.expect_where(wc(wanted_op::ENTRY), 3000, "ligne du tableau (op 4)").await?;
    let m = mini_of(&line);
    anyhow::ensure!(line.int(3) == Some(1) && m.text(0).as_deref() == Some(ca) && m.int(1) == Some(BOUNTY) && m.int(4) == Some(1), "ligne : rang {:?}, {:?}, {:?}, en ligne {:?}", line.int(3), m.text(0), m.int(1), m.int(4));
    let end = b.world.expect_where(wc(wanted_op::ENTRY), 3000, "fin de page").await?;
    anyhow::ensure!(end.int(2) == Some(wanted_op::END_OF_PAGE), "fin de page : {:?}", end.int(2));
    println!("  ✓ op 4 : rang 1, {ca}, {BOUNTY}, niveau {:?}, en ligne ; fin de page (-100)", m.int(2));

    step(&format!("{cb} (niveau 60) tue {ca} (niveau 20) et touche la prime"));
    gm(&mut a, "/level 20").await?;
    gm(&mut a, &format!("/level 60 \"{cb}\"")).await?;
    place(&mut a, SPOT).await?;
    place(&mut b, SPOT).await?;
    tokio::time::sleep(Duration::from_millis(800)).await;
    // La prime (op 8) part avant le message de victoire du combat entre joueurs.
    let act = &schema::OPT_CHAR_ACTION;
    b.world.send(act, Fields::new(act).with(0, bcid).with(1, acid).with(6, 1)).await?;
    let prize = b.world.expect_where(wc(wanted_op::NOTIFY), 90_000, "prime touchée (op 8)").await?;
    anyhow::ensure!(prize.int(4) == Some(wanted_notice::KILL) && mini_of(&prize).int(1) == Some(BOUNTY) && mini_of(&prize).text(0).as_deref() == Some(ca), "op 8 chez {cb} : genre {:?}, {:?}", prize.int(4), mini_of(&prize).int(1));
    let dead = a.world.expect_where(wc(wanted_op::NOTIFY), 5000, "avis de capture (op 8)").await?;
    anyhow::ensure!(dead.int(4) == Some(wanted_notice::DEAD) && mini_of(&dead).text(0).as_deref() == Some(cb), "op 8 chez {ca} : genre {:?}", dead.int(4));
    let off = a.world.expect_where(wc(wanted_op::WANTED), 3000, "plus recherché (op 7)").await?;
    anyhow::ensure!(off.int(4) == Some(0), "op 7 : {:?}", off.int(4));
    println!("  ✓ {cb} : op 8 genre 4 ({BOUNTY} pièces) ; {ca} : op 8 genre 5 ({} d'expérience perdue), op 7 drapeau 0", mini_of(&dead).int(1).unwrap_or(0));

    step("remise des niveaux");
    a.world.send(c, Fields::new(c).with(0, 9).with(1, acid)).await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    gm(&mut a, &format!("/level {level_a}")).await?;
    gm(&mut a, &format!("/level {} \"{cb}\"", level_b.max(1))).await?;
    println!("\navis de recherche : OK");
    Ok(())
}
