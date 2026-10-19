//! `archcore config migrate <archcore.json> [dossier] [--force]` : convertit une ancienne configuration
//! JSON en `auth.conf` et `world.conf`, à partir des modèles `.dist` (commentaires conservés).

use crate::config::{self, Which};
use std::path::PathBuf;

pub fn run(args: &[String]) -> anyhow::Result<()> {
    let force = args.iter().any(|a| a == "--force");
    let positional: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    match positional.first().map(|s| s.as_str()) {
        Some("migrate") => {
            let json = positional.get(1).ok_or_else(|| anyhow::anyhow!("usage : archcore config migrate <archcore.json> [dossier] [--force]"))?;
            let root = config::root();
            let dir = positional.get(2).map(PathBuf::from).unwrap_or_else(|| config::conf_dir(&root));
            let cfg = config::load_json(json.as_ref())?;
            std::fs::create_dir_all(&dir)?;
            for which in [Which::Auth, Which::World] {
                let out = dir.join(which.file_name());
                anyhow::ensure!(force || !out.exists(), "{} existe déjà (--force pour le remplacer)", out.display());
                let dist = root.join("config").join(format!("{}.dist", which.file_name()));
                let template = std::fs::read_to_string(&dist).map_err(|e| anyhow::anyhow!("{} : {e}", dist.display()))?;
                std::fs::write(&out, config::fill_template(&template, &config::to_values(&cfg, which)))?;
                println!("{} écrit", out.display());
            }
            Ok(())
        }
        _ => Err(anyhow::anyhow!("usage : archcore config migrate <archcore.json> [dossier] [--force]")),
    }
}
