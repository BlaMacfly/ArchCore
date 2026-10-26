//! `archcore sim pvp` : portails des zones de combat entre joueurs (AGPMEVENTTELEPORT 0x31).
//!
//! Un compte MJ frappe un Ver vert au nord d'Anchorville : en combat, sa demande au portail de la cité de
//! Tullan est ignorée ; dix secondes après le dernier coup, elle est accordée. Il part pour l'arène de
//! guilde (zone de combat entre joueurs, `RegionType=1`), d'où « Retour » (op 7) le ramène au portail de
//! Tullan ; l'op 7 est refusée depuis un portail ordinaire.

use super::{enter_world, factors_hp, step, Recv};
use archcore_net::schema::{self, chat_type, mini};
use archcore_net::{build_mini, decode_mini, Fields, Vec3};
use std::time::Duration;

/// Portail de la cité de Tullan (« 도시 툴란 ») et de l'arène de guilde (« 길드경기장 »).
const TULLAN: (i64, Vec3) = (139_331_500, Vec3 { x: -351_857.0, y: 4_000.0, z: 117_796.0 });
const ARENA: (i64, Vec3) = (197_787_762, Vec3 { x: 563_200.0, y: 2_022.0, z: -678_400.0 });
/// Ver verts au nord d'Anchorville.
const HUNT: Vec3 = Vec3 { x: -466_468.0, y: 4_700.0, z: -50_010.0 };
const STOP: i64 = 0x04;

fn dist(a: Vec3, b: Vec3) -> f32 {
    ((a.x - b.x).powi(2) + (a.z - b.z).powi(2)).sqrt()
}

pub async fn run(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let (tp, ch, act, mv, ov, eb) = (&schema::EVENT_TELEPORT, &schema::CHATTING, &schema::OPT_CHAR_ACTION, &schema::OPT_CHAR_MOVE, &schema::OPT_VIEW, &mini::EVENT_BASE);
    let say = |cid: u32, text: &str| Fields::new(ch).with(0, 0).with(1, cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text));
    let base = |object: i64| build_mini(eb, &Fields::new(eb).with(0, 1).with(1, object).with(3, 8));
    let name = |n: &str| encoding_rs::EUC_KR.encode(n).0.into_owned();
    let place = |cid: u32, at: Vec3| Fields::new(mv).with(0, cid).with(1, at).with(2, at).with(5, STOP).with(6, 0);

    step(&format!("{char_name} entre en jeu, invincible, chez les Vers verts"));
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let me = w.cid as i64;
    w.world.send(ch, say(w.cid, "/invincible 1")).await?;
    w.world.send(mv, place(w.cid, HUNT)).await?;
    let near_hunt = |p: Vec3| dist(p, HUNT) < 6000.0;
    let mut worm = w.monsters.iter().find(|m| m.name == "Green Worm" && m.hp > 0 && near_hunt(m.pos)).map(|m| m.cid as i64);
    while let Ok(p) = w.world.expect_where(|q| q.is(ov, Some(1)), 1500, "monstres").await {
        if let Ok(view) = decode_mini(&mini::OPT_VIEW_CHAR, p.fields().bytes(1).unwrap_or_default()) {
            if view.text(2).as_deref() == Some("Green Worm") && view.bytes(5).and_then(factors_hp).unwrap_or(0) > 0 {
                worm.get_or_insert(view.int(0).unwrap_or(0));
            }
        }
    }
    let worm = worm.ok_or_else(|| anyhow::anyhow!("aucun Green Worm en vue"))?;

    step("coup porté : le personnage est en combat");
    w.world.send(act, Fields::new(act).with(0, w.cid).with(1, worm).with(6, 1)).await?;
    let blow = |q: &Recv| q.is(act, None) && ((q.int(0) == Some(me) && q.int(1) == Some(worm)) || (q.int(0) == Some(worm) && q.int(1) == Some(me)));
    w.world.expect_where(blow, 20_000, "premier coup échangé").await?;
    // Plus de coups : le joueur part (le déplacement arrête son attaque), loin du ver.
    w.world.send(mv, place(w.cid, TULLAN.1)).await?;
    let fought = tokio::time::Instant::now();
    println!("  ✓ coup échangé avec le ver {worm:#x}, départ pour le portail de Tullan");

    step("en combat : la demande au portail est ignorée");
    w.world.send(tp, Fields::new(tp).with(0, 4).with(1, base(TULLAN.0)).with(2, w.cid)).await?;
    anyhow::ensure!(w.world.expect_where(|q| q.is(tp, Some(5)), 1500, "accord").await.is_err(), "accord donné en combat");
    println!("  ✓ aucun accord");

    step("dix secondes après le dernier coup : accord");
    tokio::time::sleep_until(fought + Duration::from_millis(10_500)).await;
    w.world.send(tp, Fields::new(tp).with(0, 4).with(1, base(TULLAN.0)).with(2, w.cid)).await?;
    let ok = w.world.expect_where(|q| q.is(tp, Some(5)), 3000, "accord").await?;
    anyhow::ensure!(ok.int(2) == Some(me), "CID de l'accord {:?}", ok.int(2));
    println!("  ✓ accord (op 5)");

    step("trajet vers l'arène de guilde (zone de combat entre joueurs)");
    w.world.send(tp, Fields::new(tp).with(0, 0).with(1, base(TULLAN.0)).with(2, w.cid).with(3, name("길드경기장"))).await?;
    let start = w.world.expect_where(|q| q.is(tp, Some(1)), 3000, "départ").await?;
    let at = start.fields().vec3(4).ok_or_else(|| anyhow::anyhow!("position d'arrivée absente"))?;
    anyhow::ensure!(dist(at, ARENA.1) <= 1_601.0, "arrivée à {:.0} unités de l'arène", dist(at, ARENA.1));
    w.world.expect_where(|q| q.is(tp, Some(8)), 3000, "chargement").await?;
    println!("  ✓ arrivée en {:.0}, {:.0} ({:.0} unités du portail de l'arène)", at.x, at.z, dist(at, ARENA.1));

    step("« Retour » (op 7) depuis l'arène : de retour au portail de départ");
    w.world.send(tp, Fields::new(tp).with(0, 4).with(1, base(ARENA.0)).with(2, w.cid)).await?;
    w.world.expect_where(|q| q.is(tp, Some(5)), 3000, "accord à l'arène").await?;
    w.world.send(tp, Fields::new(tp).with(0, 7).with(1, base(ARENA.0)).with(2, w.cid)).await?;
    let back = w.world.expect_where(|q| q.is(tp, Some(1)), 3000, "départ du retour").await?;
    let at = back.fields().vec3(4).ok_or_else(|| anyhow::anyhow!("position du retour absente"))?;
    anyhow::ensure!(dist(at, TULLAN.1) <= 1_601.0, "retour à {:.0} unités du portail de Tullan", dist(at, TULLAN.1));
    w.world.expect_where(|q| q.is(tp, Some(8)), 3000, "chargement").await?;
    println!("  ✓ retour en {:.0}, {:.0}, à {:.0} unités du portail de Tullan", at.x, at.z, dist(at, TULLAN.1));

    step("op 7 refusée depuis un portail ordinaire");
    w.world.send(tp, Fields::new(tp).with(0, 7).with(1, base(TULLAN.0)).with(2, w.cid)).await?;
    anyhow::ensure!(w.world.expect_where(|q| q.is(tp, Some(1)), 1000, "départ").await.is_err(), "retour accepté hors d'une zone de combat");
    println!("  ✓ aucun départ");

    w.world.send(ch, say(w.cid, "/invincible 0")).await?;
    w.world.expect_where(|q| q.ptype == ch.id, 3000, "réponse à /invincible 0").await?;
    println!("\nzones de combat entre joueurs : OK");
    Ok(())
}
