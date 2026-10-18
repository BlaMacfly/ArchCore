use archrust_data::magpack::MagPack;
use archrust_data::rw::SectorGeometry;
use std::{fs, path::PathBuf};

fn main() -> anyhow::Result<()> {
    let game = PathBuf::from(std::env::args().nth(1).expect("dossier Game"));
    let tile = std::env::args().nth(2).unwrap_or_else(|| "002024".into());
    let sector = std::env::args().nth(3).unwrap_or_else(|| "325,389".into());
    let pack = MagPack::parse(&fs::read(game.join(format!("world/b{tile}x.ma2")))?)?;
    if sector == "all" {
        let mut parsed = 0;
        let mut failed = Vec::new();
        for entry in pack
            .entries
            .iter()
            .filter(|entry| entry.name.starts_with('D'))
        {
            match pack.unpack_named(&entry.name).expect("secteur absent")? {
                bytes if SectorGeometry::parse(&bytes).is_ok() => parsed += 1,
                _ => failed.push(entry.name.clone()),
            }
        }
        println!("{parsed}/{} secteurs lisibles", parsed + failed.len());
        if !failed.is_empty() {
            println!("échecs : {failed:?}");
        }
        return Ok(());
    }
    let data = pack
        .unpack_named(&format!("D{sector}.dff"))
        .expect("secteur absent")?;
    println!("{} octets", data.len());
    match SectorGeometry::parse(&data) {
        Ok(s) => println!(
            "{} sommets, {} triangles",
            s.geometry.vertices.len(),
            s.geometry.triangles.len()
        ),
        Err(e) => println!("échec : {e}"),
    }
    Ok(())
}
