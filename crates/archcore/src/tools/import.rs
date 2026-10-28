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
/// vide. Rend (tid, niveau, colonnes) et le nombre de noms sans modèle. Skill_Const2.txt a la même forme, ses
/// colonnes finissant par « 2 » (`skill_level2`, `ignore_physical_defence_percent2`…).
pub fn parse_skill_levels(text: &str, tids: &HashMap<String, u32>) -> (Vec<(u32, u32, serde_json::Value)>, usize) {
    let (header, rows) = parse_tsv(text);
    let header = group_columns(&header);
    let level_col = header.iter().position(|h| h == "skill_level" || h == "skill_level2").unwrap_or(1);
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
        o.remove("skill_level2");
        out.push((tid, level, serde_json::Value::Object(o)));
    }
    (out, missing)
}

/// Ajoute aux niveaux de SKILL_CONST.txt les colonnes de Skill_Const2.txt du même niveau ; rend le nombre de
/// niveaux complétés.
pub fn merge_skill_levels(levels: &mut [(u32, u32, serde_json::Value)], more: Vec<(u32, u32, serde_json::Value)>) -> usize {
    let mut extra: HashMap<(u32, u32), serde_json::Map<String, serde_json::Value>> = HashMap::new();
    for (tid, level, v) in more {
        if let serde_json::Value::Object(o) = v {
            extra.entry((tid, level)).or_default().extend(o);
        }
    }
    let mut n = 0;
    for (tid, level, v) in levels.iter_mut() {
        if let (Some(o), serde_json::Value::Object(target)) = (extra.remove(&(*tid, *level)), v) {
            if !o.is_empty() {
                target.extend(o);
                n += 1;
            }
        }
    }
    n
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
    /// Comme `read`, mais un fichier déjà en UTF-8 (SiegeWar.txt d'Archonia) est gardé tel quel.
    fn read_utf8_or_korean(&self, name: &str) -> anyhow::Result<String> {
        let bytes = std::fs::read(self.0.join(name)).map_err(|e| anyhow::anyhow!("{name} : {e}"))?;
        Ok(match String::from_utf8(bytes) {
            Ok(text) => text.trim_start_matches('\u{feff}').to_string(),
            Err(e) => encoding_rs::EUC_KR.decode(e.as_bytes()).0.into_owned(),
        })
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
/// Range un fichier entier dans `client_table` (ligne 0), à la place de l'ancien.
async fn store_text(db: &Databases, name: &str, text: &str) -> anyhow::Result<()> {
    db.world.exec("DELETE FROM client_table WHERE name = ?", (name,)).await?;
    db.world.exec("INSERT INTO client_table (name,row_idx,data) VALUES (?,0,?)", (name, serde_json::Value::String(text.to_string()).to_string())).await?;
    println!("✓ client_table : {name}");
    Ok(())
}

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
    let game = cfg.root.join(&cfg.client.data_dir);
    import_region_glossary(&db, &src, &game).await?;
    finish_run(&db, run).await?;
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
            // Nombre d'options et de sockets des objets créés (tables du serveur d'origine).
            let server = Source(d.clone());
            for name in ["optionnumdroprate.txt", "socketnumdroprate.txt", "refineryrecipetable.txt"] {
                if server.exists(name) {
                    store_text(&db, name, &server.read(name)?).await?;
                }
            }
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

/// RegionGlossary.txt (une ligne par région, « nom coréen<TAB>nom affiché ») : clé décodée en EUC-KR, nom dans la page de
/// code du dossier de langue (`encoding`), comme le client qui lit chaque colonne ainsi.
pub fn parse_region_glossary(plain: &[u8], encoding: &'static encoding_rs::Encoding) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in plain.split(|b| *b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(tab) = line.iter().position(|b| *b == b'\t') else { continue };
        let key = encoding_rs::EUC_KR.decode(&line[..tab]).0.trim().to_string();
        let name = encoding.decode(&line[tab + 1..]).0.trim().to_string();
        if !key.is_empty() && !name.is_empty() && !out.iter().any(|(k, _)| *k == key) {
            out.push((key, name));
        }
    }
    out
}

/// Dossier et page de code du glossaire d'une langue : `client_folder` de la table `language`, sinon `ini_<langue de
/// base>` (`ini` pour l'anglais).
pub fn glossary_folder(code: &str, client_folder: Option<&str>) -> (String, &'static encoding_rs::Encoding) {
    let base = archcore_net::ext_text::base_language(code).to_lowercase();
    let folder = client_folder.map(str::to_string).unwrap_or_else(|| if base == "en" { "ini".into() } else { format!("ini_{base}") });
    let encoding = match base.as_str() {
        "ko" => encoding_rs::EUC_KR,
        "ja" => encoding_rs::SHIFT_JIS,
        "zh" => encoding_rs::GBK,
        _ => encoding_rs::WINDOWS_1252,
    };
    (folder, encoding)
}

/// Glossaire des régions (`region_glossary`) : RegionGlossary.txt du dossier de chaque langue de la table `language`
/// dans le dossier Game du client (chiffré ou non), et, pour l'anglais, celui des tables importées à défaut.
async fn import_region_glossary(db: &Databases, src: &Source, game: &std::path::Path) -> anyhow::Result<()> {
    let langs = db.world.rows("SELECT code, client_folder FROM language", ()).await.unwrap_or_default();
    let mut out: Vec<Vec<Value>> = Vec::new();
    let mut found = Vec::new();
    for r in &langs {
        let code: String = crate::db::col(r, "code");
        let (folder, encoding) = glossary_folder(&code, crate::db::col::<Option<String>>(r, "client_folder").as_deref());
        let path = ["regionglossary.txt", "RegionGlossary.txt"].iter().map(|f| game.join(&folder).join(f)).find(|p| p.is_file());
        let plain = match path {
            Some(p) => super::load_ini(&p)?.1,
            None if code == "en" && src.exists("regionglossary.txt") => super::load_ini(&src.0.join("regionglossary.txt"))?.1,
            None => continue,
        };
        let rows = parse_region_glossary(&plain, encoding);
        found.push(format!("{code} {}", rows.len()));
        out.extend(rows.into_iter().map(|(k, n)| vec![v(code.clone()), v(k), v(n)]));
    }
    let n = replace(db, "region_glossary", "INSERT INTO region_glossary (lang,korean_name,name) VALUES (?,?,?)", out).await?;
    println!("✓ region_glossary : {n} ({})", if found.is_empty() { "aucun dossier de langue trouvé".to_string() } else { found.join(", ") });
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

/// Un château de SiegeWar.txt.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SiegeWarRow {
    pub id: u32,
    pub name: String,
    /// Noms des régions du château et de ses abords (colonne Region, « château/abords »).
    pub regions: (String, String),
    pub playing_ms: i64,
    pub eye_open_ms: i64,
    pub stamping_ms: i64,
    pub declare_hours: i64,
    pub select_hours: i64,
    pub interval_days: i64,
    pub begin_hour: i64,
    pub stamp_skill_tid: i64,
    pub stamp_skill_level: i64,
    pub disabled: bool,
}

/// SiegeWar.txt : un château par ligne (colonnes cherchées par leur nom ; StartDay et IsDisabled, lues par le client
/// d'Archonia, manquent au fichier livré). Une heure de début au-delà de 24 est ramenée à 21, comme le lecteur du client.
pub fn parse_siege_war(text: &str) -> Vec<SiegeWarRow> {
    let (header, rows) = parse_tsv(text);
    let col = |name: &str| header.iter().position(|h| h.eq_ignore_ascii_case(name));
    let get = |r: &[String], name: &str| col(name).and_then(|i| r.get(i)).map(|s| s.trim().to_string()).unwrap_or_default();
    let n = |r: &[String], name: &str| js_number(&get(r, name), 0.0) as i64;
    rows.iter()
        .filter(|r| is_uint(&get(r, "Index")))
        .map(|r| {
            let region = get(r, "Region");
            let (castle, outskirts) = region.split_once('/').map(|(a, b)| (a.trim().to_string(), b.trim().to_string())).unwrap_or((region.clone(), String::new()));
            let hour = n(r, "SWBeginTime");
            SiegeWarRow {
                id: get(r, "Index").parse().unwrap_or(0),
                name: get(r, "Castle"),
                regions: (castle, outskirts),
                playing_ms: n(r, "SWPlayingTime"),
                eye_open_ms: n(r, "ArchonEyeOpenTime"),
                stamping_ms: n(r, "StampingTime"),
                declare_hours: n(r, "NextSWDeclaring(Hours)"),
                select_hours: n(r, "SelectGuildForSW(Hours)"),
                interval_days: n(r, "Duration4nextSW(Days)"),
                begin_hour: if hour > 24 { 21 } else { hour },
                stamp_skill_tid: n(r, "StampSkillTID"),
                stamp_skill_level: n(r, "StampSkillLevel"),
                disabled: n(r, "IsDisabled") != 0,
            }
        })
        .collect()
}

/// SiegeWarObject.txt : (type, description, réparations, coût, durée en minutes, objets requis [TID, nombre]) ; colonnes
/// par position (en-têtes en coréen), jusqu'à huit paires d'objets.
pub fn parse_siege_war_objects(text: &str) -> Vec<(u32, String, i64, i64, i64, Vec<(i64, i64)>)> {
    let (_, rows) = parse_tsv(text);
    rows.iter()
        .filter(|r| r.first().is_some_and(|t| is_uint(t.trim())))
        .map(|r| {
            let n = |i: usize| r.get(i).map(|s| js_number(s.trim(), 0.0) as i64).unwrap_or(0);
            let items = (0..8).map(|k| (n(5 + 2 * k), n(6 + 2 * k))).filter(|(tid, count)| *tid > 0 && *count > 0).collect();
            (n(0) as u32, r.get(1).map(|s| s.trim().to_string()).unwrap_or_default(), n(2), n(3), n(4), items)
        })
        .collect()
}

/// Niveaux d'entrée de RegionTemplate.ini : (numéro, LevelMin, LevelLimit), pour les régions qui en ont.
pub fn parse_region_levels(text: &str) -> Vec<(u32, i64, i64)> {
    parse_ini(text)
        .iter()
        .filter(|s| is_uint(&s.id))
        .map(|s| (s.id.parse().unwrap(), num(s.first("LevelMin")) as i64, num(s.first("LevelLimit")) as i64))
        .filter(|(_, min, limit)| *min > 0 || *limit > 0)
        .collect()
}

/// Boss de la fenêtre des boss d'Archonia (UIBoss.xml) : (TID, catégorie 1 à 7 ou 0, nom affiché), dans l'ordre du
/// fichier.
pub fn parse_boss_window(xml: &str) -> Vec<(u32, u8, String)> {
    let attr = |s: &str, name: &str| -> Option<String> {
        let at = s.find(&format!("{name}=\""))? + name.len() + 2;
        Some(s[at..].split('"').next()?.to_string())
    };
    let category = |c: &str| match c.trim().to_ascii_lowercase().as_str() {
        "chaoticfrontier" => 1,
        "asylum" => 2,
        "battlesquare" => 3,
        "shinewood" => 4,
        "heavenshell" => 5,
        "firmament" => 6,
        "purgatory" => 7,
        _ => 0,
    };
    let mut out = Vec::new();
    for boss in xml.split("<Boss ").skip(1) {
        let boss = boss.split("</Boss>").next().unwrap_or("");
        let Some(tid) = attr(boss, "TID").and_then(|t| t.trim().parse::<u32>().ok()) else { continue };
        let name = boss.find("<Name").and_then(|i| attr(&boss[i..], "Text")).unwrap_or_default();
        out.push((tid, category(&attr(boss, "Category").unwrap_or_default()), name.trim().chars().take(96).collect()));
    }
    out
}

/// pvpskulldroptable.txt : (écart de niveau tueur − victime, chance en pour cent) et (niveau de la victime de, à, niveau
/// du crâne) pour les lignes « 1:40 ».
pub fn parse_skull_table(text: &str) -> (Vec<(i64, f64)>, Vec<(i64, i64, i64)>) {
    let (mut chances, mut levels) = (Vec::new(), Vec::new());
    // La première ligne est l'en-tête.
    for line in text.lines().skip(1) {
        let cells: Vec<&str> = line.split('\t').map(str::trim).collect();
        let (Some(first), Some(second), Some(third)) = (cells.first(), cells.get(1), cells.get(2)) else { continue };
        if let Some((a, b)) = first.split_once(':') {
            if let (Ok(a), Ok(b), Ok(l)) = (a.trim().parse::<i64>(), b.trim().parse::<i64>(), second.parse::<i64>()) {
                levels.push((a, b, l));
            }
        } else if let (Ok(gap), Ok(chance)) = (first.parse::<i64>(), third.parse::<f64>()) {
            chances.retain(|(g, _)| *g != gap);
            chances.push((gap, chance.max(0.0)));
        }
    }
    (chances, levels)
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

/// Une ligne de charactercustomizelist.txt (coiffure ou visage proposé à un modèle).
#[derive(Clone, Debug, PartialEq)]
pub struct CustomizeRow {
    /// 1 coiffure (« Hair »), 2 visage (« Face »).
    pub kind: u8,
    pub number: i64,
    pub name: String,
    pub case_kind: i64,
    pub character_tid: u32,
    pub use_level: i64,
    pub price: i64,
    pub price_item_tid: u32,
}

/// Coiffures et visages (charactercustomizelist.txt), dans l'ordre du fichier ; une ligne de genre inconnu arrête la
/// lecture, comme le client (`Type` vide ou autre que Hair / Face = fin de table).
pub fn parse_customize(text: &str) -> Vec<CustomizeRow> {
    let (h, rows) = parse_tsv(text);
    let c = ["Type", "Number", "Sell Name", "Case", "CharacterTID", "UseLevel", "Price(Money)", "Price(Skull)"].map(|n| h.iter().position(|x| x == n));
    let mut out = Vec::new();
    for r in &rows {
        let cell = |i: Option<usize>| i.and_then(|i| r.get(i)).map(|s| s.trim().to_string()).unwrap_or_default();
        let n = |i: Option<usize>| js_number(&cell(i), 0.0) as i64;
        let kind = match cell(c[0]).as_str() {
            "Hair" => 1,
            "Face" => 2,
            _ => break,
        };
        out.push(CustomizeRow {
            kind,
            number: n(c[1]),
            name: cell(c[2]),
            case_kind: n(c[3]),
            character_tid: n(c[4]) as u32,
            use_level: n(c[5]),
            price: n(c[6]),
            price_item_tid: n(c[7]) as u32,
        });
    }
    out
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

/// Un élément d'emblème de guildmark.txt (lu comme AgpmGuild::ReadGuildMarkTemplate, FUN_1402050f0).
#[derive(Debug, Clone, PartialEq)]
pub struct GuildMarkRow {
    /// Numéro décalé de 8 bits par couche : fond, motif << 8, symbole << 16, couleur << 24.
    pub tid: u32,
    /// 1 fond (Bottom), 2 motif (Pattern), 3 symbole (Symbol), 4 couleur (Color).
    pub layer: u8,
    pub number: i64,
    pub name: String,
    pub gold: i64,
    pub skull_tid: i64,
    pub skull_count: i64,
    /// Prix en argent liquide (colonne « Price (CC) »).
    pub cash: i64,
    /// Couleur ARGB (couche 4, cinquième colonne en hexadécimal).
    pub argb: i64,
}

/// guildmark.txt : type, numéro, nom, or, crâne (TID, ou couleur ARGB pour « Color »), nombre de crânes,
/// argent liquide.
pub fn parse_guild_marks(text: &str) -> Vec<GuildMarkRow> {
    let (_, rows) = parse_tsv(text);
    rows.iter()
        .filter_map(|r| {
            let cell = |i: usize| r.get(i).map(|s| s.trim()).unwrap_or("");
            let layer = match cell(0) {
                "Bottom" => 1u8,
                "Pattern" => 2,
                "Symbol" => 3,
                "Color" => 4,
                _ => return None,
            };
            let number = js_number(cell(1), 0.0) as i64;
            let num = |i: usize| js_number(cell(i), 0.0) as i64;
            if !(1..=255).contains(&number) || cell(2).is_empty() {
                return None;
            }
            let (skull_tid, argb) = if layer == 4 { (0, i64::from_str_radix(cell(4), 16).unwrap_or(0)) } else { (num(4), 0) };
            Some(GuildMarkRow {
                tid: (number as u32) << (8 * (layer as u32 - 1)),
                layer,
                number,
                name: cell(2).to_string(),
                gold: num(3),
                skull_tid,
                skull_count: num(5),
                cash: num(6),
                argb,
            })
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

/// Catégorie de production (colonne de productcategory.txt et productcompose.txt, sans casse) : Hunt 0, Cook 1,
/// Alchemy 2 ; une autre valeur n'en est pas une.
pub fn product_category(name: &str) -> Option<i64> {
    ["hunt", "cook", "alchemy"].iter().position(|c| c.eq_ignore_ascii_case(name.trim())).map(|i| i as i64)
}

/// productcategory.txt : les lignes de compétence (catégorie en colonne 0), pas les lignes de recette qui suivent.
/// Rend (catégorie, nom de la compétence, prix, niveau, PNJ).
pub fn parse_product_categories(text: &str) -> Vec<(i64, String, i64, i64, String)> {
    let (_, rows) = parse_tsv(text);
    rows.iter()
        .filter_map(|r| {
            let cell = |i: usize| r.get(i).map(|s| s.trim()).unwrap_or("");
            let category = product_category(cell(0))?;
            Some((category, cell(1).to_string(), js_number(cell(3), 0.0) as i64, js_number(cell(4), 0.0) as i64, cell(6).to_string()))
        })
        .collect()
}

/// Une recette de productcompose.txt.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ComposeRow {
    pub result_tid: u32,
    pub category: i64,
    pub name: String,
    pub title_tid: u32,
    pub item_level: i64,
    pub price: i64,
    pub success_rate: i64,
    pub recipe_tid: u32,
    pub recipe_bonus: i64,
    /// (TID, quantité) des colonnes ItemA à ItemD renseignées, dans l'ordre (un matériau peut revenir).
    pub materials: Vec<(u32, i64)>,
}

/// productcompose.txt : colonnes lues par leur en-tête ; ligne sans catégorie ou sans objet produit ignorée,
/// niveau exigé 1 par défaut.
pub fn parse_product_composes(text: &str) -> Vec<ComposeRow> {
    let (header, rows) = parse_tsv(text);
    let col = |name: &str| header.iter().position(|h| h == name);
    let first_material = col("ItemA_TID");
    rows.iter()
        .filter_map(|r| {
            let cell = |c: Option<usize>| c.and_then(|i| r.get(i)).map(|s| s.trim()).unwrap_or("");
            let n = |c: Option<usize>, d: f64| js_number(cell(c), d);
            let category = product_category(cell(col("Category")))?;
            let result_tid = n(col("Result_TID"), 0.0) as u32;
            if result_tid == 0 {
                return None;
            }
            let materials = (0..4)
                .filter_map(|k| {
                    let at = first_material? + k * 3;
                    let (tid, count) = (n(Some(at), 0.0) as u32, n(Some(at + 2), 0.0) as i64);
                    (tid > 0 && count > 0).then_some((tid, count))
                })
                .collect();
            let level = n(col("ItemLv"), 1.0) as i64;
            Some(ComposeRow {
                result_tid,
                category,
                name: cell(col("Result_name")).chars().take(64).collect(),
                title_tid: n(col("UsingMonster"), 0.0) as u32,
                item_level: if level > 0 { level } else { 1 },
                price: n(col("MakingPrice"), 0.0) as i64,
                success_rate: n(col("SucessProb"), 0.0) as i64,
                recipe_tid: n(col("Recipe_TID"), 0.0) as u32,
                recipe_bonus: n(col("Recipe_addProb"), 0.0) as i64,
                materials,
            })
        })
        .collect()
}

/// productexp.txt : (nom de la compétence, niveau, expérience min et max par action, facteur de réussite,
/// expérience du niveau, expérience cumulée) ; niveaux 1 à 15 seulement, comme le client.
pub fn parse_product_exp(text: &str) -> Vec<(String, i64, i64, i64, i64, i64, i64)> {
    let (_, rows) = parse_tsv(text);
    rows.iter()
        .filter_map(|r| {
            let cell = |i: usize| r.get(i).map(|s| s.trim()).unwrap_or("");
            let n = |i: usize| js_number(cell(i), 0.0) as i64;
            let level = n(1);
            (!cell(0).is_empty() && (1..=15).contains(&level)).then(|| (cell(0).to_string(), level, n(2), n(3), n(4), n(5), n(6)))
        })
        .collect()
}

/// productgather.txt : (TID du monstre, classes en bits 1 à 4, compétence, chance, ensemble) des lignes qui ont une
/// compétence (les autres ne donnent qu'un TID). Les classes « 1;2;3;4 » hors 1 à 4 sont ignorées, comme par le client.
pub fn parse_product_gather(text: &str) -> Vec<(u32, u8, u32, i64, i64)> {
    let (_, rows) = parse_tsv(text);
    rows.iter()
        .filter_map(|r| {
            let cell = |i: usize| r.get(i).map(|s| s.trim()).unwrap_or("");
            let (tid, skill) = (js_number(cell(1), 0.0) as u32, js_number(cell(3), 0.0) as u32);
            if tid == 0 || skill == 0 {
                return None;
            }
            let classes = cell(2).split(';').filter_map(|c| c.trim().parse::<u8>().ok()).filter(|c| (1..=4).contains(c)).fold(0u8, |m, c| m | (1 << c));
            Some((tid, classes, skill, js_number(cell(4), 0.0) as i64, js_number(cell(5), 0.0) as i64))
        })
        .collect()
}

/// productresultitem.txt : (ensemble, niveau de compétence, objet, poids). L'ensemble (colonne 0) et le niveau
/// (colonne 3) ne sont donnés que sur la première ligne de leur bloc : les lignes suivantes en héritent.
pub fn parse_product_results(text: &str) -> Vec<(i64, i64, u32, i64)> {
    let (_, rows) = parse_tsv(text);
    let (mut set, mut level) = (0i64, 0i64);
    let mut out = Vec::new();
    for r in &rows {
        let cell = |i: usize| r.get(i).map(|s| s.trim()).unwrap_or("");
        if is_uint(cell(0)) {
            set = js_number(cell(0), 0.0) as i64;
        }
        if is_uint(cell(3)) {
            level = js_number(cell(3), 0.0) as i64;
        }
        let tid = js_number(cell(1), 0.0) as u32;
        if set > 0 && level > 0 && tid > 0 {
            out.push((set, level, tid, js_number(cell(2), 0.0) as i64));
        }
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
        let (mut levels, missing_const) = parse_skill_levels(&src.read("SKILL_CONST.txt")?, &tids);
        if src.exists("Skill_Const2.txt") {
            let (more, _) = parse_skill_levels(&src.read("Skill_Const2.txt")?, &tids);
            println!("✓ Skill_Const2.txt : {} niveau(x) complété(s)", merge_skill_levels(&mut levels, more));
        }
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

    // ---------------------------------------------------------------- Product*.txt (production)
    // Skill_Product.txt (ancien système, noms en GBK) et les colonnes Product_* de CharacterDataTable (vides) ne
    // servent pas.
    if src.exists("productcompose.txt") && src.exists("SKILLTEMPLATE.ini") {
        let tids = skill_tids(&src.read("SKILLTEMPLATE.ini")?);
        if src.exists("productcategory.txt") {
            let mut missing = 0;
            let out: Vec<Vec<Value>> = parse_product_categories(&src.read("productcategory.txt")?)
                .into_iter()
                .filter_map(|(category, skill, price, level, npc)| {
                    let Some(&tid) = tids.get(&skill) else {
                        missing += 1;
                        return None;
                    };
                    Some(vec![v(category), v(tid), v(skill), v(price), v(level), v(npc)])
                })
                .collect();
            let n = replace(db, "product_category", "INSERT IGNORE INTO product_category (category,skill_tid,skill_name,price,level,npc) VALUES (?,?,?,?,?,?)", out).await?;
            println!("✓ product_category : {n} ({missing} compétence(s) sans modèle)");
        }
        let out: Vec<Vec<Value>> = parse_product_composes(&src.read("productcompose.txt")?)
            .into_iter()
            .map(|c| {
                vec![
                    v(c.result_tid), v(c.category), v(c.name), v(c.title_tid), v(c.item_level), v(c.price), v(c.success_rate),
                    v(c.recipe_tid), v(c.recipe_bonus), v(json!(c.materials).to_string()),
                ]
            })
            .collect();
        let n = replace(db, "product_compose", "INSERT IGNORE INTO product_compose (result_tid,category,name,title_tid,item_level,price,success_rate,recipe_tid,recipe_bonus,materials) VALUES (?,?,?,?,?,?,?,?,?,?)", out).await?;
        println!("✓ product_compose : {n}");
        if src.exists("productexp.txt") {
            let mut missing = 0;
            let out: Vec<Vec<Value>> = parse_product_exp(&src.read("productexp.txt")?)
                .into_iter()
                .filter_map(|(skill, level, lo, hi, bonus, level_exp, total)| {
                    let Some(&tid) = tids.get(&skill) else {
                        missing += 1;
                        return None;
                    };
                    Some(vec![v(tid), v(level), v(lo), v(hi), v(bonus), v(level_exp), v(total)])
                })
                .collect();
            let n = replace(db, "product_exp", "INSERT IGNORE INTO product_exp (skill_tid,level,exp_min,exp_max,rate_bonus,level_exp,total_exp) VALUES (?,?,?,?,?,?,?)", out).await?;
            println!("✓ product_exp : {n} ({missing} ligne(s) sans compétence connue)");
        }
        if src.exists("productgather.txt") {
            let out: Vec<Vec<Value>> = parse_product_gather(&src.read("productgather.txt")?).into_iter().map(|(t, c, s, r, i)| vec![v(t), v(c), v(s), v(r), v(i)]).collect();
            let n = replace(db, "product_gather", "INSERT IGNORE INTO product_gather (monster_tid,classes,skill_tid,rate,item_set) VALUES (?,?,?,?,?)", out).await?;
            println!("✓ product_gather : {n}");
        }
        if src.exists("productresultitem.txt") {
            let out: Vec<Vec<Value>> = parse_product_results(&src.read("productresultitem.txt")?).into_iter().map(|(s, l, t, w)| vec![v(s), v(l), v(t), v(w)]).collect();
            let n = replace(db, "product_result_item", "INSERT IGNORE INTO product_result_item (item_set,skill_level,item_tid,weight) VALUES (?,?,?,?)", out).await?;
            println!("✓ product_result_item : {n}");
        }
    }

    // ---------------------------------------------------------------- ItemConvertTable.txt, itemoptiontable.txt
    // Textes entiers (ItemConvertTable : plusieurs tableaux séparés par des lignes vides) : le monde les lit
    // au chargement.
    for name in ["ItemConvertTable.txt", "itemoptiontable.txt", "itemruneattributetable.txt", "RefineryItem2.txt"] {
        if src.exists(name) {
            store_text(db, name, &src.read(name)?).await?;
        }
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
        let mut siege_points = Vec::new();
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
            // Points des sièges (types 3 à 7), à part : ils ne servent qu'aux résurrections de siège (world/siege).
            let mut siege_ranks: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
            for b in parse_bindings(&text).into_iter().filter(|b| (3..=7).contains(&b.binding_type)) {
                let rank = siege_ranks.entry(b.object.clone()).or_insert(0);
                *rank += 1;
                let key = format!("{file}#{}#s{rank}", b.object);
                if let Some(&region) = by_name.get(b.town.as_str()) {
                    siege_points.push((region, key, b));
                }
            }
        }
        let out: Vec<Vec<Value>> = siege_points
            .iter()
            .enumerate()
            .map(|(i, (region, key, b))| vec![v(i as u32 + 1), v(key.clone()), v(*region), v(b.name.clone()), v(b.binding_type), v(b.pos[0]), v(b.pos[1]), v(b.pos[2])])
            .collect();
        let k = replace(db, "siege_binding", "INSERT INTO siege_binding (id,stable_key,region_id,name,binding_type,pos_x,pos_y,pos_z) VALUES (?,?,?,?,?,?,?,?)", out).await?;
        println!("✓ siege_binding : {k}");
        // ------------------------------------------------------------ SiegeWar.txt, SiegeWarObject.txt (guerres de siège)
        if src.exists("SiegeWar.txt") {
            let rows = parse_siege_war(&src.read_utf8_or_korean("SiegeWar.txt")?);
            let region = |name: &str| by_name.get(name).copied().unwrap_or(0);
            let out: Vec<Vec<Value>> = rows
                .iter()
                .map(|r| {
                    vec![
                        v(r.id), v(r.name.clone()), v(region(&r.regions.0)), v(region(&r.regions.1)), v(r.playing_ms), v(r.eye_open_ms), v(r.stamping_ms),
                        v(r.declare_hours), v(r.select_hours), v(r.interval_days), v(r.begin_hour), v(r.stamp_skill_tid), v(r.stamp_skill_level), v(r.disabled as u8),
                    ]
                })
                .collect();
            let unknown = rows.iter().filter(|r| region(&r.regions.0) == 0).count();
            let n = replace(
                db,
                "siege_war",
                "INSERT INTO siege_war (id,name,castle_region,outskirts_region,playing_ms,eye_open_ms,stamping_ms,declare_hours,select_hours,interval_days,begin_hour,stamp_skill_tid,stamp_skill_level,disabled) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                out,
            )
            .await?;
            println!("✓ siege_war : {n} ({unknown} sans région connue)");
        }
        let objects = ["siegewarobject.txt", "SiegeWarObject.txt"].into_iter().find(|f| src.exists(f));
        if let Some(file) = objects {
            let out: Vec<Vec<Value>> = parse_siege_war_objects(&src.read(file)?)
                .into_iter()
                .map(|(t, d, count, cost, minutes, items)| vec![v(t), v(d), v(count), v(cost), v(minutes), v(serde_json::to_string(&items.iter().map(|(a, b)| [*a, *b]).collect::<Vec<_>>()).unwrap())])
                .collect();
            let n = replace(db, "siege_war_object", "INSERT INTO siege_war_object (type,description,repair_count,repair_cost,repair_minutes,items) VALUES (?,?,?,?,?,?)", out).await?;
            println!("✓ siege_war_object : {n}");
        }
        // Niveaux d'entrée (étages de la Battle Square).
        let levels: std::collections::HashMap<u32, (i64, i64)> = parse_region_levels(&src.read("RegionTemplate.ini")?).into_iter().map(|(r, a, b)| (r, (a, b))).collect();
        let out: Vec<Vec<Value>> = regions
            .iter()
            .map(|r| {
                let (min, limit) = levels.get(&r.0).copied().unwrap_or((0, 0));
                vec![v(r.0), v(r.1.clone()), v(r.2), v(r.3), v((r.3 >> 2) & 3), v(min), v(limit)]
            })
            .collect();
        let n = replace(db, "region_template", "INSERT INTO region_template (id,name,parent_id,type_bits,safety,level_min,level_limit) VALUES (?,?,?,?,?,?,?)", out).await?;
        let out: Vec<Vec<Value>> = points
            .iter()
            .enumerate()
            .map(|(i, (region, key, b))| vec![v(i as u32 + 1), v(key.clone()), v(*region), v(b.name.clone()), v(b.pos[0]), v(b.pos[1]), v(b.pos[2]), v(b.race), v(b.class)])
            .collect();
        let m = replace(db, "resurrection_point", "INSERT INTO resurrection_point (id,stable_key,region_id,name,pos_x,pos_y,pos_z,race,class) VALUES (?,?,?,?,?,?,?,?,?)", out).await?;
        println!("✓ region_template : {n} ({} avec un niveau d'entrée) ; resurrection_point : {m} (sur {} fichier(s) obj*.ini)", levels.len(), files.len());
    }

    // ---------------------------------------------------------------- UIBoss.xml (fenêtre des boss d'Archonia)
    if src.exists("UIBoss.xml") {
        let out: Vec<Vec<Value>> =
            parse_boss_window(&src.read("UIBoss.xml")?).into_iter().enumerate().map(|(i, (tid, category, name))| vec![v(tid), v(i as u32), v(category), v(name)]).collect();
        let n = replace(db, "boss_window", "INSERT INTO boss_window (tid,row_order,category,name) VALUES (?,?,?,?)", out).await?;
        println!("✓ boss_window : {n}");
    }

    // ---------------------------------------------------------------- pvpskulldroptable.txt (crânes de la Battle Square)
    if src.exists("pvpskulldroptable.txt") {
        let (chances, levels) = parse_skull_table(&src.read("pvpskulldroptable.txt")?);
        let n = replace(db, "pvp_skull_chance", "INSERT INTO pvp_skull_chance (level_gap,chance) VALUES (?,?)", chances.iter().map(|(g, c)| vec![v(*g), v(*c)]).collect()).await?;
        let out = levels.iter().map(|(a, b, l)| vec![v(*a), v(*b), v(*l)]).collect();
        let m = replace(db, "pvp_skull_level", "INSERT INTO pvp_skull_level (victim_level_min,victim_level_max,skull_level) VALUES (?,?,?)", out).await?;
        println!("✓ pvp_skull_chance : {n} ; pvp_skull_level : {m}");
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

    // ---------------------------------------------------------------- guildmark.txt (emblèmes de guilde)
    if src.exists("guildmark.txt") {
        let out: Vec<Vec<Value>> = parse_guild_marks(&src.read("guildmark.txt")?)
            .into_iter()
            .map(|m| vec![v(m.tid), v(m.layer), v(m.number), v(m.name), v(m.gold), v(m.skull_tid), v(m.skull_count), v(m.cash), v(m.argb)])
            .collect();
        let n = replace(db, "guild_mark", "INSERT INTO guild_mark (tid,layer,number,name,gold,skull_tid,skull_count,cash,argb) VALUES (?,?,?,?,?,?,?,?,?)", out).await?;
        println!("✓ guild_mark : {n}");
    }

    // ---------------------------------------------------------------- bankslotprice.txt (pages de banque)
    if src.exists("bankslotprice.txt") {
        let out: Vec<Vec<Value>> = parse_bank_slot_prices(&src.read("bankslotprice.txt")?).into_iter().map(|(s, p)| vec![v(s), v(p)]).collect();
        let n = replace(db, "bank_slot_price", "INSERT INTO bank_slot_price (slot,price) VALUES (?,?)", out).await?;
        println!("✓ bank_slot_price : {n}");
    }

    // ---------------------------------------------------------------- charactercustomizelist.txt (coiffures, visages)
    if src.exists("charactercustomizelist.txt") {
        let out: Vec<Vec<Value>> = parse_customize(&src.read("charactercustomizelist.txt")?)
            .into_iter()
            .enumerate()
            .map(|(i, r)| vec![v(i as u32), v(r.kind), v(r.number), v(r.name), v(r.case_kind), v(r.character_tid), v(r.use_level), v(r.price), v(r.price_item_tid)])
            .collect();
        let n = replace(
            db,
            "character_customize",
            "INSERT INTO character_customize (row_order,kind,number,name,case_kind,character_tid,use_level,price,price_item_tid) VALUES (?,?,?,?,?,?,?,?,?)",
            out,
        )
        .await?;
        println!("✓ character_customize : {n}");
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
    fn niveaux_de_competence_et_skill_const2() {
        let tids: HashMap<String, u32> = [("Lethal Attack".to_string(), 30)].into_iter().collect();
        let (mut levels, missing) = parse_skill_levels("name\tskill_level\tduration\r\nLethal Attack\t1\t396000\r\n\t2\t402000\r\nInconnue\t1\t5\r\n", &tids);
        assert_eq!((levels.len(), missing), (2, 1));
        let (more, _) = parse_skill_levels("name2\tskill_level2\tignore_physical_defence_percent2\r\nLethal Attack\t1\t5\r\n\t2\t\r\n", &tids);
        assert_eq!(merge_skill_levels(&mut levels, more), 1, "le niveau 2 n'a rien en plus");
        assert_eq!(levels[0].2.get("ignore_physical_defence_percent2").and_then(|v| v.as_str()), Some("5"));
        assert_eq!(levels[0].2.get("duration").and_then(|v| v.as_str()), Some("396000"));
        assert!(levels[1].2.get("skill_level2").is_none());
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
        let marks = parse_guild_marks("Type\tNumber\tName\tPrice\tSkull\tN\tCC\r\nBottom\t3\tFond 3\t10000\t0\t0\t\r\nSymbol\t65\tSymbole\t40000\t0\t0\t99999\r\nColor\t2\tColor 2\t0\tFF6C261A\t0\t\r\n");
        assert_eq!((marks[0].tid, marks[0].gold, marks[1].tid, marks[1].cash), (3, 10_000, 65 << 16, 99_999));
        assert_eq!((marks[2].layer, marks[2].tid, marks[2].argb), (4, 2 << 24, 0xFF6C_261A));
    }

    #[test]
    fn coiffures_et_visages() {
        let text = "Type\tNumber\tSell Name\tCase\tCharacterTID\tUseLevel\tPrice(Money)\tPrice(Skull)\r\n\
                    Hair\t0\tSpiked 1\t0\t96\t1\t0\t0\r\nHair\t4\tTop Volume 1\t1\t96\t7\t1000\t0\r\n\
                    Face\t2\tType 12\t1\t96\t10\t3000\t0\r\n-\t0\t\t0\t0\t0\t0\t0\r\nFace\t3\tAprès la fin\t0\t96\t1\t0\t0\r\n";
        let rows = parse_customize(text);
        assert_eq!(rows.len(), 3, "une ligne de genre inconnu clôt la table");
        assert_eq!((rows[1].kind, rows[1].number, rows[1].name.as_str(), rows[1].case_kind, rows[1].use_level, rows[1].price), (1, 4, "Top Volume 1", 1, 7, 1000));
        assert_eq!((rows[2].kind, rows[2].number, rows[2].character_tid, rows[2].price_item_tid), (2, 2, 96, 0));
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
    fn tables_de_production() {
        let category = "분류\tSkill\tNemu\t겔드\t레벨\t비고\t판매 NPC\r\n\
                        Hunt\tCollection Skill\t\t100\t1\t\tAdventurer\r\n\
                        Cook\tCooking\t\t1000\t15\tMastery Level\tCook\r\n\
                        \t\tBroiled Meat of Vitality\t500\t1\t\t\r\n\
                        ALCHEMY\tAlchemy\t\t2000\t20\t\tAlchemist\r\n";
        let c = parse_product_categories(category);
        assert_eq!(c.iter().map(|r| (r.0, r.1.as_str(), r.2)).collect::<Vec<_>>(), vec![(0, "Collection Skill", 100), (1, "Cooking", 1000), (2, "Alchemy", 2000)]);
        let compose = "MakeId\tCategory\tResult_TID\tResult_name\tUsingMonster\tItemLv\tMakingPrice\tSucessProb\tRecipe_TID\tRecipe_Name\tRecipe_addProb\tItemA_TID\tItemA_name\tItemA_quantity\tItemB_TID\tItemB_name\tItemB_quantity\tItemC_TID\tItemC_name\tItemC_quantity\tItemD_TID\tItemD_name\tItemD_quantity\r\n\
                       \tCook\t644\tBroiled Meat of Vitality\t\t1\t500\t70\t623\tSalty Seasoning\t5\t4960\tBug Meat\t3\t4963\tSpider Web\t1\t641\tPotato\t1\t\t\t\r\n\
                       \tAlchemy\t406\tSpider Potion\t\t\t500\t70\t427\tRecipe\t5\t4960\tBug Meat\t3\t4963\tSpider Web\t2\t\t\t\t\t\t\r\n\
                       \tCook\t650\tSoup of Fury\t\t4\t900\t60\t\t\t\t4938\tMeat\t2\t4938\tMeat\t2\t\t\t\t\t\t\r\n\
                       \t\t999\tSans catégorie\t\t1\t1\t1\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\r\n";
        let r = parse_product_composes(compose);
        assert_eq!(r.len(), 3);
        assert_eq!(
            r[0],
            ComposeRow {
                result_tid: 644, category: 1, name: "Broiled Meat of Vitality".into(), title_tid: 0, item_level: 1, price: 500, success_rate: 70,
                recipe_tid: 623, recipe_bonus: 5, materials: vec![(4960, 3), (4963, 1), (641, 1)],
            }
        );
        assert_eq!((r[1].category, r[1].item_level, r[1].materials.clone()), (2, 1, vec![(4960, 3), (4963, 2)]), "niveau vide : 1");
        assert_eq!(r[2].materials, vec![(4938, 2), (4938, 2)], "matériau répété gardé tel quel");
        let exp = "스킬종류\t레벨\tmin\tmax\t팩터\t레벨업\t누적\r\n\
                   Collection Skill\t1\t5\t7\t0\t103\t103\t\t\r\n\
                   Collection Skill\t3\t10\t13\t4\t597\t960\t\t\r\n\
                   Collection Skill\t16\t1\t1\t1\t1\t1\r\n";
        assert_eq!(parse_product_exp(exp), vec![("Collection Skill".into(), 1, 5, 7, 0, 103, 103), ("Collection Skill".into(), 3, 10, 13, 4, 597, 960)]);
        let gather = "Target Type\tTarget TID\tClass\tSkill TID\tProbability\tItem Set\r\n\
                      \t33\t1;2;3;4\t663\t90\t2\r\n\
                      \t450\t2;7\t663\t50\t46\r\n\
                      \t1200\t\t\t\t\r\n";
        assert_eq!(parse_product_gather(gather), vec![(33, 0b11110, 663, 90, 2), (450, 0b100, 663, 50, 46)]);
        let results = "Item Set\tItem TID\tProbability\tSkill Level\n\
                       1\t4930\t40\t1\n\
                       \t4921\t30\t\n\
                       \t4933\t30\t\n\
                       \t4930\t40\t2\n\
                       2\t4963\t100\t1\n";
        assert_eq!(parse_product_results(results), vec![(1, 1, 4930, 40), (1, 1, 4921, 30), (1, 1, 4933, 30), (1, 2, 4930, 40), (2, 1, 4963, 100)]);
    }

    #[test]
    fn delais_de_la_fenetre_des_boss() {
        let xml = "<UIBoss>\n\t<Boss TID=\"2073\" Category=\"Battlesquare\" Icon=\"14\">\n\t\t<Name Text=\"Basilis (Lv.30)\"/>\n\t\t<SpawnPeriod Text=\"Spawn Period: 30-45 minutes\"/>\n\t</Boss>\n\
                   \t<Boss TID=\"5794\" Category=\"Firmament\" Icon=\"1\">\n\t\t<SpawnPeriod Text=\"Event-based spawn\"/>\n\t</Boss>\n\
                   \t<Boss TID=\"5799\" Category=\"Firmament\" Icon=\"2\">\n\t\t<SpawnPeriod Text=\"25-35 minutes\"/> \n\t</Boss>\n</UIBoss>";
        assert_eq!(parse_boss_periods(xml), vec![(2073, 1800, 2700), (5799, 1500, 2100)]);
        assert_eq!(parse_boss_window(xml), vec![(2073, 3, "Basilis (Lv.30)".to_string()), (5794, 6, String::new()), (5799, 6, String::new())]);
    }

    #[test]
    fn niveaux_des_regions_et_cranes() {
        let r = parse_region_levels("[198]\nName=BS\nLevelLimit=30\nLevelMin=20\n[9]\nName=Anchorville\n");
        assert_eq!(r, vec![(198, 20, 30)]);
        let (chances, levels) = parse_skull_table("PC\tniveau\tchance\t\r\n40\t1\t0\t\r\n5\t1\t10\t\r\n-35\t1\t1\t\r\n1:40\t1\t-1\t\r\n41:70\t2\t-1\r\n");
        assert_eq!(chances, vec![(40, 0.0), (5, 10.0), (-35, 1.0)]);
        assert_eq!(levels, vec![(1, 40, 1), (41, 70, 2)]);
    }

    #[test]
    fn glossaire_des_regions() {
        // Clé en EUC-KR, nom en Windows-1252 (dossier ini_fr) ; ligne sans tabulation ignorée, doublon gardé une fois.
        let mut plain = encoding_rs::EUC_KR.encode("인플레임 성 인근").0.into_owned();
        plain.extend(b"\tCh\xe2teau d'Inflame \r\nsans tabulation\r\n");
        plain.extend(encoding_rs::EUC_KR.encode("인플레임 성 인근\tdoublon\r\n").0.iter());
        let rows = parse_region_glossary(&plain, encoding_rs::WINDOWS_1252);
        assert_eq!(rows, vec![("인플레임 성 인근".to_string(), "Château d'Inflame".to_string())]);
        assert_eq!(glossary_folder("en", None).0, "ini");
        assert_eq!(glossary_folder("fr", Some("ini_fr")).0, "ini_fr");
        assert_eq!(glossary_folder("zh-CN", None), ("ini_zh".to_string(), encoding_rs::GBK));
        assert_eq!(glossary_folder("ko", None).1, encoding_rs::EUC_KR);
    }

    #[test]
    fn chateaux_et_objets_de_siege() {
        let text = "Index\tCastle\tRegion\tSWPlayingTime\tArchonEyeOpenTime\tStampingTime\tNextSWDeclaring(Hours)\tSelectGuildForSW(Hours)\tDuration4nextSW(Days)\tSWBeginTime\tStampSkillTID\tStampSkillLevel\r\n\
                    0\tRensphere\t렌스피어 성/렌스피어 성 인근\t3600000\t0\t300000\t0\t0\t30\t20\t930\t1\r\n\
                    4\tDrayger\t드래이거 성/드래이거 성 인근\t3600000\t300000\t180000\t144\t143\t7\t30\t\t\r\n";
        let rows = parse_siege_war(text);
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].id, rows[0].name.as_str(), rows[0].regions.1.as_str(), rows[0].stamp_skill_tid, rows[0].interval_days), (0, "Rensphere", "렌스피어 성 인근", 930, 30));
        assert_eq!((rows[1].regions.0.as_str(), rows[1].eye_open_ms, rows[1].declare_hours, rows[1].begin_hour, rows[1].disabled), ("드래이거 성", 300_000, 144, 21, false));
        let objects = parse_siege_war_objects("타입\t타입설명\t수리횟수\t수리비용\t수리시간(분)\t필요아이템(TID)\t필요아이템(개수)\r\n10\t투석기\t5\t500000\t5\t4553\t1\r\n9\t공성부활탑\t5\t500000\t5\t4554\t1\r\n");
        assert_eq!(objects[0], (10, "투석기".to_string(), 5, 500_000, 5, vec![(4553, 1)]));
        assert_eq!((objects[1].0, objects[1].5.clone()), (9, vec![(4554, 1)]));
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
