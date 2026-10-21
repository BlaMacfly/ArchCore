//! Couche de contenu et journal des imports.
//!
//! * `content/*.sql` (à la racine du dépôt, versionné et publiable) : le contenu propre à ArchCore —
//!   corrections, apparitions ajoutées, PNJ créés avec l'éditeur — appliqué à la base monde **après**
//!   l'import, qui régénère les tables depuis les fichiers du client et les efface donc à chaque fois.
//!   `archcore import` l'applique à la fin ; `archcore content` le réapplique seul. Les fichiers passent
//!   dans l'ordre de leur nom, chacun dans une transaction (tout ou rien). Ils doivent être rejouables et ne
//!   contenir que des données (`INSERT … ON DUPLICATE KEY UPDATE`, `UPDATE` ou `DELETE` ciblés par TID ou par
//!   clé stable), sans instruction de schéma, qui validerait la transaction en cours.
//! * `import_run` : une ligne par source lue (client, relief, serveur, contenu), avec l'empreinte des fichiers
//!   et la version d'ArchCore ; `finished_at` reste vide si l'import s'arrête en route.

use crate::db::Databases;
use mysql_async::prelude::Queryable;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Fichiers d'un dossier retenus par `keep` (nom), triés par nom.
fn files_in(dir: &Path, keep: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.is_file() && p.file_name().and_then(|n| n.to_str()).is_some_and(&keep)).collect())
        .unwrap_or_default();
    files.sort();
    files
}

fn hex(d: impl AsRef<[u8]>) -> String {
    d.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

/// Empreinte d'un dossier source : SHA-256 des noms et tailles des fichiers retenus (rapide, même pour le
/// relief du client), et leur nombre.
pub fn fingerprint(dir: &Path, keep: impl Fn(&str) -> bool) -> (usize, String) {
    let files = files_in(dir, keep);
    let mut h = Sha256::new();
    for f in &files {
        let name = f.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let size = std::fs::metadata(f).map(|m| m.len()).unwrap_or(0);
        h.update(format!("{name}\t{size}\n").as_bytes());
    }
    (files.len(), hex(h.finalize()))
}

/// Ouvre une ligne d'`import_run` ; rend son identifiant.
pub async fn begin_run(db: &Databases, source: &str, location: &Path, files: usize, fingerprint: &str) -> anyhow::Result<u64> {
    db.world
        .exec(
            "INSERT INTO import_run (source, location, files, fingerprint, archcore_version) VALUES (?, ?, ?, ?, ?)",
            (source, location.display().to_string().chars().take(255).collect::<String>(), files as u64, fingerprint, env!("CARGO_PKG_VERSION")),
        )
        .await
}

/// Ferme une ligne d'`import_run` : la source a été lue jusqu'au bout.
pub async fn finish_run(db: &Databases, id: u64) -> anyhow::Result<()> {
    db.world.exec("UPDATE import_run SET finished_at = CURRENT_TIMESTAMP WHERE id = ?", (id,)).await?;
    Ok(())
}

/// Applique `content/*.sql` à la base monde, fichier par fichier, chacun dans une transaction ; rend le
/// nombre de fichiers appliqués.
pub async fn apply(db: &Databases, root: &Path) -> anyhow::Result<usize> {
    let dir = root.join("content");
    let files = files_in(&dir, |n| n.ends_with(".sql"));
    if files.is_empty() {
        println!("· contenu : aucun fichier dans {}", dir.display());
        return Ok(0);
    }
    let mut texts = Vec::new();
    let mut h = Sha256::new();
    for f in &files {
        let text = std::fs::read_to_string(f).map_err(|e| anyhow::anyhow!("{} : {e}", f.display()))?;
        h.update(f.file_name().and_then(|n| n.to_str()).unwrap_or_default().as_bytes());
        h.update(b"\n");
        h.update(text.as_bytes());
        texts.push(text);
    }
    let run = begin_run(db, "contenu", &dir, files.len(), &hex(h.finalize())).await?;
    for (f, text) in files.iter().zip(&texts) {
        let name = f.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let mut tx = db.world.begin().await?;
        tx.query_drop(text.as_str()).await.map_err(|e| anyhow::anyhow!("content/{name} : {e}"))?;
        tx.commit().await?;
        println!("✓ content/{name}");
    }
    finish_run(db, run).await?;
    Ok(files.len())
}

/// `archcore content` : réapplique la couche de contenu sans réimporter.
pub async fn run(_args: &[String]) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let db = Databases::new(&cfg.database);
    let n = apply(&db, &cfg.root).await?;
    println!("{n} fichier(s) de contenu appliqué(s) à {}", cfg.database.world);
    db.close().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empreinte_des_noms_et_tailles() {
        let dir = std::env::temp_dir().join("archcore-test-empreinte");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("b.sql"), "x").unwrap();
        std::fs::write(dir.join("a.sql"), "yy").unwrap();
        std::fs::write(dir.join("note.txt"), "z").unwrap();
        let (n, first) = fingerprint(&dir, |f| f.ends_with(".sql"));
        assert_eq!(n, 2);
        assert_eq!(files_in(&dir, |f| f.ends_with(".sql")).iter().map(|p| p.file_name().unwrap().to_str().unwrap().to_string()).collect::<Vec<_>>(), vec!["a.sql", "b.sql"]);
        std::fs::write(dir.join("b.sql"), "xx").unwrap();
        assert_ne!(fingerprint(&dir, |f| f.ends_with(".sql")).1, first, "une taille changée change l'empreinte");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
