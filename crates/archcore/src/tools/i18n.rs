//! Traduction du client (dossier `Game/ini_fr`).
//!
//! ```text
//!   archcore i18n coverage [Game]      lignes de ini_fr encore identiques à l'anglais (ini)
//!   archcore i18n strings [Game]       chaînes affichables encore en anglais → data/i18n/pending-en.json
//!   archcore i18n apply [--ascii] [--dry]
//! ```
//! `apply` applique la mémoire de traduction `data/i18n/fr.json`. La première exécution copie ini_fr
//! vers ini_fr.orig ; chaque application repart de ini_fr.orig (rejouable sans empiler les
//! remplacements). Les fichiers chiffrés à l'origine (RC4 / MD5("1111")) sont rechiffrés.
//!
//! Format de fr.json :
//! `{ "files": { "<fichier>": { "<texte actuel exact>": "<traduction>", "ko:<coréen décodé>": "…" } },
//!    "global": { "<texte actuel exact>": "<traduction>" } }`. Les cellules comparées sont les colonnes
//! séparées par des tabulations, sinon la valeur après « = ».

use super::{latin1, lines, load_ini, to_cp1252, INI_KEY};
use regex::Regex;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use unicode_normalization::UnicodeNormalization;

pub fn run(args: &[String]) -> anyhow::Result<()> {
    match args.first().map(String::as_str) {
        Some("coverage") => coverage(args.get(1)),
        Some("strings") => strings(args.get(1)),
        Some("apply") => apply(args.iter().any(|a| a == "--ascii"), args.iter().any(|a| a == "--dry")),
        _ => anyhow::bail!("usage : archcore i18n coverage|strings [Game] | apply [--ascii] [--dry]"),
    }
}

fn sorted_files(dir: &Path) -> anyhow::Result<Vec<String>> {
    let mut v: Vec<String> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    Ok(v)
}

fn ini_text(path: &Path) -> anyhow::Result<String> {
    Ok(latin1(&load_ini(path)?.1))
}

// --------------------------------------------------------------------------------- coverage

fn coverage(game: Option<&String>) -> anyhow::Result<()> {
    let game = super::game_dir(game)?;
    let tech = Regex::new(r"\b(ModuleData\d*|Agp\w+|Agc\w+|Apm\w+|TID|ID|Name|0x[0-9a-f]+)\b").unwrap();
    let word3 = Regex::new(r"[A-Za-z]{3,}").unwrap();
    let has_words = |s: &str| word3.is_match(&tech.replace_all(s, ""));
    let w = [26, 8, 8, 8, 14];
    let row = |cells: [String; 5]| cells.iter().enumerate().map(|(i, s)| format!("{s:<width$}", width = w[i])).collect::<String>();
    println!("{}", row(["fichier".into(), "lignes EN".into(), "lignes FR".into(), "texte FR".into(), "encore anglais".into()]));
    let (mut total_text, mut total_same) = (0usize, 0usize);
    for f in sorted_files(&game.join("ini_fr"))? {
        let en = game.join("ini").join(&f);
        if !en.exists() {
            println!("{}", row([f, "absent de ini".into(), String::new(), String::new(), String::new()]));
            continue;
        }
        let a = ini_text(&en)?;
        let b = ini_text(&game.join("ini_fr").join(&f))?;
        let en_set: HashSet<&str> = lines(&a).filter(|l| has_words(l)).collect();
        let (mut text, mut same) = (0usize, 0usize);
        for l in lines(&b) {
            if !has_words(l) {
                continue;
            }
            text += 1;
            if en_set.contains(l) {
                same += 1;
            }
        }
        total_text += text;
        total_same += same;
        let pct = if text > 0 { (100.0 * same as f64 / text as f64).round() } else { 0.0 };
        println!("{}", row([f, lines(&a).count().to_string(), lines(&b).count().to_string(), text.to_string(), format!("{same} ({pct} %)")]));
    }
    let pct = if total_text > 0 { (100.0 * total_same as f64 / total_text as f64).round() } else { 0.0 };
    println!("TOTAL : {total_same} lignes encore en anglais sur {total_text} ({pct} %)");
    Ok(())
}

// --------------------------------------------------------------------------------- strings

/// Chaînes « affichables » : au moins une lettre, pas un identifiant technique.
pub fn display_strings(text: &str) -> HashSet<String> {
    let tech = Regex::new(
        r"(?i)^(?:[A-Za-z_]+\d*|[\w./-]+\.(?:dds|tga|png|bmp|ini|txt|xml|wav|mp3|ogg|ecl|rws|dff|uvw)|[A-Z0-9_]+|-?[\d.:,;\s-]+|0x[0-9a-f]+)$",
    )
    .unwrap();
    let mut out = HashSet::new();
    for line in lines(text) {
        let cells: Vec<&str> = if line.contains('\t') { line.split('\t').collect() } else { vec![line.find('=').map(|i| &line[i + 1..]).unwrap_or(line)] };
        for c in cells {
            let c = c.trim();
            if c.chars().count() < 2 || !c.chars().any(|ch| ch.is_ascii_alphabetic()) || tech.is_match(c) {
                continue;
            }
            let tagged = c.starts_with('<') && c[1..].contains('>');
            if !c.chars().any(char::is_whitespace) && !tagged && c.chars().count() < 4 {
                continue;
            }
            out.insert(c.to_string());
        }
    }
    out
}

fn strings(game: Option<&String>) -> anyhow::Result<()> {
    let game = super::game_dir(game)?;
    let mut pending: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let (mut total, mut english, mut chars) = (0usize, 0usize, 0usize);
    println!("{:<28}{:<10}{:<12}caractères", "fichier", "chaînes", "en anglais");
    for f in sorted_files(&game.join("ini_fr"))? {
        let en_path = game.join("ini").join(&f);
        let en = if en_path.exists() { display_strings(&ini_text(&en_path)?) } else { HashSet::new() };
        let fr = display_strings(&ini_text(&game.join("ini_fr").join(&f))?);
        let mut still: Vec<String> = fr.iter().filter(|s| en.contains(*s)).cloned().collect();
        still.sort();
        let c: usize = still.iter().map(|s| s.chars().count()).sum();
        total += fr.len();
        english += still.len();
        chars += c;
        println!("{f:<28}{:<10}{:<12}{c}", fr.len(), still.len());
        pending.insert(f, still);
    }
    let pct = if total > 0 { (100.0 * english as f64 / total as f64).round() } else { 0.0 };
    println!("TOTAL : {english} chaînes distinctes encore en anglais sur {total} ({pct} %), {chars} caractères");
    let out = crate::config::root().join("data").join("i18n");
    std::fs::create_dir_all(&out)?;
    std::fs::write(out.join("pending-en.json"), serde_json::to_string_pretty(&pending)?)?;
    Ok(())
}

// --------------------------------------------------------------------------------- apply

fn strip_accents(s: &str) -> String {
    s.nfd()
        .filter(|c| !('\u{0300}'..='\u{036f}').contains(c))
        .collect::<String>()
        .replace('œ', "oe")
        .replace('Œ', "OE")
        .replace(['«', '»'], "\"")
        .replace('’', "'")
}

fn apply(ascii: bool, dry: bool) -> anyhow::Result<()> {
    let root = crate::config::root();
    let game = super::game_dir(None)?;
    let (src, dst) = (game.join("ini_fr.orig"), game.join("ini_fr"));
    let tm: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root.join("data").join("i18n").join("fr.json"))?)?;
    let as_map = |v: Option<&serde_json::Value>| -> HashMap<String, String> {
        v.and_then(|v| v.as_object())
            .map(|o| o.iter().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string()))).collect())
            .unwrap_or_default()
    };
    let global = as_map(tm.get("global"));
    if !src.exists() {
        std::fs::create_dir_all(&src)?;
        for f in sorted_files(&dst)? {
            std::fs::copy(dst.join(&f), src.join(&f))?;
        }
        println!("copie d'origine créée : {}", src.display());
    }
    let mut total = 0usize;
    for f in sorted_files(&src)? {
        let mut map = global.clone();
        map.extend(as_map(tm.get("files").and_then(|x| x.get(&f))));
        let ko: HashMap<&str, &str> = map.iter().filter_map(|(k, v)| k.strip_prefix("ko:").map(|k| (k, v.as_str()))).collect();
        if map.is_empty() {
            if !dry {
                std::fs::copy(src.join(&f), dst.join(&f))?;
            }
            continue;
        }
        let (encrypted, plain) = load_ini(&src.join(&f))?;
        let text = latin1(&plain);
        let mut count = 0usize;
        let mut translate = |cell: &str| -> String {
            let key = cell.trim();
            let mut out = map.get(key).map(String::as_str);
            if out.is_none() && key.chars().any(|c| ('\u{80}'..='\u{ff}').contains(&c)) {
                let bytes: Vec<u8> = key.chars().map(|c| c as u32 as u8).collect();
                let decoded = encoding_rs::EUC_KR.decode(&bytes).0;
                out = ko.get(decoded.as_ref()).copied();
            }
            let Some(out) = out else { return cell.to_string() };
            count += 1;
            let out = if ascii { strip_accents(out) } else { out.to_string() };
            cell.replacen(key, &out, 1)
        };
        let mut result = String::with_capacity(text.len());
        for (i, piece) in text.split('\n').enumerate() {
            if i > 0 {
                result.push('\n');
            }
            let (line, cr) = match piece.strip_suffix('\r') {
                Some(l) => (l, "\r"),
                None => (piece, ""),
            };
            if line.contains('\t') {
                result.push_str(&line.split('\t').map(&mut translate).collect::<Vec<_>>().join("\t"));
            } else if let Some(eq) = line.find('=') {
                result.push_str(&line[..eq + 1]);
                result.push_str(&translate(&line[eq + 1..]));
            } else {
                result.push_str(line);
            }
            result.push_str(cr);
        }
        total += count;
        println!("{f:<28} {count:>6} remplacement(s){}", if encrypted { " (rechiffré)" } else { "" });
        if dry {
            continue;
        }
        let bytes = to_cp1252(&result);
        let out = if encrypted { archcore_net::rc4::capi_crypt(INI_KEY, &bytes) } else { bytes };
        std::fs::write(dst.join(&f), out)?;
    }
    println!("{}{total} remplacement(s) au total{}", if dry { "[essai] " } else { "" }, if ascii { " (sans accents)" } else { "" });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chaines_affichables() {
        let s = display_strings("1=Sword of Light\n2=ICON_SWORD\n3=icon.dds\n4=Hi\n5=<Merchant>\nA\tGreat Axe\t12");
        assert!(s.contains("Sword of Light"));
        assert!(s.contains("Great Axe"));
        assert!(s.contains("<Merchant>"));
        assert!(!s.contains("ICON_SWORD"));
        assert!(!s.contains("icon.dds"));
        assert!(!s.contains("Hi"));
    }

    #[test]
    fn sans_accents() {
        assert_eq!(strip_accents("Épée « cœur »’"), "Epee \" coeur \"'");
    }
}
