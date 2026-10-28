//! `archcore sim itemset` : commutateur d'équipement (AgpmItemSet 0x6A).
//!
//! Jeux du MJ effacés. À l'entrée : op 4 (aucun actif). Il reçoit une Short Sword et une Halfsteel Armor (champ 26 =
//! IID), les porte, enregistre l'emplacement 3 (op 0 → op 3 rempli, op 5 « E,2,armure:E,10,épée »), les range, puis
//! porte l'emplacement 3 (op 1 : les deux objets équipés, op 4 actif 3). Déplacement (op 6) puis retrait (op 7) d'un
//! objet du jeu. Emplacement vide 5 porté : tout est retiré. Reconnexion : ops 3, 5 puis 4 relus. Emplacement vidé
//! (op 2).

use super::{enter_world, step, InWorld, Recv};
use archcore_net::schema::{self, chat_type, item_set_op, mini};
use archcore_net::{build_mini, Fields};
use std::time::Duration;

const SWORD: i64 = 11;
const ARMOR: i64 = 324;

fn say(w: &InWorld, text: &str) -> Fields {
    let ch = &schema::CHATTING;
    Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text))
}

async fn give(w: &mut InWorld, tid: i64) -> anyhow::Result<i64> {
    w.world.send(&schema::CHATTING, say(w, &format!("/additem {tid} 1"))).await?;
    let it = &schema::ITEM;
    let r = w.world.expect_where(|q| q.is(it, Some(0)) && q.int(3) == Some(tid) && q.int(1) == Some(2), 3000, "objet ajouté").await?;
    anyhow::ensure!(r.int(26) == r.int(2), "champ 26 {:?} pour l'IID {:?}", r.int(26), r.int(2));
    Ok(r.int(2).unwrap_or(0))
}

/// Paquet du client : les 4 champs, objets vides.
fn set(op: i64, slot: i64, arg: i64) -> Fields {
    let s = &schema::ITEM_SET;
    Fields::new(s).with(0, op).with(1, slot).with(2, arg).with(3, "")
}

fn is_op(op: i64) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| q.is(&schema::ITEM_SET, Some(op))
}

/// Op 3 puis op 5 d'un emplacement : (rempli, objets).
async fn content(w: &mut InWorld, slot: i64) -> anyhow::Result<(i64, String)> {
    let flag = w.world.expect_where(|q| is_op(item_set_op::FLAG)(q) && q.int(1) == Some(slot), 3000, "op 3").await?;
    let c = w.world.expect_where(|q| is_op(item_set_op::CONTENT)(q) && q.int(1) == Some(slot), 3000, "op 5").await?;
    Ok((flag.int(2).unwrap_or(-1), c.fields().text(3).unwrap_or_default()))
}

async fn active(w: &mut InWorld) -> anyhow::Result<i64> {
    Ok(w.world.expect_where(is_op(item_set_op::ACTIVE), 3000, "op 4").await?.int(1).unwrap_or(-9))
}

/// Objet porté (op 2, statut 3) ou rangé (statut 2).
async fn placed(w: &mut InWorld, iid: i64, status: i64) -> anyhow::Result<()> {
    let it = &schema::ITEM;
    w.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(iid) && q.int(1) == Some(status), 3000, if status == 3 { "objet porté" } else { "objet rangé" }).await?;
    Ok(())
}

pub async fn run(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let (it, s) = (&schema::ITEM, &schema::ITEM_SET);
    let db = crate::db::Databases::new(&cfg.database);
    let id = super::character_id(&db, char_name).await?;
    let wipe = || async {
        db.characters.exec("DELETE FROM character_item_set WHERE character_id = ?", (id,)).await?;
        db.characters.exec("UPDATE characters SET item_set_active = -1 WHERE id = ?", (id,)).await
    };
    wipe().await?;

    step(&format!("{char_name} entre, reçoit et porte une épée et une armure"));
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    anyhow::ensure!(w.item_keys.iter().all(|(iid, key)| iid == key), "champ 26 à l'entrée : {:?}", w.item_keys);
    anyhow::ensure!(active(&mut w).await? == -1, "emplacement actif à l'entrée");
    println!("  ✓ entrée : aucun jeu, op 4 (-1)");
    let (sword, armor) = (give(&mut w, SWORD).await?, give(&mut w, ARMOR).await?);
    for iid in [sword, armor] {
        w.world.send(it, Fields::new(it).with(0, 2).with(1, 3).with(2, iid)).await?;
        placed(&mut w, iid, 3).await?;
    }

    step("enregistrer l'équipement dans l'emplacement 3");
    w.world.send(s, set(item_set_op::SAVE, 3, 0)).await?;
    let (flag, items) = content(&mut w, 3).await?;
    let expected = format!("E,2,{armor}:E,10,{sword}");
    anyhow::ensure!(flag == 1 && items == expected, "emplacement 3 : {flag}, « {items} »");
    anyhow::ensure!(w.world.expect_where(is_op(item_set_op::ACTIVE), 600, "op 4").await.is_err(), "op 4 après un enregistrement");
    println!("  ✓ op 3 (rempli), op 5 « {items} », pas d'op 4");

    step("tout ranger, puis porter l'emplacement 3 (op 1)");
    let any = &mini::ITEM_GRID;
    let grid = build_mini(any, &Fields::new(any).with(0, -1).with(1, -1).with(2, -1));
    for iid in [sword, armor] {
        w.world.send(it, Fields::new(it).with(0, 2).with(1, 2).with(2, iid).with(7, grid.clone())).await?;
        placed(&mut w, iid, 2).await?;
    }
    w.world.send(s, set(item_set_op::EQUIP, 3, 0)).await?;
    placed(&mut w, armor, 3).await?;
    placed(&mut w, sword, 3).await?;
    anyhow::ensure!(active(&mut w).await? == 3, "emplacement actif");
    println!("  ✓ armure et épée portées, op 4 (3)");

    step("déplacer puis retirer un objet du jeu");
    w.world.send(s, set(item_set_op::MOVE, 3, 1)).await?; // de 0 vers 1
    let (_, items) = content(&mut w, 3).await?;
    anyhow::ensure!(items == format!("E,10,{sword}:E,2,{armor}"), "après op 6 : « {items} »");
    w.world.send(s, set(item_set_op::REMOVE, 3, 1)).await?;
    let (flag, items) = content(&mut w, 3).await?;
    anyhow::ensure!(flag == 1 && items == format!("E,10,{sword}"), "après op 7 : {flag}, « {items} »");
    println!("  ✓ op 6 puis op 7 : « {items} »");

    step("porter l'emplacement vide 5 : tout est retiré");
    w.world.send(s, set(item_set_op::EQUIP, 5, 0)).await?;
    placed(&mut w, armor, 2).await?;
    placed(&mut w, sword, 2).await?;
    anyhow::ensure!(active(&mut w).await? == 5, "emplacement actif");
    println!("  ✓ épée et armure rangées, op 4 (5)");

    step("reconnexion : jeux relus");
    drop(w);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let (flag, items) = content(&mut w, 3).await?;
    anyhow::ensure!(flag == 1 && items == format!("E,10,{sword}"), "emplacement 3 à l'entrée : {flag}, « {items} »");
    anyhow::ensure!(active(&mut w).await? == 5, "emplacement actif à l'entrée");
    println!("  ✓ ops 3, 5 (« {items} ») puis 4 (5)");
    w.world.send(s, set(item_set_op::CLEAR, 3, 0)).await?;
    let (flag, items) = content(&mut w, 3).await?;
    anyhow::ensure!(flag == 0 && items.is_empty(), "emplacement vidé : {flag}, « {items} »");
    println!("  ✓ op 2 : emplacement 3 vidé");

    for iid in [sword, armor] {
        w.world.send(it, Fields::new(it).with(0, 21).with(2, iid)).await?;
        w.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(iid), 3000, "objet détruit").await?;
    }
    wipe().await?;
    println!("\ncommutateur d'équipement : OK");
    Ok(())
}
