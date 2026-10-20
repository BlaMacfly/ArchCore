//! Sonde des jeux d'effets `effect/ini/new/efffile_set.eff` + `efffile_var.eff` : liste les jeux
//! (id, nom, bloc, sons), ou détaille un jeu et affiche son bloc en hexadécimal.
//!
//! Usage : `cargo run -p archrust-data --example eff_probe -- <Game> [id] [--all]`
//! (sans `--all`, seuls les jeux ayant un son sont listés).

use archrust_data::EffectSets;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let game = PathBuf::from(args.first().expect("dossier Game"));
    let want: Option<u32> = args.get(1).and_then(|s| s.parse().ok());
    let all = args.iter().any(|a| a == "--all");
    let set_file = std::fs::read(game.join("effect/ini/new/efffile_set.eff"))?;
    let var_file = std::fs::read(game.join("effect/ini/new/efffile_var.eff"))?;
    let sets = EffectSets::parse(&set_file, &var_file);
    println!("{} jeux, blocs : {} octets", sets.len(), var_file.len());
    for set in sets.iter() {
        if let Some(w) = want {
            if w != set.id {
                continue;
            }
        } else if !all && set.sounds.is_empty() {
            continue;
        }
        println!(
            "[{}] {:?} bloc {}+{} sons {:?}{}",
            set.id,
            set.name,
            set.offset,
            set.len,
            set.sounds,
            if set.scanned { " (balayage)" } else { "" }
        );
        if want.is_some() {
            let block = &var_file[set.offset as usize..(set.offset + set.len) as usize];
            for (k, line) in block.chunks(32).enumerate() {
                let hex: Vec<String> = line.iter().map(|b| format!("{b:02x}")).collect();
                let asc: String = line
                    .iter()
                    .map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' })
                    .collect();
                println!("{:06x} {} {asc}", k * 32, hex.join(" "));
            }
        }
    }
    Ok(())
}
