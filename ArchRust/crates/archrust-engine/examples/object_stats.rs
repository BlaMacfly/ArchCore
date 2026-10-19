//! Statistiques des objets d'une tuile : catégorie, drapeaux `ObjectType` d'AgcmObject, modèle.
//!
//! Usage : `cargo run -p archrust-engine --example object_stats -- <Game> <tuile>`

use archrust_engine::assets::Assets;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let assets = Assets::new(Some(PathBuf::from(&args[0])), &[]);
    let tile: u32 = args[1].parse()?;
    let objects = assets.table("ini", &format!("obj{tile:05}.ini"))?;
    let templates = assets.table("ini", "objecttemplate.ini")?;
    let mut stats: BTreeMap<(String, String), (usize, String)> = BTreeMap::new();
    for o in &objects.sections {
        let Some(tid) = o.get("TID") else { continue };
        let Some(t) = templates.section(tid) else {
            continue;
        };
        // Colonnes : 4 = Category, la seconde clé 2 = ObjectType d'AgcmObject, 10 = DFF.
        let category = t.get("4").unwrap_or("?").to_string();
        let agcm = t.get_all("2").nth(1).unwrap_or("?");
        let agcm = agcm
            .parse::<u32>()
            .map(|v| format!("{v:#08x}"))
            .unwrap_or(agcm.into());
        let e = stats.entry((category, agcm)).or_insert((0, String::new()));
        e.0 += 1;
        if e.1.is_empty() {
            e.1 = format!(
                "TID {tid} {} {}",
                t.get("1").unwrap_or(""),
                t.get("10").unwrap_or("-")
            );
        }
    }
    for ((cat, agcm), (n, ex)) in stats {
        println!("{n:>5}  {agcm:<10} {cat:<14} ex. {ex}");
    }
    Ok(())
}
