//! Relevé des tables d'animation du client Archonia.

use archrust_data::ini::IniTable;
use std::{fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let game = PathBuf::from(std::env::args_os().nth(1).expect("dossier Game"));
    for name in [
        "charactertemplateanimation.ini",
        "charactertemplateclient.ini",
        "charactertemplate.ini",
    ] {
        let table = IniTable::from_encrypted(&fs::read(game.join("ini").join(name))?);
        println!(
            "{name}: {} sections, premiers noms={:?}",
            table.len(),
            table
                .sections
                .iter()
                .take(8)
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>()
        );
        for section in table
            .sections
            .iter()
            .filter(|s| {
                s.name == "1" || s.name == "96" || s.name == "0000001" || s.name == "0000096"
            })
            .take(4)
        {
            println!("  [{}]", section.name);
            for (key, value) in &section.entries {
                let upper = key.to_ascii_uppercase();
                if upper.starts_with("ANIMATION_NAME")
                    || upper.starts_with("B_ANIMATION_NAME")
                    || upper.starts_with("SUB_ANIMATION_NAME")
                    || [
                        "DFF",
                        "DEFAULT_ARMOUR_DFF",
                        "ANIMATION_TYPE2",
                        "RACE",
                        "GENDER",
                        "CLASS",
                    ]
                    .iter()
                    .any(|k| key.eq_ignore_ascii_case(k))
                {
                    println!("    {key}={value}");
                }
            }
        }
    }
    Ok(())
}
