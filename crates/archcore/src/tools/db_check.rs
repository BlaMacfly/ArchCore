//! `archcore db-check` : contrôle de cohérence des bases, en lecture seule (voir `check.rs`). Affiche les
//! versions du schéma, les anomalies et l'occupation des plages d'identifiants ; se termine en erreur s'il y
//! a une anomalie grave ou une base pas à jour.

use crate::check::{self, Names, Severity};
use crate::db::Databases;

pub async fn run(_args: &[String]) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let d = &cfg.database;
    let names = Names { auth: &d.auth, characters: &d.characters, world: &d.world };
    let db = Databases::new(d);
    println!("Bases : {}, {}, {}, {}", d.auth, d.characters, d.world, d.logs);

    // Versions du schéma.
    let schema_ok = match crate::migrations::verify(&db, [&d.auth, &d.characters, &d.world, &d.logs]).await {
        Ok(()) => {
            println!("✓ schéma à jour");
            true
        }
        Err(e) => {
            println!("✗ {e:#}");
            false
        }
    };

    // Références entre bases et plages d'identifiants.
    let found = check::run(&db, &names).await?;
    if found.is_empty() {
        println!("✓ aucune anomalie de cohérence");
    }
    for f in &found {
        let mark = if f.severity == Severity::Error { "✗" } else { "!" };
        println!("{mark} {}", check::describe(f));
    }
    for (label, max, limit) in check::id_usage(&db, &names).await? {
        println!("  identifiants des {label} : {max} sur {limit} ({:.4} %)", max as f64 * 100.0 / limit as f64);
    }
    db.close().await;
    let errors = found.iter().filter(|f| f.severity == Severity::Error).count();
    anyhow::ensure!(schema_ok && errors == 0, "{errors} anomalie(s) grave(s){}", if schema_ok { "" } else { ", schéma pas à jour" });
    Ok(())
}
