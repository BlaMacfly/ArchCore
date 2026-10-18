//! Accès MariaDB : un pool par base (auth, characters, world).

use crate::config::DatabaseConfig;
use mysql_async::prelude::*;
use mysql_async::{OptsBuilder, Params, PoolConstraints, PoolOpts, Row};

#[derive(Clone)]
pub struct Pool(mysql_async::Pool);

impl Pool {
    fn new(cfg: &DatabaseConfig, db: &str) -> Self {
        let constraints = PoolConstraints::new(1, cfg.connection_limit.max(1)).unwrap_or_default();
        let opts = OptsBuilder::default()
            .ip_or_hostname(cfg.host.clone())
            .tcp_port(cfg.port)
            .user(Some(cfg.user.clone()))
            .pass(Some(cfg.password.clone()))
            .db_name(Some(db.to_string()))
            .pool_opts(PoolOpts::default().with_constraints(constraints));
        Pool(mysql_async::Pool::new(opts))
    }

    /// Requête qui rend des lignes.
    pub async fn rows(&self, sql: &str, params: impl Into<Params> + Send) -> anyhow::Result<Vec<Row>> {
        let mut conn = self.0.get_conn().await?;
        Ok(conn.exec(sql, params).await?)
    }

    /// Première ligne, s'il y en a une.
    pub async fn first(&self, sql: &str, params: impl Into<Params> + Send) -> anyhow::Result<Option<Row>> {
        let mut conn = self.0.get_conn().await?;
        Ok(conn.exec_first(sql, params).await?)
    }

    /// Requête sans résultat ; rend l'identifiant inséré (0 s'il n'y en a pas).
    pub async fn exec(&self, sql: &str, params: impl Into<Params> + Send) -> anyhow::Result<u64> {
        let mut conn = self.0.get_conn().await?;
        conn.exec_drop(sql, params).await?;
        Ok(conn.last_insert_id().unwrap_or(0))
    }

    /// Même requête pour chaque jeu de paramètres (insertions en masse).
    pub async fn batch<P: Into<Params> + Send>(&self, sql: &str, rows: Vec<P>) -> anyhow::Result<()> {
        let mut conn = self.0.get_conn().await?;
        conn.exec_batch(sql, rows).await?;
        Ok(())
    }

    pub async fn close(self) {
        let _ = self.0.disconnect().await;
    }
}

#[derive(Clone)]
pub struct Databases {
    pub auth: Pool,
    pub characters: Pool,
    pub world: Pool,
}

impl Databases {
    pub fn new(cfg: &DatabaseConfig) -> Self {
        Databases {
            auth: Pool::new(cfg, &cfg.auth),
            characters: Pool::new(cfg, &cfg.characters),
            world: Pool::new(cfg, &cfg.world),
        }
    }

    pub async fn check(&self) -> anyhow::Result<()> {
        for (name, pool) in [("auth", &self.auth), ("characters", &self.characters), ("world", &self.world)] {
            let row = pool.first("SELECT DATABASE() AS db, VERSION() AS v", ()).await?.ok_or_else(|| anyhow::anyhow!("base {name} muette"))?;
            let v: String = row.get("v").unwrap_or_default();
            let db: String = row.get("db").unwrap_or_default();
            crate::info!("MariaDB {v} — base {name} = {db}");
        }
        Ok(())
    }

    pub async fn close(self) {
        self.auth.close().await;
        self.characters.close().await;
        self.world.close().await;
    }
}

/// Lecture tolérante d'une colonne (NULL ou type inattendu → valeur par défaut).
pub fn col<T: FromValue + Default>(row: &Row, name: &str) -> T {
    row.get_opt::<Option<T>, _>(name).and_then(|r| r.ok()).flatten().unwrap_or_default()
}
