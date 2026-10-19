//! Vérifie le sens de rotation des objets du monde sur les barrières d'une tuile.
//!
//! Les segments de barrière se suivent : avec la bonne convention d'angle, les extrémités de
//! segments voisins se touchent. Pour `DegreeY` appliqué tel quel puis opposé, on compte les
//! extrémités à moins de 40 unités de celle d'un autre segment (tous modèles de la catégorie).
//!
//! Usage : `cargo run -p archrust-engine --example object_rotation_check -- <Game> <tuile> <catégorie>`

use archrust_engine::assets::Assets;
use archrust_engine::objects::{root_inverse, template_model, tile_placements};
use glam::Vec3;
use std::collections::HashMap;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let assets = Assets::new(Some(PathBuf::from(&args[0])), &[]);
    let tile: u32 = args[1].parse()?;
    let category = &args[2];
    let templates = assets.table("ini", "objecttemplate.ini")?;
    // Extrémités locales (axe horizontal le plus long de la boîte) par TID.
    let mut ends: HashMap<u32, Option<[Vec3; 2]>> = HashMap::new();
    let mut segments = Vec::new();
    for p in tile_placements(&assets, tile) {
        let e = ends.entry(p.tid).or_insert_with(|| {
            let t = templates.section(&p.tid.to_string())?;
            if t.get("4") != Some(category.as_str()) {
                return None;
            }
            let file = template_model(&assets, p.tid)?;
            let clump = assets.clump("object", &file).ok()?;
            let root = root_inverse(&assets, &file);
            let world = clump.world_matrices();
            let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
            for a in &clump.atomics {
                let m = root * world[a.frame as usize];
                for v in &clump.geometries[a.geometry as usize].vertices {
                    let q = m.transform_point3(*v);
                    lo = lo.min(q);
                    hi = hi.max(q);
                }
            }
            let mid = (lo + hi) / 2.0;
            Some(if hi.x - lo.x >= hi.z - lo.z {
                [Vec3::new(lo.x, 0.0, mid.z), Vec3::new(hi.x, 0.0, mid.z)]
            } else {
                [Vec3::new(mid.x, 0.0, lo.z), Vec3::new(mid.x, 0.0, hi.z)]
            })
        });
        if let Some(e) = *e {
            segments.push((p, e));
        }
    }
    println!("{} segments de « {category} »", segments.len());
    for (label, sign) in [("DegreeY tel quel", 1.0f32), ("DegreeY opposé", -1.0)] {
        let pts: Vec<[Vec3; 2]> = segments
            .iter()
            .map(|(p, e)| {
                let mut q = p.clone();
                q.degree_y *= sign;
                let m = q.matrix();
                e.map(|v| {
                    let w = m.transform_point3(v);
                    Vec3::new(w.x, 0.0, w.z)
                })
            })
            .collect();
        let mut joined = 0;
        for (i, a) in pts.iter().enumerate() {
            for e in a {
                let best = pts
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| *j != i)
                    .flat_map(|(_, b)| b.iter())
                    .map(|f| f.distance(*e))
                    .fold(f32::MAX, f32::min);
                if best < 40.0 {
                    joined += 1;
                }
            }
        }
        println!(
            "  {label:<18} extrémités jointes {joined}/{}",
            pts.len() * 2
        );
    }
    Ok(())
}
