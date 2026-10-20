//! Versions du schéma des bases : base de référence et migrations numérotées.
//!
//! * `sql/001…004` forment la **base de référence** : `archcore db-init` les rejoue à chaque fois (elles
//!   sont idempotentes) et le note sous l'identifiant [`BASELINE`].
//! * Les changements suivants vont dans `sql/migrations/<base>/<AAAAMMJJHHMM>_<sujet>.sql`, où `<base>` vaut
//!   `auth`, `characters` ou `world` : la base cible se lit dans le chemin. Chacune n'est appliquée qu'une
//!   fois, dans l'ordre des identifiants, puis enregistrée dans la table `schema_migrations` de sa base
//!   (identifiant, date, empreinte SHA-256 du texte).
//! * Les scripts sont embarqués dans l'exécutable (`build.rs`) : le serveur sait à la compilation quelles
//!   migrations il attend, et **refuse de démarrer** si une base est en retard (migrations à appliquer) ou en
//!   avance (migrations inconnues de cet exécutable).
//! * Une migration est additive par défaut ; un renommage ou une suppression se fait en deux versions
//!   (ajout et double lecture, puis retrait).

use crate::db::{col, Databases, Pool};
use sha2::{Digest, Sha256};

include!(concat!(env!("OUT_DIR"), "/sql_files.rs"));

/// Identifiant de la base de référence dans `schema_migrations`.
pub const BASELINE: &str = "0000_reference";

/// Table des versions, créée dans chaque base.
pub const TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS schema_migrations (\n  id         VARCHAR(96) NOT NULL COMMENT 'base de référence ou fichier de sql/migrations/<base>/ sans .sql',\n  applied_at DATETIME    NOT NULL DEFAULT CURRENT_TIMESTAMP,\n  checksum   CHAR(64)    NOT NULL COMMENT 'SHA-256 hexadécimal du texte appliqué',\n  PRIMARY KEY (id)\n) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4";

/// Bases logiques, dans l'ordre d'application.
pub const BASES: [&str; 3] = ["auth", "characters", "world"];

/// Base logique de chaque fichier de la base de référence.
const BASELINE_TARGETS: [(&str, &str); 4] = [("001_auth.sql", "auth"), ("002_characters.sql", "characters"), ("003_world.sql", "world"), ("004_locale.sql", "world")];

/// Une migration embarquée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Migration {
    pub base: &'static str,
    pub id: &'static str,
    pub sql: &'static str,
}

/// Fichiers de la base de référence d'une base logique : (fichier, texte).
pub fn baseline(base: &str) -> Vec<(&'static str, &'static str)> {
    BASELINE_FILES.iter().filter(|(f, _)| BASELINE_TARGETS.iter().any(|(t, b)| t == f && *b == base)).copied().collect()
}

/// Fichiers de `sql/` qui n'appartiennent à aucune base (signalés par `db-init`).
pub fn baseline_unknown() -> Vec<&'static str> {
    BASELINE_FILES.iter().map(|(f, _)| *f).filter(|f| !BASELINE_TARGETS.iter().any(|(t, _)| t == f)).collect()
}

/// Migrations attendues pour une base, dans l'ordre.
pub fn expected(base: &str) -> Vec<Migration> {
    MIGRATION_FILES.iter().filter(|(b, _, _)| *b == base).map(|&(base, id, sql)| Migration { base, id, sql }).collect()
}

pub fn checksum(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// Empreinte de la base de référence d'une base logique.
pub fn baseline_checksum(base: &str) -> String {
    checksum(&baseline(base).iter().map(|(_, t)| *t).collect::<Vec<_>>().join("\n"))
}

/// État d'une base par rapport à cet exécutable.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Verdict {
    /// Table `schema_migrations` ou base de référence absente : base jamais passée par ce `db-init`.
    pub not_initialized: bool,
    /// Migrations attendues pas encore appliquées.
    pub behind: Vec<String>,
    /// Migrations appliquées inconnues de cet exécutable.
    pub ahead: Vec<String>,
    /// Migrations appliquées dont le texte a changé depuis (avertissement seulement).
    pub altered: Vec<String>,
    /// La base de référence a changé depuis le dernier `db-init` (avertissement seulement).
    pub baseline_changed: bool,
}

impl Verdict {
    /// La base peut servir : ni en retard, ni en avance, ni jamais initialisée.
    pub fn usable(&self) -> bool {
        !self.not_initialized && self.behind.is_empty() && self.ahead.is_empty()
    }
}

/// Compare les lignes de `schema_migrations` (identifiant, empreinte) à ce que l'exécutable attend.
pub fn compare(base: &str, applied: Option<&[(String, String)]>) -> Verdict {
    let Some(applied) = applied else { return Verdict { not_initialized: true, ..Default::default() } };
    let mut v = Verdict::default();
    match applied.iter().find(|(id, _)| id == BASELINE) {
        None => v.not_initialized = true,
        Some((_, sum)) => v.baseline_changed = *sum != baseline_checksum(base),
    }
    let expected = expected(base);
    for m in &expected {
        match applied.iter().find(|(id, _)| id == m.id) {
            None => v.behind.push(m.id.to_string()),
            Some((_, sum)) if *sum != checksum(m.sql) => v.altered.push(m.id.to_string()),
            Some(_) => {}
        }
    }
    v.ahead = applied.iter().map(|(id, _)| id).filter(|id| *id != BASELINE && !expected.iter().any(|m| m.id == id.as_str())).cloned().collect();
    v
}

/// Lignes de `schema_migrations` d'une base, `None` si la table n'existe pas.
pub async fn applied(pool: &Pool) -> anyhow::Result<Option<Vec<(String, String)>>> {
    let exists = pool.first("SELECT 1 AS x FROM information_schema.tables WHERE table_schema = DATABASE() AND table_name = 'schema_migrations'", ()).await?;
    if exists.is_none() {
        return Ok(None);
    }
    let rows = pool.rows("SELECT id, checksum FROM schema_migrations ORDER BY id", ()).await?;
    Ok(Some(rows.iter().map(|r| (col::<String>(r, "id"), col::<String>(r, "checksum"))).collect()))
}

/// Contrôle de version au démarrage du serveur : refuse une base en retard, en avance ou jamais
/// initialisée, avec la marche à suivre.
pub async fn verify(db: &Databases, names: [&str; 3]) -> anyhow::Result<()> {
    let mut errors = Vec::new();
    for ((base, pool), name) in BASES.iter().zip([&db.auth, &db.characters, &db.world]).zip(names) {
        let v = compare(base, applied(pool).await?.as_deref());
        if v.not_initialized {
            errors.push(format!("base {name} : pas encore suivie par les migrations — lancer `archcore db-init`"));
        }
        if !v.behind.is_empty() {
            errors.push(format!("base {name} : {} migration(s) à appliquer ({}) — lancer `archcore db-init`", v.behind.len(), v.behind.join(", ")));
        }
        if !v.ahead.is_empty() {
            errors.push(format!(
                "base {name} : migration(s) inconnue(s) de cet exécutable ({}) — la base vient d'une version plus récente d'ArchCore : mettre à jour l'exécutable",
                v.ahead.join(", ")
            ));
        }
        if v.baseline_changed && !v.not_initialized {
            crate::warn!("base {name} : la base de référence (sql/00*.sql) a changé depuis le dernier `archcore db-init`");
        }
        for id in &v.altered {
            crate::warn!("base {name} : la migration {id} a été modifiée après avoir été appliquée");
        }
        if v.usable() {
            let last = expected(base).last().map(|m| m.id).unwrap_or(BASELINE);
            crate::info!("base {name} : schéma à jour ({last})");
        }
    }
    anyhow::ensure!(errors.is_empty(), "schéma des bases à mettre à jour :\n  {}", errors.join("\n  "));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, sum: &str) -> (String, String) {
        (id.to_string(), sum.to_string())
    }

    #[test]
    fn base_de_reference_embarquee() {
        for base in BASES {
            assert!(!baseline(base).is_empty(), "base de référence vide pour {base}");
        }
        assert_eq!(baseline("world").iter().map(|(f, _)| *f).collect::<Vec<_>>(), vec!["003_world.sql", "004_locale.sql"]);
        assert!(baseline_unknown().is_empty(), "fichiers sans base : {:?}", baseline_unknown());
    }

    #[test]
    fn verdicts() {
        let base = "auth";
        // Table absente ou base de référence absente : jamais initialisée.
        assert!(compare(base, None).not_initialized);
        assert!(compare(base, Some(&[])).not_initialized);
        // À jour : la base de référence et toutes les migrations attendues.
        let mut rows = vec![row(BASELINE, &baseline_checksum(base))];
        rows.extend(expected(base).iter().map(|m| row(m.id, &checksum(m.sql))));
        let ok = compare(base, Some(&rows));
        assert!(ok.usable() && !ok.baseline_changed, "{ok:?}");
        // En avance : une migration inconnue.
        rows.push(row("209901010000_futur", "x"));
        let ahead = compare(base, Some(&rows));
        assert_eq!(ahead.ahead, vec!["209901010000_futur".to_string()]);
        assert!(!ahead.usable());
        // Base de référence modifiée : avertissement seulement.
        let changed = compare(base, Some(&[row(BASELINE, "autre")]));
        assert!(changed.baseline_changed);
        assert_eq!(changed.usable(), expected(base).is_empty());
    }
}
