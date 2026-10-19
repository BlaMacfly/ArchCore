//! Sonde d'un secteur de terrain du client : matériaux, textures, plages des jeux d'UV.
//!
//! Usage : `cargo run -p archrust-engine --example sector_probe -- <Game> <sx> <sy> [secteur.bin]`

use archrust_data::rw::SectorGeometry;
use archrust_engine::assets::Assets;
use archrust_engine::world::tile_of;
use std::collections::HashMap;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let assets = Assets::new(Some(PathBuf::from(&args[0])), &[]);
    let (sx, sy): (i32, i32) = (args[1].parse()?, args[2].parse()?);
    let b = assets.magpack('b', tile_of(sx, sy), "ma2")?;
    let name = format!("D{sx},{sy}.dff");
    let plain = b
        .unpack_named(&name)
        .ok_or_else(|| anyhow::anyhow!("{name} absent"))??;
    if let Some(out) = args.get(3) {
        std::fs::write(out, &plain)?;
    }
    let s = SectorGeometry::parse(&plain)?;
    let g = &s.geometry;
    println!(
        "{name} : atomic {:?}, drapeaux {:#x}, {} sommets, {} triangles, {} bandes={}",
        s.atomic_struct,
        g.flags,
        g.vertices.len(),
        g.triangles.len(),
        g.meshes.len(),
        g.tristrip
    );
    let (lo, hi) = s.bounds();
    println!("  bornes {lo:?} .. {hi:?}");
    for (i, set) in g.uv_sets.iter().enumerate() {
        let (mut a, mut z) = ([f32::MAX; 2], [f32::MIN; 2]);
        for uv in set {
            for k in 0..2 {
                a[k] = a[k].min(uv[k]);
                z[k] = z[k].max(uv[k]);
            }
        }
        println!(
            "  UV{i} : u {:.3}..{:.3}  v {:.3}..{:.3}  (ex. {:?} {:?})",
            a[0],
            z[0],
            a[1],
            z[1],
            set.first(),
            set.get(1)
        );
    }
    let mut per_mat: HashMap<u16, usize> = HashMap::new();
    for t in &g.triangles {
        *per_mat.entry(t.material).or_default() += 1;
    }
    for (i, m) in g.materials.iter().enumerate() {
        let layers: Vec<String> = m
            .layers
            .iter()
            .map(|l| {
                format!(
                    "{}:{}",
                    l.kind,
                    l.texture.as_ref().map_or("-", |t| t.name.as_str())
                )
            })
            .collect();
        println!(
            "  mat {i:>2} : couleur {:?} texture {:?} couches {:?} — {} triangles",
            m.color,
            m.texture.as_ref().map(|t| &t.name),
            layers,
            per_mat.get(&(i as u16)).unwrap_or(&0)
        );
    }
    // Jeu d'UV proportionnel à la position dans le secteur (u = x relatif / 6400, v = z relatif / 6400).
    for (i, set) in g.uv_sets.iter().enumerate() {
        let mut err_u = 0f32;
        let mut err_v = 0f32;
        for (p, uv) in g.vertices.iter().zip(set) {
            err_u = err_u.max((uv[0] - (p.x - lo.x) / 6400.0).abs());
            err_v = err_v.max((uv[1] - (p.z - lo.z) / 6400.0).abs());
        }
        println!("  UV{i} écart à (x, z)/6400 : {err_u:.4} {err_v:.4}");
    }
    // Premiers sommets : position relative et UV de chaque jeu.
    for v in 0..8 {
        let uvs: Vec<_> = g.uv_sets.iter().map(|s| s[v]).collect();
        println!("  sommet {v} : {:?} uv {:?}", g.vertices[v] - lo, uvs);
    }
    // Par cellule (2 triangles) : matériau et coin min / étendue de chaque jeu d'UV.
    for (c, pair) in g.triangles.chunks(2).enumerate() {
        let vs: Vec<usize> = pair
            .iter()
            .flat_map(|t| t.v.iter().map(|&i| i as usize))
            .collect();
        let sets: Vec<String> = g
            .uv_sets
            .iter()
            .map(|set| {
                let (mut a, mut z) = ([f32::MAX; 2], [f32::MIN; 2]);
                for &i in &vs {
                    for k in 0..2 {
                        a[k] = a[k].min(set[i][k]);
                        z[k] = z[k].max(set[i][k]);
                    }
                }
                format!("({:.2},{:.2})+{:.2}", a[0], a[1], z[0] - a[0])
            })
            .collect();
        println!(
            "  cellule {c:>3} mat {:>2} : {}",
            pair[0].material,
            sets.join("  ")
        );
    }
    Ok(())
}
