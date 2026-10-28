//! `archcore sim repair` : usure des objets et réparation chez Halo (AgpmEventItemRepair 0x34, fonction 17).
//!
//! Configuration du simulateur : `Item.WeaponWearChance = 100` (chaque coup qui blesse use l'arme). Le MJ reçoit une
//! Short Sword (DUR 88, 100 au PNJ) et une Halfsteel Armor (DUR 63, 30), les porte, frappe un Ver vert jusqu'à voir
//! l'épée s'user d'un point par coup (AGPMITEM op 2, champ 10). /durability 0 : l'épée cassée ne compte plus dans
//! les dégâts ; /durability 40 : durabilité relue à la reconnexion. Les deux objets rangés, chez Halo : ouverture
//! (op 3 → op 4), coût faux (code 4), réparation (argent débité, objets pleins, code 1), objets déjà pleins (code 3),
//! potion (code 5).

use super::{enter_world, factors_combat, item_durability, step, InWorld, Recv, SeenMonster};
use archcore_net::packet::Value;
use archcore_net::schema::{self, chat_type, mini};
use archcore_net::{build_mini, decode_mini, Fields, Vec3};
use std::time::Duration;

const SWORD: i64 = 11; // Short Sword : DUR 88, NPC Price 100, guerrier
const ARMOR: i64 = 324; // Halfsteel Armor : DUR 63, NPC Price 30, guerrier humain niveau 2
const POTION: i64 = 190; // Small Life Potion : sans durabilité
const SMITH: Vec3 = Vec3 { x: -479_400.0, y: 4_955.0, z: -69_300.0 };
const HUNT: Vec3 = Vec3 { x: -466_468.0, y: 4_700.0, z: -50_010.0 };
const STOP: i64 = 0x04;

fn say(w: &InWorld, text: &str) -> Fields {
    let ch = &schema::CHATTING;
    Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text))
}

/// Commande MJ et sa réponse (message système).
async fn gm(w: &mut InWorld, text: &str, expect: &str) -> anyhow::Result<String> {
    w.world.send(&schema::CHATTING, say(w, text)).await?;
    let ch = &schema::CHATTING;
    let r = w.world.expect_where(|q| q.ptype == ch.id && q.fields().text(6).is_some_and(|t| t.contains(expect)), 3000, text).await?;
    Ok(r.fields().text(6).unwrap_or_default())
}

async fn give(w: &mut InWorld, tid: i64) -> anyhow::Result<i64> {
    w.world.send(&schema::CHATTING, say(w, &format!("/additem {tid} 1"))).await?;
    let it = &schema::ITEM;
    Ok(w.world.expect_where(|q| q.is(it, Some(0)) && q.int(3) == Some(tid) && q.int(1) == Some(2), 3000, "objet ajouté").await?.int(2).unwrap_or(0))
}

async fn move_to(w: &mut InWorld, at: Vec3) -> anyhow::Result<()> {
    let mv = &schema::OPT_CHAR_MOVE;
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, at).with(2, at).with(5, STOP).with(6, 0)).await
}

/// Mise à jour de durabilité d'un objet : op 2 avec le champ 10.
fn is_wear(iid: i64) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| q.is(&schema::ITEM, Some(2)) && q.int(2) == Some(iid) && q.fields().bytes(10).is_some()
}

fn wear_of(q: &Recv) -> i64 {
    q.fields().bytes(10).and_then(item_durability).unwrap_or(-1)
}

fn repair_fields(op: i64, w: &InWorld, smith: i64, iids: &[i64], cost: i64) -> Fields {
    let (s, eb) = (&schema::EVENT_ITEM_REPAIR, &mini::EVENT_BASE);
    let event = build_mini(eb, &Fields::new(eb).with(0, 3).with(1, smith).with(3, 17));
    let grid = Value::List((0..20).map(|i| Value::Int(iids.get(i).copied().unwrap_or(0))).collect());
    let f = Fields::new(s).with(0, op).with(1, event).with(2, w.cid);
    if op == 0 { f.with(3, grid).with(4, cost).with(5, 0) } else { f }
}

async fn expect_code(w: &mut InWorld, code: i64, what: &str) -> anyhow::Result<()> {
    let s = &schema::EVENT_ITEM_REPAIR;
    let r = w.world.expect_where(|q| q.is(s, Some(1)), 3000, what).await?;
    anyhow::ensure!(r.int(5) == Some(code) && r.int(2) == Some(w.cid as i64), "{what} : code {:?}", r.int(5));
    println!("  ✓ {what} : code {code}");
    Ok(())
}

pub async fn run(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let (c, it, act) = (&schema::CHARACTER, &schema::ITEM, &schema::OPT_CHAR_ACTION);
    step(&format!("{char_name} entre en jeu, reçoit une épée et une armure"));
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let me = w.cid as i64;
    gm(&mut w, "/invincible 1", "Invincibilité").await?;
    gm(&mut w, "/money 1000", "pièce(s) d'or").await?;
    let (sword, armor) = (give(&mut w, SWORD).await?, give(&mut w, ARMOR).await?);
    let mut damage = (0, 0);
    for iid in [sword, armor] {
        w.world.send(it, Fields::new(it).with(0, 2).with(1, 3).with(2, iid)).await?;
        w.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(iid) && q.int(1) == Some(3), 3000, "objet porté").await?;
        let f = w.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(me) && q.fields().bytes(7).is_some(), 3000, "facteurs").await?;
        damage = factors_combat(f.fields().bytes(7).unwrap_or_default()).map(|f| f.0).unwrap_or_default();
    }
    println!("  ✓ épée et armure portées : dégâts {} à {}", damage.0, damage.1);

    step("usure : un Ver vert frappé jusqu'à user l'épée");
    move_to(&mut w, HUNT).await?;
    let mut seen = w.monsters.clone();
    let ov = &schema::OPT_VIEW;
    while let Ok(p) = w.world.expect_where(|q| q.is(ov, Some(1)), 1500, "monstres").await {
        if let Ok(view) = decode_mini(&mini::OPT_VIEW_CHAR, p.fields().bytes(1).unwrap_or_default()) {
            if view.int(0).unwrap_or(0) >= crate::world::monsters::MONSTER_CID_BASE as i64 {
                seen.push(SeenMonster::from_view(&view));
            }
        }
    }
    let dist = |p: Vec3| (((p.x - HUNT.x) as f64).powi(2) + ((p.z - HUNT.z) as f64).powi(2)).sqrt();
    let mut wears = Vec::new();
    for target in {
        let mut worms: Vec<SeenMonster> = seen.into_iter().filter(|m| m.hp > 0 && m.name == "Green Worm").collect();
        worms.sort_by_key(|m| dist(m.pos) as i64);
        worms
    } {
        w.world.send(act, Fields::new(act).with(0, w.cid).with(1, target.cid).with(6, 1)).await?;
        // Chaque coup qui blesse use l'épée d'un point (probabilité 100 % dans la configuration du simulateur).
        while wears.len() < 3 {
            let Ok(q) = w.world.expect_where(|q| is_wear(sword)(q) || (q.is(c, Some(1)) && q.int(1) == Some(target.cid as i64) && q.int(11) == Some(2)), 30_000, "usure de l'épée").await else { break };
            if q.ptype == c.id {
                break; // ver mort : au suivant
            }
            wears.push(wear_of(&q));
        }
        if wears.len() >= 3 {
            break;
        }
    }
    anyhow::ensure!(wears.len() >= 3, "épée usée {} fois seulement", wears.len());
    anyhow::ensure!(wears[0] == 87 && wears.windows(2).all(|p| p[1] == p[0] - 1), "durabilités {wears:?}");
    println!("  ✓ épée usée d'un point par coup : {wears:?}");
    move_to(&mut w, SMITH).await?;
    tokio::time::sleep(Duration::from_millis(300)).await;

    step("épée cassée : elle ne compte plus");
    // Les facteurs recalculés précèdent la réponse à la commande.
    w.world.send(&schema::CHATTING, say(&w, "/durability 0")).await?;
    let f = w.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(me) && q.fields().bytes(7).is_some(), 3000, "facteurs").await?;
    let broken = factors_combat(f.fields().bytes(7).unwrap_or_default()).map(|f| f.0).unwrap_or_default();
    anyhow::ensure!(broken.0 <= damage.0 - 11 && broken.1 <= damage.1 - 13, "dégâts {broken:?} avec l'épée cassée (portée : {damage:?})");
    println!("  ✓ dégâts {} à {} (épée comprise : {} à {})", broken.0, broken.1, damage.0, damage.1);
    gm(&mut w, "/durability 40", "Durabilité 40").await?;
    for iid in [sword, armor] {
        let any = &mini::ITEM_GRID;
        let grid = build_mini(any, &Fields::new(any).with(0, -1).with(1, -1).with(2, -1));
        w.world.send(it, Fields::new(it).with(0, 2).with(1, 2).with(2, iid).with(7, grid)).await?;
        w.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(iid) && q.int(1) == Some(2), 3000, "objet rangé").await?;
    }

    step("reconnexion : durabilité relue");
    drop(w);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let read = |iid: i64| w.durabilities.iter().find(|d| d.0 == iid).map(|d| d.1);
    anyhow::ensure!(read(sword) == Some(40) && read(armor) == Some(40), "durabilités relues : {:?}", w.durabilities);
    println!("  ✓ épée et armure à 40 à l'entrée en jeu");

    step("réparation chez Halo");
    move_to(&mut w, SMITH).await?;
    let mut smith = w.npcs.iter().find(|n| n.1.ends_with(" Halo")).map(|n| n.0);
    while let Ok(p) = w.world.expect_where(|q| q.is(ov, Some(1)), 1500, "PNJ").await {
        if let Ok(view) = decode_mini(&mini::OPT_VIEW_CHAR, p.fields().bytes(1).unwrap_or_default()) {
            if view.text(2).is_some_and(|n| n.ends_with(" Halo")) {
                smith = view.int(0);
            }
        }
    }
    let smith = smith.ok_or_else(|| anyhow::anyhow!("Halo introuvable"))?;
    let s = &schema::EVENT_ITEM_REPAIR;
    w.world.send(s, repair_fields(3, &w, smith, &[], 0)).await?;
    let grant = w.world.expect_where(|q| q.is(s, Some(4)), 3000, "fenêtre de réparation").await?;
    anyhow::ensure!(grant.int(2) == Some(me), "accord pour {:?}", grant.int(2));
    println!("  ✓ fenêtre ouverte ({smith:#x}, op 4)");
    let cost = crate::world::repair::repair_cost(100, 88, 40) + crate::world::repair::repair_cost(30, 63, 40);
    w.world.send(s, repair_fields(0, &w, smith, &[sword, armor], cost + 1)).await?;
    expect_code(&mut w, 4, "coût faux").await?;
    w.world.send(s, repair_fields(0, &w, smith, &[sword, armor], cost)).await?;
    let money = w.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(me) && q.int(8).is_some(), 3000, "argent débité").await?.int(8).unwrap_or(0);
    anyhow::ensure!(money == 1000 - cost, "argent {money} après une réparation de {cost}");
    for (iid, max) in [(sword, 88), (armor, 63)] {
        let q = w.world.expect_where(is_wear(iid), 3000, "objet réparé").await?;
        anyhow::ensure!(wear_of(&q) == max, "objet {iid} réparé à {}", wear_of(&q));
    }
    expect_code(&mut w, 1, &format!("réparation pour {cost} pièces (reste {money})")).await?;
    w.world.send(s, repair_fields(0, &w, smith, &[sword, armor], 0)).await?;
    expect_code(&mut w, 3, "objets déjà pleins").await?;
    let potion = give(&mut w, POTION).await?;
    w.world.send(s, repair_fields(0, &w, smith, &[potion], 0)).await?;
    expect_code(&mut w, 5, "potion").await?;

    // Remise en état : objets détruits, plus d'invincibilité.
    for iid in [sword, armor, potion] {
        w.world.send(it, Fields::new(it).with(0, 21).with(2, iid)).await?;
        w.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(iid), 3000, "objet détruit").await?;
    }
    gm(&mut w, "/invincible 0", "Invincibilité").await?;
    println!("\nréparation : OK");
    Ok(())
}
