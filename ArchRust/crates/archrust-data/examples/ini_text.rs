//! Déchiffre une table ini du client et l'écrit en clair sur la sortie standard (usage privé).
//! Usage : cargo run -p archrust-data --example ini_text -- <table.ini>
fn main() {
    let path = std::env::args().nth(1).expect("table .ini");
    let raw = std::fs::read(&path).expect("lecture");
    print!("{}", archrust_data::ini::decrypt_ini(&raw));
}
