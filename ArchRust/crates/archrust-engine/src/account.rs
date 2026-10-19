//! Compte enregistré de l'écran de connexion (case « Enregistrer le compte »).
//!
//! Identifiant, mot de passe et personnage sont chiffrés avec DPAPI (`CryptProtectData`) : seul
//! le même utilisateur Windows, sur la même machine, peut relire le fichier
//! `saved-variables/compte.dat`. Rien n'est écrit en clair. Hors Windows, l'enregistrement est
//! indisponible.

use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};

/// Nom du fichier, dans le dossier des SavedVariables.
pub const FILE_NAME: &str = "compte.dat";
/// En-tête du texte chiffré, pour reconnaître un fichier valide.
const MAGIC: &str = "ARCHRUST-COMPTE 1";

/// Identifiants mémorisés.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedAccount {
    pub user: String,
    pub password: String,
    pub character: String,
}

impl SavedAccount {
    fn encode(&self) -> String {
        format!(
            "{MAGIC}\n{}\n{}\n{}\n",
            self.user, self.password, self.character
        )
    }

    fn decode(text: &str) -> Option<SavedAccount> {
        let mut lines = text.split('\n');
        if lines.next()? != MAGIC {
            return None;
        }
        Some(SavedAccount {
            user: lines.next()?.to_string(),
            password: lines.next()?.to_string(),
            character: lines.next()?.to_string(),
        })
    }
}

pub fn path(saved_dir: &Path) -> PathBuf {
    saved_dir.join(FILE_NAME)
}

/// Compte enregistré, s'il y en a un lisible.
pub fn load(saved_dir: &Path) -> Option<SavedAccount> {
    let bytes = std::fs::read(path(saved_dir)).ok()?;
    match unprotect(&bytes) {
        Ok(plain) => SavedAccount::decode(&String::from_utf8_lossy(&plain)),
        Err(e) => {
            log::warn!("{} illisible : {e:#}", path(saved_dir).display());
            None
        }
    }
}

/// Enregistre (chiffré) le compte.
pub fn save(saved_dir: &Path, account: &SavedAccount) -> Result<()> {
    if account.user.contains('\n') || account.password.contains('\n') {
        return Err(anyhow!(
            "saut de ligne dans l'identifiant ou le mot de passe"
        ));
    }
    std::fs::create_dir_all(saved_dir)?;
    let blob = protect(account.encode().as_bytes())?;
    std::fs::write(path(saved_dir), blob)?;
    Ok(())
}

/// Oublie le compte enregistré.
pub fn forget(saved_dir: &Path) {
    let p = path(saved_dir);
    if p.exists() {
        if let Err(e) = std::fs::remove_file(&p) {
            log::warn!("{} : {e}", p.display());
        }
    }
}

#[cfg(windows)]
mod dpapi {
    use anyhow::{anyhow, Result};
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    fn call(
        data: &[u8],
        f: impl FnOnce(*const CRYPT_INTEGER_BLOB, *mut CRYPT_INTEGER_BLOB) -> i32,
    ) -> Result<Vec<u8>> {
        let input = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        if f(&input, &mut output) == 0 {
            return Err(anyhow!("DPAPI : {}", std::io::Error::last_os_error()));
        }
        // SAFETY : en cas de succès, DPAPI alloue `cbData` octets à `pbData` (LocalAlloc).
        let bytes =
            unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
        unsafe { LocalFree(output.pbData as _) };
        Ok(bytes)
    }

    pub fn protect(data: &[u8]) -> Result<Vec<u8>> {
        call(data, |i, o| unsafe {
            CryptProtectData(
                i,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                o,
            )
        })
    }

    pub fn unprotect(data: &[u8]) -> Result<Vec<u8>> {
        call(data, |i, o| unsafe {
            CryptUnprotectData(
                i,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                o,
            )
        })
    }
}

#[cfg(windows)]
use dpapi::{protect, unprotect};

#[cfg(not(windows))]
fn protect(_: &[u8]) -> Result<Vec<u8>> {
    Err(anyhow!(
        "enregistrement du compte disponible sous Windows seulement"
    ))
}

#[cfg(not(windows))]
fn unprotect(_: &[u8]) -> Result<Vec<u8>> {
    Err(anyhow!(
        "enregistrement du compte disponible sous Windows seulement"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_is_encrypted() {
        let dir = std::env::temp_dir().join(format!("archrust-compte-{}", std::process::id()));
        let account = SavedAccount {
            user: "test".into(),
            password: "mot de passe très secret".into(),
            character: "Testeur".into(),
        };
        save(&dir, &account).unwrap();
        let raw = std::fs::read(path(&dir)).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("secret"));
        assert_eq!(load(&dir), Some(account));
        forget(&dir);
        assert_eq!(load(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
