//! `archcore sim fixedoptions` : options fixes des équipements (« OptionTID » d'itemdatatable.txt).
//!
//! GmTest crée (`/create`) des équipements dont le modèle a des options fixes : la base porte exactement ces options,
//! dans l'ordre. Un équipement sans options fixes garde son tirage. Les objets créés sont ensuite supprimés.

use super::{enter_world, step};
use archcore_net::schema::{self, chat_type};
use archcore_net::Fields;

pub async fn run(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let db = crate::db::Databases::new(&cfg.database);
    let rows = db
        .world
        .rows("SELECT tid, name, JSON_VALUE(raw, '$.OptionTID') AS fixed FROM item_data WHERE part = 2 AND JSON_VALUE(raw, '$.OptionTID') <> '' ORDER BY tid LIMIT 4", ())
        .await?;
    anyhow::ensure!(!rows.is_empty(), "aucun équipement à options fixes dans item_data");
    let id = super::character_id(&db, char_name).await?;
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let ch = &schema::CHATTING;
    let mut created = Vec::new();

    step("équipements à options fixes créés par /create");
    for r in &rows {
        let (tid, name, fixed): (u32, String, String) = (crate::db::col(r, "tid"), crate::db::col(r, "name"), crate::db::col(r, "fixed"));
        let want: Vec<u16> = fixed.split(';').filter_map(|t| t.trim().parse().ok()).filter(|t| *t > 0).take(crate::world::options::MAX_OPTIONS).collect();
        let say = Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(&format!("/create \"{char_name}\" {tid} 1")));
        w.world.send(ch, say).await?;
        let added = w.world.expect_where(|q| q.is(&schema::ITEM, Some(0)) && q.int(3) == Some(tid as i64), 5000, "objet créé").await?;
        let iid = added.int(2).unwrap_or(0);
        created.push(iid);
        let row = db.characters.first("SELECT option_tids FROM character_items WHERE id = ? AND character_id = ?", (iid, id)).await?.ok_or_else(|| anyhow::anyhow!("objet {iid} absent de la base"))?;
        let got = crate::world::options::options_from_column(&crate::db::col::<String>(&row, "option_tids"));
        anyhow::ensure!(got == want, "{name} (TID {tid}) : options {got:?} au lieu de {want:?}");
        println!("  ✓ {name} (TID {tid}) : {got:?}");
    }
    drop(w);
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    for iid in created {
        db.characters.exec("DELETE FROM character_items WHERE id = ? AND character_id = ?", (iid, id)).await?;
    }
    db.close().await;
    println!("\noptions fixes : OK");
    Ok(())
}
