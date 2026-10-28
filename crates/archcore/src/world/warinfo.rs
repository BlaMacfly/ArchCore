//! Tableau d'information des guerres (AgpmWarInfo 0x61, serveur → client ; fenêtre UI_WarInfo d'Archonia).
//!
//! ```text
//!   0x13 drapeaux (0x0F) · 0x14 op · 0x15 type de guerre · 0x16 étape · 0x17 paramètre · puis, selon l'op :
//!   op 0  étape : textes STEP MESSAGE et CENTER MESSAGE de warinfodata.txt (type, étape)
//!   op 2  compteur affiché (0x16)            op 3  message central numéro 0x16
//!   op 4, op 5  deux INT32 en 0x18 et 0x1C    op 6  effacer le tableau
//!   op 7  trois chaînes de 0x21 octets en 0x16, 0x37 et 0x58 : arguments « %s » de l'étape suivante
//! ```
//! Le client lit le paquet brut à ces décalages : on écrit donc toujours les quatre octets (drapeaux 0x0F), suivis des
//! données de l'op. Système commun à toutes les guerres (types `war_info_op` : siège 0, Battleground 1, Shrine 2,
//! Archlord 3) ; seuls les sièges s'en servent pour l'instant (`siege/`). Sens des ops 2, 4 et 5 (valeurs affichées) :
//! choix d'ArchCore, à vérifier à l'écran (voir `siege::war`).

use archcore_net::schema::{self, war_info_op};
use archcore_net::{build_packet, Fields};

/// Paquet des quatre octets, suivis de `extra` (bruts) avant le pied.
pub fn packet(op: i64, kind: i64, step: i64, param: i64, extra: &[u8]) -> Vec<u8> {
    let s = &schema::WAR_INFO;
    let mut out = build_packet(s, &Fields::new(s).with(0, op).with(1, kind).with(2, step).with(3, param));
    let tail = out.pop().unwrap_or(archcore_net::packet::GUARD_TAIL);
    out.extend_from_slice(extra);
    out.push(tail);
    let len = out.len() as u32;
    out[1..5].copy_from_slice(&len.to_le_bytes());
    out
}

/// Op 0 : étape `step` d'une guerre de type `kind`.
pub fn step(kind: i64, step: i64, param: i64) -> Vec<u8> {
    packet(war_info_op::STEP, kind, step, param, &[])
}

/// Op 2 : compteur.
pub fn counter(kind: i64, value: i64) -> Vec<u8> {
    packet(war_info_op::COUNTER, kind, value.clamp(0, 127), 0, &[])
}

/// Op 4 ou 5 : deux valeurs INT32 en 0x18 et 0x1C.
pub fn values(op: i64, kind: i64, a: i64, b: i64) -> Vec<u8> {
    let mut extra = Vec::with_capacity(8);
    extra.extend_from_slice(&(a as i32).to_le_bytes());
    extra.extend_from_slice(&(b as i32).to_le_bytes());
    packet(op, kind, 0, 0, &extra)
}

/// Op 6 : effacer le tableau.
pub fn clear(kind: i64) -> Vec<u8> {
    packet(war_info_op::CLEAR, kind, 0, 0, &[])
}

/// Op 7 : trois arguments « %s » (0x21 octets chacun, à partir de 0x16 : ils recouvrent l'étape et le paramètre).
pub fn args(kind: i64, texts: [&str; 3]) -> Vec<u8> {
    let mut b = vec![0u8; 3 * 0x21];
    for (i, t) in texts.iter().enumerate() {
        let enc = archcore_net::text::encode(t);
        let n = enc.len().min(0x20);
        b[i * 0x21..i * 0x21 + n].copy_from_slice(&enc[..n]);
    }
    // Les deux premiers octets de la première chaîne occupent les places de l'étape et du paramètre.
    let mut out = packet(war_info_op::ARGS, kind, b[0] as i8 as i64, b[1] as i8 as i64, &b[2..]);
    // L'encodage des INT8 conserve les octets tels quels.
    out[0x16] = b[0];
    out[0x17] = b[1];
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use archcore_net::{decode_packet, parse_header};

    #[test]
    fn paquets_bruts() {
        let p = step(war_info_op::SIEGE_WAR, 3, 1);
        assert_eq!((p.len(), p[0x13], p[0x14], p[0x15], p[0x16], p[0x17]), (0x19, 0x0f, 0, 0, 3, 1));
        assert_eq!(parse_header(&p).map(|h| (h.ptype, h.size)), Some((0x61, 0x19)));
        assert_eq!(decode_packet(&schema::WAR_INFO, &p).unwrap().int(2), Some(3));
        let v = values(war_info_op::VALUES_4, 0, 600, 3600);
        assert_eq!((v.len(), &v[0x18..0x1c], &v[0x1c..0x20]), (0x21, &600i32.to_le_bytes()[..], &3600i32.to_le_bytes()[..]));
        let a = args(0, ["SimGuild", "GmTest", "Inflame"]);
        assert_eq!(a.len(), 0x16 + 3 * 0x21 + 1);
        assert_eq!(&a[0x16..0x1e], b"SimGuild");
        assert_eq!(&a[0x37..0x3d], b"GmTest");
        assert_eq!(&a[0x58..0x5f], b"Inflame");
        assert_eq!((clear(1)[0x14], clear(1)[0x15], counter(0, 500)[0x16]), (6, 1, 127));
    }
}
