//! `archcore import [dossier] [--server dossier]` : importe les tables déchiffrées du client
//! (`data/client-ini`) dans `archcore_world`. Les fichiers sont en CP949 (coréen) ; ils sont convertis en
//! UTF-8. Avec `--server` (par défaut `data/server-ini` s'il existe), importe aussi, depuis les tables du
//! serveur d'origine, les apparitions de monstres (`spawndatatable.txt`, `spawninstancetable.txt`), la carte
//! (`world/node_data`), le butin (`itemdroptable.txt`, `groupdroprate.txt`, `droprankrate.txt`) et l'IA des
//! monstres (`aidatatable.txt` : compétences qu'ils lancent).

use crate::db::Databases;
use mysql_async::prelude::Queryable;
use mysql_async::Value;
use serde_json::{json, Map};
use std::collections::HashMap;
use std::path::PathBuf;

/// `Number(String(v).trim())` de JavaScript : vide → 0, hexadécimal accepté, sinon `d`.
pub fn js_number(v: &str, d: f64) -> f64 {
    let t = v.trim();
    if t.is_empty() {
        return 0.0;
    }
    let parsed = if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        i64::from_str_radix(h, 16).ok().map(|n| n as f64)
    } else if t.chars().all(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | 'e' | 'E')) {
        t.parse::<f64>().ok()
    } else {
        None
    };
    parsed.filter(|n| n.is_finite()).unwrap_or(d)
}

fn num(v: Option<&str>) -> f64 {
    js_number(v.unwrap_or(""), 0.0)
}

fn is_uint(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

/// Section d'un .ini ArchLord : identifiant, clés dans l'ordre, valeurs (répétées si la clé revient).
pub struct Section {
    pub id: String,
    pub order: Vec<(String, String)>,
}

impl Section {
    pub fn all(&self, key: &str) -> Vec<&str> {
        self.order.iter().filter(|(k, _)| k == key).map(|(_, v)| v.as_str()).collect()
    }
    pub fn first(&self, key: &str) -> Option<&str> {
        self.order.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
    /// Clés → valeur, ou liste de valeurs si la clé est répétée (comme l'import d'origine).
    pub fn keys_json(&self) -> serde_json::Value {
        let mut m: Map<String, serde_json::Value> = Map::new();
        for (k, v) in &self.order {
            match m.get_mut(k) {
                None => {
                    m.insert(k.clone(), json!(v));
                }
                Some(serde_json::Value::Array(a)) => a.push(json!(v)),
                Some(prev) => *prev = json!([prev.clone(), v]),
            }
        }
        serde_json::Value::Object(js_key_order(m))
    }
}

/// Ordre des propriétés d'un objet JavaScript : clés numériques (index de tableau) d'abord, en ordre
/// croissant, puis les autres dans l'ordre d'insertion — pour un JSON identique à l'import d'origine.
fn js_key_order(m: Map<String, serde_json::Value>) -> Map<String, serde_json::Value> {
    let is_index = |k: &str| k == "0" || (!k.starts_with('0') && k.parse::<u32>().is_ok_and(|n| n < u32::MAX));
    let (mut idx, other): (Vec<_>, Vec<_>) = m.into_iter().partition(|(k, _)| is_index(k));
    idx.sort_by_key(|(k, _)| k.parse::<u32>().unwrap());
    idx.into_iter().chain(other).collect()
}

pub fn parse_ini(text: &str) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    for raw in super::lines(text) {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') && line.len() > 2 {
            sections.push(Section { id: line[1..line.len() - 1].to_string(), order: Vec::new() });
            continue;
        }
        let (Some(cur), Some(eq)) = (sections.last_mut(), line.find('=')) else { continue };
        cur.order.push((line[..eq].trim().to_string(), line[eq + 1..].trim().to_string()));
    }
    sections
}

/// Nom → TID des compétences (SKILLTEMPLATE.ini, clé 1 = nom).
pub fn skill_tids(template_ini: &str) -> HashMap<String, u32> {
    parse_ini(template_ini)
        .iter()
        .filter(|s| is_uint(&s.id))
        .filter_map(|s| Some((s.first("1")?.trim().to_string(), s.id.parse::<u32>().ok()?)))
        .collect()
}

/// Colonnes non vides d'une ligne, en objet JSON (en-tête → texte).
fn row_object(header: &[String], row: &[String], skip: usize) -> Map<String, serde_json::Value> {
    let mut o = Map::new();
    for (i, h) in header.iter().enumerate().skip(skip) {
        if let Some(c) = row.get(i).map(|c| c.trim()).filter(|c| !c.is_empty()) {
            o.insert(h.clone(), json!(c));
        }
    }
    o
}

/// Les colonnes qui suivent les marqueurs FACTOR_POINT, FACTOR_PERCENT et FACTOR_MAGNIFY de SKILL_CONST.txt
/// répètent les mêmes noms (str, movement…) : on les préfixe de « point. », « percent. » et « magnify. ».
pub fn group_columns(header: &[String]) -> Vec<String> {
    let mut prefix = String::new();
    header
        .iter()
        .map(|h| match h.strip_prefix("FACTOR_") {
            Some(group) => {
                prefix = format!("{}.", group.to_lowercase());
                h.clone()
            }
            None => format!("{prefix}{h}"),
        })
        .collect()
}

/// SKILL_SPEC.txt : une ligne par compétence, nommée en colonne 0. Rend (tid, nom, colonnes) et le nombre de
/// noms sans modèle.
pub fn parse_skill_specs(text: &str, tids: &HashMap<String, u32>) -> (Vec<(u32, String, serde_json::Value)>, usize) {
    let (header, rows) = parse_tsv(text);
    let (mut out, mut missing) = (Vec::new(), 0);
    for r in &rows {
        let name = r.first().map(|n| n.trim()).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        match tids.get(name) {
            Some(&tid) => out.push((tid, name.to_string(), serde_json::Value::Object(row_object(&header, r, 1)))),
            None => missing += 1,
        }
    }
    (out, missing)
}

/// SKILL_CONST.txt : le nom figure sur la première ligne d'une compétence, les niveaux suivants le laissent
/// vide. Rend (tid, niveau, colonnes) et le nombre de noms sans modèle.
pub fn parse_skill_levels(text: &str, tids: &HashMap<String, u32>) -> (Vec<(u32, u32, serde_json::Value)>, usize) {
    let (header, rows) = parse_tsv(text);
    let header = group_columns(&header);
    let level_col = header.iter().position(|h| h == "skill_level").unwrap_or(1);
    let (mut out, mut missing, mut current) = (Vec::new(), 0, None);
    for r in &rows {
        let name = r.first().map(|n| n.trim()).unwrap_or("");
        if !name.is_empty() {
            current = tids.get(name).copied();
            if current.is_none() {
                missing += 1;
            }
        }
        let (Some(tid), Some(level)) = (current, r.get(level_col).and_then(|l| l.trim().parse::<u32>().ok())) else { continue };
        let mut o = row_object(&header, r, 1);
        o.remove("skill_level");
        out.push((tid, level, serde_json::Value::Object(o)));
    }
    (out, missing)
}

/// questtemplate.ini (tabulé, une quête par ligne) : (tid, nom, colonnes). Une quête répétée garde sa
/// dernière ligne.
pub fn parse_quests(text: &str) -> Vec<(u32, String, serde_json::Value)> {
    let (header, rows) = parse_tsv(text);
    let mut by_tid: std::collections::BTreeMap<u32, (String, serde_json::Value)> = Default::default();
    for r in &rows {
        let Some(tid) = r.first().and_then(|t| t.trim().parse::<u32>().ok()) else { continue };
        let o = row_object(&header, r, 1);
        let name = o.get("QuestName").and_then(|n| n.as_str()).unwrap_or("").to_string();
        by_tid.insert(tid, (name, serde_json::Value::Object(o)));
    }
    by_tid.into_iter().map(|(tid, (name, data))| (tid, name, data)).collect()
}

/// questgroup.ini (tabulé, sans en-tête) : groupe, nom, quêtes proposées et terminées (« 4611:4612: »).
pub fn parse_quest_groups(text: &str) -> Vec<(u32, String, Vec<u32>, Vec<u32>)> {
    let list = |s: Option<&str>| s.unwrap_or("").split(':').filter_map(|t| t.trim().parse::<u32>().ok()).collect::<Vec<u32>>();
    super::lines(text)
        .filter_map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            let id = c.first()?.trim().parse::<u32>().ok()?;
            Some((id, c.get(1).unwrap_or(&"").trim().to_string(), list(c.get(2).copied()), list(c.get(3).copied())))
        })
        .collect()
}

/// .txt tabulé avec en-tête : (en-tête, lignes).
pub fn parse_tsv(text: &str) -> (Vec<String>, Vec<Vec<String>>) {
    let mut it = super::lines(text).filter(|l| !l.trim().is_empty());
    let header: Vec<String> = it.next().unwrap_or("").split('\t').map(|h| h.trim().to_string()).collect();
    let rows = it.map(|l| l.split('\t').map(str::to_string).collect()).collect();
    (header, rows)
}

struct Source(PathBuf);

impl Source {
    fn read(&self, name: &str) -> anyhow::Result<String> {
        let bytes = std::fs::read(self.0.join(name)).map_err(|e| anyhow::anyhow!("{name} : {e}"))?;
        Ok(encoding_rs::EUC_KR.decode(&bytes).0.into_owned())
    }
    fn exists(&self, name: &str) -> bool {
        self.0.join(name).exists()
    }
}

fn v<T: Into<Value>>(x: T) -> Value {
    x.into()
}

/// Remplace le contenu d'une table en une transaction : interrompu, l'import laisse la table telle qu'elle
/// était.
async fn replace(db: &Databases, table: &str, sql: &str, rows: Vec<Vec<Value>>) -> anyhow::Result<usize> {
    let n = rows.len();
    let mut tx = db.world.begin().await?;
    tx.query_drop(format!("DELETE FROM {table}")).await?;
    for chunk in rows.chunks(500) {
        tx.exec_batch(sql, chunk.to_vec()).await?;
    }
    tx.commit().await?;
    Ok(n)
}

pub async fn run(args: &[String]) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let mut positional = Vec::new();
    let mut server_dir = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--server" {
            server_dir = Some(PathBuf::from(it.next().ok_or_else(|| anyhow::anyhow!("--server : dossier attendu"))?));
        } else {
            positional.push(a);
        }
    }
    let dir = positional.first().map(PathBuf::from).unwrap_or_else(|| cfg.root.join("data").join("client-ini"));
    let server_dir = server_dir.or_else(|| Some(cfg.root.join("data").join("server-ini")).filter(|d| d.exists()));
    let src = Source(dir.clone());
    let db = Databases::new(&cfg.database);
    use super::content::{begin_run, finish_run, fingerprint};
    // Chaque source lue est notée dans import_run (empreinte, version) ; finished_at reste vide si l'import
    // s'arrête en route.
    let (n, sum) = fingerprint(&dir, |_| true);
    let run = begin_run(&db, "client", &dir, n, &sum).await?;
    import_all(&db, &src).await?;
    finish_run(&db, run).await?;
    let game = cfg.root.join(&cfg.client.data_dir);
    let (n, sum) = fingerprint(&game.join("world"), |f| f.starts_with('b') && f.ends_with("x.ma2"));
    let run = begin_run(&db, "relief", &game.join("world"), n, &sum).await?;
    import_terrain(&db, &game).await?;
    finish_run(&db, run).await?;
    match server_dir {
        Some(d) => {
            let (n, sum) = fingerprint(&d, |_| true);
            let run = begin_run(&db, "serveur", &d, n, &sum).await?;
            import_spawns(&db, &src, &Source(d.clone())).await?;
            import_map(&db, &d).await?;
            import_loot(&db, &Source(d.clone())).await?;
            import_monster_ai(&db, &Source(d.clone())).await?;
            finish_run(&db, run).await?;
        }
        None => println!("· monster_spawn : pas de tables du serveur (data/server-ini ou --server), ignoré"),
    }
    // Couche de contenu d'ArchCore (content/*.sql), par-dessus les tables tout juste régénérées.
    super::content::apply(&db, &cfg.root).await?;
    db.close().await;
    Ok(())
}

/// Ligne d'apparition prête à insérer : spawndatatable.txt (quoi) jointe à spawninstancetable.txt (où).
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnRow {
    pub id: u32,
    /// Clé stable : « point#rang », rang de la ligne parmi celles du même point, dans l'ordre des tables.
    pub key: String,
    pub group: String,
    pub name: String,
    pub tid: u32,
    pub level: i64,
    pub count: i64,
    pub max_count: i64,
    pub respawn_sec: i64,
    pub rate: i64,
    pub ai_tid: i64,
    pub siege_war: i64,
    pub is_boss: i64,
    pub pos: [f64; 3],
    pub degree: [f64; 2],
    pub radius: f64,
}

/// Joint les deux tables par nom de point. Un nom peut avoir plusieurs positions : la ligne est alors
/// reprise à chacune. Rend les lignes et le nombre de lignes sans aucune position.
pub fn join_spawns(data: &str, instances: &str) -> (Vec<SpawnRow>, usize) {
    let (ih, irows) = parse_tsv(instances);
    let icol = |n: &str| ih.iter().position(|h| h == n);
    let (i_name, i_pos, i_rot, i_rad) = (icol("Name"), icol("Position"), icol("Rotation"), icol("Radius"));
    let cell = |r: &[String], c: Option<usize>| c.and_then(|i| r.get(i)).map(|x| x.trim().to_string()).unwrap_or_default();
    let floats = |t: &str| t.split(',').map(|x| js_number(x, 0.0)).collect::<Vec<f64>>();
    let mut places: std::collections::HashMap<String, Vec<([f64; 3], [f64; 2], f64)>> = std::collections::HashMap::new();
    for r in &irows {
        let name = cell(r, i_name);
        if name.is_empty() {
            continue;
        }
        let p = floats(&cell(r, i_pos));
        let d = floats(&cell(r, i_rot));
        let g = |a: &Vec<f64>, i: usize| a.get(i).copied().unwrap_or(0.0);
        places.entry(name).or_default().push(([g(&p, 0), g(&p, 1), g(&p, 2)], [g(&d, 0), g(&d, 1)], js_number(&cell(r, i_rad), 0.0)));
    }
    let (dh, drows) = parse_tsv(data);
    let dcol = |n: &str| dh.iter().position(|h| h == n);
    let c = ["GroupName", "Name", "TotalMobCount", "Spawntime", "MobTID", "MobLV", "Rate", "MaxMobCount", "MobAITID", "SiegeWarType", "IsBoss"].map(dcol);
    let (mut out, mut missing) = (Vec::new(), 0);
    let mut ranks: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for r in &drows {
        let n = |i: usize| js_number(&cell(r, c[i]), 0.0) as i64;
        let name = cell(r, c[1]);
        let tid = n(4);
        if name.is_empty() || tid <= 0 {
            continue;
        }
        let Some(at) = places.get(&name) else {
            missing += 1;
            continue;
        };
        for &(pos, degree, radius) in at {
            let rank = ranks.entry(name.clone()).or_insert(0);
            *rank += 1;
            out.push(SpawnRow {
                id: out.len() as u32 + 1,
                key: format!("{name}#{rank}"),
                group: cell(r, c[0]),
                name: name.clone(),
                tid: tid as u32,
                level: n(5),
                count: n(2),
                max_count: n(7),
                respawn_sec: n(3),
                rate: if cell(r, c[6]).is_empty() { 10_000 } else { n(6) },
                ai_tid: n(8),
                siege_war: n(9),
                is_boss: n(10),
                pos,
                degree,
                radius,
            });
        }
    }
    (out, missing)
}

/// `node_data` : à côté des tables, dans `world/`, ou dans le `world/` voisin du dossier des tables.
fn find_node_data(dir: &std::path::Path) -> Option<PathBuf> {
    [dir.join("node_data"), dir.join("world").join("node_data"), dir.join("..").join("world").join("node_data")].into_iter().find(|p| p.is_file())
}

/// Découpe `node_data` : enregistrements de 1032 octets (secteur X u32, secteur Z u32, 256 segments u32).
pub fn parse_node_data(bytes: &[u8]) -> Vec<(u16, u16, Vec<u8>)> {
    bytes
        .chunks_exact(1032)
        .filter_map(|r| {
            let x = u32::from_le_bytes(r[0..4].try_into().unwrap());
            let z = u32::from_le_bytes(r[4..8].try_into().unwrap());
            (x < 800 && z < 800).then(|| (x as u16, z as u16, r[8..].to_vec()))
        })
        .collect()
}

async fn import_map(db: &Databases, server_dir: &std::path::Path) -> anyhow::Result<()> {
    let Some(path) = find_node_data(server_dir) else {
        println!("· map_segment : node_data introuvable près de {}, ignoré", server_dir.display());
        return Ok(());
    };
    let bytes = std::fs::read(&path).map_err(|e| anyhow::anyhow!("{} : {e}", path.display()))?;
    let out: Vec<Vec<Value>> = parse_node_data(&bytes).into_iter().map(|(x, z, segs)| vec![v(x), v(z), v(segs)]).collect();
    let n = replace(db, "map_segment", "INSERT INTO map_segment (sector_x,sector_z,segments) VALUES (?,?,?)", out).await?;
    println!("✓ map_segment : {n} secteur(s)");
    Ok(())
}

/// Hauteurs du sol, lues dans la géométrie du client (`world/b*x.ma2` du dossier Game).
async fn import_terrain(db: &Databases, game_dir: &std::path::Path) -> anyhow::Result<()> {
    if !game_dir.join("world").is_dir() {
        println!("· map_height : pas de dossier world/ dans {}, ignoré", game_dir.display());
        return Ok(());
    }
    let (sectors, bad) = super::terrain::read_world(game_dir)?;
    let out: Vec<Vec<Value>> = sectors.into_iter().map(|(x, z, h)| vec![v(x), v(z), v(h)]).collect();
    let n = replace(db, "map_height", "INSERT INTO map_height (sector_x,sector_z,heights) VALUES (?,?,?)", out).await?;
    println!("✓ map_height : {n} secteur(s){}", if bad > 0 { format!(", {bad} illisible(s)") } else { String::new() });
    Ok(())
}

/// Régions de RegionTemplate.ini : (numéro, nom, région parente, champ Type).
pub fn parse_regions(text: &str) -> Vec<(u32, String, i64, i64)> {
    parse_ini(text)
        .iter()
        .filter(|s| is_uint(&s.id))
        .map(|s| (s.id.parse().unwrap(), s.first("Name").unwrap_or("").to_string(), js_number(s.first("ParentIndex").unwrap_or("-1"), -1.0) as i64, num(s.first("Type")) as i64))
        .collect()
}

/// Événement « binding » (fonction 19) d'un objet d'obj*.ini.
#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
    /// Section de l'objet dans son fichier obj*.ini.
    pub object: String,
    pub name: String,
    pub town: String,
    pub binding_type: i64,
    pub race: i64,
    pub class: i64,
    pub pos: [f64; 3],
}

/// Race ou classe exigée par un point : -1 (ou 4294967295, -1 sur 32 bits) et 0 veulent dire « toutes ».
fn restriction(v: &str) -> i64 {
    let n = js_number(v, 0.0);
    if n > 0.0 && n < 1000.0 { n as i64 } else { 0 }
}

/// Événements « binding » des objets d'un fichier obj*.ini, avec la position de leur objet.
pub fn parse_bindings(text: &str) -> Vec<Binding> {
    let mut out = Vec::new();
    for s in parse_ini(text) {
        let Some(p) = s.first("Position") else { continue };
        let pos: Vec<f64> = p.split(',').map(|x| js_number(x, 0.0)).collect();
        let pos = [pos.first().copied().unwrap_or(0.0), pos.get(1).copied().unwrap_or(0.0), pos.get(2).copied().unwrap_or(0.0)];
        let mut current: Option<Binding> = None;
        for (k, val) in &s.order {
            match k.as_str() {
                "Function" => {
                    out.extend(current.take());
                    if val.trim() == "19" {
                        current = Some(Binding { object: s.id.clone(), name: String::new(), town: String::new(), binding_type: 0, race: 0, class: 0, pos });
                    }
                }
                "BindingEnd" => out.extend(current.take()),
                _ => {
                    if let Some(b) = current.as_mut() {
                        match k.as_str() {
                            "BindingName" => b.name = val.clone(),
                            "TownName" => b.town = val.clone(),
                            "BindingType" => b.binding_type = js_number(val, 0.0) as i64,
                            "CharRaceType" => b.race = restriction(val),
                            "CharClassType" => b.class = restriction(val),
                            _ => {}
                        }
                    }
                }
            }
        }
        out.extend(current.take());
    }
    out
}

/// Lignes de butin propre à un monstre : (monstre, objet, min, max, taux sur 100 000).
pub fn parse_monster_drops(text: &str) -> Vec<(u32, u32, i64, i64, i64)> {
    let (h, rows) = parse_tsv(text);
    let c = ["MonsterTID", "ItemTID", "Min", "Max", "Rate"].map(|n| h.iter().position(|x| x == n));
    let cell = |r: &[String], i: Option<usize>| js_number(i.and_then(|i| r.get(i)).map(String::as_str).unwrap_or(""), 0.0) as i64;
    rows.iter()
        .filter_map(|r| {
            let (monster, item) = (cell(r, c[0]), cell(r, c[1]));
            let min = cell(r, c[2]).max(1);
            (monster > 0 && item > 0).then(|| (monster as u32, item as u32, min, cell(r, c[3]).max(min), cell(r, c[4]).clamp(0, 100_000)))
        })
        .collect()
}

/// Clés stables du butin : « monstre:objet#rang », rang parmi les lignes du même couple.
pub fn drop_keys(rows: &[(u32, u32, i64, i64, i64)]) -> Vec<String> {
    let mut ranks: std::collections::HashMap<(u32, u32), u32> = std::collections::HashMap::new();
    rows.iter()
        .map(|r| {
            let rank = ranks.entry((r.0, r.1)).or_insert(0);
            *rank += 1;
            format!("{}:{}#{rank}", r.0, r.1)
        })
        .collect()
}

/// Point de téléportation de TeleportPoint.ini.
#[derive(Clone, Debug, PartialEq)]
pub struct TeleportPointRow {
    pub name: String,
    pub target_type: i64,
    pub pos: [f64; 3],
    pub radius_min: f64,
    pub radius_max: f64,
    pub region_type: i64,
    pub special_type: i64,
    pub use_type: i64,
    pub groups: Vec<String>,
    pub target_groups: Vec<String>,
}

/// Points de TeleportPoint.ini : une section par point ; « Group » (répété) donne les groupes où il sert
/// d'arrivée, « TargetGroup » ceux vers lesquels il mène, « PointPos » (x:y:z) la position d'un point fixe.
pub fn parse_teleport_points(text: &str) -> Vec<TeleportPointRow> {
    parse_ini(text)
        .iter()
        .map(|s| {
            let pos: Vec<f64> = s.first("PointPos").unwrap_or("").split(':').map(|x| js_number(x, 0.0)).collect();
            let strings = |key: &str| s.all(key).into_iter().filter(|v| !v.is_empty()).map(str::to_string).collect();
            TeleportPointRow {
                name: s.id.clone(),
                target_type: js_number(s.first("PointTargetType").unwrap_or(""), 2.0) as i64,
                pos: [pos.first().copied().unwrap_or(0.0), pos.get(1).copied().unwrap_or(0.0), pos.get(2).copied().unwrap_or(0.0)],
                radius_min: num(s.first("RadiusMin")),
                radius_max: num(s.first("RadiusMax")),
                region_type: num(s.first("RegionType")) as i64,
                special_type: num(s.first("SpecialType")) as i64,
                use_type: num(s.first("UseType")) as i64,
                groups: strings("Group"),
                target_groups: strings("TargetGroup"),
            }
        })
        .collect()
}

/// Objets d'un fichier obj*.ini qui portent un point de téléportation (événement 8, clé « PointName ») :
/// (objet, point, position de l'objet).
pub fn parse_portals(text: &str) -> Vec<(u32, String, [f64; 3])> {
    let mut out = Vec::new();
    for s in parse_ini(text).iter().filter(|s| is_uint(&s.id)) {
        let Some(p) = s.first("Position") else { continue };
        let p: Vec<f64> = p.split(',').map(|x| js_number(x, 0.0)).collect();
        let pos = [p.first().copied().unwrap_or(0.0), p.get(1).copied().unwrap_or(0.0), p.get(2).copied().unwrap_or(0.0)];
        let mut teleport = false;
        for (k, val) in &s.order {
            match k.as_str() {
                "Function" => teleport = val.trim() == "8",
                "PointName" if teleport && !val.is_empty() => out.push((s.id.parse().unwrap(), val.clone(), pos)),
                _ => {}
            }
        }
    }
    out
}

/// Prix des trajets de TeleportFee.txt : (départ, arrivée, prix) ; un couple répété garde sa dernière ligne.
pub fn parse_teleport_fees(text: &str) -> Vec<(String, String, i64)> {
    let (h, rows) = parse_tsv(text);
    let c = ["start_portal", "finish_portal", "cost"].map(|n| h.iter().position(|x| x == n));
    let cell = |r: &[String], i: Option<usize>| i.and_then(|i| r.get(i)).map(|s| s.trim().to_string()).unwrap_or_default();
    let mut fees: Vec<(String, String, i64)> = Vec::new();
    for r in &rows {
        let (from, to) = (cell(r, c[0]), cell(r, c[1]));
        let cost = cell(r, c[2]);
        let fee = js_number(&cost, -1.0) as i64;
        if from.is_empty() || to.is_empty() || cost.is_empty() || fee < 0 {
            continue;
        }
        fees.retain(|(a, b, _)| !(*a == from && *b == to));
        fees.push((from, to, fee));
    }
    fees
}

/// Délais de réapparition des boss de la fenêtre des boss d'Archonia (UIBoss.xml) : (TID, minimum, maximum)
/// en secondes, de « Spawn Period: 30-45 minutes ». Les boss « Event-based spawn » n'ont pas de délai.
pub fn parse_boss_periods(xml: &str) -> Vec<(u32, i64, i64)> {
    let attr = |s: &str, name: &str| -> Option<String> {
        let at = s.find(&format!("{name}=\""))? + name.len() + 2;
        Some(s[at..].split('"').next()?.to_string())
    };
    let mut out = Vec::new();
    for boss in xml.split("<Boss ").skip(1) {
        let boss = boss.split("</Boss>").next().unwrap_or("");
        let Some(tid) = attr(boss, "TID").and_then(|t| t.trim().parse::<u32>().ok()) else { continue };
        let Some(period) = boss.find("<SpawnPeriod").and_then(|i| attr(&boss[i..], "Text")) else { continue };
        let minutes: Vec<i64> = period
            .trim_start_matches("Spawn Period:")
            .trim()
            .trim_end_matches("minutes")
            .split('-')
            .filter_map(|m| m.trim().parse().ok())
            .collect();
        if let [min, max] = minutes[..] {
            out.push((tid, min * 60, max.max(min) * 60));
        }
    }
    out
}

/// Prix des pages de banque en plus (bankslotprice.txt) : (page, prix), -1 pour une page pas en vente.
pub fn parse_bank_slot_prices(text: &str) -> Vec<(u8, i64)> {
    let (h, rows) = parse_tsv(text);
    let c = ["Slot", "Price"].map(|n| h.iter().position(|x| x == n));
    rows.iter()
        .filter_map(|r| {
            let cell = |i: Option<usize>| i.and_then(|i| r.get(i)).map(|s| s.trim().to_string()).unwrap_or_default();
            let slot = js_number(&cell(c[0]), 0.0);
            (slot >= 1.0 && slot <= 255.0).then(|| (slot as u8, js_number(&cell(c[1]), -1.0) as i64))
        })
        .collect()
}

/// Tailles de guilde de guildmaxmember.txt : (taille, or, TID et nombre de crânes exigés). La première
/// colonne, sans titre, est la taille ; les colonnes suivantes sont lues à leur place.
pub fn parse_guild_max_members(text: &str) -> Vec<(i64, i64, i64, i64)> {
    let (_, rows) = parse_tsv(text);
    rows.iter()
        .filter_map(|r| {
            let cell = |i: usize| js_number(r.get(i).map(|s| s.trim()).unwrap_or(""), 0.0) as i64;
            (cell(0) > 0).then(|| (cell(0), cell(1), cell(3), cell(4)))
        })
        .collect()
}

/// aidatatable.txt (tables du serveur) : (TID de l'IA, nom, colonnes non vides). Les colonnes répétées
/// (SkillCondition, SkillCount après chaque SkillN) prennent le numéro de la compétence qui les précède.
pub fn parse_monster_ai(text: &str) -> Vec<(u32, String, serde_json::Value)> {
    let (header, rows) = parse_tsv(text);
    let mut seen: HashMap<String, u32> = HashMap::new();
    let header: Vec<String> = header
        .iter()
        .map(|h| {
            let n = seen.entry(h.clone()).or_insert(0);
            *n += 1;
            if header.iter().filter(|x| *x == h).count() > 1 { format!("{h}{n}") } else { h.clone() }
        })
        .collect();
    rows.iter()
        .filter_map(|r| {
            let tid = r.first().and_then(|t| t.trim().parse::<u32>().ok())?;
            let o = row_object(&header, r, 1);
            let name = o.get("NAME").and_then(|n| n.as_str()).unwrap_or("").chars().take(128).collect();
            Some((tid, name, serde_json::Value::Object(o)))
        })
        .collect()
}

/// SkillMastery_High.txt (arbre 1) ou SkillMastery_Heroic.txt (arbre 2) : une ligne par classe (son TID seul en
/// colonne 0), puis ses compétences (TID en colonne 1, nom, ConditionA à C « TID;points », ConditionD
/// « total; »). Rend (classe, compétence, exigences, total).
pub fn parse_masteries(text: &str) -> Vec<(u32, u32, Vec<(u32, u32)>, i64)> {
    let (_, rows) = parse_tsv(text);
    let mut class = None;
    let mut out = Vec::new();
    for r in &rows {
        let cell = |i: usize| r.get(i).map(|s| s.trim()).unwrap_or("");
        if !cell(0).is_empty() {
            // Une valeur qui n'est pas un TID (« 10004;4 ») ne commence pas de classe.
            class = cell(0).parse::<u32>().ok();
            continue;
        }
        let (Some(class), Ok(skill)) = (class, cell(1).parse::<u32>()) else { continue };
        let requires = (3..=5)
            .filter_map(|i| {
                let mut p = cell(i).split(';');
                Some((p.next()?.trim().parse().ok()?, p.next()?.trim().parse().ok()?))
            })
            .collect();
        let total = cell(6).split(';').next().and_then(|t| t.trim().parse().ok()).unwrap_or(0);
        out.push((class, skill, requires, total));
    }
    out
}

async fn import_loot(db: &Databases, server: &Source) -> anyhow::Result<()> {
    if server.exists("itemdroptable.txt") {
        let rows = parse_monster_drops(&server.read("itemdroptable.txt")?);
        let keys = drop_keys(&rows);
        let out: Vec<Vec<Value>> = rows.iter().zip(keys).enumerate().map(|(i, (r, k))| vec![v(i as u32 + 1), v(k), v(r.0), v(r.1), v(r.2), v(r.3), v(r.4)]).collect();
        let n = replace(db, "monster_drop", "INSERT INTO monster_drop (id,stable_key,monster_tid,item_tid,min_count,max_count,rate) VALUES (?,?,?,?,?,?,?)", out).await?;
        println!("✓ monster_drop : {n}");
    }
    if server.exists("groupdroprate.txt") {
        let (h, rows) = parse_tsv(&server.read("groupdroprate.txt")?);
        let col = |n: &str| h.iter().position(|x| x == n);
        let bonus_cols: Vec<Option<usize>> = (1..=10).map(|k| col(&format!("LevelBonus{}", k * 10))).collect();
        let cell = |r: &[String], i: Option<usize>| i.and_then(|i| r.get(i)).map(|x| x.trim().to_string()).unwrap_or_default();
        let out: Vec<Vec<Value>> = rows
            .iter()
            .filter(|r| js_number(&cell(r, col("GroupID")), 0.0) > 0.0)
            .map(|r| {
                let bonus: Vec<i64> = bonus_cols.iter().map(|c| js_number(&cell(r, *c), 0.0) as i64).collect();
                vec![
                    v(js_number(&cell(r, col("GroupID")), 0.0) as u32),
                    v(cell(r, col("GroupName"))),
                    v(js_number(&cell(r, col("IsAffectedByDropMed")), 0.0) as i64),
                    v(js_number(&cell(r, col("DropRate")), 0.0) as i64),
                    v(serde_json::to_string(&bonus).unwrap()),
                ]
            })
            .collect();
        let n = replace(db, "drop_group", "INSERT INTO drop_group (group_id,name,meditation,rate,level_bonus) VALUES (?,?,?,?,?)", out).await?;
        println!("✓ drop_group : {n}");
    }
    if server.exists("droprankrate.txt") {
        let (_, rows) = parse_tsv(&server.read("droprankrate.txt")?);
        let out: Vec<Vec<Value>> = rows
            .iter()
            .filter(|r| r.first().is_some_and(|x| js_number(x, 0.0) > 0.0))
            .map(|r| {
                let rates: Vec<i64> = (1..=10).map(|i| js_number(r.get(i).map(String::as_str).unwrap_or(""), 0.0) as i64).collect();
                vec![v(js_number(&r[0], 0.0) as u32), v(serde_json::to_string(&rates).unwrap())]
            })
            .collect();
        let n = replace(db, "drop_rank_rate", "INSERT INTO drop_rank_rate (monster_rank,rates) VALUES (?,?)", out).await?;
        println!("✓ drop_rank_rate : {n}");
    }
    Ok(())
}

async fn import_monster_ai(db: &Databases, server: &Source) -> anyhow::Result<()> {
    if !server.exists("aidatatable.txt") {
        println!("· monster_ai : pas d'aidatatable.txt dans les tables du serveur, ignoré");
        return Ok(());
    }
    let rows = parse_monster_ai(&server.read("aidatatable.txt")?);
    let skilled = rows.iter().filter(|(_, _, d)| d.get("Skill1").is_some()).count();
    let out: Vec<Vec<Value>> = rows.into_iter().map(|(tid, name, data)| vec![v(tid), v(name), v(data.to_string())]).collect();
    let n = replace(db, "monster_ai", "INSERT IGNORE INTO monster_ai (tid,name,data) VALUES (?,?,?)", out).await?;
    println!("✓ monster_ai : {n} ({skilled} avec des compétences)");
    Ok(())
}

async fn import_spawns(db: &Databases, client: &Source, server: &Source) -> anyhow::Result<()> {
    let (rows, missing) = join_spawns(&server.read("spawndatatable.txt")?, &server.read("spawninstancetable.txt")?);
    // Noms publics des monstres (clé 1 de CharacterTemplatePublic.ini), en anglais.
    let public: std::collections::HashMap<u32, String> = if client.exists("CharacterTemplatePublic.ini") {
        parse_ini(&client.read("CharacterTemplatePublic.ini")?)
            .iter()
            .filter_map(|s| Some((s.id.parse::<u32>().ok()?, s.first("1")?.to_string())))
            .collect()
    } else {
        Default::default()
    };
    // Niveau des monstres : celui de CharacterDataTable.txt du client (Archonia en a relevé certains), le
    // niveau des tables du serveur seulement à défaut.
    let levels: std::collections::HashMap<u32, i64> = {
        let (h, rows) = parse_tsv(&client.read("CharacterDataTable.txt")?);
        let (t, l) = (h.iter().position(|x| x == "TID"), h.iter().position(|x| x == "LVL"));
        rows.iter()
            .filter_map(|r| {
                let cell = |i: Option<usize>| i.and_then(|i| r.get(i)).map(|s| js_number(s.trim(), 0.0) as i64).unwrap_or(0);
                (cell(t) > 0 && cell(l) > 0).then(|| (cell(t) as u32, cell(l)))
            })
            .collect()
    };
    // Délais des boss annoncés par la fenêtre des boss d'Archonia, à la place de ceux des tables du serveur.
    let periods: std::collections::HashMap<u32, (i64, i64)> =
        if client.exists("UIBoss.xml") { parse_boss_periods(&client.read("UIBoss.xml")?).into_iter().map(|(t, a, b)| (t, (a, b))).collect() } else { Default::default() };
    let (mut relevelled, mut timed) = (0, 0);
    let monsters: i64 = rows.iter().map(|r| r.count).sum();
    let out: Vec<Vec<Value>> = rows
        .iter()
        .map(|r| {
            let display: String = public.get(&r.tid).cloned().unwrap_or_default().chars().take(96).collect();
            let level = levels.get(&r.tid).copied().unwrap_or(r.level);
            relevelled += (level != r.level) as usize;
            let (respawn, respawn_max) = periods.get(&r.tid).copied().unwrap_or((r.respawn_sec, 0));
            timed += periods.contains_key(&r.tid) as usize;
            vec![
                v(r.id), v(r.key.clone()), v(r.group.clone()), v(r.name.clone()), v(r.tid), v(display), v(level), v(r.count), v(r.max_count),
                v(respawn), v(respawn_max), v(r.rate), v(r.ai_tid), v(r.siege_war), v(r.is_boss),
                v(r.pos[0]), v(r.pos[1]), v(r.pos[2]), v(r.degree[0]), v(r.degree[1]), v(r.radius),
            ]
        })
        .collect();
    let n = replace(
        db,
        "monster_spawn",
        "INSERT INTO monster_spawn (id,stable_key,group_name,name,tid,display_name,level,count,max_count,respawn_sec,respawn_max_sec,rate,ai_tid,siege_war,is_boss,pos_x,pos_y,pos_z,degree_x,degree_y,radius) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        out,
    )
    .await?;
    println!("✓ monster_spawn : {n} point(s), {monsters} monstre(s) ; {missing} ligne(s) sans position ignorée(s)");
    println!("  niveau du client sur {relevelled} point(s) ; délai de la fenêtre des boss (UIBoss.xml) sur {timed} point(s)");
    Ok(())
}

async fn import_all(db: &Databases, src: &Source) -> anyhow::Result<()> {
    // ---------------------------------------------------------------- charactertemplate.ini
    {
        let rows: Vec<Vec<Value>> = parse_ini(&src.read("charactertemplate.ini")?)
            .iter()
            .filter(|s| is_uint(&s.id))
            .map(|s| {
                let k = |key: &str| num(s.first(key));
                let last = |key: &str| num(s.all(key).last().copied());
                vec![
                    v(s.id.parse::<u32>().unwrap()),
                    v(s.first("Name").unwrap_or("").to_string()),
                    v(k("CharacterType")), v(k("Race")), v(k("Gender")), v(k("Class")),
                    v(js_number(s.first("Level").unwrap_or(""), 1.0)),
                    v(k("Con")), v(k("Str")), v(k("Int")), v(k("Dex")), v(k("Cha")), v(k("Luk")), v(k("Wis")),
                    v(k("Movement")), v(k("MovementFast")), v(k("Hp")), v(k("Mp")), v(k("Sp")),
                    v(k("AttackPoint")), v(k("MagicAttackPoint")), v(k("AR")), v(k("DR")), v(k("BaseEXP")),
                    v(last("MinDamage")), v(last("MaxDamage")), v(k("AttackRange")), v(k("AttackSpeed")), v(k("Sight")),
                    v(s.keys_json().to_string()),
                ]
            })
            .collect();
        let n = replace(db, "character_template", "INSERT INTO character_template (tid,name,character_type,race,gender,class,level,con,str,intel,dex,cha,luk,wis,movement,movement_fast,hp,mp,sp,attack_point,magic_attack,ar,dr,base_exp,min_damage,max_damage,attack_range,attack_speed,sight,raw) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)", rows).await?;
        println!("✓ character_template : {n}");
    }

    // ---------------------------------------------------------------- CharacterDataTable.txt
    {
        let (header, rows) = parse_tsv(&src.read("CharacterDataTable.txt")?);
        let out: Vec<Vec<Value>> = rows
            .iter()
            .filter(|r| r.first().is_some_and(|c| is_uint(c.trim())))
            .map(|r| {
                let mut o = Map::new();
                for (i, h) in header.iter().enumerate() {
                    if let Some(c) = r.get(i).filter(|c| !c.is_empty()) {
                        o.insert(h.clone(), json!(c));
                    }
                }
                vec![v(r[0].trim().parse::<u32>().unwrap()), v(r.get(1).cloned().unwrap_or_default()), v(serde_json::Value::Object(o).to_string())]
            })
            .collect();
        let n = replace(db, "character_data_table", "INSERT INTO character_data_table (tid,name,data) VALUES (?,?,?)", out).await?;
        println!("✓ character_data_table : {n}");
    }

    // ---------------------------------------------------------------- LevelUpExp.txt
    {
        let (_, rows) = parse_tsv(&src.read("LevelUpExp.txt")?);
        let out: Vec<Vec<Value>> = rows
            .iter()
            .filter(|r| r.first().is_some_and(|c| is_uint(c.trim())))
            .map(|r| vec![v(r[0].trim().parse::<u32>().unwrap()), v(r.get(1).and_then(|e| e.trim().parse::<u64>().ok()).unwrap_or(0))])
            .collect();
        let n = replace(db, "level_exp", "INSERT INTO level_exp (level,exp) VALUES (?,?)", out).await?;
        println!("✓ level_exp : {n}");
    }

    // ---------------------------------------------------------------- GrowUpFactor.txt
    {
        let text = src.read("GrowUpFactor.txt")?;
        let mut out: Vec<Vec<Value>> = Vec::new();
        let mut tid: Option<f64> = None;
        for line in super::lines(&text) {
            let cells: Vec<&str> = line.split('\t').map(str::trim).collect();
            if cells[0].is_empty() {
                continue;
            }
            if is_uint(cells[0]) && cells[1..].iter().all(|c| c.is_empty()) {
                tid = Some(js_number(cells[0], 0.0));
                continue;
            }
            let Some(t) = tid else { continue };
            if cells[0] == "LV" || !is_uint(cells[0]) {
                continue;
            }
            // LV STR Dex CON WIS INT CHA HP MP SP AC AR DR FIRE WATER EARTH AIR MAGIC MinDmg MaxDmg
            let mut row = vec![v(t)];
            row.extend((0..20).map(|i| v(num(cells.get(i).copied()))));
            out.push(row);
        }
        let n = replace(db, "growup_factor", "INSERT INTO growup_factor (template_id,level,str,dex,con,wis,intel,cha,hp,mp,sp,ac,ar,dr,res_fire,res_water,res_earth,res_air,res_magic,min_damage,max_damage) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)", out).await?;
        println!("✓ growup_factor : {n}");
    }

    // ---------------------------------------------------------------- logincharpos.ini
    {
        let out: Vec<Vec<Value>> = parse_ini(&src.read("logincharpos.ini")?)
            .iter()
            .filter(|s| is_uint(&s.id))
            .map(|s| {
                let p: Vec<f64> = s.first("Position").unwrap_or("0,0,0").split(',').map(|x| js_number(x, 0.0)).collect();
                let g = |i: usize| p.get(i).copied().unwrap_or(0.0);
                vec![v(s.id.parse::<u32>().unwrap()), v(s.first("Race").unwrap_or("").to_string()), v(g(0)), v(g(1)), v(g(2)), v(num(s.first("DegreeX"))), v(num(s.first("DegreeY")))]
            })
            .collect();
        let n = replace(db, "login_char_pos", "INSERT INTO login_char_pos (idx,race,pos_x,pos_y,pos_z,degree_x,degree_y) VALUES (?,?,?,?,?,?,?)", out).await?;
        println!("✓ login_char_pos : {n}");
    }

    // ---------------------------------------------------------------- itemtemplateentry.ini
    if src.exists("itemtemplateentry.ini") {
        let out: Vec<Vec<Value>> = parse_ini(&src.read("itemtemplateentry.ini")?)
            .iter()
            .filter(|s| is_uint(&s.id))
            .map(|s| vec![v(s.id.parse::<u32>().unwrap()), v(s.first("0").unwrap_or("").to_string()), s.first("1").map(|f| v(f.to_string())).unwrap_or(Value::NULL), v(s.keys_json().to_string())])
            .collect();
        let n = replace(db, "item_template", "INSERT INTO item_template (tid,name,file,raw) VALUES (?,?,?,?)", out).await?;
        println!("✓ item_template : {n}");
    }

    // ---------------------------------------------------------------- SKILLTEMPLATE.ini
    if src.exists("SKILLTEMPLATE.ini") {
        let out: Vec<Vec<Value>> = parse_ini(&src.read("SKILLTEMPLATE.ini")?)
            .iter()
            .filter(|s| is_uint(&s.id))
            .map(|s| vec![v(s.id.parse::<u32>().unwrap()), v(s.first("1").or(s.first("Name")).unwrap_or("").to_string()), v(s.keys_json().to_string())])
            .collect();
        let n = replace(db, "skill_template", "INSERT INTO skill_template (tid,name,raw) VALUES (?,?,?)", out).await?;
        println!("✓ skill_template : {n}");
    }

    // ---------------------------------------------------------------- SKILL_SPEC.txt, SKILL_CONST.txt
    if src.exists("SKILLTEMPLATE.ini") && src.exists("SKILL_SPEC.txt") && src.exists("SKILL_CONST.txt") {
        let tids = skill_tids(&src.read("SKILLTEMPLATE.ini")?);
        let (specs, missing_spec) = parse_skill_specs(&src.read("SKILL_SPEC.txt")?, &tids);
        let (levels, missing_const) = parse_skill_levels(&src.read("SKILL_CONST.txt")?, &tids);
        let out: Vec<Vec<Value>> = specs.into_iter().map(|(tid, name, spec)| vec![v(tid), v(name), v(spec.to_string())]).collect();
        let n = replace(db, "skill_spec", "INSERT INTO skill_spec (tid,name,spec) VALUES (?,?,?)", out).await?;
        println!("✓ skill_spec : {n} ({missing_spec} nom(s) sans modèle)");
        let out: Vec<Vec<Value>> = levels.into_iter().map(|(tid, level, consts)| vec![v(tid), v(level), v(consts.to_string())]).collect();
        let n = replace(db, "skill_level", "INSERT INTO skill_level (tid,level,consts) VALUES (?,?,?)", out).await?;
        println!("✓ skill_level : {n} ({missing_const} nom(s) sans modèle)");
    }

    // ---------------------------------------------------------------- SkillMastery_High.txt, SkillMastery_Heroic.txt
    if src.exists("SkillMastery_High.txt") || src.exists("SkillMastery_Heroic.txt") {
        let mut out: Vec<Vec<Value>> = Vec::new();
        for (tree, file) in [(1, "SkillMastery_High.txt"), (2, "SkillMastery_Heroic.txt")] {
            if !src.exists(file) {
                continue;
            }
            for (class, skill, requires, total) in parse_masteries(&src.read(file)?) {
                out.push(vec![v(class), v(skill), v(tree), v(json!(requires).to_string()), v(total)]);
            }
        }
        let n = replace(db, "skill_mastery", "INSERT IGNORE INTO skill_mastery (class_tid,skill_tid,tree,requires,total_points) VALUES (?,?,?,?,?)", out).await?;
        println!("✓ skill_mastery : {n}");
    }

    // ---------------------------------------------------------------- questtemplate.ini (tabulé)
    if src.exists("questtemplate.ini") {
        let quests = parse_quests(&src.read("questtemplate.ini")?);
        let out: Vec<Vec<Value>> = quests.into_iter().map(|(tid, name, data)| vec![v(tid), v(name), v(data.to_string())]).collect();
        let n = replace(db, "quest_template", "INSERT INTO quest_template (tid,name,data) VALUES (?,?,?)", out).await?;
        println!("✓ quest_template : {n}");
    }
    if src.exists("questgroup.ini") {
        let out: Vec<Vec<Value>> = parse_quest_groups(&src.read("questgroup.ini")?)
            .into_iter()
            .map(|(id, name, give, confirm)| vec![v(id), v(name), v(json!(give).to_string()), v(json!(confirm).to_string())])
            .collect();
        let n = replace(db, "quest_group", "INSERT INTO quest_group (id,name,give,confirm) VALUES (?,?,?,?)", out).await?;
        println!("✓ quest_group : {n}");
    }

    // ---------------------------------------------------------------- npc.ini (PNJ placés dans le monde)
    if src.exists("npc.ini") {
        let out: Vec<Vec<Value>> = parse_ini(&src.read("npc.ini")?)
            .iter()
            .filter(|s| is_uint(&s.id) && s.first("TID").is_some_and(|t| !t.is_empty()) && s.first("Position").is_some_and(|p| !p.is_empty()))
            .map(|s| {
                let p: Vec<f64> = s.first("Position").unwrap().split(',').map(|x| js_number(x, 0.0)).collect();
                let d: Vec<f64> = s.first("Degree").unwrap_or("0,0").split(',').map(|x| js_number(x, 0.0)).collect();
                let g = |a: &Vec<f64>, i: usize| a.get(i).copied().unwrap_or(0.0);
                vec![
                    v(s.id.parse::<u32>().unwrap()),
                    v(num(s.first("TID"))),
                    v(s.first("Name").unwrap_or("").to_string()),
                    v(g(&p, 0)), v(g(&p, 1)), v(g(&p, 2)), v(g(&d, 0)), v(g(&d, 1)),
                    v(serde_json::to_string(&s.order).unwrap()),
                ]
            })
            .collect();
        let n = replace(db, "npc_spawn", "INSERT INTO npc_spawn (id,tid,name,pos_x,pos_y,pos_z,degree_x,degree_y,raw) VALUES (?,?,?,?,?,?,?,?,?)", out).await?;
        println!("✓ npc_spawn : {n}");
    }

    // ---------------------------------------------------------------- RegionTemplate.ini, obj*.ini (régions, résurrection)
    if src.exists("RegionTemplate.ini") {
        let regions = parse_regions(&src.read("RegionTemplate.ini")?);
        let by_name: std::collections::HashMap<&str, u32> = regions.iter().map(|r| (r.1.as_str(), r.0)).collect();
        let mut points = Vec::new();
        let mut files: Vec<PathBuf> = std::fs::read_dir(&src.0)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("obj") && n.ends_with(".ini")))
            .collect();
        files.sort();
        for f in &files {
            let file = f.file_name().unwrap().to_str().unwrap();
            let text = src.read(file)?;
            let mut ranks: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
            for b in parse_bindings(&text).into_iter().filter(|b| b.binding_type == 1) {
                // Clé stable : fichier, objet et rang de l'événement dans l'objet.
                let rank = ranks.entry(b.object.clone()).or_insert(0);
                *rank += 1;
                let key = format!("{file}#{}#{rank}", b.object);
                if let Some(&region) = by_name.get(b.town.as_str()) {
                    points.push((region, key, b));
                }
            }
        }
        let out: Vec<Vec<Value>> = regions.iter().map(|r| vec![v(r.0), v(r.1.clone()), v(r.2), v(r.3), v((r.3 >> 2) & 3)]).collect();
        let n = replace(db, "region_template", "INSERT INTO region_template (id,name,parent_id,type_bits,safety) VALUES (?,?,?,?,?)", out).await?;
        let out: Vec<Vec<Value>> = points
            .iter()
            .enumerate()
            .map(|(i, (region, key, b))| vec![v(i as u32 + 1), v(key.clone()), v(*region), v(b.name.clone()), v(b.pos[0]), v(b.pos[1]), v(b.pos[2]), v(b.race), v(b.class)])
            .collect();
        let m = replace(db, "resurrection_point", "INSERT INTO resurrection_point (id,stable_key,region_id,name,pos_x,pos_y,pos_z,race,class) VALUES (?,?,?,?,?,?,?,?,?)", out).await?;
        println!("✓ region_template : {n} ; resurrection_point : {m} (sur {} fichier(s) obj*.ini)", files.len());
    }

    // ---------------------------------------------------------------- TeleportPoint.ini, obj*.ini, TeleportFee.txt (portails)
    if src.exists("TeleportPoint.ini") {
        let points = parse_teleport_points(&src.read("TeleportPoint.ini")?);
        let out: Vec<Vec<Value>> = points
            .iter()
            .map(|p| {
                let list = |l: &Vec<String>| serde_json::to_string(l).unwrap();
                vec![
                    v(p.name.clone()), v(p.target_type), v(p.pos[0]), v(p.pos[1]), v(p.pos[2]), v(p.radius_min), v(p.radius_max),
                    v(p.region_type), v(p.special_type), v(p.use_type), v(list(&p.groups)), v(list(&p.target_groups)),
                ]
            })
            .collect();
        let n = replace(db, "teleport_point", "INSERT INTO teleport_point (name,target_type,pos_x,pos_y,pos_z,radius_min,radius_max,region_type,special_type,use_type,point_groups,target_groups) VALUES (?,?,?,?,?,?,?,?,?,?,?,?)", out).await?;
        let mut files: Vec<String> = std::fs::read_dir(&src.0)?
            .filter_map(|e| e.ok().and_then(|e| e.file_name().to_str().map(str::to_string)))
            .filter(|n| n.starts_with("obj") && n.ends_with(".ini"))
            .collect();
        files.sort();
        let mut portals: std::collections::BTreeMap<u32, (String, [f64; 3])> = std::collections::BTreeMap::new();
        for f in &files {
            for (object, point, pos) in parse_portals(&src.read(f)?) {
                portals.insert(object, (point, pos));
            }
        }
        let out: Vec<Vec<Value>> = portals.iter().map(|(id, (point, pos))| vec![v(*id), v(point.clone()), v(pos[0]), v(pos[1]), v(pos[2])]).collect();
        let m = replace(db, "teleport_object", "INSERT INTO teleport_object (object_id,point,pos_x,pos_y,pos_z) VALUES (?,?,?,?,?)", out).await?;
        let fees = if src.exists("TeleportFee.txt") { parse_teleport_fees(&src.read("TeleportFee.txt")?) } else { Vec::new() };
        let out: Vec<Vec<Value>> = fees.into_iter().map(|(a, b, fee)| vec![v(a), v(b), v(fee)]).collect();
        let f = replace(db, "teleport_fee", "INSERT INTO teleport_fee (source,destination,fee) VALUES (?,?,?)", out).await?;
        println!("✓ teleport_point : {n} ; teleport_object : {m} ; teleport_fee : {f}");
    }

    // ---------------------------------------------------------------- guildmaxmember.txt (taille des guildes)
    if src.exists("guildmaxmember.txt") {
        let out: Vec<Vec<Value>> = parse_guild_max_members(&src.read("guildmaxmember.txt")?).into_iter().map(|(m, g, t, c)| vec![v(m), v(g), v(t), v(c)]).collect();
        let n = replace(db, "guild_max_member", "INSERT INTO guild_max_member (max_members,gold,skull_tid,skull_count) VALUES (?,?,?,?)", out).await?;
        println!("✓ guild_max_member : {n}");
    }

    // ---------------------------------------------------------------- bankslotprice.txt (pages de banque)
    if src.exists("bankslotprice.txt") {
        let out: Vec<Vec<Value>> = parse_bank_slot_prices(&src.read("bankslotprice.txt")?).into_iter().map(|(s, p)| vec![v(s), v(p)]).collect();
        let n = replace(db, "bank_slot_price", "INSERT INTO bank_slot_price (slot,price) VALUES (?,?)", out).await?;
        println!("✓ bank_slot_price : {n}");
    }

    // ---------------------------------------------------------------- itemdatatable.txt (prix, piles)
    {
        let (header, rows) = parse_tsv(&src.read("itemdatatable.txt")?);
        let col = |name: &str| header.iter().position(|h| h == name);
        let (c_tid, c_name) = (col("TID"), col("ItemName"));
        let cols = ["Level", "Part", "Kind", "ItemType", "Type", "NPC Price", "PC Price"].map(|n| col(n));
        let c_stack = col("Stack");
        fn cell(r: &[String], c: Option<usize>) -> Option<&str> {
            c.and_then(|i| r.get(i)).map(String::as_str)
        }
        let mut out: Vec<Vec<Value>> = Vec::new();
        for r in &rows {
            let tid = js_number(cell(r, c_tid).unwrap_or(""), f64::NAN);
            if !(tid.fract() == 0.0 && tid > 0.0) {
                continue;
            }
            let name: String = cell(r, c_name).unwrap_or("").trim().chars().take(128).collect();
            let mut raw = Map::new();
            for (i, h) in header.iter().enumerate() {
                raw.insert(h.clone(), json!(r.get(i).cloned().unwrap_or_default()));
            }
            let mut row = vec![v(tid as u32), v(name)];
            row.extend(cols.iter().map(|c| v(num(cell(r, *c)))));
            row.push(v(js_number(cell(r, c_stack).unwrap_or(""), 1.0).max(1.0)));
            row.push(v(serde_json::Value::Object(raw).to_string()));
            out.push(row);
        }
        let n = replace(db, "item_data", "INSERT INTO item_data (tid,name,level,part,kind,item_type,type,npc_price,pc_price,stack,raw) VALUES (?,?,?,?,?,?,?,?,?,?,?) ON DUPLICATE KEY UPDATE name = VALUES(name)", out).await?;
        println!("✓ item_data : {n}");
    }

    // ---------------------------------------------------------------- npctradeitemlist.txt (boutiques)
    {
        let text = src.read("npctradeitemlist.txt")?;
        let mut out: Vec<Vec<Value>> = Vec::new();
        let mut template: Option<f64> = None;
        let mut idx = 0u32;
        let mut shops = std::collections::HashSet::new();
        for line in super::lines(&text) {
            let cells: Vec<&str> = line.split('\t').map(str::trim).collect();
            if cells[0] == "Npc" {
                template = Some(js_number(cells.get(1).copied().unwrap_or(""), f64::NAN)).filter(|t| t.fract() == 0.0);
                idx = 0;
                continue;
            }
            if let (Some(t), "Item") = (template, cells[0]) {
                out.push(vec![v(t), v(idx), v(num(cells.get(1).copied())), v(js_number(cells.get(2).copied().unwrap_or(""), 1.0).max(1.0))]);
                shops.insert(t as i64);
                idx += 1;
            }
        }
        let n = replace(db, "npc_trade_list", "INSERT INTO npc_trade_list (template_id,idx,tid,count) VALUES (?,?,?,?)", out).await?;
        println!("✓ npc_trade_list : {n} ligne(s), {} boutique(s)", shops.len());
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nombres_comme_javascript() {
        assert_eq!(js_number("", 7.0), 0.0);
        assert_eq!(js_number(" 12 ", 7.0), 12.0);
        assert_eq!(js_number("0x10", 7.0), 16.0);
        assert_eq!(js_number("abc", 7.0), 7.0);
        assert_eq!(js_number("1.5", 7.0), 1.5);
    }

    #[test]
    fn sections_ini() {
        let s = parse_ini("// x\n[12]\nName=Foo\nMinDamage=1\nMinDamage=3\n[a]\nk=v\n");
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].first("Name"), Some("Foo"));
        assert_eq!(s[0].all("MinDamage"), vec!["1", "3"]);
        assert_eq!(s[0].keys_json().to_string(), r#"{"Name":"Foo","MinDamage":["1","3"]}"#);
        let t = parse_ini("[1]\nb=x\n10=y\n2=z\na=w\n");
        assert_eq!(t[0].keys_json().to_string(), r#"{"2":"z","10":"y","b":"x","a":"w"}"#);
    }
    #[test]
    fn decoupage_de_node_data() {
        let mut bytes = Vec::new();
        for (x, z) in [(325u32, 389u32), (900, 1)] {
            bytes.extend(x.to_le_bytes());
            bytes.extend(z.to_le_bytes());
            bytes.extend(vec![7u8; 1024]);
        }
        bytes.extend([1, 2, 3]); // reste incomplet ignoré
        let s = parse_node_data(&bytes);
        assert_eq!(s.len(), 1, "secteur hors du monde écarté");
        assert_eq!((s[0].0, s[0].1, s[0].2.len()), (325, 389, 1024));
    }

    #[test]
    fn regions_et_points_de_resurrection() {
        let r = parse_regions("[9]\nName=Anchorville\nParentIndex=-1\nType=786929\n[x]\nName=?\n");
        assert_eq!(r, vec![(9, "Anchorville".to_string(), -1, 786_929)]);
        assert_eq!((786_929 >> 2) & 3, 0, "région sûre");
        let obj = "[1]\nPosition=-579188.8,7930.4,26739.5\nFunction=8\nEventID=1\nFunction=19\nBindingName=Point\nTownName=Delfaras\nBindingType=1\nCharRaceType=2\nCharClassType=4294967295\nBindingEnd=0\n[2]\nFunction=19\nTownName=SansPosition\n";
        let b = parse_bindings(obj);
        assert_eq!(b.len(), 1);
        assert_eq!((b[0].town.as_str(), b[0].binding_type, b[0].race, b[0].class, b[0].pos[0]), ("Delfaras", 1, 2, 0, -579188.8));
        assert_eq!(b[0].object, "1");
    }

    #[test]
    fn portails_et_pages_de_banque() {
        let pts = parse_teleport_points("[Village]\nModuleData1=AgpmEventTeleport\nRadiusMin=400\nRadiusMax=600\nPointTargetType=2\nPointPos=0:0:0\nUseType=3\nGroup=1.A\nGroup=2.B\nTargetGroup=1.A\nEnumEnd=0\n[Fixe]\nPointTargetType=1\nPointPos=10:20:30\n");
        assert_eq!(pts.len(), 2);
        assert_eq!((pts[0].name.as_str(), pts[0].radius_min, pts[0].radius_max, pts[0].use_type), ("Village", 400.0, 600.0, 3));
        assert_eq!((pts[0].groups.clone(), pts[0].target_groups.clone()), (vec!["1.A".to_string(), "2.B".to_string()], vec!["1.A".to_string()]));
        assert_eq!((pts[1].target_type, pts[1].pos), (1, [10.0, 20.0, 30.0]));
        let obj = "[19990004]\nPosition=-762800.0,1600.0,-578820.0\nFunction=19\nPointName=Leurre\nFunction=8\nEventID=1\nPointName=Village\nTeleportEnd=0\n[abc]\nPosition=0,0,0\nFunction=8\nPointName=X\n";
        assert_eq!(parse_portals(obj), vec![(19_990_004, "Village".to_string(), [-762800.0, 1600.0, -578820.0])]);
        let fees = parse_teleport_fees("start_portal\tfinish_portal\tcost\r\nA\tB\t500 \r\nA\tB\t700\r\nB\tA\t\r\n");
        assert_eq!(fees, vec![("A".to_string(), "B".to_string(), 700)]);
        assert_eq!(parse_bank_slot_prices("Slot\tPrice\t\r\n1\t1000000\t# commentaire\r\n2\t-1\t\r\n"), vec![(1, 1_000_000), (2, -1)]);
        assert_eq!(parse_guild_max_members("\tOr\tCrâne\tID\tNombre\r\n50\t10000\t\t\t\r\n60\t1000000\tX\t7\t2\r\n"), vec![(50, 10_000, 0, 0), (60, 1_000_000, 7, 2)]);
    }

    #[test]
    fn ia_des_monstres_et_maitrises() {
        let ai = "TID\tNAME\tUseNormalAttack\tSkill1\tSkillCondition\tSkillCount\tSkill2\tSkillCondition\tSkillCount\tGuard\r\n\
                  2750\tChef\t1\t179;1\tHP;100%;이하\t무한;\t195;1\tHP;50%;이하\t횟수;1\t\r\n\
                  100000\tGarde\t0\t\t\t\t\t\t\t고정\r\n";
        let rows = parse_monster_ai(ai);
        assert_eq!(rows.len(), 2);
        let (tid, name, d) = &rows[0];
        assert_eq!((*tid, name.as_str()), (2750, "Chef"));
        assert_eq!((d["Skill1"].as_str(), d["SkillCondition2"].as_str(), d["SkillCount2"].as_str()), (Some("179;1"), Some("HP;50%;이하"), Some("횟수;1")));
        assert!(d.get("SkillCondition").is_none(), "colonnes répétées numérotées");
        assert_eq!(rows[1].2["UseNormalAttack"].as_str(), Some("0"));
        let high = "Class TID\tSkill TID\tSkill Name\tConditionA\tConditionB\tConditionC\tConditionD\r\n\
                    96\t\t\t\t\t\t\r\n\
                    \t526\tChant\t\t\t\t\r\n\
                    \t558\tBouclier\t525;2\t527;2\t\t\r\n\
                    10004;4\t\t\t\t\t\t\r\n\
                    1722\t\t\t\t\t\t\r\n\
                    \t1237\tCourage\t1189;1\t1236;3\t\t30;\r\n";
        assert_eq!(parse_masteries(high), vec![(96, 526, vec![], 0), (96, 558, vec![(525, 2), (527, 2)], 0), (1722, 1237, vec![(1189, 1), (1236, 3)], 30)]);
    }

    #[test]
    fn delais_de_la_fenetre_des_boss() {
        let xml = "<UIBoss>\n\t<Boss TID=\"2073\" Category=\"Battlesquare\" Icon=\"14\">\n\t\t<Name Text=\"Basilis (Lv.30)\"/>\n\t\t<SpawnPeriod Text=\"Spawn Period: 30-45 minutes\"/>\n\t</Boss>\n\
                   \t<Boss TID=\"5794\" Category=\"Firmament\" Icon=\"1\">\n\t\t<SpawnPeriod Text=\"Event-based spawn\"/>\n\t</Boss>\n\
                   \t<Boss TID=\"5799\" Category=\"Firmament\" Icon=\"2\">\n\t\t<SpawnPeriod Text=\"25-35 minutes\"/> \n\t</Boss>\n</UIBoss>";
        assert_eq!(parse_boss_periods(xml), vec![(2073, 1800, 2700), (5799, 1500, 2100)]);
    }

    #[test]
    fn butin_propre_aux_monstres() {
        let t = "MonsterName\tMonsterTID\tItemCount\tItemName\tItemTID\tMin\tMax\tRate\r\n\
                 Taravan\t15\t2\t\t5018\t1\t3\t10\r\n\
                 Taravan\t15\t2\t\t1856\t0\t0\t500\r\n\
                 Vide\t0\t1\t\t99\t1\t1\t5\r\n";
        assert_eq!(parse_monster_drops(t), vec![(15, 5018, 1, 3, 10), (15, 1856, 1, 1, 500)]);
        let rows = [(15, 5018, 1, 3, 10), (15, 1856, 1, 1, 500), (15, 5018, 2, 2, 5)];
        assert_eq!(drop_keys(&rows), vec!["15:5018#1", "15:1856#1", "15:5018#2"]);
    }

    #[test]
    fn jointure_des_apparitions() {
        let data = "GroupName\tName\tTotalMobCount\tSpawntime\tMobTID\tMobLV\tRate\tMaxMobCount\tMobAITID\tSiegeWarType\tIsBoss\r\n\
                    g1\tg1_1\t5\t8\t459\t13\t10000\t0\t4590\t0\t0\r\n\
                    g1\tg1_2\t3\t30\t292\t12\t\t0\t2920\t0\t1\r\n\
                    g2\tg2_1\t4\t9\t645\t14\t10000\t0\t6450\t10\t0\r\n";
        let inst = "Name\tPosition\tRotation\tRadius\r\n\
                    g1_1\t-100,0,200.5\t0,90\t2500\r\n\
                    g2_1\t1,2,3\t0,0\t0\r\n\
                    g1_1\t9,9,9\t0,0\t1\r\n";
        let (rows, missing) = join_spawns(data, inst);
        assert_eq!(missing, 1, "g1_2 n'a pas de position");
        assert_eq!(rows.len(), 3, "g1_1 a deux positions");
        let a = &rows[0];
        assert_eq!((a.id, a.group.as_str(), a.name.as_str(), a.tid, a.level, a.count, a.respawn_sec), (1, "g1", "g1_1", 459, 13, 5, 8));
        assert_eq!((a.pos, a.degree, a.radius), ([-100.0, 0.0, 200.5], [0.0, 90.0], 2500.0));
        assert_eq!((rows[1].id, rows[1].name.as_str(), rows[1].pos), (2, "g1_1", [9.0, 9.0, 9.0]));
        assert_eq!((rows[2].id, rows[2].siege_war), (3, 10));
        // Clés stables : rang de la ligne parmi celles du même point.
        assert_eq!(rows.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(), vec!["g1_1#1", "g1_1#2", "g2_1#1"]);
    }
}
