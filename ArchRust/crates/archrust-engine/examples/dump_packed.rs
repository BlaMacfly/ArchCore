//! Écrit une entrée brute d'un pack du client sur disque (PNG d'interface, ini, sons…).
//!
//! ```text
//! cargo run -p archrust-engine --example dump_packed -- <Game> <pack> <entrée> <sortie>
//! cargo run -p archrust-engine --example dump_packed -- assets-archonia/Game texture/ui lg_title_1600.png /tmp/login.png
//! ```

use archrust_engine::Assets;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [game, pack, entry, out] = args.as_slice() else {
        anyhow::bail!("usage : dump_packed <Game> <pack> <entrée> <sortie>");
    };
    let assets = Assets::new(Some(PathBuf::from(game)), &[]);
    let bytes = assets.read_packed(pack, entry)?;
    std::fs::write(out, &bytes)?;
    println!("{pack}/{entry} → {out} ({} octets)", bytes.len());
    Ok(())
}
