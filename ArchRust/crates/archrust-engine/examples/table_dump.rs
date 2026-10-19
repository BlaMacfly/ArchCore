//! Affiche une section complète d'une table `ini` du client, colonnes nommées par l'en-tête `[]`
//! quand la table est positionnelle.
//!
//! Usage : `cargo run -p archrust-engine --example table_dump -- <Game> <table.ini> <section>`

use archrust_engine::assets::Assets;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let assets = Assets::new(Some(PathBuf::from(&args[0])), &[]);
    let table = assets.table("ini", &args[1])?;
    let header = table.section("");
    let section = table
        .section(&args[2])
        .ok_or_else(|| anyhow::anyhow!("section [{}] absente", args[2]))?;
    for (k, v) in &section.entries {
        let name = header.and_then(|h| h.get(k)).unwrap_or("");
        println!("{k:>4} {name:<28} {v}");
    }
    Ok(())
}
