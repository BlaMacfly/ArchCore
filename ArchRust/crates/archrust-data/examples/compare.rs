//! Compare deux images (ex. une texture « fan » et l'originale du client) pour estimer si l'une
//! dérive de l'autre : les deux sont réduites à 64 × 64 en niveaux de gris, puis on calcule la
//! corrélation de Pearson, la corrélation des contours (gradients) et la distance de teinte.
//! `cargo run -p archrust-data --example compare -- <a.png> <b.png>`
use std::path::Path;

fn load(path: &Path) -> anyhow::Result<(Vec<f32>, Vec<[f32; 3]>)> {
    let img = image::open(path)?
        .resize_exact(64, 64, image::imageops::FilterType::Triangle)
        .to_rgb8();
    let gray: Vec<f32> = img
        .pixels()
        .map(|p| 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32)
        .collect();
    let rgb: Vec<[f32; 3]> = img
        .pixels()
        .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])
        .collect();
    Ok((gray, rgb))
}

fn pearson(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len() as f32;
    let (ma, mb) = (a.iter().sum::<f32>() / n, b.iter().sum::<f32>() / n);
    let (mut sab, mut saa, mut sbb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        sab += (x - ma) * (y - mb);
        saa += (x - ma) * (x - ma);
        sbb += (y - mb) * (y - mb);
    }
    if saa == 0.0 || sbb == 0.0 {
        0.0
    } else {
        sab / (saa * sbb).sqrt()
    }
}

fn gradients(g: &[f32]) -> Vec<f32> {
    let mut out = vec![0.0; g.len()];
    for y in 1..63 {
        for x in 1..63 {
            let i = y * 64 + x;
            out[i] = (g[i + 1] - g[i - 1]).abs() + (g[i + 64] - g[i - 64]).abs();
        }
    }
    out
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let (ga, ca) = load(Path::new(&args[1]))?;
    let (gb, cb) = load(Path::new(&args[2]))?;
    let corr = pearson(&ga, &gb);
    let edges = pearson(&gradients(&ga), &gradients(&gb));
    let hue: f32 = ca
        .iter()
        .zip(&cb)
        .map(|(p, q)| ((p[0] - q[0]).abs() + (p[1] - q[1]).abs() + (p[2] - q[2]).abs()) / 3.0)
        .sum::<f32>()
        / ca.len() as f32;
    let verdict = if corr > 0.9 && edges > 0.7 {
        "quasi identique (agrandissement ou nettoyage de l'originale)"
    } else if corr > 0.6 || edges > 0.4 {
        "dérivée probable (même composition, retouches)"
    } else {
        "différente (composition indépendante)"
    };
    println!("{} vs {} : corrélation {corr:.2}, contours {edges:.2}, écart couleur moyen {hue:.0}/255 → {verdict}", args[1], args[2]);
    Ok(())
}
