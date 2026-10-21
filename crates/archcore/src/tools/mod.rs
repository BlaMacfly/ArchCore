//! Outils en ligne de commande d'ArchCore (base, import, déchiffrement, traduction, simulateur, relevés).

pub mod conf;
pub mod content;
pub mod db_check;
pub mod db_init;
pub mod decrypt;
pub mod i18n;
pub mod import;
pub mod re;
pub mod sim;
pub mod terrain;

use std::path::{Path, PathBuf};

/// Clé des tables ini du client (CryptoAPI : MD5("1111") → RC4).
pub const INI_KEY: &[u8] = b"1111";

/// Vrai si le tampon ressemble à du texte en clair (moins de 5 % d'octets de contrôle sur 256).
pub fn looks_plain(buf: &[u8]) -> bool {
    let n = buf.len().min(256);
    if n == 0 {
        return true;
    }
    let ctrl = buf[..n].iter().filter(|&&c| c < 0x20 && c != 0x09 && c != 0x0a && c != 0x0d).count();
    (ctrl as f64) / (n as f64) < 0.05
}

/// Fichier du client : (chiffré ?, contenu en clair).
pub fn load_ini(path: &Path) -> anyhow::Result<(bool, Vec<u8>)> {
    let raw = std::fs::read(path).map_err(|e| anyhow::anyhow!("{} : {e}", path.display()))?;
    if looks_plain(&raw) {
        Ok((false, raw))
    } else {
        Ok((true, archcore_net::rc4::capi_crypt(INI_KEY, &raw)))
    }
}

/// Octets → texte, un caractère par octet (comme « latin1 » de Node).
pub fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

/// Texte → octets Windows-1252 (latin1 + signes typographiques ; « ? » pour le reste).
pub fn to_cp1252(s: &str) -> Vec<u8> {
    s.chars()
        .map(|ch| {
            let code = ch as u32;
            if code < 0x100 {
                return code as u8;
            }
            match ch {
                '€' => 0x80,
                '‚' => 0x82,
                '„' => 0x84,
                '…' => 0x85,
                '‘' => 0x91,
                '’' => 0x92,
                '“' => 0x93,
                '”' => 0x94,
                '–' => 0x96,
                '—' => 0x97,
                'œ' => 0x9c,
                'Œ' => 0x8c,
                _ => b'?',
            }
        })
        .collect()
}

/// Dossier `Game` du client : argument, sinon `client.dataDir` de la configuration.
pub fn game_dir(arg: Option<&String>) -> anyhow::Result<PathBuf> {
    if let Some(a) = arg {
        return Ok(PathBuf::from(a));
    }
    let cfg = crate::config::load()?;
    Ok(cfg.root.join(&cfg.client.data_dir))
}

/// Découpe en lignes en acceptant \r\n et \n.
pub fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l))
}

#[cfg(test)]
mod tests {
    #[test]
    fn detection_et_encodages() {
        assert!(super::looks_plain(b"0=ModuleData1\r\n1=Name\r\n"));
        assert!(!super::looks_plain(&[1, 2, 3, 4, 5, 6, 7, 8]));
        assert_eq!(super::latin1(&[0x41, 0xe9, 0x92]), "Aé\u{92}");
        assert_eq!(super::to_cp1252("é’日"), vec![0xe9, 0x92, b'?']);
    }
}
