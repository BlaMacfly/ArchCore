//! `archcore sim archlord` : titre d'ArchLord et guerre de Rensphere (AgpmArchlord 0x55 ; `world/archlord.rs`).
//!
//! GmTest (MJ) crée la guilde SimLordA, qui reçoit Inflame (`/siege owner`). `/archlord set SimA` : op 0 « SimA » reçu
//! par les deux, SimA reçoit les compétences de l'ArchLord. `/siege start Rensphere` : op 5 étape 1 pour les deux,
//! SimLordA assaillante ; GmTest grave le sceau de la fontaine d'Archon (op 0x1B, 0x10 puis 0x12) : étape 3 et statut 7 ;
//! `/siege end Rensphere` : étape 4, op 1 « SimA » puis op 0 « GmTest » reçus par SimA, GmTest reçoit ses compétences.
//! GmTest lance Rain Control (522) : pluie (AgpmEventNature 3) reçue par les deux ; `/guardactivate` : op 2 « Royal
//! Knight », invocation (AgpmSummons op 0) et op 4 (1/10) ; `/archlord <message>` reçu par SimA. SimA, qui n'est plus
//! l'ArchLord, lance Rain Control : refusé ; `/guardactivate` : op 6 message 1. Fin : `/archlord clear` (op 1), temps
//! clair, château rendu.
//!
//! Configuration du serveur du simulateur : `Siege.CarveSeconds = 5` ; `isole.sh` coupe le calendrier
//! (`ARCHCORE_SIEGE_ENABLED=0`) et vide les tables `castle`, `castle_application`, `archlord` et des taxes.

use super::{cast_fields, enter_world, skill_info, step, InWorld, Recv, SeenMonster};
use archcore_net::schema::{self, archlord_msg, archlord_op, archlord_step, chat_type, mini, siege_op, siege_result, weather};
use archcore_net::{decode_mini, Fields};
use std::time::Duration;

const GUILD: &str = "SimLordA";
const PASSWORD: &str = "simpass";
const RENSPHERE: &str = "Rensphere";
/// Devant la fontaine d'Archon (198), « trône » de Rensphere.
const FOUNTAIN: i64 = 198;
const FOUNTAIN_AT: (f32, f32) = (-48_000.0, -74_900.0);
const RAIN_CONTROL: i64 = 522;

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

fn al(op: i64) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| q.is(&schema::ARCHLORD, Some(op))
}

/// Op 0 ou 1 portant ce nom.
fn named(op: i64, name: &'static str) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| al(op)(q) && q.fields().text(1).as_deref() == Some(name)
}

fn step_is(s: i64) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| al(archlord_op::STEP)(q) && q.int(4) == Some(s)
}

/// Tous les paquets reçus pendant `ms` millisecondes.
async fn collect(w: &mut InWorld, ms: u64) -> Vec<Recv> {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(ms);
    let mut out = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now()).as_millis() as u64;
        if left == 0 {
            return out;
        }
        match w.world.expect_where(|_| true, left, "paquets").await {
            Ok(q) => out.push(q),
            Err(_) => return out,
        }
    }
}

/// Identifiant de Rain Control parmi les compétences reçues (AGPMSKILL op 0).
fn rain_id(list: &[Recv]) -> Option<i64> {
    list.iter().filter(|q| q.is(&schema::SKILL, Some(0))).filter_map(|q| q.packet.as_ref().map(skill_info)).find(|(_, tid, _)| *tid == RAIN_CONTROL).map(|(id, _, _)| id)
}

pub async fn run(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let gd = &schema::GUILD;

    step(&format!("{ca} (MJ) et {cb} entrent ; guilde {GUILD}, propriétaire d'Inflame"));
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    let mut b = enter_world(&cfg, ub, pb, cb, "B-").await?;
    let (level_a, acid) = (a.level, a.cid as i64);
    gm(&mut a, "/level 60").await?;
    gm(&mut a, "/invincible 1").await?;
    gm(&mut a, "/money 100000").await?;
    a.world.send(gd, Fields::new(gd).with(0, 0).with(1, a.cid).with(2, GUILD).with(9, PASSWORD)).await?;
    a.world.expect_where(|q| q.is(gd, Some(10)) && q.int(1) == Some(11), 3000, "création de la guilde").await?;
    gm(&mut a, &format!("/siege owner Inflame {GUILD}")).await?;

    step(&format!("/archlord set {cb} : op 0 pour tous, compétences de l'ArchLord à {cb}"));
    a.world.send(&schema::CHATTING, say(&a, &format!("/archlord set {cb}"))).await?;
    a.world.expect_where(named(archlord_op::SET, "SimA"), 3000, "op 0 « SimA » chez GmTest").await?;
    b.world.expect_where(named(archlord_op::SET, "SimA"), 3000, "op 0 « SimA » chez SimA").await?;
    let got = collect(&mut b, 1500).await;
    let b_rain = rain_id(&got).ok_or_else(|| anyhow::anyhow!("Rain Control non reçue par SimA"))?;
    let given = got.iter().filter(|q| q.is(&schema::SKILL, Some(0))).count();
    println!("  ✓ op 0 « {cb} » ; {cb} reçoit {given} compétence(s) d'ArchLord (Rain Control : {b_rain})");

    step("/siege start Rensphere : étape 1 pour tous, SimLordA assaillante");
    a.world.send(&schema::CHATTING, say(&a, &format!("/move {},{}", FOUNTAIN_AT.0, FOUNTAIN_AT.1))).await?;
    collect(&mut a, 1500).await;
    a.world.send(&schema::CHATTING, say(&a, &format!("/siege start {RENSPHERE}"))).await?;
    let got = collect(&mut a, 2500).await;
    anyhow::ensure!(got.iter().any(step_is(archlord_step::PORTAL)), "op 5 étape 1 absente chez GmTest");
    b.world.expect_where(step_is(archlord_step::PORTAL), 3000, "op 5 étape 1 chez SimA").await?;
    let seen: Vec<SeenMonster> = got
        .iter()
        .filter(|q| q.is(&schema::OPT_VIEW, Some(1)))
        .filter_map(|q| decode_mini(&mini::OPT_VIEW_CHAR, q.fields().bytes(1).unwrap_or_default()).ok())
        .map(|v| SeenMonster::from_view(&v))
        .collect();
    let fountain = seen.iter().find(|m| m.tid == FOUNTAIN).map(|m| m.cid).ok_or_else(|| anyhow::anyhow!("fontaine d'Archon pas en vue : {:?}", seen.iter().map(|m| m.tid).collect::<Vec<_>>()))?;
    println!("  ✓ étape 1 pour {ca} et {cb} ; fontaine d'Archon {fountain:#x}");

    step(&format!("{ca} grave le sceau de Rensphere : étape 3"));
    let s = &schema::SIEGE_WAR;
    a.world.send(s, Fields::new(s).with(0, siege_op::CARVE).with(3, a.cid).with(5, fountain)).await?;
    a.world.expect_where(|q| q.is(s, Some(siege_op::RESULT)) && q.int(1) == Some(siege_result::CARVE_START), 3000, "op 2 0x10").await?;
    a.world.expect_where(|q| q.is(s, Some(siege_op::RESULT)) && q.int(1) == Some(siege_result::CARVE_DONE), 15_000, "op 2 0x12").await?;
    b.world.expect_where(step_is(archlord_step::BATTLE), 5000, "op 5 étape 3 chez SimA").await?;
    println!("  ✓ carving fait, étape 3 (bataille de l'ArchLord)");

    step(&format!("/siege end Rensphere : {ca} devient l'ArchLord"));
    a.world.send(&schema::CHATTING, say(&a, &format!("/siege end {RENSPHERE}"))).await?;
    b.world.expect_where(step_is(archlord_step::END), 5000, "op 5 étape 4 chez SimA").await?;
    b.world.expect_where(named(archlord_op::CANCEL, "SimA"), 3000, "op 1 « SimA » chez SimA").await?;
    b.world.expect_where(named(archlord_op::SET, "GmTest"), 3000, "op 0 « GmTest » chez SimA").await?;
    let got = collect(&mut a, 2000).await;
    anyhow::ensure!(got.iter().any(named(archlord_op::SET, "GmTest")), "op 0 « GmTest » absent chez GmTest");
    let a_rain = rain_id(&got).ok_or_else(|| anyhow::anyhow!("Rain Control non reçue par GmTest"))?;
    println!("  ✓ étape 4 ; {cb} : op 1 « {cb} » puis op 0 « {ca} » ; {ca} reçoit Rain Control ({a_rain})");

    step(&format!("Rain Control de {ca} : pluie pour tous"));
    a.world.send(&schema::SKILL, cast_fields(a_rain, acid)).await?;
    let rain = |q: &Recv| q.is(&schema::EVENT_NATURE, None) && q.int(0) == Some(weather::RAIN);
    a.world.expect_where(rain, 10_000, "pluie chez GmTest").await?;
    b.world.expect_where(rain, 10_000, "pluie chez SimA").await?;
    println!("  ✓ météo 3 reçue par {ca} et {cb}");

    step("garde royal, message de l'ArchLord");
    a.world.send(&schema::CHATTING, say(&a, "/guardactivate")).await?;
    let added = a.world.expect_where(al(archlord_op::GUARD_ADD), 3000, "op 2").await?;
    let guard = added.fields().text(2).unwrap_or_default();
    anyhow::ensure!(!guard.is_empty(), "garde refusé");
    let info = a.world.expect_where(al(archlord_op::GUARD_INFO), 3000, "op 4").await?;
    anyhow::ensure!(info.int(5) == Some(1) && info.int(6).is_some_and(|m| m >= 1), "gardes {:?}/{:?}", info.int(5), info.int(6));
    a.world.send(&schema::CHATTING, say(&a, "/archlord Salut à tous")).await?;
    let heard = |q: &Recv| q.ptype == schema::CHATTING.id && q.fields().text(6).is_some_and(|t| t.contains("[ArchLord] GmTest"));
    let msg = b.world.expect_where(heard, 3000, "message de l'ArchLord").await?;
    a.world.expect_where(heard, 3000, "message de l'ArchLord chez lui").await?;
    println!("  ✓ op 2 « {guard} », op 4 {}/{} ; {cb} lit « {} »", info.int(5).unwrap_or(0), info.int(6).unwrap_or(0), msg.fields().text(6).unwrap_or_default());

    step(&format!("{cb}, qui n'est plus l'ArchLord : compétence et garde refusées"));
    let cbid = b.cid as i64;
    b.world.send(&schema::SKILL, cast_fields(b_rain, cbid)).await?;
    b.world.expect_where(|q| q.ptype == schema::CHATTING.id && q.fields().text(6).is_some_and(|t| t.contains("Seul l'ArchLord")), 3000, "refus de Rain Control").await?;
    b.world.send(&schema::CHATTING, say(&b, "/guardactivate")).await?;
    b.world.expect_where(|q| al(archlord_op::MESSAGE)(q) && q.int(7) == Some(archlord_msg::GUARD_ADD_UNAVAILABLE), 3000, "op 6 message 1").await?;
    let late = b.world.expect_where(|q| q.is(&schema::EVENT_NATURE, None), 5000, "météo").await;
    anyhow::ensure!(late.is_err(), "la compétence refusée a changé la météo");
    println!("  ✓ Rain Control refusée à {cb}, op 6 message 1");

    step("remise en état");
    gm(&mut a, "/archlord clear").await?;
    b.world.expect_where(named(archlord_op::CANCEL, "GmTest"), 3000, "op 1 « GmTest »").await?;
    gm(&mut a, "/weather clear").await?;
    gm(&mut a, "/siege owner Inflame -").await?;
    gm(&mut a, "/invincible 0").await?;
    gm(&mut a, &format!("/level {level_a}")).await?;
    println!("\nArchLord : OK");
    Ok(())
}
