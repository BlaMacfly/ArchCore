//! `archcore sim summons` : invocations (AgpmSummons 0x4F, `world/summons.rs`).
//!
//! GmEffets (compte MJ gmtest, déjà plein) reçoit en base Griolli (esprit de l'Élémentaliste, TID 315) et Bomba (619),
//! passe niveau 80, invincible, chez les Vers verts. Griolli : vue de la créature (TID 543), état 0x800 puis op 0, dans
//! cet ordre. Il attaque un ver éloigné : l'esprit (portée 1500) frappe et le tue avant que le maître n'arrive,
//! l'expérience revient au maître. Propension « défense » (op 2) renvoyée. /move : l'esprit disparaît (op 1). Bomba
//! (Big Tim, 10 s) : disparition à l'expiration. Griolli puis Bomba : la limite d'une invocation renvoie Griolli.
//! Familier : Doogie <Rank 1> (TID 7564, compétence 970) sort à l'utilisation (type 8, forcé), rentre à la suivante.

use super::{cast_fields, character_id, enter_world, factors_hp, step, InWorld, Recv};
use archcore_net::schema::{self, chat_type, mini};
use archcore_net::{decode_mini, Fields, Vec3};

const GRIOLLI: i64 = 315;
const BOMBA: i64 = 619;
const GRIOLLI_TID: i64 = 543;
const BIG_TIM_TID: i64 = 380;
const DOOGIE_ITEM: i64 = 7564;
const DOOGIE_TID: i64 = 1362;
const HUNT: Vec3 = Vec3 { x: -466_468.0, y: 4_700.0, z: -50_010.0 };
const STOP: i64 = 0x04;
const SUMMONER: i64 = 0x800;

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

/// Vue d'une créature (0x42 op 1) : (CID, TID, PV).
fn view_of(q: &Recv) -> Option<(i64, i64, i64)> {
    if !q.is(&schema::OPT_VIEW, Some(1)) {
        return None;
    }
    let v = decode_mini(&mini::OPT_VIEW_CHAR, q.fields().bytes(1)?).ok()?;
    Some((v.int(0)?, v.int(1)?, v.bytes(5).and_then(factors_hp).unwrap_or(0)))
}

/// Apparition d'une invocation de modèle `tid` : vue, état spécial, op 0, dans cet ordre. Rend (CID, op 0).
async fn appears(w: &mut InWorld, tid: i64, state: i64, kind: i64, timeout_ms: u64) -> anyhow::Result<(i64, Recv)> {
    let (c, su, me) = (&schema::CHARACTER, &schema::SUMMONS, w.cid as i64);
    let view = w.world.expect_where(|q| view_of(q).is_some_and(|v| v.1 == tid), timeout_ms, "vue de la créature").await?;
    let (cid, _, hp) = view_of(&view).unwrap();
    w.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(cid) && q.int(17) == Some(state), 2000, "état spécial après la vue").await?;
    let add = w.world.expect_where(|q| q.is(su, Some(0)) && q.int(2) == Some(cid), 2000, "AgpmSummons op 0 après l'état").await?;
    anyhow::ensure!(add.int(1) == Some(me) && add.int(3) == Some(tid) && add.int(4) == Some(kind), "op 0 : maître {:?}, TID {:?}, type {:?}", add.int(1), add.int(3), add.int(4));
    let (a, b) = (add.int(6).unwrap_or(0), add.int(7).unwrap_or(0));
    anyhow::ensure!(a > 0 && b > a, "horodatages {a} / {b}");
    println!("  ✓ {cid:#x} (TID {tid}, {hp} PV) : vue, état {state:#x}, op 0 (type {kind}, propension {}, forcée {}, {} s)", add.int(5).unwrap_or(-1), add.int(8).unwrap_or(-1), (b - a) / 1000);
    Ok((cid, add))
}

async fn removed(w: &mut InWorld, cid: i64, timeout_ms: u64, what: &str) -> anyhow::Result<()> {
    let su = &schema::SUMMONS;
    w.world.expect_where(|q| q.is(su, Some(1)) && q.int(2) == Some(cid) && q.int(1) == Some(w.cid as i64), timeout_ms, what).await?;
    Ok(())
}

pub async fn run(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let db = crate::db::Databases::new(&cfg.database);
    let (ch, su, act, mv, c, it) = (&schema::CHATTING, &schema::SUMMONS, &schema::OPT_CHAR_ACTION, &schema::OPT_CHAR_MOVE, &schema::CHARACTER, &schema::ITEM);
    step(&format!("{char_name} reçoit Griolli et Bomba"));
    let w = enter_world(&cfg, user, pass, char_name, "").await?;
    let id = character_id(&db, char_name).await?;
    let missing = [GRIOLLI, BOMBA].into_iter().any(|t| !w.skills.iter().any(|s| s.1 == t));
    let mut w = if missing {
        for tid in [GRIOLLI, BOMBA] {
            db.characters.exec("INSERT INTO character_skills (character_id, skill_tid, level) VALUES (?, ?, 1) ON DUPLICATE KEY UPDATE level = 1", (id, tid)).await?;
        }
        drop(w);
        println!("  compétences ajoutées en base : nouvelle entrée en jeu");
        enter_world(&cfg, user, pass, char_name, "").await?
    } else {
        w
    };
    let me = w.cid as i64;
    let skill = |w: &InWorld, tid: i64| w.skills.iter().find(|s| s.1 == tid).map(|s| s.0).ok_or_else(|| anyhow::anyhow!("compétence {tid} absente"));
    let (griolli, bomba) = (skill(&w, GRIOLLI)?, skill(&w, BOMBA)?);
    let level = w.level.max(1);
    gm(&mut w, "/level 80", "niveau 80").await?;
    gm(&mut w, "/invincible 1", "Invincibilité activée").await?;
    gm(&mut w, "/recast", "Délais de relance").await?;
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, HUNT).with(2, HUNT).with(5, STOP).with(6, 0)).await?;
    let away = |at: Vec3| (((at.x - HUNT.x) as f64).powi(2) + ((at.z - HUNT.z) as f64).powi(2)).sqrt();
    // Vers déjà vus à l'entrée (personnage resté sur place), puis ceux qui apparaissent après le déplacement.
    let mut worms: Vec<(i64, f64)> = w.monsters.iter().filter(|m| m.name == "Green Worm" && m.hp > 0).map(|m| (m.cid as i64, away(m.pos))).collect();
    while let Ok(p) = w.world.expect_where(|q| q.is(&schema::OPT_VIEW, Some(1)), 1500, "monstres").await {
        if let Ok(v) = decode_mini(&mini::OPT_VIEW_CHAR, p.fields().bytes(1).unwrap_or_default()) {
            if v.text(2).as_deref() == Some("Green Worm") && v.bytes(5).and_then(factors_hp).unwrap_or(0) > 0 {
                worms.push((v.int(0).unwrap_or(0), away(v.vec3(8).unwrap_or_default())));
            }
        }
    }
    println!("  {} Ver(s) vert(s) en vue", worms.len());

    step("Griolli : vue, état 0x800, op 0");
    w.world.send(&schema::SKILL, cast_fields(griolli, me)).await?;
    let (spirit, add) = appears(&mut w, GRIOLLI_TID, SUMMONER, 1, 8000).await?;
    anyhow::ensure!(add.int(5) == Some(0) && add.int(8) == Some(0), "propension {:?}, forcée {:?}", add.int(5), add.int(8));

    step("l'esprit frappe la cible du maître ; l'expérience revient au maître");
    // Vers à distance : le maître court vers sa cible pendant que l'esprit (portée 1500) frappe déjà.
    let mut far: Vec<(i64, f64)> = worms.iter().copied().filter(|(_, d)| (500.0..=1300.0).contains(d)).collect();
    far.sort_by(|a, b| b.1.total_cmp(&a.1));
    anyhow::ensure!(!far.is_empty(), "aucun Ver vert entre 500 et 1300 unités de {HUNT:?}");
    let mut proven = false;
    for (worm, d) in far.into_iter().take(4) {
        w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, HUNT).with(2, HUNT).with(5, STOP).with(6, 0)).await?;
        w.world.send(act, Fields::new(act).with(0, w.cid).with(1, worm)).await?;
        let (mut last, mut by_spirit) = (None, 0);
        loop {
            let q = w
                .world
                .expect_where(|q| (q.is(act, None) && q.int(1) == Some(worm)) || (q.is(c, Some(1)) && q.int(1) == Some(worm) && q.int(11) == Some(2)), 10_000, "coups sur le ver")
                .await?;
            if q.ptype == c.id {
                break;
            }
            if q.fields().bytes(3).is_some() {
                last = q.int(0);
                by_spirit += i32::from(q.int(0) == Some(spirit));
            }
        }
        let exp = w.world.expect_where(|q| q.ptype == ch.id && q.int(2) == Some(chat_type::WHOLE_WORLD) && q.fields().text(6).is_some_and(|t| t.contains("expérience")), 3000, "expérience").await?;
        println!("  ver {worm:#x} à {d:.0} : {by_spirit} coup(s) de l'esprit, tué par {}, « {} »", if last == Some(spirit) { "l'esprit" } else { "le maître" }, exp.fields().text(6).unwrap_or_default());
        if last == Some(spirit) {
            proven = true;
            break;
        }
    }
    anyhow::ensure!(proven, "aucun ver tué par l'esprit");
    println!("  ✓ ver tué par l'esprit, expérience au maître");

    step("propension « défense » (op 2)");
    w.world.send(su, Fields::new(su).with(0, 2).with(1, me).with(2, spirit).with(5, 1).with(8, 0)).await?;
    let back = w.world.expect_where(|q| q.is(su, Some(2)) && q.int(2) == Some(spirit), 3000, "op 2 renvoyée").await?;
    anyhow::ensure!(back.int(1) == Some(me) && back.int(5) == Some(1), "op 2 : maître {:?}, propension {:?}", back.int(1), back.int(5));
    // Celle d'une créature qui n'est pas à lui : ignorée.
    w.world.send(su, Fields::new(su).with(0, 2).with(1, me).with(2, 0x5000_0001).with(5, 0).with(8, 0)).await?;
    anyhow::ensure!(w.world.expect_where(|q| q.is(su, Some(2)), 1000, "op 2 refusée").await.is_err(), "propension acceptée pour une autre créature");
    println!("  ✓ propension 1 appliquée et renvoyée ; refusée pour un monstre");

    step("téléportation du maître : l'esprit disparaît");
    // L'op 1 précède la réponse de la commande : attendue directement.
    w.world.send(ch, say(&w, "/move -466000,-51500")).await?;
    removed(&mut w, spirit, 3000, "op 1 après /move").await?;
    w.world.expect_where(|q| q.is(c, Some(2)) && q.int(1) == Some(spirit), 2000, "esprit retiré de la vue").await?;
    println!("  ✓ op 1, puis retrait de la vue");

    step("Bomba (Big Tim, 10 s) : disparition à l'expiration");
    // /level rend les PM dépensés (Bomba en coûte 210).
    gm(&mut w, "/level 80", "niveau 80").await?;
    w.world.send(&schema::SKILL, cast_fields(bomba, me)).await?;
    let (tim, _) = appears(&mut w, BIG_TIM_TID, SUMMONER, 1, 8000).await?;
    let t0 = std::time::Instant::now();
    removed(&mut w, tim, 14_000, "op 1 à l'expiration").await?;
    w.world.expect_where(|q| q.is(c, Some(2)) && q.int(1) == Some(tim), 2000, "créature retirée de la vue").await?;
    println!("  ✓ op 1 au bout de {:.1} s, créature retirée", t0.elapsed().as_secs_f64());

    step("limite d'une invocation : la plus ancienne part");
    gm(&mut w, "/recast", "Délais de relance").await?;
    gm(&mut w, "/level 80", "niveau 80").await?;
    w.world.send(&schema::SKILL, cast_fields(griolli, me)).await?;
    let (first, _) = appears(&mut w, GRIOLLI_TID, SUMMONER, 1, 8000).await?;
    w.world.send(&schema::SKILL, cast_fields(bomba, me)).await?;
    removed(&mut w, first, 8000, "Griolli renvoyé par Bomba").await?;
    let (tim, _) = appears(&mut w, BIG_TIM_TID, SUMMONER, 1, 3000).await?;
    println!("  ✓ Griolli {first:#x} renvoyé (op 1) avant Big Tim {tim:#x} (op 0)");

    step("familier : Doogie <Rank 1>");
    w.world.send(ch, say(&w, &format!("/additem {DOOGIE_ITEM} 1"))).await?;
    let item = w.world.expect_where(|q| q.is(it, Some(0)) && q.int(3) == Some(DOOGIE_ITEM), 3000, "objet reçu").await?;
    let iid = item.int(2).unwrap_or(0);
    w.world.send(it, Fields::new(it).with(0, 3).with(2, iid)).await?;
    let (pet, add) = appears(&mut w, DOOGIE_TID, SUMMONER, 8, 3000).await?;
    anyhow::ensure!(add.int(8) == Some(1), "familier non forcé : {:?}", add.int(8));
    w.world.send(it, Fields::new(it).with(0, 3).with(2, iid)).await?;
    removed(&mut w, pet, 3000, "familier renvoyé par son objet").await?;
    println!("  ✓ sorti (type 8, forcé, à côté de Big Tim) puis renvoyé en réutilisant l'objet");

    step("nettoyage");
    gm(&mut w, "/invincible 0", "Invincibilité désactivée").await?;
    gm(&mut w, &format!("/level {level}"), &format!("niveau {level}")).await?;
    drop(w);
    db.characters.exec("DELETE FROM character_skills WHERE character_id = ? AND skill_tid IN (?, ?)", (id, GRIOLLI, BOMBA)).await?;
    db.characters.exec("DELETE FROM character_items WHERE character_id = ? AND template_id = ?", (id, DOOGIE_ITEM)).await?;
    println!("  ✓ niveau {level} rétabli, compétences et familier retirés de la base");
    println!("\ninvocations : OK");
    Ok(())
}
