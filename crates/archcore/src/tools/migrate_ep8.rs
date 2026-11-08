//! `archcore migrate-ep8 [--apply | --restore n]` : met les personnages existants aux valeurs de l'épisode 8, après un
//! `archcore import` (base monde EP8).
//!
//! ```text
//!   Sans option : rapport seulement, par personnage — objets hors EP8, options et runes hors EP8, niveaux de
//!        compétence au-delà du maximum EP8, compétences hors EP8, quêtes (en cours ou terminées) et titres
//!        (possédés ou visés) absents des tables importées (quêtes et titres de Webzen, `import --webzen`).
//!   --apply : une passe numérotée (`ep8_quarantine.run_id`), dans une transaction ; rien n'est détruit :
//!        objet hors EP8 (character_items : inventaire, banque, entrepôt de guilde, courrier, vente) → ligne
//!        d'origine en JSON dans `ep8_quarantine`, avec les courriers et ventes qui le citaient, puis retiré ;
//!        option ou rune hors EP8 → retirée de `option_tids` / `socket_tids` (valeur d'origine gardée) ;
//!        niveau de compétence au-delà du maximum EP8 → ramené au maximum ; compétence hors EP8 → retirée
//!        (gardée) ; quête ou titre disparu → ligne d'origine (character_quests, character_titles,
//!        character_title_goals) en JSON dans `ep8_quarantine`, puis retirée ; « skill init » remis à vide pour tous :
//!        l'oubli gratuit des compétences est offert, les coûts en points des niveaux ayant changé.
//!   --restore n : remet ce que la passe n a changé (objets, quêtes et titres réinsérés, valeurs d'origine).
//! ```
//! Faire une sauvegarde (`mariadb-dump`) des bases avant `--apply`.

use crate::db::{col, Databases};
use mysql_async::prelude::Queryable;
use mysql_async::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Ensembles EP8 lus dans la base monde.
#[derive(Debug, Default, Clone)]
pub struct Sets {
    pub items: HashSet<u32>,
    pub options: HashSet<u32>,
    /// Compétences EP8 (`skill_template`) et niveau maximal de celles qui ont des niveaux (`skill_level`).
    pub skills: HashSet<u32>,
    pub skill_max: HashMap<u32, u32>,
    /// Quêtes (`quest_template`) et titres (`title_template`) importés ; vide : non contrôlés (table non importée).
    pub quests: HashSet<u32>,
    pub titles: HashSet<u32>,
}

/// Quêtes et titres des personnages : (personnage, quête, statut 0 en cours / 1 terminée), (personnage, titre
/// possédé), (personnage, titre visé).
#[derive(Debug, Default, Clone)]
pub struct Progress {
    pub quests: Vec<(u32, u32, u8)>,
    pub titles: Vec<(u32, u32)>,
    pub goals: Vec<(u32, u32)>,
}

/// Objet d'un personnage (`character_items`).
#[derive(Debug, Clone, PartialEq)]
pub struct ItemRow {
    pub id: u64,
    pub character_id: u32,
    pub tid: u32,
    pub option_tids: String,
    pub socket_tids: String,
}

/// Changement d'une passe.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// Objet hors EP8 mis en quarantaine.
    Item { id: u64, tid: u32 },
    Options { id: u64, before: String, after: String },
    Sockets { id: u64, before: String, after: String },
    SkillLevel { tid: u32, from: u32, to: u32 },
    Skill { tid: u32, level: u32 },
    /// Quête absente des tables importées (en cours ou terminée).
    Quest { tid: u32, done: bool },
    /// Titre possédé ou visé absent des tables importées.
    Title { tid: u32 },
    TitleGoal { tid: u32 },
}

fn keep_list(list: &str, keep: impl Fn(u32) -> bool) -> String {
    list.split(',').map(str::trim).filter(|t| !t.is_empty()).filter(|t| t.parse::<u32>().is_ok_and(&keep)).collect::<Vec<_>>().join(",")
}

/// Changements à faire, par personnage : objets, options et runes (TID sertis absents des objets EP8), compétences.
pub fn changes(items: &[ItemRow], skills: &[(u32, u32, u32)], progress: &Progress, sets: &Sets) -> BTreeMap<u32, Vec<Change>> {
    let mut out: BTreeMap<u32, Vec<Change>> = BTreeMap::new();
    for i in items {
        let list = out.entry(i.character_id).or_default();
        if !sets.items.contains(&i.tid) {
            list.push(Change::Item { id: i.id, tid: i.tid });
            continue;
        }
        let options = keep_list(&i.option_tids, |t| sets.options.contains(&t));
        if options != keep_list(&i.option_tids, |_| true) {
            list.push(Change::Options { id: i.id, before: i.option_tids.clone(), after: options });
        }
        let sockets = keep_list(&i.socket_tids, |t| sets.items.contains(&t));
        if sockets != keep_list(&i.socket_tids, |_| true) {
            list.push(Change::Sockets { id: i.id, before: i.socket_tids.clone(), after: sockets });
        }
    }
    for &(cid, tid, level) in skills {
        let list = out.entry(cid).or_default();
        if !sets.skills.contains(&tid) {
            list.push(Change::Skill { tid, level });
        } else if let Some(&max) = sets.skill_max.get(&tid).filter(|m| level > **m) {
            list.push(Change::SkillLevel { tid, from: level, to: max });
        }
    }
    if !sets.quests.is_empty() {
        for &(cid, tid, status) in progress.quests.iter().filter(|q| !sets.quests.contains(&q.1)) {
            out.entry(cid).or_default().push(Change::Quest { tid, done: status != 0 });
        }
    }
    if !sets.titles.is_empty() {
        for &(cid, tid) in progress.titles.iter().filter(|t| !sets.titles.contains(&t.1)) {
            out.entry(cid).or_default().push(Change::Title { tid });
        }
        for &(cid, tid) in progress.goals.iter().filter(|t| !sets.titles.contains(&t.1)) {
            out.entry(cid).or_default().push(Change::TitleGoal { tid });
        }
    }
    out.retain(|_, v| !v.is_empty());
    out
}

/// Ligne du rapport pour un personnage.
pub fn describe(name: &str, list: &[Change]) -> String {
    let (mut items, mut options, mut runes, mut levels, mut skills) = (Vec::new(), 0, 0, Vec::new(), Vec::new());
    let (mut quests, mut titles) = (Vec::new(), Vec::new());
    let count = |a: &str, b: &str| keep_list(a, |_| true).split(',').filter(|t| !t.is_empty()).count() - b.split(',').filter(|t| !t.is_empty()).count();
    for c in list {
        match c {
            Change::Item { tid, .. } => items.push(tid.to_string()),
            Change::Options { before, after, .. } => options += count(before, after),
            Change::Sockets { before, after, .. } => runes += count(before, after),
            Change::SkillLevel { tid, from, to } => levels.push(format!("{tid} {from}→{to}")),
            Change::Skill { tid, .. } => skills.push(tid.to_string()),
            Change::Quest { tid, done } => quests.push(format!("{tid}{}", if *done { " terminée" } else { "" })),
            Change::Title { tid } => titles.push(tid.to_string()),
            Change::TitleGoal { tid } => titles.push(format!("{tid} visé")),
        }
    }
    let list = |v: &[String]| if v.is_empty() { String::new() } else { format!(" [{}]", v.iter().take(12).cloned().collect::<Vec<_>>().join(", ")) };
    let mut line = format!(
        "{name} : {} objet(s) hors EP8{}, {options} option(s) et {runes} rune(s) retirée(s), {} niveau(x) ramené(s){}, {} compétence(s) hors EP8{}",
        items.len(),
        list(&items),
        levels.len(),
        list(&levels),
        skills.len(),
        list(&skills)
    );
    if !quests.is_empty() || !titles.is_empty() {
        line.push_str(&format!(", {} quête(s) disparue(s){}, {} titre(s) disparu(s){}", quests.len(), list(&quests), titles.len(), list(&titles)));
    }
    line
}

async fn load_sets(db: &Databases) -> anyhow::Result<Sets> {
    let mut s = Sets::default();
    s.items = db.world.rows("SELECT tid FROM item_data", ()).await?.iter().map(|r| col(r, "tid")).collect();
    s.skills = db.world.rows("SELECT tid FROM skill_template", ()).await?.iter().map(|r| col(r, "tid")).collect();
    for r in db.world.rows("SELECT tid, MAX(level) AS m FROM skill_level GROUP BY tid", ()).await? {
        s.skill_max.insert(col(&r, "tid"), col(&r, "m"));
    }
    if let Some(r) = db.world.first("SELECT data FROM client_table WHERE name = 'itemoptiontable.txt' AND row_idx = 0", ()).await? {
        let text = serde_json::from_str::<String>(&col::<String>(&r, "data")).unwrap_or_default();
        s.options = super::ep8::tsv_keys(&text, "tid");
    }
    s.quests = db.world.rows("SELECT tid FROM quest_template", ()).await?.iter().map(|r| col(r, "tid")).collect();
    s.titles = db.world.rows("SELECT tid FROM title_template", ()).await?.iter().map(|r| col(r, "tid")).collect();
    anyhow::ensure!(!s.items.is_empty() && !s.skill_max.is_empty(), "base monde vide : lancer `archcore import` d'abord");
    Ok(s)
}

/// Valeur MySQL → JSON (nombres et textes) ; réinsérée telle quelle par `--restore`.
fn to_json(v: &Value) -> serde_json::Value {
    match v {
        Value::NULL => serde_json::Value::Null,
        Value::Int(i) => (*i).into(),
        Value::UInt(u) => (*u).into(),
        Value::Float(f) => (*f as f64).into(),
        Value::Double(f) => (*f).into(),
        Value::Bytes(b) => String::from_utf8_lossy(b).into_owned().into(),
        other => other.as_sql(true).trim_matches('\'').to_string().into(),
    }
}

fn from_json(v: &serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::NULL,
        serde_json::Value::Number(n) => n.as_i64().map(Value::Int).or_else(|| n.as_u64().map(Value::UInt)).unwrap_or_else(|| Value::Double(n.as_f64().unwrap_or(0.0))),
        serde_json::Value::String(s) => Value::Bytes(s.clone().into_bytes()),
        other => Value::Bytes(other.to_string().into_bytes()),
    }
}

pub async fn run(args: &[String]) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let db = Databases::new(&cfg.database);
    let result = match args.first().map(String::as_str) {
        None => report(&db, false).await,
        Some("--apply") => report(&db, true).await,
        Some("--restore") => restore(&db, args.get(1).and_then(|n| n.parse().ok()).ok_or_else(|| anyhow::anyhow!("--restore : numéro de passe attendu"))?).await,
        Some(other) => Err(anyhow::anyhow!("option inconnue {other:?} : archcore migrate-ep8 [--apply | --restore n]")),
    };
    db.close().await;
    result
}

async fn report(db: &Databases, apply: bool) -> anyhow::Result<()> {
    let sets = load_sets(db).await?;
    let items: Vec<ItemRow> = db
        .characters
        .rows("SELECT id, character_id, template_id, option_tids, socket_tids FROM character_items", ())
        .await?
        .iter()
        .map(|r| ItemRow { id: col(r, "id"), character_id: col(r, "character_id"), tid: col(r, "template_id"), option_tids: col(r, "option_tids"), socket_tids: col(r, "socket_tids") })
        .collect();
    let skills: Vec<(u32, u32, u32)> = db.characters.rows("SELECT character_id, skill_tid, level FROM character_skills", ()).await?.iter().map(|r| (col(r, "character_id"), col(r, "skill_tid"), col(r, "level"))).collect();
    let names: HashMap<u32, String> = db.characters.rows("SELECT id, name, skill_init FROM characters", ()).await?.iter().map(|r| (col(r, "id"), col(r, "name"))).collect();
    let pairs = |rows: Vec<mysql_async::Row>, k: &str| -> Vec<(u32, u32)> { rows.iter().map(|r| (col(r, "character_id"), col(r, k))).collect() };
    let progress = Progress {
        quests: db.characters.rows("SELECT character_id, quest_tid, status FROM character_quests", ()).await?.iter().map(|r| (col(r, "character_id"), col(r, "quest_tid"), col(r, "status"))).collect(),
        titles: pairs(db.characters.rows("SELECT character_id, title_tid FROM character_titles", ()).await?, "title_tid"),
        goals: pairs(db.characters.rows("SELECT character_id, title_tid FROM character_title_goals", ()).await?, "title_tid"),
    };
    let plan = changes(&items, &skills, &progress, &sets);
    println!(
        "Personnages : {} ; {} à migrer (base monde : {} objet(s), {} option(s), {} compétence(s) EP8, {} quête(s), {} titre(s))",
        names.len(),
        plan.len(),
        sets.items.len(),
        sets.options.len(),
        sets.skills.len(),
        sets.quests.len(),
        sets.titles.len()
    );
    for (cid, list) in &plan {
        println!("  {}", describe(&format!("{} ({cid})", names.get(cid).map(String::as_str).unwrap_or("?")), list));
    }
    let total = |f: fn(&Change) -> bool| plan.values().flatten().filter(|c| f(c)).count();
    println!(
        "Total : {} objet(s) en quarantaine, {} objet(s) aux options et {} aux runes corrigées, {} niveau(x) ramené(s), {} compétence(s) retirée(s), {} quête(s) et {} titre(s) disparus en quarantaine",
        total(|c| matches!(c, Change::Item { .. })),
        total(|c| matches!(c, Change::Options { .. })),
        total(|c| matches!(c, Change::Sockets { .. })),
        total(|c| matches!(c, Change::SkillLevel { .. })),
        total(|c| matches!(c, Change::Skill { .. })),
        total(|c| matches!(c, Change::Quest { .. })),
        total(|c| matches!(c, Change::Title { .. } | Change::TitleGoal { .. }))
    );
    if !apply {
        println!("Rapport seulement : `archcore migrate-ep8 --apply` applique (sauvegarder les bases avant).");
        return Ok(());
    }
    let run: u32 = db.characters.first("SELECT COALESCE(MAX(run_id), 0) + 1 AS n FROM ep8_quarantine", ()).await?.map(|r| col(&r, "n")).unwrap_or(1);
    let mut tx = db.characters.begin().await?;
    let columns: Vec<String> = tx.query::<mysql_async::Row, _>("SHOW COLUMNS FROM character_items").await?.iter().map(|c| c.get::<String, _>(0).unwrap_or_default()).collect();
    // Ligne d'une table de progression (quêtes, titres) en JSON, retirée : (données, nombre de lignes).
    async fn take_row(tx: &mut mysql_async::Transaction<'_>, table: &str, key: &str, cid: u32, tid: u32) -> anyhow::Result<Option<String>> {
        let cols: Vec<String> = tx.query::<mysql_async::Row, _>(format!("SHOW COLUMNS FROM {table}")).await?.iter().map(|c| c.get::<String, _>(0).unwrap_or_default()).collect();
        let row: Option<mysql_async::Row> = tx.exec_first(format!("SELECT * FROM {table} WHERE character_id = ? AND {key} = ?"), (cid, tid)).await?;
        let Some(row) = row else { return Ok(None) };
        let fields: serde_json::Map<String, serde_json::Value> = cols.iter().enumerate().map(|(i, c)| (c.clone(), row.as_ref(i).map(to_json).unwrap_or_default())).collect();
        tx.exec_drop(format!("DELETE FROM {table} WHERE character_id = ? AND {key} = ?"), (cid, tid)).await?;
        Ok(Some(serde_json::json!({ "table": table, "row": fields }).to_string()))
    }
    let mut rows: Vec<(u32, String, u64, String, String)> = Vec::new();
    for (cid, list) in &plan {
        for c in list {
            match c {
                Change::Item { id, tid } => {
                    let row: Option<mysql_async::Row> = tx.exec_first("SELECT * FROM character_items WHERE id = ?", (id,)).await?;
                    let Some(row) = row else { continue };
                    let fields: serde_json::Map<String, serde_json::Value> = columns.iter().enumerate().map(|(i, c)| (c.clone(), row.as_ref(i).map(to_json).unwrap_or_default())).collect();
                    let mail: Vec<u64> = tx.exec("SELECT id FROM mail WHERE item_id = ?", (id,)).await?;
                    let auction: Vec<u64> = tx.exec("SELECT id FROM auction_sale WHERE item_id = ?", (id,)).await?;
                    let data = serde_json::json!({ "row": fields, "mail": mail, "auction": auction });
                    rows.push((*cid, "item".into(), *id, data.to_string(), format!("objet {tid} hors EP8")));
                    tx.exec_drop("DELETE FROM character_items WHERE id = ?", (id,)).await?;
                }
                Change::Options { id, before, after } => {
                    rows.push((*cid, "options".into(), *id, serde_json::json!({ "option_tids": before }).to_string(), "options hors EP8".into()));
                    tx.exec_drop("UPDATE character_items SET option_tids = ? WHERE id = ?", (after, id)).await?;
                }
                Change::Sockets { id, before, after } => {
                    rows.push((*cid, "sockets".into(), *id, serde_json::json!({ "socket_tids": before }).to_string(), "runes hors EP8".into()));
                    tx.exec_drop("UPDATE character_items SET socket_tids = ? WHERE id = ?", (after, id)).await?;
                }
                Change::SkillLevel { tid, from, to } => {
                    rows.push((*cid, "skill_level".into(), *tid as u64, serde_json::json!({ "level": from }).to_string(), format!("niveau au-delà du maximum EP8 ({to})")));
                    tx.exec_drop("UPDATE character_skills SET level = ? WHERE character_id = ? AND skill_tid = ?", (to, cid, tid)).await?;
                }
                Change::Skill { tid, level } => {
                    rows.push((*cid, "skill".into(), *tid as u64, serde_json::json!({ "level": level }).to_string(), "compétence hors EP8".into()));
                    tx.exec_drop("DELETE FROM character_skills WHERE character_id = ? AND skill_tid = ?", (cid, tid)).await?;
                }
                Change::Quest { tid, done } => {
                    if let Some(data) = take_row(&mut tx, "character_quests", "quest_tid", *cid, *tid).await? {
                        rows.push((*cid, "quest".into(), *tid as u64, data, format!("quête {} absente des tables importées", if *done { "terminée" } else { "en cours" })));
                    }
                }
                Change::Title { tid } => {
                    if let Some(data) = take_row(&mut tx, "character_titles", "title_tid", *cid, *tid).await? {
                        rows.push((*cid, "title".into(), *tid as u64, data, "titre absent des tables importées".into()));
                    }
                }
                Change::TitleGoal { tid } => {
                    if let Some(data) = take_row(&mut tx, "character_title_goals", "title_tid", *cid, *tid).await? {
                        rows.push((*cid, "title_goal".into(), *tid as u64, data, "titre visé absent des tables importées".into()));
                    }
                }
            }
        }
    }
    // Oubli des compétences offert à tous : « skill init » remis à vide.
    let inits: Vec<(u32, String)> = tx.query("SELECT id, skill_init FROM characters WHERE skill_init <> ''").await?;
    for (cid, value) in &inits {
        rows.push((*cid, "skill_init".into(), 0, serde_json::json!({ "skill_init": value }).to_string(), "oubli des compétences offert".into()));
    }
    tx.query_drop("UPDATE characters SET skill_init = '' WHERE skill_init <> ''").await?;
    for chunk in rows.chunks(200) {
        let params: Vec<(u32, u32, &str, u64, &str, &str)> = chunk.iter().map(|(cid, kind, r, data, reason)| (run, *cid, kind.as_str(), *r, data.as_str(), reason.as_str())).collect();
        tx.exec_batch("INSERT INTO ep8_quarantine (run_id, character_id, kind, ref_id, data, reason) VALUES (?, ?, ?, ?, ?, ?)", params).await?;
    }
    tx.commit().await?;
    println!("✓ passe {run} appliquée : {} ligne(s) dans ep8_quarantine ({} oubli(s) des compétences offert(s)) ; `archcore migrate-ep8 --restore {run}` l'annule", rows.len(), inits.len());
    Ok(())
}

async fn restore(db: &Databases, run: u32) -> anyhow::Result<()> {
    let rows = db.characters.rows("SELECT id, character_id, kind, ref_id, data FROM ep8_quarantine WHERE run_id = ? AND restored_at IS NULL ORDER BY id DESC", (run,)).await?;
    anyhow::ensure!(!rows.is_empty(), "passe {run} : rien à remettre");
    let mut tx = db.characters.begin().await?;
    for r in &rows {
        let (cid, kind, ref_id): (u32, String, u64) = (col(r, "character_id"), col(r, "kind"), col(r, "ref_id"));
        let data: serde_json::Value = serde_json::from_str(&col::<String>(r, "data")).unwrap_or_default();
        let text = |k: &str| data.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let level = data.get("level").and_then(|v| v.as_u64()).unwrap_or(1);
        match kind.as_str() {
            "item" => {
                let Some(fields) = data.get("row").and_then(|v| v.as_object()) else { continue };
                let cols: Vec<&String> = fields.keys().collect();
                let sql = format!("INSERT INTO character_items ({}) VALUES ({})", cols.iter().map(|c| format!("`{c}`")).collect::<Vec<_>>().join(","), vec!["?"; cols.len()].join(","));
                tx.exec_drop(sql, fields.values().map(from_json).collect::<Vec<_>>()).await?;
                for (table, key) in [("mail", "mail"), ("auction_sale", "auction")] {
                    for id in data.get(key).and_then(|v| v.as_array()).into_iter().flatten().filter_map(|v| v.as_u64()) {
                        tx.exec_drop(format!("UPDATE {table} SET item_id = ? WHERE id = ?"), (ref_id, id)).await?;
                    }
                }
            }
            "options" => tx.exec_drop("UPDATE character_items SET option_tids = ? WHERE id = ?", (text("option_tids"), ref_id)).await?,
            "sockets" => tx.exec_drop("UPDATE character_items SET socket_tids = ? WHERE id = ?", (text("socket_tids"), ref_id)).await?,
            "skill_level" => tx.exec_drop("UPDATE character_skills SET level = ? WHERE character_id = ? AND skill_tid = ?", (level, cid, ref_id)).await?,
            "skill" => tx.exec_drop("INSERT IGNORE INTO character_skills (character_id, skill_tid, level) VALUES (?, ?, ?)", (cid, ref_id, level)).await?,
            "skill_init" => tx.exec_drop("UPDATE characters SET skill_init = ? WHERE id = ?", (text("skill_init"), cid)).await?,
            // Quête ou titre : ligne d'origine réinsérée dans sa table.
            "quest" | "title" | "title_goal" => {
                let (Some(table), Some(fields)) = (data.get("table").and_then(|v| v.as_str()), data.get("row").and_then(|v| v.as_object())) else { continue };
                let cols: Vec<&String> = fields.keys().collect();
                let sql = format!("INSERT IGNORE INTO {table} ({}) VALUES ({})", cols.iter().map(|c| format!("`{c}`")).collect::<Vec<_>>().join(","), vec!["?"; cols.len()].join(","));
                tx.exec_drop(sql, fields.values().map(from_json).collect::<Vec<_>>()).await?
            }
            _ => continue,
        }
    }
    tx.exec_drop("UPDATE ep8_quarantine SET restored_at = CURRENT_TIMESTAMP WHERE run_id = ? AND restored_at IS NULL", (run,)).await?;
    tx.commit().await?;
    println!("✓ passe {run} remise : {} changement(s) annulé(s)", rows.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rapport_de_migration() {
        let sets = Sets { items: [10, 463, 20].into(), options: [5, 6].into(), skills: [30, 31].into(), skill_max: [(30, 15), (31, 20)].into(), ..Default::default() };
        let item = |id, cid, tid, o: &str, s: &str| ItemRow { id, character_id: cid, tid, option_tids: o.into(), socket_tids: s.into() };
        let items = [item(1, 7, 10, "5,13437,6", "463,15000"), item(2, 7, 14611, "13437", ""), item(3, 8, 20, "5", "463"), item(4, 8, 10, "", "")];
        let skills = [(7, 30, 20), (7, 31, 20), (8, 4000, 3), (8, 30, 15)];
        let plan = changes(&items, &skills, &Progress::default(), &sets);
        assert_eq!(
            plan[&7],
            vec![
                Change::Options { id: 1, before: "5,13437,6".into(), after: "5,6".into() },
                Change::Sockets { id: 1, before: "463,15000".into(), after: "463".into() },
                Change::Item { id: 2, tid: 14611 },
                Change::SkillLevel { tid: 30, from: 20, to: 15 },
            ],
            "objet hors EP8 en quarantaine sans autre changement ; 20 → 15 niveaux"
        );
        assert_eq!(plan[&8], vec![Change::Skill { tid: 4000, level: 3 }]);
        assert_eq!(
            describe("Grokk", &plan[&7]),
            "Grokk : 1 objet(s) hors EP8 [14611], 1 option(s) et 1 rune(s) retirée(s), 1 niveau(x) ramené(s) [30 20→15], 0 compétence(s) hors EP8"
        );
        assert!(changes(&[item(4, 8, 10, "5", "")], &[(8, 30, 15)], &Progress::default(), &sets).is_empty(), "rien à migrer");
        // Quêtes et titres disparus (quêtes et titres de Webzen importés) ; table vide : rien n'est contrôlé.
        let progress = Progress { quests: vec![(7, 2, 1), (7, 4599, 0), (8, 4621, 1)], titles: vec![(7, 140_001), (8, 11_000)], goals: vec![(7, 36)] };
        let with = Sets { quests: [2].into(), titles: [140_001].into(), ..sets.clone() };
        let plan = changes(&[], &[], &progress, &with);
        assert_eq!(plan[&7], vec![Change::Quest { tid: 4599, done: false }, Change::TitleGoal { tid: 36 }]);
        assert_eq!(plan[&8], vec![Change::Quest { tid: 4621, done: true }, Change::Title { tid: 11_000 }]);
        assert!(describe("Grokk", &plan[&8]).ends_with("1 quête(s) disparue(s) [4621 terminée], 1 titre(s) disparu(s) [11000]"));
        assert!(changes(&[], &[], &progress, &sets).is_empty(), "quêtes et titres non importés : non contrôlés");
        assert_eq!((to_json(&Value::UInt(7)), from_json(&serde_json::json!("a"))), (serde_json::json!(7), Value::Bytes(b"a".to_vec())));
    }
}
