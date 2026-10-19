//! Ressources Windows de `ArchRust.exe` : icône et informations de version.
//!
//! L'icône `res/archrust.ico` est celle d'Archonia.exe (groupe 135, PNG 256×256) : c'est une donnée
//! du client, gardée en local et jamais publiée (voir `.gitignore`). Sans elle, l'exécutable est
//! construit sans icône.

fn main() {
    println!("cargo:rerun-if-changed=res/archrust.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    if std::path::Path::new("res/archrust.ico").exists() {
        res.set_icon("res/archrust.ico");
    }
    res.set("ProductName", "ArchRust")
        .set("FileDescription", "ArchRust — client ArchLord")
        .set("OriginalFilename", "ArchRust.exe");
    if let Err(e) = res.compile() {
        // Sans rc.exe (SDK Windows absent), l'exécutable reste valide, sans icône.
        println!("cargo:warning=ressources Windows non intégrées : {e}");
    }
}
