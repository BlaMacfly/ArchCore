//! `archcore import [dossier] [--server dossier]` : importe les tables déchiffrées du client
//! (`data/client-ini`) dans `archcore_world`. Les fichiers sont en CP949 (coréen) ; ils sont convertis en
//! UTF-8. Avec `--server` (par défaut `data/server-ini` s'il existe), importe aussi, depuis les tables du
//! serveur d'origine, les apparitions de monstres (`spawndatatable.txt`, `spawninstancetable.txt`), la carte
//! (`world/node_data`) et le butin (`itemdroptable.txt`, `groupdroprate.txt`, `droprankrate.txt`).

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
    let monsters: i64 = rows.iter().map(|r| r.count).sum();
    let out: Vec<Vec<Value>> = rows
        .iter()
        .map(|r| {
            let display: String = public.get(&r.tid).cloned().unwrap_or_default().chars().take(96).collect();
            vec![
                v(r.id), v(r.key.clone()), v(r.group.clone()), v(r.name.clone()), v(r.tid), v(display), v(r.level), v(r.count), v(r.max_count),
                v(r.respawn_sec), v(r.rate), v(r.ai_tid), v(r.siege_war), v(r.is_boss),
                v(r.pos[0]), v(r.pos[1]), v(r.pos[2]), v(r.degree[0]), v(r.degree[1]), v(r.radius),
            ]
        })
        .collect();
    let n = replace(
        db,
        "monster_spawn",
        "INSERT INTO monster_spawn (id,stable_key,group_name,name,tid,display_name,level,count,max_count,respawn_sec,rate,ai_tid,siege_war,is_boss,pos_x,pos_y,pos_z,degree_x,degree_y,radius) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        out,
    )
    .await?;
    println!("✓ monster_spawn : {n} point(s), {monsters} monstre(s) ; {missing} ligne(s) sans position ignorée(s)");
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
