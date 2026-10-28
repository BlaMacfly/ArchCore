//! `archcore sim boss` : fenêtre des boss d'Archonia (AgpmBoss 0x64, `world/boss.rs`) et heure du serveur (AgpmTimer).
//!
//! GmTest (MJ) reçoit à l'entrée AGPMTIMER champs 2 (heure Unix) et 3 (fuseau), puis l'op 1 (boss du monde, Basilis
//! compris). Il choisit Basilis (op 4) : op 3 (butin possible, TID du boss) et op 5 (classement, une page) ; l'op 6
//! redemande la page 0 (op 5). Puis, niveau 100 et invincible, il frappe Basilis (premier étage de la Battle Square) :
//! compteur de dégâts (op 7) avec sa ligne. Le niveau est remis à la fin.

use super::{enter_world, step, InWorld, Recv, SeenMonster};
use archcore_net::schema::{self, boss_op, chat_type, mini};
use archcore_net::{build_mini, decode_mini, Fields, Value};
use std::time::Duration;

const BASILIS: i64 = 2073;
const BASILIS_AT: (f32, f32) = (-252_616.0, -252_848.0);

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

fn boss(op: i64) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| q.is(&schema::BOSS, Some(op))
}

/// Mini BOSS_SPAWN (structure brute) : (TID, prochaine apparition, vivant).
fn spawn_of(v: &Value) -> Option<(i64, i64, bool)> {
    let Value::Bytes(b) = v else { return None };
    let raw = decode_mini(&mini::BOSS_SPAWN, b).ok()?.bytes(0)?.to_vec();
    let tid = u32::from_le_bytes(raw[0..4].try_into().ok()?) as i64;
    let next = u64::from_le_bytes(raw[0x18..0x20].try_into().ok()?) as i64;
    Some((tid, next, raw[0x28] != 0))
}

pub async fn run(user: &str, pass: &str, name: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let s = &schema::BOSS;

    step(&format!("{name} entre : heure du serveur et liste des boss"));
    let mut w = enter_world(&cfg, user, pass, name, "").await?;
    let level = w.level;
    let (unix, offset) = w.server_time;
    let now = crate::character::unix_now();
    anyhow::ensure!((unix - now).abs() < 120, "AGPMTIMER champ 2 = {unix}, horloge {now}");
    println!("  ✓ AGPMTIMER : heure {unix} (écart {} s), décalage du fuseau {offset} s", unix - now);
    let list = w.world.expect_where(boss(boss_op::LIST), 3000, "op 1 à l'entrée").await?;
    let spawns: Vec<(i64, i64, bool)> = list.fields().list(2).unwrap_or_default().iter().filter_map(spawn_of).collect();
    let basilis = spawns.iter().find(|s| s.0 == BASILIS).ok_or_else(|| anyhow::anyhow!("Basilis absent de la liste ({} boss)", spawns.len()))?;
    println!("  ✓ op 1 : {} boss, Basilis {}", spawns.len(), if basilis.2 { "vivant".to_string() } else { format!("réapparaît à {}", basilis.1) });

    step("Basilis choisi dans la liste : butin et classement");
    w.world.send(s, Fields::new(s).with(0, boss_op::SELECT).with(4, BASILIS)).await?;
    let drops = w.world.expect_where(boss(boss_op::DROPS), 3000, "op 3").await?;
    let raw = decode_mini(&mini::BOSS_DROP, drops.fields().bytes(3).unwrap_or_default())?.bytes(0).unwrap_or_default().to_vec();
    anyhow::ensure!(raw.len() == 196 && u32::from_le_bytes(raw[0..4].try_into()?) == BASILIS as u32, "butin : {} o", raw.len());
    let items = (0..16).filter(|i| raw[4 + i * 4..8 + i * 4] != [0, 0, 0, 0]).count();
    anyhow::ensure!(items > 0, "aucun objet possible pour Basilis");
    let rank = w.world.expect_where(boss(boss_op::RANKING), 3000, "op 5").await?;
    let r = decode_mini(&mini::BOSS_RANK, rank.fields().bytes(5).unwrap_or_default())?;
    anyhow::ensure!(rank.int(4) == Some(BASILIS) && r.int(1).unwrap_or(0) >= 1, "classement : TID {:?}, pages {:?}", rank.int(4), r.int(1));
    println!("  ✓ op 3 : {items} objet(s) ; op 5 : page {}/{}, rang {}", r.int(0).unwrap_or(0), r.int(1).unwrap_or(0), r.int(2).unwrap_or(0));
    let page = build_mini(&mini::BOSS_RANK, &Fields::new(&mini::BOSS_RANK).with(0, 0));
    w.world.send(s, Fields::new(s).with(0, boss_op::RANKING_PAGE).with(4, BASILIS).with(5, page)).await?;
    w.world.expect_where(|q| boss(boss_op::RANKING)(q) && q.int(4) == Some(BASILIS), 3000, "op 5 après l'op 6").await?;
    println!("  ✓ op 6 → op 5");

    step(&format!("{name} frappe Basilis : compteur de dégâts"));
    gm(&mut w, "/level 100").await?;
    gm(&mut w, "/invincible 1").await?;
    // Les vues arrivent avant la réponse de la commande : pas de `gm`, qui les écarterait.
    let to = format!("/move {},{}", BASILIS_AT.0 + 300.0, BASILIS_AT.1);
    w.world.send(&schema::CHATTING, say(&w, &to)).await?;
    let ov = &schema::OPT_VIEW;
    let mut target = None;
    while let Ok(p) = w.world.expect_where(|q| q.is(ov, Some(1)), 3000, "vue de Basilis").await {
        if let Ok(view) = decode_mini(&mini::OPT_VIEW_CHAR, p.fields().bytes(1).unwrap_or_default()) {
            if view.int(1) == Some(BASILIS) {
                target = Some(SeenMonster::from_view(&view));
                break;
            }
        }
    }
    let target = target.ok_or_else(|| anyhow::anyhow!("Basilis n'est pas en vue"))?;
    let act = &schema::OPT_CHAR_ACTION;
    w.world.send(act, Fields::new(act).with(0, w.cid).with(1, target.cid).with(6, 1)).await?;
    let meter = w.world.expect_where(boss(boss_op::DAMAGE), 20_000, "op 7").await?;
    let f = decode_mini(&mini::BOSS_DAMAGE, meter.fields().bytes(6).unwrap_or_default())?;
    let rows = f.list(2).unwrap_or_default();
    let first = rows.first().and_then(|v| if let Value::Bytes(b) = v { decode_mini(&mini::BOSS_DAMAGE_ROW, b).ok() } else { None });
    anyhow::ensure!(f.int(0) == Some(BASILIS) && first.as_ref().and_then(|r| r.text(0)).as_deref() == Some(name), "compteur : TID {:?}, {} ligne(s)", f.int(0), rows.len());
    println!("  ✓ op 7 : {} ligne(s), {name} {} dégât(s)", f.int(1).unwrap_or(0), first.and_then(|r| r.int(6)).unwrap_or(0));
    // Le suivant arrive 2 s plus tard au plus tôt.
    let started = std::time::Instant::now();
    w.world.expect_where(boss(boss_op::DAMAGE), 10_000, "second op 7").await?;
    anyhow::ensure!(started.elapsed() > Duration::from_millis(1500), "compteurs trop rapprochés : {:?}", started.elapsed());
    println!("  ✓ second op 7 après {:.1} s", started.elapsed().as_secs_f64());

    step("remise en état");
    // Un déplacement interrompt l'attaque, puis retour au village.
    let here = archcore_net::Vec3::new(BASILIS_AT.0 + 300.0, target.pos.y, BASILIS_AT.1);
    let mv = &schema::OPT_CHAR_MOVE;
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, here).with(2, here).with(5, 0x04).with(6, 0)).await?;
    gm(&mut w, "/move -475955,-66219").await?;
    gm(&mut w, "/invincible 0").await?;
    gm(&mut w, &format!("/level {level}")).await?;
    println!("\nfenêtre des boss : OK");
    Ok(())
}
