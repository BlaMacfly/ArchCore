//! Liste les fenêtres d'une définition d'interface Archonia (`UI_1024X768.ini`) : position,
//! taille, textures et contrôles (type, nom, position, taille, textures). Filtre facultatif.
//! Usage : cargo run -p archrust-data --example ui_windows -- <UI.ini> [motif] [--controls]
use archrust_data::ini::IniTable;
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let t = IniTable::from_encrypted(&std::fs::read(&a[0]).expect("lecture"));
    let pat = a.get(1).filter(|s| !s.starts_with("--")).map(|s| s.to_ascii_lowercase());
    let controls = a.iter().any(|s| s == "--controls");
    for s in &t.sections {
        let name = &s.name;
        if let Some(p) = &pat {
            if !name.to_ascii_lowercase().contains(p.as_str()) {
                continue;
            }
        }
        let get = |k: &str| s.entries.iter().find(|(kk, _)| kk == k).map(|(_, v)| v.as_str()).unwrap_or("");
        let tex: Vec<_> = s.entries.iter().take_while(|(k, _)| k != "CtrlType").filter(|(k, _)| k == "Texture").map(|(_, v)| v.as_str()).collect();
        let nctrl = s.entries.iter().filter(|(k, _)| k == "CtrlType").count();
        println!("[{name}] pos={} size={} tex={:?} contrôles={nctrl}", get("Pos"), get("Size"), tex);
        if controls {
            let mut cur: Vec<String> = Vec::new();
            for (k, v) in s.entries.iter().skip_while(|(k, _)| k != "CtrlType") {
                if k == "CtrlType" && !cur.is_empty() {
                    println!("    {}", cur.join(" "));
                    cur.clear();
                }
                if matches!(k.as_str(), "CtrlType" | "CtrlName" | "Pos" | "Size" | "Texture" | "CtrlUserData" | "CtrlUserData2" | "DefaultIDs") {
                    cur.push(format!("{k}={}", v.chars().take(60).collect::<String>()));
                }
            }
            if !cur.is_empty() {
                println!("    {}", cur.join(" "));
            }
        }
    }
}
