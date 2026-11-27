//! `archcore sim charging` : inventaire étendu (statut 15, `world/charging.rs`).
//!
//! GmTest reçoit un petit sac (Small Size Inventory <1day>, TID 50318) et un grand sac (Large Size Inventory <1day>,
//! TID 50317), objets Cash d'usage 38, et une Short Sword (TID 11). Sans sac allumé, le dépôt au statut 15 est refusé
//! (épée renvoyée au statut 2). Petit sac allumé (op 3 : op 26 à 1) : la ligne 4 est refusée (hors du sac de 4 × 4),
//! la case [0, 3, 3] admise (op 2 statut 15, en base statut 8). Petit sac éteint, grand sac allumé : l'épée passe en
//! [2, 7, 3]. Retrait vers l'inventaire (statut 2), puis le grand sac éteint.

use super::{character_id, enter_world, step, InWorld, Recv};
use archcore_net::schema::{self, chat_type, mini};
use archcore_net::{build_mini, decode_mini, Fields};

const SMALL_BAG: i64 = 50318;
const LARGE_BAG: i64 = 50317;
const SWORD: i64 = 11;
const CHARGING: i64 = 15;

fn say(w: &InWorld, text: &str) -> Fields {
    let ch = &schema::CHATTING;
    Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text))
}

fn grid(g: [i64; 3]) -> Vec<u8> {
    let s = &mini::ITEM_GRID;
    build_mini(s, &Fields::new(s).with(0, g[0]).with(1, g[1]).with(2, g[2]))
}

fn grid_of(r: &Recv) -> Option<[i64; 3]> {
    let g = decode_mini(&mini::ITEM_GRID, r.fields().bytes(7)?).ok()?;
    Some([g.int(0)?, g.int(1)?, g.int(2)?])
}

fn in_use(r: &Recv) -> Option<i64> {
    decode_mini(&mini::ITEM_CASH, r.fields().bytes(23)?).ok()?.int(0)
}

async fn add_item(w: &mut InWorld, tid: i64) -> anyhow::Result<Recv> {
    let it = &schema::ITEM;
    w.world.send(&schema::CHATTING, say(w, &format!("/additem {tid} 1"))).await?;
    w.world.expect_where(|q| q.is(it, Some(0)) && q.int(3) == Some(tid), 3000, "objet reçu").await
}

/// Op 3 sur un sac : le mini Cash (op 26) attendu.
async fn toggle(w: &mut InWorld, iid: i64, want: i64, what: &str) -> anyhow::Result<()> {
    let it = &schema::ITEM;
    w.world.send(it, Fields::new(it).with(0, 3).with(2, iid)).await?;
    let q = w.world.expect_where(|q| q.is(it, Some(26)) && q.int(2) == Some(iid), 3000, what).await?;
    anyhow::ensure!(in_use(&q) == Some(want), "{what} : mini Cash {:?}", in_use(&q));
    Ok(())
}

/// Dépôt de l'épée au statut `status`, case `g` : le paquet renvoyé.
async fn put(w: &mut InWorld, sword: i64, status: i64, g: Option<[i64; 3]>, what: &str) -> anyhow::Result<Recv> {
    let it = &schema::ITEM;
    let mut f = Fields::new(it).with(0, 2).with(1, status).with(2, sword).with(4, w.cid as i64);
    if let Some(g) = g {
        f = f.with(7, grid(g));
    }
    w.world.send(it, f).await?;
    w.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(sword), 3000, what).await
}

async fn stored(db: &crate::db::Databases, iid: i64) -> anyhow::Result<i64> {
    let row = db.characters.first("SELECT status FROM character_items WHERE id = ?", (iid,)).await?;
    Ok(row.map(|r| crate::db::col::<i64>(&r, "status")).unwrap_or(-1))
}

pub async fn run(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let db = crate::db::Databases::new(&cfg.database);
    let id = character_id(&db, char_name).await?;
    // Restes d'un essai précédent : sacs et contenu de l'inventaire étendu.
    db.characters.exec("DELETE FROM character_items WHERE character_id = ? AND (template_id IN (?, ?) OR status = 8)", (id, SMALL_BAG, LARGE_BAG)).await?;

    step(&format!("{char_name} reçoit un petit sac, un grand sac et une épée"));
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let small = add_item(&mut w, SMALL_BAG).await?.int(2).unwrap_or(0);
    let large = add_item(&mut w, LARGE_BAG).await?.int(2).unwrap_or(0);
    let sword = add_item(&mut w, SWORD).await?.int(2).unwrap_or(0);
    println!("  ✓ sacs {small} et {large}, épée {sword}");

    step("sans sac allumé : dépôt refusé");
    let back = put(&mut w, sword, CHARGING, Some([0, 0, 0]), "épée renvoyée").await?;
    anyhow::ensure!(back.int(1) == Some(2), "épée au statut {:?}", back.int(1));
    println!("  ✓ épée renvoyée au statut 2");

    step("petit sac allumé : un sac de 4 × 4");
    toggle(&mut w, small, 1, "petit sac allumé").await?;
    let out = put(&mut w, sword, CHARGING, Some([0, 4, 0]), "ligne 4 refusée").await?;
    anyhow::ensure!(out.int(1) == Some(2), "ligne 4 admise : statut {:?}", out.int(1));
    let inside = put(&mut w, sword, CHARGING, Some([0, 3, 3]), "épée dans le petit sac").await?;
    anyhow::ensure!(inside.int(1) == Some(CHARGING) && grid_of(&inside) == Some([0, 3, 3]), "statut {:?} en {:?}", inside.int(1), grid_of(&inside));
    anyhow::ensure!(stored(&db, sword).await? == 8, "épée enregistrée au statut {}", stored(&db, sword).await?);
    println!("  ✓ ligne 4 refusée ; [0, 3, 3] admise (statut 15, en base 8)");

    step("grand sac : 4 sacs de 8 × 4");
    toggle(&mut w, small, 0, "petit sac éteint").await?;
    toggle(&mut w, large, 1, "grand sac allumé").await?;
    let moved = put(&mut w, sword, CHARGING, Some([2, 7, 3]), "épée en [2, 7, 3]").await?;
    anyhow::ensure!(moved.int(1) == Some(CHARGING) && grid_of(&moved) == Some([2, 7, 3]), "statut {:?} en {:?}", moved.int(1), grid_of(&moved));
    println!("  ✓ épée déplacée en [2, 7, 3]");

    step("retrait vers l'inventaire, grand sac éteint");
    let taken = put(&mut w, sword, 2, None, "épée sortie").await?;
    anyhow::ensure!(taken.int(1) == Some(2) && stored(&db, sword).await? == 0, "épée au statut {:?}", taken.int(1));
    toggle(&mut w, large, 0, "grand sac éteint").await?;
    println!("  ✓ épée au statut 2 (en base 0), sac éteint");
    println!("\ninventaire étendu : OK");
    Ok(())
}
