//! `archcore sim aggro` : monstres préventifs (`PreemptiveType` 1 de leur IA).
//!
//! Au nord d'Anchorville, quatre Vers jaunes ont une IA préventive (portée 7 m, 700 unités) et se promènent ; leurs
//! voisins de même nom n'en ont pas. Le personnage se place à côté des Vers jaunes en vue, du plus proche au plus
//! loin, sans rien attaquer : l'un d'eux doit le frapper de lui-même (AGPMOPTIMIZEDCHARACTION, acteur = le monstre).
//! Un poste fixe ne suffit pas : les vers en promenade peuvent rester hors de portée.

use super::{enter_world, step};
use archcore_net::schema::{self, chat_type, mini};
use archcore_net::{decode_mini, Fields, Vec3};
use std::collections::BTreeMap;

const YELLOW_WORM: Vec3 = Vec3 { x: -471_120.0, y: 4_700.0, z: -45_262.0 };
const MONSTER_CID_BASE: i64 = crate::world::monsters::MONSTER_CID_BASE as i64;
/// Vers essayés, et attente près de chacun (ms).
const TRIES: usize = 12;
const WAIT_MS: u64 = 4000;

fn flat(a: Vec3, b: Vec3) -> f64 {
    (((a.x - b.x) as f64).powi(2) + ((a.z - b.z) as f64).powi(2)).sqrt()
}

pub async fn run(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let (ch, act, mv, ov) = (&schema::CHATTING, &schema::OPT_CHAR_ACTION, &schema::OPT_CHAR_MOVE, &schema::OPT_VIEW);
    let say = |cid: u32, text: &str| Fields::new(ch).with(0, 0).with(1, cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text));

    step(&format!("{char_name} se poste près des Vers jaunes, sans attaquer"));
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let me = w.cid as i64;
    anyhow::ensure!(w.level < 90, "{char_name} est au niveau {} : ces vers n'attaquent pas d'eux-mêmes au niveau 90 et plus", w.level);
    w.world.send(ch, say(w.cid, "/invincible 0")).await?;
    w.world.expect_where(|q| q.ptype == ch.id, 3000, "invincibilité levée").await?;

    // Dernière position connue de chaque Ver jaune en vue.
    let mut worms: BTreeMap<i64, Vec3> = w.monsters.iter().filter(|m| m.name == "Yellow Worm").map(|m| (m.cid as i64, m.pos)).collect();
    let mut tried: Vec<i64> = Vec::new();
    let mut spot = YELLOW_WORM;
    step("un Ver jaune attaque de lui-même");
    for _ in 0..=TRIES {
        w.world.send(ch, say(w.cid, &format!("/move {:.0},{:.0}", spot.x, spot.z))).await?;
        // Les vers arrivent avec la téléportation, avant la réponse du MJ : rien n'est jeté en l'attendant.
        let end = tokio::time::Instant::now() + std::time::Duration::from_millis(WAIT_MS);
        loop {
            let left = end.saturating_duration_since(tokio::time::Instant::now()).as_millis() as u64;
            if left == 0 {
                break;
            }
            let Ok(q) = w.world.expect_where(|q| q.is(act, None) || q.is(mv, None) || q.is(ov, Some(1)), left, "").await else { break };
            if q.is(act, None) {
                if q.int(1) == Some(me) && q.int(0).is_some_and(|c| c >= MONSTER_CID_BASE) {
                    println!("  ✓ frappé par le monstre {:#x} sans l'avoir attaqué ({} ver(s) essayé(s))", q.int(0).unwrap_or(0), tried.len());
                    w.world.send(ch, say(w.cid, "/move -476000,-61000")).await?;
                    println!("\nmonstres préventifs : OK");
                    return Ok(());
                }
                continue;
            }
            let f = q.fields();
            if q.is(mv, None) {
                if let (Some(cid), Some(to)) = (f.int(0), f.vec3(2)) {
                    if let Some(p) = worms.get_mut(&cid) {
                        *p = to;
                    }
                }
            } else if let Ok(view) = decode_mini(&mini::OPT_VIEW_CHAR, f.bytes(1).unwrap_or_default()) {
                if view.text(2).as_deref() == Some("Yellow Worm") {
                    if let (Some(cid), Some(p)) = (view.int(0), view.vec3(8)) {
                        worms.insert(cid, p);
                    }
                }
            }
        }
        let Some((cid, pos)) = worms.iter().filter(|(c, _)| !tried.contains(c)).min_by(|a, b| flat(*a.1, YELLOW_WORM).total_cmp(&flat(*b.1, YELLOW_WORM))).map(|(c, p)| (*c, *p)) else { break };
        tried.push(cid);
        spot = pos;
    }
    anyhow::bail!("aucun des {} Vers jaunes essayés n'a attaqué ({} en vue)", tried.len(), worms.len())
}
