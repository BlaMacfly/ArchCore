//! `archcore sim siege` : guerre de siège du château d'Inflame (AgpmSiegeWar 0x49, AgpmEventSiegeWarNPC 0x50, AgpmWarInfo
//! 0x61 ; `world/siege/`).
//!
//! GmTest (MJ) crée la guilde SimSiegeA, SimA la guilde SimSiegeB ; `/siege owner Inflame SimSiegeB` donne le château à
//! SimA. GmTest ne peut pas déclarer (code 3) ; SimA déclare le siège pour le lendemain à 20 h (code 0x0B, statut 1).
//! Chez le commandant Garrison (0x50 op 1 → op 0 du château, op 2), GmTest candidate à la défense (8), de nouveau
//! (6 : déjà inscrite), SimA voit sa candidature (op 8 → op 10), GmTest la retire (0) et candidate à l'attaque (9, liste
//! op 7 → op 9) ; SimA ne peut pas attaquer son propre château (6). `/siege start Inflame` : statut 3, op 0x1C code 1,
//! WarInfo étape 1, op 0xE. GmTest tue SimA sans Ctrl et sans points de meurtrier ; SimA ressuscite au point intérieur des
//! défenseurs (op 0x13). Objets ramenés à 1 PV (`/siege hp`) : la porte intérieure est refusée tant que la porte
//! extérieure tient ; porte extérieure (code 4 type 3, étape 2), porte intérieure (étape 3), trône (code 5, statut 4),
//! deux Yeux d'Archon (code 6, statut 5) ; carving de GmTest (op 2 0x10, code 8, puis 0x12, code 10, carve guild
//! SimSiegeA) ; `/siege end Inflame` : SimSiegeA propriétaire (code 11, étape 9), SimA renvoyée en ville ; 30 s plus tard
//! statut 0 et WarInfo op 6.
//!
//! Configuration du serveur du simulateur (`$TEMP/claude/scan/conf-mig/world.conf`) : `Siege.MinGuildMembers = 1`,
//! `Siege.MinGuildAgeDays = 0` (guildes d'un membre créées à l'instant) et `Siege.CarveSeconds = 5` ; `isole.sh` coupe le
//! calendrier (`ARCHCORE_SIEGE_ENABLED=0`) et vide les tables `castle` et `castle_application`.

use super::{enter_world, step, InWorld, Recv, SeenMonster};
use archcore_net::schema::{self, chat_type, mini, siege_msg, siege_npc_op, siege_op, siege_result, war_info_op};
use archcore_net::{build_mini, decode_mini, Fields, Value, Vec3};
use std::time::Duration;

const CASTLE: &str = "Inflame";
const GA: &str = "SimSiegeA";
const GB: &str = "SimSiegeB";
const PASSWORD: &str = "simpass";
/// Commandant de siège Garrison (PNJ 373), dans les abords d'Inflame (région 76).
const GARRISON: u32 = 0x4000_0000 + 373;
const GARRISON_AT: (f32, f32) = (-354_400.0, -49_400.0);
/// Devant la porte extérieure, près de la porte intérieure, du trône, des deux Yeux.
const BEFORE_GATE: (f32, f32) = (-354_400.0, -29_000.0);
const INNER_GATE_AT: (f32, f32) = (-354_400.0, -17_000.0);
const THRONE_AT: (f32, f32) = (-354_400.0, -1_400.0);
const WEST_EYE_AT: (f32, f32) = (-362_000.0, -8_600.0);
const EAST_EYE_AT: (f32, f32) = (-346_800.0, -8_600.0);

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

/// Objets de siège montrés : (CID, TID).
fn views(list: &[Recv]) -> Vec<SeenMonster> {
    let ov = &schema::OPT_VIEW;
    list.iter()
        .filter(|q| q.is(ov, Some(1)))
        .filter_map(|q| decode_mini(&mini::OPT_VIEW_CHAR, q.fields().bytes(1).unwrap_or_default()).ok())
        .filter(|v| v.int(0).unwrap_or(0) >= crate::world::monsters::MONSTER_CID_BASE as i64)
        .map(|v| SeenMonster::from_view(&v))
        .collect()
}

fn sw(op: i64) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| q.is(&schema::SIEGE_WAR, Some(op))
}

/// Op 2 de code `r`.
fn result(r: i64) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| sw(siege_op::RESULT)(q) && q.int(1) == Some(r)
}

fn castle_of(q: &Recv) -> Option<Fields> {
    decode_mini(&mini::SIEGE_CASTLE, q.fields().bytes(2)?).ok()
}

/// Op 0 d'Inflame au statut `s`.
fn castle_status(s: i64) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| sw(siege_op::CASTLE)(q) && castle_of(q).is_some_and(|c| c.text(0).as_deref() == Some(CASTLE) && c.int(6) == Some(s))
}

/// Op 0x1C de code `code` (et de type d'objet `object`, s'il est donné).
fn message(code: i64, object: Option<i64>) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| {
        sw(siege_op::STATUS_MSG)(q)
            && q.fields().bytes(8).and_then(|b| decode_mini(&mini::SIEGE_STATUS_MSG, b).ok()).is_some_and(|m| m.int(0) == Some(code) && (object.is_none() || m.int(4) == object))
    }
}

/// WarInfo op 0, étape `step` d'une guerre de siège.
fn war_step(step: i64) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| q.ptype == schema::WAR_INFO.id && q.int(0) == Some(war_info_op::STEP) && q.int(1) == Some(war_info_op::SIEGE_WAR) && q.int(2) == Some(step)
}

fn base(w: &InWorld, op: i64) -> Fields {
    Fields::new(&schema::SIEGE_WAR).with(0, op).with(2, crate::world::siege::castle_name_mini(CASTLE)).with(3, w.cid)
}

fn page(n: i64) -> Vec<u8> {
    build_mini(&mini::SIEGE_GUILD_PAGE, &Fields::new(&mini::SIEGE_GUILD_PAGE).with(0, n))
}

/// Noms des guildes d'une page (champ 4).
fn page_guilds(q: &Recv) -> Vec<String> {
    let Some(p) = q.fields().bytes(4).and_then(|b| decode_mini(&mini::SIEGE_GUILD_PAGE, b).ok()) else { return Vec::new() };
    p.list(2)
        .unwrap_or_default()
        .iter()
        .filter_map(|e| match e {
            Value::Bytes(b) => decode_mini(&mini::SIEGE_GUILD_ENTRY, b).ok().and_then(|f| f.text(0)),
            _ => None,
        })
        .collect()
}

async fn place(w: &mut InWorld, at: (f32, f32)) -> anyhow::Result<()> {
    let mv = &schema::OPT_CHAR_MOVE;
    let pos = Vec3::new(at.0, 0.0, at.1);
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, pos).with(2, pos).with(5, 0x04).with(6, 0)).await
}

fn murderer_points(bytes: &[u8]) -> Option<i64> {
    let f = decode_mini(&mini::CHAR_FACTORS, bytes).ok()?;
    decode_mini(&mini::FACTOR_STATUS, f.bytes(1)?).ok()?.int(11)
}

fn near(p: Vec3, at: (f32, f32), within: f64) -> bool {
    (((p.x - at.0) as f64).powi(2) + ((p.z - at.1) as f64).powi(2)).sqrt() < within
}

/// GmTest se place en `at` (MJ /move) et rend les objets qu'il voit alors.
async fn go(a: &mut InWorld, at: (f32, f32)) -> anyhow::Result<Vec<SeenMonster>> {
    a.world.send(&schema::CHATTING, say(a, &format!("/move {},{}", at.0, at.1))).await?;
    Ok(views(&collect(a, 1500).await))
}

/// GmTest frappe l'objet de TID `tid` le plus proche de `at`, et attend le message de sa destruction.
async fn destroy(a: &mut InWorld, seen: &[SeenMonster], at: (f32, f32), tid: i64, what: &str, expect: impl Fn(&Recv) -> bool) -> anyhow::Result<Recv> {
    let far = |m: &&SeenMonster| (((m.pos.x - at.0) as f64).powi(2) + ((m.pos.z - at.1) as f64).powi(2)) as i64;
    let Some(m) = seen.iter().filter(|m| m.tid == tid).min_by_key(far) else { anyhow::bail!("{what} ({tid}) pas en vue : {:?}", seen.iter().map(|m| m.tid).collect::<Vec<_>>()) };
    let act = &schema::OPT_CHAR_ACTION;
    a.world.send(act, Fields::new(act).with(0, a.cid).with(1, m.cid).with(6, 0)).await?;
    let q = a.world.expect_where(expect, 20_000, what).await?;
    println!("  ✓ {what} ({:#x}) détruit", m.cid);
    Ok(q)
}

pub async fn run(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let (gd, c) = (&schema::GUILD, &schema::CHARACTER);
    let s = &schema::SIEGE_WAR;

    step(&format!("{ca} (MJ) et {cb} entrent ; chaque château reçu (op 0)"));
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    let mut b = enter_world(&cfg, ub, pb, cb, "B-").await?;
    let (acid, bcid) = (a.cid as i64, b.cid as i64);
    let (level_a, level_b) = (a.level, b.level);
    a.world.expect_where(castle_status(0), 3000, "op 0 d'Inflame à l'entrée").await?;
    println!("  ✓ op 0 d'Inflame à l'entrée");

    step(&format!("guildes {GA} ({ca}) et {GB} ({cb}) ; {GB} reçoit Inflame"));
    gm(&mut a, "/level 60").await?;
    gm(&mut a, "/invincible 1").await?;
    gm(&mut a, "/money 100000").await?;
    gm(&mut a, &format!("/level 20 \"{cb}\"")).await?;
    gm(&mut a, &format!("/money 100000 \"{cb}\"")).await?;
    a.world.send(gd, Fields::new(gd).with(0, 0).with(1, a.cid).with(2, GA).with(9, PASSWORD)).await?;
    a.world.expect_where(|q| q.is(gd, Some(10)) && q.int(1) == Some(11), 3000, "création de SimSiegeA").await?;
    b.world.send(gd, Fields::new(gd).with(0, 0).with(1, b.cid).with(2, GB).with(9, PASSWORD)).await?;
    b.world.expect_where(|q| q.is(gd, Some(10)) && q.int(1) == Some(11), 3000, "création de SimSiegeB").await?;
    gm(&mut a, &format!("/siege owner {CASTLE} {GB}")).await?;
    let owned = b.world.expect_where(castle_status(0), 3000, "op 0 : Inflame à SimSiegeB").await?;
    let castle = castle_of(&owned).unwrap();
    anyhow::ensure!(castle.text(2).as_deref() == Some(GB) && castle.text(1).as_deref() == Some(cb), "propriétaire {:?} {:?}", castle.text(2), castle.text(1));
    println!("  ✓ op 0 : guilde {GB}, propriétaire {cb}");

    step("déclaration du siège : refusée à GmTest (3), acceptée de SimA (0x0B)");
    let tz = cfg.world.time_zone;
    let now = crate::character::unix_now();
    let tomorrow = crate::world::siege::to_local(tz, now + 86_400).date().and_hms_opt(20, 0, 0).unwrap();
    let ft = crate::world::siege::filetime(tz, crate::world::siege::from_local(tz, tomorrow));
    let with_date = |w: &InWorld| {
        let m = &mini::SIEGE_CASTLE;
        base(w, siege_op::DECLARE).with(2, build_mini(m, &Fields::new(m).with(0, crate::world::siege::mb(CASTLE)).with(5, ft)))
    };
    a.world.send(s, with_date(&a)).await?;
    a.world.expect_where(result(siege_result::NOT_OWNER), 3000, "code 3 (pas le propriétaire)").await?;
    b.world.send(s, with_date(&b)).await?;
    b.world.expect_where(result(siege_result::DATE_SET), 3000, "code 0x0B").await?;
    let declared = b.world.expect_where(castle_status(1), 3000, "op 0 statut 1").await?;
    anyhow::ensure!(castle_of(&declared).and_then(|c| c.int(5)) == Some(ft as i64), "date {:?} au lieu de {ft}", castle_of(&declared).and_then(|c| c.int(5)));
    println!("  ✓ code 3 à GmTest ; code 0x0B, statut 1, prochain siège le {tomorrow}");

    step("commandant Garrison et candidatures");
    gm(&mut a, &format!("/move {},{}", GARRISON_AT.0, GARRISON_AT.1)).await?;
    place(&mut b, (GARRISON_AT.0 + 200.0, GARRISON_AT.1)).await?;
    let npc = &schema::EVENT_SIEGE_WAR_NPC;
    let ev = crate::world::npcs::event_base(GARRISON, crate::world::npcs::event_function::SIEGE_WAR_NPC);
    a.world.send(npc, Fields::new(npc).with(0, siege_npc_op::REQUEST).with(1, ev).with(2, a.cid)).await?;
    a.world.expect_where(castle_status(1), 3000, "op 0 du château du commandant").await?;
    a.world.expect_where(|q| q.is(npc, Some(siege_npc_op::GRANT)), 3000, "0x50 op 2").await?;
    println!("  ✓ 0x50 op 1 → op 0 d'Inflame, 0x50 op 2");
    a.world.send(s, base(&a, siege_op::APPLY_DEFENSE)).await?;
    a.world.expect_where(result(siege_result::DEFENSE_APPLIED), 3000, "code 8").await?;
    a.world.send(s, base(&a, siege_op::APPLY_DEFENSE)).await?;
    a.world.expect_where(result(siege_result::UNABLE), 3000, "code 6 (déjà inscrite)").await?;
    b.world.send(s, base(&b, siege_op::DEFENSE_PAGE_REQUEST).with(4, page(1))).await?;
    let list = b.world.expect_where(sw(siege_op::DEFENSE_PAGE), 3000, "op 10").await?;
    anyhow::ensure!(page_guilds(&list) == vec![GA.to_string()], "candidats à la défense : {:?}", page_guilds(&list));
    a.world.send(s, base(&a, siege_op::CANCEL_DEFENSE)).await?;
    a.world.expect_where(result(siege_result::OK), 3000, "code 0 (retrait)").await?;
    a.world.send(s, base(&a, siege_op::APPLY_ATTACK)).await?;
    a.world.expect_where(result(siege_result::ATTACK_APPLIED), 3000, "code 9").await?;
    a.world.send(s, base(&a, siege_op::ATTACK_PAGE_REQUEST).with(4, page(1))).await?;
    let list = a.world.expect_where(sw(siege_op::ATTACK_PAGE), 3000, "op 9").await?;
    anyhow::ensure!(page_guilds(&list) == vec![GA.to_string()], "candidats à l'attaque : {:?}", page_guilds(&list));
    b.world.send(s, base(&b, siege_op::APPLY_ATTACK)).await?;
    b.world.expect_where(result(siege_result::UNABLE), 3000, "code 6 (propriétaire)").await?;
    println!("  ✓ défense 8 puis 6, op 10 [{GA}], retrait 0, attaque 9, op 9 [{GA}], {cb} refusé (6)");

    step("/siege start Inflame");
    a.world.send(&schema::CHATTING, say(&a, &format!("/move {},{}", BEFORE_GATE.0, BEFORE_GATE.1))).await?;
    collect(&mut a, 1000).await;
    // À portée de coup de GmTest : les tours de garde ne visent plus les défenseurs, c'est lui qui doit tuer SimA.
    place(&mut b, (BEFORE_GATE.0 + 60.0, BEFORE_GATE.1)).await?;
    a.world.send(&schema::CHATTING, say(&a, &format!("/siege start {CASTLE}"))).await?;
    let got = collect(&mut a, 2500).await;
    let mut seen = views(&got);
    anyhow::ensure!(got.iter().any(castle_status(3)), "op 0 statut 3 absent");
    anyhow::ensure!(got.iter().any(message(siege_msg::START, None)), "op 0x1C code 1 absent");
    anyhow::ensure!(got.iter().any(war_step(1)), "WarInfo étape 1 absente");
    let attackers = got.iter().find(|q| sw(siege_op::ATTACKERS)(q)).map(page_guilds).unwrap_or_default();
    anyhow::ensure!(attackers == vec![GA.to_string()], "op 0xE : {attackers:?}");
    anyhow::ensure!(got.iter().any(|q| sw(siege_op::TIME)(q)), "op 0x1D absent");
    anyhow::ensure!(seen.iter().any(|m| m.tid == 1269), "porte extérieure pas montrée : {:?}", seen.iter().map(|m| m.tid).collect::<Vec<_>>());
    println!("  ✓ statut 3, code 1, WarInfo étape 1, op 0xE [{GA}], op 0x1D ; {} objet(s) en vue", seen.len());

    step(&format!("{ca} tue {cb} sans Ctrl ; résurrection au point intérieur des défenseurs (op 0x13)"));
    let act = &schema::OPT_CHAR_ACTION;
    a.world.send(act, Fields::new(act).with(0, acid).with(1, bcid).with(6, 0)).await?;
    b.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(bcid) && q.int(11) == Some(2), 60_000, "mort de SimA").await?;
    let points = a.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(acid) && q.fields().bytes(7).and_then(murderer_points).is_some_and(|p| p > 0), 2000, "points de meurtrier").await;
    anyhow::ensure!(points.is_err(), "points de meurtrier donnés pendant le siège");
    b.world.send(c, Fields::new(c).with(0, crate::world::siege::war::RESURRECT_INNER).with(1, bcid)).await?;
    let mv = &schema::OPT_CHAR_MOVE;
    let at = b.world.expect_where(|q| q.is(mv, None) && q.int(0) == Some(bcid), 5000, "résurrection de siège").await?.fields().vec3(1).unwrap_or_default();
    anyhow::ensure!(near(at, (-354_400.0, -15_000.0), 20_000.0), "SimA ressuscite en {at:?}");
    println!("  ✓ {cb} tué sans points de meurtrier, ressuscité en {:.0}, {:.0}", at.x, at.z);

    step("objets à 1 PV ; porte intérieure refusée, puis portes, trône et Yeux");
    gm(&mut a, &format!("/siege hp {CASTLE} 1")).await?;
    seen.extend(go(&mut a, INNER_GATE_AT).await?);
    let inner = seen.iter().rev().find(|m| m.tid == 1265).map(|m| m.cid).ok_or_else(|| anyhow::anyhow!("porte intérieure pas en vue"))?;
    a.world.send(act, Fields::new(act).with(0, acid).with(1, inner).with(6, 0)).await?;
    let early = a.world.expect_where(|q| q.is(act, None) && q.int(1) == Some(inner as i64), 2000, "coup sur la porte intérieure").await;
    anyhow::ensure!(early.is_err(), "porte intérieure frappée avant la porte extérieure");
    println!("  ✓ porte intérieure intouchable tant que la porte extérieure tient");
    seen.extend(go(&mut a, BEFORE_GATE).await?);
    destroy(&mut a, &seen, BEFORE_GATE, 1269, "porte extérieure", message(siege_msg::OBJECT_DESTROYED, Some(3))).await?;
    a.world.expect_where(war_step(2), 3000, "WarInfo étape 2").await?;
    seen.extend(go(&mut a, INNER_GATE_AT).await?);
    destroy(&mut a, &seen, INNER_GATE_AT, 1265, "porte intérieure", message(siege_msg::OBJECT_DESTROYED, Some(2))).await?;
    a.world.expect_where(war_step(3), 3000, "WarInfo étape 3").await?;
    seen.extend(go(&mut a, THRONE_AT).await?);
    destroy(&mut a, &seen, THRONE_AT, 1312, "trône", message(siege_msg::THRONE_RELEASED, None)).await?;
    a.world.expect_where(castle_status(4), 3000, "statut 4").await?;
    let throne = seen.iter().rev().find(|m| m.tid == 1312).map(|m| m.cid).unwrap();
    seen.extend(go(&mut a, WEST_EYE_AT).await?);
    destroy(&mut a, &seen, WEST_EYE_AT, 1266, "Œil d'Archon ouest", message(siege_msg::EYE_OPENED, None)).await?;
    seen.extend(go(&mut a, EAST_EYE_AT).await?);
    destroy(&mut a, &seen, EAST_EYE_AT, 1266, "Œil d'Archon est", message(siege_msg::EYE_OPENED, None)).await?;
    a.world.expect_where(castle_status(5), 3000, "statut 5").await?;
    println!("  ✓ codes 4 (types 3 et 2), étapes 2 et 3, code 5 et statut 4, deux codes 6 et statut 5");

    step(&format!("carving du trône par {ca}"));
    gm(&mut a, &format!("/move {},{}", THRONE_AT.0, THRONE_AT.1)).await?;
    a.world.send(s, base(&a, siege_op::CARVE).with(5, throne)).await?;
    a.world.expect_where(result(siege_result::CARVE_START), 3000, "op 2 0x10").await?;
    a.world.expect_where(message(siege_msg::CARVE_START, None), 3000, "code 8").await?;
    a.world.expect_where(result(siege_result::CARVE_DONE), 15_000, "op 2 0x12").await?;
    let carved = a.world.expect_where(|q| castle_status(5)(q) && castle_of(q).and_then(|c| c.text(10)).as_deref() == Some(GA), 3000, "op 0 carve guild").await?;
    anyhow::ensure!(castle_of(&carved).is_some());
    println!("  ✓ 0x10, code 8, 0x12, carve guild {GA}");

    step("/siege end Inflame : changement de propriétaire");
    a.world.send(&schema::CHATTING, say(&a, &format!("/siege end {CASTLE}"))).await?;
    let got = collect(&mut a, 2500).await;
    let ended = got.iter().find(|q| castle_status(6)(q)).and_then(castle_of).ok_or_else(|| anyhow::anyhow!("op 0 statut 6 absent"))?;
    anyhow::ensure!(ended.text(2).as_deref() == Some(GA) && ended.text(1).as_deref() == Some(ca), "propriétaire {:?} {:?}", ended.text(2), ended.text(1));
    anyhow::ensure!(got.iter().any(message(siege_msg::END, None)) && got.iter().any(war_step(9)), "code 11 ou étape 9 absents");
    let home = b.world.expect_where(|q| q.is(mv, None) && q.int(0) == Some(bcid), 5000, "SimA renvoyée en ville").await?.fields().vec3(1).unwrap_or_default();
    anyhow::ensure!(!near(home, (-354_400.0, -20_000.0), 40_000.0), "SimA toujours au château : {home:?}");
    println!("  ✓ statut 6, {GA} propriétaire ({ca}), code 11, étape 9 ; {cb} renvoyée en {:.0}, {:.0}", home.x, home.z);
    a.world.expect_where(|q| q.ptype == schema::WAR_INFO.id && q.int(0) == Some(war_info_op::CLEAR), 40_000, "WarInfo op 6 (30 s après la fin)").await?;
    a.world.expect_where(castle_status(0), 3000, "statut 0").await?;
    println!("  ✓ statut 0, WarInfo op 6");

    step("remise en état");
    gm(&mut a, &format!("/siege owner {CASTLE} -")).await?;
    gm(&mut a, "/invincible 0").await?;
    gm(&mut a, &format!("/level {level_a}")).await?;
    gm(&mut a, &format!("/level {} \"{cb}\"", level_b.max(1))).await?;
    println!("\nguerre de siège : OK");
    Ok(())
}

