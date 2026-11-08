//! Valeurs de jeu de l'épisode 8 : les tables du serveur d'origine (« tables EP8 ») décident de la présence de chaque
//! ligne de valeurs (compétence, niveau de compétence, objet, option, rune, modèle, recette, région, boutique, point de
//! téléportation) et de ses valeurs ; les tables du client d'Archonia ne donnent plus que les noms et les visuels.
//!
//! ```text
//!   Présence : une ligne de valeurs n'existe que si elle existe dans la table EP8 ; une ligne propre au client
//!        (ajout d'Archonia) est écartée, une ligne propre à la table EP8 (boutique, PNJ, portail que le client ne
//!        connaît pas) aussi : elle n'a ni nom ni visuel.
//!   Valeurs : colonnes de la table EP8 ; les colonnes propres au client (Archonia) ne sont pas importées, celles
//!        propres à la table EP8 (« NPC Arena Coin ») non plus.
//!   Noms : ceux du client, joints par TID (« ItemName », « Name », « Description » des options).
//!   Compétences : jointure nom → TID par le modèle EP8 (930 s'y appelle « <Battle>Skill Upgrade », le nom du
//!        524 est mal encodé côté client) ; `skill_spec.name` garde le nom EP8 (runes : « Skill_Name »),
//!        `skill_template` le nom et les visuels du client.
//!   Tables purement clientes (quêtes, maîtrises, titres, production, fenêtre des boss, sièges) : gardées, sans
//!        les lignes qui citent un TID hors EP8.
//!   PNJ (npc.ini) : fusion — modèle, position et événements EP8, mais quêtes (27), réparation (17) et avis de
//!        recherche (32) du client, que la table EP8 a retirés ; un PNJ dont le numéro désigne un autre PNJ dans la
//!        table EP8 (ni le même nom ni le même modèle) garde ses valeurs du client ; boutique (fonction 9) EP8 si
//!        elle est importée, sinon celle du client.
//!   Expérience : niveaux de la table EP8 (1 à 100), puis ceux du client au-delà (101 à 121, seul barème connu).
//!   Croissance : modèles de la table EP8 ; un modèle qu'elle n'a pas (1276, ArchLord) garde celle du client.
//! ```

use super::import::{is_uint, js_number, parse_ini, parse_tsv, Section};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Colonnes propres aux tables EP8, jamais importées (lignes propres au serveur qui les a publiées).
pub const EP8_ONLY_ITEM_COLUMNS: [&str; 1] = ["NPC Arena Coin"];

/// Bilan de l'import : par table, lignes gardées, écartées et remarque.
#[derive(Debug, Default)]
pub struct Bilan {
    pub rows: Vec<(String, usize, usize, String)>,
    /// Lignes écartées une à une, avec leur raison (journal de `--webzen`, `data/import-webzen.log`).
    pub details: Vec<String>,
}

impl Bilan {
    pub fn add(&mut self, table: &str, kept: usize, dropped: usize, note: impl Into<String>) {
        self.rows.push((table.to_string(), kept, dropped, note.into()));
    }

    pub fn print(&self) {
        println!("\nRésumé de l'import (valeurs de l'épisode 8) :");
        println!("  {:<26} {:>8} {:>8}  remarque", "table", "gardées", "écartées");
        for (t, k, d, n) in &self.rows {
            println!("  {t:<26} {k:>8} {d:>8}  {n}");
        }
    }
}

/// Ensembles EP8 : TID des objets, des compétences, des modèles (monstres, PNJ, classes) et des options.
#[derive(Clone, Debug, Default)]
pub struct Ep8Sets {
    pub items: HashSet<u32>,
    pub skills: HashSet<u32>,
    pub models: HashSet<u32>,
    pub options: HashSet<u32>,
}

/// Clés numériques de la colonne `key` d'une table tabulée.
pub fn tsv_keys(text: &str, key: &str) -> HashSet<u32> {
    let (h, rows) = parse_tsv(text);
    let Some(c) = h.iter().position(|x| x == key) else { return HashSet::new() };
    rows.iter().filter_map(|r| r.get(c).map(|v| v.trim()).filter(|v| is_uint(v)).and_then(|v| v.parse().ok())).collect()
}

/// Sections numériques d'un .ini.
pub fn ini_keys(text: &str) -> HashSet<u32> {
    parse_ini(text).iter().filter(|s| is_uint(&s.id)).filter_map(|s| s.id.parse().ok()).collect()
}

impl Ep8Sets {
    /// `itemdatatable.txt`, `characterdatatable.txt`, `skilltemplate.ini`, `itemoptiontable.txt` des tables EP8.
    pub fn from_texts(items: &str, models: &str, skills: &str, options: &str) -> Self {
        Ep8Sets { items: tsv_keys(items, "TID"), models: tsv_keys(models, "TID"), skills: ini_keys(skills), options: tsv_keys(options, "tid") }
    }
}

/// Texte tabulé en CRLF.
pub fn write_tsv(header: &[String], rows: &[Vec<String>]) -> String {
    let mut out = header.join("\t");
    out.push_str("\r\n");
    for r in rows {
        out.push_str(&r.join("\t"));
        out.push_str("\r\n");
    }
    out
}

/// Résultat d'une fusion de table : texte prêt pour l'import, lignes EP8 gardées, lignes du client écartées
/// (absentes de la table EP8), lignes EP8 sans équivalent dans le client.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Merged {
    pub text: String,
    pub kept: usize,
    pub dropped: usize,
    pub ep8_only: usize,
}

/// Fusion d'une table à une ligne par clé (TID en colonne `key`) : lignes et valeurs de la table EP8, colonnes
/// `text_cols` (noms) reprises du client quand il a la ligne, colonnes `drop_cols` retirées.
pub fn merge_by_key(ep8: &str, client: &str, key: &str, text_cols: &[&str], drop_cols: &[&str]) -> Merged {
    let (eh, erows) = parse_tsv(ep8);
    let (ch, crows) = parse_tsv(client);
    let ek = eh.iter().position(|h| h == key);
    let ck = ch.iter().position(|h| h == key);
    let cell_key = |r: &[String], c: Option<usize>| c.and_then(|i| r.get(i)).map(|v| v.trim().to_string()).filter(|v| is_uint(v));
    let client_rows: HashMap<String, &Vec<String>> = crows.iter().filter_map(|r| Some((cell_key(r, ck)?, r))).collect();
    let keep_cols: Vec<usize> = (0..eh.len()).filter(|i| !drop_cols.contains(&eh[*i].as_str())).collect();
    let header: Vec<String> = keep_cols.iter().map(|i| eh[*i].clone()).collect();
    let text_at: Vec<(usize, Option<usize>)> = text_cols.iter().filter_map(|t| Some((eh.iter().position(|h| h == t)?, ch.iter().position(|h| h == t)))).collect();
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut ep8_only = 0;
    for r in &erows {
        let Some(k) = cell_key(r, ek) else { continue };
        let mut r = r.clone();
        r.resize(eh.len(), String::new());
        match client_rows.get(&k) {
            Some(c) => {
                for (ei, ci) in &text_at {
                    if let Some(v) = ci.and_then(|ci| c.get(ci)) {
                        r[*ei] = v.clone();
                    }
                }
            }
            None => ep8_only += 1,
        }
        seen.insert(k);
        out.push(keep_cols.iter().map(|i| r[*i].clone()).collect());
    }
    let dropped = client_rows.keys().filter(|k| !seen.contains(*k)).count();
    Merged { text: write_tsv(&header, &out), kept: out.len(), dropped, ep8_only }
}

/// LevelUpExp : niveaux de la table EP8, puis ceux du client au-delà de son dernier niveau. Rend le texte, le nombre
/// de niveaux EP8 et le nombre de niveaux repris du client.
pub fn merge_level_exp(ep8: &str, client: &str) -> (String, usize, usize) {
    let levels = |text: &str| -> BTreeMap<u32, String> {
        parse_tsv(text).1.iter().filter_map(|r| Some((r.first()?.trim().parse::<u32>().ok()?, r.get(1)?.trim().to_string()))).collect()
    };
    let (e, c) = (levels(ep8), levels(client));
    let last = e.keys().max().copied().unwrap_or(0);
    let added: Vec<(u32, String)> = c.into_iter().filter(|(l, _)| *l > last).collect();
    let rows: Vec<Vec<String>> = e.iter().map(|(l, x)| vec![l.to_string(), x.clone()]).chain(added.iter().map(|(l, x)| vec![l.to_string(), x.clone()])).collect();
    (write_tsv(&["level".to_string(), "exp".to_string()], &rows), e.len(), added.len())
}

/// Blocs de GrowUpFactor (une ligne « TID » seule, puis l'en-tête « LV … » et les niveaux), dans l'ordre du fichier.
pub fn growup_blocks(text: &str) -> Vec<(u32, Vec<String>)> {
    let mut out: Vec<(u32, Vec<String>)> = Vec::new();
    for line in super::lines(text) {
        let cells: Vec<&str> = line.split('\t').map(str::trim).collect();
        if cells[0].is_empty() {
            continue;
        }
        if is_uint(cells[0]) && cells[1..].iter().all(|c| c.is_empty()) {
            out.push((cells[0].parse().unwrap_or(0), vec![line.to_string()]));
            continue;
        }
        if let Some(b) = out.last_mut() {
            b.1.push(line.to_string());
        }
    }
    out
}

/// GrowUpFactor : blocs de la table EP8, puis ceux des modèles qu'elle n'a pas (repris du client). Rend le texte, le
/// nombre de modèles EP8 et les modèles repris du client.
pub fn merge_growup(ep8: &str, client: &str) -> (String, usize, Vec<u32>) {
    let e = growup_blocks(ep8);
    let known: HashSet<u32> = e.iter().map(|b| b.0).collect();
    let extra: Vec<(u32, Vec<String>)> = growup_blocks(client).into_iter().filter(|b| !known.contains(&b.0)).collect();
    let mut text = String::new();
    for (_, lines) in e.iter().chain(extra.iter()) {
        for l in lines {
            text.push_str(l);
            text.push_str("\r\n");
        }
    }
    (text, e.len(), extra.iter().map(|b| b.0).collect())
}

/// Section .ini en texte (« [id] » puis « clé=valeur »), en CRLF.
pub fn write_section(s: &Section) -> String {
    let mut out = format!("[{}]\r\n", s.id);
    for (k, v) in &s.order {
        out.push_str(&format!("{k}={v}\r\n"));
    }
    out
}

/// SKILLTEMPLATE : compétences de la table EP8 ; section du client (nom, icônes) quand il a la compétence, sinon celle
/// de la table EP8. Rend le texte, le nombre de compétences gardées, de compétences du client écartées, et les TID
/// dont le nom diffère (la jointure par nom se fait avec le modèle EP8).
pub fn merge_skill_template(ep8: &str, client: &str) -> (String, usize, usize, Vec<u32>) {
    let client_secs: HashMap<String, Section> = parse_ini(client).into_iter().filter(|s| is_uint(&s.id)).map(|s| (s.id.clone(), s)).collect();
    let mut text = String::new();
    let (mut kept, mut renamed) = (0, Vec::new());
    let mut seen = HashSet::new();
    for s in parse_ini(ep8).into_iter().filter(|s| is_uint(&s.id)) {
        let c = client_secs.get(&s.id);
        if let Some(c) = c {
            if c.first("1").map(str::trim) != s.first("1").map(str::trim) {
                renamed.push(s.id.parse().unwrap_or(0));
            }
        }
        text.push_str(&write_section(c.unwrap_or(&s)));
        seen.insert(s.id.clone());
        kept += 1;
    }
    let dropped = client_secs.keys().filter(|k| !seen.contains(*k)).count();
    renamed.sort_unstable();
    (text, kept, dropped, renamed)
}

/// Sorte de TID d'une colonne de questtemplate.ini (d'après son nom) : « monstre », « compétence » ou « objet » ;
/// `None` pour une quête (chaîne de quêtes) ou une colonne qui n'est pas un TID.
pub fn quest_column_kind(column: &str) -> Option<&'static str> {
    let c = column.to_ascii_lowercase();
    if !c.ends_with("tid") || c == "tid" || c.contains("quest") {
        return None;
    }
    Some(if c.contains("monster") || c.contains("npc") {
        "monstre"
    } else if c.contains("skill") {
        "compétence"
    } else {
        "objet"
    })
}

/// Ensemble EP8 visé par une colonne de TID de questtemplate.ini (voir `quest_column_kind`).
pub fn quest_column_set<'a>(column: &str, sets: &'a Ep8Sets) -> Option<&'a HashSet<u32>> {
    Some(match quest_column_kind(column)? {
        "monstre" => &sets.models,
        "compétence" => &sets.skills,
        _ => &sets.items,
    })
}

/// questtemplate.ini : quêtes du client sans celles qui citent un monstre, un objet ou une compétence hors EP8 (colonnes
/// « …Tid », « …TID »), ni celles dont la quête précédente (« Need_PreviousQuestTid ») est retirée. Rend le texte et
/// les quêtes retirées.
pub fn filter_quests(text: &str, sets: &Ep8Sets) -> (String, Vec<u32>) {
    let (h, rows) = parse_tsv(text);
    let cols: Vec<(usize, &HashSet<u32>)> = h.iter().enumerate().filter_map(|(i, c)| Some((i, quest_column_set(c, sets)?))).collect();
    let prev = h.iter().position(|c| c == "Need_PreviousQuestTid");
    let tid = |r: &[String]| r.first().and_then(|t| t.trim().parse::<u32>().ok());
    let mut removed: HashSet<u32> = HashSet::new();
    for r in &rows {
        let Some(t) = tid(r) else { continue };
        // Une récompense s'écrit « TID;nombre » (« 65448;1 ») : le TID est le premier nombre.
        let bad = cols.iter().any(|(i, set)| r.get(*i).map(|v| js_number(v.split(';').next().unwrap_or(""), 0.0) as i64).is_some_and(|v| v > 0 && !set.contains(&(v as u32))));
        if bad {
            removed.insert(t);
        }
    }
    // Chaînes : une quête qui suit une quête retirée l'est aussi.
    loop {
        let more: Vec<u32> = rows
            .iter()
            .filter_map(|r| Some((tid(r)?, prev.and_then(|p| r.get(p)).and_then(|v| v.trim().parse::<u32>().ok())?)))
            .filter(|(t, p)| !removed.contains(t) && removed.contains(p))
            .map(|(t, _)| t)
            .collect();
        if more.is_empty() {
            break;
        }
        removed.extend(more);
    }
    let kept: Vec<Vec<String>> = rows.into_iter().filter(|r| tid(r).is_none_or(|t| !removed.contains(&t))).collect();
    let mut removed: Vec<u32> = removed.into_iter().collect();
    removed.sort_unstable();
    (write_tsv(&h, &kept), removed)
}

/// questgroup.ini (sans en-tête : groupe, nom, quêtes proposées, quêtes terminées « 4611:4612: ») : quêtes retirées
/// ôtées des listes ; un groupe qui n'a plus aucune quête à cause d'elles est retiré (un groupe déjà vide reste).
/// Rend le texte et les groupes retirés.
pub fn filter_quest_groups(text: &str, removed: &[u32]) -> (String, Vec<u32>) {
    let mut out = String::new();
    let mut gone = Vec::new();
    for line in super::lines(text) {
        let mut c: Vec<String> = line.split('\t').map(str::to_string).collect();
        let Some(id) = c.first().and_then(|t| t.trim().parse::<u32>().ok()) else {
            if !line.trim().is_empty() {
                out.push_str(line);
                out.push_str("\r\n");
            }
            continue;
        };
        let (mut before, mut left) = (0, 0);
        for i in [2, 3] {
            let Some(cell) = c.get_mut(i) else { continue };
            before += cell.split(':').filter(|t| !t.trim().is_empty()).count();
            let kept: Vec<&str> = cell.split(':').map(str::trim).filter(|t| !t.is_empty() && t.parse::<u32>().is_ok_and(|q| !removed.contains(&q))).collect();
            left += kept.len();
            if kept.len() != cell.split(':').filter(|t| !t.trim().is_empty()).count() {
                *cell = kept.iter().map(|t| format!("{t}:")).collect();
            }
        }
        if left == 0 && before > 0 {
            gone.push(id);
            continue;
        }
        out.push_str(&c.join("\t"));
        out.push_str("\r\n");
    }
    (out, gone)
}

/// Fonctions de PNJ que la table EP8 a retirées et que le client garde (en plus des quêtes, fonction 27) :
/// réparation (17), avis de recherche (32).
pub const KEPT_CLIENT_FUNCTIONS: [&str; 2] = ["17", "32"];

/// Règles de la fusion des PNJ.
pub struct NpcRules<'a> {
    pub models: &'a HashSet<u32>,
    /// Boutiques importées (`npc_trade_list`).
    pub shops: &'a HashSet<u32>,
    /// Groupes de quêtes gardés.
    pub quest_groups: &'a HashSet<u32>,
    /// Position (x, z) sur le terrain du client.
    pub on_terrain: &'a dyn Fn(f64, f64) -> bool,
    /// Événements de quête (fonction 27) d'une autre source que le client (npc.ini de Webzen, `--webzen`), par numéro de
    /// PNJ : ils remplacent ceux du client quand le PNJ de cette source a le même nom ou le même modèle.
    pub quest_events: Option<&'a HashMap<String, Section>>,
}

/// Bilan de la fusion des PNJ.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct NpcStats {
    pub kept: usize,
    /// PNJ du client absents de la table EP8.
    pub client_only: usize,
    /// PNJ propres à la table EP8 (non importés).
    pub ep8_only: usize,
    /// PNJ dont le modèle est hors EP8.
    pub bad_model: usize,
    /// Déplacés de plus de 1 000 unités hors du terrain du client : position du client gardée.
    pub client_position: usize,
    /// Boutiques EP8 non importées : boutique du client gardée.
    pub client_shop: usize,
    pub quest_events: usize,
    /// Événements du client gardés (`KEPT_CLIENT_FUNCTIONS` : réparation, avis de recherche).
    pub client_repair: usize,
    /// PNJ du client dont le numéro désigne un autre PNJ dans la table EP8 : gardés tels quels.
    pub client_kept: usize,
}

/// Événements d'un PNJ : en-tête (clés avant le premier « Function ») et blocs « Function … » jusqu'à « EnumEnd ».
fn npc_parts(s: &Section) -> (Vec<(String, String)>, Vec<Vec<(String, String)>>) {
    let (mut head, mut blocks): (Vec<(String, String)>, Vec<Vec<(String, String)>>) = (Vec::new(), Vec::new());
    for (k, v) in &s.order {
        match k.as_str() {
            "Function" => blocks.push(vec![(k.clone(), v.clone())]),
            "EnumEnd" => {}
            _ => match blocks.last_mut() {
                Some(b) => b.push((k.clone(), v.clone())),
                None => head.push((k.clone(), v.clone())),
            },
        }
    }
    (head, blocks)
}

fn get<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

fn set(pairs: &mut [(String, String)], key: &str, value: &str) {
    if let Some(p) = pairs.iter_mut().find(|(k, _)| k == key) {
        p.1 = value.to_string();
    }
}

fn xz(pos: Option<&str>) -> Option<(f64, f64)> {
    let p: Vec<f64> = pos?.split(',').map(|x| js_number(x, 0.0)).collect();
    Some((*p.first()?, *p.get(2)?))
}

/// npc.ini : PNJ communs au client et à la table EP8, dans l'ordre du client. Nom du client ; modèle, position et
/// événements EP8, élément par élément (les listes Function / EventID / CondStart / CondEnd sont parallèles) ; quêtes
/// (fonction 27) du client si leur groupe est gardé ; boutique (fonction 9) EP8 si elle est importée, sinon celle du
/// même rang chez le client. Un PNJ déplacé de plus de 1 000 unités hors du terrain du client garde sa position.
pub fn merge_npcs(ep8: &str, client: &str, rules: &NpcRules) -> (String, NpcStats) {
    let ep8: HashMap<String, Section> = parse_ini(ep8).into_iter().filter(|s| is_uint(&s.id)).map(|s| (s.id.clone(), s)).collect();
    let mut st = NpcStats::default();
    let mut text = String::new();
    let mut seen = HashSet::new();
    let fun = |b: &Vec<(String, String)>| get(b, "Function").unwrap_or("").trim().to_string();
    for c in parse_ini(client).into_iter().filter(|s| is_uint(&s.id)) {
        let Some(found) = ep8.get(&c.id) else {
            st.client_only += 1;
            continue;
        };
        let (mut head, client_blocks) = npc_parts(&c);
        // Même numéro, autre PNJ (ni le nom ni le modèle : un sapin de Noël à la place d'un maître des titres) : la
        // table EP8 n'a pas ce PNJ ; il garde ses valeurs du client, et celui de la table EP8 n'est pas importé.
        let same = |k: &str| c.first(k).map(str::trim).is_some_and(|v| found.first(k).map(str::trim) == Some(v));
        let own = same("Name") || same("TID");
        if own {
            seen.insert(c.id.clone());
        } else {
            st.client_kept += 1;
        }
        let e = if own { found } else { &c };
        let (ehead, eblocks) = npc_parts(e);
        let tid = get(&ehead, "TID").or(get(&head, "TID")).unwrap_or("").trim().to_string();
        if !tid.parse::<u32>().is_ok_and(|t| rules.models.contains(&t)) {
            st.bad_model += 1;
            continue;
        }
        set(&mut head, "TID", &tid);
        let moved = match (xz(get(&head, "Position")), xz(get(&ehead, "Position"))) {
            (Some(a), Some(b)) => ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt() > 1000.0 && !(rules.on_terrain)(b.0, b.1),
            _ => false,
        };
        if moved {
            st.client_position += 1;
        } else {
            for k in ["Position", "Degree"] {
                if let Some(v) = get(&ehead, k) {
                    set(&mut head, k, v);
                }
            }
        }
        let client_shops: Vec<&Vec<(String, String)>> = client_blocks.iter().filter(|b| fun(b) == "9").collect();
        let mut blocks: Vec<Vec<(String, String)>> = Vec::new();
        let mut shop_rank = 0;
        for b in &eblocks {
            match fun(b).as_str() {
                "27" => {}
                "9" => {
                    let rank = shop_rank;
                    shop_rank += 1;
                    let ok = |b: &Vec<(String, String)>| get(b, "Template").and_then(|t| t.trim().parse::<u32>().ok()).is_some_and(|t| rules.shops.contains(&t));
                    if ok(b) {
                        blocks.push(b.clone());
                    } else if let Some(cb) = client_shops.get(rank).filter(|cb| ok(cb)) {
                        let mut b = b.clone();
                        set(&mut b, "Template", get(cb, "Template").unwrap_or("0"));
                        blocks.push(b);
                        st.client_shop += 1;
                    }
                }
                _ => blocks.push(b.clone()),
            }
        }
        // Réparation des forgerons (fonction 17) et tableau des avis de recherche des chefs de village (fonction 32) :
        // la table EP8 les a retirés (28 et 2 PNJ), comme les quêtes ; ceux du client sont gardés quand elle n'en a pas.
        for f in KEPT_CLIENT_FUNCTIONS {
            if !eblocks.iter().any(|b| fun(b) == f) {
                for b in client_blocks.iter().filter(|b| fun(b) == f) {
                    blocks.push(b.clone());
                    st.client_repair += 1;
                }
            }
        }
        // Quêtes : celles de l'autre source (Webzen) quand elle a ce PNJ, sous le même nom ou le même modèle.
        let quest_blocks: Vec<Vec<(String, String)>> = match rules.quest_events {
            None => client_blocks.iter().filter(|b| fun(b) == "27").cloned().collect(),
            Some(other) => other
                .get(&c.id)
                .filter(|o| {
                    let same = |k: &str| c.first(k).map(str::trim).is_some_and(|v| o.first(k).map(str::trim) == Some(v));
                    same("Name") || same("TID")
                })
                .map(|o| npc_parts(o).1.into_iter().filter(|b| fun(b) == "27").collect())
                .unwrap_or_default(),
        };
        for b in quest_blocks.iter() {
            if get(b, "QuestGroupID").and_then(|g| g.trim().parse::<u32>().ok()).is_some_and(|g| rules.quest_groups.contains(&g)) {
                blocks.push(b.clone());
                st.quest_events += 1;
            }
        }
        let mut order = head;
        for b in blocks {
            order.extend(b);
        }
        order.push(("EnumEnd".into(), "0".into()));
        text.push_str(&write_section(&Section { id: c.id.clone(), order }));
        st.kept += 1;
    }
    st.ep8_only = ep8.keys().filter(|k| !seen.contains(*k)).count();
    (text, st)
}

/// TeleportPoint.ini : points du client, dans son ordre (le numéro d'un point est son rang dans le fichier), avec le
/// contenu de la table EP8 ; les points propres à la table EP8 ne sont pas importés. Rend le texte, les points gardés,
/// ceux du client écartés et ceux de la table EP8 non importés.
pub fn merge_teleport_points(ep8: &str, client: &str) -> (String, usize, usize, usize) {
    let e: HashMap<String, Section> = parse_ini(ep8).into_iter().map(|s| (s.id.clone(), s)).collect();
    let mut text = String::new();
    let (mut kept, mut dropped) = (0, 0);
    let mut seen = HashSet::new();
    for c in parse_ini(client) {
        match e.get(&c.id) {
            Some(s) => {
                text.push_str(&write_section(s));
                seen.insert(c.id.clone());
                kept += 1;
            }
            None => dropped += 1,
        }
    }
    (text, kept, dropped, e.len() - seen.len())
}

/// Boutiques (npctradeitemlist.txt / npctradelist.txt) : bloc « Npc » et ses lignes, par numéro.
pub fn shop_blocks(text: &str) -> Vec<(u32, Vec<String>)> {
    let mut out: Vec<(u32, Vec<String>)> = Vec::new();
    let mut current: Option<u32> = None;
    for line in super::lines(text) {
        let first = line.split('\t').next().unwrap_or("").trim();
        if first == "Npc" {
            current = line.split('\t').nth(1).and_then(|t| t.trim().parse().ok());
            if let Some(id) = current {
                out.push((id, Vec::new()));
            }
        }
        if let (Some(_), Some(b)) = (current, out.last_mut()) {
            if !line.trim().is_empty() {
                b.1.push(line.to_string());
            }
        }
    }
    out
}

/// Boutiques : contenu EP8 des boutiques communes au client et à la table EP8 ; celles du client seul et celles de la
/// table EP8 seule sont écartées. Rend le texte, les boutiques gardées et (client seul, EP8 seul) écartées.
pub fn merge_shops(ep8: &str, client: &str) -> (String, HashSet<u32>, usize, usize) {
    let client_ids: HashSet<u32> = shop_blocks(client).into_iter().map(|b| b.0).collect();
    let e = shop_blocks(ep8);
    let mut text = String::new();
    let mut kept = HashSet::new();
    for (id, lines) in &e {
        if client_ids.contains(id) && kept.insert(*id) {
            for l in lines {
                text.push_str(l);
                text.push_str("\r\n");
            }
        }
    }
    let ep8_only = e.iter().filter(|b| !client_ids.contains(&b.0)).count();
    (text, kept.clone(), client_ids.len() - kept.len(), ep8_only)
}

/// Fenêtre des boss (UIBoss.xml) : blocs « <Boss …> » dont le TID est un modèle EP8. Rend le texte et les boss retirés.
pub fn filter_boss_xml(xml: &str, models: &HashSet<u32>) -> (String, Vec<u32>) {
    let mut out = String::new();
    let mut removed = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find("<Boss ") {
        let Some(end) = rest[at..].find("</Boss>").map(|e| at + e + "</Boss>".len()) else { break };
        let block = &rest[at..end];
        let tid = block.find("TID=\"").and_then(|i| block[i + 5..].split('"').next()).and_then(|t| t.trim().parse::<u32>().ok());
        // Le retrait emporte l'indentation qui précède le bloc et la fin de ligne qui le suit.
        let line_start = rest[..at].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let indent_only = rest[line_start..at].trim().is_empty();
        match tid {
            Some(t) if !models.contains(&t) => {
                removed.push(t);
                out.push_str(&rest[..if indent_only { line_start } else { at }]);
                let after = &rest[end..];
                let skip = if after.starts_with("\r\n") { 2 } else if after.starts_with('\n') { 1 } else { 0 };
                rest = &after[skip..];
            }
            _ => {
                out.push_str(&rest[..end]);
                rest = &rest[end..];
            }
        }
    }
    out.push_str(rest);
    (out, removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fusion_par_tid() {
        let ep8 = "TID\tItemName\tNPC Price\tNPC Arena Coin\r\n10\tSword\t500\t3\r\n11\tBow\t20\t\r\n12\tEP8 seul\t1\t\r\n";
        let client = "TID\tItemName\tNPC Price\tTransmogPrice\r\n10\tÉpée longue\t900\t7\r\n11\tBow\t20\t1\r\n14611\tArchonia\t1\t1\r\n";
        let m = merge_by_key(ep8, client, "TID", &["ItemName"], &EP8_ONLY_ITEM_COLUMNS);
        assert_eq!((m.kept, m.dropped, m.ep8_only), (3, 1, 1));
        let (h, rows) = parse_tsv(&m.text);
        assert_eq!(h, vec!["TID", "ItemName", "NPC Price"], "colonne propre à la table EP8 retirée, colonne d'Archonia absente");
        assert_eq!(rows[0], vec!["10", "Épée longue", "500"], "nom du client, prix EP8");
        assert_eq!(rows[2], vec!["12", "EP8 seul", "1"]);
        assert!(m.text.ends_with("\r\n"));
    }

    #[test]
    fn experience_et_croissance() {
        let (text, ep8, client) = merge_level_exp("level\texp\r\n1\t8\r\n2\t26\r\n", "level\texp\r\n1\t9\r\n2\t27\r\n3\t50\r\n");
        assert_eq!((ep8, client), (2, 1));
        assert_eq!(text, "level\texp\r\n1\t8\r\n2\t26\r\n3\t50\r\n", "1–2 EP8, 3 du client");
        let e = "96\t\t\r\nLV\tSTR\t\r\n1 \t17.00 \t\r\n2 \t18.00 \t\r\n";
        let c = "96\t\t\r\nLV\tSTR\t\r\n1\t9\t\r\n1276\t\t\r\nLV\tSTR\t\r\n1\t50\t\r\n";
        let (text, n, extra) = merge_growup(e, c);
        assert_eq!((n, extra), (1, vec![1276]));
        assert!(text.contains("1 \t17.00") && text.contains("1276\t\t\r\nLV\tSTR\t\r\n1\t50") && !text.contains("1\t9\t"));
        assert_eq!(growup_blocks(&text).iter().map(|b| (b.0, b.1.len())).collect::<Vec<_>>(), vec![(96, 4), (1276, 3)]);
    }

    #[test]
    fn competences_jointes_par_le_modele_ep8() {
        let ep8 = "0=ModuleData1\r\n1=Name\r\n[930]\r\n1=<Battle>Skill Upgrade\r\n3=P930\r\n[1]\r\n1=Dual Slash\r\n";
        let client = "[1]\r\n1=Dual Slash\r\n3=P0000001\r\n[930]\r\n1=Stamper Buff\r\n3=P0000930\r\n[4000]\r\n1=Archonia\r\n";
        let (text, kept, dropped, renamed) = merge_skill_template(ep8, client);
        assert_eq!((kept, dropped, renamed), (2, 1, vec![930]));
        let s = parse_ini(&text);
        assert_eq!((s[0].id.as_str(), s[0].first("1"), s[0].first("3")), ("930", Some("Stamper Buff"), Some("P0000930")), "nom et icône du client");
        // La table EP8 nomme 930 « <Battle>Skill Upgrade » : SKILL_SPEC et SKILL_CONST EP8 se joignent par ce nom.
        let tids = super::super::import::skill_tids(ep8);
        let (specs, missing) = super::super::import::parse_skill_specs("스킬명\trange\r\n<Battle>Skill Upgrade\t3\r\nStamper Buff\t9\r\n", &tids);
        assert_eq!((specs.len(), specs[0].0, specs[0].1.as_str(), missing), (1, 930, "<Battle>Skill Upgrade", 1));
    }

    #[test]
    fn quetes_hors_ep8_retirees() {
        let sets = Ep8Sets { items: [100].into(), models: [5].into(), skills: HashSet::new(), options: HashSet::new() };
        let text = "Tid\tQuestName\tNeed_PreviousQuestTid\tComplete_MonsterA_Tid\tComplete_ItemTid\tGift_ItemA_Tid\tGift_NextQuest_TID\r\n\
                    1\tA\t\t5\t\t100\t2\r\n\
                    2\tB\t1\t5604\t\t\t\r\n\
                    3\tC\t2\t5\t\t\t\r\n\
                    4\tD\t\t\t15513\t\t\r\n\
                    5\tE\t\t\t\t\t\r\n\
                    6\tF\t\t5\t\t65448;1\t\r\n\
                    7\tG\t\t5\t\t100;5;1\t\r\n";
        let (out, removed) = filter_quests(text, &sets);
        assert_eq!(removed, vec![2, 3, 4, 6], "monstre, objet et récompense « TID;nombre » d'Archonia ; 3 suit la quête 2");
        assert_eq!(parse_tsv(&out).1.len(), 3);
        let (groups, gone) = filter_quest_groups("38\t<A> Quest\t1:2:\t1:2:\t\r\n360\t<B> Quest\t3:4:\t4:\t\r\n7\t<C>\t\t\t\r\n", &removed);
        assert_eq!(gone, vec![360]);
        assert_eq!(groups, "38\t<A> Quest\t1:\t1:\t\r\n7\t<C>\t\t\t\r\n", "groupe déjà vide gardé");
    }

    #[test]
    fn fusion_des_pnj() {
        let client = "[3]\r\nModuleData1=AgpmCharacter\r\nName=<Merchant> Darryl\r\nTID=584\r\nPosition=0,0,0\r\nDegree=0,81\r\n\
                      Function=21\r\nEventID=1\r\nCondStart=0\r\nCondEnd=0\r\nNPCDialogStart=0\r\nTemplate=1\r\nNPCDialogEnd=0\r\n\
                      Function=9\r\nEventID=2\r\nCondStart=0\r\nCondEnd=0\r\nNPCTradeStart=0\r\nTemplate=104\r\nNPCTradeEnd=0\r\n\
                      Function=17\r\nEventID=4\r\nCondStart=0\r\nCondEnd=0\r\n\
                      Function=27\r\nEventID=3\r\nCondStart=0\r\nCondEnd=0\r\nQuestGroupID=1\r\nQuestEnd=0\r\nEnumEnd=0\r\n\
                      [814]\r\nName=Item Exchange\r\nTID=584\r\nPosition=0,0,0\r\nEnumEnd=0\r\n\
                      [7]\r\nName=Loin\r\nTID=584\r\nPosition=0,0,0\r\nDegree=0,0\r\nEnumEnd=0\r\n\
                      [8]\r\nName=Archonia\r\nTID=2207\r\nPosition=0,0,0\r\nEnumEnd=0\r\n\
                      [535]\r\nName=<Title Manager> Warrante\r\nTID=278\r\nPosition=1,0,1\r\nFunction=21\r\nEventID=9\r\nEnumEnd=0\r\n";
        let ep8 ="[3]\r\nModuleData1=AgpmCharacter\r\nName=<Merchant> Darryl\r\nTID=585\r\nPosition=500,0,0\r\nDegree=0,90\r\n\
                   Function=21\r\nEventID=1\r\nCondStart=0\r\nCondEnd=0\r\nNPCDialogStart=0\r\nTemplate=1\r\nNPCDialogEnd=0\r\n\
                   Function=9\r\nEventID=2\r\nCondStart=0\r\nCondEnd=0\r\nNPCTradeStart=0\r\nTemplate=17\r\nNPCTradeEnd=0\r\nEnumEnd=0\r\n\
                   [7]\r\nName=Loin\r\nTID=584\r\nPosition=900000,0,0\r\nDegree=0,0\r\nEnumEnd=0\r\n\
                   [8]\r\nName=Archonia\r\nTID=2207\r\nPosition=0,0,0\r\nEnumEnd=0\r\n\
                   [1]\r\nName=<Delfaras> Event\r\nTID=426\r\nPosition=0,0,0\r\nEnumEnd=0\r\n\
                   [535]\r\nName=Christmas Tree <Golundo>\r\nTID=2075\r\nPosition=9,0,9\r\nEnumEnd=0\r\n";
        let models: HashSet<u32> = [584, 585, 426, 278, 2075].into();
        let shops: HashSet<u32> = [104].into();
        let groups: HashSet<u32> = [1].into();
        let terrain = |x: f64, _z: f64| x.abs() < 100_000.0;
        let (text, st) = merge_npcs(ep8, client, &NpcRules { models: &models, shops: &shops, quest_groups: &groups, on_terrain: &terrain, quest_events: None });
        assert_eq!((st.kept, st.client_only, st.ep8_only, st.bad_model, st.client_position, st.client_shop, st.quest_events), (3, 1, 2, 1, 1, 1, 1));
        assert_eq!(st.client_kept, 1, "535 : autre PNJ au même numéro dans la table EP8");
        let warrante = parse_ini(&text).into_iter().find(|s| s.id == "535").unwrap();
        assert_eq!((warrante.first("Name"), warrante.first("TID"), warrante.first("Position")), (Some("<Title Manager> Warrante"), Some("278"), Some("1,0,1")));
        let s = parse_ini(&text);
        assert_eq!((s[0].first("TID"), s[0].first("Position"), s[0].first("Degree")), (Some("585"), Some("500,0,0"), Some("0,90")), "modèle et position EP8");
        assert_eq!(s[0].all("Function"), vec!["21", "9", "17", "27"], "réparation et quête du client ajoutées");
        assert_eq!(st.client_repair, 1);
        assert_eq!(s[0].all("Template"), vec!["1", "104"], "boutique EP8 (17) non importée : celle du client");
        assert_eq!(s[0].first("QuestGroupID"), Some("1"));
        assert_eq!(s[0].order.last().map(|p| p.0.as_str()), Some("EnumEnd"));
        assert_eq!(s[1].first("Position"), Some("0,0,0"), "déplacé hors du terrain : position du client");
        // Quêtes d'une autre source (npc.ini de Webzen) : celles du PNJ de même nom remplacent celles du client.
        let webzen: HashMap<String, Section> = parse_ini(
            "[3]\r\nName=<Merchant> Darryl\r\nTID=584\r\nFunction=27\r\nEventID=3\r\nCondStart=0\r\nCondEnd=0\r\nQuestGroupID=7\r\nQuestEnd=0\r\nEnumEnd=0\r\n\
             [7]\r\nName=Autre\r\nTID=999\r\nFunction=27\r\nEventID=3\r\nQuestGroupID=7\r\nEnumEnd=0\r\n",
        )
        .into_iter()
        .map(|s| (s.id.clone(), s))
        .collect();
        let groups: HashSet<u32> = [1, 7].into();
        let (text, st) = merge_npcs(ep8, client, &NpcRules { models: &models, shops: &shops, quest_groups: &groups, on_terrain: &terrain, quest_events: Some(&webzen) });
        let s = parse_ini(&text);
        assert_eq!((s[0].all("QuestGroupID"), s[1].all("QuestGroupID"), st.quest_events), (vec!["7"], vec![], 1), "groupe 7 de Webzen ; 7 : autre PNJ chez Webzen");
    }

    #[test]
    fn portails_boutiques_et_boss() {
        let (text, kept, dropped, ep8_only) = merge_teleport_points("[B]\r\nGroup=2\r\n[A]\r\nGroup=1\r\nSpecialType=3\r\n[C]\r\nGroup=9\r\n", "[A]\r\nGroup=1\r\n[B]\r\nGroup=1\r\n[Z]\r\n");
        assert_eq!((kept, dropped, ep8_only), (2, 1, 1));
        let s = parse_ini(&text);
        assert_eq!((s[0].id.as_str(), s[0].first("SpecialType"), s[1].first("Group")), ("A", Some("3"), Some("2")), "ordre du client, valeurs EP8");
        let ep8 = "Npc\t104\tDarryl\t//\r\nSell\t1\r\nItem\t11\t1\r\nItem\t12\t1\r\nNpc\t17\tGlobal Merchant\r\nItem\t1\t1\r\n";
        let client = "Npc\t104\t<만물상인>\t//\r\nSell\t1\r\nItem\t11\t1\r\nNpc\t379\tArena\r\nItem\t2\t1\r\n";
        let (text, kept, client_only, ep8_only) = merge_shops(ep8, client);
        assert_eq!((kept, client_only, ep8_only), ([104].into(), 1, 1));
        assert_eq!(text, "Npc\t104\tDarryl\t//\r\nSell\t1\r\nItem\t11\t1\r\nItem\t12\t1\r\n");
        let xml = "<UIBoss>\r\n\t<Boss TID=\"2073\" Category=\"Battlesquare\">\r\n\t\t<Name Text=\"Basilis\"/>\r\n\t</Boss>\r\n\t<Boss TID=\"5794\">\r\n\t</Boss>\r\n</UIBoss>";
        let (out, removed) = filter_boss_xml(xml, &[2073].into());
        assert_eq!(removed, vec![5794]);
        assert_eq!(out, "<UIBoss>\r\n\t<Boss TID=\"2073\" Category=\"Battlesquare\">\r\n\t\t<Name Text=\"Basilis\"/>\r\n\t</Boss>\r\n</UIBoss>");
    }
}
