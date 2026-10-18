//! Événements des PNJ (ApmEventManager) : dialogue.
//!
//! Le client demande l'ouverture d'un dialogue par AGPMEVENTNPCDIALOG (0x37) op 0, avec au champ 1 un
//! mini EVENT_BASE désignant l'événement (type de la source, CID du PNJ, fonction), au champ 2 son CID.
//! Le serveur accorde par l'op 1 en renvoyant les mêmes champs ; le texte du dialogue est dans le client.

use super::npcs::{event_function, Npc, NpcEvent, NpcRegistry};
use super::{WorldCtx, WorldState};
use crate::{info, warn};
use archcore_net::schema::{self, mini};
use archcore_net::{build_mini, decode_mini, Fields, Packet};

pub mod dialog_op {
    pub const REQUEST: i64 = 0;
    pub const GRANT: i64 = 1;
}

/// Distance maximale (unités du jeu, ~1 cm) entre le joueur et le PNJ pour lui parler ou commercer.
pub const MAX_NPC_DISTANCE: f64 = 1500.0;

/// Distance au sol (x, z) entre un PNJ et une position.
pub fn npc_distance(npc: &Npc, x: f64, z: f64) -> f64 {
    let dx = npc.pos.x as f64 - x;
    let dz = npc.pos.z as f64 - z;
    (dx * dx + dz * dz).sqrt()
}

/// Désigne l'événement visé : PNJ trouvé par CID (champ 1) ou par identifiant d'événement (champ 2).
pub fn resolve<'a>(npcs: &'a NpcRegistry, base: &Fields) -> Option<(&'a Npc, Option<&'a NpcEvent>)> {
    if let Some(cid) = base.int(1).filter(|&c| c != 0) {
        if let Some(npc) = npcs.get(cid as u32) {
            let f = base.int(3);
            return Some((npc, npc.events.iter().find(|e| Some(e.function) == f)));
        }
    }
    if let Some(id) = base.int(2).filter(|&i| i != 0) {
        for npc in &npcs.npcs {
            if let Some(ev) = npc.events.iter().find(|e| e.event_id == id) {
                return Some((npc, Some(ev)));
            }
        }
    }
    None
}

fn describe(base: &Fields) -> String {
    let v = |i: usize| base.int(i).map(|x| x.to_string()).unwrap_or("-".into());
    format!("source {}/{}, événement {}, fonction {}", v(0), v(1), v(2), v(3))
}

pub fn on_dialog(ctx: &WorldCtx, st: &WorldState, sid: u32, p: &Packet) {
    let Some(player) = st.players.get(&sid) else { return };
    if p.int(0) != Some(dialog_op::REQUEST) {
        warn!("[pnj] #{sid} AGPMEVENTNPCDIALOG op {:?} inattendu", p.int(0));
        return;
    }
    let base = p.bytes(1).and_then(|b| decode_mini(&mini::EVENT_BASE, b).ok()).unwrap_or_else(|| Fields::new(&mini::EVENT_BASE));
    let desc = describe(&base);
    let Some((npc, event)) = resolve(&ctx.npcs, &base) else {
        warn!("[pnj] #{sid} dialogue avec un PNJ inconnu ({desc})");
        return;
    };
    let ch = &player.ch;
    let dist = npc_distance(npc, ch.pos_x, ch.pos_z);
    if dist > MAX_NPC_DISTANCE {
        warn!("[pnj] #{sid} {} trop loin de {} ({dist:.0}) pour lui parler", ch.name, npc.name);
        return;
    }
    if let Some(e) = event.filter(|e| e.function != event_function::NPC_DIALOG) {
        info!("[pnj] {} → {} : fonction {} demandée par le dialogue ({desc})", ch.name, npc.name, e.function);
    }
    info!("[pnj] {} parle à {} ({desc})", ch.name, npc.name);
    // Accord : mêmes données que la demande, dans un mini reconstruit (préfixe de longueur compris).
    let grant_base = if p.bytes(1).is_some() { build_mini(&mini::EVENT_BASE, &base) } else { super::npcs::event_base(npc.cid, event_function::NPC_DIALOG) };
    let s = &schema::EVENT_NPC_DIALOG;
    player.send(s, Fields::new(s).with(0, dialog_op::GRANT).with(1, grant_base).with(2, p.int(2).unwrap_or(ch.id as i64)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::npcs::tests::darryl;

    #[test]
    fn resolution_par_cid_ou_par_evenement() {
        let reg = NpcRegistry::from_npcs(vec![darryl()]);
        let s = &mini::EVENT_BASE;
        let by_cid = Fields::new(s).with(0, 3).with(1, darryl().cid).with(3, 9);
        let (npc, ev) = resolve(&reg, &by_cid).unwrap();
        assert_eq!(npc.cid, darryl().cid);
        assert_eq!(ev.unwrap().param("Template"), Some("104"));
        let by_id = Fields::new(s).with(2, 0x4000_0011);
        assert_eq!(resolve(&reg, &by_id).unwrap().1.unwrap().function, 21);
        assert!(resolve(&reg, &Fields::new(s).with(1, 12345)).is_none());
    }
}
