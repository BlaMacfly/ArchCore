//! Combat, mort, résurrection et butin (paquets 0x41, AGPMCHARACTER et AGPMITEM).
//!
//! Dialogue validé avec ArchCore (scénario `sim combat`) :
//!
//! ```text
//! 0x41 champs 0 attaquant, 1 cible, 6 attaque forcée            client → monde, une fois
//! 0x40 (marche jusqu'à la cible), puis 0x41 par coup             monde → clients
//!      champ 2 résultat (1 manqué, 5 touché, 6 critique, 7 esquive, 9 blocage),
//!      champ 3 AGPMCHAR_FACTORS : sous-paquet 0 « résultat » (points affichés, dégâts en négatif
//!      au champ 9), sous-paquet 3 PV et PM restants de la cible
//! AGPMCHARACTER op 1 champ 11 = 2                                mort (0 au retour à la vie)
//! AGPMCHARACTER op 9 (champ 1 = CID)                             résurrection au village
//! AGPMCHARACTER op 5                                             montée de niveau
//! AGPMITEM op 0 statut 1, champ 6 = mini VEC3F                   objet au sol (or : TID 268)
//! AGPMITEM op 11 (champ 2 = IID)                                 ramassage demandé
//! AGPMITEM op 12 (champ 1 : 0 réussi, 1 échec ; 2 IID, 3 TID, 5 quantité), op 1 retrait du sol
//! ```

use crate::world::ActionFactorPoints;
use archrust_protocol::packet::{decode_mini, sparse, Value};
use archrust_protocol::schema::{character_op, item_op, main, mini, Schema};
use glam::Vec3;

/// TID de l'or au sol : la quantité est la somme.
pub const GOLD_TID: i32 = 268;

/// Résultats d'une action 0x41 (champ 2).
pub mod outcome {
    pub const MISS: i8 = 1;
    pub const SUCCESS: i8 = 5;
    pub const CRITICAL: i8 = 6;
    pub const EVADE: i8 = 7;
    pub const BLOCK: i8 = 9;
}

/// Bits du type de modèle (`CharacterType` de `CharacterTemplatePublic.ini`) qui rendent un
/// personnage attaquable (0x10000) et ciblable (0x20000) ; les deux sont exigés.
pub const ATTACKABLE_TYPE: u32 = 0x10000 | 0x20000;

/// Objet posé au sol (AGPMITEM op 0, statut 1).
#[derive(Debug, Clone, PartialEq)]
pub struct GroundItem {
    pub iid: i32,
    pub tid: i32,
    /// Tueur à qui l'objet est réservé un moment.
    pub owner_cid: Option<i32>,
    pub count: i32,
    pub position: Vec3,
}

impl GroundItem {
    pub fn is_gold(&self) -> bool {
        self.tid == GOLD_TID
    }
}

/// Demandes de combat du moteur.
#[derive(Debug, Clone, PartialEq)]
pub enum CombatCommand {
    /// Attaque d'une cible (une seule fois : le serveur porte ensuite les coups).
    Attack { target: i32, forced: bool },
    /// Ramassage d'un objet au sol.
    PickUp(i32),
    /// Résurrection au village après la mort.
    ResurrectTown,
}

/// Paquet d'une demande de combat pour le personnage `me`.
pub fn command_packet(cmd: &CombatCommand, me: i32) -> (&'static Schema, Vec<Option<Value>>) {
    match cmd {
        CombatCommand::Attack { target, forced } => (
            main::OPT_CHAR_ACTION,
            sparse(
                main::OPT_CHAR_ACTION,
                &[(0, Value::I32(me)), (1, Value::I32(*target)), (6, Value::I8(*forced as i8))],
            ),
        ),
        CombatCommand::PickUp(iid) => (
            main::ITEM,
            sparse(main::ITEM, &[(0, Value::U8(item_op::PICKUP)), (2, Value::I32(*iid))]),
        ),
        CombatCommand::ResurrectTown => (
            main::CHARACTER,
            sparse(main::CHARACTER, &[(0, Value::I8(character_op::RESURRECT_TOWN)), (1, Value::I32(me))]),
        ),
    }
}

fn int(values: &[Option<Value>], i: usize) -> Option<i64> {
    values.get(i).and_then(Option::as_ref).and_then(Value::as_i64)
}

/// Objet au sol d'un paquet AGPMITEM (champs décodés), sinon `None`.
pub fn decode_ground_item(values: &[Option<Value>]) -> Option<GroundItem> {
    if int(values, 1)? as i8 != archrust_protocol::schema::ITEM_STATUS_FIELD {
        return None;
    }
    let position = values
        .get(6)
        .and_then(Option::as_ref)
        .and_then(Value::as_bytes)
        .and_then(|b| decode_mini(mini::ITEM_FIELD, b).ok())
        .and_then(|(_, v)| v.first().and_then(Option::as_ref).and_then(Value::as_vec3))
        .map(Vec3::from_array)?;
    Some(GroundItem {
        iid: int(values, 2)? as i32,
        tid: int(values, 3)? as i32,
        owner_cid: int(values, 4).map(|v| v as i32),
        count: int(values, 5).unwrap_or(1) as i32,
        position,
    })
}

/// Points d'un mini-paquet `AGPMCHAR_FACTOR_POINT`.
pub fn decode_points(bytes: &[u8]) -> Option<ActionFactorPoints> {
    let (_, v) = decode_mini(mini::FACTOR_POINT, bytes).ok()?;
    let f = |i: usize| int(&v, i).map(|x| x as i32);
    Some(ActionFactorPoints {
        hp: f(0),
        mp: f(1),
        normal_damage: f(9),
        magic_damage: f(10),
        water_damage: f(11),
        fire_damage: f(12),
        earth_damage: f(13),
        air_damage: f(14),
        poison_damage: f(15),
        lightning_damage: f(16),
        ice_damage: f(17),
        heroic_damage: f(19),
    })
}

/// Points du sous-paquet 3 d'un `AGPMCHAR_FACTORS`.
fn factors_points(bytes: &[u8]) -> Option<ActionFactorPoints> {
    let (_, v) = decode_mini(mini::CHAR_FACTORS, bytes).ok()?;
    decode_points(v.get(3)?.as_ref()?.as_bytes()?)
}

/// Facteurs d'un coup (champ 3 du 0x41, schéma complet `AGPMCHAR_FACTORS`) : PV et PM restants de la
/// cible (sous-paquet 3), dégâts par élément tirés du sous-paquet 0 « résultat », et total des dégâts
/// (le serveur les écrit en négatif).
pub fn decode_hit_factors(bytes: &[u8]) -> Option<(ActionFactorPoints, Option<i32>)> {
    let (_, v) = decode_mini(mini::CHAR_FACTORS, bytes).ok()?;
    let sub = |i: usize| v.get(i).and_then(Option::as_ref).and_then(Value::as_bytes);
    let current = sub(3).and_then(decode_points);
    let shown = sub(0).and_then(factors_points);
    if current.is_none() && shown.is_none() {
        return None;
    }
    let mut points = shown.clone().unwrap_or_default();
    if let Some(c) = &current {
        points.hp = c.hp.or(points.hp);
        points.mp = c.mp.or(points.mp);
    }
    let damage = shown.map(|s| {
        [
            s.normal_damage,
            s.magic_damage,
            s.water_damage,
            s.fire_damage,
            s.earth_damage,
            s.air_damage,
            s.poison_damage,
            s.lightning_damage,
            s.ice_damage,
            s.heroic_damage,
        ]
        .into_iter()
        .flatten()
        .map(|d| d.unsigned_abs() as i32)
        .sum::<i32>()
    });
    Some((points, damage))
}

#[cfg(test)]
mod tests {
    use super::*;
    use archrust_protocol::packet::{build_mini, build_packet, decode_packet};

    /// Facteurs d'un coup tels qu'ArchCore les forme (`damage_factors`).
    fn hit(hp: i32, mp: i32, damage: i32) -> Vec<u8> {
        let pt = mini::FACTOR_POINT;
        let cf = mini::CHAR_FACTORS;
        let shown = build_mini(pt, &sparse(pt, &[(0, Value::I32(hp)), (1, Value::I32(mp)), (9, Value::I32(-damage))])).unwrap();
        let result = build_mini(cf, &sparse(cf, &[(3, Value::Packet(shown))])).unwrap();
        let current = build_mini(pt, &sparse(pt, &[(0, Value::I32(hp)), (1, Value::I32(mp))])).unwrap();
        build_mini(cf, &sparse(cf, &[(0, Value::Packet(result)), (3, Value::Packet(current))])).unwrap()
    }

    #[test]
    fn coup_degats_et_pv_restants() {
        let (points, damage) = decode_hit_factors(&hit(80, 12, 27)).unwrap();
        assert_eq!((points.hp, points.mp, damage), (Some(80), Some(12), Some(27)));
        assert_eq!(points.normal_damage, Some(-27));
    }

    #[test]
    fn paquets_de_demande() {
        let (s, v) = command_packet(&CombatCommand::Attack { target: 0x4000_0010, forced: true }, 7);
        let raw = build_packet(s, &v).unwrap();
        let p = decode_packet(s, &raw).unwrap();
        assert_eq!((p.get(0).and_then(Value::as_i64), p.get(1).and_then(Value::as_i64), p.get(6).and_then(Value::as_i64)), (Some(7), Some(0x4000_0010), Some(1)));
        assert!(p.get(2).is_none());
        let (s, v) = command_packet(&CombatCommand::PickUp(0x3000_0001), 7);
        let p = decode_packet(s, &build_packet(s, &v).unwrap()).unwrap();
        assert_eq!((p.op(), p.get(2).and_then(Value::as_i64)), (Some(11), Some(0x3000_0001)));
        let (s, v) = command_packet(&CombatCommand::ResurrectTown, 7);
        let p = decode_packet(s, &build_packet(s, &v).unwrap()).unwrap();
        assert_eq!((p.op(), p.get(1).and_then(Value::as_i64)), (Some(9), Some(7)));
    }

    #[test]
    fn objet_au_sol() {
        let field = build_mini(mini::ITEM_FIELD, &sparse(mini::ITEM_FIELD, &[(0, Value::Vec3([1.0, 2.0, 3.0]))])).unwrap();
        let values = sparse(
            main::ITEM,
            &[
                (0, Value::U8(0)),
                (1, Value::I8(1)),
                (2, Value::I32(0x3000_0005)),
                (3, Value::I32(GOLD_TID)),
                (4, Value::I32(7)),
                (5, Value::I32(42)),
                (6, Value::Packet(field)),
            ],
        );
        let p = decode_packet(main::ITEM, &build_packet(main::ITEM, &values).unwrap()).unwrap();
        let g = decode_ground_item(&p.values).unwrap();
        assert_eq!(g, GroundItem { iid: 0x3000_0005, tid: GOLD_TID, owner_cid: Some(7), count: 42, position: Vec3::new(1.0, 2.0, 3.0) });
        assert!(g.is_gold());
    }
}
