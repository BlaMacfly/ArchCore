//! Amplitude de mouvement d'animations `.ean` du client (angle maximal parcouru par piste).
//! Usage : `cargo run -p archrust-data --example ean_motion -- <Game> a0060000.ean a0060200.ean …`
use archrust_data::{ean::EanFile, pack::PackIndex};
use glam::Quat;
use std::{fs, fs::File, path::Path};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let d = Path::new(&a[0]).join("character/animation");
    let index = PackIndex::parse_encrypted(&fs::read(d.join("Reference.Dat"))?)?;
    let mut data = File::open(d.join("Data.Dat"))?;
    for name in &a[1..] {
        let Some(e) = index.find(name) else {
            println!("{name} : absent");
            continue;
        };
        let ean = EanFile::parse(&index.read_entry(&mut data, e)?)?;
        let chunk = ean
            .chunks
            .iter()
            .find(|c| c.id == 0x1b)
            .ok_or("pas de 0x1b")?;
        let dur = f32::from_bits(chunk.animation_layout().unwrap().prefix[4]);
        let steps = 24;
        let poses: Vec<_> = (0..=steps)
            .map(|i| {
                chunk
                    .sample_linked_tracks(dur * i as f32 / steps as f32)
                    .unwrap()
            })
            .collect();
        let n = poses[0].len();
        let mut angles: Vec<f32> = (0..n)
            .map(|t| {
                let q0 = Quat::from_array(poses[0][t].rotation);
                poses
                    .iter()
                    .map(|p| {
                        q0.angle_between(Quat::from_array(p[t].rotation))
                            .to_degrees()
                    })
                    .fold(0f32, f32::max)
            })
            .collect();
        let root_move: f32 = poses
            .iter()
            .map(|p| {
                (glam::Vec3::from_array(p[0].translation)
                    - glam::Vec3::from_array(poses[0][0].translation))
                .length()
            })
            .fold(0f32, f32::max);
        angles.sort_by(|x, y| y.total_cmp(x));
        let mean = angles.iter().sum::<f32>() / n as f32;
        println!("{name} : durée {dur:.2} s, {n} pistes, angle max {:.0}°, moyen {mean:.1}°, 5 plus grands {:?}, racine déplacée {root_move:.1}", angles[0], &angles[..5].iter().map(|v| v.round()).collect::<Vec<_>>());
    }
    Ok(())
}
