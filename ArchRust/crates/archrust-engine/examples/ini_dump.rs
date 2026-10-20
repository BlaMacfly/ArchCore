//! Déchiffre une table `ini` du client et l'écrit en clair (UTF-8), toutes sections et clés.
//!
//! Usage : `cargo run -p archrust-engine --example ini_dump -- <Game> <table.ini> [dir]`

use archrust_engine::assets::Assets;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let assets = Assets::new(Some(PathBuf::from(&args[0])), &[]);
    let dir = args.get(2).map(String::as_str).unwrap_or("ini");
    let table = assets.table(dir, &args[1])?;
    for section in &table.sections {
        let name = &section.name;
        println!("[{name}]");
        for (k, v) in &section.entries {
            println!("{k}={v}");
        }
    }
    Ok(())
}
