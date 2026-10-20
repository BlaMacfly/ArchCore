//! Sonde de diagnostic : journal détaillé de ce que chaque joueur reçoit et envoie, pour comprendre un
//! comportement du client sans lui poser de questions.
//!
//! Activée par `ARCHCORE_PROBE=1`. Écrit `data/probe-AAAAMMJJ-HHMMSS.log` (UTF-8, une ligne par événement, horodatée
//! à la milliseconde) :
//! ```text
//!   OUT #sid ADD|VIEW cid nom (x, y, z)        personnage montré (AGPMCHARACTER op 0, vue optimisée op 1)
//!   OUT #sid DEL cid op=2|3                    personnage retiré
//!   OUT #sid UPD cid statut=… état=…           mise à jour (op 1 avec statut d'action ou états spéciaux)
//!   OUT #sid MOVE cid (de) -> (vers) fl=…     déplacement relayé
//!   OUT #sid ACT attaquant -> cible r=…        coup (vue optimisée de l'action)
//!   IN  #sid MOVE (pos?) -> (dest) fl=…        déplacement demandé par le client
//!   IN  #sid ACT cible op=…                    attaque demandée
//!   IN  #sid CHAT texte                        message de discussion (repère tapé par le joueur)
//!   POS #sid nom (x, y, z) secteur vus=N       position suivie par le serveur, chaque seconde
//! ```

use archcore_net::{decode_mini, decode_packet, parse_header, schema, schema::mini, Packet, Vec3};
use std::collections::HashMap;
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

struct Probe {
    out: Mutex<std::io::BufWriter<std::fs::File>>,
    last: Mutex<HashMap<u32, Instant>>,
}

static PROBE: OnceLock<Option<Probe>> = OnceLock::new();

fn probe() -> Option<&'static Probe> {
    PROBE
        .get_or_init(|| {
            if std::env::var("ARCHCORE_PROBE").as_deref() != Ok("1") {
                return None;
            }
            let _ = std::fs::create_dir_all("data");
            let path = format!("data/probe-{}.log", chrono::Local::now().format("%Y%m%d-%H%M%S"));
            match std::fs::File::create(&path) {
                Ok(f) => {
                    crate::info!("sonde : journal détaillé dans {path}");
                    Some(Probe { out: Mutex::new(std::io::BufWriter::new(f)), last: Mutex::new(HashMap::new()) })
                }
                Err(e) => {
                    crate::warn!("sonde : {path} : {e}");
                    None
                }
            }
        })
        .as_ref()
}

pub fn enabled() -> bool {
    probe().is_some()
}

/// Écrit une ligne horodatée.
pub fn line(text: &str) {
    let Some(p) = probe() else { return };
    let mut out = p.out.lock().unwrap();
    let _ = writeln!(out, "{} {text}", chrono::Local::now().format("%H:%M:%S%.3f"));
    let _ = out.flush();
}

/// Vrai au plus une fois par seconde pour `key` (positions périodiques).
pub fn every_second(key: u32) -> bool {
    let Some(p) = probe() else { return false };
    let now = Instant::now();
    let mut last = p.last.lock().unwrap();
    match last.get(&key) {
        Some(t) if now.duration_since(*t).as_millis() < 1000 => false,
        _ => {
            last.insert(key, now);
            true
        }
    }
}

fn v(p: Option<Vec3>) -> String {
    match p {
        Some(p) => format!("({:.0}, {:.0}, {:.0})", p.x, p.y, p.z),
        None => "(-)".into(),
    }
}

fn cid(p: &Packet, i: usize) -> String {
    p.int(i).map(|c| format!("{:#x}", c)).unwrap_or_else(|| "-".into())
}

/// Paquet envoyé à la session `sid` du serveur de monde : personnages montrés, retirés, déplacés, coups.
pub fn outgoing(sid: u32, plain: &[u8]) {
    if !enabled() {
        return;
    }
    let Some(h) = parse_header(plain) else { return };
    let text = match h.ptype {
        t if t == schema::CHARACTER.id => {
            let Ok(p) = decode_packet(&schema::CHARACTER, plain) else { return };
            match p.int(0) {
                Some(0) => format!("ADD {} {} {}", cid(&p, 1), p.text(3).unwrap_or_default(), v(p.bytes(5).and_then(move_pos))),
                Some(op @ (2 | 3)) => format!("DEL {} op={op}", cid(&p, 1)),
                Some(1) if p.int(11).is_some() || p.int(17).is_some() => {
                    format!("UPD {} statut={} état={}", cid(&p, 1), p.int(11).map(|s| s.to_string()).unwrap_or("-".into()), p.int(17).map(|s| format!("{s:#x}")).unwrap_or("-".into()))
                }
                _ => return,
            }
        }
        t if t == schema::OPT_VIEW.id => {
            let Ok(p) = decode_packet(&schema::OPT_VIEW, plain) else { return };
            if p.int(0) != Some(1) {
                return;
            }
            let Some(view) = p.bytes(1).and_then(|b| decode_mini(&mini::OPT_VIEW_CHAR, b).ok()) else { return };
            format!("VIEW {} {} {} statut={}", view.int(0).map(|c| format!("{:#x}", c)).unwrap_or_default(), view.text(2).unwrap_or_default(), v(view.vec3(8)), view.int(3).unwrap_or(0))
        }
        t if t == schema::OPT_CHAR_MOVE.id => {
            let Ok(p) = decode_packet(&schema::OPT_CHAR_MOVE, plain) else { return };
            format!("MOVE {} {} -> {} fl={:#x}", cid(&p, 0), v(p.vec3(1)), v(p.vec3(2)), p.int(5).unwrap_or(0))
        }
        t if t == schema::OPT_CHAR_ACTION.id => {
            let Ok(p) = decode_packet(&schema::OPT_CHAR_ACTION, plain) else { return };
            format!("ACT {} -> {} r={}", cid(&p, 0), cid(&p, 1), p.int(2).unwrap_or(-1))
        }
        _ => return,
    };
    line(&format!("OUT #{sid} {text}"));
}

/// Position d'un mini « déplacement » (champ 5 d'AGPMCHARACTER).
fn move_pos(b: &[u8]) -> Option<Vec3> {
    decode_mini(&mini::CHAR_MOVE, b).ok()?.vec3(0)
}

/// Paquet reçu d'un client du serveur de monde : déplacements et attaques demandés.
pub fn incoming(sid: u32, ptype: u8, p: &Packet) {
    if !enabled() {
        return;
    }
    let text = if ptype == schema::OPT_CHAR_MOVE.id {
        format!("MOVE {} -> {} fl={:#x}", v(p.vec3(1)), v(p.vec3(2)), p.int(5).unwrap_or(0))
    } else if ptype == schema::OPT_CHAR_ACTION.id {
        format!("ACT {} op={}", cid(p, 1), p.int(6).unwrap_or(-1))
    } else if ptype == schema::CHATTING.id {
        format!("CHAT {}", p.text(6).unwrap_or_default())
    } else {
        return;
    };
    line(&format!("IN  #{sid} {text}"));
}
