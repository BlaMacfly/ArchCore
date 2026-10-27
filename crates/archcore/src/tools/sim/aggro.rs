//! `archcore sim aggro` : monstres préventifs (`PreemptiveType` 1 de leur IA).
//!
//! Le personnage se poste sur le point d'apparition d'un Ver jaune (IA préventive, portée 7 m) au nord d'Anchorville,
//! sans rien attaquer : le monstre doit le frapper de lui-même (AGPMOPTIMIZEDCHARACTION, acteur = le monstre).

use super::{enter_world, step};
use archcore_net::schema::{self, chat_type};
use archcore_net::{Fields, Vec3};

const YELLOW_WORM: Vec3 = Vec3 { x: -471_120.0, y: 4_700.0, z: -45_262.0 };
const MONSTER_CID_BASE: i64 = crate::world::monsters::MONSTER_CID_BASE as i64;

pub async fn run(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let (ch, act) = (&schema::CHATTING, &schema::OPT_CHAR_ACTION);
    let say = |cid: u32, text: &str| Fields::new(ch).with(0, 0).with(1, cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text));

    step(&format!("{char_name} se poste près d'un Ver jaune, sans attaquer"));
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let me = w.cid as i64;
    w.world.send(ch, say(w.cid, "/invincible 0")).await?;
    w.world.expect_where(|q| q.ptype == ch.id, 3000, "invincibilité levée").await?;
    w.world.send(ch, say(w.cid, &format!("/move {},{}", YELLOW_WORM.x, YELLOW_WORM.z))).await?;
    w.world.expect_where(|q| q.ptype == ch.id, 3000, "déplacement").await?;

    step("le Ver jaune attaque de lui-même");
    let hit = w.world.expect_where(|q| q.is(act, None) && q.int(1) == Some(me) && q.int(0).is_some_and(|c| c >= MONSTER_CID_BASE), 30_000, "coup d'un monstre").await?;
    println!("  ✓ frappé par le monstre {:#x} sans l'avoir attaqué", hit.int(0).unwrap_or(0));
    w.world.send(ch, say(w.cid, "/move -476000,-61000")).await?;
    println!("\nmonstres préventifs : OK");
    Ok(())
}
