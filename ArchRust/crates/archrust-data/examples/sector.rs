//! Analyse pas à pas d'un secteur : `cargo run -p archrust-data --example sector -- <Game> <tuile> <entrée>`
//! ex. `… 2024 D325,389.dff`. Affiche chaque bloc rencontré pour localiser une erreur de lecture.
use archrust_data::magpack::MagPack;
use archrust_data::rw::{Reader, SectorGeometry};
use std::fs;
use std::path::PathBuf;

fn dump(data: &[u8], depth: usize, max_depth: usize) {
    let mut r = Reader::new(data);
    while r.remaining() >= 12 {
        let c = match r.chunk() {
            Ok(c) => c,
            Err(e) => {
                println!(
                    "{}!! {e} à l'offset {} (reste {} o) : {:02x?}",
                    "  ".repeat(depth),
                    r.pos,
                    r.remaining(),
                    &data[r.pos..(r.pos + 24).min(data.len())]
                );
                return;
            }
        };
        println!(
            "{}type 0x{:x} taille {} ver 0x{:x} @{}",
            "  ".repeat(depth),
            c.id,
            c.size,
            c.version,
            c.start - 12
        );
        if depth < max_depth && matches!(c.id, 0x14 | 0x0f | 0x08 | 0x07 | 0x06 | 0x03) {
            dump(&data[c.start..c.end()], depth + 1, max_depth);
        }
        r.skip(&c);
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let game = PathBuf::from(&args[1]);
    let tile: u32 = args[2].parse()?;
    let entry = &args[3];
    let prefix = if entry.starts_with('D') || entry == "all" {
        'b'
    } else {
        'a'
    };
    let path = game.join(format!("world/{prefix}{tile:06}x.ma2"));
    let pack = MagPack::parse(&fs::read(&path)?)?;
    if entry == "all" {
        let (mut ok, mut bad) = (0, Vec::new());
        let mut mats = std::collections::BTreeMap::new();
        for e in &pack.entries {
            match pack
                .unpack(e)
                .map_err(|x| x.to_string())
                .and_then(|p| SectorGeometry::parse(&p).map_err(|x| x.to_string()))
            {
                Ok(s) => {
                    ok += 1;
                    *mats.entry(s.geometry.materials.len()).or_insert(0) += 1;
                }
                Err(x) => bad.push(format!("{} : {x}", e.name)),
            }
        }
        println!(
            "tuile {tile} : {ok}/{} secteurs lus ; nombres de matériaux {mats:?} ; échecs {:?}",
            pack.entries.len(),
            bad.iter().take(5).collect::<Vec<_>>()
        );
        return Ok(());
    }
    let plain = pack
        .unpack_named(entry)
        .ok_or_else(|| anyhow::anyhow!("{entry} absent"))??;
    println!(
        "{entry} : {} o décompressés, u32 de tête {}",
        plain.len(),
        u32::from_le_bytes(plain[..4].try_into()?)
    );
    dump(&plain[4..], 0, 4);
    match SectorGeometry::parse(&plain) {
        Ok(s) => {
            let (lo, hi) = s.bounds();
            println!(
                "OK : {} sommets, {} tri, {} UV, {} mat, bornes {lo:?}..{hi:?}",
                s.geometry.vertices.len(),
                s.geometry.triangles.len(),
                s.geometry.uv_sets.len(),
                s.geometry.materials.len()
            );
        }
        Err(e) => println!("ÉCHEC : {e}"),
    }
    Ok(())
}
