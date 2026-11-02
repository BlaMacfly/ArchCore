//! Événements des PNJ (ApmEventManager) : dialogue.
//!
//! Le client demande l'ouverture d'un dialogue par AGPMEVENTNPCDIALOG (0x37) op 0, avec au champ 1 un
//! mini EVENT_BASE désignant l'événement (type de la source, CID du PNJ, fonction), au champ 2 son CID.
//! Le serveur accorde par l'op 1 en renvoyant les mêmes champs ; le texte du dialogue est dans le client.

use super::npcs::{event_function, Npc, NpcEvent, NpcRegistry};
use super::{Player, WorldCtx, WorldState};
use crate::{info, warn};
use archcore_net::schema::{self, mini};
use archcore_net::{build_mini, decode_mini, Fields, Packet};
use std::time::Instant;

pub mod dialog_op {
    pub const REQUEST: i64 = 0;
    pub const GRANT: i64 = 1;
}

/// Portées d'usage des fonctions de PNJ et d'objet : distance au sol (unités du jeu, ~1 cm) que vérifie le code
/// partagé d'Archonia à la réception de chaque module, par son contrôle de portée d'ApmEventManager
/// (`FUN_140304580`, appelé avec une portée fixe par module).
pub mod range {
    /// Dialogue (AgpmEventNPCDialog, `FUN_140289730`).
    pub const DIALOG: f64 = 600.0;
    /// Boutique : son module (AgpmEventNPCTrade) ne contrôle aucune portée ; elle s'ouvre depuis le dialogue, dont
    /// elle garde la portée.
    pub const NPC_TRADE: f64 = DIALOG;
    /// Commandant de siège (AgpmEventSiegeWarNPC, `FUN_140285090`).
    pub const SIEGE_WAR_NPC: f64 = 600.0;
    /// Banque (AgpmEventBank, `FUN_1402b2810`).
    pub const BANK: f64 = 800.0;
    /// Conversion chez le forgeron (AgpmEventItemConvert, `FUN_14029f1f0`).
    pub const ITEM_CONVERT: f64 = 800.0;
    /// Maître des compétences (AgpmEventSkillMaster, `FUN_1402b7550`).
    pub const SKILL_MASTER: f64 = 800.0;
    /// Guilde et entrepôt de guilde (AgpmEventGuild, `FUN_14027b630`).
    pub const GUILD: f64 = 800.0;
    /// Coiffeur (AgpmEventCharCustomize, `FUN_14022d200`).
    pub const CHAR_CUSTOMIZE: f64 = 800.0;
    /// Réparation (AgpmEventItemRepair, `FUN_1402b4020`).
    pub const ITEM_REPAIR: f64 = 1600.0;
    /// Maison des ventes (AgpmAuction, `FUN_140269a70`).
    pub const AUCTION: f64 = 1600.0;
    /// Quêtes (AgpmEventQuest, `FUN_1402a56b0` : 1600 au plus).
    pub const QUEST: f64 = 1600.0;
    /// Production (AgpmEventProduct, `FUN_1402d05a0`).
    pub const PRODUCT: f64 = 1600.0;
    /// Rémission (AgpmRemission, `FUN_140276e50`).
    pub const REMISSION: f64 = 1600.0;
    /// Tableau des avis de recherche (AgpmWantedCriminal, `FUN_14028a700`).
    pub const WANTED_CRIMINAL: f64 = 1600.0;
    /// Intendant des taxes (AgpmTax, `FUN_14026c7b0`).
    pub const TAX: f64 = 1600.0;
    /// Portails (AgpmEventTeleport, `FUN_1402e8780`).
    pub const TELEPORT: f64 = 1600.0;
    /// Sphères d'alchimie (AgpmEventRefinery, `FUN_14029d820`).
    pub const REFINERY: f64 = 1600.0;
}

/// Marge ajoutée aux portées (choix d'ArchCore) : positions arrondies du client et marche suivie par le serveur.
pub const RANGE_TOLERANCE: f64 = 50.0;

/// Distance au sol (x, z) entre un PNJ et une position.
pub fn npc_distance(npc: &Npc, x: f64, z: f64) -> f64 {
    let dx = npc.pos.x as f64 - x;
    let dz = npc.pos.z as f64 - z;
    (dx * dx + dz * dz).sqrt()
}

/// Le joueur est à portée `range` du PNJ : position suivie à l'instant (marche en cours comprise), plus la marge.
pub fn in_range(npc: &Npc, p: &Player, range: f64) -> bool {
    let at = super::presence::position_now(p, Instant::now());
    npc_distance(npc, at.x as f64, at.z as f64) <= range + RANGE_TOLERANCE
}

/// Le joueur est resté à moins de `range` (plus la marge) du point (x, z) : fenêtre ouverte à distance par un objet
/// (`/safekeeper`, `/merchant`, `consumables.rs`), valable tant qu'il ne s'éloigne pas de l'endroit de l'ouverture.
pub fn near_point(p: &Player, at: (f64, f64), range: f64) -> bool {
    let now = super::presence::position_now(p, Instant::now());
    let (dx, dz) = (now.x as f64 - at.0, now.z as f64 - at.1);
    (dx * dx + dz * dz).sqrt() <= range + RANGE_TOLERANCE
}

/// PNJ le plus proche du joueur, n'importe où, qui a la fonction d'événement `function` (`/safekeeper`, `/merchant`).
pub fn nearest_with<'a>(npcs: &'a NpcRegistry, p: &Player, function: i64) -> Option<&'a Npc> {
    let at = super::presence::position_now(p, Instant::now());
    npcs.npcs
        .iter()
        .filter(|n| n.events.iter().any(|e| e.function == function))
        .min_by(|a, b| npc_distance(a, at.x as f64, at.z as f64).total_cmp(&npc_distance(b, at.x as f64, at.z as f64)))
}

/// PNJ à portée `range` du joueur (même mesure que `in_range`).
pub fn npcs_in_range<'a>(npcs: &'a NpcRegistry, p: &Player, range: f64) -> impl Iterator<Item = &'a Npc> + 'a {
    npcs.near(super::presence::position_now(p, Instant::now()), range + RANGE_TOLERANCE)
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
    // Sous métamorphose (sauf l'ArchLord) : aucun dialogue, sans réponse, comme le client (morph.rs).
    if super::morph::restricted(st, sid) {
        return;
    }
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
    if !in_range(npc, player, range::DIALOG) {
        warn!("[pnj] #{sid} {} trop loin de {} ({:.0}) pour lui parler", ch.name, npc.name, npc_distance(npc, ch.pos_x, ch.pos_z));
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
