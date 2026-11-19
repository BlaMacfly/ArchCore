//! AgpmScript (0x45, client de Webzen seulement) : lignes de la console de développement envoyées au serveur.
//!
//! Paquet `{op INT8, CID INT32, texte MEMORY_BLOCK}`. La console de développement du client (AgcmUIConsole,
//! interpréteur Lua) envoie une ligne en **op 0** quand elle est en mode « -- send command to server » (0x5c2840 →
//! 0x588100). Le module est partagé client/serveur : chez le serveur d'origine, l'op 0 passait par un rappel
//! d'autorisation puis l'interpréteur Lua (récepteur 0x587fa0). La console reçoit la réponse en **op 2** : texte découpé
//! par « ; », une ligne de la console par morceau (abonné 0x5c54c0). L'op 1 reçue n'a pas d'abonné. Archonia n'a pas
//! ce type. Ce n'est ni un événement ni un script de quête : un outil de développement.
//!
//! Choix d'ArchCore (pas d'interpréteur Lua) : la ligne d'un compte MJ est lue comme une commande MJ de discussion
//! (`gm.rs`, « / » facultatif), avec les mêmes niveaux et le même journal ; la réponse part en op 2, ses lignes jointes
//! par « ; ». Joueur sans droits, ops 1 et 2 venues du client : ignorés.

use super::gm;
use super::{World, WorldState};
use crate::{debug, info};
use archcore_net::schema;
use archcore_net::{build_packet, Fields, Packet};
use std::sync::Arc;

/// Ligne de la console (client → serveur).
pub const CONSOLE_LINE: i64 = 0;
/// Réponse affichée dans la console (serveur → client).
pub const CONSOLE_OUTPUT: i64 = 2;

/// Longueur au plus d'une réponse (le bloc mémoire du protocole a une longueur sur 16 bits).
pub const MAX_OUTPUT: usize = 4000;

/// Ligne de console → commande de `gm.rs` (« / » ajouté s'il manque), `None` si elle est vide.
pub fn command_line(text: &str) -> Option<String> {
    let t = text.trim().trim_end_matches(';').trim();
    if t.is_empty() {
        return None;
    }
    Some(if t.starts_with('/') { t.to_string() } else { format!("/{t}") })
}

/// Réponse de l'op 2 : lignes jointes par « ; » (le séparateur de la console), « ; » du texte remplacés par « , ».
pub fn output_text(reply: &str) -> String {
    let mut s = reply.lines().map(|l| l.replace(';', ",")).collect::<Vec<_>>().join(";");
    if s.len() > MAX_OUTPUT {
        let mut end = MAX_OUTPUT;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
    }
    s
}

pub fn output_packet(cid: u32, reply: &str) -> Vec<u8> {
    let s = &schema::SCRIPT;
    build_packet(s, &Fields::new(s).with(0, CONSOLE_OUTPUT).with(1, cid).with(2, output_text(reply).as_str()))
}

pub async fn on_packet(world: &Arc<World>, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some(player) = st.players.get(&sid) else { return Ok(()) };
    let opc = p.int(0).unwrap_or(-1);
    if opc != CONSOLE_LINE || player.gm_level <= 0 {
        debug!("[world] #{sid} AGPMSCRIPT op {opc} de {} ignorée", player.ch.name);
        return Ok(());
    }
    let cid = player.ch.id;
    let Some(line) = command_line(&p.text(2).unwrap_or_default()) else { return Ok(()) };
    info!("[mj] console de {} : {line}", player.ch.name);
    let reply = match gm::execute_quiet(world, st, sid, &line).await? {
        Some(Some(r)) => r,
        _ => format!("Commande inconnue : {line}"),
    };
    if let Some(p) = st.players.get(&sid) {
        p.send_raw(output_packet(cid, &reply));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ligne_et_reponse() {
        assert_eq!(command_line("  GetExp  ").as_deref(), Some("/GetExp"));
        assert_eq!(command_line("/level 50;").as_deref(), Some("/level 50"));
        assert_eq!(command_line(" ; "), None);
        assert_eq!(output_text("a\nb; c"), "a;b, c");
        let p = archcore_net::decode_packet(&schema::SCRIPT, &output_packet(9, "Taux : 1.0\nfin")).unwrap();
        assert_eq!((p.int(0), p.int(1), p.text(2).as_deref()), (Some(CONSOLE_OUTPUT), Some(9), Some("Taux : 1.0;fin")));
        assert!(output_text(&"é".repeat(3000)).len() <= MAX_OUTPUT);
    }
}
