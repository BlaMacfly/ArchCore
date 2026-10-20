//! Autres joueurs en vue. ArchCore annonce un joueur qui entre dans la vue par une fiche
//! AGPMCHARACTER op 0 complète (CID, TID, nom, position au champ 5, facteurs au champ 7, visage et
//! cheveux aux champs 20/21), suivie des AGPMITEM de son équipement porté (statut 3, propriétaire =
//! son CID), puis d'un 0x40 s'il marche ; il le retire par un op 3. Ces fiches arrivent en
//! `WorldEvent::SelfCharacter`, comme celles du joueur (op 25 puis 0) : un CID différent du sien en
//! fait une entité, et ses objets portés vont dans l'équipement de cette entité, pas dans
//! l'inventaire. Les déplacements (0x40, op 1) et le retrait suivent le chemin des autres entités.

use super::App;
use crate::scene::{entity_meshes_with_equipment, Entity, Head};
use archrust_netclient::world::{CharacterStats, EquipmentView, ItemRecord};
use glam::{Mat4, Vec3};

/// Fiche complète d'un autre joueur (champs de `WorldEvent::SelfCharacter`).
pub(super) struct OtherPlayer {
    pub cid: i32,
    pub tid: i32,
    pub name: String,
    pub position: Option<Vec3>,
    pub face: Option<i32>,
    pub hair: Option<i32>,
    pub stats: Option<CharacterStats>,
}

/// Statut AgpmItem « équipé ».
const STATUS_EQUIP: i8 = 3;

/// Applique un paquet AgpmItem à l'équipement porté d'un personnage : ajout ou mise à jour d'un
/// objet équipé, retrait d'un objet supprimé (op 1) ou rangé ailleurs. Rend `true` si l'équipement
/// a changé (modèle à recharger).
fn apply_worn_item(equipment: &mut Vec<EquipmentView>, item: &ItemRecord) -> bool {
    let existing = equipment.iter().position(|i| i.item_id == item.id);
    let worn = item.operation != 1
        && match item.status {
            Some(status) => status == STATUS_EQUIP,
            None => existing.is_some(),
        };
    let before = equipment.clone();
    match (worn, existing) {
        (true, Some(k)) => {
            if let Some(tid) = item.template_id {
                equipment[k].template_id = tid;
            }
            if let Some(count) = item.count {
                equipment[k].count = count as i16;
            }
        }
        (true, None) => {
            if let Some(tid) = item.template_id {
                equipment.push(EquipmentView {
                    item_id: item.id,
                    template_id: tid,
                    count: item.count.unwrap_or(1) as i16,
                });
            }
        }
        (false, Some(k)) => {
            equipment.remove(k);
        }
        (false, None) => {}
    }
    *equipment != before
}

impl App {
    /// Vrai pour le CID d'un autre joueur : le joueur a déjà son CID (fiche op 25), et c'en est un
    /// autre. Le personnage local du mode sans client (CID 0) est toujours remplacé par le vrai.
    pub(super) fn is_other_player(&self, cid: i32) -> bool {
        self.player.as_ref().is_some_and(|p| p.cid != 0 && p.cid != cid)
    }

    /// Fiche d'un autre joueur : nouvelle entité, ou mise à jour de celle qui est déjà en vue.
    pub(super) fn other_player_seen(&mut self, o: OtherPlayer) {
        let hp = o.stats.as_ref().and_then(|s| s.hp);
        if let Some(index) = self.entities.iter().position(|e| e.cid == o.cid) {
            let e = &mut self.entities[index];
            if !o.name.is_empty() {
                e.name = o.name;
            }
            if hp.is_some() {
                e.hp = hp;
            }
            if let Some(pos) = o.position {
                e.position = pos;
                e.destination = pos;
            }
            let head = Head {
                face: o.face.unwrap_or(e.head.face),
                hair: o.hair.unwrap_or(e.head.hair),
            };
            let tid = if o.tid != 0 { o.tid } else { e.tid };
            if head != e.head || tid != e.tid {
                e.head = head;
                e.tid = tid;
                self.reload_entity_meshes(index);
            }
            return;
        }
        let head = Head {
            face: o.face.unwrap_or(0),
            hair: o.hair.unwrap_or(0),
        };
        let pos = o.position.unwrap_or(Vec3::ZERO);
        self.entities.push(Entity {
            cid: o.cid,
            tid: o.tid,
            name: o.name,
            hp,
            position: pos,
            destination: pos,
            turn_y: 0.0,
            head,
            meshes: Vec::new(),
            equipment: Vec::new(),
            is_player: true,
        });
        let index = self.entities.len() - 1;
        self.reload_entity_meshes(index);
        let e = &self.entities[index];
        log::info!(
            "autre joueur {} (cid {}, tid {}) en vue en {:.0}, {:.0} : {} maillage(s)",
            e.name,
            e.cid,
            e.tid,
            pos.x,
            pos.z,
            e.meshes.len()
        );
    }

    /// Objet d'un autre joueur : ses objets portés vont dans l'équipement de son entité (modèle
    /// rechargé), jamais dans l'inventaire du joueur. Rend `true` si l'objet le concernait.
    pub(super) fn other_player_item(&mut self, item: &ItemRecord) -> bool {
        let other_owner = item.owner_cid.filter(|c| self.is_other_player(*c));
        let holder = other_owner
            .and_then(|c| self.entities.iter().position(|e| e.cid == c))
            .or_else(|| {
                self.entities
                    .iter()
                    .position(|e| e.equipment.iter().any(|i| i.item_id == item.id))
            });
        let Some(index) = holder else {
            // Propriétaire hors de vue : rien à afficher, mais pas dans l'inventaire non plus.
            return other_owner.is_some();
        };
        if apply_worn_item(&mut self.entities[index].equipment, item) {
            self.reload_entity_meshes(index);
        }
        true
    }

    /// Recharge le corps, la tête et l'équipement d'une entité ; la boîte de repère en cas d'échec.
    fn reload_entity_meshes(&mut self, index: usize) {
        let Some(gpu) = self.gpu.as_mut() else { return };
        let fallback = gpu.marker.clone();
        let e = &self.entities[index];
        let meshes = entity_meshes_with_equipment(
            &mut gpu.renderer,
            &self.assets,
            e.tid,
            e.head,
            &e.equipment,
            &fallback,
        )
        .unwrap_or_else(|err| {
            log::warn!("TID {} : {err:#}", e.tid);
            vec![(fallback.clone(), Mat4::IDENTITY)]
        });
        self.entities[index].meshes = meshes;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(operation: u8, id: i32, tid: Option<i32>, status: Option<i8>) -> ItemRecord {
        ItemRecord { operation, id, template_id: tid, owner_cid: Some(7), count: Some(1), status, grid: None }
    }

    #[test]
    fn worn_items_follow_equip_change_and_removal() {
        let mut gear = Vec::new();
        // Équipement envoyé après la fiche op 0 : AGPMITEM op 0, statut 3.
        assert!(apply_worn_item(&mut gear, &item(0, 11, Some(1), Some(3))));
        assert!(apply_worn_item(&mut gear, &item(0, 12, Some(2), Some(3))));
        assert!(!apply_worn_item(&mut gear, &item(0, 12, Some(2), Some(3))), "même objet : pas de rechargement");
        assert_eq!(gear.iter().map(|i| (i.item_id, i.template_id)).collect::<Vec<_>>(), vec![(11, 1), (12, 2)]);
        // Rangé dans le sac (statut 2), puis supprimé (op 1) : retiré de l'équipement.
        assert!(apply_worn_item(&mut gear, &item(2, 11, None, Some(2))));
        assert!(apply_worn_item(&mut gear, &item(1, 12, None, None)));
        assert!(gear.is_empty());
        // Un objet du sac d'un autre joueur ne s'affiche pas.
        assert!(!apply_worn_item(&mut gear, &item(0, 13, Some(3), Some(2))));
    }
}
