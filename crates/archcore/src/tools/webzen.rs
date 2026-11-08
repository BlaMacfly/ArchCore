//! Source « Webzen » de l'import (`archcore import … --webzen <dossier ini>`) : dossier `ini` du client officiel de
//! Webzen (fichiers chiffrés RC4 sur le fichier entier, clé MD5("1111") brute, ou déjà en clair). Ses données datent de
//! 2011 (épisode 7) : elles ne servent qu'aux tables que les tables EP8 n'ont pas, et seulement pour ce qu'elles citent
//! de présent dans les tables EP8.
//!
//! ```text
//!   Quêtes : fr/questtemplate.ini (3 270 quêtes, TID 2 à 4584 ; trois lignes d'en-tête, la troisième nomme les
//!        colonnes) pour les valeurs ; textes anglais de questtemplate.ini (2 969 quêtes) quand il a la quête, sinon
//!        ceux du dossier fr/ (souvent restés en coréen) ; textes de fr/, de/, esp/ et portu/ par langue.
//!        Groupes : fr/questgroup.ini (listes), noms de questgroup.ini quand il a le groupe. PNJ : événements de quête
//!        (fonction 27) de npc.ini, sur les PNJ gardés par la fusion EP8.
//!        Écartées : quête qui cite un monstre, un objet ou une compétence hors EP8 ; quête qu'aucun PNJ gardé ne
//!        donne ou ne reprend (sauf quête « personnelle », Self_Quest), dont celles d'un objet du monde (donneur
//!        désigné par un numéro d'objet : le serveur ne gère les quêtes que sur les PNJ) ; quête qui suit une quête
//!        écartée.
//!   Expérience : levelupexp.txt, niveaux 1 à 121 (au-delà de 2³² dès le niveau 113 : entiers 64 bits).
//!   Titres : titledatatable.txt (2 768) et titlestringtable.txt (catégories, textes) ; Title_Check_N y est le TID
//!        du premier titre de la famille qui définit la condition (10001 récolte, 30001 monstres tués…), là où
//!        Archonia écrit un numéro de type (2, 5, 15…) : `world::titles::check::normalize`.
//!   Maîtrises : skillmastery_high.txt et skillmastery_heroic.txt (conditions et TID d'origine, un seul arbre
//!        héroïque par classe).
//!   Textes des dossiers de langue : une cellule qui se lit en coréen (CP949 valide, syllabes hangûl) est du
//!        coréen resté tel quel ; sinon Windows-1252.
//! ```

use super::ep8::{self, Ep8Sets};
use super::import::{js_number, parse_tsv, TitleRow};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Dossiers de langue de Webzen : (dossier, code de la table `language`, dossier de langue du client d'Archonia).
pub const LANGUAGES: [(&str, &str, &str); 4] = [("fr", "fr", "ini_fr"), ("de", "de", "ini_de"), ("esp", "es", "ini_es"), ("portu", "pt-BR", "ini_pt")];

/// Dossier `ini` du client de Webzen.
pub struct Webzen {
    pub dir: PathBuf,
}

/// Nom réel d'un fichier ou d'un dossier de `dir`, casse ignorée (`QuestTemplate.ini` / `questtemplate.ini`).
fn real_name(dir: &Path, name: &str) -> Option<PathBuf> {
    let rd = std::fs::read_dir(dir).ok()?;
    rd.flatten().map(|e| e.path()).find(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.eq_ignore_ascii_case(name)))
}

impl Webzen {
    pub fn new(dir: PathBuf) -> anyhow::Result<Self> {
        let w = Webzen { dir };
        anyhow::ensure!(w.exists("questtemplate.ini") && w.exists("levelupexp.txt"), "{} : pas un dossier ini du client de Webzen (questtemplate.ini, levelupexp.txt)", w.dir.display());
        Ok(w)
    }

    /// Chemin d'un fichier (« fr/questtemplate.ini »), casse ignorée.
    fn path(&self, rel: &str) -> Option<PathBuf> {
        rel.split('/').try_fold(self.dir.clone(), |d, part| real_name(&d, part))
    }

    pub fn exists(&self, rel: &str) -> bool {
        self.path(rel).is_some()
    }

    /// Octets en clair (déchiffrés si besoin).
    pub fn bytes(&self, rel: &str) -> anyhow::Result<Vec<u8>> {
        let p = self.path(rel).ok_or_else(|| anyhow::anyhow!("{} : {rel} introuvable", self.dir.display()))?;
        Ok(super::load_ini(&p)?.1)
    }

    /// Fichier du dossier racine, en CP949.
    pub fn text(&self, rel: &str) -> anyhow::Result<String> {
        Ok(encoding_rs::EUC_KR.decode_without_bom_handling(&self.bytes(rel)?).0.into_owned())
    }
}

/// Syllabes ou lettres hangûl.
pub fn has_hangul(s: &str) -> bool {
    s.chars().any(|c| ('\u{AC00}'..='\u{D7A3}').contains(&c) || ('\u{3131}'..='\u{318E}').contains(&c))
}

/// Cellule d'un dossier de langue : coréen (CP949) si elle se lit ainsi et contient du hangûl, sinon Windows-1252.
pub fn cell_text(b: &[u8]) -> String {
    if b.is_ascii() {
        return String::from_utf8_lossy(b).into_owned();
    }
    if let Some(s) = encoding_rs::EUC_KR.decode_without_bom_handling_and_without_replacement(b) {
        if has_hangul(&s) {
            return s.into_owned();
        }
    }
    encoding_rs::WINDOWS_1252.decode_without_bom_handling(b).0.into_owned()
}

/// Cellules d'une ligne en octets, décodées : `western` pour un dossier de langue (`cell_text`), sinon CP949.
fn decode_cells(line: &[u8], western: bool) -> Vec<String> {
    line.split(|c| *c == b'\t')
        .map(|c| if western { cell_text(c) } else { encoding_rs::EUC_KR.decode_without_bom_handling(c).0.into_owned() })
        .collect()
}

fn byte_lines(b: &[u8]) -> impl Iterator<Item = &[u8]> {
    b.split(|c| *c == b'\n').map(|l| l.strip_suffix(b"\r").unwrap_or(l))
}

/// Table tabulée de Webzen : en-tête (la ligne dont la première cellule vaut `first`, après les lignes de titres en
/// coréen) et lignes par numéro (une ligne répétée garde la dernière).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    pub header: Vec<String>,
    pub rows: BTreeMap<u32, Vec<String>>,
}

impl Table {
    pub fn parse(bytes: &[u8], first: &str, western: bool) -> Self {
        let mut t = Table::default();
        let mut found = false;
        for l in byte_lines(bytes) {
            let cells = decode_cells(l, western);
            if !found {
                if cells.first().is_some_and(|c| c.trim() == first) {
                    t.header = cells.iter().map(|c| c.trim().to_string()).collect();
                    found = true;
                }
                continue;
            }
            if let Some(id) = cells.first().and_then(|c| c.trim().parse::<u32>().ok()) {
                t.rows.insert(id, cells);
            }
        }
        t
    }

    pub fn col(&self, name: &str) -> Option<usize> {
        self.header.iter().position(|h| h == name)
    }

    /// Cellule (rognée) de la ligne `id`, vide si absente.
    pub fn get(&self, id: u32, name: &str) -> &str {
        self.col(name).and_then(|i| self.rows.get(&id)?.get(i)).map(|s| s.trim()).unwrap_or("")
    }
}

/// Colonne de texte de questtemplate.ini (noms, dialogues, résumés, lieu) : prise dans la langue voulue, les autres
/// (valeurs) dans fr/questtemplate.ini.
pub fn is_quest_text(column: &str) -> bool {
    column.ends_with("Name") || column.starts_with("Dialogue_") || column.starts_with("Complete_Summary") || matches!(column, "Area" | "Location")
}

/// Quête prête pour `quest_template` : nom et colonnes non vides (valeurs de `values`, textes de `texts` quand il a
/// la quête, sinon de `values`).
pub fn quest_data(values: &Table, texts: &Table, tid: u32) -> (String, serde_json::Value) {
    let mut o = serde_json::Map::new();
    let row = values.rows.get(&tid);
    let text_row = texts.rows.get(&tid);
    for (i, h) in values.header.iter().enumerate().skip(1) {
        let from_text = is_quest_text(h) && text_row.is_some();
        let cell = if from_text { texts.col(h).and_then(|j| text_row?.get(j)) } else { row.and_then(|r| r.get(i)) };
        if let Some(c) = cell.map(|c| c.trim()).filter(|c| !c.is_empty()) {
            o.insert(h.clone(), serde_json::Value::String(c.to_string()));
        }
    }
    let name = o.get("QuestName").and_then(|v| v.as_str()).unwrap_or("").to_string();
    (name, serde_json::Value::Object(o))
}

/// Textes d'une langue qui diffèrent des textes de base (`base` : TID → colonne → texte) et ne sont pas restés en
/// coréen : TID → colonne → texte.
pub fn language_texts(lang: &Table, base: &BTreeMap<u32, serde_json::Value>) -> BTreeMap<u32, BTreeMap<String, String>> {
    let mut out: BTreeMap<u32, BTreeMap<String, String>> = BTreeMap::new();
    for (tid, data) in base {
        let Some(row) = lang.rows.get(tid) else { continue };
        for (i, h) in lang.header.iter().enumerate().filter(|(_, h)| is_quest_text(h)) {
            let Some(t) = row.get(i).map(|c| c.trim()).filter(|c| !c.is_empty() && !has_hangul(c)) else { continue };
            if data.get(h).and_then(|v| v.as_str()) != Some(t) {
                out.entry(*tid).or_default().insert(h.clone(), t.to_string());
            }
        }
    }
    out
}

/// Choix des quêtes : gardées et écartées (TID → raison).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QuestPlan {
    pub kept: BTreeSet<u32>,
    pub dropped: BTreeMap<u32, String>,
}

/// Quêtes gardées : celles dont les monstres, objets et compétences cités sont dans les tables EP8, qu'un PNJ gardé
/// donne (`offered`) et reprend (`returned`) — une quête « personnelle » (Self_Quest) n'en a pas besoin —, et dont la
/// quête précédente est gardée.
pub fn plan_quests(t: &Table, sets: &Ep8Sets, offered: &HashSet<u32>, returned: &HashSet<u32>) -> QuestPlan {
    let mut plan = QuestPlan::default();
    let refs: Vec<(usize, &str, &HashSet<u32>)> =
        t.header.iter().enumerate().filter_map(|(i, c)| Some((i, ep8::quest_column_kind(c)?, ep8::quest_column_set(c, sets)?))).collect();
    for (tid, row) in &t.rows {
        // Une récompense s'écrit « TID;nombre » : le TID est le premier nombre.
        let bad = refs.iter().find_map(|(i, kind, set)| {
            let v = js_number(row.get(*i)?.split(';').next().unwrap_or(""), 0.0) as i64;
            (v > 0 && !set.contains(&(v as u32))).then(|| format!("{kind} {v} hors EP8 ({})", t.header[*i]))
        });
        let personal = js_number(t.get(*tid, "Self_Quest"), 0.0) != 0.0;
        let npc = |col: &str| match t.get(*tid, col) {
            "" => String::new(),
            n => format!(" ({n})"),
        };
        // Donneur ou receveur numéro d'objet du monde (obj*.ini, fonction 27 sur un objet) : le serveur ne gère les
        // événements de quête que sur les PNJ.
        let object = |col: &str| is_object(t.get(*tid, col)).then(|| format!("objet du monde {} ({col}) : quêtes des objets non gérées", t.get(*tid, col)));
        let why = bad
            .or_else(|| (!personal && !offered.contains(tid)).then(|| object("GiveNpcName").unwrap_or_else(|| format!("aucun PNJ gardé ne la donne{}", npc("GiveNpcName")))))
            .or_else(|| (!personal && !returned.contains(tid)).then(|| object("ConfirmNpcName").unwrap_or_else(|| format!("aucun PNJ gardé ne la reprend{}", npc("ConfirmNpcName")))));
        match why {
            Some(w) => {
                plan.dropped.insert(*tid, w);
            }
            None => {
                plan.kept.insert(*tid);
            }
        }
    }
    // Chaînes : une quête qui suit une quête écartée ou absente l'est aussi.
    loop {
        let more: Vec<(u32, String)> = plan
            .kept
            .iter()
            .filter_map(|tid| {
                let prev = t.get(*tid, "Need_PreviousQuestTid").parse::<u32>().ok().filter(|p| *p > 0)?;
                (!plan.kept.contains(&prev)).then(|| (*tid, format!("quête précédente {prev} {}", if t.rows.contains_key(&prev) { "écartée" } else { "absente" })))
            })
            .collect();
        if more.is_empty() {
            break;
        }
        for (tid, why) in more {
            plan.kept.remove(&tid);
            plan.dropped.insert(tid, why);
        }
    }
    plan
}

/// Donneur de quête désigné par un numéro d'objet du monde (« 165609488 ») plutôt que par un nom de PNJ.
fn is_object(name: &str) -> bool {
    name.len() >= 6 && name.chars().all(|c| c.is_ascii_digit())
}

/// Raison sans ses nombres ni le nom du PNJ, pour compter les raisons principales (« objet hors EP8
/// (Gift_ItemA_Tid) », « aucun PNJ gardé ne la donne »).
pub fn reason_key(why: &str) -> String {
    let w = if why.starts_with("aucun PNJ") { why.split(" (").next().unwrap_or(why) } else { why };
    w.split_whitespace().filter(|t| !t.trim_end_matches(')').chars().all(|c| c.is_ascii_digit() || c == ';')).collect::<Vec<_>>().join(" ")
}

/// questgroup.ini de Webzen (sans en-tête : groupe, nom, quêtes proposées, quêtes reprises « a:b: ») : listes de
/// `lists` (fr/), nom de `names` (racine, en anglais) quand il a le groupe. Rend (groupe, nom, proposées, reprises).
pub fn quest_groups(lists: &[u8], names: Option<&[u8]>) -> Vec<(u32, String, Vec<u32>, Vec<u32>)> {
    let parse = |b: &[u8], western: bool| -> Vec<Vec<String>> { byte_lines(b).map(|l| decode_cells(l, western)).filter(|c| c.first().is_some_and(|x| x.trim().parse::<u32>().is_ok())).collect() };
    let english: HashMap<u32, String> = names.map(|b| parse(b, false)).unwrap_or_default().into_iter().filter_map(|c| Some((c[0].trim().parse().ok()?, c.get(1)?.trim().to_string()))).collect();
    let list = |s: Option<&String>| s.map(|s| s.split(':').filter_map(|t| t.trim().parse::<u32>().ok()).collect()).unwrap_or_default();
    parse(lists, true)
        .into_iter()
        .map(|c| {
            let id: u32 = c[0].trim().parse().unwrap_or(0);
            let name = english.get(&id).cloned().unwrap_or_else(|| c.get(1).map(|s| s.trim().to_string()).unwrap_or_default());
            (id, name, list(c.get(2)), list(c.get(3)))
        })
        .collect()
}

/// Groupes de quêtes sans les quêtes écartées ; un groupe vidé par elles est retiré (un groupe déjà vide reste).
/// Rend les groupes gardés et le nombre de groupes retirés.
pub fn keep_groups(groups: Vec<(u32, String, Vec<u32>, Vec<u32>)>, kept: &BTreeSet<u32>) -> (Vec<(u32, String, Vec<u32>, Vec<u32>)>, usize) {
    let mut gone = 0;
    let out = groups
        .into_iter()
        .filter_map(|(id, name, give, confirm)| {
            let before = give.len() + confirm.len();
            let (give, confirm): (Vec<u32>, Vec<u32>) = (give.into_iter().filter(|q| kept.contains(q)).collect(), confirm.into_iter().filter(|q| kept.contains(q)).collect());
            if before > 0 && give.is_empty() && confirm.is_empty() {
                gone += 1;
                return None;
            }
            Some((id, name, give, confirm))
        })
        .collect();
    (out, gone)
}

/// levelupexp.txt (« level exp ») : (niveau, expérience pour passer au suivant), en 64 bits.
pub fn level_exp(text: &str) -> Vec<(u32, u64)> {
    parse_tsv(text).1.iter().filter_map(|r| Some((r.first()?.trim().parse().ok()?, r.get(1)?.trim().parse().ok()?))).collect()
}

/// Titres gardés et écartés (TID → raison) : coûts, compétences, cibles des conditions et des effets dans les tables
/// EP8. Les conditions sont déjà normalisées (`check::normalize`).
pub fn plan_titles(all: Vec<TitleRow>, sets: &Ep8Sets) -> (Vec<TitleRow>, BTreeMap<u32, String>) {
    use crate::world::titles::{check, effect};
    let mut dropped = BTreeMap::new();
    let mut kept = Vec::new();
    for t in all {
        let model = |x: i64| x > 0 && sets.models.contains(&(x as u32));
        let item = |x: i64| x > 0 && sets.items.contains(&(x as u32));
        let why = t
            .cost_items
            .iter()
            .find(|(i, _)| !sets.items.contains(i))
            .map(|(i, _)| format!("objet de coût {i} hors EP8"))
            .or_else(|| t.skills.iter().find(|s| !sets.skills.contains(s)).map(|s| format!("compétence {s} hors EP8")))
            .or_else(|| {
                t.checks.iter().find_map(|&(kind, set, _)| match kind {
                    check::COLLECT | check::KILL if !model(set) => Some(format!("monstre {set} hors EP8 (condition {kind})")),
                    check::COLLECT_ITEM | check::USE_ITEM if !item(set) => Some(format!("objet {set} hors EP8 (condition {kind})")),
                    _ => None,
                })
            })
            .or_else(|| {
                t.effects.iter().find_map(|&(kind, _, v1, _)| match kind {
                    effect::DOUBLE_COLLECT | effect::KILL_EXP if !model(v1) => Some(format!("monstre {v1} hors EP8 (effet {kind})")),
                    effect::POTION | effect::REFINE if !item(v1) => Some(format!("objet {v1} hors EP8 (effet {kind})")),
                    _ => None,
                })
            });
        match why {
            Some(w) => {
                dropped.insert(t.tid, w);
            }
            None => kept.push(t),
        }
    }
    (kept, dropped)
}

/// Lignes de titlestringtable.txt de Webzen (TID, FirstCategory, Title_Class_Korea, SecondCategory, Title_Name_Korea…
/// : les colonnes « _Korea » portent le texte du dossier) au format du client d'Archonia (TID, FirstCategory,
/// Title_Class, SecondCategory, Title_Name, Necessary_Condition_Desc, Title_Effect_Desc), pour les titres `kept`.
pub fn title_strings(text: &str, kept: &HashSet<u32>) -> String {
    let (h, rows) = parse_tsv(text);
    let at = |n: &str| h.iter().position(|x| x == n);
    let cols = ["TID", "FirstCategory", "Title_Class_Korea", "SecondCategory", "Title_Name_Korea", "Necessary_Condition_Desc_Korea", "Title_Effect_Desc_Korea"].map(at);
    let mut out = String::from("TID\tFirstCategory\tTitle_Class\tSecondCategory\tTitle_Name\tNecessary_Condition_Desc\tTitle_Effect_Desc\r\n");
    for r in rows {
        if !r.first().and_then(|t| t.trim().parse::<u32>().ok()).is_some_and(|t| kept.contains(&t)) {
            continue;
        }
        let line: Vec<&str> = cols.iter().map(|c| c.and_then(|i| r.get(i)).map(|s| s.trim()).unwrap_or("")).collect();
        out.push_str(&line.join("\t"));
        out.push_str("\r\n");
    }
    out
}

/// Fichier de Webzen filtré sur les lignes gardées (première cellule numérique dans `kept`, autres lignes gardées).
pub fn filter_rows(text: &str, kept: &HashSet<u32>) -> String {
    let mut out = String::new();
    for l in super::lines(text) {
        let id = l.split('\t').next().and_then(|c| c.trim().parse::<u32>().ok());
        if id.is_none_or(|t| kept.contains(&t)) && !(id.is_none() && l.trim().is_empty()) {
            out.push_str(l);
            out.push_str("\r\n");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quests() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice("퀘스트 Tid\t시나리오 이름\r\n".as_bytes());
        b.extend_from_slice(b"\t\tNiveau\r\n");
        b.extend_from_slice(b"Tid\tQuestName\tGiveNpcName\tComplete_MonsterA_Tid\tGift_ItemA_Tid\tNeed_PreviousQuestTid\tSelf_Quest\tGift_Exp\r\n");
        b.extend_from_slice(b"2\tLe Village humain\tRembrandt\t450\t\t\t\t10\r\n");
        b.extend_from_slice(b"3\tL'ancre perdue\tRembrandt\t\t65448;1\t2\t\t20\r\n");
        b.extend_from_slice(b"4\tPlus loin\tRembrandt\t\t\t3\t\t30\r\n");
        b.extend_from_slice(b"5\tSeul\tPersonne\t\t\t\t1\t40\r\n");
        b.extend_from_slice(b"6\tSans PNJ\tPersonne\t\t\t\t\t50\r\n");
        b.extend_from_slice(b"7\t\xb8\xb6\xc0\xbb\tx\t\t\t\t1\t0\r\n");
        b
    }

    #[test]
    fn cellules_coreennes_ou_occidentales() {
        assert_eq!(cell_text(b"Bienvenue \xe0 Anchorville"), "Bienvenue à Anchorville");
        assert_eq!(cell_text(b"Ghoul King \xbf\xa4\xb7\xce\xb8\xa3"), "Ghoul King 엘로르");
        assert_eq!(cell_text(b"S\xf6ldner"), "Söldner");
        assert!(has_hangul("마을") && !has_hangul("Söldner"));
    }

    #[test]
    fn table_a_trois_lignes_d_en_tete() {
        let t = Table::parse(&quests(), "Tid", true);
        assert_eq!(t.header[..3], ["Tid", "QuestName", "GiveNpcName"]);
        assert_eq!(t.rows.len(), 6);
        assert_eq!((t.get(2, "QuestName"), t.get(7, "QuestName"), t.get(3, "Gift_ItemA_Tid")), ("Le Village humain", "마을", "65448;1"));
        let en = Table::parse(b"Tid\tQuestName\tGiveNpcName\tGift_Exp\r\n2\tThe Human Village\tRembrandt\t99\r\n", "Tid", false);
        let (name, data) = quest_data(&t, &en, 2);
        assert_eq!((name.as_str(), data["Gift_Exp"].as_str(), data["Complete_MonsterA_Tid"].as_str()), ("The Human Village", Some("10"), Some("450")), "texte anglais, valeurs de fr/");
        assert_eq!(quest_data(&t, &en, 3).0, "L'ancre perdue", "pas en anglais : texte de fr/");
        let base: BTreeMap<u32, serde_json::Value> = [2, 7].iter().map(|tid| (*tid, quest_data(&t, &en, *tid).1)).collect();
        let fr = language_texts(&t, &base);
        assert_eq!(fr[&2]["QuestName"], "Le Village humain");
        assert!(!fr.contains_key(&7), "coréen : pas une traduction");
    }

    #[test]
    fn quetes_gardees_et_ecartees() {
        let t = Table::parse(&quests(), "Tid", true);
        let sets = Ep8Sets { models: [450].into(), items: [11].into(), ..Default::default() };
        let offered: HashSet<u32> = [2, 3, 4].into();
        let plan = plan_quests(&t, &sets, &offered, &offered);
        assert_eq!(plan.kept, [2, 5, 7].into());
        assert_eq!(plan.dropped[&3], "objet 65448 hors EP8 (Gift_ItemA_Tid)");
        assert_eq!(plan.dropped[&4], "quête précédente 3 écartée");
        assert_eq!(plan.dropped[&6], "aucun PNJ gardé ne la donne (Personne)");
        let mut objects = t.clone();
        objects.rows.get_mut(&6).unwrap()[2] = "165609488".into();
        assert_eq!(plan_quests(&objects, &sets, &offered, &offered).dropped[&6], "objet du monde 165609488 (GiveNpcName) : quêtes des objets non gérées");
        assert_eq!((reason_key(&plan.dropped[&3]), reason_key(&plan.dropped[&6])), ("objet hors EP8 (Gift_ItemA_Tid)".into(), "aucun PNJ gardé ne la donne".into()));
        let returned: HashSet<u32> = [3].into();
        assert_eq!(plan_quests(&t, &sets, &offered, &returned).dropped[&2], "aucun PNJ gardé ne la reprend");
        let groups = quest_groups(b"1\t\xb8\xb6\xc0\xbb\t2:3:\t2:\t\r\n9\tx\t4:\t\t\r\n10\tvide\t\t\t\r\n", Some(b"1\t<Village Chief> Rembrandt\t2:\t2:\r\n"));
        assert_eq!(groups[0], (1, "<Village Chief> Rembrandt".into(), vec![2, 3], vec![2]));
        assert_eq!(groups[1].1, "x");
        let (kept, gone) = keep_groups(groups, &plan.kept);
        assert_eq!((kept.iter().map(|g| (g.0, g.2.clone())).collect::<Vec<_>>(), gone), (vec![(1, vec![2]), (10, vec![])], 1));
    }

    #[test]
    fn experience_en_64_bits() {
        let lv = level_exp("level\texp\r\n1\t48\r\n121\t6640723530\r\n");
        assert_eq!(lv, vec![(1, 48), (121, 6_640_723_530)]);
        assert!(lv[1].1 > u32::MAX as u64);
    }

    #[test]
    fn textes_des_titres_au_format_du_client() {
        let text = "TID\tFirstCategory\tTitle_Class_Korea\tSecondCategory\tTitle_Name_Korea\tTitle_Class_English\tNecessary_Condition_Desc_Korea\tTitle_Effect_Desc_Korea\r\n\
                    30001\t401\tNatural Enemy (1-10)\t1\tBlack Spider's Natural Enemy\t0\tKill 6\tBonus XP\r\n90001\t1000\tKiller\t1\tKiller\t0\t30 PK\tNone\r\n";
        let out = title_strings(text, &[30001].into());
        assert_eq!(out.lines().nth(1), Some("30001\t401\tNatural Enemy (1-10)\t1\tBlack Spider's Natural Enemy\tKill 6\tBonus XP"));
        assert_eq!(out.lines().count(), 2);
        assert_eq!(filter_rows("TID\tx\r\n1\ta\r\n2\tb\r\n", &[2].into()), "TID\tx\r\n2\tb\r\n");
    }
}
