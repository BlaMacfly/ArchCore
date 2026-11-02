//! `archcore sim petbag` : inventaire Cash (statut 51) et sac du familier (statut 13, `world/pet_bag.rs`).
//!
//! Un Tullan Passport (TID 1830, objet Cash) enregistré dans l'inventaire ordinaire avant l'inventaire Cash passe au
//! statut 51 à l'entrée en jeu (en base 6). GmTest reçoit Doogie <Rank 1> (TID 7564, objet Cash 6) : op 0 au statut 51, sans case, mini Cash « en cours
//! d'utilisation » à 0. Une Short Sword (TID 11) reçue dans l'inventaire ne peut pas aller dans le sac tant que le
//! familier n'est pas sorti (renvoyée au statut 2). Doogie utilisé : il sort (AgpmSummons op 0, type 8) et l'objet passe
//! à 1 (AGPMITEM op 26). L'épée, posée sur l'onglet du deuxième sac (ligne et colonne à -1), y prend la première case
//! (op 2, statut 13, [1, 0, 0]). Rappel par l'op 23 : le familier rentre (op 1), l'objet repasse à 0 ; l'épée reste
//! au statut 13 (en base, statut 7) et ne peut plus en sortir. « Auto Pick Up » coupé (op 30) : renvoyé et enregistré.
//! Nouvelle entrée en jeu : Doogie revient au statut 51, l'épée n'est pas envoyée ; Doogie de nouveau utilisé, le sac
//! part (op 0 statut 13 en [1, 0, 0]) ; l'épée revient dans l'inventaire ; rappel en réutilisant l'objet (op 3).

use super::{character_id, enter_world, step, InWorld, Recv};
use archcore_net::schema::{self, chat_type, mini};
use archcore_net::{build_mini, decode_mini, Fields};

const DOOGIE_ITEM: i64 = 7564;
const DOOGIE_TID: i64 = 1362;
const SWORD: i64 = 11;
const TULLAN: i64 = 1830;
const CASH: i64 = 51;
const SUB: i64 = 13;

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

/// Octet « en cours d'utilisation » du mini Cash (champ 23).
fn in_use(r: &Recv) -> Option<i64> {
    decode_mini(&mini::ITEM_CASH, r.fields().bytes(23)?).ok()?.int(0)
}

/// Objet reçu par /additem (op 0 de ce TID) : le paquet.
async fn add_item(w: &mut InWorld, tid: i64) -> anyhow::Result<Recv> {
    let it = &schema::ITEM;
    w.world.send(&schema::CHATTING, say(w, &format!("/additem {tid} 1"))).await?;
    w.world.expect_where(|q| q.is(it, Some(0)) && q.int(3) == Some(tid), 3000, "objet reçu").await
}

/// Mini Cash d'un objet (op 26) : l'octet « en cours d'utilisation ».
async fn use_time(w: &mut InWorld, iid: i64, what: &str) -> anyhow::Result<i64> {
    let it = &schema::ITEM;
    let q = w.world.expect_where(|q| q.is(it, Some(26)) && q.int(2) == Some(iid), 3000, what).await?;
    in_use(&q).ok_or_else(|| anyhow::anyhow!("op 26 sans mini Cash"))
}

/// Statut enregistré d'un objet.
async fn stored(db: &crate::db::Databases, iid: i64) -> anyhow::Result<i64> {
    let row = db.characters.first("SELECT status FROM character_items WHERE id = ?", (iid,)).await?;
    Ok(row.map(|r| crate::db::col::<i64>(&r, "status")).unwrap_or(-1))
}

pub async fn run(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let db = crate::db::Databases::new(&cfg.database);
    let (it, su) = (&schema::ITEM, &schema::SUMMONS);
    let id = character_id(&db, char_name).await?;
    // Restes d'un essai précédent : familiers et contenu du sac.
    db.characters.exec("DELETE FROM character_items WHERE character_id = ? AND (template_id IN (?, ?) OR status = 7)", (id, DOOGIE_ITEM, TULLAN)).await?;
    // Objet Cash d'avant l'inventaire Cash : dans la dernière case de l'inventaire ordinaire.
    let old = db.characters.exec(crate::world::items::INSERT_ITEM, (id, TULLAN, 0, 3, 3, 3, 1)).await? as i64;

    step(&format!("{char_name} entre en jeu : l'objet Cash ancien passe dans l'inventaire Cash"));
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let me = w.cid as i64;
    anyhow::ensure!(w.items.iter().any(|i| i.0 == old && i.2 == CASH), "Tullan Passport pas au statut 51 : {:?}", w.items.iter().find(|i| i.0 == old));
    anyhow::ensure!(stored(&db, old).await? == 6, "Tullan Passport enregistré au statut {}", stored(&db, old).await?);
    println!("  ✓ Tullan Passport {old} : statut 51 à l'entrée, en base 6 (case [3, 3, 3] libérée)");

    step("Doogie et une épée reçus");
    let pet_item = add_item(&mut w, DOOGIE_ITEM).await?;
    let pet = pet_item.int(2).unwrap_or(0);
    anyhow::ensure!(pet_item.int(1) == Some(CASH), "Doogie au statut {:?}", pet_item.int(1));
    anyhow::ensure!(pet_item.fields().bytes(7).is_none(), "case envoyée avec un objet Cash");
    anyhow::ensure!(in_use(&pet_item) == Some(0), "mini Cash : {:?}", in_use(&pet_item));
    anyhow::ensure!(stored(&db, pet).await? == 6, "Doogie enregistré au statut {}", stored(&db, pet).await?);
    let sword_item = add_item(&mut w, SWORD).await?;
    let sword = sword_item.int(2).unwrap_or(0);
    anyhow::ensure!(sword_item.int(1) == Some(2), "épée au statut {:?}", sword_item.int(1));
    println!("  ✓ Doogie {pet} au statut 51 (sans case, mini Cash à 0, en base 6) ; épée {sword} au statut 2");

    step("sac indisponible sans familier : dépôt refusé");
    w.world.send(it, Fields::new(it).with(0, 2).with(1, SUB).with(2, sword).with(4, me).with(7, grid([0, 0, 0]))).await?;
    let back = w.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(sword), 3000, "épée renvoyée").await?;
    anyhow::ensure!(back.int(1) == Some(2), "épée au statut {:?}", back.int(1));
    println!("  ✓ épée renvoyée dans l'inventaire");

    step("Doogie utilisé : il sort, l'objet passe « en cours d'utilisation »");
    w.world.send(it, Fields::new(it).with(0, 3).with(2, pet)).await?;
    let out = w.world.expect_where(|q| q.is(su, Some(0)) && q.int(3) == Some(DOOGIE_TID), 3000, "familier sorti").await?;
    let pet_cid = out.int(2).unwrap_or(0);
    anyhow::ensure!(out.int(4) == Some(8), "type {:?}", out.int(4));
    anyhow::ensure!(use_time(&mut w, pet, "op 26 à la sortie").await? == 1, "mini Cash pas à 1");
    println!("  ✓ familier {pet_cid:#x} (type 8), op 26 : en cours d'utilisation");

    step("dépôt sur l'onglet du deuxième sac");
    w.world.send(it, Fields::new(it).with(0, 2).with(1, SUB).with(2, sword).with(4, me).with(7, grid([1, -1, -1]))).await?;
    let moved = w.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(sword), 3000, "épée dans le sac").await?;
    anyhow::ensure!(moved.int(1) == Some(SUB) && grid_of(&moved) == Some([1, 0, 0]), "épée au statut {:?} en {:?}", moved.int(1), grid_of(&moved));
    anyhow::ensure!(stored(&db, sword).await? == 7, "épée enregistrée au statut {}", stored(&db, sword).await?);
    println!("  ✓ op 2 statut 13 en [1, 0, 0], en base statut 7");

    step("rappel (op 23) : l'épée reste dans le sac fermé");
    w.world.send(it, Fields::new(it).with(0, 23).with(2, pet).with(4, me)).await?;
    w.world.expect_where(|q| q.is(su, Some(1)) && q.int(2) == Some(pet_cid), 3000, "familier rentré").await?;
    anyhow::ensure!(use_time(&mut w, pet, "op 26 au rappel").await? == 0, "mini Cash pas à 0");
    w.world.send(it, Fields::new(it).with(0, 2).with(1, 2).with(2, sword).with(4, me)).await?;
    let kept = w.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(sword), 3000, "retrait refusé").await?;
    anyhow::ensure!(kept.int(1) == Some(SUB) && stored(&db, sword).await? == 7, "épée sortie du sac fermé");
    println!("  ✓ op 1 et op 26 à 0 ; retrait refusé, épée toujours au statut 13 (base 7)");

    step("« Auto Pick Up » coupé (op 30)");
    let m = &mini::ITEM_AUTO_PICK;
    w.world.send(it, Fields::new(it).with(0, 30).with(2, pet).with(4, me).with(20, build_mini(m, &Fields::new(m).with(0, 0)))).await?;
    let echo = w.world.expect_where(|q| q.is(it, Some(30)), 3000, "op 30 renvoyée").await?;
    let value = echo.fields().bytes(20).and_then(|b| decode_mini(m, b).ok()).and_then(|f| f.int(0));
    anyhow::ensure!(echo.int(2) == Some(pet) && value == Some(0), "op 30 : IID {:?}, valeur {value:?}", echo.int(2));
    let row = db.characters.first("SELECT pet_auto_pick FROM characters WHERE id = ?", (id,)).await?;
    anyhow::ensure!(row.map(|r| crate::db::col::<i64>(&r, "pet_auto_pick")) == Some(0), "valeur non enregistrée");
    println!("  ✓ op 30 renvoyée (0), characters.pet_auto_pick = 0");

    step("nouvelle entrée en jeu : Doogie au statut 51, le sac attend le familier");
    drop(w);
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let me = w.cid as i64;
    anyhow::ensure!(w.items.iter().any(|i| i.0 == pet && i.2 == CASH), "Doogie absent de l'inventaire Cash à l'entrée");
    anyhow::ensure!(!w.items.iter().any(|i| i.0 == sword), "épée du sac envoyée sans familier");
    w.world.send(it, Fields::new(it).with(0, 3).with(2, pet)).await?;
    let out = w.world.expect_where(|q| q.is(su, Some(0)) && q.int(3) == Some(DOOGIE_TID), 3000, "familier sorti").await?;
    let pet_cid = out.int(2).unwrap_or(0);
    anyhow::ensure!(use_time(&mut w, pet, "op 26 à la sortie").await? == 1, "mini Cash pas à 1");
    let bag = w.world.expect_where(|q| q.is(it, Some(0)) && q.int(2) == Some(sword), 3000, "sac envoyé").await?;
    anyhow::ensure!(bag.int(1) == Some(SUB) && grid_of(&bag) == Some([1, 0, 0]), "épée au statut {:?} en {:?}", bag.int(1), grid_of(&bag));
    println!("  ✓ Doogie au statut 51, l'épée arrive (op 0 statut 13 en [1, 0, 0]) avec le familier");

    step("retrait vers l'inventaire, puis rappel en réutilisant l'objet");
    w.world.send(it, Fields::new(it).with(0, 2).with(1, 2).with(2, sword).with(4, me)).await?;
    let taken = w.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(sword), 3000, "épée retirée").await?;
    anyhow::ensure!(taken.int(1) == Some(2) && grid_of(&taken).is_some() && stored(&db, sword).await? == 0, "épée au statut {:?}", taken.int(1));
    w.world.send(it, Fields::new(it).with(0, 3).with(2, pet)).await?;
    w.world.expect_where(|q| q.is(su, Some(1)) && q.int(2) == Some(pet_cid), 3000, "familier rentré").await?;
    anyhow::ensure!(use_time(&mut w, pet, "op 26 au rappel").await? == 0, "mini Cash pas à 0");
    println!("  ✓ épée en {:?} (statut 2), familier rentré, op 26 à 0", grid_of(&taken).unwrap_or_default());

    step("nettoyage");
    let m = &mini::ITEM_AUTO_PICK;
    w.world.send(it, Fields::new(it).with(0, 30).with(2, pet).with(4, me).with(20, build_mini(m, &Fields::new(m).with(0, 1)))).await?;
    w.world.expect_where(|q| q.is(it, Some(30)), 3000, "op 30 rétablie").await?;
    for iid in [sword, pet, old] {
        w.world.send(it, Fields::new(it).with(0, 21).with(2, iid)).await?;
        w.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(iid), 3000, "objet détruit").await?;
    }
    println!("  ✓ ramassage automatique rétabli, épée, Doogie et Tullan Passport détruits");
    println!("\nsac du familier : OK");
    Ok(())
}
