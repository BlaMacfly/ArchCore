//! `archcore sim shrine` : combat de Shrine (AgpmShrineBattle 0x62, structure brute ; `world/shrine.rs`).
//!
//! GmTest (MJ, humain) et Grokk (compte test2, orc) demandent la liste (op 0 → op 2, 11, 12). Le MJ ouvre le Shrine de
//! la Foudre (`/shrine open 1`, op 1 état 1) ; Grokk, niveau 1, est refusé (code système 47), puis entre au niveau 50
//! (téléporté au point orc de la région 245), comme GmTest (niveau 60, point humain de la région 244). GmTest frappe
//! Grokk sans Ctrl (races différentes en combat de Shrine) jusqu'à la mort : aucun point de meurtrier bien qu'il ait
//! frappé le premier un innocent de 10 niveaux de moins ; Grokk ressuscite dans la zone orc. GmTest grave le sceau au
//! centre (op 1 état 2, message 9, jauge op 3), occupe (état 4), gagne (état 6, messages 17 et 22) : pièces 53000 et
//! charisme (messages 19 et 18), score (op 11). Quinze secondes plus tard, message 27, retour en ville, état 0.
//! `isole.sh` raccourcit la gravure et l'occupation (5 et 10 s) et coupe l'horaire (`Shrine.Enabled = 0`).

use super::{enter_world, step, InWorld, Recv};
use archcore_net::schema::{self, chat_type, mini, ptype, shrine_op};
use archcore_net::{decode_mini, Fields, Vec3};
use std::time::Duration;

const SHRINE: u32 = 1;
/// Points de résurrection des zones humaine (244) et orc (245), centre (236) du Shrine de la Foudre.
const HUMAN_ZONE: (f32, f32) = (640_000.0, -685_400.0);
const ORC_ZONE: (f32, f32) = (656_600.0, -685_400.0);
const CENTER: (f32, f32) = (648_600.0, -685_000.0);

fn say(w: &InWorld, text: &str) -> Fields {
    let ch = &schema::CHATTING;
    Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text))
}

async fn gm(w: &mut InWorld, text: &str) -> anyhow::Result<String> {
    w.world.send(&schema::CHATTING, say(w, text)).await?;
    let r = w.world.expect_where(|q| q.ptype == schema::CHATTING.id, 5000, text).await?;
    let reply = r.fields().text(6).unwrap_or_default();
    println!("  {text} → {reply}");
    Ok(reply)
}

fn raw(q: &Recv) -> Vec<u8> {
    q.packet.as_ref().map(|p| p.raw.clone()).unwrap_or_default()
}

fn u32_at(b: &[u8], off: usize) -> i64 {
    b.get(off..off + 4).map(|x| u32::from_le_bytes(x.try_into().unwrap()) as i64).unwrap_or(-1)
}

fn sh(op: i64) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| q.ptype == ptype::SHRINE_BATTLE && q.int(0) == Some(op)
}

/// Op 1 du Shrine dans l'état `state`.
fn state(st: i64) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| sh(shrine_op::STATE)(q) && u32_at(&raw(q), 0x18) == SHRINE as i64 && u32_at(&raw(q), 0x20 + 0x80) == st
}

/// Op 7, message `tid`.
fn message(tid: i64) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| sh(shrine_op::MESSAGE)(q) && u32_at(&raw(q), 0x1c) == tid
}

/// Position d'une téléportation (déplacement du serveur vers soi).
async fn landing(w: &mut InWorld, what: &str) -> anyhow::Result<Vec3> {
    let (mv, me) = (&schema::OPT_CHAR_MOVE, w.cid as i64);
    let q = w.world.expect_where(|q| q.is(mv, None) && q.int(0) == Some(me), 8000, what).await?;
    Ok(q.fields().vec3(1).unwrap_or_default())
}

fn near(p: Vec3, at: (f32, f32), within: f64) -> bool {
    (((p.x - at.0) as f64).powi(2) + ((p.z - at.1) as f64).powi(2)).sqrt() < within
}

fn murderer_points(bytes: &[u8]) -> Option<i64> {
    let f = decode_mini(&mini::CHAR_FACTORS, bytes).ok()?;
    decode_mini(&mini::FACTOR_STATUS, f.bytes(1)?).ok()?.int(11)
}

async fn join(w: &mut InWorld) -> anyhow::Result<()> {
    let plain = crate::world::shrine::raw_packet(shrine_op::JOIN, 0x20, |b| {
        b[0x18..0x1c].copy_from_slice(&SHRINE.to_le_bytes());
        b[0x1c..0x20].copy_from_slice(&w.cid.to_le_bytes());
    });
    println!("[{}] → AGPMSHRINEBATTLE op 9 (brut, {} o)", w.cid, plain.len());
    w.world.send_plain(plain).await
}

pub async fn run(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let (act, c) = (&schema::OPT_CHAR_ACTION, &schema::CHARACTER);

    step(&format!("{ca} (humain) et {cb} (orc) entrent ; liste des Shrines"));
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    let mut b = enter_world(&cfg, ub, pb, cb, "B-").await?;
    let (acid, bcid) = (a.cid as i64, b.cid as i64);
    let (level_a, level_b) = (a.level, b.level);
    let list = a.world.expect_where(sh(shrine_op::LIST), 3000, "op 2 à l'entrée").await?;
    anyhow::ensure!(raw(&list).len() == 0x981 && u32_at(&raw(&list), 0x40) == 1, "liste : {} o, premier Id {}", raw(&list).len(), u32_at(&raw(&list), 0x40));
    let ask = crate::world::shrine::raw_packet(shrine_op::LIST_REQUEST, 0x20, |x| x[0x1c..0x20].copy_from_slice(&a.cid.to_le_bytes()));
    a.world.send_plain(ask).await?;
    a.world.expect_where(sh(shrine_op::LIST), 3000, "op 2 après l'op 0").await?;
    let mine = a.world.expect_where(sh(shrine_op::MY_SCORE), 3000, "op 11").await?;
    let wins_before = u32_at(&raw(&mine), 0x1c);
    a.world.expect_where(sh(shrine_op::RACE_SCORE), 3000, "op 12").await?;
    println!("  ✓ op 2 à l'entrée (0x981 o) ; op 0 → op 2, 11 ({wins_before} victoire(s)), 12");

    step("ouverture forcée du Shrine de la Foudre");
    gm(&mut a, "/level 60").await?;
    gm(&mut a, "/invincible 1").await?;
    gm(&mut a, &format!("/level 1 \"{cb}\"")).await?;
    gm(&mut a, "/shrine open 1").await?;
    a.world.expect_where(state(1), 5000, "op 1 état 1 (GmTest)").await?;
    b.world.expect_where(state(1), 5000, "op 1 état 1 (Grokk)").await?;
    b.world.expect_where(message(crate::world::shrine::msg::STARTED), 3000, "message 5").await?;
    println!("  ✓ état 1 chez les deux joueurs, message 5");

    step(&format!("{cb} (niveau 1) demande à entrer : refusé"));
    join(&mut b).await?;
    let sm = &schema::SYSTEM_MESSAGE;
    let refused = b.world.expect_where(|q| q.is(sm, None) && q.int(1) == Some(crate::world::shrine::code::LEVEL), 3000, "code système 47").await?;
    println!("  ✓ code 47 (niveau {} exigé)", refused.int(2).unwrap_or(0));

    step("entrée de chacun dans la zone de sa race");
    gm(&mut a, &format!("/level 50 \"{cb}\"")).await?;
    join(&mut b).await?;
    let at = landing(&mut b, "téléportation de Grokk").await?;
    anyhow::ensure!(near(at, ORC_ZONE, 1500.0), "Grokk arrive en {at:?}");
    join(&mut a).await?;
    let at = landing(&mut a, "téléportation de GmTest").await?;
    anyhow::ensure!(near(at, HUMAN_ZONE, 1500.0), "GmTest arrive en {at:?}");
    println!("  ✓ zones orc (245) et humaine (244)");

    step(&format!("{ca} frappe {cb} sans Ctrl jusqu'à la mort"));
    gm(&mut a, &format!("/move {},{}", ORC_ZONE.0 - 200.0, ORC_ZONE.1)).await?;
    tokio::time::sleep(Duration::from_millis(600)).await;
    a.world.send(act, Fields::new(act).with(0, acid).with(1, bcid).with(6, 0)).await?;
    a.world.expect_where(|q| q.is(&schema::PVP, Some(2)), 8000, "inimitié sans Ctrl").await?;
    a.world.expect_where(|q| q.is(&schema::PVP, Some(18)) && q.int(1) == Some(12), 120_000, "message 12 (victoire)").await?;
    let points = a.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(acid) && q.fields().bytes(7).and_then(murderer_points).is_some_and(|p| p > 0), 2000, "points de meurtrier").await;
    anyhow::ensure!(points.is_err(), "points de meurtrier donnés dans le Shrine");
    println!("  ✓ {cb} tué, aucun point de meurtrier");

    step(&format!("{cb} ressuscite dans la zone orc"));
    b.world.send(c, Fields::new(c).with(0, 9).with(1, bcid)).await?;
    let at = landing(&mut b, "résurrection").await?;
    anyhow::ensure!(near(at, ORC_ZONE, 1500.0), "Grokk ressuscite en {at:?}");
    println!("  ✓ ressuscité en {:.0}, {:.0}", at.x, at.z);

    step(&format!("{ca} grave le sceau au centre, occupe et gagne"));
    gm(&mut a, &format!("/move {},{}", CENTER.0, CENTER.1)).await?;
    let carving = a.world.expect_where(state(2), 8000, "op 1 état 2 (gravure)").await?;
    anyhow::ensure!(u32_at(&raw(&carving), 0x20 + 0xb8) == acid && u32_at(&raw(&carving), 0x20 + 0x84) == 1, "graveur {} race {}", u32_at(&raw(&carving), 0x20 + 0xb8), u32_at(&raw(&carving), 0x20 + 0x84));
    a.world.expect_where(sh(shrine_op::GAUGE), 3000, "jauge op 3").await?;
    b.world.expect_where(message(crate::world::shrine::msg::KILL), 3000, "message 15 (tuer le graveur) chez Grokk").await?;
    println!("  ✓ gravure : état 2, jauge, message 15 aux orcs");
    a.world.expect_where(state(4), 15_000, "op 1 état 4 (occupation)").await?;
    println!("  ✓ occupation par les humains");
    a.world.expect_where(state(6), 20_000, "op 1 état 6 (fin)").await?;
    a.world.expect_where(message(crate::world::shrine::msg::WON), 3000, "message 17").await?;
    let coins = a.world.expect_where(message(crate::world::shrine::msg::COINS), 5000, "message 19 (pièces)").await?;
    anyhow::ensure!(u32_at(&raw(&coins), 0x20) == 16, "pièces de GmTest : {}", u32_at(&raw(&coins), 0x20));
    let score = a.world.expect_where(sh(shrine_op::MY_SCORE), 5000, "op 11 après la victoire").await?;
    anyhow::ensure!(u32_at(&raw(&score), 0x1c) == wins_before + 1, "victoires {} (avant {wins_before})", u32_at(&raw(&score), 0x1c));
    let theirs = b.world.expect_where(message(crate::world::shrine::msg::COINS), 5000, "message 19 chez Grokk").await?;
    anyhow::ensure!(u32_at(&raw(&theirs), 0x20) == 1, "pièces de Grokk : {}", u32_at(&raw(&theirs), 0x20));
    println!("  ✓ victoire humaine : 16 pièces à GmTest, 1 à Grokk, op 11");

    step("fin : retour en ville");
    a.world.expect_where(message(crate::world::shrine::msg::TO_TOWN), 25_000, "message 27").await?;
    let at = landing(&mut a, "retour en ville").await?;
    anyhow::ensure!(!near(at, CENTER, 50_000.0), "toujours au Shrine : {at:?}");
    a.world.expect_where(state(0), 5000, "op 1 état 0").await?;
    println!("  ✓ message 27, retour en {:.0}, {:.0}, état 0", at.x, at.z);

    step("remise des niveaux");
    gm(&mut a, "/invincible 0").await?;
    gm(&mut a, &format!("/level {level_a}")).await?;
    gm(&mut a, &format!("/level {} \"{cb}\"", level_b.max(1))).await?;
    println!("\ncombat de Shrine : OK");
    Ok(())
}
