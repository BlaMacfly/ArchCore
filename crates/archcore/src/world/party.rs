//! Groupes (AGPMPARTY 0x08, gestion AGPMPARTYMANAGEMENT 0x09).
//!
//! ```text
//!   Gestion (0x09, drapeaux 1) : 0 opération, 1 CID de l'auteur, 2 CID de la cible, 3 nom (49 octets).
//!     A → op 0 (cible par CID ou par nom)  B ← op 0 (auteur = A, cible = B, nom = A) ; A ← 10 (absent), 5 (déjà
//!                                           dans un autre groupe), 8 (A n'est pas chef), 7 (groupe plein)
//!     B → op 2 (cible = A) accepte          groupe formé ou agrandi ; A ← op 2 (nom = B) ; B ← 6 s'il a déjà un groupe
//!     B → op 1 (cible = A) refuse           A ← op 1 (nom = B)
//!     → op 3 quitte ; chef → op 4 (cible) exclut, la cible ← op 4 ; chef → op 11 (cible) lui passe la main,
//!       le groupe ← op 11 (auteur = nouveau chef, cible = ancien)
//!   Groupe (0x08, drapeaux 2) : 0 opération, 1 identifiant, 2 membres au plus, 3 membres, 4 CID d'un membre,
//!     5 liste des CID (u32), 6 facteurs, 7 zone d'effet, 8 répartition de l'expérience, 9 bonus, 10 répartition
//!     des objets.
//!     Arrivée : le nouveau ← fiche de chaque autre membre, op 0 (groupe avec la liste) puis op 3 (membre) pour
//!     chacun ; les autres ← sa fiche et op 3 (lui). Départ : le groupe et lui ← op 4 (lui), lui ← op 1 ; un
//!     groupe réduit à un membre est dissous. Le chef règle l'expérience (op 5) et les objets (op 6).
//! ```
//! Cinq membres au plus, chef en tête ; seize invitations en attente par joueur. Les membres se voient à toute
//! distance ; un membre déconnecté quitte le groupe. Répartition de l'expérience d'un monstre tué (ajout
//! d'ArchCore, le serveur d'origine ne la décrit pas) : entre les membres vivants assez proches (5 × 5 secteurs
//! autour du monstre), selon les dégâts infligés (par défaut) ou selon le niveau.

use super::{presence, WorldState};
use crate::info;
use archcore_net::schema;
use archcore_net::{build_packet, Fields, Packet, Value};
use std::collections::HashMap;

pub const MAX_MEMBERS: usize = 5;
pub const MAX_INVITATIONS: usize = 16;
/// Premier identifiant de groupe.
const FIRST_ID: u32 = 100;

pub mod op {
    pub const ADD: i64 = 0;
    pub const REMOVE: i64 = 1;
    pub const ADD_MEMBER: i64 = 3;
    pub const REMOVE_MEMBER: i64 = 4;
    pub const UPDATE_EXP_TYPE: i64 = 5;
    pub const UPDATE_ITEM_DIVISION: i64 = 6;
}

pub mod mgmt {
    pub const INVITE: i64 = 0;
    pub const REJECT: i64 = 1;
    pub const INVITE_ACCEPT: i64 = 2;
    pub const LEAVE: i64 = 3;
    pub const BANISH: i64 = 4;
    pub const FAILED_OTHER_PARTY: i64 = 5;
    pub const FAILED_ALREADY_MEMBER: i64 = 6;
    pub const FAILED_FULL: i64 = 7;
    pub const FAILED_NOT_LEADER: i64 = 8;
    pub const FAILED_NO_LOGIN: i64 = 10;
    pub const DELEGATION_LEADER: i64 = 11;
    /// L'invité refuse les invitations de groupe (options) : « Option_Party_Off_User ».
    pub const FAILED_REFUSE: i64 = 14;
    /// Meurtrier (40 points ou plus) : l'invitant (« Party_Murderer_Error_Self »), l'invité
    /// (« Party_Murderer_Error_Target »), exclusion d'un membre devenu meurtrier (« Party_Murderer_Error_Leave »).
    pub const FAILED_MURDERER_SELF: i64 = 18;
    pub const FAILED_MURDERER_TARGET: i64 = 19;
    pub const LEAVE_BY_MURDERER: i64 = 20;
}

/// Répartition de l'expérience : selon les dégâts (défaut) ou selon le niveau.
pub const EXP_BY_DAMAGE: i64 = 0;
pub const EXP_BY_LEVEL: i64 = 1;
/// Répartition des objets : à tour de rôle, libre, selon les dégâts (défaut).
pub const ITEM_SEQUENCE: i64 = 0;
pub const ITEM_DAMAGE: i64 = 2;

#[derive(Clone, Debug, PartialEq)]
pub struct Party {
    pub id: u32,
    /// CID des membres, chef en tête.
    pub members: Vec<u32>,
    pub exp_type: i64,
    pub item_division: i64,
}

#[derive(Debug, Default)]
pub struct Parties {
    pub parties: HashMap<u32, Party>,
    /// CID → groupe.
    pub of: HashMap<u32, u32>,
    /// CID de l'auteur → CID invités en attente.
    pub invitations: HashMap<u32, Vec<u32>>,
    next_id: u32,
}

impl Parties {
    pub fn party_of(&self, cid: u32) -> Option<&Party> {
        self.of.get(&cid).and_then(|id| self.parties.get(id))
    }

    /// Deux personnages du même groupe.
    pub fn same(&self, a: u32, b: u32) -> bool {
        self.of.get(&a).is_some_and(|x| self.of.get(&b) == Some(x))
    }

    fn invite(&mut self, from: u32, to: u32) -> bool {
        let list = self.invitations.entry(from).or_default();
        if list.contains(&to) {
            return true;
        }
        if list.len() >= MAX_INVITATIONS {
            return false;
        }
        list.push(to);
        true
    }

    /// Consomme l'invitation de `from` à `to` ; faux si elle n'existait pas.
    fn consume(&mut self, from: u32, to: u32) -> bool {
        let Some(list) = self.invitations.get_mut(&from) else { return false };
        let Some(i) = list.iter().position(|c| *c == to) else { return false };
        list.remove(i);
        true
    }

    /// Nouveau groupe de deux membres (chef d'abord).
    fn form(&mut self, leader: u32, member: u32) -> u32 {
        self.next_id = self.next_id.max(FIRST_ID) + 1;
        let id = self.next_id;
        self.parties.insert(id, Party { id, members: vec![leader, member], exp_type: EXP_BY_DAMAGE, item_division: ITEM_DAMAGE });
        self.of.insert(leader, id);
        self.of.insert(member, id);
        id
    }
}

/// AGPMPARTY : paquet de groupe.
fn party_packet(kind: i64, party: &Party, member: Option<u32>, full: bool) -> Vec<u8> {
    let s = &schema::PARTY;
    let mut f = Fields::new(s).with(0, kind).with(1, party.id);
    if full {
        let list: Vec<u8> = party.members.iter().flat_map(|c| c.to_le_bytes()).collect();
        f = f.with(2, MAX_MEMBERS as i64).with(3, party.members.len() as i64).with(5, Value::Bytes(list)).with(8, party.exp_type).with(10, party.item_division);
    }
    if let Some(c) = member {
        f = f.with(4, c);
    }
    if kind == op::UPDATE_EXP_TYPE {
        f = f.with(4, party.members[0]).with(8, party.exp_type);
    }
    if kind == op::UPDATE_ITEM_DIVISION {
        f = f.with(4, party.members[0]).with(10, party.item_division);
    }
    build_packet(s, &f)
}

/// AGPMPARTYMANAGEMENT : paquet de gestion.
pub fn management_packet(kind: i64, author: Option<u32>, target: Option<u32>, name: Option<&str>) -> Vec<u8> {
    let s = &schema::PARTY_MANAGEMENT;
    let mut f = Fields::new(s).with(0, kind);
    if let Some(a) = author {
        f = f.with(1, a);
    }
    if let Some(t) = target {
        f = f.with(2, t);
    }
    if let Some(n) = name {
        f = f.with(3, n);
    }
    build_packet(s, &f)
}

fn sid_of(st: &WorldState, cid: u32) -> Option<u32> {
    st.by_cid.get(&cid).copied().filter(|sid| st.players.get(sid).is_some_and(|p| p.in_game))
}

fn send(st: &WorldState, cid: u32, plain: Vec<u8>) {
    if let Some(p) = sid_of(st, cid).and_then(|sid| st.players.get(&sid)) {
        p.send_raw(plain);
    }
}

fn send_party(st: &WorldState, party: &Party, plain: &[u8], except: Option<u32>) {
    for &c in party.members.iter().filter(|c| Some(**c) != except) {
        send(st, c, plain.to_vec());
    }
}

/// Cible d'un paquet de gestion : par CID (champ 2), sinon par nom (champ 3).
fn target_of(st: &WorldState, p: &Packet) -> Option<u32> {
    if let Some(cid) = p.int(2).filter(|c| *c != 0) {
        return sid_of(st, cid as u32).map(|_| cid as u32);
    }
    let name = p.text(3).filter(|n| !n.is_empty())?;
    st.by_name(&name).filter(|sid| st.players[sid].in_game).map(|sid| st.players[&sid].ch.id)
}

fn name_of(st: &WorldState, cid: u32) -> String {
    sid_of(st, cid).map(|sid| st.players[&sid].ch.name.clone()).unwrap_or_default()
}

/// AGPMPARTYMANAGEMENT reçu.
pub fn on_management(st: &mut WorldState, sid: u32, p: &Packet) {
    let Some(me) = st.players.get(&sid).filter(|x| x.in_game).map(|x| x.ch.id) else { return };
    let my_name = name_of(st, me);
    match p.int(0) {
        Some(mgmt::INVITE) => {
            let Some(target) = target_of(st, p) else {
                send(st, me, management_packet(mgmt::FAILED_NO_LOGIN, Some(me), None, p.text(3).as_deref()));
                return;
            };
            if target == me {
                return;
            }
            let target_name = name_of(st, target);
            // Invitations refusées dans les options, ou auteur ignoré avec « Block Invites » (buddy.rs).
            let refuses = |t: &u32| {
                super::option_flags::refuses(st, *t, super::option_flags::REFUSE_PARTY_IN) || super::buddy::blocks(st, *t, me, schema::buddy_flag::BLOCK_INVITE)
            };
            if st.by_cid.get(&target).is_some_and(refuses) {
                info!("[groupe] {target_name} refuse les invitations de {my_name}");
                send(st, me, management_packet(mgmt::FAILED_REFUSE, Some(me), Some(target), Some(&target_name)));
                return;
            }
            let murderer = |cid: u32| st.by_cid.get(&cid).and_then(|s| st.players.get(s)).is_some_and(|p| super::pvp::murderer_level(p.ch.murderer) >= 1);
            if murderer(me) {
                send(st, me, management_packet(mgmt::FAILED_MURDERER_SELF, Some(me), Some(target), Some(&target_name)));
                return;
            }
            if murderer(target) {
                send(st, me, management_packet(mgmt::FAILED_MURDERER_TARGET, Some(me), Some(target), Some(&target_name)));
                return;
            }
            if st.parties.of.contains_key(&target) {
                send(st, me, management_packet(mgmt::FAILED_OTHER_PARTY, Some(me), Some(target), Some(&target_name)));
                return;
            }
            if let Some(party) = st.parties.party_of(me) {
                if party.members[0] != me {
                    send(st, me, management_packet(mgmt::FAILED_NOT_LEADER, Some(me), Some(target), Some(&target_name)));
                    return;
                }
                if party.members.len() >= MAX_MEMBERS {
                    send(st, me, management_packet(mgmt::FAILED_FULL, Some(me), Some(target), Some(&target_name)));
                    return;
                }
            }
            if !st.parties.invite(me, target) {
                return;
            }
            info!("[groupe] {my_name} invite {target_name}");
            send(st, target, management_packet(mgmt::INVITE, Some(me), Some(target), Some(&my_name)));
        }
        Some(mgmt::REJECT) => {
            let Some(inviter) = target_of(st, p) else { return };
            if !st.parties.consume(inviter, me) {
                return;
            }
            info!("[groupe] {my_name} refuse l'invitation de {}", name_of(st, inviter));
            send(st, inviter, management_packet(mgmt::REJECT, Some(inviter), Some(me), Some(&my_name)));
        }
        Some(mgmt::INVITE_ACCEPT) => {
            let Some(inviter) = target_of(st, p) else { return };
            if !st.parties.consume(inviter, me) {
                return;
            }
            if st.parties.of.contains_key(&me) {
                send(st, me, management_packet(mgmt::FAILED_ALREADY_MEMBER, Some(inviter), Some(me), Some(&my_name)));
                return;
            }
            match st.parties.of.get(&inviter).copied() {
                Some(id) => {
                    if st.parties.parties[&id].members.len() >= MAX_MEMBERS {
                        send(st, me, management_packet(mgmt::FAILED_FULL, Some(inviter), Some(me), Some(&my_name)));
                        return;
                    }
                    st.parties.parties.get_mut(&id).unwrap().members.push(me);
                    st.parties.of.insert(me, id);
                    join(st, id, me, false);
                }
                None => {
                    let id = st.parties.form(inviter, me);
                    join(st, id, inviter, true);
                    join(st, id, me, true);
                }
            }
            info!("[groupe] {my_name} rejoint le groupe de {}", name_of(st, inviter));
            send(st, inviter, management_packet(mgmt::INVITE_ACCEPT, Some(inviter), Some(me), Some(&my_name)));
        }
        Some(mgmt::LEAVE) => {
            if st.parties.of.contains_key(&me) {
                remove_member(st, me);
            }
        }
        Some(mgmt::BANISH) => {
            let Some(target) = target_of(st, p) else { return };
            if !st.parties.same(me, target) || st.parties.party_of(me).map(|x| x.members[0]) != Some(me) || target == me {
                return;
            }
            info!("[groupe] {my_name} exclut {}", name_of(st, target));
            remove_member(st, target);
            send(st, target, management_packet(mgmt::BANISH, Some(target), None, None));
        }
        Some(mgmt::DELEGATION_LEADER) => {
            let Some(target) = target_of(st, p) else { return };
            if !st.parties.same(me, target) || st.parties.party_of(me).map(|x| x.members[0]) != Some(me) || target == me {
                return;
            }
            let id = st.parties.of[&me];
            let party = st.parties.parties.get_mut(&id).unwrap();
            if let Some(i) = party.members.iter().position(|c| *c == target) {
                party.members.swap(0, i);
            }
            let party = party.clone();
            info!("[groupe] {my_name} passe la main à {}", name_of(st, target));
            send_party(st, &party, &management_packet(mgmt::DELEGATION_LEADER, Some(target), Some(me), None), None);
        }
        other => crate::debug!("[groupe] #{sid} gestion op {other:?} (ignorée)"),
    }
}

/// AGPMPARTY reçu : réglages du chef.
pub fn on_party(st: &mut WorldState, sid: u32, p: &Packet) {
    let Some(me) = st.players.get(&sid).map(|x| x.ch.id) else { return };
    let Some(id) = st.parties.of.get(&me).copied() else { return };
    let party = st.parties.parties.get_mut(&id).unwrap();
    if party.members[0] != me {
        return;
    }
    match p.int(0) {
        Some(op::UPDATE_EXP_TYPE) => {
            let Some(t) = p.int(8).filter(|t| matches!(*t, EXP_BY_DAMAGE | EXP_BY_LEVEL)) else { return };
            party.exp_type = t;
        }
        Some(op::UPDATE_ITEM_DIVISION) => {
            let Some(t) = p.int(10).filter(|t| (ITEM_SEQUENCE..=ITEM_DAMAGE).contains(t)) else { return };
            party.item_division = t;
        }
        _ => return,
    }
    let party = party.clone();
    let kind = p.int(0).unwrap();
    send_party(st, &party, &party_packet(kind, &party, None, false), None);
}

/// Annonces d'une arrivée dans le groupe `id` (référence : rappel « rejoint »).
fn join(st: &mut WorldState, id: u32, cid: u32, newly_formed: bool) {
    let party = st.parties.parties[&id].clone();
    let Some(sid) = sid_of(st, cid) else { return };
    for &other in party.members.iter().filter(|c| **c != cid) {
        let Some(osid) = sid_of(st, other) else { continue };
        if !newly_formed {
            presence::set_visible(st, osid, sid, true);
            send(st, other, party_packet(op::ADD_MEMBER, &party, Some(cid), false));
        }
        presence::set_visible(st, sid, osid, true);
    }
    send(st, cid, party_packet(op::ADD, &party, None, true));
    for &m in &party.members {
        send(st, cid, party_packet(op::ADD_MEMBER, &party, Some(m), false));
    }
}

/// Retire `cid` de son groupe (départ, exclusion, déconnexion) ; un groupe réduit à un membre est dissous.
pub fn remove_member(st: &mut WorldState, cid: u32) {
    let Some(id) = st.parties.of.remove(&cid) else { return };
    st.parties.invitations.remove(&cid);
    let party = st.parties.parties.get_mut(&id).unwrap();
    party.members.retain(|c| *c != cid);
    let party = party.clone();
    let leave = party_packet(op::REMOVE_MEMBER, &party, Some(cid), false);
    send_party(st, &party, &leave, None);
    send(st, cid, leave);
    send(st, cid, party_packet(op::REMOVE, &party, None, false));
    // Les membres hors de vue ne se voient plus.
    if let Some(sid) = sid_of(st, cid) {
        for &other in &party.members {
            let Some(osid) = sid_of(st, other) else { continue };
            let near = super::sector::keeps(st.players[&sid].view_pos, st.players[&osid].view_pos);
            presence::set_visible(st, sid, osid, near);
            presence::set_visible(st, osid, sid, near);
        }
    }
    info!("[groupe] {} quitte le groupe {id} ({} membre(s) restant(s))", name_of(st, cid), party.members.len());
    match party.members.len() {
        0 => {
            st.parties.parties.remove(&id);
        }
        1 => remove_member(st, party.members[0]),
        _ => {}
    }
}

/// Un membre devenu meurtrier est exclu de son groupe (« Party_Murderer_Error_Leave »).
pub fn expel_murderer(st: &mut WorldState, sid: u32) {
    let Some(cid) = st.players.get(&sid).map(|p| p.ch.id) else { return };
    if !st.parties.of.contains_key(&cid) {
        return;
    }
    send(st, cid, management_packet(mgmt::LEAVE_BY_MURDERER, Some(cid), None, None));
    remove_member(st, cid);
}

/// Déconnexion : le joueur quitte son groupe et ses invitations tombent.
pub fn on_leave_world(st: &mut WorldState, cid: u32) {
    st.parties.invitations.remove(&cid);
    if st.parties.of.contains_key(&cid) {
        remove_member(st, cid);
    }
}

/// Part de l'expérience `gain` d'un monstre tué en `pos` revenant à chaque membre du groupe du tueur : membres
/// vivants assez proches, selon les dégâts infligés (`damage` : CID → dégâts) ou le niveau. Sans groupe, tout au
/// tueur. Rend (session, part) ; la somme vaut `gain` (arrondi au profit du tueur).
pub fn share_exp(st: &WorldState, killer_sid: u32, gain: u64, pos: archcore_net::Vec3, damage: &HashMap<u32, u64>) -> Vec<(u32, u64)> {
    let Some(killer) = st.players.get(&killer_sid).map(|p| p.ch.id) else { return vec![(killer_sid, gain)] };
    let Some(party) = st.parties.party_of(killer) else { return vec![(killer_sid, gain)] };
    let present: Vec<(u32, u32)> = party
        .members
        .iter()
        .filter_map(|&c| sid_of(st, c).map(|sid| (c, sid)))
        .filter(|(_, sid)| {
            let p = &st.players[sid];
            !p.dead && super::sector::keeps(p.view_pos, pos)
        })
        .collect();
    let weight = |c: u32, sid: u32| -> f64 {
        if party.exp_type == EXP_BY_LEVEL {
            st.players[&sid].factors.status[7].max(1) as f64
        } else {
            damage.get(&c).copied().unwrap_or(0) as f64
        }
    };
    let total: f64 = present.iter().map(|(c, sid)| weight(*c, *sid)).sum();
    if present.len() <= 1 || total <= 0.0 {
        return vec![(killer_sid, gain)];
    }
    let mut out: Vec<(u32, u64)> = present.iter().map(|(c, sid)| (*sid, (gain as f64 * weight(*c, *sid) / total).floor() as u64)).collect();
    let given: u64 = out.iter().map(|(_, g)| g).sum();
    match out.iter_mut().find(|(sid, _)| *sid == killer_sid) {
        Some(k) => k.1 += gain - given,
        None => out.push((killer_sid, gain - given)),
    }
    out.retain(|(_, g)| *g > 0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invitations_et_formation() {
        let mut p = Parties::default();
        assert!(p.invite(1, 2));
        assert!(p.invite(1, 2), "deux fois la même invitation");
        assert!(!p.consume(1, 3));
        assert!(p.consume(1, 2));
        assert!(!p.consume(1, 2), "consommée");
        let id = p.form(1, 2);
        assert_eq!(id, FIRST_ID + 1);
        assert!(p.same(1, 2) && !p.same(1, 3));
        assert_eq!(p.party_of(2).unwrap().members, vec![1, 2]);
        for i in 0..MAX_INVITATIONS as u32 {
            assert!(p.invite(5, 100 + i));
        }
        assert!(!p.invite(5, 999), "seize invitations au plus");
    }

    #[test]
    fn paquets_de_groupe() {
        let party = Party { id: 101, members: vec![7, 9], exp_type: EXP_BY_DAMAGE, item_division: ITEM_DAMAGE };
        let p = archcore_net::decode_packet(&schema::PARTY, &party_packet(op::ADD, &party, None, true)).unwrap();
        assert_eq!((p.int(0), p.int(1), p.int(2), p.int(3)), (Some(0), Some(101), Some(5), Some(2)));
        assert_eq!(p.bytes(5).unwrap(), &[7, 0, 0, 0, 9, 0, 0, 0]);
        let m = archcore_net::decode_packet(&schema::PARTY_MANAGEMENT, &management_packet(mgmt::INVITE, Some(7), Some(9), Some("Alice"))).unwrap();
        assert_eq!((m.int(0), m.int(1), m.int(2), m.text(3).as_deref()), (Some(0), Some(7), Some(9), Some("Alice")));
    }
}
