//! `archcore sim evolution` : évolution du Dragon Scion (AGPMCHARACTER ops 26 et 27, AGPMSKILL op 7 action 4).
//!
//! SimScion (Scion, TID 1724, compte sans droits MJ ; à créer une fois par `ARCHCORE_SIM_CREATE_TID=1724 archcore sim
//! client test test SimScion`) se tient chez Gilliwen, maître des compétences du Dragon Scion ; GmTest (MJ) le rejoint, le
//! passe niveau 20 et lui donne de l'or. Apprentissage de Slayer et Orbiter of Evolution (1189, 1192), refus de Power
//! Stunner (réservée au Slayer : code 5). Slayer of Evolution lancée : facteurs du Slayer (classe 1, FOR 83 au niveau
//! 20), AGPMSKILL op 4, op 26 (1722) chez lui et chez GmTest. Orbiter of Evolution : op 27 (1724) puis op 26 (1723) ;
//! relancée, refusée. Reconnexion : TID 1723 et bit 2 du champ 18 à l'entrée, op 4 de la compétence, GmTest le voit en
//! Orbiter. Annulation (AGPMSKILL op 7, action 4) : op 5, op 27 (1724), classe du Scion, forme effacée en base.

use super::{enter_world, start_at, step, InWorld, Recv};
use archcore_net::schema::{self, chat_type, mini};
use archcore_net::{build_mini, decode_mini, Fields, Vec3};
use std::time::Duration;

const SCION: i64 = 1724;
const SLAYER: i64 = 1722;
const ORBITER: i64 = 1723;
const SLAYER_OF_EVOLUTION: i64 = 1189;
const ORBITER_OF_EVOLUTION: i64 = 1192;
const POWER_STUNNER: i64 = 1147;
/// Gilliwen (npc_spawn 410) et une place à côté.
const GILLIWEN: i64 = 0x4000_0000 + 410;
const SPOT: Vec3 = Vec3 { x: -596_250.0, y: 16_300.0, z: 493_700.0 };
const STOP: i64 = 0x04;

fn say(w: &InWorld, text: &str) -> Fields {
    let ch = &schema::CHATTING;
    Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text))
}

async fn gm(w: &mut InWorld, text: &str, expect: &str) -> anyhow::Result<()> {
    w.world.send(&schema::CHATTING, say(w, text)).await?;
    let ch = &schema::CHATTING;
    let r = w.world.expect_where(|q| q.ptype == ch.id && q.fields().text(6).is_some_and(|t| t.contains(expect)), 3000, text).await?;
    println!("  {text} → {}", r.fields().text(6).unwrap_or_default());
    Ok(())
}

/// Classe (facteur « type », valeur 2) et force (facteur « statut », valeur 1, × 100) d'un mini-paquet de facteurs.
fn class_and_str(bytes: &[u8]) -> Option<(i64, i64)> {
    let f = decode_mini(&mini::CHAR_FACTORS, bytes).ok()?;
    let ty = decode_mini(&mini::FACTOR_TYPE, f.bytes(2)?).ok()?;
    let status = decode_mini(&mini::FACTOR_STATUS, f.bytes(1)?).ok()?;
    Some((ty.int(2)?, status.int(1)?))
}

/// Attend les facteurs de `cid` à la classe `class` : rend la force.
async fn factors_of_class(w: &mut InWorld, class: i64, what: &str) -> anyhow::Result<i64> {
    let (c, me) = (&schema::CHARACTER, w.cid as i64);
    let pick = |q: &Recv| q.fields().bytes(7).and_then(class_and_str);
    let f = w.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(me) && pick(q).is_some_and(|(k, _)| k == class), 5000, what).await?;
    Ok(pick(&f).map(|(_, s)| s).unwrap_or(0))
}

/// AGPMSKILL op 7 sur soi : action 0 (lancer) ou 4 (annuler).
fn cast(w: &InWorld, id: i64, action: i64) -> Fields {
    let (k, sb, sa) = (&schema::SKILL, &mini::SKILL_BASE, &mini::SKILL_ACTION);
    let me = build_mini(sb, &Fields::new(sb).with(0, 3).with(1, w.cid));
    let act = build_mini(sa, &Fields::new(sa).with(0, action).with(1, me.clone()));
    Fields::new(k).with(0, 7).with(1, id).with(2, me).with(6, act)
}

async fn op_char(w: &mut InWorld, op: i64, cid: i64, tid: i64, what: &str) -> anyhow::Result<()> {
    let c = &schema::CHARACTER;
    let p = w.world.expect_where(|q| q.is(c, Some(op)) && q.int(1) == Some(cid), 5000, what).await?;
    anyhow::ensure!(p.int(2) == Some(tid), "{what} : TID {:?} au lieu de {tid}", p.int(2));
    Ok(())
}

pub async fn run(user: &str, pass: &str, char_name: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let (k, sm) = (&schema::SKILL, &schema::EVENT_SKILL_MASTER);
    // Départ reproductible : chez Gilliwen, sans forme ni compétence d'évolution.
    start_at(&cfg, cb, SPOT).await?;
    let db = crate::db::Databases::new(&cfg.database);
    db.characters.exec("UPDATE characters SET evolution_tid = 0 WHERE name = ?", (cb.to_string(),)).await?;
    db.characters
        .exec(
            "DELETE FROM character_skills WHERE skill_tid IN (?, ?) AND character_id = (SELECT id FROM characters WHERE name = ?)",
            (SLAYER_OF_EVOLUTION, ORBITER_OF_EVOLUTION, cb.to_string()),
        )
        .await?;

    step(&format!("{cb} (Scion) et {char_name} (MJ) entrent en jeu"));
    let mut s = enter_world(&cfg, ub, pb, cb, "S-").await?;
    anyhow::ensure!(s.tid == SCION, "{cb} n'est pas un Scion (TID {}) : le créer avec ARCHCORE_SIM_CREATE_TID=1724", s.tid);
    let me = s.cid as i64;
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let mv = &schema::OPT_CHAR_MOVE;
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, SPOT).with(2, SPOT).with(5, STOP).with(6, 0)).await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    gm(&mut w, &format!("/level 20 \"{cb}\""), "20").await?;
    tokio::time::sleep(Duration::from_millis(150)).await;
    gm(&mut w, &format!("/money 100000 \"{cb}\""), "100").await?;

    step("apprendre les évolutions chez Gilliwen");
    let eb = &mini::EVENT_BASE;
    let event = build_mini(eb, &Fields::new(eb).with(0, 3).with(1, GILLIWEN).with(3, 25));
    let master = |op: i64| Fields::new(sm).with(0, op).with(1, event.clone()).with(2, me);
    s.world.send(sm, master(4)).await?;
    s.world.expect_where(|q| q.is(sm, Some(5)) && q.int(5) == Some(0), 3000, "fenêtre du maître").await?;
    let mut ids = Vec::new();
    for tid in [SLAYER_OF_EVOLUTION, ORBITER_OF_EVOLUTION] {
        s.world.send(sm, master(1).with(4, tid)).await?;
        let add = s.world.expect_where(|q| q.is(k, Some(0)) && q.int(4) == Some(tid), 3000, "compétence ajoutée").await?;
        s.world.expect_where(|q| q.is(sm, Some(6)) && q.int(4) == Some(tid) && q.int(5) == Some(0), 3000, "apprise").await?;
        ids.push(add.int(1).unwrap_or(0));
    }
    let (slayer_id, orbiter_id) = (ids[0], ids[1]);
    s.world.send(sm, master(1).with(4, POWER_STUNNER)).await?;
    s.world.expect_where(|q| q.is(sm, Some(6)) && q.int(4) == Some(POWER_STUNNER) && q.int(5) == Some(5), 3000, "refus de classe").await?;
    println!("  ✓ Slayer et Orbiter of Evolution apprises ({slayer_id}, {orbiter_id}) ; Power Stunner refusée au Scion (code 5)");

    step("Slayer of Evolution");
    s.world.send(k, cast(&s, slayer_id, 0)).await?;
    let strength = factors_of_class(&mut s, 1, "facteurs du Slayer").await?;
    anyhow::ensure!(strength == 8300, "force du Slayer niveau 20 : {strength} au lieu de 8300");
    s.world.expect_where(|q| q.is(k, Some(4)) && q.int(1) == Some(slayer_id), 3000, "op 4 de l'évolution").await?;
    op_char(&mut s, 26, me, SLAYER, "op 26").await?;
    op_char(&mut w, 26, me, SLAYER, "op 26 chez le MJ").await?;
    println!("  ✓ facteurs du Slayer (classe 1, FOR {}), op 4, op 26 → {SLAYER} chez les deux", strength / 100);

    step("passage à Orbiter of Evolution");
    tokio::time::sleep(Duration::from_millis(10_300)).await;
    s.world.send(k, cast(&s, orbiter_id, 0)).await?;
    op_char(&mut s, 27, me, SCION, "op 27").await?;
    factors_of_class(&mut s, 2, "facteurs de l'Orbiter").await?;
    op_char(&mut s, 26, me, ORBITER, "op 26").await?;
    op_char(&mut w, 27, me, SCION, "op 27 chez le MJ").await?;
    op_char(&mut w, 26, me, ORBITER, "op 26 chez le MJ").await?;
    println!("  ✓ op 27 → {SCION} puis op 26 → {ORBITER}, facteurs de l'Orbiter (classe 2)");
    tokio::time::sleep(Duration::from_millis(10_300)).await;
    s.world.send(k, cast(&s, orbiter_id, 0)).await?;
    let ch = &schema::CHATTING;
    let no = s.world.expect_where(|q| q.ptype == ch.id && q.fields().text(6).is_some_and(|t| t.contains("déjà")), 3000, "refus de la forme active").await?;
    println!("  ✓ relancée : « {} »", no.fields().text(6).unwrap_or_default());

    step("reconnexion en Orbiter");
    drop(s);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut s = enter_world(&cfg, ub, pb, cb, "S-").await?;
    let orbiter_id = s.skills.iter().find(|x| x.1 == ORBITER_OF_EVOLUTION).map(|x| x.0).unwrap_or(0);
    anyhow::ensure!(s.tid == ORBITER && s.transform & 0x04 != 0, "entrée : TID {}, champ 18 = {}", s.tid, s.transform);
    anyhow::ensure!(s.effects.contains(&orbiter_id), "op 4 d'Orbiter of Evolution ({orbiter_id}) absent : {:?}", s.effects);
    let c = &schema::CHARACTER;
    let seen = w.world.expect_where(|q| q.is(c, Some(0)) && q.int(1) == Some(me), 5000, "op 0 chez le MJ").await?;
    anyhow::ensure!(seen.int(2) == Some(ORBITER) && seen.int(18).unwrap_or(0) & 0x04 != 0, "vue : TID {:?}, champ 18 {:?}", seen.int(2), seen.int(18));
    println!("  ✓ TID {ORBITER}, champ 18 = {}, op 4 ({orbiter_id}) à l'entrée ; le MJ le voit en Orbiter", s.transform);

    step("annulation (AGPMSKILL op 7, action 4)");
    s.world.send(k, cast(&s, orbiter_id, 4)).await?;
    s.world.expect_where(|q| q.is(k, Some(5)) && q.int(1) == Some(orbiter_id), 3000, "op 5").await?;
    factors_of_class(&mut s, 3, "facteurs du Scion").await?;
    op_char(&mut s, 27, me, SCION, "op 27").await?;
    op_char(&mut w, 27, me, SCION, "op 27 chez le MJ").await?;
    let row = db.characters.first("SELECT evolution_tid FROM characters WHERE name = ?", (cb.to_string(),)).await?;
    let saved: u32 = row.map(|r| crate::db::col(&r, "evolution_tid")).unwrap_or(99);
    anyhow::ensure!(saved == 0, "evolution_tid = {saved} après l'annulation");
    println!("  ✓ op 5, op 27 → {SCION} chez les deux, classe du Scion, forme effacée en base");
    db.close().await;
    println!("\névolution : OK");
    Ok(())
}
