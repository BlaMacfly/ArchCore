//! Écrit un patch d'exemple : un cube texturé en GLB (généré, CC0) qui remplace le corps du
//! personnage humain par défaut (TID 96 → `character/a0000060.ecl`).
//!
//! ```text
//! cargo run -p archrust-engine --example cube_glb -- patches/exemple-gltf
//! cargo run -- inspect gltf patches/exemple-gltf/models/cube.glb
//! cargo run -- play --no-client --screenshot docs/captures/gltf.png
//! ```

use archrust_engine::gltf_model::{sample_cube_glb, sample_patch_manifest};
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("patches/exemple-gltf"));
    std::fs::create_dir_all(dir.join("models"))?;
    let glb = sample_cube_glb();
    std::fs::write(dir.join("models/cube.glb"), &glb)?;
    std::fs::write(dir.join("patch.toml"), sample_patch_manifest())?;
    println!(
        "patch écrit dans {} (models/cube.glb : {} octets)",
        dir.display(),
        glb.len()
    );
    Ok(())
}
