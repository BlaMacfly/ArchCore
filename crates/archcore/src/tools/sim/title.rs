//! `archcore sim title` : titres (AgpmTitle 0x60, structure brute ; `world/titles.rs`).
//!
//! GmTest (MJ, chevalier humain) entre sans titre : op 5 unique, 0x5C = 0. Il parle à Warrante (« <Title Manager> »,
//! npc.ini 535, près d'Anchorville) : op 10 au lieu du dialogue. Au niveau 10, il ne peut pas viser Novice Human Knight
//! (140001, niveau 15 exigé : 6.1 résultat 0) ; au niveau 15, l'objectif est créé (6.1) et sa condition de niveau déjà
//! remplie (6.3 = 15) ; au niveau 16, le compteur suit (6.3 = 16). Il termine (6.5 puis op 1) et l'équipe (op 3, op 9,
//! Str + 2 de la compétence « <Title> Novice Knight ») : SimA, posé à côté, reçoit l'op 9 de GmTest, puis de nouveau
//! quand GmTest rentre dans sa vue. Le retrait aussitôt après est refusé (op 3 code 3, `Title.ChangeDelaySeconds`). La
//! suppression du titre actif l'efface chez SimA (op 9 bUse 0) et répond op 8 ; un objectif de collecte (10001) est
//! fixé puis abandonné (op 7 → op 8). `isole.sh` vide les tables des titres et l'heure du dernier changement.

use super::{enter_world, gm_place, step, InWorld, Recv};
use archcore_net::schema::{self, chat_type, mini, ptype, title_op, title_quest};
use archcore_net::{build_mini, Fields, Vec3};
use std::time::Duration;

const NOVICE_KNIGHT: u32 = 140_001;
const COLLECTOR: u32 = 10_001;
/// Warrante, « <Title Manager> » d'Anchorville (npc.ini 535), un point à côté, et un point hors de sa vue.
const WARRANTE: i64 = 0x4000_0000 + 535;
const NEAR_WARRANTE: Vec3 = Vec3 { x: -483_900.0, y: 5_060.0, z: -64_900.0 };
const BESIDE: Vec3 = Vec3 { x: -483_700.0, y: 5_060.0, z: -64_800.0 };
const FAR: Vec3 = Vec3 { x: -443_900.0, y: 5_060.0, z: -64_900.0 };

fn say(w: &InWorld, text: &str) -> Fields {
    let ch = &schema::CHATTING;
    Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text))
}

/// Commande MJ envoyée, sans attendre la réponse (les paquets qu'elle provoque arrivent avant).
async fn gm_send(w: &mut InWorld, text: &str) -> anyhow::Result<()> {
    tokio::time::sleep(Duration::from_millis(150)).await;
    w.world.send(&schema::CHATTING, say(w, text)).await
}

async fn gm_reply(w: &mut InWorld, text: &str) -> anyhow::Result<String> {
    let r = w.world.expect_where(|q| q.ptype == schema::CHATTING.id, 5000, text).await?;
    let reply = r.fields().text(6).unwrap_or_default();
    println!("  {text} → {reply}");
    Ok(reply)
}

async fn gm(w: &mut InWorld, text: &str) -> anyhow::Result<String> {
    gm_send(w, text).await?;
    gm_reply(w, text).await
}

fn raw(q: &Recv) -> Vec<u8> {
    q.packet.as_ref().map(|p| p.raw.clone()).unwrap_or_default()
}

fn i32_at(b: &[u8], off: usize) -> i64 {
    b.get(off..off + 4).map(|x| i32::from_le_bytes(x.try_into().unwrap()) as i64).unwrap_or(-1)
}

/// Paquet AgpmTitle d'opération `op` (et de sous-opération `sub` pour l'op 6).
fn title(op: i64, sub: Option<i64>) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| q.ptype == ptype::TITLE && q.int(0) == Some(op) && sub.is_none_or(|s| i32_at(&raw(q), 0x4C) == s)
}

/// Demande du client comme l'envoie Archonia : structure de `size` octets, nom en 0x16, TID en 0x48.
async fn request(w: &mut InWorld, name: &str, op: i64, size: usize, tid: u32, fill: impl FnOnce(&mut [u8])) -> anyhow::Result<()> {
    let plain = crate::world::shrine::raw_packet_of(ptype::TITLE, op, size, |b| {
        crate::world::guild::society::put_text(b, 0x16, 49, name);
        b[0x48..0x4C].copy_from_slice(&tid.to_le_bytes());
        fill(b);
    });
    println!("[{}] → AGPMTITLE op {op} (brut, {} o, titre {tid})", w.cid, plain.len());
    w.world.send_plain(plain).await
}

async fn quest(w: &mut InWorld, name: &str, sub: i64, size: usize, tid: u32) -> anyhow::Result<()> {
    request(w, name, title_op::QUEST, size, tid, |b| b[0x4C..0x50].copy_from_slice(&(sub as i32).to_le_bytes())).await
}

async fn equip(w: &mut InWorld, name: &str, tid: u32, on: bool) -> anyhow::Result<()> {
    request(w, name, title_op::USE, 0x54, tid, |b| {
        b[0x4C] = 2;
        b[0x50] = on as u8;
    })
    .await
}

/// Force (centièmes) des facteurs d'un AGPMCHARACTER op 1 (champ 7).
fn strength(q: &Recv) -> Option<i64> {
    let f = archcore_net::decode_mini(&mini::CHAR_FACTORS, q.fields().bytes(7)?).ok()?;
    archcore_net::decode_mini(&mini::FACTOR_STATUS, f.bytes(1)?).ok()?.int(1)
}

/// Op 9 reçu : (nom du porteur, TID, bUse).
fn near_of(q: &Recv) -> (String, i64, i64) {
    let b = raw(q);
    (crate::world::guild::society::get_text(&b, 0x16, 49), i32_at(&b, 0x48), i32_at(&b, 0x4C))
}

pub async fn run(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let c = &schema::CHARACTER;

    step(&format!("{ca} entre sans titre"));
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    let (acid, level_a) = (a.cid as i64, a.level);
    let list = a.world.expect_where(title(title_op::LIST_RESULT, None), 3000, "op 5 à l'entrée").await?;
    anyhow::ensure!(raw(&list).len() == 0x69 && i32_at(&raw(&list), 0x5C) == 0 && i32_at(&raw(&list), 0x48) == 0, "op 5 : {} o, 0x5C = {}", raw(&list).len(), i32_at(&raw(&list), 0x5C));
    println!("  ✓ op 5 unique, liste vide (0x5C = 0)");

    step("Warrante (<Title Manager>) ouvre les fenêtres des titres");
    gm(&mut a, "/level 10").await?;
    gm(&mut a, &format!("/move {},{}", NEAR_WARRANTE.x, NEAR_WARRANTE.z)).await?;
    tokio::time::sleep(Duration::from_millis(400)).await;
    let (d, eb) = (&schema::EVENT_NPC_DIALOG, &mini::EVENT_BASE);
    let base = build_mini(eb, &Fields::new(eb).with(0, 3).with(1, WARRANTE).with(3, 21));
    a.world.send(d, Fields::new(d).with(0, 0).with(1, base).with(2, a.cid)).await?;
    let open = a.world.expect_where(|q| title(title_op::UI_OPEN, None)(q) || q.is(d, Some(1)), 3000, "op 10").await?;
    anyhow::ensure!(open.ptype == ptype::TITLE && raw(&open).len() == 0x4D && i32_at(&raw(&open), 0x48) == 0, "réponse au dialogue : type 0x{:x}", open.ptype);
    println!("  ✓ op 10 (0x4D o, TID 0 : Schola écartées) au lieu du dialogue");

    step(&format!("objectif {NOVICE_KNIGHT} (Novice Human Knight) : refusé au niveau 10, créé au niveau 15"));
    quest(&mut a, ca, title_quest::ADD, 0x50, NOVICE_KNIGHT).await?;
    let r = a.world.expect_where(title(title_op::QUEST, Some(title_quest::ADD_RESULT)), 3000, "6.1 refusé").await?;
    anyhow::ensure!(i32_at(&raw(&r), 0x58) == 0, "6.1 au niveau 10 : résultat {}", i32_at(&raw(&r), 0x58));
    gm(&mut a, "/level 15").await?;
    quest(&mut a, ca, title_quest::ADD, 0x50, NOVICE_KNIGHT).await?;
    let r = a.world.expect_where(title(title_op::QUEST, Some(title_quest::ADD_RESULT)), 3000, "6.1").await?;
    anyhow::ensure!(i32_at(&raw(&r), 0x58) == 1 && i32_at(&raw(&r), 0x50) > 0, "6.1 : résultat {}, date {}", i32_at(&raw(&r), 0x58), i32_at(&raw(&r), 0x50));
    let chk = a.world.expect_where(title(title_op::QUEST, Some(title_quest::CHECK)), 3000, "6.3 (condition remplie)").await?;
    anyhow::ensure!(raw(&chk).len() == 0x69 && i32_at(&raw(&chk), 0x50) == 15 && i32_at(&raw(&chk), 0x64) == 1, "6.3 : {}", i32_at(&raw(&chk), 0x50));
    println!("  ✓ 6.1 résultat 0 au niveau 10 ; 6.1 puis 6.3 = 15 au niveau 15");

    step("le compteur suit le niveau");
    gm_send(&mut a, "/level 16").await?;
    let f = a.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(acid) && q.fields().bytes(7).is_some(), 3000, "facteurs du niveau 16").await?;
    let str_before = strength(&f).unwrap_or(-1);
    let chk = a.world.expect_where(title(title_op::QUEST, Some(title_quest::CHECK)), 3000, "6.3 au niveau 16").await?;
    anyhow::ensure!(i32_at(&raw(&chk), 0x50) == 16, "6.3 : {}", i32_at(&raw(&chk), 0x50));
    gm_reply(&mut a, "/level 16").await?;
    println!("  ✓ 6.3 = 16");

    step("objectif terminé : 6.5 puis op 1");
    quest(&mut a, ca, title_quest::COMPLETE, 0x54, NOVICE_KNIGHT).await?;
    let done = a.world.expect_where(title(title_op::QUEST, Some(title_quest::COMPLETE_RESULT)), 3000, "6.5").await?;
    anyhow::ensure!(i32_at(&raw(&done), 0x58) == 1, "6.5 : résultat {}", i32_at(&raw(&done), 0x58));
    let add = a.world.expect_where(title(title_op::ADD_RESULT, None), 3000, "op 1").await?;
    anyhow::ensure!(i32_at(&raw(&add), 0x4C) == 1 && i32_at(&raw(&add), 0x48) == NOVICE_KNIGHT as i64, "op 1 : {}", i32_at(&raw(&add), 0x4C));
    println!("  ✓ 6.5 (résultat 1), op 1 (titre {NOVICE_KNIGHT} ajouté)");

    step(&format!("{cb} entre et se tient à côté de {ca}"));
    let mut b = enter_world(&cfg, ub, pb, cb, "B-").await?;
    gm_place(&mut a, cb, BESIDE).await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    step(&format!("{ca} équipe le titre : op 3, op 9 chez les deux, facteurs"));
    equip(&mut a, ca, NOVICE_KNIGHT, true).await?;
    let used = a.world.expect_where(title(title_op::USE_RESULT, None), 3000, "op 3").await?;
    anyhow::ensure!(raw(&used).len() == 0x65 && i32_at(&raw(&used), 0x4C) == 1 && i32_at(&raw(&used), 0x50) == 1, "op 3 : bUse {}, résultat {}", i32_at(&raw(&used), 0x4C), i32_at(&raw(&used), 0x50));
    let mine = a.world.expect_where(title(title_op::USE_NEAR, None), 3000, "op 9 (soi)").await?;
    anyhow::ensure!(near_of(&mine) == (ca.to_string(), NOVICE_KNIGHT as i64, 1), "op 9 : {:?}", near_of(&mine));
    let f = a.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(acid) && q.fields().bytes(7).is_some(), 3000, "facteurs du titre").await?;
    // « <Title> Novice Knight » (1555) : Str +2 et Con +2 tant que le titre est équipé.
    anyhow::ensure!(strength(&f) == Some(str_before + 200), "force {:?}, {str_before} avant le titre", strength(&f));
    let theirs = b.world.expect_where(title(title_op::USE_NEAR, None), 3000, "op 9 chez SimA").await?;
    anyhow::ensure!(near_of(&theirs) == (ca.to_string(), NOVICE_KNIGHT as i64, 1), "op 9 chez SimA : {:?}", near_of(&theirs));
    println!("  ✓ op 3 (1, 1), op 9 à {ca} et à {cb}, force {} → {} (compétence 1555)", str_before / 100, str_before / 100 + 2);

    step(&format!("{cb} perd {ca} de vue puis le retrouve : op 9 à l'entrée dans la vue"));
    gm_place(&mut a, cb, FAR).await?;
    tokio::time::sleep(Duration::from_millis(300)).await;
    gm_place(&mut a, cb, BESIDE).await?;
    let again = b.world.expect_where(title(title_op::USE_NEAR, None), 5000, "op 9 au retour").await?;
    anyhow::ensure!(near_of(&again) == (ca.to_string(), NOVICE_KNIGHT as i64, 1), "op 9 au retour : {:?}", near_of(&again));
    println!("  ✓ op 9 de {ca} chez {cb} quand il revient en vue");

    step("retrait aussitôt après : refusé (moins de 2 h)");
    equip(&mut a, ca, NOVICE_KNIGHT, false).await?;
    let refused = a.world.expect_where(title(title_op::USE_RESULT, None), 3000, "op 3 refusé").await?;
    let rb = raw(&refused);
    anyhow::ensure!(i32_at(&rb, 0x50) == 0 && i32_at(&rb, 0x60) == 3 && i32_at(&rb, 0x58) > 0, "op 3 : résultat {}, code {}", i32_at(&rb, 0x50), i32_at(&rb, 0x60));
    println!("  ✓ op 3 résultat 0, code 3, heure du dernier changement {}", i32_at(&rb, 0x58));

    step("suppression du titre actif, objectif abandonné");
    request(&mut a, ca, title_op::DELETE, 0x4C, NOVICE_KNIGHT, |_| {}).await?;
    let off = b.world.expect_where(title(title_op::USE_NEAR, None), 3000, "op 9 bUse 0 chez SimA").await?;
    anyhow::ensure!(near_of(&off) == (ca.to_string(), NOVICE_KNIGHT as i64, 0), "op 9 : {:?}", near_of(&off));
    let del = a.world.expect_where(title(title_op::DELETE_RESULT, None), 3000, "op 8").await?;
    anyhow::ensure!(i32_at(&raw(&del), 0x4C) == 1, "op 8 : {}", i32_at(&raw(&del), 0x4C));
    quest(&mut a, ca, title_quest::ADD, 0x50, COLLECTOR).await?;
    let r = a.world.expect_where(title(title_op::QUEST, Some(title_quest::ADD_RESULT)), 3000, "6.1 collecte").await?;
    anyhow::ensure!(i32_at(&raw(&r), 0x58) == 1, "6.1 collecte : {}", i32_at(&raw(&r), 0x58));
    request(&mut a, ca, title_op::DELETE, 0x4C, COLLECTOR, |_| {}).await?;
    let del = a.world.expect_where(title(title_op::DELETE_RESULT, None), 3000, "op 8 (abandon)").await?;
    anyhow::ensure!(i32_at(&raw(&del), 0x4C) == 1, "op 8 : {}", i32_at(&raw(&del), 0x4C));
    let state = gm(&mut a, "/title").await?;
    anyhow::ensure!(state.contains("aucun"), "/title : {state}");
    println!("  ✓ op 9 bUse 0 chez {cb}, op 8 ; objectif {COLLECTOR} fixé puis abandonné (op 8)");

    step("remise du niveau");
    gm(&mut a, &format!("/level {}", level_a.max(1))).await?;
    println!("\ntitres : OK");
    Ok(())
}
