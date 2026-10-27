//! Contrôles de cohérence des bases, en lecture seule : références entre bases (que des clés étrangères ne
//! peuvent pas porter d'une base à l'autre) et plages d'identifiants.
//!
//! * `archcore db-check` affiche le rapport complet et se termine en erreur s'il y a une anomalie grave.
//! * Au démarrage, le serveur signale chaque anomalie dans le journal sans refuser de partir.

use crate::db::{col, Databases};

/// Gravité d'une anomalie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Donnée inutilisable (référence vers rien, identifiant hors de sa plage).
    Error,
    /// Donnée suspecte mais sans effet bloquant.
    Warning,
}

/// Anomalie trouvée par un contrôle.
#[derive(Debug, Clone)]
pub struct Finding {
    pub label: &'static str,
    pub severity: Severity,
    pub count: i64,
    /// Quelques exemples, pour retrouver les lignes.
    pub samples: Vec<String>,
}

/// Noms des trois bases (configuration).
pub struct Names<'a> {
    pub auth: &'a str,
    pub characters: &'a str,
    pub world: &'a str,
}

/// Plage des CID des PNJ : les personnages des joueurs doivent rester en dessous (voir `world/npcs.rs`).
pub const CHARACTER_ID_LIMIT: i64 = 0x4000_0000;
/// Plage des IID des objets de boutique : les objets en base doivent rester en dessous (`world/items.rs`).
pub const ITEM_ID_LIMIT: i64 = 0x2000_0000;

/// Un contrôle : libellé, gravité, `FROM … WHERE …` des lignes en faute, expression d'un exemple.
struct Check {
    label: &'static str,
    severity: Severity,
    rows: String,
    sample: &'static str,
}

fn checks(n: &Names) -> Vec<Check> {
    let (a, c, w) = (n.auth, n.characters, n.world);
    vec![
        Check {
            label: "personnages sans compte",
            severity: Severity::Error,
            rows: format!("`{c}`.characters x LEFT JOIN `{a}`.account r ON r.id = x.account_id WHERE r.id IS NULL"),
            sample: "CONCAT(x.name, ' (personnage ', x.id, ', compte ', x.account_id, ')')",
        },
        Check {
            label: "personnages au modèle inconnu",
            severity: Severity::Error,
            rows: format!("`{c}`.characters x LEFT JOIN `{w}`.character_template r ON r.tid = x.template_id WHERE r.tid IS NULL"),
            sample: "CONCAT(x.name, ' (TID ', x.template_id, ')')",
        },
        Check {
            label: "objets sans définition dans item_data",
            severity: Severity::Error,
            rows: format!(
                "`{c}`.character_items x LEFT JOIN `{w}`.item_data r ON r.tid = x.template_id \
                 LEFT JOIN `{c}`.characters p ON p.id = x.character_id WHERE r.tid IS NULL"
            ),
            sample: "CONCAT('objet ', x.id, ' (TID ', x.template_id, ') de ', COALESCE(p.name, CONCAT('personnage ', x.character_id)))",
        },
        Check {
            label: "compétences inconnues",
            severity: Severity::Error,
            rows: format!(
                "`{c}`.character_skills x LEFT JOIN `{w}`.skill_template r ON r.tid = x.skill_tid \
                 LEFT JOIN `{c}`.characters p ON p.id = x.character_id WHERE r.tid IS NULL"
            ),
            sample: "CONCAT('TID ', x.skill_tid, ' de ', COALESCE(p.name, CONCAT('personnage ', x.character_id)))",
        },
        Check {
            label: "quêtes absentes de quest_template",
            severity: Severity::Warning,
            rows: format!(
                "`{c}`.character_quests x LEFT JOIN `{w}`.quest_template r ON r.tid = x.quest_tid \
                 LEFT JOIN `{c}`.characters p ON p.id = x.character_id WHERE r.tid IS NULL"
            ),
            sample: "CONCAT('TID ', x.quest_tid, ' de ', COALESCE(p.name, CONCAT('personnage ', x.character_id)))",
        },
        Check {
            label: "recettes absentes de product_compose",
            severity: Severity::Warning,
            rows: format!(
                "`{c}`.character_recipes x LEFT JOIN `{w}`.product_compose r ON r.result_tid = x.recipe_tid \
                 LEFT JOIN `{c}`.characters p ON p.id = x.character_id WHERE r.result_tid IS NULL"
            ),
            sample: "CONCAT('recette ', x.recipe_tid, ' de ', COALESCE(p.name, CONCAT('personnage ', x.character_id)))",
        },
        Check {
            label: "banques sans compte",
            severity: Severity::Warning,
            rows: format!("`{c}`.account_bank x LEFT JOIN `{a}`.account r ON r.id = x.account_id WHERE r.id IS NULL"),
            sample: "CONCAT('compte ', x.account_id)",
        },
        Check {
            label: "langue de compte inconnue",
            severity: Severity::Warning,
            rows: format!("`{a}`.account x LEFT JOIN `{w}`.language r ON r.code = x.language COLLATE utf8mb4_general_ci WHERE x.language IS NOT NULL AND r.code IS NULL"),
            sample: "CONCAT(x.username, ' (', x.language, ')')",
        },
        Check {
            label: "dernier import interrompu",
            severity: Severity::Warning,
            rows: format!("`{w}`.import_run x WHERE x.finished_at IS NULL AND x.id = (SELECT MAX(y.id) FROM `{w}`.import_run y WHERE y.source = x.source)"),
            sample: "CONCAT(x.source, ' (', x.started_at, ')')",
        },
        Check {
            label: "identifiants de personnage dans la plage des PNJ",
            severity: Severity::Error,
            rows: format!("`{c}`.characters x WHERE x.id >= {CHARACTER_ID_LIMIT}"),
            sample: "CONCAT(x.name, ' (', x.id, ')')",
        },
        Check {
            label: "identifiants d'objet dans la plage des boutiques",
            severity: Severity::Error,
            rows: format!("`{c}`.character_items x WHERE x.id >= {ITEM_ID_LIMIT}"),
            sample: "CONCAT('objet ', x.id)",
        },
    ]
}

/// Lance tous les contrôles (lecture seule) ; rend les anomalies trouvées.
pub async fn run(db: &Databases, names: &Names<'_>) -> anyhow::Result<Vec<Finding>> {
    let mut out = Vec::new();
    for k in checks(names) {
        let count: i64 = db.characters.first(&format!("SELECT COUNT(*) AS n FROM {}", k.rows), ()).await?.map(|r| col(&r, "n")).unwrap_or(0);
        if count == 0 {
            continue;
        }
        let samples = db
            .characters
            .rows(&format!("SELECT {} AS s FROM {} LIMIT 5", k.sample, k.rows), ())
            .await?
            .iter()
            .map(|r| col::<String>(r, "s"))
            .collect();
        out.push(Finding { label: k.label, severity: k.severity, count, samples });
    }
    Ok(out)
}

/// Occupation des plages d'identifiants : (libellé, plus grand identifiant, limite).
pub async fn id_usage(db: &Databases, names: &Names<'_>) -> anyhow::Result<Vec<(&'static str, i64, i64)>> {
    let c = names.characters;
    let mut out = Vec::new();
    for (label, sql, limit) in [
        ("personnages", format!("SELECT COALESCE(MAX(id), 0) AS m FROM `{c}`.characters"), CHARACTER_ID_LIMIT),
        ("objets", format!("SELECT COALESCE(MAX(id), 0) AS m FROM `{c}`.character_items"), ITEM_ID_LIMIT),
    ] {
        let max: i64 = db.characters.first(&sql, ()).await?.map(|r| col(&r, "m")).unwrap_or(0);
        out.push((label, max, limit));
    }
    Ok(out)
}

/// Ligne de rapport d'une anomalie.
pub fn describe(f: &Finding) -> String {
    let more = if f.count > f.samples.len() as i64 { ", …" } else { "" };
    format!("{} : {} ({}{more})", f.label, f.count, f.samples.join(", "))
}

/// Contrôle au démarrage : chaque anomalie va dans le journal ; le serveur démarre quand même.
pub async fn at_startup(db: &Databases, names: &Names<'_>) {
    match run(db, names).await {
        Ok(found) if found.is_empty() => crate::info!("cohérence des bases : aucune anomalie"),
        Ok(found) => {
            for f in &found {
                match f.severity {
                    Severity::Error => crate::error!("cohérence des bases : {}", describe(f)),
                    Severity::Warning => crate::warn!("cohérence des bases : {}", describe(f)),
                }
            }
            crate::warn!("cohérence des bases : {} anomalie(s), détail par `archcore db-check` ; le serveur démarre quand même", found.len());
        }
        Err(e) => crate::warn!("cohérence des bases non vérifiée : {e:#}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requetes_des_controles() {
        let n = Names { auth: "a_auth", characters: "a_chars", world: "a_world" };
        let all = checks(&n);
        assert_eq!(all.len(), 11);
        // Chaque contrôle nomme ses bases : il passe par le pool des personnages.
        for k in &all {
            assert!(k.rows.contains("`a_chars`.") || k.rows.contains("`a_auth`.") || k.rows.contains("`a_world`.import_run"), "{}", k.label);
        }
        assert!(all.iter().any(|k| k.rows.contains("`a_world`.item_data")));
        let f = Finding { label: "objets sans définition dans item_data", severity: Severity::Error, count: 7, samples: vec!["objet 1".into()] };
        assert_eq!(describe(&f), "objets sans définition dans item_data : 7 (objet 1, …)");
    }
}
