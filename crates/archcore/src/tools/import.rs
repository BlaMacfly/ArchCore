//! `archcore import [dossier]` : importe les tables déchiffrées du client (`data/client-ini`) dans
//! `archcore_world`. Les fichiers sont en CP949 (coréen) ; ils sont convertis en UTF-8.

use crate::db::Databases;
use mysql_async::Value;
use serde_json::{json, Map};
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

async fn replace(db: &Databases, table: &str, sql: &str, rows: Vec<Vec<Value>>) -> anyhow::Result<usize> {
    let n = rows.len();
    db.world.exec(&format!("DELETE FROM {table}"), ()).await?;
    for chunk in rows.chunks(500) {
        db.world.batch(sql, chunk.to_vec()).await?;
    }
    Ok(n)
}

pub async fn run(args: &[String]) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let dir = args.first().map(PathBuf::from).unwrap_or_else(|| cfg.root.join("data").join("client-ini"));
    let src = Source(dir);
    let db = Databases::new(&cfg.database);
    import_all(&db, &src).await?;
    db.close().await;
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
}
