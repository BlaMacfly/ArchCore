//! Affiche une section d'une table ini chiffrée du client (toutes les clés, dans l'ordre).
//! Usage : cargo run -p archrust-data --example ui_section -- <fichier.ini> <section> [max]
use archrust_data::ini::IniTable;
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let t = IniTable::from_encrypted(&std::fs::read(&a[0]).expect("lecture"));
    let max: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(60);
    let s = t.section(&a[1]).expect("section absente");
    println!("[{}] {} clés", a[1], s.entries.len());
    for (k, v) in s.entries.iter().take(max) {
        println!("{k}={}", v.chars().take(160).collect::<String>());
    }
}
