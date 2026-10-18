//! Compare les pistes d'une animation de personnage avec les indices HAnim d'un clump.
//!
//! Usage : `cargo run -p archrust-data --example inspect_animation_rig -- <Game> <rig.ecl> <anim.ean>`

use archrust_data::{ean::EanFile, pack::PackIndex, rw::Clump};
use glam::{Mat4, Quat, Vec3};
use std::{
    fs::{self, File},
    path::PathBuf,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let game = PathBuf::from(args.next().expect("dossier Game"));
    let rig_name = args
        .next()
        .expect("clump .ecl")
        .to_string_lossy()
        .into_owned();
    let anim_name = args
        .next()
        .expect("animation .ean")
        .to_string_lossy()
        .into_owned();

    let character_dir = game.join("character");
    let rig_index = PackIndex::parse_encrypted(&fs::read(character_dir.join("Reference.Dat"))?)?;
    let rig_entry = rig_index
        .find(&rig_name)
        .ok_or_else(|| format!("{rig_name} absent"))?;
    let mut rig_data = File::open(character_dir.join("Data.Dat"))?;
    let clump = Clump::parse(&rig_index.read_entry(&mut rig_data, rig_entry)?)?;

    let anim_dir = character_dir.join("animation");
    let anim_index = PackIndex::parse_encrypted(&fs::read(anim_dir.join("Reference.Dat"))?)?;
    let anim_entry = anim_index
        .find(&anim_name)
        .ok_or_else(|| format!("{anim_name} absent"))?;
    let mut anim_data = File::open(anim_dir.join("Data.Dat"))?;
    let ean = EanFile::parse(&anim_index.read_entry(&mut anim_data, anim_entry)?)?;
    let chunk = ean
        .chunks
        .iter()
        .find(|c| c.id == 0x1b)
        .ok_or("no 0x1b animation chunk")?;
    let keys: Vec<_> = chunk
        .h_anim_keys()
        .ok_or("unsupported key layout")?
        .collect();
    for key_index in 0..3 {
        let start = 20 + key_index * 36;
        let words: Vec<_> = chunk.payload[start..start + 36]
            .chunks_exact(4)
            .map(|raw| u32::from_le_bytes(raw.try_into().unwrap()))
            .collect();
        println!("raw key {key_index}: {words:08x?}");
    }
    let tracks = chunk
        .linked_tracks()
        .ok_or("cannot construct candidate tracks")?;

    let root = clump
        .frames
        .iter()
        .filter_map(|f| f.hanim.as_ref())
        .find(|h| !h.nodes.is_empty())
        .ok_or("clump has no HAnim palette")?;
    let mut frame_for_index = vec![None; root.nodes.len()];
    for node in &root.nodes {
        if let Some(slot) = frame_for_index.get_mut(node.index as usize) {
            *slot = clump
                .frames
                .iter()
                .position(|f| f.hanim.as_ref().is_some_and(|h| h.node_id == node.id));
        }
    }

    let mut error_sum = [0.0f64; 2];
    let mut best_ordinal = [0usize; 2];
    let mut compared = 0;
    let mut translation_aligned = 0;
    let mut unmatched = 0;
    let mut correspondence = Vec::new();
    for (track_index, track) in tracks.iter().enumerate() {
        let Some(&key_index) = track
            .iter()
            .min_by(|&&a, &&b| keys[a].time.total_cmp(&keys[b].time))
        else {
            continue;
        };
        let key = keys[key_index];
        let key_matrix = Mat4::from_rotation_translation(
            Quat::from_xyzw(
                key.rotation[0],
                key.rotation[1],
                key.rotation[2],
                key.rotation[3],
            )
            .normalize(),
            Vec3::from_array(key.translation),
        );
        if let Some(frame_index) = frame_for_index.get(track_index).copied().flatten() {
            let local = clump.frames[frame_index].local_matrix();
            let translation_error =
                (clump.frames[frame_index].position - Vec3::from_array(key.translation)).length();
            if translation_error < 1e-3 {
                translation_aligned += 1;
            } else {
                println!("    piste {track_index} : translation frame ordinal={frame_index}, erreur={translation_error:.3}, key={:?}, frame={:?}", key.translation, clump.frames[frame_index].position);
            }
            for mode in 0..2 {
                let candidate = if mode == 0 { local } else { local.transpose() };
                let error = matrix_error(candidate, key_matrix);
                error_sum[mode] += error as f64;
                let nearest = frame_for_index
                    .iter()
                    .flatten()
                    .map(|&i| {
                        matrix_error(
                            if mode == 0 {
                                clump.frames[i].local_matrix()
                            } else {
                                clump.frames[i].local_matrix().transpose()
                            },
                            key_matrix,
                        )
                    })
                    .fold(f32::INFINITY, f32::min);
                if (error - nearest).abs() < 1e-4 {
                    best_ordinal[mode] += 1;
                } else if mode == 0 {
                    let nearest_frame =
                        frame_for_index.iter().flatten().copied().min_by(|&a, &b| {
                            let ea = matrix_error(clump.frames[a].local_matrix(), key_matrix);
                            let eb = matrix_error(clump.frames[b].local_matrix(), key_matrix);
                            ea.total_cmp(&eb)
                        });
                    let expected = frame_for_index.get(track_index).copied().flatten();
                    let nearest_node = nearest_frame.and_then(|frame| {
                        root.nodes.iter().find(|node| {
                            frame_for_index.get(node.index as usize).copied().flatten()
                                == Some(frame)
                        })
                    });
                    let expected_position_error = expected
                        .map(|i| {
                            (clump.frames[i].position - Vec3::from_array(key.translation)).length()
                        })
                        .unwrap_or(f32::INFINITY);
                    let closest_position =
                        frame_for_index.iter().flatten().copied().min_by(|&a, &b| {
                            let ea = (clump.frames[a].position - Vec3::from_array(key.translation))
                                .length();
                            let eb = (clump.frames[b].position - Vec3::from_array(key.translation))
                                .length();
                            ea.total_cmp(&eb)
                        });
                    println!("    piste {track_index} : ordinal={expected:?}, meilleur transform={nearest_frame:?}/node {:?}, erreur matrice={error:.3}/{nearest:.3}, erreur translation={expected_position_error:.3}, meilleur frame translation={closest_position:?}", nearest_node.map(|node| (node.index, node.id)));
                }
            }
            compared += 1;
            if track_index < 8 || track_index == 32 {
                let (_, rest_rotation, _) = local.to_scale_rotation_translation();
                correspondence.push((track_index, key, frame_index, local));
                if track_index == 32 {
                    println!("    piste 32 rotation repos={:?}", rest_rotation.to_array());
                }
            }
        } else {
            unmatched += 1;
        }
    }
    println!(
        "rig={rig_name}: {} HAnim frames, animation={anim_name}: {} tracks/{} keys",
        root.nodes.len(),
        tracks.len(),
        keys.len()
    );
    println!("ordinal agreement with nearest rest-frame transform: {best_ordinal:?}/{compared}, translations equal to bind={translation_aligned}/{compared}, aggregate errors={error_sum:?}, tracks without HAnim node={unmatched}");
    for (track_index, key, frame_index, local) in correspondence {
        println!(
            "  piste {track_index} key t={:.3} p={:?} q={:?}; frame {frame_index} p={:?}",
            key.time,
            key.translation,
            key.rotation,
            local.w_axis.truncate()
        );
    }
    let duration = keys
        .iter()
        .map(|key| key.time)
        .filter(|time| time.is_finite())
        .fold(0.0f32, f32::max);
    for time in [0.0, duration * 0.5] {
        let pose = chunk
            .sample_linked_tracks(time)
            .ok_or("cannot sample animation")?;
        let animated_world = clump.world_matrices_with_pose(&pose);
        let bind_world = clump.world_matrices();
        let max_matrix_delta = animated_world
            .iter()
            .zip(&bind_world)
            .map(|(a, b)| matrix_error(*a, *b))
            .fold(0.0f32, f32::max);
        println!("  t={time:.3}s : écart maximal des matrices monde={max_matrix_delta:.4}");
        let largest_frame = animated_world
            .iter()
            .zip(&bind_world)
            .enumerate()
            .map(|(i, (a, b))| (i, matrix_error(*a, *b)))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, delta)) = largest_frame {
            let node = root.nodes.iter().find(|node| {
                clump.frames[i]
                    .hanim
                    .as_ref()
                    .is_some_and(|h| h.node_id == node.id)
            });
            println!(
                "    frame le plus divergent {i} : {delta:.4} ({}), nœud HAnim {:?}",
                clump.frames[i].name,
                node.map(|node| (node.index, node.id))
            );
        }
        for (geometry_index, geometry) in clump.geometries.iter().enumerate() {
            if geometry.skin.is_none() {
                continue;
            }
            let deformed = clump.deform_geometry_positions(geometry_index, &pose)?;
            let (minimum, maximum) = geometry.vertices.iter().fold(
                (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
                |(minimum, maximum), point| (minimum.min(*point), maximum.max(*point)),
            );
            let diagonal = (maximum - minimum).length();
            let (max_delta, mean_delta) = deformed
                .iter()
                .zip(&geometry.vertices)
                .map(|(a, b)| (*a - *b).length())
                .fold((0.0f32, 0.0f64), |(max, sum), delta| {
                    (max.max(delta), sum + delta as f64)
                });
            println!(
                "  géo {geometry_index}, t={time:.3}s : boîte={diagonal:.2}, max déplacement={max_delta:.4}, moyen={:.4}",
                mean_delta / geometry.vertices.len().max(1) as f64
            );
        }
    }
    for (geometry_index, geometry) in clump.geometries.iter().enumerate() {
        if let Some(skin) = &geometry.skin {
            let undeformed = clump.deform_geometry_positions(geometry_index, &[])?;
            let max_delta = undeformed
                .iter()
                .zip(&geometry.vertices)
                .map(|(a, b)| (*a - *b).length())
                .fold(0.0f32, f32::max);
            println!(
                "  géo {geometry_index}, pose bind : écart max={max_delta:.6} (palette {})",
                skin.inverse_bind.len()
            );
        }
    }
    Ok(())
}

fn matrix_error(a: Mat4, b: Mat4) -> f32 {
    a.to_cols_array()
        .iter()
        .zip(b.to_cols_array())
        .map(|(x, y)| (x - y).abs())
        .sum()
}
