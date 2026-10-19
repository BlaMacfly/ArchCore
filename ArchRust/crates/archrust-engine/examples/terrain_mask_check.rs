//! Vérifie l'orientation des masques de fondu du sol sur un secteur réel.
//!
//! Pour chaque cellule à une couche (masque à l'étage 1, couche à l'étage 2) bordée d'une cellule
//! sans couche, l'alpha du masque sur le bord commun doit valoir 1 si la voisine porte la texture
//! de la couche, 0 si elle porte la même base. On mesure l'écart moyen pour quatre lectures de la
//! case d'atlas : telle quelle, retournée en U, en V, ou les deux.
//!
//! Usage : `cargo run -p archrust-engine --example terrain_mask_check -- <Game> <sx> <sy>`

use archrust_data::rw::SectorGeometry;
use archrust_engine::assets::Assets;
use archrust_engine::world::{stage_textures, tile_of};
use std::collections::HashMap;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let assets = Assets::new(Some(PathBuf::from(&args[0])), &[]);
    let (sx, sy): (i32, i32) = (args[1].parse()?, args[2].parse()?);
    let b = assets.magpack('b', tile_of(sx, sy), "ma2")?;
    let plain = b
        .unpack_named(&format!("D{sx},{sy}.dff"))
        .ok_or_else(|| anyhow::anyhow!("secteur absent"))??;
    let s = SectorGeometry::parse(&plain)?;
    let g = &s.geometry;
    let (lo, _) = s.bounds();
    let mask = assets.texture("a0001c0t")?;
    let (amin, amax) = mask
        .data
        .chunks(4)
        .fold((255u8, 0u8), |(a, b), p| (a.min(p[3]), b.max(p[3])));
    let (rmin, rmax) = mask
        .data
        .chunks(4)
        .fold((255u8, 0u8), |(a, b), p| (a.min(p[0]), b.max(p[0])));
    println!(
        "masque {}×{} alpha {amin}..{amax} rouge {rmin}..{rmax}",
        mask.width, mask.height
    );
    let alpha = |u: f32, v: f32| -> f32 {
        let x = ((u.rem_euclid(1.0)) * mask.width as f32).min(mask.width as f32 - 1.0) as usize;
        let y = ((v.rem_euclid(1.0)) * mask.height as f32).min(mask.height as f32 - 1.0) as usize;
        mask.data[(y * mask.width as usize + x) * 4 + 3] as f32 / 255.0
    };
    // Cellules : (i, j) → (matériau, sommets).
    let mut cells: HashMap<(i32, i32), (usize, Vec<usize>)> = HashMap::new();
    for pair in g.triangles.chunks(2) {
        let vs: Vec<usize> = pair.iter().flat_map(|t| t.v.map(|i| i as usize)).collect();
        let minx = vs.iter().map(|&i| g.vertices[i].x).fold(f32::MAX, f32::min);
        let minz = vs.iter().map(|&i| g.vertices[i].z).fold(f32::MAX, f32::min);
        let key = (
            ((minx - lo.x) / 400.0).round() as i32,
            ((minz - lo.z) / 400.0).round() as i32,
        );
        cells.insert(key, (pair[0].material as usize, vs));
    }
    let stages = |m: usize| stage_textures(&g.materials[m]);
    let mut err = [0f32; 4];
    let mut count = 0;
    for (&(i, j), (m, vs)) in &cells {
        let st = stages(*m);
        if st[1].is_none() || st[3].is_some() {
            continue;
        }
        let uv1 = &g.uv_sets[1];
        let cu = vs.iter().map(|&k| uv1[k][0]).sum::<f32>() / vs.len() as f32;
        let cv = vs.iter().map(|&k| uv1[k][1]).sum::<f32>() / vs.len() as f32;
        let (ou, ov) = ((cu * 4.0).floor() / 4.0, (cv * 4.0).floor() / 4.0);
        for (di, dj) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let Some((nm, _)) = cells.get(&(i + di, j + dj)) else {
                continue;
            };
            let ns = stages(*nm);
            if ns[1].is_some() {
                continue;
            }
            let expected = if ns[0] == st[2] {
                1.0
            } else if ns[0] == st[0] {
                0.0
            } else {
                continue;
            };
            // Sommets du bord commun : position extrême dans la direction.
            let key = |k: usize| {
                let p = g.vertices[k];
                (p.x - lo.x) * di as f32 + (p.z - lo.z) * dj as f32
            };
            let best = vs.iter().map(|&k| key(k)).fold(f32::MIN, f32::max);
            let edge: Vec<usize> = vs
                .iter()
                .copied()
                .filter(|&k| (key(k) - best).abs() < 1.0)
                .collect();
            for (h, e) in err.iter_mut().enumerate() {
                let mut sum = 0.0;
                for &k in &edge {
                    let (mut u, mut v) = (uv1[k][0], uv1[k][1]);
                    // Un peu vers l'intérieur de la case, pour ne pas lire la voisine de l'atlas.
                    u = u + (cu - u) * 0.15;
                    v = v + (cv - v) * 0.15;
                    if h & 1 != 0 {
                        u = ou + 0.25 - (u - ou);
                    }
                    if h & 2 != 0 {
                        v = ov + 0.25 - (v - ov);
                    }
                    sum += alpha(u, v);
                }
                *e += (sum / edge.len() as f32 - expected).abs();
            }
            count += 1;
        }
    }
    println!("{count} bords comparés");
    for (h, name) in ["tel quel", "U retourné", "V retourné", "U et V retournés"]
        .iter()
        .enumerate()
    {
        println!(
            "  {name:<18} écart moyen {:.3}",
            err[h] / count.max(1) as f32
        );
    }
    Ok(())
}
