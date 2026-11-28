//! `archcore sim itemlock` : verrou des objets (AGPMITEM op 36, `world/item_lock.rs`).
//!
//! GmTest reçoit une Short Sword (TID 11) et la verrouille (op 36, champ 19 = 0x2000 ← op 36 à 0x2000, en base
//! `locked` = 1). La destruction (op 21) est refusée : l'épée reste, y compris après une nouvelle entrée en jeu.
//! Déverrouillée (← op 36 à 0), elle se détruit (op 1).

use super::{character_id, enter_world, step, InWorld, Recv};
use archcore_net::schema::{self, chat_type};
use archcore_net::Fields;

const SWORD: i64 = 11;
const LOCKED: i64 = 0x2000;

fn say(w: &InWorld, text: &str) -> Fields {
    let ch = &schema::CHATTING;
    Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text))
}

async fn lock(w: &mut InWorld, iid: i64, what: &str) -> anyhow::Result<Recv> {
    let it = &schema::ITEM;
    w.world.send(it, Fields::new(it).with(0, 36).with(2, iid).with(4, w.cid as i64).with(19, LOCKED)).await?;
    w.world.expect_where(|q| q.is(it, Some(36)) && q.int(2) == Some(iid), 3000, what).await
}

async fn locked_in_db(db: &crate::db::Databases, iid: i64) -> anyhow::Result<i64> {
    let row = db.characters.first("SELECT locked FROM character_items WHERE id = ?", (iid,)).await?;
    Ok(row.map(|r| crate::db::col::<i64>(&r, "locked")).unwrap_or(-1))
}

pub async fn run(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let db = crate::db::Databases::new(&cfg.database);
    let it = &schema::ITEM;
    let id = character_id(&db, char_name).await?;
    db.characters.exec("UPDATE character_items SET locked = 0 WHERE character_id = ?", (id,)).await?;

    step(&format!("{char_name} reçoit une épée et la verrouille"));
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    w.world.send(&schema::CHATTING, say(&w, &format!("/additem {SWORD} 1"))).await?;
    let sword = w.world.expect_where(|q| q.is(it, Some(0)) && q.int(3) == Some(SWORD), 3000, "épée reçue").await?.int(2).unwrap_or(0);
    let on = lock(&mut w, sword, "verrou").await?;
    anyhow::ensure!(on.int(19) == Some(LOCKED) && locked_in_db(&db, sword).await? == 1, "op 36 : drapeaux {:?}", on.int(19));
    println!("  ✓ op 36 à 0x2000, en base locked = 1");

    step("destruction refusée");
    w.world.send(it, Fields::new(it).with(0, 21).with(2, sword).with(4, w.cid as i64)).await?;
    anyhow::ensure!(w.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(sword), 800, "destruction").await.is_err(), "épée verrouillée détruite");
    println!("  ✓ épée gardée");

    step("nouvelle entrée en jeu : le verrou tient");
    drop(w);
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    w.world.send(it, Fields::new(it).with(0, 21).with(2, sword).with(4, w.cid as i64)).await?;
    anyhow::ensure!(w.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(sword), 800, "destruction").await.is_err(), "verrou perdu à l'entrée");
    println!("  ✓ verrou relu de la base (destruction toujours refusée)");

    step("déverrouillée, l'épée se détruit");
    let off = lock(&mut w, sword, "déverrou").await?;
    anyhow::ensure!(off.int(19) == Some(0) && locked_in_db(&db, sword).await? == 0, "op 36 : drapeaux {:?}", off.int(19));
    w.world.send(it, Fields::new(it).with(0, 21).with(2, sword).with(4, w.cid as i64)).await?;
    w.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(sword), 3000, "destruction").await?;
    println!("  ✓ op 36 à 0, épée détruite (op 1)");
    println!("\nverrou des objets : OK");
    Ok(())
}
