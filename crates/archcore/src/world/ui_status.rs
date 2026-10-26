//! Statut d'interface du joueur (AgpmUIStatus, 0x14) : potions de vie et de mana de la barre, seuils de
//! leur prise automatique (les deux curseurs posés sur les jauges de PV et de PM de `UI_CharInfo`, en
//! pourcentage, 0 : désactivé) et affichage du casque. Le client les reçoit à l'entrée en jeu (op 0) et
//! renvoie chaque changement (op 1 ; op 3 pour le casque). C'est le client qui boit la potion quand une
//! jauge passe sous son seuil ; le serveur garde seulement les réglages. La barre de raccourcis (champ 2)
//! n'est pas gardée ici.

use super::WorldState;
use crate::db::{col, Databases};
use archcore_net::{schema, Fields, Packet};
use crate::info;

pub mod op {
    pub const ADD: i64 = 0;
    pub const UPDATE_ITEM: i64 = 1;
    pub const VIEW_HELMET: i64 = 3;
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
}

impl Default for UiStatus {
    fn default() -> Self {
        Self { hp_potion_tid: 0, mp_potion_tid: 0, view_helmet: true, auto_hp: 0, auto_mp: 0 }
    }
}

impl UiStatus {
    pub async fn load(db: &Databases, character_id: u32) -> anyhow::Result<Self> {
        let row = db
            .characters
            .first("SELECT hp_potion_tid, mp_potion_tid, view_helmet, auto_hp, auto_mp FROM character_ui_status WHERE character_id = ?", (character_id,))
            .await?;
        Ok(row.map_or_else(Self::default, |r| Self {
            hp_potion_tid: col(&r, "hp_potion_tid"),
            mp_potion_tid: col(&r, "mp_potion_tid"),
            view_helmet: col::<i32>(&r, "view_helmet") != 0,
            auto_hp: col(&r, "auto_hp"),
            auto_mp: col(&r, "auto_mp"),
        }))
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

    /// Applique un paquet op 1 ou op 3 ; seuls les champs présents changent. Rend `true` si un réglage a
    /// changé.
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

    /// Paquet op 0 envoyé à l'entrée en jeu.
    pub fn add_fields(&self, cid: u32) -> Fields {
        let s = &schema::UI_STATUS;
        Fields::new(s)
            .with(0, op::ADD)
            .with(1, cid)
            .with(6, self.hp_potion_tid)
            .with(7, self.mp_potion_tid)
            .with(8, self.view_helmet as i64)
            .with(9, self.auto_hp as i64)
            .with(10, self.auto_mp as i64)
    }
}

/// Entrée en jeu : réglages envoyés au client.
pub fn on_enter(st: &WorldState, sid: u32) {
    let Some(p) = st.players.get(&sid) else { return };
    p.send(&schema::UI_STATUS, p.ui_status.add_fields(p.ch.id));
}

/// Changement envoyé par le client : gardé en base.
pub async fn on_packet(ctx: &super::WorldCtx, st: &mut WorldState, sid: u32, packet: &Packet) -> anyhow::Result<()> {
    let Some(p) = st.players.get_mut(&sid) else { return Ok(()) };
    if packet.int(1).is_some_and(|c| c != p.ch.id as i64) || !p.ui_status.apply(packet) {
        return Ok(());
    }
    let (status, cid, name) = (p.ui_status, p.ch.id, p.ch.name.clone());
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
        let add = decode_packet(s, &build_packet(s, &st.add_fields(7))).unwrap();
        assert_eq!((add.int(0), add.int(1), add.int(6), add.int(8)), (Some(op::ADD), Some(7), Some(190), Some(0)));
    }
}
