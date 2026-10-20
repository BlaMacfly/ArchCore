//! Embarque les scripts SQL dans l'exécutable : la base de référence (`sql/00*.sql`) et les migrations
//! (`sql/migrations/<base>/<AAAAMMJJHHMM>_<sujet>.sql`). Le serveur connaît ainsi, à la compilation,
//! les migrations qu'il attend (voir `src/migrations.rs`).

use std::fmt::Write as _;
use std::path::Path;

fn sql_files(dir: &Path) -> Vec<String> {
    let mut files: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|f| f.ends_with(".sql")).collect())
        .unwrap_or_default();
    files.sort();
    files
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sql");
    let root = root.canonicalize().unwrap_or(root);
    println!("cargo:rerun-if-changed={}", root.display());
    let mut out = String::from("// Généré par build.rs : ne pas modifier.\n\n/// Base de référence : (fichier, texte), dans l'ordre.\npub static BASELINE_FILES: &[(&str, &str)] = &[\n");
    for f in sql_files(&root) {
        let path = root.join(&f);
        writeln!(out, "    ({f:?}, include_str!({:?})),", path.display().to_string()).unwrap();
    }
    out.push_str("];\n\n/// Migrations : (base, identifiant, texte), dans l'ordre des identifiants.\npub static MIGRATION_FILES: &[(&str, &str, &str)] = &[\n");
    for base in ["auth", "characters", "world"] {
        let dir = root.join("migrations").join(base);
        println!("cargo:rerun-if-changed={}", dir.display());
        for f in sql_files(&dir) {
            let id = f.trim_end_matches(".sql");
            writeln!(out, "    ({base:?}, {id:?}, include_str!({:?})),", dir.join(&f).display().to_string()).unwrap();
        }
    }
    out.push_str("];\n");
    let dest = Path::new(&std::env::var("OUT_DIR").unwrap()).join("sql_files.rs");
    std::fs::write(dest, out).unwrap();
}
