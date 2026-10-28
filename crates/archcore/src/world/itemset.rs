//! Commutateur d'équipement (AgpmItemSet 0x6A, fenêtre UI_SwitchEditor d'Archonia) : jeux d'équipement enregistrés et
//! remis d'un clic ou d'une touche. Archonia n'a pas de bonus de panoplie : porter un jeu ne fait que changer
//! l'équipement.
//!
//! ```text
//!   Paquet (drapeaux 1) : 0 op, 1 emplacement 0-99 (op 4 : actif, -1 aucun), 2 argument, 3 objets (CHAR[4000]).
//!   Emplacement = (profil × 10 + configuration) × 2 + jeu (0 A, 1 B), numéroté par le client.
//!   Objets : « E,<partie>,<identifiant> » séparés par « : », 10 au plus ; l'identifiant est le champ 26 d'AGPMITEM
//!   (l'IID, identifiant en base de l'objet), la partie n'est pas lue par le client.
//!   Entrée en jeu  ← op 3 (emplacement, 1) et op 5 (emplacement, objets) par emplacement rempli, puis op 4 (actif)
//!   Enregistrer    → op 0 (emplacement) : équipement porté    ← op 3, op 5
//!   Porter         → op 1 (emplacement)                       ← changements d'équipement (AGPMITEM), op 4
//!   Vider          → op 2 (emplacement)                       ← op 3 (emplacement, 0), op 5 (vide)
//!   Déplacer       → op 6 (emplacement, (de << 4) | vers)     ← op 5
//!   Retirer        → op 7 (emplacement, position)             ← op 5 (et op 3 à 0 s'il ne reste rien)
//! ```
//! Choix d'ArchCore (le serveur d'origine n'est pas connu) : gratuit, sans limite de niveau. Porter un jeu passe par
//! le même chemin que l'équipement manuel (`equipment::equip` : niveau, race, classe, sexe, monture, place pour ranger
//! ce qui est remplacé), objet par objet, sans message en cas de refus (le client n'en a pas) ; les objets qui ne sont
//! plus dans l'inventaire sont ignorés ; un emplacement vide retire tout l'équipement (« unequip everything » du
//! client). L'emplacement actif ne change qu'en portant un jeu (pas d'op 4 après un enregistrement). Déplacer vers une
//! position au-delà du dernier objet le met en fin de liste ; entre deux objets, ils sont permutés. Emplacements hors
//! de 0-99 et positions hors de 0-9 ignorés. En base : `character_item_set`, `characters.item_set_active`.

use super::items::Inventory;
use super::{WorldCtx, WorldState};
use crate::db::{col, Databases};
use crate::{debug, info};
use archcore_net::schema::{self, item_set_op};
use archcore_net::{build_packet, Fields, Packet};
use std::collections::BTreeMap;

/// Emplacements par personnage.
pub const SLOTS: i64 = 100;
/// Objets au plus dans un jeu (cases de la grille du client).
pub const MAX_ITEMS: usize = 10;

/// Jeux d'un personnage en jeu.
#[derive(Clone, Debug)]
pub struct ItemSets {
    /// Emplacement → objets (chaîne du paquet), emplacements remplis seulement.
    pub slots: BTreeMap<u8, String>,
    /// Emplacement porté en dernier, -1 aucun.
    pub active: i64,
}

impl Default for ItemSets {
    fn default() -> Self {
        ItemSets { slots: BTreeMap::new(), active: -1 }
    }
}

impl ItemSets {
    pub async fn load(db: &Databases, cid: u32) -> anyhow::Result<Self> {
        let rows = db.characters.rows("SELECT slot, items FROM character_item_set WHERE character_id = ? ORDER BY slot", (cid,)).await?;
        let slots = rows.iter().map(|r| (col::<u8>(r, "slot"), col::<String>(r, "items"))).filter(|(s, i)| i64::from(*s) < SLOTS && !i.is_empty()).collect();
        let active = db.characters.first("SELECT item_set_active FROM characters WHERE id = ?", (cid,)).await?.map(|r| col::<i64>(&r, "item_set_active")).unwrap_or(-1);
        Ok(ItemSets { slots, active: if (0..SLOTS).contains(&active) { active } else { -1 } })
    }
}

/// Jetons « E,… » d'une liste d'objets (les autres sont ignorés, comme le client).
pub fn tokens(items: &str) -> Vec<&str> {
    items.split(':').map(str::trim).filter(|t| t.starts_with('E')).take(MAX_ITEMS).collect()
}

/// Identifiant d'objet d'un jeton « E,<partie>,<identifiant> ».
pub fn token_item(token: &str) -> Option<u64> {
    let mut parts = token.split(',');
    (parts.next()? == "E").then_some(())?;
    parts.next()?.trim().parse::<i64>().ok()?;
    parts.next()?.trim().parse().ok()
}

/// Liste d'objets de l'équipement porté (par partie, 10 au plus).
pub fn capture(inv: &Inventory) -> String {
    inv.equipment.iter().take(MAX_ITEMS).map(|(part, i)| format!("E,{part},{}", i.iid)).collect::<Vec<_>>().join(":")
}

/// Op 6 : déplace le jeton `from` vers `to` (permutation, ou fin de liste au-delà du dernier) ; `None` si rien ne bouge.
pub fn moved(items: &str, from: usize, to: usize) -> Option<String> {
    let mut t = tokens(items);
    if from == to || from >= t.len() || to >= MAX_ITEMS {
        return None;
    }
    if to < t.len() {
        t.swap(from, to);
    } else {
        let tok = t.remove(from);
        t.push(tok);
    }
    Some(t.join(":"))
}

/// Op 7 : retire le jeton `pos` ; `None` s'il n'existe pas.
pub fn removed(items: &str, pos: usize) -> Option<String> {
    let mut t = tokens(items);
    (pos < t.len()).then(|| {
        t.remove(pos);
        t.join(":")
    })
}

fn packet(op: i64, slot: i64, arg: Option<i64>, items: Option<&str>) -> Vec<u8> {
    let s = &schema::ITEM_SET;
    build_packet(s, &Fields::new(s).with(0, op).with(1, slot).with_opt(2, arg).with_opt(3, items))
}

/// Op 3 puis op 5 d'un emplacement (rempli ou vidé).
fn content_packets(slot: u8, items: &str) -> [Vec<u8>; 2] {
    let slot = i64::from(slot);
    [packet(item_set_op::FLAG, slot, Some(i64::from(!items.is_empty())), None), packet(item_set_op::CONTENT, slot, Some(0), Some(items))]
}

/// Entrée en jeu : emplacements remplis (op 3, op 5), puis l'emplacement actif (op 4).
pub fn on_enter(st: &WorldState, sid: u32) {
    let Some(p) = st.players.get(&sid) else { return };
    for (slot, items) in &p.item_sets.slots {
        for pkt in content_packets(*slot, items) {
            p.send_raw(pkt);
        }
    }
    p.send_raw(packet(item_set_op::ACTIVE, p.item_sets.active, Some(0), None));
}

/// Enregistre un emplacement (vide : effacé) et le renvoie au client.
async fn store(ctx: &WorldCtx, st: &mut WorldState, sid: u32, slot: u8, items: String) -> anyhow::Result<()> {
    let p = st.players.get_mut(&sid).unwrap();
    let cid = p.ch.id;
    if items.is_empty() {
        ctx.db.characters.exec("DELETE FROM character_item_set WHERE character_id = ? AND slot = ?", (cid, slot)).await?;
        p.item_sets.slots.remove(&slot);
    } else {
        ctx.db
            .characters
            .exec("INSERT INTO character_item_set (character_id, slot, items) VALUES (?, ?, ?) ON DUPLICATE KEY UPDATE items = VALUES(items)", (cid, slot, items.clone()))
            .await?;
        p.item_sets.slots.insert(slot, items.clone());
    }
    for pkt in content_packets(slot, &items) {
        p.send_raw(pkt);
    }
    Ok(())
}

/// AgpmItemSet reçu.
pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some(player) = st.players.get(&sid) else { return Ok(()) };
    if !player.in_game {
        return Ok(());
    }
    let slot = p.int(1).unwrap_or(-1);
    if !(0..SLOTS).contains(&slot) {
        debug!("[commutateur] #{sid} emplacement {slot} ignoré");
        return Ok(());
    }
    let slot = slot as u8;
    let arg = (p.int(2).unwrap_or(0) as u8) as usize;
    let current = player.item_sets.slots.get(&slot).cloned().unwrap_or_default();
    let name = player.ch.name.clone();
    match p.int(0).unwrap_or(-1) {
        item_set_op::SAVE => {
            let items = capture(&player.inventory);
            info!("[commutateur] {name} enregistre l'emplacement {slot} : {}", if items.is_empty() { "(rien de porté)" } else { &items });
            store(ctx, st, sid, slot, items).await?;
        }
        item_set_op::EQUIP => wear(ctx, st, sid, slot).await?,
        item_set_op::CLEAR => {
            info!("[commutateur] {name} vide l'emplacement {slot}");
            store(ctx, st, sid, slot, String::new()).await?;
        }
        item_set_op::MOVE => {
            if let Some(items) = moved(&current, arg >> 4, arg & 0x0f) {
                store(ctx, st, sid, slot, items).await?;
            }
        }
        item_set_op::REMOVE => {
            if let Some(items) = removed(&current, arg) {
                store(ctx, st, sid, slot, items).await?;
            }
        }
        op => debug!("[commutateur] #{sid} op {op} ignorée"),
    }
    Ok(())
}

/// Op 1 : porte le jeu de l'emplacement (vide : tout retirer), puis l'annonce comme actif.
async fn wear(ctx: &WorldCtx, st: &mut WorldState, sid: u32, slot: u8) -> anyhow::Result<()> {
    let p = &st.players[&sid];
    if p.dead {
        return Ok(());
    }
    let (cid, name) = (p.ch.id, p.ch.name.clone());
    let (mut worn, mut missing) = (0, 0);
    match p.item_sets.slots.get(&slot).cloned() {
        None => {
            let parts: Vec<i64> = p.inventory.equipment.keys().copied().collect();
            for at in parts {
                if !super::equipment::unequip(ctx, st, sid, at, None, true).await? {
                    break;
                }
                worn += 1;
            }
            info!("[commutateur] {name} porte l'emplacement vide {slot} : {worn} objet(s) retiré(s)");
        }
        Some(items) => {
            for iid in tokens(&items).into_iter().filter_map(token_item).filter_map(|id| u32::try_from(id).ok()) {
                let p = &st.players[&sid];
                if p.inventory.equipment.values().any(|i| i.iid == iid) {
                    continue;
                }
                let in_trade = p.trade.as_ref().is_some_and(|t| t.items.contains_key(&iid));
                if !p.inventory.items.contains_key(&iid) || in_trade {
                    missing += 1;
                    continue;
                }
                if super::equipment::equip(ctx, st, sid, iid, true).await? {
                    worn += 1;
                }
            }
            info!("[commutateur] {name} porte l'emplacement {slot} : {worn} objet(s) mis{}", if missing > 0 { format!(", {missing} absent(s)") } else { String::new() });
        }
    }
    ctx.db.characters.exec("UPDATE characters SET item_set_active = ? WHERE id = ?", (i64::from(slot), cid)).await?;
    let p = st.players.get_mut(&sid).unwrap();
    p.item_sets.active = i64::from(slot);
    p.send_raw(packet(item_set_op::ACTIVE, i64::from(slot), Some(0), None));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::items::{InventoryItem, DURABILITY_FULL};
    use archcore_net::decode_packet;

    #[test]
    fn jetons() {
        let items = "E,2,10234:X,1,5:E,9,10240:E,12,10251";
        assert_eq!(tokens(items), vec!["E,2,10234", "E,9,10240", "E,12,10251"]);
        assert_eq!(token_item("E,9,10240"), Some(10240));
        assert_eq!(token_item("E,x,1"), None);
        assert_eq!(token_item("E,9"), None);
        assert!(tokens("").is_empty());
    }

    #[test]
    fn equipement_capture() {
        let mut inv = Inventory::default();
        for (part, iid) in [(10, 7), (2, 5)] {
            inv.equipment.insert(part, InventoryItem { iid, tid: 1, count: 1, grid: [part, 0, 0], conv: Default::default(), options: Vec::new(), durability: DURABILITY_FULL });
        }
        assert_eq!(capture(&inv), "E,2,5:E,10,7");
        assert_eq!(capture(&Inventory::default()), "");
    }

    #[test]
    fn deplacer_et_retirer() {
        let items = "E,2,1:E,3,2:E,10,3";
        assert_eq!(moved(items, 0, 2).as_deref(), Some("E,10,3:E,3,2:E,2,1"));
        assert_eq!(moved(items, 0, 7).as_deref(), Some("E,3,2:E,10,3:E,2,1"), "au-delà : en fin de liste");
        assert_eq!(moved(items, 4, 0), None);
        assert_eq!(moved(items, 1, 1), None);
        assert_eq!(removed(items, 1).as_deref(), Some("E,2,1:E,10,3"));
        assert_eq!(removed("E,2,1", 0).as_deref(), Some(""));
        assert_eq!(removed(items, 3), None);
    }

    #[test]
    fn paquets() {
        let [flag, content] = content_packets(42, "E,2,5");
        let f = decode_packet(&schema::ITEM_SET, &flag).unwrap();
        assert_eq!((f.int(0), f.int(1), f.int(2), f.bytes(3)), (Some(3), Some(42), Some(1), None));
        let c = decode_packet(&schema::ITEM_SET, &content).unwrap();
        assert_eq!((c.int(0), c.int(1), c.text(3).as_deref()), (Some(5), Some(42), Some("E,2,5")));
        assert_eq!(content.len(), 19 + 1 + 1 + 4 + 1 + 4000 + 1, "objets sur 4000 octets");
        let a = decode_packet(&schema::ITEM_SET, &packet(item_set_op::ACTIVE, -1, Some(0), None)).unwrap();
        assert_eq!((a.int(0), a.int(1)), (Some(4), Some(-1)));
    }
}
