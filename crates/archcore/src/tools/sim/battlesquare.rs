//! `archcore sim battlesquare` : Battle Square (`world/battlesquare.rs`).
//!
//! SimA (niveau 1) parle au superviseur Valiant (PNJ 551, près d'Anchorville) : refusé (AgpmPvP op 18 code 18). Au
//! niveau 25, il est envoyé à la porte du premier étage (codes 2 puis 13). GmTest (MJ, niveau 30) l'y rejoint par
//! `/move` (mêmes codes), force l'attaque et le tue : aucun point de meurtrier (5 niveaux d'écart, innocent frappé le
//! premier) et, `Rate.Drop` porté à 10, un Skull of Spite (TID 500) tombe au sol (10 % × 10). Les niveaux et le taux sont
//! remis à la fin.

use super::{enter_world, step, InWorld, Recv};
use archcore_net::schema::{self, chat_type, mini};
use archcore_net::{build_mini, decode_mini, Fields, Vec3};
use std::time::Duration;

/// Valiant, superviseur de la Battle Square (npc.ini 551), et un point à côté de lui.
const VALIANT: i64 = 0x4000_0000 + 551;
const NEAR_VALIANT: Vec3 = Vec3 { x: -467_000.0, y: 6_015.0, z: -62_400.0 };
/// Porte du premier étage (région 195).
const FLOOR_1: (f32, f32) = (-268_600.0, -237_000.0);

fn say(w: &InWorld, text: &str) -> Fields {
    let ch = &schema::CHATTING;
    Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text))
}

async fn gm(w: &mut InWorld, text: &str) -> anyhow::Result<()> {
    w.world.send(&schema::CHATTING, say(w, text)).await?;
    let r = w.world.expect_where(|q| q.ptype == schema::CHATTING.id, 5000, text).await?;
    println!("  {text} → {}", r.fields().text(6).unwrap_or_default());
    Ok(())
}

fn pvp_code(code: i64) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| q.is(&schema::PVP, Some(18)) && q.int(1) == Some(code)
}

/// Demande de dialogue à Valiant (AGPMEVENTNPCDIALOG op 0, fonction 21).
async fn talk(w: &mut InWorld) -> anyhow::Result<()> {
    let (d, eb) = (&schema::EVENT_NPC_DIALOG, &mini::EVENT_BASE);
    let base = build_mini(eb, &Fields::new(eb).with(0, 3).with(1, VALIANT).with(3, 21));
    w.world.send(d, Fields::new(d).with(0, 0).with(1, base).with(2, w.cid)).await
}

fn murderer_points(bytes: &[u8]) -> Option<i64> {
    let f = decode_mini(&mini::CHAR_FACTORS, bytes).ok()?;
    decode_mini(&mini::FACTOR_STATUS, f.bytes(1)?).ok()?.int(11)
}

pub async fn run(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let (mv, act, c, it) = (&schema::OPT_CHAR_MOVE, &schema::OPT_CHAR_ACTION, &schema::CHARACTER, &schema::ITEM);

    step(&format!("{cb} (niveau 1) parle au superviseur Valiant : refusé"));
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    let mut b = enter_world(&cfg, ub, pb, cb, "B-").await?;
    let (acid, bcid) = (a.cid as i64, b.cid as i64);
    let (level_a, level_b) = (a.level, b.level);
    gm(&mut a, &format!("/level 1 \"{cb}\"")).await?;
    // B n'a pas de droits MJ : A (MJ) le pose, le serveur refusant qu'il se téléporte lui-même.
    super::gm_place(&mut a, cb, NEAR_VALIANT).await?;
    b.world.send(mv, Fields::new(mv).with(0, b.cid).with(1, NEAR_VALIANT).with(2, NEAR_VALIANT).with(5, 0x04).with(6, 0)).await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    talk(&mut b).await?;
    b.world.expect_where(|q| q.is(&schema::EVENT_NPC_DIALOG, Some(1)), 3000, "dialogue accordé").await?;
    let refused = b.world.expect_where(pvp_code(18), 3000, "code 18 (niveau insuffisant)").await?;
    println!("  ✓ dialogue accordé, code 18 (niveau {})", refused.int(3).unwrap_or(0));

    step(&format!("{cb} (niveau 25) part pour le premier étage"));
    gm(&mut a, &format!("/level 25 \"{cb}\"")).await?;
    talk(&mut b).await?;
    let me = bcid;
    let tp = b.world.expect_where(|q| q.is(mv, None) && q.int(0) == Some(me), 5000, "voyage vers la Battle Square").await?;
    let at = tp.fields().vec3(1).unwrap_or_default();
    anyhow::ensure!(((at.x - FLOOR_1.0).abs() + (at.z - FLOOR_1.1).abs()) < 1000.0, "arrivé en {at:?}");
    b.world.expect_where(pvp_code(2), 3000, "code 2 (zone de combat)").await?;
    b.world.expect_where(pvp_code(13), 3000, "code 13 (Battle Square)").await?;
    println!("  ✓ porte du premier étage ({:.0}, {:.0}), codes 2 puis 13", at.x, at.z);

    step(&format!("{ca} (niveau 30) le rejoint par /move"));
    gm(&mut a, "/level 30").await?;
    // Les codes précèdent la réponse de la commande : pas de `gm`, qui écarte ce qui vient avant la réponse.
    let to = format!("/move {},{}", FLOOR_1.0 + 150.0, FLOOR_1.1);
    a.world.send(&schema::CHATTING, say(&a, &to)).await?;
    a.world.expect_where(pvp_code(2), 3000, "code 2 chez le MJ").await?;
    a.world.expect_where(pvp_code(13), 3000, "code 13 chez le MJ").await?;
    println!("  ✓ codes 2 et 13 à l'arrivée par commande MJ");

    step(&format!("{ca} tue {cb} : ni points de meurtrier, crâne au sol"));
    gm(&mut a, "/SetDrop 10").await?;
    gm(&mut a, "/invincible 1").await?;
    a.world.send(act, Fields::new(act).with(0, acid).with(1, bcid).with(6, 1)).await?;
    a.world.expect_where(|q| q.is(&schema::PVP, Some(18)) && q.int(1) == Some(12), 120_000, "message 12 (victoire)").await?;
    let skull = a.world.expect_where(|q: &Recv| q.is(it, Some(0)) && q.int(3) == Some(500), 5000, "crâne au sol (TID 500)").await?;
    println!("  ✓ {cb} tué ; Skull of Spite au sol (IID {})", skull.int(2).unwrap_or(0));
    let points = a.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(acid) && q.fields().bytes(7).and_then(murderer_points).is_some_and(|p| p > 0), 2000, "points de meurtrier").await;
    anyhow::ensure!(points.is_err(), "points de meurtrier donnés dans la Battle Square");
    println!("  ✓ aucun point de meurtrier");

    step("remise en état");
    b.world.send(c, Fields::new(c).with(0, 9).with(1, bcid)).await?;
    let _ = b.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(bcid) && q.int(11) == Some(0), 5000, "résurrection").await;
    gm(&mut a, "/SetDrop 1").await?;
    gm(&mut a, "/invincible 0").await?;
    gm(&mut a, &format!("/level {level_a}")).await?;
    gm(&mut a, &format!("/level {} \"{cb}\"", level_b.max(1))).await?;
    gm(&mut a, "/move -475955,-66219").await?;
    println!("\nBattle Square : OK");
    Ok(())
}
