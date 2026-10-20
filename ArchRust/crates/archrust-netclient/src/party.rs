//! Groupes : paquets AGPMPARTY (0x08) et AGPMPARTYMANAGEMENT (0x09), état du groupe côté client.
//!
//! ```text
//! Gestion (0x09, drapeaux 1) : 0 opération, 1 CID de l'auteur, 2 CID de la cible, 3 nom (49 octets).
//!   envoyé : 0 invitation (cible par CID ou par nom), 1 refus et 2 acceptation (cible = l'inviteur),
//!            3 départ, 4 exclusion (chef), 11 passage de chef (chef)
//!   reçu   : 0 invitation (auteur, nom de l'auteur), 1 refus et 2 acceptation (cible, nom de l'invité),
//!            4 exclusion, 11 nouveau chef (auteur), échecs 5 autre groupe, 6 déjà membre, 7 plein,
//!            8 pas chef, 10 joueur absent
//! Groupe (0x08, drapeaux 2) : 0 opération, 1 identifiant, 2 membres au plus, 3 membres, 4 CID d'un
//!   membre, 5 liste des CID (u32, chef en tête), 8 répartition de l'expérience, 10 des objets.
//!   reçu   : 0 groupe et liste, 3 membre ajouté, 4 membre retiré, 1 groupe retiré, 5 et 6 réglages
//!   envoyé : 5 (champ 8) et 6 (champ 10) par le chef
//! ```
//!
//! À l'acceptation, le nouveau membre reçoit l'op 0 puis un op 3 par membre (lui compris), les
//! autres un op 3 (lui). À un départ, le groupe et le partant reçoivent l'op 4 (le partant), puis
//! le partant l'op 1 ; un groupe réduit à un membre est dissous de la même façon.

use crate::connection::Incoming;
use archrust_protocol::packet::{sparse, Value};
use archrust_protocol::schema::{main, packet_type};
use std::collections::HashMap;

/// Opérations du paquet de groupe (0x08).
pub mod op {
    pub const ADD: i8 = 0;
    pub const REMOVE: i8 = 1;
    pub const ADD_MEMBER: i8 = 3;
    pub const REMOVE_MEMBER: i8 = 4;
    pub const UPDATE_EXP_TYPE: i8 = 5;
    pub const UPDATE_ITEM_DIVISION: i8 = 6;
}

/// Opérations du paquet de gestion (0x09).
pub mod mgmt {
    pub const INVITE: i8 = 0;
    pub const REJECT: i8 = 1;
    pub const INVITE_ACCEPT: i8 = 2;
    pub const LEAVE: i8 = 3;
    pub const BANISH: i8 = 4;
    pub const FAILED_OTHER_PARTY: i8 = 5;
    pub const FAILED_ALREADY_MEMBER: i8 = 6;
    pub const FAILED_FULL: i8 = 7;
    pub const FAILED_NOT_LEADER: i8 = 8;
    pub const FAILED_NO_LOGIN: i8 = 10;
    pub const DELEGATION_LEADER: i8 = 11;
}

/// Répartition de l'expérience (champ 8).
pub const EXP_BY_DAMAGE: i8 = 0;
pub const EXP_BY_LEVEL: i8 = 1;
/// Répartition des objets (champ 10) : à tour de rôle, libre, selon les dégâts.
pub const ITEM_SEQUENCE: i8 = 0;
pub const ITEM_FREE: i8 = 1;
pub const ITEM_DAMAGE: i8 = 2;

/// Libellé d'une répartition de l'expérience.
pub fn exp_type_label(t: i8) -> &'static str {
    match t {
        EXP_BY_LEVEL => "selon le niveau",
        _ => "selon les dégâts",
    }
}

/// Libellé d'une répartition des objets.
pub fn item_division_label(t: i8) -> &'static str {
    match t {
        ITEM_SEQUENCE => "à tour de rôle",
        ITEM_FREE => "libre",
        _ => "selon les dégâts",
    }
}

/// Paquet de groupe (0x08) décodé.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartyPacket {
    pub op: i8,
    pub id: Option<i32>,
    pub max_members: Option<i8>,
    pub member: Option<i32>,
    /// Liste des CID, chef en tête (op 0).
    pub members: Option<Vec<i32>>,
    pub exp_type: Option<i8>,
    pub item_division: Option<i8>,
}

/// Paquet de gestion (0x09) décodé.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartyManagement {
    pub op: i8,
    pub author: Option<i32>,
    pub target: Option<i32>,
    pub name: String,
}

fn int(p: &Incoming, i: usize) -> Option<i64> {
    p.get(i).and_then(Value::as_i64)
}

/// Décode un paquet de groupe (0x08).
pub fn decode_party(p: &Incoming) -> Option<PartyPacket> {
    if p.packet_type != packet_type::PARTY {
        return None;
    }
    let members = p.get(5).and_then(Value::as_bytes).map(|b| {
        b.chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]) as i32)
            .collect()
    });
    Some(PartyPacket {
        op: int(p, 0)? as i8,
        id: int(p, 1).map(|v| v as i32),
        max_members: int(p, 2).map(|v| v as i8),
        member: int(p, 4).map(|v| v as i32),
        members,
        exp_type: int(p, 8).map(|v| v as i8),
        item_division: int(p, 10).map(|v| v as i8),
    })
}

/// Décode un paquet de gestion (0x09).
pub fn decode_management(p: &Incoming) -> Option<PartyManagement> {
    if p.packet_type != packet_type::PARTY_MANAGEMENT {
        return None;
    }
    Some(PartyManagement {
        op: int(p, 0)? as i8,
        author: int(p, 1).map(|v| v as i32).filter(|c| *c != 0),
        target: int(p, 2).map(|v| v as i32).filter(|c| *c != 0),
        name: p.get(3).and_then(Value::as_text).unwrap_or_default(),
    })
}

/// Demandes du joueur concernant les groupes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartyCommand {
    /// Invite un joueur, par CID (cible sélectionnée) ou par nom.
    Invite { cid: Option<i32>, name: Option<String> },
    /// Accepte l'invitation de ce joueur (CID de l'inviteur).
    Accept(i32),
    /// Refuse l'invitation de ce joueur.
    Reject(i32),
    Leave,
    /// Exclut ce membre (chef).
    Banish(i32),
    /// Passe la main à ce membre (chef).
    Delegate(i32),
    /// Répartition de l'expérience (chef) : `EXP_BY_DAMAGE` ou `EXP_BY_LEVEL`.
    SetExpType(i8),
    /// Répartition des objets (chef).
    SetItemDivision(i8),
}

/// Paquet à envoyer pour une demande : schéma et valeurs. `me` = CID du joueur.
pub fn command_packet(
    cmd: &PartyCommand,
    me: i32,
) -> (&'static archrust_protocol::schema::Schema, Vec<Option<Value>>) {
    let management = |kind: i8, target: Option<i32>, name: Option<&str>| {
        let mut pairs = vec![(0, Value::I8(kind)), (1, Value::I32(me))];
        if let Some(t) = target {
            pairs.push((2, Value::I32(t)));
        }
        if let Some(n) = name.filter(|n| !n.is_empty()) {
            pairs.push((3, Value::str(n.as_bytes())));
        }
        (main::PARTY_MANAGEMENT, sparse(main::PARTY_MANAGEMENT, &pairs))
    };
    match cmd {
        PartyCommand::Invite { cid, name } => management(mgmt::INVITE, *cid, name.as_deref()),
        PartyCommand::Accept(inviter) => management(mgmt::INVITE_ACCEPT, Some(*inviter), None),
        PartyCommand::Reject(inviter) => management(mgmt::REJECT, Some(*inviter), None),
        PartyCommand::Leave => management(mgmt::LEAVE, None, None),
        PartyCommand::Banish(cid) => management(mgmt::BANISH, Some(*cid), None),
        PartyCommand::Delegate(cid) => management(mgmt::DELEGATION_LEADER, Some(*cid), None),
        PartyCommand::SetExpType(t) => (
            main::PARTY,
            sparse(
                main::PARTY,
                &[(0, Value::I8(op::UPDATE_EXP_TYPE)), (4, Value::I32(me)), (8, Value::I8(*t))],
            ),
        ),
        PartyCommand::SetItemDivision(t) => (
            main::PARTY,
            sparse(
                main::PARTY,
                &[
                    (0, Value::I8(op::UPDATE_ITEM_DIVISION)),
                    (4, Value::I32(me)),
                    (10, Value::I8(*t)),
                ],
            ),
        ),
    }
}

/// Invitation reçue, en attente de réponse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invitation {
    pub cid: i32,
    pub name: String,
}

/// Ce qui a changé, pour les messages de l'interface et les événements des AddOns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartyNotice {
    /// Le joueur entre dans un groupe (op 0).
    Joined,
    /// Le joueur n'est plus dans un groupe (départ, exclusion, dissolution).
    Left,
    MemberJoined(i32),
    MemberLeft(i32),
    /// Nouveau chef (CID).
    LeaderChanged(i32),
    ExpTypeChanged(i8),
    ItemDivisionChanged(i8),
    Invited(Invitation),
    /// L'invité a refusé (nom).
    Rejected(String),
    /// L'invité a accepté (nom).
    Accepted(String),
    /// Le joueur a été exclu par le chef.
    Banished,
    /// Échec d'une demande : opération (`mgmt::FAILED_*`) et nom concerné.
    Failed { code: i8, name: String },
}

impl PartyNotice {
    /// Message de discussion, avec les noms connus.
    pub fn message(&self, state: &PartyState) -> String {
        let name = |cid: &i32| state.name_of(*cid);
        match self {
            PartyNotice::Joined => "Vous avez rejoint un groupe.".into(),
            PartyNotice::Left => "Vous n'êtes plus dans un groupe.".into(),
            PartyNotice::MemberJoined(c) => format!("{} rejoint le groupe.", name(c)),
            PartyNotice::MemberLeft(c) => format!("{} quitte le groupe.", name(c)),
            PartyNotice::LeaderChanged(c) => format!("{} est maintenant le chef du groupe.", name(c)),
            PartyNotice::ExpTypeChanged(t) => {
                format!("Expérience répartie {}.", exp_type_label(*t))
            }
            PartyNotice::ItemDivisionChanged(t) => {
                format!("Objets répartis {}.", item_division_label(*t))
            }
            PartyNotice::Invited(i) => format!(
                "{} vous invite dans son groupe (/accepter ou /refuser).",
                i.name
            ),
            PartyNotice::Rejected(n) => format!("{n} refuse votre invitation."),
            PartyNotice::Accepted(n) => format!("{n} accepte votre invitation."),
            PartyNotice::Banished => "Vous avez été exclu du groupe.".into(),
            PartyNotice::Failed { code, name } => {
                let who = if name.is_empty() { "Ce joueur" } else { name.as_str() };
                match *code {
                    mgmt::FAILED_OTHER_PARTY => format!("{who} est déjà dans un autre groupe."),
                    mgmt::FAILED_ALREADY_MEMBER => "Vous êtes déjà dans un groupe.".into(),
                    mgmt::FAILED_FULL => "Le groupe est plein.".into(),
                    mgmt::FAILED_NOT_LEADER => "Seul le chef du groupe peut inviter.".into(),
                    mgmt::FAILED_NO_LOGIN => format!("{who} n'est pas en jeu."),
                    other => format!("Échec de la demande de groupe ({other})."),
                }
            }
        }
    }
}

/// État du groupe du joueur, tenu à partir des paquets 0x08 et 0x09.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PartyState {
    pub id: Option<i32>,
    pub max_members: usize,
    /// CID des membres, chef en tête ; vide hors groupe.
    pub members: Vec<i32>,
    pub exp_type: i8,
    pub item_division: i8,
    /// Invitations reçues en attente, la plus récente en dernier.
    pub invitations: Vec<Invitation>,
    /// Noms connus (paquets de gestion, fiches de personnage).
    pub names: HashMap<i32, String>,
}

impl PartyState {
    pub fn in_party(&self) -> bool {
        !self.members.is_empty()
    }

    pub fn leader(&self) -> Option<i32> {
        self.members.first().copied()
    }

    pub fn is_leader(&self, cid: i32) -> bool {
        self.leader() == Some(cid)
    }

    pub fn contains(&self, cid: i32) -> bool {
        self.members.contains(&cid)
    }

    /// Retient le nom d'un personnage (fiche reçue, vue).
    pub fn learn_name(&mut self, cid: i32, name: &str) {
        if cid != 0 && !name.is_empty() {
            self.names.insert(cid, name.to_string());
        }
    }

    pub fn name_of(&self, cid: i32) -> String {
        self.names
            .get(&cid)
            .cloned()
            .unwrap_or_else(|| format!("CID {cid}"))
    }

    /// Membre désigné par son nom (insensible à la casse).
    pub fn member_by_name(&self, name: &str) -> Option<i32> {
        self.members.iter().copied().find(|c| {
            self.names
                .get(c)
                .is_some_and(|n| n.eq_ignore_ascii_case(name.trim()))
        })
    }

    fn clear_party(&mut self) -> bool {
        let was = self.in_party();
        self.id = None;
        self.members.clear();
        was
    }

    /// Applique un paquet de groupe reçu ; `me` = CID du joueur.
    pub fn apply_party(&mut self, p: &PartyPacket, me: i32) -> Option<PartyNotice> {
        match p.op {
            op::ADD => {
                self.id = p.id;
                self.max_members = p.max_members.map_or(5, |m| m.max(1) as usize);
                self.members = p.members.clone().unwrap_or_default();
                if !self.members.contains(&me) {
                    self.members.push(me);
                }
                self.exp_type = p.exp_type.unwrap_or(EXP_BY_DAMAGE);
                self.item_division = p.item_division.unwrap_or(ITEM_DAMAGE);
                // Les invitations reçues n'ont plus d'objet une fois dans un groupe.
                self.invitations.clear();
                Some(PartyNotice::Joined)
            }
            op::ADD_MEMBER => {
                let cid = p.member?;
                if !self.in_party() || self.members.contains(&cid) {
                    return None;
                }
                self.members.push(cid);
                Some(PartyNotice::MemberJoined(cid))
            }
            op::REMOVE_MEMBER => {
                let cid = p.member?;
                if cid == me {
                    return self.clear_party().then_some(PartyNotice::Left);
                }
                let before = self.members.len();
                self.members.retain(|c| *c != cid);
                (self.members.len() != before).then_some(PartyNotice::MemberLeft(cid))
            }
            op::REMOVE => self.clear_party().then_some(PartyNotice::Left),
            op::UPDATE_EXP_TYPE => {
                let t = p.exp_type?;
                self.exp_type = t;
                Some(PartyNotice::ExpTypeChanged(t))
            }
            op::UPDATE_ITEM_DIVISION => {
                let t = p.item_division?;
                self.item_division = t;
                Some(PartyNotice::ItemDivisionChanged(t))
            }
            _ => None,
        }
    }

    /// Applique un paquet de gestion reçu.
    pub fn apply_management(&mut self, m: &PartyManagement) -> Option<PartyNotice> {
        // Nom transmis : celui de l'auteur pour une invitation, de la cible sinon.
        let named = if m.op == mgmt::INVITE { m.author } else { m.target };
        if let Some(cid) = named {
            self.learn_name(cid, &m.name);
        }
        match m.op {
            mgmt::INVITE => {
                let cid = m.author?;
                let name = if m.name.is_empty() { self.name_of(cid) } else { m.name.clone() };
                let invitation = Invitation { cid, name };
                self.invitations.retain(|i| i.cid != cid);
                self.invitations.push(invitation.clone());
                Some(PartyNotice::Invited(invitation))
            }
            mgmt::REJECT => Some(PartyNotice::Rejected(m.name.clone())),
            mgmt::INVITE_ACCEPT => Some(PartyNotice::Accepted(m.name.clone())),
            mgmt::BANISH => {
                self.clear_party();
                Some(PartyNotice::Banished)
            }
            mgmt::DELEGATION_LEADER => {
                let leader = m.author?;
                if let Some(i) = self.members.iter().position(|c| *c == leader) {
                    self.members.swap(0, i);
                }
                Some(PartyNotice::LeaderChanged(leader))
            }
            code @ (mgmt::FAILED_OTHER_PARTY
            | mgmt::FAILED_ALREADY_MEMBER
            | mgmt::FAILED_FULL
            | mgmt::FAILED_NOT_LEADER
            | mgmt::FAILED_NO_LOGIN) => Some(PartyNotice::Failed {
                code,
                name: m.name.clone(),
            }),
            _ => None,
        }
    }

    /// Retire l'invitation de ce joueur (réponse donnée) ; rend `true` si elle existait.
    pub fn take_invitation(&mut self, cid: i32) -> bool {
        let before = self.invitations.len();
        self.invitations.retain(|i| i.cid != cid);
        self.invitations.len() != before
    }

    /// Déconnexion : plus de groupe ni d'invitation.
    pub fn reset(&mut self) {
        self.clear_party();
        self.invitations.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archrust_protocol::packet::{build_packet, decode_packet};

    fn incoming(schema: &'static archrust_protocol::schema::Schema, values: &[Option<Value>]) -> Incoming {
        let raw = build_packet(schema, values).unwrap();
        Incoming {
            packet_type: schema.id,
            packet: Some(decode_packet(schema, &raw).unwrap()),
            raw,
        }
    }

    fn party(pairs: &[(usize, Value)]) -> PartyPacket {
        decode_party(&incoming(main::PARTY, &sparse(main::PARTY, pairs))).unwrap()
    }

    fn management(kind: i8, author: i32, target: i32, name: &str) -> PartyManagement {
        let mut pairs = vec![(0, Value::I8(kind)), (1, Value::I32(author)), (2, Value::I32(target))];
        if !name.is_empty() {
            pairs.push((3, Value::str(name)));
        }
        decode_management(&incoming(main::PARTY_MANAGEMENT, &sparse(main::PARTY_MANAGEMENT, &pairs)))
            .unwrap()
    }

    fn list(cids: &[u32]) -> Value {
        Value::Block(cids.iter().flat_map(|c| c.to_le_bytes()).collect())
    }

    #[test]
    fn formation_dans_l_ordre_du_serveur() {
        // Côté invité (CID 9) : invitation de 7, acceptation, op 0 puis un op 3 par membre.
        let mut s = PartyState::default();
        let n = s.apply_management(&management(mgmt::INVITE, 7, 9, "Alice"));
        assert_eq!(n, Some(PartyNotice::Invited(Invitation { cid: 7, name: "Alice".into() })));
        assert_eq!(s.invitations.len(), 1);
        let n = s.apply_party(
            &party(&[
                (0, Value::I8(op::ADD)),
                (1, Value::I32(101)),
                (2, Value::I8(5)),
                (3, Value::I8(2)),
                (5, list(&[7, 9])),
                (8, Value::I8(EXP_BY_DAMAGE)),
                (10, Value::I8(ITEM_DAMAGE)),
            ]),
            9,
        );
        assert_eq!(n, Some(PartyNotice::Joined));
        assert_eq!((s.id, s.max_members, s.members.clone()), (Some(101), 5, vec![7, 9]));
        assert!(s.invitations.is_empty());
        assert!(s.is_leader(7) && !s.is_leader(9));
        for m in [7, 9] {
            let n = s.apply_party(&party(&[(0, Value::I8(op::ADD_MEMBER)), (1, Value::I32(101)), (4, Value::I32(m))]), 9);
            assert_eq!(n, None, "membre déjà connu par l'op 0");
        }
        assert_eq!(s.name_of(7), "Alice");
        // Troisième membre.
        let n = s.apply_party(&party(&[(0, Value::I8(op::ADD_MEMBER)), (4, Value::I32(12))]), 9);
        assert_eq!(n, Some(PartyNotice::MemberJoined(12)));
        assert_eq!(s.members, vec![7, 9, 12]);
    }

    #[test]
    fn depart_exclusion_et_chef() {
        let mut s = PartyState {
            id: Some(101),
            members: vec![7, 9, 12],
            ..Default::default()
        };
        // Passage de chef : auteur = nouveau chef.
        let n = s.apply_management(&management(mgmt::DELEGATION_LEADER, 12, 7, ""));
        assert_eq!(n, Some(PartyNotice::LeaderChanged(12)));
        assert_eq!(s.members, vec![12, 9, 7]);
        // Un autre membre part.
        let n = s.apply_party(&party(&[(0, Value::I8(op::REMOVE_MEMBER)), (4, Value::I32(7))]), 9);
        assert_eq!(n, Some(PartyNotice::MemberLeft(7)));
        // Exclusion du joueur : op 4 (lui), op 1, puis gestion op 4.
        let n = s.apply_party(&party(&[(0, Value::I8(op::REMOVE_MEMBER)), (4, Value::I32(9))]), 9);
        assert_eq!(n, Some(PartyNotice::Left));
        assert_eq!(s.apply_party(&party(&[(0, Value::I8(op::REMOVE))]), 9), None);
        assert_eq!(s.apply_management(&management(mgmt::BANISH, 9, 0, "")), Some(PartyNotice::Banished));
        assert!(!s.in_party());
    }

    #[test]
    fn reglages_et_echecs() {
        let mut s = PartyState { members: vec![1, 2], ..Default::default() };
        let n = s.apply_party(&party(&[(0, Value::I8(op::UPDATE_EXP_TYPE)), (4, Value::I32(1)), (8, Value::I8(EXP_BY_LEVEL))]), 2);
        assert_eq!(n, Some(PartyNotice::ExpTypeChanged(EXP_BY_LEVEL)));
        assert_eq!(s.exp_type, EXP_BY_LEVEL);
        let n = s.apply_management(&management(mgmt::FAILED_NO_LOGIN, 1, 0, "Inconnu"));
        let n = n.unwrap();
        assert_eq!(n.message(&s), "Inconnu n'est pas en jeu.");
    }

    #[test]
    fn paquets_envoyes() {
        let (schema, values) = command_packet(&PartyCommand::Invite { cid: None, name: Some("Bob".into()) }, 7);
        let p = incoming(schema, &values);
        let m = decode_management(&p).unwrap();
        assert_eq!((m.op, m.author, m.target, m.name.as_str()), (mgmt::INVITE, Some(7), None, "Bob"));
        let (schema, values) = command_packet(&PartyCommand::Accept(7), 9);
        let m = decode_management(&incoming(schema, &values)).unwrap();
        assert_eq!((m.op, m.author, m.target), (mgmt::INVITE_ACCEPT, Some(9), Some(7)));
        let (schema, values) = command_packet(&PartyCommand::SetItemDivision(ITEM_FREE), 7);
        let p = decode_party(&incoming(schema, &values)).unwrap();
        assert_eq!((p.op, p.item_division), (op::UPDATE_ITEM_DIVISION, Some(ITEM_FREE)));
    }
}
