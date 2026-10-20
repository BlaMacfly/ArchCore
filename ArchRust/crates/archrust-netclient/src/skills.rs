//! Compétences : liste du personnage, apprentissage et amélioration chez les instructeurs, lancement.
//!
//! Dialogue validé avec ArchCore (scénarios `sim skill`, `sim effects`) :
//!
//! ```text
//! vue optimisée op 2, champ 8 : paquets AGPMSKILL op 0 complets mis bout à bout (compétences connues)
//! AGPMSKILL op 0 (1 identifiant, 2 propriétaire, 3 facteurs : « dirt » indice 4 = niveau, 4 TID)
//! AGPMSKILL op 2 (1 identifiant, 3 facteurs) : nouveau niveau
//! AGPMCHARACTER op 1, champ 7 : facteurs du joueur, « dirt » indice 5 = points de compétence
//! 0x3B op 4 (1 EVENT_BASE, 2 CID)            → op 5 : fenêtre de l'instructeur
//! 0x3B op 1 (4 TID)                          → AGPMSKILL op 0, puis op 6 (4 TID, 5 résultat : 0 réussi,
//!                                              1 échec ou argent, 2 déjà apprise, 3 points, 4 niveau, 5 classe)
//! 0x3B op 2 (3 identifiant)                  → AGPMSKILL op 2, puis op 3
//! AGPMSKILL op 7 (1 identifiant, 6 action : 0 type, 1 cible en mini SKILL_BASE (3, CID))
//!     → op 7 diffusé (2 lanceur ; action : 1 cible, 5 incantation, 6 durée, 7 relance en ms)
//!     → op 8 (action 2 : 1 cible, 2 facteurs du résultat comme pour le combat)
//! ```

use crate::world::{ActionFactorPoints, WorldEvent};
use archrust_protocol::packet::{build_mini, decode_mini, decode_packet, sparse, Value};
use archrust_protocol::schema::{main, mini, packet_type, Schema};

/// Opérations d'AGPMSKILL.
pub mod op {
    pub const ADD: i64 = 0;
    pub const UPDATE: i64 = 2;
    pub const CAST: i64 = 7;
    pub const CAST_RESULT: i64 = 8;
}

/// Opérations d'AGPMEVENTSKILLMASTER (0x3B).
pub mod master_op {
    pub const LEARN: i8 = 1;
    pub const UPGRADE: i8 = 2;
    pub const UPGRADE_RESULT: i64 = 3;
    pub const OPEN: i8 = 4;
    pub const OPENED: i64 = 5;
    pub const LEARN_RESULT: i64 = 6;
}

/// Résultats d'un apprentissage (0x3B op 6, champ 5).
pub mod learn_result {
    pub const SUCCESS: i32 = 0;
    pub const FAIL: i32 = 1;
    pub const ALREADY_LEARNED: i32 = 2;
    pub const NOT_ENOUGH_POINTS: i32 = 3;
    pub const LOW_LEVEL: i32 = 4;
    pub const WRONG_CLASS: i32 = 5;
}

/// Indices du sous-paquet « dirt » des facteurs : niveau d'une compétence, points du personnage.
const DIRT_SKILL_LEVEL: usize = 4;
pub const DIRT_SKILL_POINT: usize = 5;
/// Sous-paquet « dirt » d'AGPMCHAR_FACTORS.
const FACTORS_DIRT: usize = 11;

/// Fonction « maître des compétences » d'un PNJ.
const SKILL_MASTER_FUNCTION: i32 = 25;

/// Demandes de compétences.
#[derive(Debug, Clone, PartialEq)]
pub enum SkillCommand {
    /// Lancer une compétence (identifiant) sur une cible (CID ; le joueur pour lui-même).
    Cast { id: i32, target: i32 },
    /// Ouvrir la fenêtre d'un instructeur.
    MasterOpen(i32),
    /// Apprendre une compétence (TID) auprès d'un instructeur.
    Learn { npc: i32, tid: i32 },
    /// Améliorer une compétence (identifiant) auprès d'un instructeur.
    Upgrade { npc: i32, id: i32 },
}

fn base(cid: i32) -> Value {
    let s = mini::SKILL_BASE;
    Value::Packet(build_mini(s, &[Some(Value::I8(3)), Some(Value::I32(cid))]).unwrap_or_default())
}

/// Paquet d'une demande de compétence pour le personnage `me`.
pub fn command_packet(cmd: &SkillCommand, me: i32) -> (&'static Schema, Vec<Option<Value>>) {
    let master = |op: i8, npc: i32, extra: &[(usize, Value)]| {
        let mut v = vec![(0, Value::I8(op)), (1, crate::npc::event_base(npc, SKILL_MASTER_FUNCTION)), (2, Value::I32(me))];
        v.extend_from_slice(extra);
        (main::EVENT_SKILL_MASTER, sparse(main::EVENT_SKILL_MASTER, &v))
    };
    match cmd {
        SkillCommand::Cast { id, target } => {
            let a = mini::SKILL_ACTION;
            let action = Value::Packet(build_mini(a, &sparse(a, &[(0, Value::I8(0)), (1, base(*target))])).unwrap_or_default());
            (main::SKILL, sparse(main::SKILL, &[(0, Value::I8(op::CAST as i8)), (1, Value::I32(*id)), (2, base(me)), (6, action)]))
        }
        SkillCommand::MasterOpen(npc) => master(master_op::OPEN, *npc, &[]),
        SkillCommand::Learn { npc, tid } => master(master_op::LEARN, *npc, &[(4, Value::I32(*tid))]),
        SkillCommand::Upgrade { npc, id } => master(master_op::UPGRADE, *npc, &[(3, Value::I32(*id))]),
    }
}

/// Réponses liées aux compétences.
#[derive(Debug, Clone, PartialEq)]
pub enum SkillEvent {
    /// Compétence connue (entrée en jeu ou apprise) : identifiant, TID, niveau.
    Known { owner: i32, id: i32, tid: i32, level: i32 },
    /// Niveau d'une compétence changé.
    Level { id: i32, level: i32 },
    /// Lancement commencé (op 7) : lanceur, compétence, cible, durées en ms.
    Cast { caster: i32, id: i32, tid: i32, target: i32, cast_ms: u32, recast_ms: u32 },
    /// Fenêtre d'un instructeur ouverte (0x3B op 5).
    MasterOpened { npc: i32 },
    /// Résultat d'un apprentissage (0x3B op 6).
    Learned { tid: i32, result: i32 },
    /// Amélioration faite (0x3B op 3).
    Upgraded { id: i32 },
}

fn int(values: &[Option<Value>], i: usize) -> Option<i64> {
    values.get(i).and_then(Option::as_ref).and_then(Value::as_i64)
}

fn bytes(values: &[Option<Value>], i: usize) -> Option<&[u8]> {
    values.get(i).and_then(Option::as_ref).and_then(Value::as_bytes)
}

/// Indice `index` du sous-paquet « dirt » d'un AGPMCHAR_FACTORS.
pub fn factors_dirt(factors: &[u8], index: usize) -> Option<i32> {
    let (_, cf) = decode_mini(mini::CHAR_FACTORS, factors).ok()?;
    let (_, dirt) = decode_mini(mini::FACTOR_DIRT, bytes(&cf, FACTORS_DIRT)?).ok()?;
    int(&dirt, index).map(|v| v as i32)
}

/// CID d'un mini SKILL_BASE.
fn base_cid(b: Option<&[u8]>) -> Option<i32> {
    let (_, v) = decode_mini(mini::SKILL_BASE, b?).ok()?;
    int(&v, 1).map(|v| v as i32)
}

/// Paquet AGPMSKILL (op 0, 2, 7, 8) ou 0x3B traduit, sinon `None`.
pub fn translate(t: u8, values: &[Option<Value>]) -> Option<WorldEvent> {
    match t {
        packet_type::SKILL => match int(values, 0)? {
            op::ADD => Some(WorldEvent::Skill(SkillEvent::Known {
                owner: base_cid(bytes(values, 2)).unwrap_or(0),
                id: int(values, 1)? as i32,
                tid: int(values, 4)? as i32,
                level: bytes(values, 3).and_then(|f| factors_dirt(f, DIRT_SKILL_LEVEL)).unwrap_or(1),
            })),
            op::UPDATE => Some(WorldEvent::Skill(SkillEvent::Level {
                id: int(values, 1)? as i32,
                level: bytes(values, 3).and_then(|f| factors_dirt(f, DIRT_SKILL_LEVEL))?,
            })),
            op::CAST => {
                let (_, a) = decode_mini(mini::SKILL_ACTION, bytes(values, 6)?).ok()?;
                Some(WorldEvent::Skill(SkillEvent::Cast {
                    caster: base_cid(bytes(values, 2)).unwrap_or(0),
                    id: int(values, 1)? as i32,
                    tid: int(values, 4).unwrap_or(0) as i32,
                    target: base_cid(bytes(&a, 1)).unwrap_or(0),
                    cast_ms: int(&a, 5).unwrap_or(0) as u32,
                    recast_ms: int(&a, 7).unwrap_or(0) as u32,
                }))
            }
            // Effet de la compétence : présenté comme un coup (dégâts, PV restants), comme le 0x41.
            op::CAST_RESULT => {
                let (_, a) = decode_mini(mini::SKILL_ACTION, bytes(values, 6)?).ok()?;
                let factor_packet = bytes(&a, 2).map(<[u8]>::to_vec);
                let hit = factor_packet.as_deref().and_then(crate::combat::decode_hit_factors);
                let factors: Option<ActionFactorPoints> = hit.as_ref().map(|h| h.0.clone());
                let damage = hit.as_ref().and_then(|h| h.1);
                Some(WorldEvent::CharacterAction {
                    cid: base_cid(bytes(values, 2)).unwrap_or(0),
                    target_cid: base_cid(bytes(&a, 1)).unwrap_or(0),
                    result: Some(crate::combat::outcome::SUCCESS),
                    target_hp: factors.as_ref().and_then(|f| f.hp),
                    factors,
                    damage,
                    factor_packet,
                    combo: None,
                    force_attack: None,
                    additional_effect: None,
                    hit_index: None,
                })
            }
            _ => None,
        },
        packet_type::EVENT_SKILL_MASTER => {
            let npc = bytes(values, 1)
                .and_then(|b| decode_mini(mini::EVENT_BASE, b).ok())
                .and_then(|(_, v)| int(&v, 1))
                .unwrap_or(0) as i32;
            Some(WorldEvent::Skill(match int(values, 0)? {
                master_op::OPENED => SkillEvent::MasterOpened { npc },
                master_op::LEARN_RESULT => SkillEvent::Learned { tid: int(values, 4).unwrap_or(0) as i32, result: int(values, 5).unwrap_or(-1) as i32 },
                master_op::UPGRADE_RESULT => SkillEvent::Upgraded { id: int(values, 3).unwrap_or(0) as i32 },
                _ => return None,
            }))
        }
        _ => None,
    }
}

/// Compétences du champ 8 de la vue optimisée op 2 : paquets AGPMSKILL complets mis bout à bout.
pub fn decode_skill_block(mut b: &[u8]) -> Vec<WorldEvent> {
    let mut out = Vec::new();
    while b.len() > 5 {
        let Ok(len) = b[1..5].try_into().map(u32::from_le_bytes) else { break };
        let len = len as usize;
        if len < archrust_protocol::packet::HEADER_SIZE || len > b.len() {
            break;
        }
        let (one, rest) = b.split_at(len);
        b = rest;
        if let Some(ev) = decode_packet(main::SKILL, one).ok().and_then(|p| translate(packet_type::SKILL, &p.values)) {
            out.push(ev);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use archrust_protocol::packet::build_packet;

    fn factors(level: i32) -> Value {
        let d = mini::FACTOR_DIRT;
        let dirt = build_mini(d, &sparse(d, &[(4, Value::I32(level)), (5, Value::I32(level))])).unwrap();
        let cf = mini::CHAR_FACTORS;
        Value::Packet(build_mini(cf, &sparse(cf, &[(11, Value::Packet(dirt))])).unwrap())
    }

    #[test]
    fn liste_et_lancement() {
        let k = main::SKILL;
        let add = |id: i32, tid: i32, level: i32| {
            build_packet(k, &sparse(k, &[(0, Value::I8(0)), (1, Value::I32(id)), (2, base(42)), (3, factors(level)), (4, Value::I32(tid))])).unwrap()
        };
        let mut block = add(1, 222, 1);
        block.extend(add(2, 4, 3));
        let evs = decode_skill_block(&block);
        assert_eq!(evs.len(), 2);
        let WorldEvent::Skill(SkillEvent::Known { owner, id, tid, level }) = &evs[1] else { panic!("{:?}", evs[1]) };
        assert_eq!((*owner, *id, *tid, *level), (42, 2, 4, 3));

        let (s, v) = command_packet(&SkillCommand::Cast { id: 2, target: 77 }, 42);
        assert_eq!(s.id, packet_type::SKILL);
        let (_, a) = decode_mini(mini::SKILL_ACTION, bytes(&v, 6).unwrap()).unwrap();
        assert_eq!((int(&v, 0), int(&v, 1), base_cid(bytes(&a, 1))), (Some(7), Some(2), Some(77)));
        let (s, v) = command_packet(&SkillCommand::Learn { npc: 9, tid: 57 }, 42);
        assert_eq!((s.id, int(&v, 0), int(&v, 4)), (0x3b, Some(1), Some(57)));
    }
}
