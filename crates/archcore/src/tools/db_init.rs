//! `archcore db-init [--account nom mdp [mj]]` : crée les bases ArchCore et applique `sql/*.sql`,
//! puis crée ou met à jour un compte si demandé.

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
    let targets = [("001_auth.sql", &db.auth), ("002_characters.sql", &db.characters), ("003_world.sql", &db.world), ("004_locale.sql", &db.world)];
    for (_, name) in &targets {
        conn.query_drop(format!("CREATE DATABASE IF NOT EXISTS `{name}` CHARACTER SET utf8mb4")).await?;
    }
    let sql_dir = cfg.root.join("sql");
    let mut files: Vec<String> = std::fs::read_dir(&sql_dir)?
        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
        .filter(|f| f.ends_with(".sql"))
        .collect();
    files.sort();
    for file in files {
        let Some((_, target)) = targets.iter().find(|(f, _)| *f == file) else {
            println!("? {file} : base cible inconnue, ignoré");
            continue;
        };
        conn.query_drop(format!("USE `{target}`")).await?;
        let text = std::fs::read_to_string(sql_dir.join(&file))?;
        conn.query_drop(text).await?;
        println!("✓ {file} → {target}");
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
