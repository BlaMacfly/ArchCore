//! `archcore sim cheat` : anti-triche côté serveur (world/anticheat.rs), GmTest (MJ) + SimA (sans droits, le tricheur).
//!
//! Légitime d'abord, rien ne doit être signalé : SimA court à sa vitesse en envoyant ses positions toutes les 200 ms,
//! puis à cheval (White Horse, Run_Buff 10 500), puis prend le portail d'Anchorville et court à l'arrivée. Triche
//! ensuite : course à 6 fois sa vitesse (position renvoyée par le serveur, anomalie « vitesse »), saut de 30 000 unités
//! (« téléportation ») ; GmTest reprend son attaque après chaque déplacement pour frapper plus vite que sa vitesse
//! d'attaque (coups retardés, « cadence ») ; double récupération d'une pièce jointe et double validation d'un échange
//! (un seul transfert, vérifié en base) ; demande d'échange acceptée par son auteur (refusée) ; `/anomalies`. Enfin SimA
//! inonde le serveur de paquets : connexion fermée, personnage sorti du monde sans attente de reconnexion, « inondation ».

use super::{enter_world, factors_attack_speed, factors_run_speed, gm_place, step, InWorld, Recv, SeenMonster};
use crate::db::col;
use crate::world::anticheat::kind;
use archcore_net::schema::{self, char_mgr_op, chat_type, mail_op, mail_result, mini};
use archcore_net::{build_mini, build_packet, decode_mini, Fields, Vec3};
use std::time::{Duration, Instant};

/// Au nord d'Anchorville, terrain dégagé.
const SPOT: Vec3 = Vec3 { x: -476_000.0, y: 4_422.0, z: -61_000.0 };
/// Point de chasse des Vers verts.
const HUNT: Vec3 = Vec3 { x: -466_468.0, y: 4_700.0, z: -50_010.0 };
/// Portail d'Anchorville (objet, position) ; destination « Repaire de l'éclaireur ».
const PORTAL: (i64, Vec3) = (132_646_586, Vec3 { x: -475_600.0, y: 4_516.0, z: -63_400.0 });
const HORSE: i64 = 3002;
const POTION: i64 = 190;
const PAPER: i64 = 3601;
const STOP: i64 = 0x04;
/// Arrêt synchronisé : la position que le serveur impose.
const STOP_SYNC: i64 = 0x0c;

fn say(w: &InWorld, text: &str) -> Fields {
    let ch = &schema::CHATTING;
    Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text))
}

/// Commande MJ et sa réponse (qui contient `expect`).
async fn gm(w: &mut InWorld, text: &str, expect: &str) -> anyhow::Result<String> {
    tokio::time::sleep(Duration::from_millis(150)).await;
    w.world.send(&schema::CHATTING, say(w, text)).await?;
    let ch = &schema::CHATTING;
    let r = w.world.expect_where(|q| q.ptype == ch.id && q.fields().text(6).is_some_and(|t| t.contains(expect)), 5000, text).await?;
    Ok(r.fields().text(6).unwrap_or_default())
}

fn position(w: &InWorld, at: Vec3) -> Fields {
    let mv = &schema::OPT_CHAR_MOVE;
    Fields::new(mv).with(0, w.cid).with(1, at).with(2, at).with(5, STOP).with(6, 0)
}

/// Position imposée par le serveur à ce joueur (0x40 arrêt synchronisé).
fn snapped(cid: u32) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| q.is(&schema::OPT_CHAR_MOVE, None) && q.int(0) == Some(cid as i64) && q.int(5) == Some(STOP_SYNC)
}

/// Écarte les positions imposées déjà reçues (téléportations du serveur), pour ne compter ensuite que les refus.
async fn drain(w: &mut InWorld) {
    while w.world.expect_where(snapped(w.cid), 300, "téléportation").await.is_ok() {}
}

/// Le MJ `a` pose `b` (nommé `name`) en `at` ; `b` y envoie sa position, comme un client qui suit la téléportation.
async fn place(a: &mut InWorld, b: &mut InWorld, name: &str, at: Vec3) -> anyhow::Result<()> {
    gm_place(a, name, at).await?;
    drain(b).await;
    b.world.send(&schema::OPT_CHAR_MOVE, position(b, at)).await
}

/// Course en ligne droite vers l'est : `steps` positions envoyées toutes les 200 ms, `per_step` unités chacune. Rend la
/// dernière position envoyée et la position imposée par le serveur s'il y en a eu une (la course s'arrête alors).
async fn race(w: &mut InWorld, from: Vec3, per_step: f32, steps: usize) -> anyhow::Result<(Vec3, Option<Vec3>)> {
    let mut at = from;
    for _ in 0..steps {
        at = Vec3::new(at.x + per_step, at.y, at.z);
        w.world.send(&schema::OPT_CHAR_MOVE, position(w, at)).await?;
        if let Ok(q) = w.world.expect_where(snapped(w.cid), 200, "position imposée").await {
            return Ok((at, Some(q.fields().vec3(1).unwrap_or_default())));
        }
    }
    Ok((at, None))
}

/// Vitesse de course (dixièmes d'unité par seconde) des facteurs que le serveur envoie à `w`.
async fn run_speed(w: &mut InWorld, what: &str) -> anyhow::Result<i64> {
    let (c, me) = (&schema::CHARACTER, w.cid as i64);
    let f = w.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(me) && q.fields().bytes(7).and_then(factors_run_speed).is_some(), 3000, what).await?;
    Ok(f.fields().bytes(7).and_then(factors_run_speed).unwrap_or(0))
}

/// Anomalies notées en base pour un personnage : (type, nombre).
async fn anomalies(db: &crate::db::Databases, cid: u32) -> anyhow::Result<Vec<(String, i64)>> {
    // Les lignes sont écrites hors du verrou du monde, dans leur propre tâche.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let rows = db.logs.rows("SELECT kind, COUNT(*) AS n FROM log_anomaly WHERE character_id = ? GROUP BY kind ORDER BY kind", (cid,)).await?;
    Ok(rows.iter().map(|r| (col::<String>(r, "kind"), col::<i64>(r, "n"))).collect())
}

fn count(list: &[(String, i64)], k: &str) -> i64 {
    list.iter().find(|(x, _)| x == k).map(|(_, n)| *n).unwrap_or(0)
}

/// Potions (TID 190) de l'inventaire d'un personnage en base : (piles, total).
async fn potions(db: &crate::db::Databases, cid: u32) -> anyhow::Result<(i64, i64)> {
    let r = db.characters.first("SELECT COUNT(*) AS n, COALESCE(SUM(count), 0) AS total FROM character_items WHERE character_id = ? AND template_id = ? AND status = 0", (cid, POTION)).await?;
    Ok(r.map(|r| (col::<i64>(&r, "n"), col::<i64>(&r, "total"))).unwrap_or((0, 0)))
}

pub async fn run(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let (c, it, mv, act, tp, tr, mb) = (
        &schema::CHARACTER,
        &schema::ITEM,
        &schema::OPT_CHAR_MOVE,
        &schema::OPT_CHAR_ACTION,
        &schema::EVENT_TELEPORT,
        &schema::PRIVATE_TRADE,
        &schema::MAILBOX,
    );
    let db = crate::db::Databases::new(&cfg.database);
    let (ida, idb) = (super::character_id(&db, ca).await?, super::character_id(&db, cb).await?);
    db.logs.exec("DELETE FROM log_anomaly WHERE character_id IN (?, ?)", (ida, idb)).await?;
    db.characters.exec("DELETE FROM mail WHERE recipient_id IN (?, ?) OR sender_id IN (?, ?)", (ida, idb, ida, idb)).await?;

    step(&format!("{ca} (MJ) et {cb} entrent en jeu ; {cb} est posé par le MJ au nord d'Anchorville"));
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    let mut b = enter_world(&cfg, ub, pb, cb, "B-").await?;
    let (acid, bcid) = (a.cid, b.cid);
    let (level_a, level_b) = (a.level, b.level);
    a.world.send(mv, position(&a, SPOT)).await?;
    gm(&mut a, &format!("/level 40 \"{cb}\""), "niveau 40").await?;
    let foot = run_speed(&mut b, "facteurs de SimA au niveau 40").await?;
    place(&mut a, &mut b, cb, SPOT).await?;
    println!("  ✓ course à pied {foot} ({} unités par seconde)", foot / 10);

    step(&format!("légitime : {cb} court à 90 % de sa vitesse pendant 3 s"));
    let per_step = (foot as f32 / 10.0) * 0.2 * 0.9;
    let (end, snap) = race(&mut b, SPOT, per_step, 15).await?;
    anyhow::ensure!(snap.is_none(), "course légitime ramenée en {snap:?}");
    println!("  ✓ {:.0} unités parcourues, aucune position imposée", end.x - SPOT.x);

    step(&format!("légitime : {cb} monte un White Horse et galope"));
    gm(&mut a, &format!("/create \"{cb}\" {HORSE} 1"), "TID 3002").await?;
    let horse = b.world.expect_where(|q| q.is(it, Some(0)) && q.int(3) == Some(HORSE), 3000, "cheval").await?.int(2).unwrap_or(0);
    b.world.send(it, Fields::new(it).with(0, 2).with(1, 3).with(2, horse)).await?;
    let mounted = run_speed(&mut b, "facteurs à cheval").await?;
    anyhow::ensure!(mounted > foot, "course à cheval {mounted} (à pied {foot})");
    b.world.expect_where(|q| q.is(&schema::RIDE, Some(2)), 3000, "AgpmRide op 2").await?;
    let per_step = (mounted as f32 / 10.0) * 0.2 * 0.95;
    let (end2, snap) = race(&mut b, end, per_step, 15).await?;
    anyhow::ensure!(snap.is_none(), "galop légitime ramené en {snap:?}");
    println!("  ✓ course à cheval {mounted} : {:.0} unités en 3 s sans position imposée", end2.x - end.x);
    let any = &mini::ITEM_GRID;
    let grid = build_mini(any, &Fields::new(any).with(0, -1).with(1, -1).with(2, -1));
    b.world.send(it, Fields::new(it).with(0, 2).with(1, 2).with(2, horse).with(7, grid)).await?;
    let back = run_speed(&mut b, "facteurs à pied").await?;
    // Juste après la descente, le saut suivant compte encore à la vitesse du cheval.
    let (_, snap) = race(&mut b, end2, (mounted as f32 / 10.0) * 0.2 * 0.9, 1).await?;
    anyhow::ensure!(snap.is_none(), "dernier pas après la descente ramené en {snap:?}");
    println!("  ✓ descendu (course {back}) ; le pas fait à cheval n'est pas compté à pied");

    step(&format!("légitime : {cb} prend le portail d'Anchorville et court à l'arrivée"));
    gm(&mut a, &format!("/money 5000 \"{cb}\""), "5000").await?;
    place(&mut a, &mut b, cb, PORTAL.1).await?;
    let eb = &mini::EVENT_BASE;
    let base = |object: i64| build_mini(eb, &Fields::new(eb).with(0, 1).with(1, object).with(3, 8));
    b.world.send(tp, Fields::new(tp).with(0, 4).with(1, base(PORTAL.0)).with(2, bcid)).await?;
    b.world.expect_where(|q| q.is(tp, Some(5)), 15_000, "accord du portail").await?;
    let scout = encoding_rs::EUC_KR.encode("정찰자의 터").0.into_owned();
    b.world.send(tp, Fields::new(tp).with(0, 0).with(1, base(PORTAL.0)).with(2, bcid).with(3, scout)).await?;
    let start = b.world.expect_where(|q| q.is(tp, Some(1)), 3000, "départ").await?;
    let landed = start.fields().vec3(4).ok_or_else(|| anyhow::anyhow!("arrivée absente"))?;
    b.world.expect_where(|q| q.is(tp, Some(8)), 3000, "chargement").await?;
    drain(&mut b).await;
    let per_step = (back as f32 / 10.0) * 0.2 * 0.9;
    let (_, snap) = race(&mut b, landed, per_step, 10).await?;
    anyhow::ensure!(snap.is_none(), "course après le portail ramenée en {snap:?}");
    let seen = anomalies(&db, idb).await?;
    anyhow::ensure!(seen.is_empty(), "anomalies sur des déplacements légitimes : {seen:?}");
    println!("  ✓ arrivé en {:.0}, {:.0}, course sans position imposée ; aucune anomalie en base", landed.x, landed.z);

    step(&format!("triche : {cb} court à 6 fois sa vitesse"));
    place(&mut a, &mut b, cb, SPOT).await?;
    let per_step = (back as f32 / 10.0) * 0.2 * 6.0;
    let (sent, snap) = race(&mut b, SPOT, per_step, 40).await?;
    let snap = snap.ok_or_else(|| anyhow::anyhow!("speedhack accepté jusqu'en {sent:?}"))?;
    anyhow::ensure!(snap.x < sent.x - per_step / 2.0, "position imposée {snap:?}, demandée {sent:?}");
    let seen = anomalies(&db, idb).await?;
    anyhow::ensure!(count(&seen, kind::SPEED) >= 1, "anomalie de vitesse absente : {seen:?}");
    println!("  ✓ ramené en {:.0} (demandé {:.0}) ; anomalie « vitesse » en base", snap.x, sent.x);

    step(&format!("triche : {cb} se téléporte de 30 000 unités"));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let far = Vec3::new(snap.x + 30_000.0, snap.y, snap.z);
    b.world.send(mv, position(&b, far)).await?;
    let q = b.world.expect_where(snapped(bcid), 3000, "position imposée").await?;
    let kept = q.fields().vec3(1).unwrap_or_default();
    anyhow::ensure!((kept.x - snap.x).abs() < 1000.0, "position gardée {kept:?} au lieu de {snap:?}");
    let seen = anomalies(&db, idb).await?;
    anyhow::ensure!(count(&seen, kind::TELEPORT) >= 1, "anomalie de téléportation absente : {seen:?}");
    println!("  ✓ gardé en {:.0}, {:.0} ; anomalie « téléportation » en base", kept.x, kept.z);

    step(&format!("triche : {ca} reprend son attaque après chaque déplacement, toutes les 120 ms"));
    gm(&mut a, "/level 1", "niveau 1").await?;
    gm(&mut a, "/invincible 1", "Invincibilit").await?;
    a.world.send(&schema::CHATTING, say(&a, "/speed 1")).await?;
    let f = a.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(acid as i64) && q.fields().bytes(7).and_then(factors_attack_speed).is_some(), 3000, "facteurs").await?;
    let atk = f.fields().bytes(7).and_then(factors_attack_speed).unwrap_or(60).max(1);
    a.world.send(mv, position(&a, HUNT)).await?;
    let mut seen_m: Vec<SeenMonster> = Vec::new();
    let ov = &schema::OPT_VIEW;
    while let Ok(p) = a.world.expect_where(|q| q.is(ov, Some(1)), 1500, "monstres").await {
        if let Ok(view) = decode_mini(&mini::OPT_VIEW_CHAR, p.fields().bytes(1).unwrap_or_default()) {
            if view.int(0).unwrap_or(0) >= crate::world::monsters::MONSTER_CID_BASE as i64 {
                seen_m.push(SeenMonster::from_view(&view));
            }
        }
    }
    let dist = |p: Vec3| (((p.x - HUNT.x) as f64).powi(2) + ((p.z - HUNT.z) as f64).powi(2)).sqrt();
    let worm = seen_m.iter().filter(|m| m.hp > 0 && m.name == "Green Worm").min_by_key(|m| dist(m.pos) as i64).cloned().ok_or_else(|| anyhow::anyhow!("aucun Ver vert en vue"))?;
    let wcid = worm.cid as i64;
    let hit = move |q: &Recv| q.is(act, None) && q.int(0) == Some(acid as i64) && q.int(1) == Some(wcid);
    let attack = Fields::new(act).with(0, acid).with(1, worm.cid).with(6, 1);
    a.world.send(act, attack.clone()).await?;
    a.world.expect_where(hit, 20_000, "premier coup").await?;
    let started = Instant::now();
    let (mut hits, mut cycles) = (0u32, 0u32);
    let resume = Fields::new(mv).with(0, acid).with(5, STOP).with(6, 0);
    while started.elapsed() < Duration::from_secs(4) {
        a.world.send(mv, resume.clone()).await?;
        a.world.send(act, attack.clone()).await?;
        cycles += 1;
        let until = Instant::now() + Duration::from_millis(120);
        loop {
            let left = until.saturating_duration_since(Instant::now()).as_millis() as u64;
            if left == 0 || a.world.expect_where(hit, left, "coup").await.is_err() {
                break;
            }
            hits += 1;
        }
    }
    let interval = 60_000.0 / atk as f64;
    let allowed = (started.elapsed().as_millis() as f64 / (interval / 1.5)).ceil() as u32 + 1;
    anyhow::ensure!(hits <= allowed, "{hits} coups en 4 s pour {cycles} reprises (vitesse d'attaque {atk} : {allowed} au plus)");
    let seen = anomalies(&db, ida).await?;
    anyhow::ensure!(count(&seen, kind::CADENCE) >= 1, "anomalie de cadence absente : {seen:?}");
    println!("  ✓ {hits} coups pour {cycles} reprises en 4 s (vitesse d'attaque {atk} : {allowed} au plus) ; anomalie « cadence » en base");
    gm(&mut a, &format!("/level {}", level_a.max(1)), "niveau").await?;

    step(&format!("duplication : {cb} récupère deux fois de suite la pièce jointe d'un courrier"));
    a.world.send(mv, position(&a, SPOT)).await?;
    place(&mut a, &mut b, cb, SPOT).await?;
    let paper = super::mail::give(&mut a, PAPER, 1).await?;
    let pots = super::mail::give(&mut a, POTION, 5).await?;
    let (_, before) = potions(&db, idb).await?;
    a.world.send(mb, super::mail::mail(mail_op::WRITE, acid).with(2, super::mail::letter(ca, cb, "Potions", "Cinq.")).with(3, pots)).await?;
    super::mail::expect_code(&mut a, mail_op::WRITE, mail_result::OK, "lettre envoyée").await?;
    let got = b.world.expect_where(|q| q.is(mb, Some(mail_op::ADD)), 3000, "courrier reçu").await?;
    let id = decode_mini(&mini::MAIL, got.fields().bytes(2).unwrap_or_default())?.int(0).unwrap_or(0);
    let save = super::mail::mail(mail_op::ITEM_SAVE, bcid).with(2, super::mail::id_only(id));
    b.world.send(mb, save.clone()).await?;
    b.world.send(mb, save).await?;
    super::mail::expect_code(&mut b, mail_op::ITEM_SAVE, mail_result::OK, "première récupération").await?;
    super::mail::expect_code(&mut b, mail_op::ITEM_SAVE, mail_result::FAILED, "seconde récupération").await?;
    let (_, after) = potions(&db, idb).await?;
    anyhow::ensure!(after - before == 5, "{cb} a {before} puis {after} potions en base");
    println!("  ✓ une seule récupération (papier {paper}) : {before} → {after} potions en base");

    step(&format!("arnaque : {ca} accepte lui-même sa demande d'échange"));
    let t = |op: i64| Fields::new(tr).with(0, op);
    let is_t = |op: i64| move |q: &Recv| q.is(tr, Some(op));
    a.world.send(tr, t(1).with(1, acid).with(2, bcid)).await?;
    b.world.expect_where(is_t(3), 3000, "demande reçue").await?;
    a.world.send(tr, t(4).with(1, acid)).await?;
    a.world.expect_where(is_t(5), 3000, "demande annulée").await?;
    anyhow::ensure!(b.world.expect_where(is_t(6), 800, "début").await.is_err(), "échange ouvert sans l'accord de {cb}");
    println!("  ✓ annulée (op 5), aucun début chez {cb}");

    step("duplication : double validation d'un échange");
    let offered = super::mail::give(&mut a, POTION, 2).await?;
    let (_, before) = potions(&db, idb).await?;
    let (_, before_a) = potions(&db, ida).await?;
    a.world.send(tr, t(1).with(1, acid).with(2, bcid)).await?;
    b.world.expect_where(is_t(3), 3000, "demande reçue").await?;
    b.world.send(tr, t(4).with(1, bcid)).await?;
    a.world.expect_where(is_t(6), 3000, "début").await?;
    a.world.send(tr, t(9).with(1, acid).with(3, offered)).await?;
    b.world.expect_where(|q| q.is(it, Some(0)) && q.int(2) == Some(offered), 3000, "pile proposée").await?;
    a.world.send(tr, t(16).with(1, acid)).await?;
    b.world.expect_where(is_t(17), 3000, "A verrouillé").await?;
    b.world.send(tr, t(16).with(1, bcid)).await?;
    a.world.expect_where(is_t(20), 3000, "verrous").await?;
    a.world.send(tr, t(21).with(1, acid)).await?;
    b.world.expect_where(is_t(22), 3000, "A prêt").await?;
    for _ in 0..3 {
        b.world.send(tr, t(21).with(1, bcid)).await?;
    }
    a.world.send(tr, t(21).with(1, acid)).await?;
    b.world.expect_where(|q| q.is(tr, Some(0)) && q.int(8) == Some(8), 3000, "échange conclu").await?;
    anyhow::ensure!(b.world.expect_where(|q| q.is(tr, Some(0)), 800, "second résultat").await.is_err(), "échange conclu deux fois");
    let (_, after) = potions(&db, idb).await?;
    let (_, after_a) = potions(&db, ida).await?;
    let owner = db.characters.first("SELECT character_id FROM character_items WHERE id = ?", (offered,)).await?.map(|r| col::<u32>(&r, "character_id"));
    anyhow::ensure!(after - before == 2 && before_a - after_a == 2 && owner == Some(idb), "potions de {cb} {before} → {after}, de {ca} {before_a} → {after_a}, pile chez {owner:?}");
    println!("  ✓ un seul transfert : 2 potions de {ca} à {cb} (pile {offered} chez {cb} en base)");

    step(&format!("{ca} consulte les anomalies de {cb}"));
    let reply = gm(&mut a, &format!("/anomalies \"{cb}\""), "en base").await?;
    anyhow::ensure!(reply.starts_with(cb), "réponse {reply:?}");
    let mut lines = Vec::new();
    while let Ok(q) = a.world.expect_where(|q| q.ptype == schema::CHATTING.id, 500, "ligne").await {
        lines.push(q.fields().text(6).unwrap_or_default());
    }
    anyhow::ensure!(lines.iter().any(|l| l.contains("vitesse")), "lignes : {lines:?}");
    println!("  ✓ « {reply} » et {} ligne(s)", lines.len());

    step(&format!("inondation : {cb} envoie 3000 paquets d'un coup"));
    gm(&mut a, &format!("/level {} \"{cb}\"", level_b.max(1)), "niveau").await?;
    gm(&mut a, "/invincible 0", "Invincibilit").await?;
    let cm = &schema::CHAR_MANAGER;
    let keepalive = build_packet(cm, &Fields::new(cm).with(0, char_mgr_op::KEEPALIVE).with(2, 0));
    let mut sent = 0;
    for _ in 0..3000 {
        if b.world.send_plain(keepalive.clone()).await.is_err() {
            break;
        }
        sent += 1;
    }
    anyhow::ensure!(b.world.closed_within(5000).await, "connexion de {cb} toujours ouverte après {sent} paquets");
    a.world.expect_where(|q| q.is(c, Some(3)) && q.int(1) == Some(bcid as i64), 5000, &format!("{cb} sort du monde")).await?;
    let seen = anomalies(&db, idb).await?;
    anyhow::ensure!(count(&seen, kind::FLOOD) == 1, "anomalie d'inondation : {seen:?}");
    println!("  ✓ connexion fermée après {sent} paquets envoyés, {cb} sorti du monde aussitôt ; anomalie « inondation » en base");
    println!("  anomalies de {cb} : {seen:?}");

    db.characters.exec("DELETE FROM mail WHERE recipient_id IN (?, ?) OR sender_id IN (?, ?)", (ida, idb, ida, idb)).await?;
    db.close().await;
    println!("\nanti-triche : OK");
    Ok(())
}
