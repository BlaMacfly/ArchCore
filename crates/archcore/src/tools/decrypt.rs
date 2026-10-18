//! `archcore decrypt [source] [destination]` : déchiffre les tables de données du client 64 bits
//! (dossier `Game/ini`) vers `data/client-ini/`. Chiffrement constaté dans le client : CryptoAPI
//! MD5("1111") → RC4. Les fichiers déjà en clair sont copiés tels quels ; les sous-dossiers sont parcourus.

use super::INI_KEY;
use std::path::{Path, PathBuf};

/// Texte en clair : moins de 5 % d'octets hors ASCII imprimable, CR/LF/tabulation et 0xA1-0xFF
/// (critère plus strict que celui des outils de traduction : 0x7F-0xA0 comptent comme suspects).
fn looks_plain(buf: &[u8]) -> bool {
    let n = buf.len().min(256);
    if n == 0 {
        return true;
    }
    let ok = buf[..n].iter().filter(|&&c| (0x20..0x7f).contains(&c) || matches!(c, 0x0d | 0x0a | 0x09) || c >= 0xa1).count();
    ((n - ok) as f64) / (n as f64) < 0.05
}

#[derive(Default)]
struct Stats {
    decrypted: usize,
    copied: usize,
    failed: usize,
}

fn walk(src: &Path, dst: &Path, root: &Path, st: &mut Stats) -> anyhow::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let (s, d) = (entry.path(), dst.join(entry.file_name()));
        if entry.file_type()?.is_dir() {
            walk(&s, &d, root, st)?;
            continue;
        }
        let data = std::fs::read(&s)?;
        if looks_plain(&data) {
            std::fs::write(&d, &data)?;
            st.copied += 1;
            continue;
        }
        let key = std::env::var("ARCHCORE_INI_KEY").unwrap_or_else(|_| String::from_utf8_lossy(INI_KEY).into_owned());
        let out = archcore_net::rc4::capi_crypt(key.as_bytes(), &data);
        if !looks_plain(&out) {
            st.failed += 1;
            println!("?? {} : toujours illisible après déchiffrement", s.strip_prefix(root).unwrap_or(&s).display());
        }
        std::fs::write(&d, out)?;
        st.decrypted += 1;
    }
    Ok(())
}

pub fn run(args: &[String]) -> anyhow::Result<()> {
    let src = match args.first() {
        Some(a) => PathBuf::from(a),
        None => super::game_dir(None)?.join("ini"),
    };
    let dst = match args.get(1) {
        Some(a) => PathBuf::from(a),
        None => crate::config::root().join("data").join("client-ini"),
    };
    let mut st = Stats::default();
    walk(&src, &dst, &src, &mut st)?;
    println!("déchiffrés : {}, copiés en clair : {}, douteux : {} → {}", st.decrypted, st.copied, st.failed, dst.display());
    Ok(())
}
