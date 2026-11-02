//! Statut d'interface du joueur (AgpmUIStatus, 0x14) : barre de raccourcis, potions de vie et de mana de la barre,
//! seuils de leur prise automatique (les deux curseurs posés sur les jauges de PV et de PM de `UI_CharInfo`, en
//! pourcentage, 0 : désactivé) et affichage du casque. Le client les reçoit à l'entrée en jeu (op 0) et renvoie chaque
//! changement (op 1 ; op 3 pour le casque), sans attendre de réponse. C'est le client qui boit la potion quand une jauge
//! passe sous son seuil ; le serveur garde seulement les réglages.
//!
//! Barre de raccourcis (épisode 8) : 5 couches de 10 cases, index = couche × 10 + colonne (0-49). Chaque case est un
//! mini `AGPMSKILL_BASE` (type de base, identifiant) : 5 objet (IID), 7 compétence (identifiant d'instance de la
//! compétence possédée) ; le client n'affiche ni n'envoie d'autre type.
//! ```text
//!   op 0 (serveur → client, après les objets et les compétences) : 1 CID, 2 les 50 minis (vide : case vide),
//!        6/7 potions, 8 casque, 9/10 seuils
//!   op 1 (client → serveur) : 3 case et 4 mini (absent : case vidée) ; 6/7 potions, 9/10 seuils ; rien n'est renvoyé
//! ```
//! Une case posée retire la même base des autres cases de sa rangée de 10, comme le client. Le serveur garde le TID
//! (table `character_quickbelt`) : une compétence est retrouvée par son TID à l'entrée en jeu (son identifiant
//! d'instance change) ; un objet disparu (pile consommée) est remplacé par un autre de même TID, sinon la case est
//! envoyée vide (la ligne reste : elle resservira quand le joueur aura de nouveau l'objet). Un objet ou une compétence
//! que le joueur n'a pas vide la case. L'op 2 (barre encodée en texte, champs 5 et 11) n'est pas employée : aucun
//! abonné dans le client.

use super::items::Inventory;
use super::skills::PlayerSkills;
use super::WorldState;
use crate::db::{col, Databases};
use crate::{debug, info};
use archcore_net::schema::{self, mini};
use archcore_net::{build_mini, decode_mini, Fields, Packet, Value};

pub mod op {
    pub const ADD: i64 = 0;
    pub const UPDATE_ITEM: i64 = 1;
    pub const VIEW_HELMET: i64 = 3;
}

/// Cases de la barre : 5 couches × 10.
pub const BELT_SLOTS: usize = 50;
/// Cases d'une rangée (couche).
pub const ROW: usize = 10;

/// Types de base d'`AGPMSKILL_BASE` gardés dans la barre.
pub mod base_type {
    /// Objet : IID.
    pub const ITEM: u8 = 5;
    /// Compétence : identifiant d'instance.
    pub const SKILL: u8 = 7;
}

/// Contenu d'une case gardée : type de base, TID de l'objet ou de la compétence, IID d'un objet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BeltSlot {
    pub base_type: u8,
    pub tid: u32,
    pub item_id: Option<u32>,
}

/// Réglages gardés pour un personnage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiStatus {
    pub hp_potion_tid: i32,
    pub mp_potion_tid: i32,
    pub view_helmet: bool,
    /// Seuils de prise automatique, en pourcentage des PV et PM maximum (0 à 100).
    pub auto_hp: i8,
    pub auto_mp: i8,
    /// Barre de raccourcis.
    pub belt: [Option<BeltSlot>; BELT_SLOTS],
}

impl Default for UiStatus {
    fn default() -> Self {
        Self { hp_potion_tid: 0, mp_potion_tid: 0, view_helmet: true, auto_hp: 0, auto_mp: 0, belt: [None; BELT_SLOTS] }
    }
}

/// Objet de l'inventaire, de l'inventaire Cash, du sac du familier ou de l'équipement : son TID.
fn item_tid(inv: &Inventory, iid: u32) -> Option<u32> {
    inv.get_usable(iid).or_else(|| inv.equipment.values().find(|i| i.iid == iid)).map(|i| i.tid)
}

/// Premier objet de TID `tid` que le joueur a (inventaire, Cash, sac du familier, équipement).
fn item_of_tid(inv: &Inventory, tid: u32) -> Option<u32> {
    inv.items.values().chain(inv.cash.values()).chain(inv.sub.values()).chain(inv.equipment.values()).find(|i| i.tid == tid).map(|i| i.iid)
}

/// Pose `slot` sur la case `index` (`None` : vidée) ; la même base est d'abord retirée des autres cases de la rangée.
/// Rend les cases changées.
pub fn place(belt: &mut [Option<BeltSlot>; BELT_SLOTS], index: usize, slot: Option<BeltSlot>) -> Vec<usize> {
    if index >= BELT_SLOTS {
        return Vec::new();
    }
    let mut changed = Vec::new();
    if let Some(s) = slot {
        let row = index / ROW * ROW;
        for i in (row..row + ROW).filter(|i| *i != index) {
            if belt[i] == Some(s) {
                belt[i] = None;
                changed.push(i);
            }
        }
    }
    if belt[index] != slot {
        belt[index] = slot;
        changed.push(index);
    }
    changed
}

/// Mini `AGPMSKILL_BASE` d'une case (sans contenu : mini vide).
pub fn base_mini(base: Option<(u8, u32)>) -> Vec<u8> {
    let s = &mini::SKILL_BASE;
    match base {
        Some((kind, id)) => build_mini(s, &Fields::new(s).with(0, kind).with(1, id)),
        None => build_mini(s, &Fields::new(s)),
    }
}

/// Contenu envoyé d'une case : identifiant de session de la compétence de même TID, IID de l'objet.
fn resolve(slot: &BeltSlot, inv: &Inventory, skills: &PlayerSkills) -> Option<(u8, u32)> {
    match slot.base_type {
        base_type::SKILL => skills.by_tid(slot.tid).map(|s| (base_type::SKILL, s.id)),
        base_type::ITEM => slot.item_id.filter(|iid| item_tid(inv, *iid) == Some(slot.tid)).map(|iid| (base_type::ITEM, iid)),
        _ => None,
    }
}

impl UiStatus {
    /// Réglages et barre enregistrés ; un objet de la barre qui n'est plus là est remplacé par un autre de même TID
    /// (enregistré aussitôt).
    pub async fn load(db: &Databases, character_id: u32, inv: &Inventory) -> anyhow::Result<Self> {
        let row = db
            .characters
            .first("SELECT hp_potion_tid, mp_potion_tid, view_helmet, auto_hp, auto_mp FROM character_ui_status WHERE character_id = ?", (character_id,))
            .await?;
        let mut st = row.map_or_else(Self::default, |r| Self {
            hp_potion_tid: col(&r, "hp_potion_tid"),
            mp_potion_tid: col(&r, "mp_potion_tid"),
            view_helmet: col::<i32>(&r, "view_helmet") != 0,
            auto_hp: col(&r, "auto_hp"),
            auto_mp: col(&r, "auto_mp"),
            belt: [None; BELT_SLOTS],
        });
        let rows = db.characters.rows("SELECT slot, base_type, tid, item_id FROM character_quickbelt WHERE character_id = ?", (character_id,)).await?;
        for r in &rows {
            let index = col::<u32>(r, "slot") as usize;
            if index >= BELT_SLOTS {
                continue;
            }
            let mut slot = BeltSlot { base_type: col(r, "base_type"), tid: col(r, "tid"), item_id: col(r, "item_id") };
            if slot.base_type == base_type::ITEM && slot.item_id.and_then(|iid| item_tid(inv, iid)) != Some(slot.tid) {
                if let Some(other) = item_of_tid(inv, slot.tid) {
                    slot.item_id = Some(other);
                    db.characters.exec("UPDATE character_quickbelt SET item_id = ? WHERE character_id = ? AND slot = ?", (other, character_id, index as u32)).await?;
                }
            }
            st.belt[index] = Some(slot);
        }
        Ok(st)
    }

    async fn save(&self, db: &Databases, character_id: u32) -> anyhow::Result<()> {
        db.characters
            .exec(
                "INSERT INTO character_ui_status (character_id, hp_potion_tid, mp_potion_tid, view_helmet, auto_hp, auto_mp) VALUES (?, ?, ?, ?, ?, ?) \
                 ON DUPLICATE KEY UPDATE hp_potion_tid = VALUES(hp_potion_tid), mp_potion_tid = VALUES(mp_potion_tid), view_helmet = VALUES(view_helmet), auto_hp = VALUES(auto_hp), auto_mp = VALUES(auto_mp)",
                (character_id, self.hp_potion_tid, self.mp_potion_tid, self.view_helmet as i32, self.auto_hp, self.auto_mp),
            )
            .await?;
        Ok(())
    }

    /// Applique les réglages d'un paquet op 1 ou op 3 (pas la barre) ; seuls les champs présents changent. Rend `true`
    /// si un réglage a changé.
    pub fn apply(&mut self, p: &Packet) -> bool {
        let before = *self;
        match p.int(0).unwrap_or(-1) {
            op::UPDATE_ITEM => {
                if let Some(t) = p.int(6) {
                    self.hp_potion_tid = t as i32;
                }
                if let Some(t) = p.int(7) {
                    self.mp_potion_tid = t as i32;
                }
                if let Some(v) = p.int(9) {
                    self.auto_hp = v.clamp(0, 100) as i8;
                }
                if let Some(v) = p.int(10) {
                    self.auto_mp = v.clamp(0, 100) as i8;
                }
            }
            op::VIEW_HELMET => {
                if let Some(v) = p.int(8) {
                    self.view_helmet = v != 0;
                }
            }
            _ => {}
        }
        *self != before
    }

    /// Paquet op 0 envoyé à l'entrée en jeu : les 50 cases (compétences et objets que le joueur a), puis les réglages.
    pub fn add_fields(&self, cid: u32, inv: &Inventory, skills: &PlayerSkills) -> Fields {
        let s = &schema::UI_STATUS;
        let belt: Vec<Value> = self.belt.iter().map(|slot| Value::Bytes(base_mini(slot.as_ref().and_then(|b| resolve(b, inv, skills))))).collect();
        Fields::new(s)
            .with(0, op::ADD)
            .with(1, cid)
            .with(2, belt)
            .with(6, self.hp_potion_tid)
            .with(7, self.mp_potion_tid)
            .with(8, self.view_helmet as i64)
            .with(9, self.auto_hp as i64)
            .with(10, self.auto_mp as i64)
    }
}

/// Case d'une op 1 : (index, contenu lu) ; `None` sans case (potions ou seuils seuls) ou hors des 50.
pub fn slot_of(p: &Packet) -> Option<(usize, Option<(u8, u32)>)> {
    let index = p.int(3).filter(|i| (0..BELT_SLOTS as i64).contains(i))? as usize;
    let base = p.bytes(4).and_then(|b| decode_mini(&mini::SKILL_BASE, b).ok()).and_then(|m| Some((m.int(0)? as u8, m.int(1)? as u32))).filter(|(_, id)| *id != 0);
    Some((index, base))
}

/// Contenu gardé d'une base reçue : objet ou compétence que le joueur a, sinon `None` (case vidée).
pub fn stored(base: Option<(u8, u32)>, inv: &Inventory, skills: &PlayerSkills) -> Option<BeltSlot> {
    match base? {
        (base_type::ITEM, iid) => item_tid(inv, iid).map(|tid| BeltSlot { base_type: base_type::ITEM, tid, item_id: Some(iid) }),
        (base_type::SKILL, id) => skills.by_id(id).map(|s| BeltSlot { base_type: base_type::SKILL, tid: s.tid, item_id: None }),
        _ => None,
    }
}

/// Entrée en jeu : barre et réglages envoyés au client.
pub fn on_enter(st: &WorldState, sid: u32) {
    let Some(p) = st.players.get(&sid) else { return };
    p.send(&schema::UI_STATUS, p.ui_status.add_fields(p.ch.id, &p.inventory, &p.skills));
}

/// Changement envoyé par le client : gardé en base, rien n'est renvoyé.
pub async fn on_packet(ctx: &super::WorldCtx, st: &mut WorldState, sid: u32, packet: &Packet) -> anyhow::Result<()> {
    let Some(p) = st.players.get_mut(&sid) else { return Ok(()) };
    if packet.int(1).is_some_and(|c| c != p.ch.id as i64) {
        return Ok(());
    }
    let (cid, name) = (p.ch.id, p.ch.name.clone());
    if packet.int(0) == Some(op::UPDATE_ITEM) {
        if let Some((index, base)) = slot_of(packet) {
            let slot = stored(base, &p.inventory, &p.skills);
            if base.is_some() && slot.is_none() {
                debug!("[interface] {name} : case {index}, base {base:?} inconnue, vidée");
            }
            let changed = place(&mut p.ui_status.belt, index, slot);
            let stmts: Vec<(&str, mysql_async::Params)> = changed
                .iter()
                .map(|&i| match p.ui_status.belt[i] {
                    Some(b) => (
                        "REPLACE INTO character_quickbelt (character_id, slot, base_type, tid, item_id) VALUES (?, ?, ?, ?, ?)",
                        (cid, i as u32, b.base_type, b.tid, b.item_id).into(),
                    ),
                    None => ("DELETE FROM character_quickbelt WHERE character_id = ? AND slot = ?", (cid, i as u32).into()),
                })
                .collect();
            if !stmts.is_empty() {
                ctx.db.characters.atomic(stmts).await?;
                debug!("[interface] {name} : barre, case {index} = {slot:?} ({} case(s) changée(s))", changed.len());
            }
        }
    }
    let p = st.players.get_mut(&sid).unwrap();
    if !p.ui_status.apply(packet) {
        return Ok(());
    }
    let status = p.ui_status;
    info!("[interface] {name} : potions {}/{}, seuils {} % / {} %, casque {}", status.hp_potion_tid, status.mp_potion_tid, status.auto_hp, status.auto_mp, status.view_helmet);
    status.save(&ctx.db, cid).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use archcore_net::{build_packet, decode_packet};

    #[test]
    fn reglages_du_statut_d_interface() {
        let s = &schema::UI_STATUS;
        let mut st = UiStatus::default();
        let p = decode_packet(s, &build_packet(s, &Fields::new(s).with(0, op::UPDATE_ITEM).with(1, 7).with(6, 190).with(9, 40))).unwrap();
        assert!(st.apply(&p));
        assert_eq!((st.hp_potion_tid, st.mp_potion_tid, st.auto_hp, st.auto_mp), (190, 0, 40, 0), "seuls les champs présents changent");
        let p = decode_packet(s, &build_packet(s, &Fields::new(s).with(0, op::UPDATE_ITEM).with(1, 7).with(9, 0))).unwrap();
        assert!(st.apply(&p));
        assert_eq!(st.auto_hp, 0, "seuil remis à 0 : désactivé");
        let p = decode_packet(s, &build_packet(s, &Fields::new(s).with(0, op::VIEW_HELMET).with(1, 7).with(8, 0))).unwrap();
        assert!(st.apply(&p) && !st.view_helmet);
        let add = decode_packet(s, &build_packet(s, &st.add_fields(7, &Inventory::default(), &PlayerSkills::default()))).unwrap();
        assert_eq!((add.int(0), add.int(1), add.int(6), add.int(8)), (Some(op::ADD), Some(7), Some(190), Some(0)));
    }

    #[test]
    fn barre_de_raccourcis() {
        use super::super::items::InventoryItem;
        use super::super::skills::Owned;
        let s = &schema::UI_STATUS;
        // Potion d'IID 900 (TID 190), compétence de TID 12 (identifiant de session 3).
        let mut inv = Inventory::default();
        let potion = InventoryItem { iid: 900, tid: 190, count: 5, grid: [0, 0, 0], conv: Default::default(), options: Vec::new(), durability: -1 };
        inv.items.insert(900, potion);
        let skills = PlayerSkills { list: vec![Owned { id: 3, tid: 12, level: 1, ready_at: None, exp: 0 }], ..Default::default() };
        // op 1 reçue : case 42, compétence 3.
        let pose = |index: i64, kind: u8, id: u32| {
            let raw = build_packet(s, &Fields::new(s).with(0, op::UPDATE_ITEM).with(1, 7).with(3, index).with(4, base_mini(Some((kind, id)))));
            decode_packet(s, &raw).unwrap()
        };
        let p = pose(42, base_type::SKILL, 3);
        assert_eq!(slot_of(&p), Some((42, Some((7, 3)))));
        let skill = stored(slot_of(&p).unwrap().1, &inv, &skills);
        assert_eq!(skill, Some(BeltSlot { base_type: 7, tid: 12, item_id: None }), "gardée par son TID");
        assert_eq!(stored(Some((7, 99)), &inv, &skills), None, "compétence inconnue");
        assert_eq!(stored(Some((5, 900)), &inv, &skills), Some(BeltSlot { base_type: 5, tid: 190, item_id: Some(900) }));
        assert_eq!(stored(Some((6, 190)), &inv, &skills), None, "type non gardé");
        // Retrait : case sans mini.
        let raw = build_packet(s, &Fields::new(s).with(0, op::UPDATE_ITEM).with(1, 7).with(3, 5));
        assert_eq!(slot_of(&decode_packet(s, &raw).unwrap()), Some((5, None)));
        let raw = build_packet(s, &Fields::new(s).with(0, op::UPDATE_ITEM).with(1, 7).with(6, 190));
        assert_eq!(slot_of(&decode_packet(s, &raw).unwrap()), None, "potion seule");
        assert_eq!(slot_of(&pose(50, 7, 3)), None, "hors des 50 cases");
        // La même base est retirée de sa rangée seulement.
        let mut belt = [None; BELT_SLOTS];
        belt[40] = skill;
        belt[49] = skill;
        belt[3] = skill;
        let changed = place(&mut belt, 42, skill);
        assert_eq!(changed, vec![40, 49, 42]);
        assert_eq!((belt[40], belt[49], belt[42], belt[3]), (None, None, skill, skill));
        assert_eq!(place(&mut belt, 42, skill), Vec::<usize>::new(), "rien de changé");
        assert_eq!(place(&mut belt, 3, None), vec![3]);
        // op 0 : 50 minis ; compétence par son identifiant de session, objet par son IID ; objet absent : case vide.
        let mut st = UiStatus::default();
        st.belt[0] = skill;
        st.belt[13] = Some(BeltSlot { base_type: 5, tid: 190, item_id: Some(900) });
        st.belt[14] = Some(BeltSlot { base_type: 5, tid: 191, item_id: Some(901) });
        let add = decode_packet(s, &build_packet(s, &st.add_fields(7, &inv, &skills))).unwrap();
        let list = add.list(2).unwrap();
        assert_eq!(list.len(), BELT_SLOTS);
        let read = |i: usize| {
            let Value::Bytes(b) = &list[i] else { panic!("mini attendu") };
            let m = decode_mini(&mini::SKILL_BASE, b).unwrap();
            m.int(0).zip(m.int(1))
        };
        assert_eq!((read(0), read(13), read(14), read(1)), (Some((7, 3)), Some((5, 900)), None, None));
        // Remplacement d'un objet consommé : un autre de même TID.
        assert_eq!(item_of_tid(&inv, 190), Some(900));
        assert_eq!(item_of_tid(&inv, 191), None);
    }
}
