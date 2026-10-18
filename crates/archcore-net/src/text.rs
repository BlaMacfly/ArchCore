//! Texte du protocole : le client parle Windows-1252 (accents français confirmés en jeu).

use encoding_rs::WINDOWS_1252;

/// Chaîne → octets Windows-1252 (caractères hors page de code remplacés par « ? »).
pub fn encode(s: &str) -> Vec<u8> {
    let (bytes, _, had_errors) = WINDOWS_1252.encode(s);
    if !had_errors {
        return bytes.into_owned();
    }
    s.chars()
        .map(|c| {
            let mut buf = [0u8; 4];
            let (b, _, err) = WINDOWS_1252.encode(c.encode_utf8(&mut buf));
            if err || b.len() != 1 { b'?' } else { b[0] }
        })
        .collect()
}

/// Octets Windows-1252 → chaîne.
pub fn decode(b: &[u8]) -> String {
    WINDOWS_1252.decode(b).0.into_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn accents_et_symboles() {
        assert_eq!(super::encode("é’"), vec![0xe9, 0x92]);
        assert_eq!(super::decode(&[0xe9, 0x92]), "é’");
        assert_eq!(super::encode("日"), b"?");
    }
}
