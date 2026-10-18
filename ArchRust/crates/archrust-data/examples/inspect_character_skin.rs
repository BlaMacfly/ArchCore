//! Inspecte le squelette et les palettes de peau d'un modèle réel.
//!
//! Usage : `cargo run -p archrust-data --example inspect_character_skin -- <dossier Game> [nom.ecl]`

use archrust_data::{pack::PackIndex, rw::Clump};
use std::{
    fs::{self, File},
    path::PathBuf,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let game = PathBuf::from(args.next().expect("dossier Game"));
    let wanted = args
        .next()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "b0000060.ecl".into());
    let dir = game.join("character");
    let index = PackIndex::parse_encrypted(&fs::read(dir.join("Reference.Dat"))?)?;
    let entry = index
        .find(&wanted)
        .ok_or_else(|| format!("{wanted} absent du pack character"))?;
    let mut data = File::open(dir.join("Data.Dat"))?;
    let bytes = index.read_entry(&mut data, entry)?;
    let clump = Clump::parse(&bytes)?;
    let hierarchy: Vec<_> = clump
        .frames
        .iter()
        .enumerate()
        .filter_map(|(frame, f)| {
            f.hanim.as_ref().map(|h| {
                (
                    frame,
                    h.node_id,
                    h.nodes.len(),
                    h.nodes.first().map(|n| (n.id, n.index, n.flags)),
                )
            })
        })
        .collect();
    println!(
        "{wanted}: {} frames ({} HAnim), {} géométries, {} atomics",
        clump.frames.len(),
        hierarchy.len(),
        clump.geometries.len(),
        clump.atomics.len()
    );
    println!(
        "HAnim exemples: {:?}",
        &hierarchy[..hierarchy.len().min(12)]
    );
    println!(
        "noms de frames : {:?}",
        clump
            .frames
            .iter()
            .filter(|f| !f.name.is_empty())
            .take(12)
            .map(|f| (f.name.as_str(), f.parent))
            .collect::<Vec<_>>()
    );
    if let Some(root) = clump
        .frames
        .iter()
        .find_map(|f| f.hanim.as_ref().filter(|h| !h.nodes.is_empty()))
    {
        println!(
            "HAnim palette (id,index,flags): {:?}",
            &root.nodes[..root.nodes.len().min(32)]
                .iter()
                .map(|n| (n.id, n.index, n.flags))
                .collect::<Vec<_>>()
        );
    }
    let world = clump.world_matrices();
    let bone_to_frame: Vec<Option<usize>> = clump
        .frames
        .iter()
        .filter_map(|f| f.hanim.as_ref())
        .find(|h| !h.nodes.is_empty())
        .map(|h| {
            let mut by_index = vec![None; h.nodes.len()];
            for node in &h.nodes {
                if let Some(slot) = by_index.get_mut(node.index as usize) {
                    *slot = clump.frames.iter().position(|f| {
                        f.hanim
                            .as_ref()
                            .is_some_and(|frame_hanim| frame_hanim.node_id == node.id)
                    });
                }
            }
            by_index
        })
        .unwrap_or_default();
    for (gi, g) in clump.geometries.iter().enumerate() {
        let Some(skin) = &g.skin else { continue };
        let max_index = skin.indices.iter().flatten().copied().max();
        let weight_sum = skin
            .weights
            .iter()
            .map(|w| w.iter().sum::<f32>())
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), x| {
                (lo.min(x), hi.max(x))
            });
        println!("géo {gi}: {} sommets, palette={}, utilisés={}, inverse_bind={}, indices={}, poids={}, max_index={max_index:?}, somme_poids={weight_sum:?}", g.vertices.len(), skin.bone_count, skin.used_bones.len(), skin.inverse_bind.len(), skin.indices.len(), skin.weights.len());
        println!(
            "  used_bones={:?}",
            &skin.used_bones[..skin.used_bones.len().min(24)]
        );
        println!("  indices={:?}", &skin.indices[..skin.indices.len().min(5)]);
        println!("  poids={:?}", &skin.weights[..skin.weights.len().min(5)]);
        let skin_matrices: Vec<_> = skin
            .inverse_bind
            .iter()
            .enumerate()
            .map(|(bone, bind)| {
                bone_to_frame
                    .get(bone)
                    .copied()
                    .flatten()
                    .map(|frame| world[frame].transpose() * bind.transpose())
                    .unwrap_or(glam::Mat4::IDENTITY)
            })
            .collect();
        let deformed = skin.deform_positions(&g.vertices, &skin_matrices)?;
        let residual_max = deformed
            .iter()
            .zip(&g.vertices)
            .map(|(a, b)| (*a - *b).length())
            .fold(0.0f32, f32::max);
        println!("  résidu pose bind (skinning) = {residual_max}");
    }
    Ok(())
}
