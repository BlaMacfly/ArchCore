//! `archcore export-client-tables <dossier> [--game dossier]` : écrit, depuis la base monde (valeurs de l'épisode 8,
//! couche `content/` comprise), les tables de valeurs au format exact du client, pour qu'ArchRust affiche les mêmes
//! valeurs que le serveur (dossier « données du serveur » d'ArchRust, `server-data/` à côté de `realmlist.wtf`).
//!
//! ```text
//!   Dossiers : ini/ et chaque dossier de langue du client (ini_fr, ini_de…) qui a sa propre copie de la table.
//!   Fichiers : noms, en-têtes et ordre des colonnes du client (Game/ini) ; une colonne propre à Archonia reste
//!        présente et vide ; texte en clair, fins de ligne CRLF, page de code du dossier : CP949 pour ini/ et
//!        ini_ko, Windows-1252 pour ini_fr, ini_de, ini_es, Shift-JIS pour ini_ja, GBK pour ini_zh.
//!   Cellules : valeur de la base ; dans un dossier de langue, un texte traduit (cellule non numérique qui
//!        diffère de celle de ini/) est gardé ; une cellule dont la valeur ne change pas garde ses octets.
//!   Lignes : celles de la base (lignes d'Archonia absentes). Quêtes (questtemplate.ini) : lignes de la base sous
//!        l'en-tête du client, textes d'un dossier de langue repris des textes de Webzen de la langue
//!        (`questtemplate.ini@<langue>` de client_table, import `--webzen`) ; groupes (questgroup.ini) : ceux de la base.
//!        Titres (titledatatable.txt, titlestringtable.txt) et maîtrises (SkillMastery_High.txt,
//!        SkillMastery_Heroic.txt) : fichiers rangés par l'import dans client_table (Webzen), sinon lignes du client
//!        gardées par l'import ; fenêtre des boss : lignes du client gardées.
//!   tables.toml : empreinte « sha256:… » des fichiers écrits (chemin et contenu), version d'ArchCore, date. Le
//!        serveur monde l'annonce (capacité client-tables/1, HELLO_ACK champ 21) quand `Client.TablesDir`
//!        désigne ce dossier.
//! ```
//! Le dossier est local (données du jeu) : il ne va jamais dans un dépôt.

use crate::db::{col, Databases};
use encoding_rs::Encoding;
use serde_json::Value as Json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// Fichier de l'empreinte, à la racine du dossier exporté (lu par ArchRust et par le serveur monde).
pub const MANIFEST_FILE: &str = "tables.toml";

/// Tables exportées (noms du client). Les dossiers de langue n'ont que celles dont ils ont une copie.
pub const TABLES: [&str; 23] = [
    "itemdatatable.txt", "CharacterDataTable.txt", "itemoptiontable.txt", "itemruneattributetable.txt", "SKILLTEMPLATE.ini", "SKILL_SPEC.txt",
    "SKILL_CONST.txt", "Skill_Const2.txt", "RegionTemplate.ini", "questtemplate.ini", "titledatatable.txt", "RefineryItem2.txt", "ItemConvertTable.txt",
    "TeleportPoint.ini", "TeleportFee.txt", "LevelUpExp.txt", "GrowUpFactor.txt", "npc.ini", "questgroup.ini", "UIBoss.xml", "titlestringtable.txt",
    "SkillMastery_High.txt", "SkillMastery_Heroic.txt",
];

/// Page de code d'un dossier de tables du client.
pub fn folder_encoding(folder: &str) -> &'static Encoding {
    match folder.to_ascii_lowercase().as_str() {
        "ini" | "ini_ko" => encoding_rs::EUC_KR,
        "ini_ja" => encoding_rs::SHIFT_JIS,
        "ini_zh" => encoding_rs::GBK,
        _ => encoding_rs::WINDOWS_1252,
    }
}

/// Texte → octets de la page de code ; un caractère qu'elle n'a pas devient « ? ».
pub fn encode(s: &str, e: &'static Encoding) -> Vec<u8> {
    let (b, _, bad) = e.encode(s);
    if !bad {
        return b.into_owned();
    }
    let mut out = Vec::new();
    let mut buf = [0u8; 4];
    for ch in s.chars() {
        let (b, _, bad) = e.encode(ch.encode_utf8(&mut buf));
        if bad {
            out.push(b'?');
        } else {
            out.extend_from_slice(&b);
        }
    }
    out
}

fn decode(b: &[u8], e: &'static Encoding) -> String {
    e.decode_without_bom_handling(b).0.into_owned()
}

fn trim(b: &[u8]) -> &[u8] {
    let s = b.iter().position(|c| !c.is_ascii_whitespace()).unwrap_or(b.len());
    let e = b.iter().rposition(|c| !c.is_ascii_whitespace()).map(|i| i + 1).unwrap_or(s);
    &b[s..e.max(s)]
}

fn number(b: &[u8]) -> Option<f64> {
    std::str::from_utf8(trim(b)).ok().filter(|t| !t.is_empty()).and_then(|t| t.parse::<f64>().ok())
}

/// Lignes d'un fichier (fins de ligne retirées), vides comprises.
fn lines(b: &[u8]) -> Vec<&[u8]> {
    let mut v: Vec<&[u8]> = b.split(|c| *c == b'\n').map(|l| l.strip_suffix(b"\r").unwrap_or(l)).collect();
    if v.last().is_some_and(|l| l.is_empty()) {
        v.pop();
    }
    v
}

fn cells(line: &[u8]) -> Vec<&[u8]> {
    line.split(|c| *c == b'\t').collect()
}

/// Cellule écrite : valeur `value` de la base, sauf un texte traduit du dossier de langue (`own`, cellule du fichier
/// du dossier, non numérique et différente de celle de ini/ `ini`), ou les octets d'origine quand la valeur est la
/// même (texte égal ou même nombre). Pour ini/ lui-même, `own` est `ini`.
pub fn overlay_cell(value: &str, ini: Option<&[u8]>, own: Option<&[u8]>, e: &'static Encoding) -> Vec<u8> {
    let (lang, base) = (own, own);
    if let (Some(i), Some(l)) = (ini, lang) {
        if l != i && number(l).is_none() && !trim(l).is_empty() {
            return l.to_vec();
        }
    }
    if let (Some(i), Some(b)) = (ini, base) {
        if b == i {
            let same_text = decode(trim(i), encoding_rs::EUC_KR) == value.trim();
            let same_number = matches!((number(i), value.trim().parse::<f64>().ok()), (Some(a), Some(v)) if a == v);
            if same_text || same_number || (trim(i).is_empty() && value.trim().is_empty()) {
                return b.to_vec();
            }
        }
    }
    encode(value, e)
}

/// En-tête nommé : noms des colonnes, une colonne répétée prenant son rang (« Resource TID#2 »), comme les clés
/// rangées par l'import (`group` : préfixes « point. », « percent. »… de SKILL_CONST).
fn header_names(header: &[u8], group: bool) -> Vec<String> {
    let names: Vec<String> = cells(header).iter().map(|c| decode(trim(c), encoding_rs::EUC_KR)).collect();
    let names = if group { super::import::group_columns(&names) } else { names };
    let mut seen: HashMap<String, usize> = HashMap::new();
    names
        .into_iter()
        .map(|n| {
            let k = seen.entry(n.clone()).or_insert(0);
            *k += 1;
            if *k == 1 { n } else { format!("{n}#{k}") }
        })
        .collect()
}

/// Ligne de valeurs à écrire : octets de sa clé dans le dossier (TID, nom de compétence…), la même clé dans `ini/`,
/// et ses cellules (en-tête → valeur).
pub struct Row {
    pub key: Vec<u8>,
    pub ini_key: Vec<u8>,
    pub cells: HashMap<String, String>,
}

fn join(cells: &[Vec<u8>]) -> Vec<u8> {
    cells.join(&b'\t')
}

/// Table tabulée à une ligne par clé (colonne `key_col` du client) : en-tête du fichier du client, lignes `rows`
/// dans l'ordre donné, cellules par `overlay_cell`.
pub fn write_keyed(ini: &[u8], lang: Option<&[u8]>, key_col: &str, rows: &[Row], e: &'static Encoding) -> Vec<u8> {
    let ini_lines = lines(ini);
    let Some(header) = ini_lines.first() else { return Vec::new() };
    let names = header_names(header, false);
    let k = names.iter().position(|n| n == key_col).unwrap_or(0);
    let index = |ls: &[&[u8]]| -> HashMap<Vec<u8>, Vec<Vec<u8>>> {
        ls.iter().skip(1).map(|l| cells(l).iter().map(|c| c.to_vec()).collect::<Vec<_>>()).filter_map(|c| Some((trim(c.get(k)?).to_vec(), c))).collect()
    };
    let ini_rows = index(&ini_lines);
    let lang_lines = lang.map(lines);
    let lang_rows = lang_lines.as_ref().map(|l| index(l));
    let out_header = lang_lines.as_ref().and_then(|l| l.first().copied()).unwrap_or(header);
    let mut out = out_header.to_vec();
    out.extend_from_slice(b"\r\n");
    for r in rows {
        let ir = ini_rows.get(trim(&r.ini_key));
        let lr = lang_rows.as_ref().map(|m| m.get(trim(&r.key))).unwrap_or(ir);
        let line: Vec<Vec<u8>> = names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                if i == k {
                    return r.key.clone();
                }
                let value = r.cells.get(n).map(String::as_str).unwrap_or("");
                overlay_cell(value, ir.and_then(|c| c.get(i)).map(Vec::as_slice), lr.and_then(|c| c.get(i)).map(Vec::as_slice), e)
            })
            .collect();
        out.extend(join(&line));
        out.extend_from_slice(b"\r\n");
    }
    out
}

/// SKILL_CONST.txt / Skill_Const2.txt : une ligne par niveau, le nom sur la première. `levels` : (octets du nom dans
/// le dossier, même nom dans `ini/`, niveaux (niveau, valeurs)).
pub fn write_levels(ini: &[u8], lang: Option<&[u8]>, skills: &[(Vec<u8>, Vec<u8>, Vec<(u32, &serde_json::Map<String, Json>)>)], e: &'static Encoding) -> Vec<u8> {
    let ini_lines = lines(ini);
    let Some(header) = ini_lines.first() else { return Vec::new() };
    let names = header_names(header, true);
    let level_col = names.iter().position(|n| n == "skill_level" || n == "skill_level2").unwrap_or(1);
    let index = |ls: &[&[u8]]| -> HashMap<(Vec<u8>, u32), Vec<Vec<u8>>> {
        let mut m = HashMap::new();
        let mut current = Vec::new();
        for l in ls.iter().skip(1) {
            let c: Vec<Vec<u8>> = cells(l).iter().map(|c| c.to_vec()).collect();
            if let Some(n) = c.first().filter(|n| !trim(n).is_empty()) {
                current = trim(n).to_vec();
            }
            if let Some(level) = c.get(level_col).and_then(|v| number(v)) {
                m.insert((current.clone(), level as u32), c);
            }
        }
        m
    };
    let ini_rows = index(&ini_lines);
    let lang_lines = lang.map(lines);
    let lang_rows = lang_lines.as_ref().map(|l| index(l));
    let mut out = lang_lines.as_ref().and_then(|l| l.first().copied()).unwrap_or(header).to_vec();
    out.extend_from_slice(b"\r\n");
    for (name, ini_name, levels) in skills {
        for (rank, (level, values)) in levels.iter().enumerate() {
            let ir = ini_rows.get(&(trim(ini_name).to_vec(), *level));
            let lr = lang_rows.as_ref().map(|m| m.get(&(trim(name).to_vec(), *level))).unwrap_or(ir);
            let line: Vec<Vec<u8>> = names
                .iter()
                .enumerate()
                .map(|(i, n)| match i {
                    0 => if rank == 0 { name.clone() } else { Vec::new() },
                    _ if i == level_col => level.to_string().into_bytes(),
                    _ => {
                        let v = values.get(n.as_str()).and_then(|v| v.as_str()).unwrap_or("");
                        overlay_cell(v, ir.and_then(|c| c.get(i)).map(Vec::as_slice), lr.and_then(|c| c.get(i)).map(Vec::as_slice), e)
                    }
                })
                .collect();
            out.extend(join(&line));
            out.extend_from_slice(b"\r\n");
        }
    }
    out
}

/// Table tabulée réécrite sous l'en-tête du client, colonne par colonne (nom et rang du nom) : `text` (table EP8
/// rangée telle quelle par l'import, une ligne par ligne). Les colonnes du client absentes restent vides.
pub fn write_by_header(client: &[u8], text: &str, e: &'static Encoding) -> Vec<u8> {
    let cl = lines(client);
    let Some(header) = cl.first() else { return Vec::new() };
    let names = header_names(header, false);
    let mut src = super::lines(text).filter(|l| !l.trim().is_empty());
    let src_names = header_names(&encode(src.next().unwrap_or(""), encoding_rs::EUC_KR), false);
    let at: Vec<Option<usize>> = names.iter().map(|n| src_names.iter().position(|s| s == n)).collect();
    let mut out = header.to_vec();
    out.extend_from_slice(b"\r\n");
    for l in src {
        let c: Vec<&str> = l.split('\t').collect();
        let line: Vec<Vec<u8>> = at.iter().map(|a| encode(a.and_then(|i| c.get(i)).map(|v| v.trim()).unwrap_or(""), e)).collect();
        out.extend(join(&line));
        out.extend_from_slice(b"\r\n");
    }
    out
}

/// ItemConvertTable.txt : tableaux lus par position. Lignes d'en-tête et lignes vides du client, lignes de valeurs de
/// la table EP8 (cellules nettoyées), à la largeur de la ligne du client ; nombres de lignes différents : table EP8
/// telle quelle.
pub fn write_convert(client: &[u8], text: &str, e: &'static Encoding) -> Vec<u8> {
    let cl = lines(client);
    let el: Vec<&str> = super::lines(text).collect::<Vec<_>>();
    let el: Vec<&str> = if el.last().is_some_and(|l| l.is_empty()) { el[..el.len() - 1].to_vec() } else { el };
    let mut out = Vec::new();
    if cl.len() != el.len() {
        for l in &el {
            out.extend(encode(l, e));
            out.extend_from_slice(b"\r\n");
        }
        return out;
    }
    for (c, l) in cl.iter().zip(&el) {
        let first = cells(c).first().map(|x| trim(x).to_vec()).unwrap_or_default();
        let data = !first.is_empty() && number(&first).is_some() || first.is_empty() && cells(c).iter().any(|x| !trim(x).is_empty());
        if !data {
            out.extend_from_slice(c);
        } else {
            let width = cells(c).len();
            let v: Vec<Vec<u8>> = (0..width).map(|i| encode(l.split('\t').nth(i).map(str::trim).unwrap_or(""), e)).collect();
            out.extend(join(&v));
        }
        out.extend_from_slice(b"\r\n");
    }
    out
}

/// Section .ini en octets : (identifiant, couples clé → valeur), dans l'ordre.
type IniSection = (Vec<u8>, Vec<(Vec<u8>, Vec<u8>)>);

/// .ini en octets : lignes avant la première section, puis sections.
fn parse_ini_bytes(b: &[u8]) -> (Vec<Vec<u8>>, Vec<IniSection>) {
    let (mut pre, mut secs): (Vec<Vec<u8>>, Vec<IniSection>) = (Vec::new(), Vec::new());
    for l in lines(b) {
        let t = trim(l);
        if t.len() > 2 && t[0] == b'[' && t[t.len() - 1] == b']' {
            secs.push((t[1..t.len() - 1].to_vec(), Vec::new()));
            continue;
        }
        match secs.last_mut() {
            Some(s) => {
                if let Some(eq) = t.iter().position(|c| *c == b'=') {
                    s.1.push((trim(&t[..eq]).to_vec(), trim(&t[eq + 1..]).to_vec()));
                }
            }
            None => pre.push(l.to_vec()),
        }
    }
    (pre, secs)
}

fn section_bytes(id: &[u8], pairs: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
    let mut out = b"[".to_vec();
    out.extend_from_slice(id);
    out.extend_from_slice(b"]\r\n");
    for (k, v) in pairs {
        out.extend_from_slice(k);
        out.push(b'=');
        out.extend_from_slice(v);
        out.extend_from_slice(b"\r\n");
    }
    out
}

/// .ini de valeurs (RegionTemplate, TeleportPoint) : sections de `text` (base), valeurs par `overlay_cell` contre la
/// section de même identifiant du client (`ini`) et du dossier de langue ; lignes d'avant la première section du client.
pub fn write_ini_values(ini: &[u8], lang: Option<&[u8]>, text: &str, e: &'static Encoding) -> Vec<u8> {
    let (pre, ini_secs) = parse_ini_bytes(ini);
    let lang_secs = lang.map(|l| parse_ini_bytes(l).1);
    let find = |secs: &[IniSection], id: &[u8]| secs.iter().find(|s| s.0 == id).map(|s| s.1.clone());
    let mut out = Vec::new();
    for l in lang.map(|l| parse_ini_bytes(l).0).unwrap_or(pre) {
        out.extend(l);
        out.extend_from_slice(b"\r\n");
    }
    for s in super::import::parse_ini(text) {
        let id = encode(&s.id, encoding_rs::EUC_KR);
        let is = find(&ini_secs, &id);
        let ls = lang_secs.as_ref().map(|secs| find(secs, &id)).unwrap_or(is.clone());
        let mut rank: HashMap<&str, usize> = HashMap::new();
        let pairs: Vec<(Vec<u8>, Vec<u8>)> = s
            .order
            .iter()
            .map(|(k, v)| {
                let r = rank.entry(k.as_str()).or_insert(0);
                *r += 1;
                let nth = |p: &Option<Vec<(Vec<u8>, Vec<u8>)>>| p.as_ref().and_then(|p| p.iter().filter(|(pk, _)| pk == k.as_bytes()).nth(*r - 1).map(|(_, v)| v.clone()));
                (k.as_bytes().to_vec(), overlay_cell(v, nth(&is).as_deref(), nth(&ls).as_deref(), e))
            })
            .collect();
        out.extend(section_bytes(&encode(&s.id, e), &pairs));
    }
    out
}

/// Fichier d'un dossier du client, déchiffré (casse du nom ignorée) : (nom réel, octets).
fn client_file(dir: &Path, name: &str) -> Option<(String, Vec<u8>)> {
    let rd = std::fs::read_dir(dir).ok()?;
    let real = rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).find(|f| f.eq_ignore_ascii_case(name))?;
    let (_, plain) = super::load_ini(&dir.join(&real)).ok()?;
    Some((real, plain))
}

/// Dossiers de tables du client : `ini` puis les dossiers de langue `ini_<lettres>`.
fn table_folders(game: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(game)
        .map(|rd| rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    v.retain(|n| n.eq_ignore_ascii_case("ini") || n.to_ascii_lowercase().strip_prefix("ini_").is_some_and(|l| !l.is_empty() && l.chars().all(|c| c.is_ascii_alphabetic())));
    v.sort_by_key(|n| (!n.eq_ignore_ascii_case("ini"), n.to_ascii_lowercase()));
    v
}

/// Contenu de la base nécessaire à l'export.
#[derive(Default)]
pub struct Data {
    items: Vec<(u32, String, serde_json::Map<String, Json>)>,
    models: Vec<(u32, serde_json::Map<String, Json>)>,
    skills: Vec<(u32, String, serde_json::Map<String, Json>)>,
    specs: Vec<(u32, serde_json::Map<String, Json>)>,
    levels: BTreeMap<u32, Vec<(u32, serde_json::Map<String, Json>)>>,
    texts: HashMap<String, String>,
    level_exp: Vec<(u32, u64)>,
    growth: BTreeMap<u32, Vec<Vec<f64>>>,
    npcs: Vec<(u32, Vec<(String, String)>)>,
    fees: Vec<(String, String, i64)>,
    quests: BTreeMap<u32, serde_json::Map<String, Json>>,
    groups: BTreeMap<u32, (String, Vec<u32>, Vec<u32>)>,
    titles: std::collections::HashSet<u32>,
    bosses: std::collections::HashSet<u32>,
}

fn object(s: &str) -> serde_json::Map<String, Json> {
    match serde_json::from_str::<Json>(s) {
        Ok(Json::Object(o)) => o,
        _ => Default::default(),
    }
}

impl Data {
    pub async fn load(db: &Databases) -> anyhow::Result<Self> {
        let w = &db.world;
        let mut d = Data::default();
        for r in w.rows("SELECT tid, name, raw FROM item_data ORDER BY tid", ()).await? {
            d.items.push((col(&r, "tid"), col(&r, "name"), object(&col::<String>(&r, "raw"))));
        }
        for r in w.rows("SELECT tid, data FROM character_data_table ORDER BY tid", ()).await? {
            d.models.push((col(&r, "tid"), object(&col::<String>(&r, "data"))));
        }
        for r in w.rows("SELECT tid, name, raw FROM skill_template ORDER BY tid", ()).await? {
            d.skills.push((col(&r, "tid"), col(&r, "name"), object(&col::<String>(&r, "raw"))));
        }
        for r in w.rows("SELECT tid, spec FROM skill_spec ORDER BY tid", ()).await? {
            d.specs.push((col(&r, "tid"), object(&col::<String>(&r, "spec"))));
        }
        for r in w.rows("SELECT tid, level, consts FROM skill_level ORDER BY tid, level", ()).await? {
            d.levels.entry(col(&r, "tid")).or_default().push((col(&r, "level"), object(&col::<String>(&r, "consts"))));
        }
        for r in w.rows("SELECT name, data FROM client_table WHERE row_idx = 0", ()).await? {
            d.texts.insert(col(&r, "name"), serde_json::from_str::<String>(&col::<String>(&r, "data")).unwrap_or_default());
        }
        for r in w.rows("SELECT level, exp FROM level_exp ORDER BY level", ()).await? {
            d.level_exp.push((col(&r, "level"), col(&r, "exp")));
        }
        const GROWTH: [&str; 20] = ["level", "str", "dex", "con", "wis", "intel", "cha", "hp", "mp", "sp", "ac", "ar", "dr", "res_fire", "res_water", "res_earth", "res_air", "res_magic", "min_damage", "max_damage"];
        for r in w.rows("SELECT * FROM growup_factor ORDER BY template_id, level", ()).await? {
            d.growth.entry(col(&r, "template_id")).or_default().push(GROWTH.iter().map(|c| col::<f64>(&r, c)).collect());
        }
        for r in w.rows("SELECT id, raw FROM npc_spawn ORDER BY id", ()).await? {
            d.npcs.push((col(&r, "id"), serde_json::from_str(&col::<String>(&r, "raw")).unwrap_or_default()));
        }
        for r in w.rows("SELECT source, destination, fee FROM teleport_fee", ()).await? {
            d.fees.push((col(&r, "source"), col(&r, "destination"), col(&r, "fee")));
        }
        for r in w.rows("SELECT tid, data FROM quest_template", ()).await? {
            d.quests.insert(col(&r, "tid"), object(&col::<String>(&r, "data")));
        }
        for r in w.rows("SELECT id, name, give, confirm FROM quest_group", ()).await? {
            let list = |k: &str| serde_json::from_str::<Vec<u32>>(&col::<String>(&r, k)).unwrap_or_default();
            d.groups.insert(col(&r, "id"), (col(&r, "name"), list("give"), list("confirm")));
        }
        d.titles = w.rows("SELECT tid FROM title_template", ()).await?.iter().map(|r| col(r, "tid")).collect();
        d.bosses = w.rows("SELECT tid FROM boss_window", ()).await?.iter().map(|r| col(r, "tid")).collect();
        Ok(d)
    }
}

fn strings(o: &serde_json::Map<String, Json>) -> HashMap<String, String> {
    o.iter().map(|(k, v)| (k.clone(), v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()))).collect()
}

fn fmt_num(x: f64) -> String {
    if x.fract() == 0.0 { format!("{}", x as i64) } else { format!("{x}") }
}

/// Noms des compétences d'un dossier (clé 1 de son SKILLTEMPLATE.ini), en octets : TID → nom.
fn skill_names(template: &[u8]) -> HashMap<u32, Vec<u8>> {
    parse_ini_bytes(template)
        .1
        .into_iter()
        .filter_map(|(id, pairs)| Some((std::str::from_utf8(&id).ok()?.parse().ok()?, pairs.into_iter().find(|(k, _)| k == b"1")?.1)))
        .collect()
}

/// Tables écrites pour un dossier : (nom du fichier du client, octets). `ini` : fichiers de Game/ini ; `lang` : ceux
/// du dossier de langue (None pour ini/ lui-même).
pub fn build_folder(d: &Data, folder: &str, ini: &HashMap<String, (String, Vec<u8>)>, lang: Option<&HashMap<String, (String, Vec<u8>)>>) -> Vec<(String, Vec<u8>)> {
    let e = folder_encoding(folder);
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    // Fichier à écrire : présent dans ini/ et, pour un dossier de langue, dans ce dossier.
    let pick = |name: &str| -> Option<(String, &[u8], Option<&[u8]>)> {
        let (real, i) = ini.get(&name.to_ascii_lowercase())?;
        match lang {
            None => Some((real.clone(), i.as_slice(), None)),
            Some(l) => l.get(&name.to_ascii_lowercase()).map(|(real, b)| (real.clone(), i.as_slice(), Some(b.as_slice()))),
        }
    };
    let text = |n: &str| d.texts.get(n).map(String::as_str).unwrap_or("");
    let tid_row = |tid: u32, cells: HashMap<String, String>| Row { key: tid.to_string().into_bytes(), ini_key: tid.to_string().into_bytes(), cells };

    if let Some((name, i, l)) = pick("itemdatatable.txt") {
        let rows: Vec<Row> = d
            .items
            .iter()
            .map(|(tid, item_name, raw)| {
                let mut c = strings(raw);
                c.insert("ItemName".into(), item_name.clone());
                tid_row(*tid, c)
            })
            .collect();
        out.push((name, write_keyed(i, l, "TID", &rows, e)));
    }
    if let Some((name, i, l)) = pick("CharacterDataTable.txt") {
        let rows: Vec<Row> = d.models.iter().map(|(tid, data)| tid_row(*tid, strings(data))).collect();
        out.push((name, write_keyed(i, l, "TID", &rows, e)));
    }
    for (file, key, stored) in [("itemoptiontable.txt", "tid", "itemoptiontable.txt"), ("itemruneattributetable.txt", "TID", "itemruneattributetable.txt")] {
        if let Some((name, i, l)) = pick(file) {
            let (h, rows) = super::import::parse_tsv(text(stored));
            let k = h.iter().position(|x| x == key).unwrap_or(0);
            let rows: Vec<Row> = rows
                .iter()
                .filter_map(|r| {
                    let tid = r.get(k)?.trim().to_string();
                    super::import::is_uint(&tid).then(|| Row {
                        key: tid.clone().into_bytes(),
                        ini_key: tid.into_bytes(),
                        cells: h.iter().cloned().zip(r.iter().map(|c| c.trim().to_string())).collect(),
                    })
                })
                .collect();
            out.push((name, write_keyed(i, l, key, &rows, e)));
        }
    }
    // Compétences : noms du dossier (son SKILLTEMPLATE.ini), ceux de ini/ pour retrouver la ligne du client.
    let ini_names: HashMap<u32, Vec<u8>> = ini.get("skilltemplate.ini").map(|(_, b)| skill_names(b)).unwrap_or_default();
    let lang_names: HashMap<u32, Vec<u8>> = lang.and_then(|l| l.get("skilltemplate.ini")).map(|(_, b)| skill_names(b)).unwrap_or_default();
    let db_names: HashMap<u32, &str> = d.skills.iter().map(|(t, n, _)| (*t, n.as_str())).collect();
    let name_in = |tid: u32| -> (Vec<u8>, Vec<u8>) {
        let ini_name = ini_names.get(&tid).cloned().unwrap_or_else(|| encode(db_names.get(&tid).copied().unwrap_or(""), encoding_rs::EUC_KR));
        let name = if lang.is_some() { lang_names.get(&tid).cloned().unwrap_or_else(|| encode(db_names.get(&tid).copied().unwrap_or(""), e)) } else { ini_name.clone() };
        (name, ini_name)
    };
    if let Some((name, i, l)) = pick("SKILLTEMPLATE.ini") {
        let base = l.unwrap_or(i);
        let (pre, secs) = parse_ini_bytes(base);
        let mut bytes = Vec::new();
        for p in pre {
            bytes.extend(p);
            bytes.extend_from_slice(b"\r\n");
        }
        let by_id: HashMap<Vec<u8>, &Vec<(Vec<u8>, Vec<u8>)>> = secs.iter().map(|(id, p)| (id.clone(), p)).collect();
        for (tid, _, raw) in &d.skills {
            let id = tid.to_string().into_bytes();
            match by_id.get(&id) {
                Some(p) => bytes.extend(section_bytes(&id, p)),
                None => {
                    // Compétence que le client n'a pas : section rangée dans la base.
                    let pairs: Vec<(Vec<u8>, Vec<u8>)> = raw
                        .iter()
                        .flat_map(|(k, v)| match v {
                            Json::Array(a) => a.iter().map(|x| (k.clone(), x.as_str().unwrap_or("").to_string())).collect::<Vec<_>>(),
                            _ => vec![(k.clone(), v.as_str().unwrap_or("").to_string())],
                        })
                        .map(|(k, v)| (k.into_bytes(), encode(&v, e)))
                        .collect();
                    bytes.extend(section_bytes(&id, &pairs));
                }
            }
        }
        out.push((name, bytes));
    }
    if let Some((name, i, l)) = pick("SKILL_SPEC.txt") {
        let first = super::import::parse_tsv(&decode(i, encoding_rs::EUC_KR)).0.first().cloned().unwrap_or_default();
        let rows: Vec<Row> = d
            .specs
            .iter()
            .map(|(tid, spec)| {
                let (key, ini_key) = name_in(*tid);
                let mut cells = strings(spec);
                cells.insert(first.clone(), String::new());
                Row { key, ini_key, cells }
            })
            .collect();
        out.push((name, write_keyed(i, l, &first, &rows, e)));
    }
    for (file, second) in [("SKILL_CONST.txt", false), ("Skill_Const2.txt", true)] {
        if let Some((name, i, l)) = pick(file) {
            let header = header_names(lines(i).first().copied().unwrap_or_default(), true);
            let own: std::collections::HashSet<&str> = header.iter().skip(2).map(String::as_str).collect();
            let skills: Vec<(Vec<u8>, Vec<u8>, Vec<(u32, &serde_json::Map<String, Json>)>)> = d
                .levels
                .iter()
                .filter_map(|(tid, levels)| {
                    let levels: Vec<(u32, &serde_json::Map<String, Json>)> =
                        levels.iter().filter(|(_, v)| !second || v.keys().any(|k| own.contains(k.as_str()))).map(|(lv, v)| (*lv, v)).collect();
                    let (n, ini_n) = name_in(*tid);
                    (!levels.is_empty()).then_some((n, ini_n, levels))
                })
                .collect();
            out.push((name, write_levels(i, l, &skills, e)));
        }
    }
    if let Some((name, i, l)) = pick("RegionTemplate.ini") {
        out.push((name, write_ini_values(i, l, text("RegionTemplate.ini"), e)));
    }
    if let Some((name, i, l)) = pick("questtemplate.ini") {
        // Quêtes de la base sous l'en-tête du client ; dans un dossier de langue, textes de Webzen de cette langue.
        let code = super::webzen::LANGUAGES.iter().find(|x| x.2.eq_ignore_ascii_case(folder)).map(|x| x.1);
        let lang_texts: HashMap<u32, HashMap<String, String>> =
            code.and_then(|c| d.texts.get(&format!("questtemplate.ini@{c}"))).and_then(|t| serde_json::from_str(t).ok()).unwrap_or_default();
        let header = lines(i).first().map(|h| header_names(h, false)).unwrap_or_default();
        let key = header.first().cloned().unwrap_or_else(|| "Tid".into());
        let rows: Vec<Row> = d
            .quests
            .iter()
            .map(|(tid, data)| {
                let mut c = strings(data);
                if let Some(t) = lang_texts.get(tid) {
                    c.extend(t.iter().map(|(k, v)| (k.clone(), v.clone())));
                }
                tid_row(*tid, c)
            })
            .collect();
        out.push((name, write_keyed(i, l, &key, &rows, e)));
    }
    // Titres et leurs textes rangés par l'import (Webzen) : tels quels ; dans un dossier de langue, textes du client
    // pour les titres qu'il traduit.
    if let (Some((name, _, l)), Some(stored)) = (pick("titlestringtable.txt"), d.texts.get("titlestringtable.txt")) {
        let lang_rows: HashMap<Vec<u8>, &[u8]> = l.map(|l| lines(l).into_iter().skip(1).filter_map(|x| Some((trim(cells(x).first()?).to_vec(), x))).collect()).unwrap_or_default();
        let mut bytes = Vec::new();
        for (n, line) in super::lines(stored).filter(|x| !x.is_empty()).enumerate() {
            let tid = line.split('\t').next().unwrap_or("").trim().as_bytes().to_vec();
            match lang_rows.get(&tid).filter(|_| n > 0) {
                Some(own) => bytes.extend_from_slice(own),
                None => bytes.extend(encode(line, e)),
            }
            bytes.extend_from_slice(b"\r\n");
        }
        out.push((name, bytes));
    }
    if let (Some((name, _, _)), Some(stored)) = (pick("titledatatable.txt"), d.texts.get("titledatatable.txt")) {
        out.push((name, super::lines(stored).filter(|x| !x.is_empty()).flat_map(|x| encode(x, e).into_iter().chain(*b"\r\n")).collect()));
    } else if let Some((name, i, l)) = pick("titledatatable.txt") {
        let base = l.unwrap_or(i);
        let mut bytes = Vec::new();
        for (n, line) in lines(base).into_iter().enumerate() {
            let tid = cells(line).first().and_then(|c| number(c)).map(|t| t as u32);
            if n == 0 || tid.is_some_and(|t| d.titles.contains(&t)) {
                bytes.extend_from_slice(line);
                bytes.extend_from_slice(b"\r\n");
            }
        }
        out.push((name, bytes));
    }
    if lang.is_some() {
        return out;
    }
    // ------------------------------------------------------------ tables lues dans ini/ seulement
    if let Some((name, i, _)) = pick("RefineryItem2.txt") {
        out.push((name, write_by_header(i, text("refineryrecipetable.txt"), e)));
    }
    if let Some((name, i, _)) = pick("ItemConvertTable.txt") {
        out.push((name, write_convert(i, text("ItemConvertTable.txt"), e)));
    }
    if let Some((name, i, _)) = pick("TeleportPoint.ini") {
        out.push((name, write_ini_values(i, None, text("TeleportPoint.ini"), e)));
    }
    if let Some((name, i, _)) = pick("TeleportFee.txt") {
        let fees: String = std::iter::once("start_portal\tfinish_portal\tcost".to_string()).chain(d.fees.iter().map(|(a, b, f)| format!("{a}\t{b}\t{f}"))).collect::<Vec<_>>().join("\n");
        out.push((name, write_by_header(i, &fees, e)));
    }
    if let Some((name, i, _)) = pick("LevelUpExp.txt") {
        let mut b = lines(i).first().map(|h| h.to_vec()).unwrap_or_else(|| b"level\texp".to_vec());
        b.extend_from_slice(b"\r\n");
        for (lv, x) in &d.level_exp {
            b.extend(format!("{lv}\t{x}\r\n").into_bytes());
        }
        out.push((name, b));
    }
    if let Some((name, i, _)) = pick("GrowUpFactor.txt") {
        // Ligne du modèle (TID puis tabulations) et en-tête « LV … » du client.
        let ls = lines(i);
        let width = ls.first().map(|l| cells(l).len()).unwrap_or(20);
        let head = ls.iter().find(|l| cells(l).first().is_some_and(|c| trim(c) == b"LV")).map(|l| l.to_vec()).unwrap_or_default();
        // Modèles dans l'ordre du fichier du client, puis les autres.
        let mut order: Vec<u32> = ls
            .iter()
            .filter_map(|l| {
                let c = cells(l);
                (c.len() > 1 && c[1..].iter().all(|x| trim(x).is_empty())).then(|| number(c[0]).map(|t| t as u32)).flatten()
            })
            .filter(|t| d.growth.contains_key(t))
            .collect();
        order.extend(d.growth.keys().filter(|t| !order.contains(t)).copied().collect::<Vec<_>>());
        let mut b = Vec::new();
        for (tid, rows) in order.iter().filter_map(|t| Some((t, d.growth.get(t)?))) {
            b.extend(format!("{tid}{}\r\n", "\t".repeat(width.saturating_sub(1))).into_bytes());
            b.extend_from_slice(&head);
            b.extend_from_slice(b"\r\n");
            for r in rows {
                b.extend(r.iter().map(|x| fmt_num(*x)).collect::<Vec<_>>().join("\t").into_bytes());
                b.extend_from_slice(b"\r\n");
            }
        }
        out.push((name, b));
    }
    if let Some((name, _, _)) = pick("npc.ini") {
        let mut b = Vec::new();
        for (id, order) in &d.npcs {
            let pairs: Vec<(Vec<u8>, Vec<u8>)> = order.iter().map(|(k, v)| (encode(k, e), encode(v, e))).collect();
            b.extend(section_bytes(id.to_string().as_bytes(), &pairs));
        }
        out.push((name, b));
    }
    if let Some((name, _, _)) = pick("questgroup.ini") {
        // Groupes de la base (numéro, nom, quêtes proposées et reprises « a:b: »), dans l'ordre des numéros.
        let mut b = Vec::new();
        let list = |l: &[u32]| l.iter().map(|t| format!("{t}:")).collect::<String>();
        for (id, (group, give, confirm)) in &d.groups {
            b.extend(encode(&format!("{id}\t{group}\t{}\t{}\t", list(give), list(confirm)), e));
            b.extend_from_slice(b"\r\n");
        }
        out.push((name, b));
    }
    // Arbres de maîtrise lus par l'import (Webzen ou client), tels quels.
    for file in ["SkillMastery_High.txt", "SkillMastery_Heroic.txt"] {
        if let (Some((name, _, _)), Some(stored)) = (pick(file), d.texts.get(file)) {
            out.push((name, super::lines(stored).flat_map(|x| encode(x, e).into_iter().chain(*b"\r\n")).collect()));
        }
    }
    if let Some((name, i, _)) = pick("UIBoss.xml") {
        let xml = super::latin1(i);
        let (kept, _) = super::ep8::filter_boss_xml(&xml, &d.bosses);
        out.push((name, kept.chars().map(|c| c as u32 as u8).collect()));
    }
    out
}

/// Empreinte des fichiers écrits : « sha256: » puis SHA-256 des chemins relatifs et des contenus, triés par chemin.
pub fn fingerprint(files: &[(String, Vec<u8>)]) -> String {
    let mut sorted: Vec<&(String, Vec<u8>)> = files.iter().collect();
    sorted.sort_by(|a, b| a.0.to_ascii_lowercase().cmp(&b.0.to_ascii_lowercase()));
    let mut h = Sha256::new();
    for (path, bytes) in sorted {
        h.update(path.to_ascii_lowercase().as_bytes());
        h.update(b"\n");
        h.update((bytes.len() as u64).to_le_bytes());
        h.update(bytes);
    }
    format!("sha256:{}", h.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>())
}

/// Empreinte annoncée : celle de `tables.toml` du dossier exporté (ASCII imprimable sans espace, 128 octets au plus),
/// `None` s'il manque ou si elle n'est pas valable.
pub fn read_fingerprint(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join(MANIFEST_FILE)).ok()?;
    let line = text.lines().find(|l| l.trim_start().starts_with("fingerprint"))?;
    let value = line.split_once('=')?.1.trim().trim_matches('"').to_string();
    (!value.is_empty() && value.len() <= 128 && value.bytes().all(|b| b.is_ascii_graphic())).then_some(value)
}

pub async fn run(args: &[String]) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let mut target: Option<PathBuf> = None;
    let mut game: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--game" {
            game = Some(PathBuf::from(it.next().ok_or_else(|| anyhow::anyhow!("--game : dossier attendu"))?));
        } else {
            target = Some(PathBuf::from(a));
        }
    }
    let target = target.ok_or_else(|| anyhow::anyhow!("usage : archcore export-client-tables <dossier> [--game dossier]"))?;
    let game = game.unwrap_or_else(|| cfg.root.join(&cfg.client.data_dir));
    anyhow::ensure!(game.join("ini").is_dir(), "{} : pas de dossier ini/ (dossier Game du client, --game)", game.display());
    let db = Databases::new(&cfg.database);
    let data = Data::load(&db).await?;
    db.close().await;
    anyhow::ensure!(!data.items.is_empty(), "base monde vide : lancer `archcore import` d'abord");
    let files = export_files(&data, &game);
    for (rel, bytes) in &files {
        let path = target.join(rel);
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::write(&path, bytes).map_err(|e| anyhow::anyhow!("{} : {e}", path.display()))?;
    }
    let sum = fingerprint(&files);
    let manifest = format!(
        "# Tables de valeurs du serveur, écrites par `archcore export-client-tables` : ne pas modifier à la main.\r\nformat = 1\r\nfingerprint = \"{sum}\"\r\nsoftware = \"ArchCore {}\"\r\ngenerated = \"{}\"\r\n",
        env!("CARGO_PKG_VERSION"),
        chrono::Utc::now().format("%Y-%m-%dT%H:%MZ")
    );
    std::fs::write(target.join(MANIFEST_FILE), manifest)?;
    let mut per: BTreeMap<String, usize> = BTreeMap::new();
    for (rel, _) in &files {
        *per.entry(rel.split('/').next().unwrap_or("").to_string()).or_default() += 1;
    }
    println!("✓ {} fichier(s) écrit(s) dans {} ({})", files.len(), target.display(), per.iter().map(|(d, n)| format!("{d} {n}")).collect::<Vec<_>>().join(", "));
    println!("  empreinte {sum} ({MANIFEST_FILE}) ; à annoncer par le serveur monde : Client.TablesDir = \"{}\"", target.display());
    Ok(())
}

/// Fichiers exportés (chemin relatif « dossier/fichier », octets) pour ini/ et chaque dossier de langue du client.
pub fn export_files(data: &Data, game: &Path) -> Vec<(String, Vec<u8>)> {
    let read_folder = |folder: &str| -> HashMap<String, (String, Vec<u8>)> {
        let dir = game.join(folder);
        let names: Vec<String> = std::fs::read_dir(&dir)
            .map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| TABLES.iter().any(|t| t.eq_ignore_ascii_case(n))).collect())
            .unwrap_or_default();
        names.into_iter().filter_map(|n| client_file(&dir, &n).map(|f| (n.to_ascii_lowercase(), f))).collect()
    };
    let ini = read_folder("ini");
    // Seules les tables exportées sont lues (et déchiffrées).
    let mut files = Vec::new();
    for folder in table_folders(game) {
        let lang = (!folder.eq_ignore_ascii_case("ini")).then(|| read_folder(&folder));
        for (name, bytes) in build_folder(data, &folder.to_ascii_lowercase(), &ini, lang.as_ref()) {
            files.push((format!("{}/{name}", folder), bytes));
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(tid: &str, cells: &[(&str, &str)]) -> Row {
        Row { key: tid.into(), ini_key: tid.into(), cells: cells.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect() }
    }

    #[test]
    fn table_au_format_du_client() {
        // ini/ : en-tête du client (colonne d'Archonia « TransmogPrice » gardée, vide), valeurs de la base, CRLF, CP949.
        let mut ini = b"TID\tItemName\tNPC Price\tTransmogPrice\r\n10\t".to_vec();
        ini.extend(encode("검", encoding_rs::EUC_KR));
        ini.extend(b"\t900\t7\r\n11\tBow\t20.00\t1\r\n14611\tArchonia\t1\t1\r\n");
        let rows = [row("10", &[("ItemName", "검"), ("NPC Price", "500")]), row("11", &[("ItemName", "Bow"), ("NPC Price", "20")]), row("12", &[("ItemName", "Neuf"), ("NPC Price", "3")])];
        let out = write_keyed(&ini, None, "TID", &rows, folder_encoding("ini"));
        let mut want = b"TID\tItemName\tNPC Price\tTransmogPrice\r\n10\t".to_vec();
        want.extend(encode("검", encoding_rs::EUC_KR));
        want.extend(b"\t500\t\r\n11\tBow\t20.00\t\r\n12\tNeuf\t3\t\r\n");
        assert_eq!(out, want, "valeur changée : celle de la base ; même nombre : octets du client ; 14611 absent");
        // ini_fr : nom traduit (Windows-1252) gardé, valeurs de la base.
        let fr = b"TID\tItemName\tNPC Price\tTransmogPrice\r\n10\t\xc9p\xe9e\t900\t7\r\n11\tBow\t20.00\t1\r\n";
        let out = write_keyed(&ini, Some(fr), "TID", &rows, folder_encoding("ini_fr"));
        assert_eq!(out, b"TID\tItemName\tNPC Price\tTransmogPrice\r\n10\t\xc9p\xe9e\t500\t\r\n11\tBow\t20.00\t\r\n12\tNeuf\t3\t\r\n".to_vec());
        assert_eq!(encode("Épée 검", encoding_rs::WINDOWS_1252), b"\xc9p\xe9e ?".to_vec(), "caractère absent de la page de code : ?");
        assert_eq!((folder_encoding("ini_ko"), folder_encoding("ini_ja"), folder_encoding("ini_zh"), folder_encoding("ini_de")), (encoding_rs::EUC_KR, encoding_rs::SHIFT_JIS, encoding_rs::GBK, encoding_rs::WINDOWS_1252));
    }

    #[test]
    fn niveaux_de_competence_au_format_du_client() {
        let ini = b"name\tskill_level\tFACTOR_POINT\tstr\tbonus_unique_drop_rate\r\nLethal Attack\t1\t\t5\t9\r\n\t2\t\t6\t9\r\n";
        let a: serde_json::Map<String, Json> = serde_json::from_str(r#"{"point.str":"7"}"#).unwrap();
        let b: serde_json::Map<String, Json> = serde_json::from_str(r#"{"point.str":"6"}"#).unwrap();
        let out = write_levels(ini, None, &[(b"Lethal Attack".to_vec(), b"Lethal Attack".to_vec(), vec![(1, &a), (2, &b)])], encoding_rs::EUC_KR);
        assert_eq!(out, b"name\tskill_level\tFACTOR_POINT\tstr\tbonus_unique_drop_rate\r\nLethal Attack\t1\t\t7\t\r\n\t2\t\t6\t\r\n".to_vec(), "nom sur la première ligne, colonne d'Archonia vide");
    }

    #[test]
    fn recettes_et_conversion_par_en_tete() {
        let client = b"Recipe ID\tKeep Item Options\tMakeItem TID\tResource TID\tResource Count\tResource TID\tResource Count\r\n";
        let ep8 = "Recipe ID\tMakeItem TID\tResource TID\tResource Count\tResource TID\tResource Count\r\n4\t10305 \t9001\t1\t10304\t2\r\n";
        assert_eq!(write_by_header(client, ep8, encoding_rs::EUC_KR), b"Recipe ID\tKeep Item Options\tMakeItem TID\tResource TID\tResource Count\tResource TID\tResource Count\r\n4\t\t10305\t9001\t1\t10304\t2\r\n".to_vec());
        let client = b"Reinforcement Level\tWeapon Increase\r\n1\t2\r\n\t\r\nSuccess\t1\r\n1\t100\r\n";
        let ep8 = "Rank\tWeapon_Damage\t\r\n1 \t3 \t\r\n\t\t\r\nSuccess\t1 \r\n1 \t95 \r\n";
        assert_eq!(write_convert(client, ep8, encoding_rs::EUC_KR), b"Reinforcement Level\tWeapon Increase\r\n1\t3\r\n\t\r\nSuccess\t1\r\n1\t95\r\n".to_vec());
    }

    #[test]
    fn ini_de_valeurs_et_empreinte() {
        let ini = b"[9]\r\nName=Anchorville\r\nType=1\r\nServerCollision=1\r\n[400]\r\nName=Archonia\r\n";
        let fr = b"[9]\r\nName=Anchorville la belle\r\nType=1\r\nServerCollision=1\r\n";
        let ep8 = "[9]\r\nName=Anchorville\r\nType=786929\r\n";
        assert_eq!(write_ini_values(ini, None, ep8, encoding_rs::EUC_KR), b"[9]\r\nName=Anchorville\r\nType=786929\r\n".to_vec());
        assert_eq!(write_ini_values(ini, Some(fr), ep8, encoding_rs::WINDOWS_1252), b"[9]\r\nName=Anchorville la belle\r\nType=786929\r\n".to_vec());
        let a = fingerprint(&[("ini/a.txt".into(), b"1".to_vec()), ("ini_fr/a.txt".into(), b"2".to_vec())]);
        let b = fingerprint(&[("ini_fr/a.txt".into(), b"2".to_vec()), ("ini/a.txt".into(), b"1".to_vec())]);
        assert_eq!(a, b, "ordre des fichiers sans effet");
        assert!(a.starts_with("sha256:") && a.len() == 71 && a.bytes().all(|c| c.is_ascii_graphic()));
        assert_ne!(a, fingerprint(&[("ini/a.txt".into(), b"3".to_vec())]));
        let dir = std::env::temp_dir().join(format!("archcore-tables-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(MANIFEST_FILE), format!("format = 1\nfingerprint = \"{a}\"\n")).unwrap();
        assert_eq!(read_fingerprint(&dir), Some(a));
        std::fs::write(dir.join(MANIFEST_FILE), "fingerprint = \"avec espace\"\n").unwrap();
        assert_eq!(read_fingerprint(&dir), None, "espace refusée");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
