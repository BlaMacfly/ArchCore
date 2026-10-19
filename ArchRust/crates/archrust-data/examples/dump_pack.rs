//! Vide un pack du client (`Reference.Dat` + `Data.Dat`) dans un dossier, une entrée = un fichier.
//!
//! ```text
//! cargo run -p archrust-data --example dump_pack -- <dossier du pack> <dossier de sortie>
//! cargo run -p archrust-data --example dump_pack -- assets-archonia/Game/texture/ui /tmp/ui
//! ```
//!
//! Les entrées sont écrites telles quelles (PNG, TIF, BMP…), sans déchiffrement supplémentaire.

use archrust_data::pack::PackIndex;
use std::fs::File;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [pack_dir, out_dir] = args.as_slice() else {
        anyhow::bail!("usage : dump_pack <dossier du pack> <dossier de sortie>");
    };
    let pack_dir = PathBuf::from(pack_dir);
    let find = |name: &str| std::fs::read_dir(&pack_dir).ok()?.flatten().map(|e| e.path()).find(|p| p.file_name().map(|f| f.to_string_lossy().eq_ignore_ascii_case(name)).unwrap_or(false));
    let ref_path = find("Reference.Dat").ok_or_else(|| anyhow::anyhow!("Reference.Dat absent de {}", pack_dir.display()))?;
    let data_path = find("Data.Dat").ok_or_else(|| anyhow::anyhow!("Data.Dat absent de {}", pack_dir.display()))?;
    let index = PackIndex::parse_encrypted(&std::fs::read(&ref_path)?)?;
    let mut data = File::open(&data_path)?;
    let out = PathBuf::from(out_dir);
    std::fs::create_dir_all(&out)?;
    let (mut ok, mut failed) = (0usize, 0usize);
    for entry in &index.entries {
        let name = entry.name.replace(['\\', '/'], "_");
        match index.read_entry(&mut data, entry) {
            Ok(bytes) => {
                std::fs::write(out.join(&name), bytes)?;
                ok += 1;
            }
            Err(e) => {
                eprintln!("{} : {e}", entry.name);
                failed += 1;
            }
        }
    }
    println!("{} : {ok} entrée(s) écrite(s) dans {}, {failed} en échec", pack_dir.display(), out.display());
    Ok(())
}
