//! Capacité `language/1` des extensions ArchRust ↔ ArchCore (version 1.1) : codes de langue et
//! encodage des champs 19 (identifiants demandés) et 20 (textes rendus) du paquet 0xAC.
//!
//! Partagé par le serveur ArchCore et le client ArchRust, qui dépend de cette crate.

/// Longueur maximale d'un code de langue.
pub const MAX_LANGUAGE_LEN: usize = 16;

/// Normalise un code BCP 47 : langue en minuscules, région de deux lettres en majuscules
/// (`FR-fr` → `fr-FR`, `pt_br` → `pt-BR`). `None` si le code n'a pas la forme attendue
/// (2 ou 3 lettres, puis des sous-étiquettes alphanumériques de 2 à 8 caractères).
pub fn normalize_language(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > MAX_LANGUAGE_LEN {
        return None;
    }
    let mut out = String::with_capacity(raw.len());
    for (i, part) in raw.split(['-', '_']).enumerate() {
        if i == 0 {
            if !(2..=3).contains(&part.len()) || !part.bytes().all(|b| b.is_ascii_alphabetic()) {
                return None;
            }
            out.push_str(&part.to_ascii_lowercase());
        } else {
            if !(2..=8).contains(&part.len()) || !part.bytes().all(|b| b.is_ascii_alphanumeric()) {
                return None;
            }
            out.push('-');
            if part.len() == 2 {
                out.push_str(&part.to_ascii_uppercase());
            } else if part.len() == 4 && part.bytes().all(|b| b.is_ascii_alphabetic()) {
                // Écriture (`Hans`) : première lettre en majuscule.
                let (h, t) = part.split_at(1);
                out.push_str(&h.to_ascii_uppercase());
                out.push_str(&t.to_ascii_lowercase());
            } else {
                out.push_str(&part.to_ascii_lowercase());
            }
        }
    }
    Some(out)
}

/// Langue de base d'un code (`fr-CA` → `fr`).
pub fn base_language(code: &str) -> &str {
    code.split('-').next().unwrap_or(code)
}

/// Champ 19 : identifiants UINT32 petit-boutistes à la suite.
pub fn encode_ids(ids: &[u32]) -> Vec<u8> {
    ids.iter().flat_map(|i| i.to_le_bytes()).collect()
}

pub fn decode_ids(bytes: &[u8]) -> Vec<u32> {
    bytes.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

/// Champ 20 : pour chaque texte, UINT32 identifiant, UINT16 longueur en octets, puis l'UTF-8.
/// Un texte de plus de 65 535 octets est tronqué à une frontière de caractère.
pub fn encode_texts<'a>(texts: impl IntoIterator<Item = (u32, &'a str)>) -> Vec<u8> {
    let mut out = Vec::new();
    for (id, text) in texts {
        let mut end = text.len().min(u16::MAX as usize);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        out.extend(id.to_le_bytes());
        out.extend((end as u16).to_le_bytes());
        out.extend(&text.as_bytes()[..end]);
    }
    out
}

/// Lit le champ 20 ; une fin tronquée est ignorée.
pub fn decode_texts(bytes: &[u8]) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos + 6 <= bytes.len() {
        let id = u32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]);
        let len = u16::from_le_bytes([bytes[pos + 4], bytes[pos + 5]]) as usize;
        pos += 6;
        if pos + len > bytes.len() {
            break;
        }
        out.push((id, String::from_utf8_lossy(&bytes[pos..pos + len]).into_owned()));
        pos += len;
    }
    out
}

/// Liste de langues du champ 17 (`fr;en;de`).
pub fn decode_language_list(raw: &str) -> Vec<String> {
    raw.split(';').filter_map(normalize_language).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_de_langue() {
        assert_eq!(normalize_language(" FR-fr ").as_deref(), Some("fr-FR"));
        assert_eq!(normalize_language("pt_br").as_deref(), Some("pt-BR"));
        assert_eq!(normalize_language("zh-hans-cn").as_deref(), Some("zh-Hans-CN"));
        assert_eq!(normalize_language("en").as_deref(), Some("en"));
        assert!(normalize_language("").is_none());
        assert!(normalize_language("f").is_none());
        assert!(normalize_language("fr-").is_none());
        assert!(normalize_language("français").is_none());
        assert!(normalize_language("abcdefghijklmnopq").is_none());
        assert_eq!(base_language("fr-CA"), "fr");
        assert_eq!(base_language("de"), "de");
        assert_eq!(decode_language_list("fr;en; de ;bad!"), vec!["fr", "en", "de"]);
    }

    #[test]
    fn identifiants_et_textes() {
        let ids = encode_ids(&[1, 0x1234_5678, u32::MAX]);
        assert_eq!(ids.len(), 12);
        assert_eq!(decode_ids(&ids), vec![1, 0x1234_5678, u32::MAX]);
        assert_eq!(decode_ids(&ids[..7]), vec![1]);
        let texts = encode_texts([(7, "Épée"), (8, ""), (9, "日本語")]);
        assert_eq!(decode_texts(&texts), vec![(7, "Épée".to_string()), (8, String::new()), (9, "日本語".to_string())]);
        // Fin tronquée ignorée.
        assert_eq!(decode_texts(&texts[..texts.len() - 2]).len(), 2);
        // Troncature à 65 535 octets sur une frontière de caractère.
        let long = "é".repeat(40_000);
        let one = decode_texts(&encode_texts([(1, long.as_str())]));
        assert_eq!(one.len(), 1);
        assert!(one[0].1.len() <= u16::MAX as usize && one[0].1.chars().all(|c| c == 'é'));
    }
}
