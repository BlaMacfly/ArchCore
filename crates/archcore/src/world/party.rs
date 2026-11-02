//! Groupes (AGPMPARTY 0x08, gestion AGPMPARTYMANAGEMENT 0x09).
//!
//! ```text
//!   Gestion (0x09, drapeaux 1) : 0 opération, 1 CID de l'auteur, 2 CID de la cible, 3 nom (49 octets).
//!     A → op 0 (cible par CID ou par nom)  B ← op 0 (auteur = A, cible = B, nom = A) ; A ← 10 (absent), 5 (déjà
//!                                           dans un autre groupe), 8 (A n'est pas chef), 7 (groupe plein)
//!     B → op 2 (cible = A) accepte          groupe formé ou agrandi ; A ← op 2 (nom = B) ; B ← 6 s'il a déjà un groupe
//!     B → op 1 (cible = A) refuse           A ← op 1 (nom = B)
//!     → op 3 quitte ; chef → op 4 (cible) exclut, la cible ← op 4 ; chef → op 11 (cible) lui passe la main,
//!       le groupe ← op 11 (auteur = nouveau chef, cible = ancien) ; refus ← op 12 (« Failed to change party leader »)
//!     Échecs : ← op 9 (« You cannot form a party. » : région sans groupes, invitations en attente trop nombreuses) ;
//!     ← op 13 quand l'invité a au moins `Party.InviteLevelGap` (10) niveaux de moins que l'invitant (texte du client)
//!   Groupe (0x08, drapeaux 2) : 0 opération, 1 identifiant, 2 membres au plus, 3 membres, 4 CID d'un membre,
//!     5 liste des CID (u32), 6 facteurs, 7 zone d'effet, 8 répartition de l'expérience, 9 bonus, 10 répartition
//!     des objets.
//!     Arrivée : le nouveau ← fiche de chaque autre membre, op 0 (groupe avec la liste) puis op 3 (membre) pour
//!     chacun ; les autres ← sa fiche et op 3 (lui). Départ : le groupe et lui ← op 4 (lui), lui ← op 1 ; un
//!     groupe réduit à un membre est dissous. Le chef règle l'expérience (op 5) et les objets (op 6).
//!     Op 2 (mise à jour) : champ 7 = zone d'effet (mini de 5 octets, un par membre dans l'ordre de la liste : 1 à portée
//!     du chef), champ 9 = bonus du groupe (mini : dégâts et défense en pour cent, PV et PM max) ; envoyée aux membres
//!     à chaque changement. Le client montre l'icône de zone selon la classe du chef et les textes `Party_Add_*`
//!     (`Party_Add_None` quand le bonus est nul).
//! ```
//! Zone et bonus (règles d'origine inconnues, réglables) : un membre est dans la zone à moins de `Party.EffectRange`
//! unités du chef (en jeu, même mort) ; avec n membres dans la zone, chacun d'eux reçoit (n − 1) ×
//! `Party.BonusPercentPerMember` pour cent de défense si le chef est chevalier, de dégâts physiques s'il est archer (ou
//! scion), ou (n − 1) × `Party.BonusPointsPerMember` PV et PM max s'il est mage ; rien quand l'écart de niveau des membres
//! en jeu dépasse `Party.BonusMaxLevelGap`. Le bonus s'ajoute aux facteurs (`equipment::recompute_factors`) ; il est
//! revu une fois par seconde (`step`).
//! Cinq membres au plus, chef en tête ; seize invitations en attente par joueur. Les membres se voient à toute
//! distance ; un membre déconnecté quitte le groupe. Répartition de l'expérience d'un monstre tué (ajout
//! d'ArchCore, le serveur d'origine ne la décrit pas) : entre les membres vivants assez proches (5 × 5 secteurs
//! autour du monstre), selon les dégâts infligés (par défaut) ou selon le niveau.
//!
//! Rappel de groupe (épisode 8, orbes d'usage 15 : Summoning Orb, Bead of Summoning…) :
//! ```text
//!   lanceur → AGPMITEM op 3 (IID)        un exemplaire consommé ; lanceur ← AGPMITEM op 4 (3 TID, 4 CID, 18 recharge)
//!                                        (« PartyUseRecallSuccess ») ; chaque autre membre en jeu ← gestion op 15
//!                                        (1 lanceur, 3 son nom) : fenêtre `UI_ConfirmRecall`
//!   membre → op 16 (1 lui, 2 lanceur)     accepte (précédé de l'annulation de son échange, 0x07 op 5) : téléporté près
//!                                        du lanceur ; lanceur ← op 16 (1 membre, 3 son nom)
//!   membre → op 17 (1 lui, 2 lanceur)     refuse ; lanceur ← op 17 (1 membre, 3 son nom)
//! ```
//! Le client ne contrôle ni la région, ni le combat, ni la distance. Règles d'ArchCore (celles d'origine ne sont pas
//! connues), les mêmes pour le lanceur à l'usage et pour les deux à l'acceptation : refus au mort, en zone de siège en
//! guerre, dans un Shrine en combat, dans la Battle Square et les Battlegrounds, dans une région sans le bit 0x100 du
//! Type (celui de la résurrection : prison, tribune…), en combat sauf `Recall.AllowInCombat`. À l'usage, refus aussi
//! sans autre membre en jeu, pendant la recharge (« Use Interval ») ou hors des niveaux de l'objet ; refus à l'usage :
//! AGPMITEM op 5. La demande vaut `Recall.RequestSeconds` ; le membre est rappelé à toute distance. Refus aussi dans une
//! région « DisableSummon » de RegionTemplate.ini (`regions.rs`). Une région « DisableParty » refuse les invitations et
//! leur acceptation à qui s'y trouve (message système).

use super::{presence, WorldCtx, WorldState};
use crate::{debug, info};
use archcore_net::schema::{self, mini};
use archcore_net::{build_mini, build_packet, Fields, Packet, Value};
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub const MAX_MEMBERS: usize = 5;
pub const MAX_INVITATIONS: usize = 16;
/// Premier identifiant de groupe.
const FIRST_ID: u32 = 100;

pub mod op {
    pub const ADD: i64 = 0;
    pub const REMOVE: i64 = 1;
    /// Zone d'effet et bonus du groupe.
    pub const UPDATE: i64 = 2;
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
    /// Échec sans autre précision : « PartyRecvOperationFailed ».
    pub const FAILED: i64 = 9;
    pub const FAILED_NO_LOGIN: i64 = 10;
    pub const DELEGATION_LEADER: i64 = 11;
    /// Passage de chef refusé : « Party_DelegationLeaderFailed ».
    pub const DELEGATION_LEADER_FAILED: i64 = 12;
    /// Invité trop bas : « Party_InviteMemeberLevelDiff ».
    pub const INVITE_FAILED_LEVEL_DIFF: i64 = 13;
    /// L'invité refuse les invitations de groupe (options) : « Option_Party_Off_User ».
    pub const FAILED_REFUSE: i64 = 14;
    /// Meurtrier (40 points ou plus) : l'invitant (« Party_Murderer_Error_Self »), l'invité
    /// (« Party_Murderer_Error_Target »), exclusion d'un membre devenu meurtrier (« Party_Murderer_Error_Leave »).
    pub const FAILED_MURDERER_SELF: i64 = 18;
    pub const FAILED_MURDERER_TARGET: i64 = 19;
    pub const LEAVE_BY_MURDERER: i64 = 20;
    /// Rappel de groupe : demande (serveur → membres), acceptation et refus (membre → serveur, puis serveur → lanceur).
    pub const REQUEST_RECALL: i64 = 15;
    pub const ACCEPT_RECALL: i64 = 16;
    pub const REJECT_RECALL: i64 = 17;
}

/// Type d'usage (« Type » d'un objet d'« ItemType » 1) des orbes de rappel : Summoning Orb, Bead of Summoning…
pub const USABLE_RECALL_PARTY: i64 = 15;
/// Décalage du membre rappelé par rapport au lanceur (unités du monde), pour ne pas le poser dessus.
pub const RECALL_OFFSET: f64 = 100.0;

/// Rappel en attente de la réponse d'un membre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Recall {
    /// CID du lanceur.
    pub leader: u32,
    pub until: Instant,
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
    /// Zone d'effet et bonus envoyés en dernier (op 2), `None` avant le premier envoi.
    pub effect: Option<(Vec<bool>, Bonus)>,
}

/// Bonus du groupe (champ 9 de l'op 2) : dégâts physiques et défense physique en pour cent, PV et PM max en plus.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bonus {
    pub damage: i64,
    pub defense: i64,
    pub hp: i64,
    pub mp: i64,
}

impl Bonus {
    pub fn is_none(&self) -> bool {
        *self == Bonus::default()
    }

    /// Ajoute le bonus aux facteurs d'un membre dans la zone.
    pub fn apply(&self, f: &mut crate::character::Factors) {
        if self.is_none() {
            return;
        }
        let pct = |v: i32, p: i64| v + (v as i64 * p / 100) as i32;
        f.damage_min[0] = pct(f.damage_min[0], self.damage);
        f.damage_max[0] = pct(f.damage_max[0], self.damage);
        f.defense_point[0] = pct(f.defense_point[0], self.defense);
        f.point_max[0] += self.hp as i32;
        f.point_max[1] += self.mp as i32;
    }
}

/// Classes du chef (champ 2 du type de personnage) qui donnent la défense et les PV/PM ; les autres, les dégâts.
const CLASS_KNIGHT: i32 = 1;
const CLASS_MAGE: i32 = 4;
/// Revue de la zone et du bonus des groupes.
const EFFECT_EVERY: Duration = Duration::from_secs(1);

/// Bonus d'un groupe dont `in_zone` membres sont dans la zone, selon la classe du chef et l'écart de niveau des
/// membres en jeu (`levels`).
pub fn bonus_of(leader_class: i32, in_zone: usize, levels: &[i32], percent: i64, points: i64, max_gap: i32) -> Bonus {
    let gap = levels.iter().max().copied().unwrap_or(0) - levels.iter().min().copied().unwrap_or(0);
    let k = in_zone.saturating_sub(1) as i64;
    if k == 0 || gap > max_gap {
        return Bonus::default();
    }
    match leader_class {
        CLASS_KNIGHT => Bonus { defense: (k * percent).min(i8::MAX as i64), ..Default::default() },
        CLASS_MAGE => Bonus { hp: (k * points).min(i16::MAX as i64), mp: (k * points).min(i16::MAX as i64), ..Default::default() },
        _ => Bonus { damage: (k * percent).min(i8::MAX as i64), ..Default::default() },
    }
}

/// L'invité (niveau `target`) est trop bas pour l'invitant (niveau `inviter`) : au moins `gap` niveaux de moins
/// (`gap` 0 : jamais).
pub fn level_gap_refused(inviter: i32, target: i32, gap: i32) -> bool {
    gap > 0 && target <= inviter - gap
}

#[derive(Debug, Default)]
pub struct Parties {
    pub parties: HashMap<u32, Party>,
    /// CID → groupe.
    pub of: HashMap<u32, u32>,
    /// CID de l'auteur → CID invités en attente.
    pub invitations: HashMap<u32, Vec<u32>>,
    /// CID d'un membre → rappel qui attend sa réponse (le dernier reçu).
    pub recalls: HashMap<u32, Recall>,
    next_id: u32,
    /// Dernière revue de la zone d'effet et du bonus.
    effect_at: Option<Instant>,
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

    /// Rappel attendu par `member` de la part de `leader`, encore valable : consommé.
    pub fn take_recall(&mut self, member: u32, leader: u32, now: Instant) -> bool {
        match self.recalls.get(&member) {
            Some(r) if r.leader == leader => {
                let fresh = r.until > now;
                self.recalls.remove(&member);
                fresh
            }
            _ => false,
        }
    }

    /// Nouveau groupe de deux membres (chef d'abord).
    fn form(&mut self, leader: u32, member: u32) -> u32 {
        self.next_id = self.next_id.max(FIRST_ID) + 1;
        let id = self.next_id;
        self.parties.insert(id, Party { id, members: vec![leader, member], exp_type: EXP_BY_DAMAGE, item_division: ITEM_DAMAGE, effect: None });
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

/// AGPMPARTY op 2 : zone d'effet (un octet par membre) et bonus du groupe.
pub fn update_packet(id: u32, zone: &[bool], bonus: &Bonus) -> Vec<u8> {
    let (z, b) = (&mini::PARTY_EFFECT_AREA, &mini::PARTY_BONUS);
    let zone = build_mini(z, &(0..MAX_MEMBERS).fold(Fields::new(z), |f, i| f.with(i, zone.get(i).copied().unwrap_or(false) as i64)));
    let bonus = build_mini(b, &Fields::new(b).with(0, bonus.damage).with(1, bonus.defense).with(2, bonus.hp).with(3, bonus.mp));
    let s = &schema::PARTY;
    build_packet(s, &Fields::new(s).with(0, op::UPDATE).with(1, id).with(7, zone).with(9, bonus))
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

/// Le joueur `cid` se trouve dans une région qui interdit ce que disent `bits` (`regions::disable`).
pub fn region_forbids(ctx: &WorldCtx, st: &WorldState, cid: u32, bits: u32) -> bool {
    st.by_cid.get(&cid).and_then(|s| st.players.get(s)).is_some_and(|p| ctx.regions.forbids(ctx.map.region(p.ch.pos_x, p.ch.pos_z), bits))
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
pub fn on_management(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) {
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
            // Région sans groupes (DisableParty), pour l'un ou l'autre : op 9.
            if [me, target].iter().any(|c| region_forbids(ctx, st, *c, super::regions::disable::PARTY)) {
                info!("[groupe] {my_name} : invitation refusée (région sans groupes)");
                send(st, me, management_packet(mgmt::FAILED, Some(me), Some(target), None));
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
            // Invité trop bas (au moins `Party.InviteLevelGap` niveaux de moins) : op 13.
            let level = |cid: u32| st.by_cid.get(&cid).and_then(|s| st.players.get(s)).map_or(1, |p| p.ch.level);
            if level_gap_refused(level(me), level(target), ctx.config.world.party_invite_level_gap) {
                info!("[groupe] {my_name} ne peut pas inviter {target_name} (niveau {} contre {})", level(target), level(me));
                send(st, me, management_packet(mgmt::INVITE_FAILED_LEVEL_DIFF, Some(me), Some(target), Some(&target_name)));
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
                send(st, me, management_packet(mgmt::FAILED, Some(me), Some(target), None));
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
            if [me, inviter].iter().any(|c| region_forbids(ctx, st, *c, super::regions::disable::PARTY)) {
                info!("[groupe] {my_name} : acceptation refusée (région sans groupes)");
                send(st, me, management_packet(mgmt::FAILED, Some(inviter), Some(me), None));
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
            // Refus (pas chef, cible absente ou hors du groupe) : op 12.
            let target = target_of(st, p);
            let leader = st.parties.party_of(me).map(|x| x.members[0]) == Some(me);
            let Some(target) = target.filter(|t| leader && *t != me && st.parties.same(me, *t)) else {
                info!("[groupe] {my_name} : passage de chef refusé");
                send(st, me, management_packet(mgmt::DELEGATION_LEADER_FAILED, Some(me), target.or(p.int(2).map(|c| c as u32)), None));
                return;
            };
            let id = st.parties.of[&me];
            let party = st.parties.parties.get_mut(&id).unwrap();
            if let Some(i) = party.members.iter().position(|c| *c == target) {
                party.members.swap(0, i);
            }
            let party = party.clone();
            info!("[groupe] {my_name} passe la main à {}", name_of(st, target));
            send_party(st, &party, &management_packet(mgmt::DELEGATION_LEADER, Some(target), Some(me), None), None);
        }
        Some(mgmt::REJECT_RECALL) => {
            let Some(leader) = p.int(2).map(|c| c as u32) else { return };
            if p.int(1).is_some_and(|c| c != me as i64) || !st.parties.take_recall(me, leader, Instant::now()) {
                return;
            }
            info!("[groupe] {my_name} refuse le rappel de {}", name_of(st, leader));
            send(st, leader, management_packet(mgmt::REJECT_RECALL, Some(me), Some(leader), Some(&my_name)));
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
    // Les rappels qu'il attendait ou qu'il avait lancés tombent.
    st.parties.recalls.retain(|m, r| *m != cid && r.leader != cid);
    let party = st.parties.parties.get_mut(&id).unwrap();
    party.members.retain(|c| *c != cid);
    // Zone et bonus revus pour les restants, et retirés au partant, au prochain tour (`step`).
    party.effect = None;
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

// ---------------------------------------------------------------------------------------------
// Zone d'effet et bonus du groupe
// ---------------------------------------------------------------------------------------------

/// Zone d'effet (un drapeau par membre, dans l'ordre de la liste) et bonus d'un groupe.
fn effect_of(ctx: &WorldCtx, st: &WorldState, party: &Party) -> (Vec<bool>, Bonus) {
    let w = &ctx.config.world;
    let player = |c: u32| sid_of(st, c).and_then(|s| st.players.get(&s));
    let Some(leader) = party.members.first().and_then(|c| player(*c)) else { return (vec![false; party.members.len()], Bonus::default()) };
    let center = leader.ch.pos();
    let zone: Vec<bool> = party.members.iter().map(|c| player(*c).is_some_and(|p| super::combat::dist2d(p.ch.pos(), center) <= w.party_effect_range)).collect();
    let levels: Vec<i32> = party.members.iter().filter_map(|c| player(*c)).map(|p| p.ch.level).collect();
    let n = zone.iter().filter(|z| **z).count();
    (zone, bonus_of(leader.factors.ftype[2], n, &levels, w.party_bonus_percent, w.party_bonus_points, w.party_bonus_max_level_gap))
}

/// Une fois par seconde : zone d'effet et bonus de chaque groupe ; op 2 aux membres s'ils changent, bonus porté par les
/// membres dans la zone (facteurs recalculés), retiré aux autres et à ceux qui n'ont plus de groupe.
pub fn step(ctx: &WorldCtx, st: &mut WorldState, now: Instant) {
    if st.parties.effect_at.is_some_and(|t| now.duration_since(t) < EFFECT_EVERY) {
        return;
    }
    st.parties.effect_at = Some(now);
    let mut want: HashMap<u32, Bonus> = HashMap::new();
    let ids: Vec<u32> = st.parties.parties.keys().copied().collect();
    for id in ids {
        let party = st.parties.parties[&id].clone();
        let (zone, bonus) = effect_of(ctx, st, &party);
        for (c, z) in party.members.iter().zip(&zone) {
            if let Some(sid) = sid_of(st, *c) {
                want.insert(sid, if *z { bonus } else { Bonus::default() });
            }
        }
        if party.effect.as_ref() != Some(&(zone.clone(), bonus)) {
            debug!("[groupe] groupe {id} : zone {zone:?}, bonus {bonus:?}");
            send_party(st, &party, &update_packet(id, &zone, &bonus), None);
            st.parties.parties.get_mut(&id).unwrap().effect = Some((zone, bonus));
        }
    }
    let changed: Vec<u32> = st.players.iter().filter(|(sid, p)| p.party_bonus != want.get(sid).copied().unwrap_or_default()).map(|(sid, _)| *sid).collect();
    for sid in changed {
        let b = want.get(&sid).copied().unwrap_or_default();
        st.players.get_mut(&sid).unwrap().party_bonus = b;
        super::equipment::refresh_factors(ctx, st, sid);
    }
}

// ---------------------------------------------------------------------------------------------
// Rappel de groupe
// ---------------------------------------------------------------------------------------------

/// Pourquoi le joueur `sid` ne peut ni rappeler ni être rappelé ici et maintenant, sinon `None`.
pub fn recall_refusal(ctx: &WorldCtx, st: &WorldState, sid: u32, now: Instant) -> Option<String> {
    let p = st.players.get(&sid)?;
    if p.dead {
        return Some("mort".into());
    }
    if super::siege::war::war_zone(ctx, st, sid).is_some() {
        return Some("zone de siège en guerre".into());
    }
    if super::shrine::active_shrine(ctx, st, sid).is_some() {
        return Some("Shrine en combat".into());
    }
    if super::battlesquare::in_battle_square(ctx, st, sid) || super::battlesquare::in_battleground(ctx, st, sid) {
        return Some("Battle Square".into());
    }
    if let Some(region) = ctx.map.region(p.ch.pos_x, p.ch.pos_z) {
        if ctx.regions.type_bits(region).is_some_and(|bits| bits & super::consumables::REGION_RESURRECTION == 0) {
            return Some(format!("région {region} fermée au rappel"));
        }
        // DisableSummon de RegionTemplate.ini.
        if ctx.regions.forbids(Some(region), super::regions::disable::SUMMON) {
            return Some(format!("région {region} sans invocation"));
        }
    }
    if !ctx.config.world.recall_allow_in_combat && now < p.timers.combat_until {
        return Some("en combat".into());
    }
    None
}

/// Paquet de la demande de rappel (op 15) envoyé à un membre.
pub fn recall_request_packet(leader: u32, leader_name: &str) -> Vec<u8> {
    management_packet(mgmt::REQUEST_RECALL, Some(leader), None, Some(leader_name))
}

/// Orbe de rappel utilisé (AGPMITEM op 3, usage 15) : un exemplaire consommé, op 4 au lanceur, op 15 aux autres membres.
pub async fn use_recall_item(ctx: &WorldCtx, st: &mut WorldState, sid: u32, item: &super::items::InventoryItem, now: Instant) -> anyhow::Result<()> {
    let Some(p) = st.players.get(&sid) else { return Ok(()) };
    let (cid, name) = (p.ch.id, p.ch.name.clone());
    let u = ctx.catalog.uses.get(&item.tid).cloned().unwrap_or_default();
    let others: Vec<u32> = st.parties.party_of(cid).map(|x| x.members.iter().copied().filter(|c| *c != cid && sid_of(st, *c).is_some()).collect()).unwrap_or_default();
    let refusal = if ctx.config.world.recall_excluded_items.contains(&item.tid) {
        Some("objet sans rappel".to_string())
    } else if others.is_empty() {
        Some("aucun autre membre du groupe en jeu".into())
    } else if let Some(left) = p.uses.left(item.tid, &[], now) {
        Some(format!("recharge ({} s)", left.as_secs().max(1)))
    } else if !u.level_ok(p.ch.level) {
        Some(format!("niveau {} hors de {:?}", p.ch.level, u.level))
    } else {
        recall_refusal(ctx, st, sid, now)
    };
    if let Some(why) = refusal {
        info!("[groupe] {name} : rappel refusé ({why})");
        p.send_raw(super::consumables::failed_packet(item.tid, cid));
        super::send_system(st, sid, &format!("Rappel du groupe impossible : {why}."));
        return Ok(());
    }
    let p = st.players.get_mut(&sid).unwrap();
    match p.inventory.consume(&ctx.db, item.iid, 1).await? {
        Some(left) => p.send_raw(p.inventory.count_packet(&left, cid)),
        None => p.send_raw(super::items::item_remove(item.iid, Some(cid))),
    }
    if u.reuse_ms > 0 {
        p.uses.item_ready.insert(item.tid, now + Duration::from_millis(u.reuse_ms));
    }
    p.send_raw(super::recovery::used_packet(item.tid, cid, Duration::from_millis(u.reuse_ms)));
    let until = now + Duration::from_secs(ctx.config.world.recall_request_secs);
    let request = recall_request_packet(cid, &name);
    for &m in &others {
        st.parties.recalls.insert(m, Recall { leader: cid, until });
        send(st, m, request.clone());
    }
    info!("[groupe] {name} rappelle son groupe ({} membre(s))", others.len());
    Ok(())
}

/// Op 16 reçue : le membre accepte le rappel ; téléporté près du lanceur, qui reçoit l'op 16.
pub async fn accept_recall(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some(me) = st.players.get(&sid).filter(|x| x.in_game).map(|x| x.ch.id) else { return Ok(()) };
    let Some(leader) = p.int(2).map(|c| c as u32) else { return Ok(()) };
    let now = Instant::now();
    if p.int(1).is_some_and(|c| c != me as i64) || !st.parties.take_recall(me, leader, now) {
        debug!("[groupe] #{sid} acceptation d'un rappel absent ou échu (lanceur {leader})");
        return Ok(());
    }
    let my_name = name_of(st, me);
    let lsid = sid_of(st, leader).filter(|_| st.parties.same(me, leader));
    let refusal = match lsid {
        None => Some("le lanceur n'est plus dans le groupe".to_string()),
        Some(l) => recall_refusal(ctx, st, l, now).map(|w| format!("lanceur : {w}")).or_else(|| recall_refusal(ctx, st, sid, now)),
    };
    let (Some(lsid), None) = (lsid, &refusal) else {
        let why = refusal.unwrap_or_default();
        info!("[groupe] rappel de {my_name} refusé ({why})");
        super::send_system(st, sid, &format!("Rappel impossible : {why}."));
        if let Some(l) = lsid {
            super::send_system(st, l, &format!("{my_name} ne peut pas être rappelé : {why}."));
        }
        return Ok(());
    };
    let target = st.players[&lsid].ch.pos();
    // Échange en cours annulé (le client l'annule aussi avant d'accepter), boutique refermée.
    super::trade::cancel(st, sid);
    super::npc_trade::close(st, sid);
    // Départ vers une position libre (AGPMEVENTTELEPORT op 3, déplacement, op 8).
    let x = target.x as f64 + RECALL_OFFSET;
    let y = ctx.map.height(x, target.z as f64).unwrap_or(target.y);
    super::portal::custom_travel(ctx, st, sid, archcore_net::Vec3::new(x as f32, y, target.z)).await?;
    info!("[groupe] {my_name} rejoint {} par le rappel", name_of(st, leader));
    send(st, leader, management_packet(mgmt::ACCEPT_RECALL, Some(me), Some(leader), Some(&my_name)));
    Ok(())
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
        let party = Party { id: 101, members: vec![7, 9], exp_type: EXP_BY_DAMAGE, item_division: ITEM_DAMAGE, effect: None };
        let p = archcore_net::decode_packet(&schema::PARTY, &party_packet(op::ADD, &party, None, true)).unwrap();
        assert_eq!((p.int(0), p.int(1), p.int(2), p.int(3)), (Some(0), Some(101), Some(5), Some(2)));
        assert_eq!(p.bytes(5).unwrap(), &[7, 0, 0, 0, 9, 0, 0, 0]);
        let m = archcore_net::decode_packet(&schema::PARTY_MANAGEMENT, &management_packet(mgmt::INVITE, Some(7), Some(9), Some("Alice"))).unwrap();
        assert_eq!((m.int(0), m.int(1), m.int(2), m.text(3).as_deref()), (Some(0), Some(7), Some(9), Some("Alice")));
        // Demande de rappel : lanceur au champ 1, son nom au champ 3, pas de cible.
        let r = archcore_net::decode_packet(&schema::PARTY_MANAGEMENT, &recall_request_packet(7, "Alice")).unwrap();
        assert_eq!((r.int(0), r.int(1), r.int(2), r.text(3).as_deref()), (Some(15), Some(7), None, Some("Alice")));
    }

    #[test]
    fn zone_et_bonus() {
        // Chevalier : défense ; archer (ou scion) : dégâts ; mage : PV et PM. Rien seul dans la zone ni au-delà de 20 niveaux.
        assert_eq!(bonus_of(1, 3, &[50, 60, 55], 2, 50, 20), Bonus { defense: 4, ..Default::default() });
        assert_eq!(bonus_of(2, 2, &[50, 60], 2, 50, 20), Bonus { damage: 2, ..Default::default() });
        assert_eq!(bonus_of(3, 5, &[50; 5], 2, 50, 20), Bonus { damage: 8, ..Default::default() });
        assert_eq!(bonus_of(4, 4, &[50; 4], 2, 50, 20), Bonus { hp: 150, mp: 150, ..Default::default() });
        assert!(bonus_of(1, 1, &[50, 60], 2, 50, 20).is_none(), "seul dans la zone");
        assert!(bonus_of(1, 3, &[30, 51, 40], 2, 50, 20).is_none(), "écart de 21 niveaux");
        assert!(!bonus_of(1, 3, &[30, 50, 40], 2, 50, 20).is_none(), "écart de 20 niveaux");
        let mut f = crate::character::Factors::default();
        (f.damage_min[0], f.damage_max[0], f.defense_point[0], f.point_max[0]) = (100, 200, 300, 1000);
        Bonus { damage: 10, defense: 5, hp: 50, mp: 20 }.apply(&mut f);
        assert_eq!((f.damage_min[0], f.damage_max[0], f.defense_point[0], f.point_max[0], f.point_max[1]), (110, 220, 315, 1050, 20));
        // Op 2 : zone (un octet par membre), bonus.
        let p = archcore_net::decode_packet(&schema::PARTY, &update_packet(101, &[true, false, true], &Bonus { defense: 4, ..Default::default() })).unwrap();
        let z = archcore_net::decode_mini(&mini::PARTY_EFFECT_AREA, p.bytes(7).unwrap()).unwrap();
        let b = archcore_net::decode_mini(&mini::PARTY_BONUS, p.bytes(9).unwrap()).unwrap();
        assert_eq!((p.int(0), p.int(1), (0..5).map(|i| z.int(i).unwrap_or(-1)).collect::<Vec<_>>()), (Some(2), Some(101), vec![1, 0, 1, 0, 0]));
        assert_eq!((b.int(0), b.int(1), b.int(2), b.int(3)), (Some(0), Some(4), Some(0), Some(0)));
    }

    #[test]
    fn ecart_de_niveau_a_l_invitation() {
        assert!(level_gap_refused(20, 10, 10) && level_gap_refused(20, 1, 10));
        assert!(!level_gap_refused(20, 11, 10) && !level_gap_refused(5, 20, 10), "invité plus haut : permis");
        assert!(!level_gap_refused(100, 1, 0), "règle coupée");
    }

    #[test]
    fn rappels_en_attente() {
        let now = Instant::now();
        let mut p = Parties::default();
        p.recalls.insert(9, Recall { leader: 7, until: now + Duration::from_secs(60) });
        assert!(!p.take_recall(9, 8, now), "autre lanceur");
        assert!(p.take_recall(9, 7, now));
        assert!(!p.take_recall(9, 7, now), "consommé");
        p.recalls.insert(9, Recall { leader: 7, until: now });
        assert!(!p.take_recall(9, 7, now + Duration::from_secs(1)), "échu");
        assert!(p.recalls.is_empty(), "un rappel échu est retiré");
    }
}
