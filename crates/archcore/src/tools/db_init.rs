//! `archcore db-init [--account nom mdp [mj]]` : crée les bases ArchCore, rejoue la base de référence
//! (`sql/00*.sql`), applique les migrations manquantes (`sql/migrations/<base>/`) en les notant dans
//! `schema_migrations`, puis crée ou met à jour un compte si demandé. Les scripts sont ceux embarqués dans
//! l'exécutable (voir `migrations.rs`).

use crate::migrations::{self, BASELINE, BASES};
use mysql_async::prelude::*;
use mysql_async::OptsBuilder;

pub async fn run(args: &[String]) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let db = &cfg.database;
    let opts = OptsBuilder::default()
        .ip_or_hostname(db.host.clone())
        .tcp_port(db.port)
        .user(Some(db.user.clone()))
        .pass(Some(db.password.clone()));
    let mut conn = mysql_async::Conn::new(opts).await?;
    for file in migrations::baseline_unknown() {
        println!("? {file} : base cible inconnue, ignoré");
    }
    let names = [&db.auth, &db.characters, &db.world, &db.logs];
    for (base, name) in BASES.iter().zip(names) {
        conn.query_drop(format!("CREATE DATABASE IF NOT EXISTS `{name}` CHARACTER SET utf8mb4")).await?;
        conn.query_drop(format!("USE `{name}`")).await?;
        // Base de référence : idempotente, rejouée à chaque fois.
        for (file, text) in migrations::baseline(base) {
            conn.query_drop(text).await?;
            println!("✓ {file} → {name}");
        }
        conn.query_drop(migrations::TABLE_SQL).await?;
        // Table nommée avec sa base : une requête préparée reste liée à la base de sa préparation, et
        // mysql_async la garde en cache d'un `USE` à l'autre.
        let table = format!("`{name}`.schema_migrations");
        conn.exec_drop(
            format!("INSERT INTO {table} (id, checksum) VALUES (?, ?) ON DUPLICATE KEY UPDATE checksum = VALUES(checksum), applied_at = CURRENT_TIMESTAMP"),
            (BASELINE, migrations::baseline_checksum(base)),
        )
        .await?;
        // Migrations : une seule fois chacune, dans l'ordre, notée après son succès.
        let applied: Vec<String> = conn.query(format!("SELECT id FROM {table}")).await?;
        let mut count = 0;
        for m in migrations::expected(base) {
            if applied.iter().any(|id| id == m.id) {
                continue;
            }
            conn.query_drop(m.sql).await.map_err(|e| anyhow::anyhow!("migration {} sur {name} : {e}", m.id))?;
            conn.exec_drop(format!("INSERT INTO {table} (id, checksum) VALUES (?, ?)"), (m.id, migrations::checksum(m.sql))).await?;
            println!("✓ migration {} → {name}", m.id);
            count += 1;
        }
        let unknown: Vec<&String> = applied.iter().filter(|id| *id != BASELINE && !migrations::expected(base).iter().any(|m| m.id == id.as_str())).collect();
        if !unknown.is_empty() {
            println!("! {name} : migration(s) inconnue(s) de cet exécutable (base plus récente) : {unknown:?}");
        }
        let last = migrations::expected(base).last().map(|m| m.id).unwrap_or(BASELINE);
        println!("  {name} : {count} migration(s) appliquée(s), schéma à {last}");
    }
    if let Some(i) = args.iter().position(|a| a == "--account") {
        let (Some(user), Some(pass)) = (args.get(i + 1), args.get(i + 2)) else {
            anyhow::bail!("--account <utilisateur> <mot de passe> [gm]");
        };
        let gm: i64 = args.get(i + 3).map(|g| g.parse()).transpose()?.unwrap_or(0);
        let hash = crate::login::sha256_hex(&crate::tools::to_cp1252(pass));
        conn.query_drop(format!("USE `{}`", db.auth)).await?;
        conn.exec_drop(
            "INSERT INTO account (username, password_hash, gm_level) VALUES (?,?,?) ON DUPLICATE KEY UPDATE password_hash = VALUES(password_hash), gm_level = VALUES(gm_level)",
            (user, hash, gm),
        )
        .await?;
        println!("✓ compte {user} (gm {gm})");
    }
    conn.disconnect().await?;
    Ok(())
}
