//! Hauteur du terrain du client sous des positions du monde (diagnostic côté serveur).
//!
//! Usage : `cargo run -p archrust-engine --example spawn_heights -- <Game> <x> <z> [<x> <z> …]`

use archrust_data::rw::SectorGeometry;
use archrust_engine::assets::Assets;
use archrust_engine::world::{sector_of, tile_of, HeightField};
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let assets = Assets::new(Some(PathBuf::from(&args[0])), &[]);
    for pair in args[1..].chunks(2) {
        let (x, z): (f32, f32) = (pair[0].parse()?, pair[1].parse()?);
        let (sx, sy) = sector_of(x, z);
        let b = assets.magpack('b', tile_of(sx, sy), "ma2")?;
        let Some(plain) = b.unpack_named(&format!("D{sx},{sy}.dff")) else {
            println!("{x:.0} {z:.0} : secteur ({sx}, {sy}) absent");
            continue;
        };
        let s = SectorGeometry::parse(&plain?)?;
        let hf = HeightField::from_vertices(sx, sy, &s.geometry.vertices);
        println!("{x:.0} {z:.0} : secteur ({sx}, {sy}), hauteur {:.0}", hf.height_at(x, z));
    }
    Ok(())
}
