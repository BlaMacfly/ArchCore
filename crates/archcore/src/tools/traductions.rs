//! Traductions d'ArchCore (`archcore import … --webzen <ini> --traductions <dossier>`) : textes de Webzen qui
//! n'existent pas dans une langue (quêtes restées en coréen, dialogues que le dossier de langue de Webzen n'a pas,
//! descriptions de titres seulement en anglais), traduits hors du jeu et rangés dans un dossier local, jamais dans
//! un dépôt (ce sont des données de jeu).
//!
//! ```text
//!   Dossier : un fichier <langue>.tsv par code de la table `language` (en.tsv, fr.tsv, pt-BR.tsv…), UTF-8.
//!   Lignes  : sorte <TAB> TID <TAB> champ <TAB> texte d'origine <TAB> traduction ; « # » en tête : commentaire ;
//!             la ligne « sorte… » est l'en-tête. Échappements dans les textes : \t, \n, \r, \\.
//!   Sortes  : quest (colonne de questtemplate.ini), title (colonne de titlestringtable.txt : Title_Class,
//!             Title_Name, Necessary_Condition_Desc, Title_Effect_Desc), group (questgroup.ini, champ name).
//!   Règles  : une traduction ne s'applique que si le texte d'origine est encore celui de l'import (sinon elle est
//!             « périmée » et notée dans data/import-webzen.log), et jamais à la place d'un texte que la langue a
//!             déjà (texte de Webzen de la langue, ligne de localized_text d'une autre source, titre traduit par le
//!             dossier de langue du client).
//!   Rangement : client_table « traduction/questtemplate.ini@<langue> », « traduction/titlestringtable.txt@<langue> »,
//!             « traduction/questgroup.ini@<langue> » (JSON, lus par export-client-tables) ; localized_text (noms et
//!             dialogues de départ des quêtes), source « archcore-traduction ».
//! ```

use std::collections::BTreeMap;
use std::path::Path;

/// Valeur de `localized_text.source` des traductions d'ArchCore.
pub const SOURCE: &str = "archcore-traduction";

/// Préfixe des noms de `client_table` des traductions (« traduction/questtemplate.ini@fr »).
pub const TABLE_PREFIX: &str = "traduction/";

/// Nom de `client_table` d'une table traduite dans une langue.
pub fn table_name(file: &str, lang: &str) -> String {
    format!("{TABLE_PREFIX}{file}@{lang}")
}

/// Sorte de texte traduit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Quest,
    Title,
    Group,
}

impl Kind {
    fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "quest" => Some(Kind::Quest),
            "title" => Some(Kind::Title),
            "group" => Some(Kind::Group),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Quest => "quest",
            Kind::Title => "title",
            Kind::Group => "group",
        }
    }
}

/// Ligne d'un fichier de traductions.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub kind: Kind,
    pub tid: u32,
    pub field: String,
    pub origin: String,
    pub text: String,
}

/// `\t`, `\n`, `\r`, `\\` → caractères ; un autre `\x` reste tel quel.
pub fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(o) => {
                out.push('\\');
                out.push(o);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Lignes d'un fichier : (lignes lues, erreurs « ligne N : … »). Une traduction vide est ignorée.
pub fn parse(text: &str) -> (Vec<Row>, Vec<String>) {
    let (mut rows, mut errors) = (Vec::new(), Vec::new());
    for (n, line) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.trim().is_empty() || line.starts_with('#') || line.starts_with("sorte\t") {
            continue;
        }
        let c: Vec<&str> = line.split('\t').collect();
        let (Some(kind), Some(tid)) = (c.first().and_then(|k| Kind::parse(k)), c.get(1).and_then(|t| t.trim().parse::<u32>().ok())) else {
            errors.push(format!("ligne {} : sorte ou TID illisible", n + 1));
            continue;
        };
        if c.len() != 5 {
            errors.push(format!("ligne {} : {} colonne(s) au lieu de 5", n + 1, c.len()));
            continue;
        }
        let text = unescape(c[4]).trim().to_string();
        if !text.is_empty() {
            rows.push(Row { kind, tid, field: c[2].trim().to_string(), origin: unescape(c[3]).trim().to_string(), text });
        }
    }
    (rows, errors)
}

/// Fichiers `<langue>.tsv` du dossier : langue (code normalisé, « pt-br » → « pt-BR ») → lignes ; erreurs de
/// lecture par fichier.
pub fn load_dir(dir: &Path) -> anyhow::Result<(BTreeMap<String, Vec<Row>>, Vec<String>)> {
    let mut out: BTreeMap<String, Vec<Row>> = BTreeMap::new();
    let mut errors = Vec::new();
    let rd = std::fs::read_dir(dir).map_err(|e| anyhow::anyhow!("--traductions {} : {e}", dir.display()))?;
    let mut files: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("tsv"))).collect();
    files.sort();
    for p in files {
        let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let Some(lang) = archcore_net::ext_text::normalize_language(stem) else {
            errors.push(format!("{} : nom de langue illisible", p.display()));
            continue;
        };
        let text = std::fs::read_to_string(&p).map_err(|e| anyhow::anyhow!("{} : {e}", p.display()))?;
        let (rows, errs) = parse(&text);
        errors.extend(errs.into_iter().map(|e| format!("{} : {e}", p.display())));
        out.entry(lang).or_default().extend(rows);
    }
    Ok((out, errors))
}

/// Traductions retenues pour une table : TID → champ → texte, et lignes périmées (texte d'origine changé).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Overlay {
    pub texts: BTreeMap<u32, BTreeMap<String, String>>,
    pub stale: Vec<String>,
    /// Lignes écartées parce que la langue a déjà son texte.
    pub kept_original: usize,
}

/// Traductions d'une sorte : retenues si `current(tid, champ)` (texte importé) vaut le texte d'origine de la ligne et
/// si `own(tid, champ)` (la langue a déjà son propre texte) est faux.
pub fn overlay(rows: &[Row], kind: Kind, current: impl Fn(u32, &str) -> Option<String>, own: impl Fn(u32, &str) -> bool) -> Overlay {
    let mut o = Overlay::default();
    for r in rows.iter().filter(|r| r.kind == kind) {
        match current(r.tid, &r.field) {
            Some(c) if c.trim() == r.origin => {
                if own(r.tid, &r.field) {
                    o.kept_original += 1;
                } else {
                    o.texts.entry(r.tid).or_default().insert(r.field.clone(), r.text.clone());
                }
            }
            _ => o.stale.push(format!("{} {} {} : texte d'origine changé ou absent", kind.name(), r.tid, r.field)),
        }
    }
    o
}

/// Cellules de titlestringtable.txt rangée par l'import (format du client : TID, FirstCategory, Title_Class,
/// SecondCategory, Title_Name, Necessary_Condition_Desc, Title_Effect_Desc) : TID → colonne → texte.
pub fn title_cells(stored: &str) -> BTreeMap<u32, BTreeMap<String, String>> {
    let (h, rows) = super::import::parse_tsv(stored);
    rows.iter()
        .filter_map(|r| {
            let tid = r.first()?.trim().parse::<u32>().ok()?;
            Some((tid, h.iter().cloned().zip(r.iter().map(|c| c.trim().to_string())).skip(1).collect()))
        })
        .collect()
}

/// Ligne de titlestringtable.txt (format du client, `line`) dont les cellules traduites (`cells`, colonne → texte)
/// remplacent celles de l'en-tête `header`.
pub fn translate_line(header: &[String], line: &str, cells: &BTreeMap<String, String>) -> String {
    line.split('\t')
        .enumerate()
        .map(|(i, c)| header.get(i).and_then(|h| cells.get(h)).map(String::as_str).unwrap_or(c).to_string())
        .collect::<Vec<_>>()
        .join("\t")
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "# Traductions d'ArchCore, langue fr\nsorte\tTID\tchamp\torigine\ttraduction\n\
        quest\t4232\tQuestName\t엘로르를 향해서\tVers Ellore\n\
        quest\t4232\tDialogue_Start\tA</n>B\\tC\tA</n>B\\\\C\n\
        title\t30001\tTitle_Effect_Desc\tKilling X grants you bonus XP.\tBonus d'expérience en tuant des X.\n\
        quest\t4233\tQuestName\tvieux texte\tAncien\n\
        quest\t4234\tQuestName\tWebzen\tDéjà traduit\n\
        quest\tx\tQuestName\ta\tb\n\
        group\t38\tname\tsans\n";

    #[test]
    fn fichier_de_traductions() {
        let (rows, errors) = parse(FILE);
        assert_eq!(rows.len(), 5);
        assert_eq!(errors, vec!["ligne 8 : sorte ou TID illisible".to_string(), "ligne 9 : 4 colonne(s) au lieu de 5".to_string()]);
        assert_eq!((rows[1].origin.as_str(), rows[1].text.as_str()), ("A</n>B\tC", "A</n>B\\C"), "échappements");
        assert_eq!(rows[2].kind, Kind::Title);
        assert_eq!(unescape("a\\nb\\x"), "a\nb\\x");
    }

    #[test]
    fn traduction_seulement_si_l_origine_est_la_meme_et_la_langue_sans_texte() {
        let (rows, _) = parse(FILE);
        let base: BTreeMap<(u32, &str), &str> = [((4232, "QuestName"), "엘로르를 향해서"), ((4232, "Dialogue_Start"), "A</n>B\tC"), ((4233, "QuestName"), "nouveau texte"), ((4234, "QuestName"), "Webzen")].into();
        let o = overlay(&rows, Kind::Quest, |t, c| base.get(&(t, c)).map(|s| s.to_string()), |t, _| t == 4234);
        assert_eq!(o.texts[&4232]["QuestName"], "Vers Ellore");
        assert_eq!(o.texts[&4232]["Dialogue_Start"], "A</n>B\\C");
        assert!(!o.texts.contains_key(&4233) && !o.texts.contains_key(&4234), "périmée, texte de la langue gardé");
        assert_eq!((o.stale, o.kept_original), (vec!["quest 4233 QuestName : texte d'origine changé ou absent".to_string()], 1));
    }

    #[test]
    fn ligne_de_titre_traduite() {
        let stored = "TID\tFirstCategory\tTitle_Class\tSecondCategory\tTitle_Name\tNecessary_Condition_Desc\tTitle_Effect_Desc\r\n\
                      30001\t401\tNatural Enemy (1-10)\t1\tBlack Spider's Natural Enemy\tKill 6\tKilling X grants you bonus XP.\r\n";
        let cells = title_cells(stored);
        assert_eq!(cells[&30001]["Title_Effect_Desc"], "Killing X grants you bonus XP.");
        let (rows, _) = parse(FILE);
        let o = overlay(&rows, Kind::Title, |t, c| cells.get(&t)?.get(c).cloned(), |_, _| false);
        let header: Vec<String> = super::super::import::parse_tsv(stored).0;
        let line = stored.lines().nth(1).unwrap();
        assert_eq!(translate_line(&header, line, &o.texts[&30001]), "30001\t401\tNatural Enemy (1-10)\t1\tBlack Spider's Natural Enemy\tKill 6\tBonus d'expérience en tuant des X.");
        assert_eq!(table_name("titlestringtable.txt", "pt-BR"), "traduction/titlestringtable.txt@pt-BR");
    }
}
