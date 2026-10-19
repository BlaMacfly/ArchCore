//! Sonde des têtes par défaut (visages, cheveux) sur les données réelles du client Archonia.
//!
//! * `ini <Game> <fichier.ini> <section>` : section complète, colonnes nommées par l'en-tête `[]` ;
//! * `pack <Game> <dossier> [filtre]` : entrées d'un pack ;
//! * `model <Game> <dossier> <entrée>` : frames, HAnim, géométries et skin d'un clump ;
//! * `raw <Game> <dossier> <entrée> <sortie>` : entrée brute (déchiffrée par le pack).
//!
//! Usage : `cargo run -p archrust-data --example head_probe -- ini <Game> charactertemplatecustomize.ini 96`

use archrust_data::{ini::decrypt_ini, pack::PackIndex, rw::Clump};
use std::{fs, fs::File, path::Path};

type R<T> = Result<T, Box<dyn std::error::Error>>;

fn read(game: &Path, dir: &str, name: &str) -> R<Vec<u8>> {
    let d = game.join(dir);
    let index = PackIndex::parse_encrypted(&fs::read(d.join("Reference.Dat"))?)?;
    let entry = index
        .find(name)
        .ok_or_else(|| format!("{name} absent de {dir}"))?;
    let mut data = File::open(d.join("Data.Dat"))?;
    Ok(index.read_entry(&mut data, entry)?)
}

fn main() -> R<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let game = Path::new(&args[1]);
    match args[0].as_str() {
        "ini" => {
            let text = decrypt_ini(&fs::read(game.join("ini").join(&args[2]))?);
            let table = archrust_data::ini::IniTable::parse(&text);
            let header = table.section("");
            let sec = table.section(&args[3]).ok_or("section absente")?;
            for (k, v) in &sec.entries {
                let name = header.and_then(|h| h.get(k)).unwrap_or("?");
                println!("{k:>4} {name:<32} {v}");
            }
        }
        "pack" => {
            let d = game.join(&args[2]);
            let index = PackIndex::parse_encrypted(&fs::read(d.join("Reference.Dat"))?)?;
            let filter = args.get(3).map(|s| s.to_ascii_lowercase());
            for e in &index.entries {
                if filter
                    .as_ref()
                    .is_none_or(|f| e.name.to_ascii_lowercase().contains(f))
                {
                    println!("{}", e.name);
                }
            }
        }
        "raw" => {
            let bytes = read(game, &args[2], &args[3])?;
            fs::write(&args[4], &bytes)?;
            println!("{} octets", bytes.len());
        }
        "model" => {
            let clump = Clump::parse(&read(game, &args[2], &args[3])?)?;
            println!(
                "{} frames, {} géométries, {} atomics",
                clump.frames.len(),
                clump.geometries.len(),
                clump.atomics.len()
            );
            for (i, f) in clump.frames.iter().enumerate() {
                let h = f
                    .hanim
                    .as_ref()
                    .map(|h| format!("hanim id={} nodes={}", h.node_id, h.nodes.len()))
                    .unwrap_or_default();
                println!(
                    "  frame {i} parent={} pos={:?} name={} {h}",
                    f.parent, f.position, f.name
                );
            }
            for (i, g) in clump.geometries.iter().enumerate() {
                println!(
                    "  géométrie {i} : {} sommets, skin={}",
                    g.vertices.len(),
                    g.skin
                        .as_ref()
                        .map(|s| s.bone_count.to_string())
                        .unwrap_or("-".into())
                );
            }
            for a in &clump.atomics {
                println!("  atomic frame={} géométrie={}", a.frame, a.geometry);
            }
        }
        _ => return Err("commande inconnue".into()),
    }
    Ok(())
}
