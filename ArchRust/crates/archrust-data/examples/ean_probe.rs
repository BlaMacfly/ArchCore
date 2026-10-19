//! Sonde des conventions d'animation sur les données réelles du client Archonia.
//!
//! Compare un squelette (`a…ecl`), une animation (`a…ean`) et un corps skinné (`b…ecl`) :
//! correspondance piste ↔ nœud HAnim, convention des quaternions, matrices inverses de bind et
//! amplitude de la déformation obtenue.
//!
//! Usage : `cargo run -p archrust-data --example ean_probe -- <Game> a0000060.ecl a0060000.ean b0000060.ecl`

use archrust_data::{ean::EanFile, pack::PackIndex, rw::Clump};
use glam::{Mat3, Mat4, Quat, Vec3, Vec4};
use std::{fs, fs::File, path::Path};

fn read(game: &Path, dir: &str, name: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let d = game.join(dir);
    let index = PackIndex::parse_encrypted(&fs::read(d.join("Reference.Dat"))?)?;
    let entry = index
        .find(name)
        .ok_or_else(|| format!("{name} absent de {dir}"))?;
    let mut data = File::open(d.join("Data.Dat"))?;
    Ok(index.read_entry(&mut data, entry)?)
}

/// Matrice inverse de bind « nettoyée » : quatrième ligne remise à (0, 0, 0, 1). RenderWare range
/// des drapeaux et du remplissage dans ces composantes.
fn affine(m: Mat4) -> Mat4 {
    let mut c = m.to_cols_array_2d();
    for col in c.iter_mut().take(3) {
        col[3] = 0.0;
    }
    c[3][3] = 1.0;
    Mat4::from_cols_array_2d(&c)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let game = Path::new(&a[0]);
    let rig = Clump::parse(&read(game, "character", &a[1])?)?;
    let ean = EanFile::parse(&read(game, "character/animation", &a[2])?)?;
    let body = Clump::parse(&read(game, "character", &a[3])?)?;

    let chunk = ean
        .chunks
        .iter()
        .find(|c| c.id == 0x1b)
        .ok_or("pas de bloc 0x1b")?;
    let layout = chunk.animation_layout().ok_or("disposition inconnue")?;
    println!(
        "préfixe {:08x?} ({} clés) ; durée si f32 : {}",
        layout.prefix,
        layout.key_count,
        f32::from_bits(layout.prefix[4])
    );
    let keys: Vec<_> = chunk.h_anim_keys().unwrap().collect();

    let h = rig
        .frames
        .iter()
        .filter_map(|f| f.hanim.as_ref())
        .find(|h| !h.nodes.is_empty())
        .ok_or("pas de hiérarchie")?;
    let n = h.nodes.len();
    println!(
        "{n} nœuds ; nodeIndex = rang ? {}",
        h.nodes
            .iter()
            .enumerate()
            .all(|(i, nd)| nd.index as usize == i)
    );
    let frame_of = |id: u32| {
        rig.frames
            .iter()
            .position(|f| f.hanim.as_ref().is_some_and(|x| x.node_id == id))
    };

    // Liens des 2n premières clés
    let firsts: Vec<_> = (0..n.min(4)).map(|i| keys[i].previous).collect();
    let seconds_ok = (0..n)
        .filter(|&i| keys[n + i].previous as usize == i * 36)
        .count();
    println!("prev des premières clés {firsts:x?} ; clés n+i liées à i : {seconds_ok}/{n}");

    // Clé i (temps 0) contre frame locale au repos du nœud i
    let (mut t_ok, mut q_same, mut q_conj) = (0, 0, 0);
    for (i, nd) in h.nodes.iter().enumerate() {
        let Some(fi) = frame_of(nd.id) else { continue };
        let f = &rig.frames[fi];
        let rest_q = Quat::from_mat3(&Mat3::from_mat4(f.local_matrix())).normalize();
        let k = keys[i];
        let q =
            Quat::from_xyzw(k.rotation[0], k.rotation[1], k.rotation[2], k.rotation[3]).normalize();
        if (Vec3::from_array(k.translation) - f.position).length() < 0.5 {
            t_ok += 1;
        }
        if rest_q.dot(q).abs() > 0.999 {
            q_same += 1;
        }
        if rest_q.dot(q.conjugate()).abs() > 0.999 {
            q_conj += 1;
        }
    }
    println!("clé 0 ↔ repos : translation {t_ok}/{n}, quaternion tel quel {q_same}/{n}, conjugué {q_conj}/{n}");

    // Matrices inverses de bind : monde_repos(nœud i) * inverse_bind[i] ≈ identité ?
    let g = body
        .geometries
        .iter()
        .enumerate()
        .filter(|(_, g)| g.skin.is_some())
        .max_by_key(|(_, g)| g.vertices.len())
        .map(|(i, _)| i)
        .ok_or("corps sans skin")?;
    println!(
        "géométries skinnées : {:?}",
        body.geometries
            .iter()
            .map(|g| (
                g.vertices.len(),
                g.skin.as_ref().map(|s| s.inverse_bind.len())
            ))
            .collect::<Vec<_>>()
    );
    let skin = body.geometries[g].skin.as_ref().unwrap();
    let rig_world = rig.world_matrices();
    let mut err_raw = 0f32;
    let mut err_aff = 0f32;
    let mut err_tr = 0f32;
    for (i, nd) in h.nodes.iter().enumerate().take(skin.inverse_bind.len()) {
        let Some(fi) = frame_of(nd.id) else { continue };
        let w = rig_world[fi];
        let ib = skin.inverse_bind[i];
        let d = |m: Mat4| {
            (m - Mat4::IDENTITY)
                .to_cols_array()
                .iter()
                .map(|v| v.abs())
                .fold(0f32, f32::max)
        };
        err_raw = err_raw.max(d(w * ib));
        err_aff = err_aff.max(d(w * affine(ib)));
        err_tr = err_tr.max(d(w.transpose() * ib.transpose()));
        let atomic_frame = body
            .atomics
            .iter()
            .find(|a| a.geometry as usize == g)
            .map(|a| a.frame as usize)
            .unwrap_or(0);
        let e = rig_world[atomic_frame].inverse() * w * affine(ib);
        if i == 0 {
            println!(
                "  frame de l atomic : {atomic_frame}, translation monde {:?}",
                rig_world[atomic_frame].w_axis.truncate()
            );
        }
        if d(e) > 1e-3 && i < 6 {
            println!(
                "  nœud {i} (frame {fi} {:?}, parent {}) : translation résiduelle {:?}, rot ok {}",
                rig.frames[fi].name,
                rig.frames[fi].parent,
                e.w_axis.truncate(),
                (Mat3::from_mat4(e) - Mat3::IDENTITY)
                    .to_cols_array()
                    .iter()
                    .all(|v| v.abs() < 1e-3)
            );
        }
    }
    println!(
        "écart max à l'identité : w*ib {err_raw:.4}, w*affine(ib) {err_aff:.4}, wᵀ*ibᵀ {err_tr:.4}"
    );
    println!(
        "4e ligne de ib[1] : {:?}",
        skin.inverse_bind.get(1).map(|m| m.row(3))
    );

    // Déformation à t = 0 et à mi-durée, deux conventions de quaternion
    let rest = &body.geometries[g].vertices;
    let bbox = |v: &[Vec3]| {
        v.iter().fold(
            (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        )
    };
    let (lo, hi) = bbox(rest);
    println!(
        "corps au repos : {} sommets, boîte {lo:.1} → {hi:.1}",
        rest.len()
    );
    let dur = keys.iter().map(|k| k.time).fold(0f32, f32::max);
    for &t in &[0.0, dur * 0.5] {
        let pose = chunk.sample_linked_tracks(t).unwrap();
        for conj in [false, true] {
            let mut world = vec![Mat4::IDENTITY; rig.frames.len()];
            let mut sample_of_frame = vec![None; rig.frames.len()];
            for (i, nd) in h.nodes.iter().enumerate() {
                if let (Some(fi), Some(s)) = (frame_of(nd.id), pose.get(i)) {
                    sample_of_frame[fi] = Some(*s);
                }
            }
            for (fi, f) in rig.frames.iter().enumerate() {
                let local = match sample_of_frame[fi] {
                    Some(s) => {
                        let q = Quat::from_array(s.rotation).normalize();
                        let q = if conj { q.conjugate() } else { q };
                        Mat4::from_rotation_translation(q, Vec3::from_array(s.translation))
                    }
                    None => f.local_matrix(),
                };
                world[fi] = if f.parent >= 0 && (f.parent as usize) < fi {
                    world[f.parent as usize] * local
                } else {
                    local
                };
            }
            let mats: Vec<Mat4> = h
                .nodes
                .iter()
                .enumerate()
                .take(skin.inverse_bind.len())
                .map(|(i, nd)| {
                    frame_of(nd.id)
                        .map(|fi| world[fi] * affine(skin.inverse_bind[i]))
                        .unwrap_or(Mat4::IDENTITY)
                })
                .collect();
            let out = skin.deform_positions(rest, &mats)?;
            let max_move = out
                .iter()
                .zip(rest)
                .map(|(a, b)| (*a - *b).length())
                .fold(0f32, f32::max);
            let (lo, hi) = bbox(&out);
            println!(
                "t={t:.3} {} : déplacement max {max_move:.1}, boîte {lo:.1} → {hi:.1}",
                if conj { "conjugué" } else { "tel quel" }
            );
        }
    }
    let _ = Vec4::ZERO;
    Ok(())
}
