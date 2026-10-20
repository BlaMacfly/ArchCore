//! Déchiffre un fichier texte du client (`ini/npcdialog.txt`, `ini_fr/questtemplate.ini`…) et l'écrit
//! tel quel en UTF-8, sans l'analyser en table.
//!
//! Usage : `cargo run -p archrust-engine --example txt_dump -- <Game> <chemin relatif>`

use archrust_engine::assets::Assets;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let assets = Assets::new(Some(PathBuf::from(&args[0])), &[]);
    let raw = assets.read_file(&args[1])?;
    print!("{}", archrust_data::ini::decrypt_ini(&raw));
    Ok(())
}
