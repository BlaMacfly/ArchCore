//! Échange entre joueurs (AGPMPRIVATETRADE, 0x07), tel que le client de référence l'envoie et le lit.
//!
//! ```text
//!   A → op 1 (champ 2 = CID de B)        B ← op 3 (champ 1 = soi, champ 2 = A, seul lu par le client) ;
//!                                        tous deux passent au statut d'action 7
//!   B → op 4 (accepte) / op 5 (refuse)   A, B ← op 6 (début, champ 1 = soi, 2 = l'autre) / op 5 (fin, champ 1 = soi)
//!   → op 9 (IID au champ 3, case visée au champ 7, genre de grille au champ 8) : objet mis dans la grille
//!     d'échange (1 × 3 × 4) : soi ← AGPMITEM op 2 statut 6, l'autre ← AGPMITEM op 0 statut 6
//!     (propriétaire = soi) ; op 10 le reprend (champ 7 : case d'arrivée du sac, prise si elle est libre)
//!   → op 23 (montant au champ 5) : soi ← op 23, l'autre ← op 24
//!   → op 16 verrouille (soi ← 16 avec son CID au champ 1, l'autre ← 17 ; tous deux verrouillés ← 20) ;
//!     op 18 déverrouille (soi ← 18 avec son CID, l'autre ← 19)
//!   → op 21 prêt (soi ← 21, l'autre ← 22) ; tous deux prêts : échange des objets et de l'argent, op 0
//!     (résultat, champ 8 = 8) à chacun, statut d'action 0
//!   → op 5 annule : objets rendus, op 5 aux deux
//!   cible qui refuse les échanges (options, « Block Trade ») : auteur ← op 25 (`Option_Trade_Off_User`)
//! ```
//! Les objets restent dans l'inventaire de leur propriétaire (et en base) jusqu'à l'échange : rien n'est
//! perdu si un joueur se déconnecte, l'échange est alors annulé.
//!
//! Le client n'envoie jamais l'op 25 (refuser une demande envoie l'op 5) : reçue, elle est tenue pour une
//! annulation, sans risque puisque l'op 5 permet déjà la même chose à tout moment. Les pièces Chantra (ops 26 / 27)
//! sont ignorées (boutique Chantra laissée de côté).
//!
//! Objets liés (« BoundType » 1, lié dès l'obtention, comme le courrier et la maison des ventes) et grille Ex (épisode 8,
//! `PrivateTradeGridEx` de `[UI_PrivateTrade]`) : l'op 9 de genre 2 y pose un Marvel Scroll of Release (usage 35) de
//! l'inventaire Cash, un seul par échange (soi ← AGPMITEM op 1 puis op 0 au statut 14, case 1,1,1 ; l'autre ne le voit
//! pas). Un objet lié ne se propose qu'avec un parchemin posé, un seul par échange, et jamais s'il est d'événement
//! (texte du parchemin). L'échange fait consomme un parchemin posé ; annulé, il revient à l'inventaire Cash
//! (op 1 puis op 0 au statut 51). Le parchemin ne se reprend pas (l'op 10 ne vise que la grille ordinaire).
//! « BoundType » 2 (lié en s'équipant) n'est pas suivi par objet : un tel objet s'échange librement.
//!
//! Contre les arnaques : seul le joueur sollicité accepte la demande (op 4) ; déverrouiller (op 18) ne touche
//! qu'à son propre verrou, comme le client qui ne relâche son bouton « Lock » qu'au clic, mais l'échange exige
//! de nouveau les deux verrous et le partenaire voit l'offre changer ; un objet proposé qui a changé depuis
//! (quantité consommée, conversion, options, durabilité) annule l'échange ; un second « prêt » est sans effet.
//! Meurtriers, comme le client : demande refusée au palier 1 du demandeur ou au palier 2 de l'un des deux
//! (menu d'échange caché), acceptation refusée au palier 1 de celui qui accepte.
//! Comme le client (menu d'échange inactif), pas d'échange si l'un des deux est métamorphosé, sauf l'ArchLord
//! (`morph.rs`) : demande annulée (op 5), et acceptation aussi si une Mutation est survenue entre-temps.

use super::combat;
use super::items::{item_remove, item_status, item_update, stored_status, Grid, InventoryItem};
use super::{send_system, WorldCtx, WorldState};
use crate::{info, warn};
use archcore_net::schema;
use archcore_net::{build_packet, Fields, Packet};
use std::collections::BTreeMap;

/// Grille d'échange : 1 page de 3 lignes × 4 colonnes.
pub const TRADE_GRID: (i64, i64, i64) = (1, 3, 4);
/// AGPMCHARACTER champ 11 : statut d'action « échange ».
const ACTION_STATUS_TRADE: i64 = 7;
/// Distance maximale entre les deux joueurs pour commencer un échange.
const MAX_TRADE_DISTANCE: f64 = 1500.0;
/// Résultat envoyé à la fin d'un échange réussi.
const RESULT_DONE: i64 = 8;
/// Op 9, champ 8 : genre de grille visée (0 grille ordinaire, 1 objet particulier posé sur la grille ordinaire,
/// 2 grille Ex : Marvel Scroll of Release).
const GRID_KIND_EX: i64 = 2;
/// AGPMITEM champ 1 : statut d'un objet posé sur la grille Ex, et sa case (celle que pose la référence, choix).
pub const STATUS_TRADE_OPTION_GRID: i64 = 14;
const OPTION_GRID_CELL: Grid = [1, 1, 1];
/// « BoundType » d'un objet lié dès l'obtention.
pub const BOUND_ON_ACQUIRE: i64 = 1;
/// Palier de meurtrier qui interdit de demander ou d'accepter un échange (pénalité du client).
const MURDERER_PENALTY_LEVEL: usize = 1;
/// Palier de meurtrier de l'un des deux qui cache le menu d'échange.
const MURDERER_HIDDEN_LEVEL: usize = 2;

pub mod op {
    pub const RESULT: i64 = 0;
    pub const REQUEST: i64 = 1;
    pub const REQUEST_CONFIRM: i64 = 3;
    pub const CONFIRM: i64 = 4;
    pub const CANCEL: i64 = 5;
    pub const START: i64 = 6;
    pub const ADD: i64 = 9;
    pub const REMOVE: i64 = 10;
    pub const LOCK: i64 = 16;
    pub const TARGET_LOCKED: i64 = 17;
    pub const UNLOCK: i64 = 18;
    pub const TARGET_UNLOCKED: i64 = 19;
    pub const ACTIVE_READY: i64 = 20;
    pub const READY: i64 = 21;
    pub const TARGET_READY: i64 = 22;
    pub const UPDATE_MONEY: i64 = 23;
    pub const TARGET_UPDATE_MONEY: i64 = 24;
    pub const REFUSE: i64 = 25;
    /// Pièces Chantra de soi / de l'autre : ignorées (boutique Chantra laissée de côté).
    pub const UPDATE_CC: i64 = 26;
    pub const TARGET_UPDATE_CC: i64 = 27;
}

/// Échange en cours, côté d'un joueur.
#[derive(Clone, Debug, Default)]
pub struct Trade {
    /// Session de l'autre joueur.
    pub peer: u32,
    /// Demande pas encore acceptée.
    pub awaiting: bool,
    pub locked: bool,
    pub ready: bool,
    pub gold: i64,
    /// Objets proposés : IID → case de la grille d'échange.
    pub items: BTreeMap<u32, Grid>,
    /// État de chaque objet proposé quand il l'a été (quantité, conversion, options, durabilité) : l'échange est
    /// annulé s'il a changé depuis (pierres consommées, conversion ratée…), car le partenaire ne le voit pas.
    pub offered: BTreeMap<u32, super::items::InventoryItem>,
    /// Auteur de la demande : lui ne peut pas l'accepter à la place de l'autre.
    pub requester: bool,
    /// Marvel Scroll of Release posé sur la grille Ex (IID, inventaire Cash).
    pub option_item: Option<u32>,
    /// Objet lié proposé grâce au parchemin (un seul par échange).
    pub bound_item: Option<u32>,
}

impl Trade {
    pub fn free_slot(&self) -> Option<Grid> {
        let (l, r, c) = TRADE_GRID;
        (0..l).flat_map(|a| (0..r).flat_map(move |b| (0..c).map(move |d| [a, b, d]))).find(|g| !self.items.values().any(|x| x == g))
    }

    /// Case où poser un objet : celle visée par le client (op 9, champ 7) si elle est dans la grille et libre,
    /// sinon la première libre.
    pub fn slot_for(&self, wanted: Option<Grid>) -> Option<Grid> {
        let (l, r, c) = TRADE_GRID;
        let inside = |g: &Grid| g[0] < l && g[1] < r && g[2] < c;
        wanted.filter(|g| inside(g) && !self.items.values().any(|x| x == g)).or_else(|| self.free_slot())
    }
}

/// Demande d'échange interdite aux meurtriers (points de meurtrier du demandeur et de la cible) : palier 1 du
/// demandeur (pénalité), ou palier 2 de l'un des deux (le client cache alors le menu d'échange).
pub fn murderer_refuses(requester: u32, target: u32) -> bool {
    let (a, b) = (super::pvp::murderer_level(requester), super::pvp::murderer_level(target));
    a >= MURDERER_PENALTY_LEVEL || a.max(b) >= MURDERER_HIDDEN_LEVEL
}

/// Un objet proposé à l'échange (ou le parchemin de la grille Ex) ne peut être ni déplacé, ni utilisé, ni détruit.
pub fn is_offered(st: &WorldState, sid: u32, iid: u32) -> bool {
    st.players.get(&sid).and_then(|p| p.trade.as_ref()).is_some_and(|t| t.items.contains_key(&iid) || t.option_item == Some(iid))
}

/// Pourquoi un objet ne se propose pas, vu sa liaison (`bound` : « BoundType » 1), s'il est d'événement et ce qui est déjà
/// posé (`t`) ; sinon `None`.
pub fn bound_refusal(t: &Trade, bound: bool, event: bool) -> Option<&'static str> {
    if !bound {
        return None;
    }
    if event {
        Some("un objet d'événement lié ne s'échange pas")
    } else if t.option_item.is_none() {
        Some("un objet lié ne s'échange qu'avec un Marvel Scroll of Release posé")
    } else if t.bound_item.is_some() {
        Some("un seul objet lié par échange")
    } else {
        None
    }
}

fn packet(op: i64, cid: Option<u32>, target: Option<u32>) -> Fields {
    let s = &schema::PRIVATE_TRADE;
    Fields::new(s).with(0, op).with_opt(1, cid).with_opt(2, target)
}

fn send(st: &WorldState, sid: u32, f: Fields) {
    if let Some(p) = st.players.get(&sid) {
        p.send(&schema::PRIVATE_TRADE, f);
    }
}

/// Statut d'action (0 normal, 7 échange) diffusé autour du joueur.
fn action_status(st: &WorldState, sid: u32, status: i64) {
    let Some(p) = st.players.get(&sid) else { return };
    let c = &schema::CHARACTER;
    combat::send_around(st, sid, &build_packet(c, &Fields::new(c).with(0, 1).with(1, p.ch.id).with(11, status)));
}

/// AGPMPRIVATETRADE reçu.
pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    if !st.players.get(&sid).is_some_and(|p| p.in_game) {
        return Ok(());
    }
    match p.int(0).unwrap_or(-1) {
        op::REQUEST => request(st, sid, p.int(2).unwrap_or(0) as u32),
        op::CONFIRM => confirm(st, sid),
        // Op 25 : jamais envoyée par le client (voir l'en-tête), tenue pour une annulation.
        op::CANCEL | op::REFUSE => cancel(st, sid),
        op::ADD => add(ctx, st, sid, p.int(3).unwrap_or(0) as u32, super::bank::grid_of(p.bytes(7)), p.int(8).unwrap_or(0)),
        op::REMOVE => {
            // Case d'arrivée (champ 7) seulement pour le sac ordinaire (statut 2) ; 13 et 15 sont hors épisode 8.
            let wanted = super::bank::grid_of(p.bytes(7)).filter(|_| p.int(5).unwrap_or(2) == 2);
            take_back(ctx, st, sid, p.int(3).unwrap_or(0) as u32, wanted).await?
        }
        op::UPDATE_MONEY => money(st, sid, p.int(5).unwrap_or(0)),
        op::LOCK => lock(st, sid),
        op::UNLOCK => unlock(st, sid),
        op::READY => ready(ctx, st, sid).await?,
        op::UPDATE_CC | op::TARGET_UPDATE_CC => info!("[échange] #{sid} pièces Chantra (op {}) : ignoré (boutique Chantra laissée de côté)", p.int(0).unwrap_or(-1)),
        other => warn!("[échange] #{sid} op {other} inattendu"),
    }
    Ok(())
}

/// Les deux côtés d'un échange en cours et accepté : (moi, l'autre).
fn sides(st: &WorldState, sid: u32) -> Option<(Trade, Trade)> {
    let mine = st.players.get(&sid)?.trade.clone()?;
    let theirs = st.players.get(&mine.peer)?.trade.clone()?;
    (theirs.peer == sid && !mine.awaiting).then_some((mine, theirs))
}

fn request(st: &mut WorldState, sid: u32, target_cid: u32) {
    let Some(target) = st.by_cid.get(&target_cid).copied() else { return cancel(st, sid) };
    let (me, other) = (&st.players[&sid], st.players.get(&target));
    let Some(other) = other else { return cancel(st, sid) };
    // Échanges refusés dans les options, ou auteur ignoré avec « Block Trade » (buddy.rs) : « Option_Trade_Off_User »
    // chez l'auteur (op 25, CID visé).
    let blocked = super::buddy::blocks(st, target, me.ch.id, archcore_net::schema::buddy_flag::BLOCK_TRADE);
    if blocked || super::option_flags::refuses(st, target, super::option_flags::REFUSE_TRADE) {
        info!("[échange] {} refuse les échanges ({} demandait{})", other.ch.name, me.ch.name, if blocked { ", ignoré" } else { "" });
        let cid = me.ch.id;
        return send(st, sid, packet(op::REFUSE, Some(target_cid), Some(cid)));
    }
    // Meurtrier, comme le client : demandeur au palier 1, ou l'un des deux au palier 2.
    if murderer_refuses(me.ch.murderer, other.ch.murderer) {
        info!("[échange] demande de {} à {} refusée : meurtrier", me.ch.name, other.ch.name);
        return cancel(st, sid);
    }
    // Métamorphose de l'un des deux (sauf l'ArchLord) : le client n'ouvre pas l'échange (morph.rs).
    if super::morph::restricted(st, sid) || super::morph::restricted(st, target) {
        info!("[échange] demande de {} à {} refusée : métamorphose", me.ch.name, other.ch.name);
        return cancel(st, sid);
    }
    let near = combat::dist2d(me.ch.pos(), other.ch.pos()) <= MAX_TRADE_DISTANCE;
    if target == sid || me.trade.is_some() || other.trade.is_some() || me.dead || other.dead || !other.in_game || !near {
        info!("[échange] demande de {} à {} refusée (déjà occupé, mort ou trop loin)", me.ch.name, other.ch.name);
        return cancel(st, sid);
    }
    let (cid, names) = (me.ch.id, (me.ch.name.clone(), other.ch.name.clone()));
    for (a, b) in [(sid, target), (target, sid)] {
        st.players.get_mut(&a).unwrap().trade = Some(Trade { peer: b, awaiting: true, requester: a == sid, ..Default::default() });
        action_status(st, a, ACTION_STATUS_TRADE);
    }
    // Le client lit le demandeur au champ 2 ; le champ 1 porte, comme ailleurs, le personnage du destinataire.
    send(st, target, packet(op::REQUEST_CONFIRM, Some(target_cid), Some(cid)));
    info!("[échange] {} propose un échange à {}", names.0, names.1);
}

fn confirm(st: &mut WorldState, sid: u32) {
    // Seul le joueur sollicité accepte : l'auteur de la demande ne peut pas ouvrir l'échange à sa place.
    let Some(peer) = st.players[&sid].trade.as_ref().filter(|t| t.awaiting && !t.requester).map(|t| t.peer) else { return cancel(st, sid) };
    // Métamorphose survenue depuis la demande (Mutation) : pas d'échange.
    if super::morph::restricted(st, sid) || super::morph::restricted(st, peer) {
        return cancel(st, sid);
    }
    // Meurtrier au palier 1 : le client répond « non » (op 5) au lieu d'accepter.
    if super::pvp::murderer_level(st.players[&sid].ch.murderer) >= MURDERER_PENALTY_LEVEL {
        info!("[échange] acceptation de {} refusée : meurtrier", st.players[&sid].ch.name);
        return cancel(st, sid);
    }
    for s in [sid, peer] {
        if let Some(t) = st.players.get_mut(&s).and_then(|p| p.trade.as_mut()) {
            t.awaiting = false;
        }
    }
    let (a, b) = (st.players[&sid].ch.id, st.players.get(&peer).map(|p| p.ch.id).unwrap_or(0));
    send(st, sid, packet(op::START, Some(a), Some(b)));
    send(st, peer, packet(op::START, Some(b), Some(a)));
    info!("[échange] début entre {} et {}", st.players[&sid].ch.name, st.players.get(&peer).map(|p| p.ch.name.as_str()).unwrap_or("?"));
}

/// Annule l'échange des deux côtés : objets rendus à l'inventaire, op 5, statut d'action normal.
pub fn cancel(st: &mut WorldState, sid: u32) {
    let peer = st.players.get(&sid).and_then(|p| p.trade.as_ref()).map(|t| t.peer);
    end_side(st, sid, peer);
    if let Some(peer) = peer {
        end_side(st, peer, Some(sid));
    }
}

fn end_side(st: &mut WorldState, sid: u32, peer: Option<u32>) {
    let Some(p) = st.players.get_mut(&sid) else { return };
    let Some(trade) = p.trade.take() else {
        p.send(&schema::PRIVATE_TRADE, packet(op::CANCEL, Some(p.ch.id), None));
        return;
    };
    let cid = p.ch.id;
    // Parchemin de la grille Ex rendu à l'inventaire Cash.
    if let Some(scroll) = trade.option_item.and_then(|iid| p.inventory.cash.get(&iid)) {
        p.send_raw(item_remove(scroll.iid, Some(cid)));
        p.send_raw(super::items::item_add_of(scroll, cid, item_status::CASH_INVENTORY, None));
    }
    for iid in trade.items.keys() {
        if let Some(item) = st.players[&sid].inventory.get(*iid) {
            st.players[&sid].send_raw(item_update(item, cid, item_status::INVENTORY));
        }
        if let Some(peer) = peer.and_then(|s| st.players.get(&s)) {
            peer.send_raw(item_remove(*iid, Some(cid)));
        }
    }
    send(st, sid, packet(op::CANCEL, Some(cid), None));
    action_status(st, sid, 0);
}

/// Op 9 : `wanted` = case visée de la grille d'échange (champ 7), `kind` = genre de grille (champ 8).
fn add(ctx: &WorldCtx, st: &mut WorldState, sid: u32, iid: u32, wanted: Option<Grid>, kind: i64) {
    let Some((mine, _)) = sides(st, sid) else { return cancel(st, sid) };
    let p = &st.players[&sid];
    if mine.locked || mine.items.contains_key(&iid) || mine.option_item == Some(iid) {
        return;
    }
    if kind == GRID_KIND_EX {
        return put_option(ctx, st, sid, iid);
    }
    let Some(item) = p.inventory.get(iid).cloned() else { return };
    let data = ctx.catalog.get(item.tid);
    let bound = data.is_some_and(|d| d.bound_type == BOUND_ON_ACQUIRE);
    // Objet de quête (« QuestItem ») ; objet à durée en cours d'utilisation ou daté (lifetime.rs).
    let quest_or_timed = if ctx.catalog.quest_items.contains_key(&item.tid) {
        Some("un objet de quête ne s'échange pas")
    } else if super::lifetime::locked(&p.inventory, iid) {
        Some("un objet à durée commencée ne s'échange pas")
    } else {
        None
    };
    if let Some(why) = quest_or_timed.or_else(|| bound_refusal(&mine, bound, data.is_some_and(|d| d.event_item))) {
        info!("[échange] {} propose l'objet {} : refusé ({why})", p.ch.name, item.tid);
        send_system(st, sid, &format!("Échange : {why}."));
        return;
    }
    let Some(grid) = mine.slot_for(wanted) else {
        send_system(st, sid, "La grille d'échange est pleine.");
        return;
    };
    let cid = p.ch.id;
    let t = st.players.get_mut(&sid).unwrap().trade.as_mut().unwrap();
    t.items.insert(iid, grid);
    t.offered.insert(iid, item.clone());
    if bound {
        t.bound_item = Some(iid);
    }
    let shown = super::items::InventoryItem { grid, ..item.clone() };
    st.players[&sid].send_raw(item_update(&shown, cid, item_status::TRADE_GRID));
    if let Some(peer) = st.players.get(&mine.peer) {
        peer.send_raw(super::items::item_add_of(&item, cid, item_status::TRADE_GRID, Some(grid)));
        if let Some(c) = super::items::convert_add(&item, cid) {
            peer.send_raw(c);
        }
    }
    let name = ctx.catalog.get(item.tid).map(|d| d.name.as_str()).unwrap_or("?");
    info!("[échange] {} propose {} × {name}", st.players[&sid].ch.name, item.count);
}

/// Op 9 de genre 2 : Marvel Scroll of Release de l'inventaire Cash posé sur la grille Ex (un par échange).
fn put_option(ctx: &WorldCtx, st: &mut WorldState, sid: u32, iid: u32) {
    let p = &st.players[&sid];
    let Some(scroll) = p.inventory.cash.get(&iid).cloned() else { return };
    let is_scroll = ctx.catalog.get(scroll.tid).is_some_and(|d| d.usable_type == super::consumables::USABLE_PRIVATE_TRADE_OPTION);
    if !is_scroll || p.trade.as_ref().is_some_and(|t| t.option_item.is_some()) {
        info!("[échange] {} : objet {} refusé sur la grille Ex", p.ch.name, scroll.tid);
        return;
    }
    let cid = p.ch.id;
    p.send_raw(item_remove(iid, Some(cid)));
    p.send_raw(super::items::item_add_of(&InventoryItem { grid: OPTION_GRID_CELL, ..scroll.clone() }, cid, STATUS_TRADE_OPTION_GRID, Some(OPTION_GRID_CELL)));
    info!("[échange] {} pose un Marvel Scroll of Release ({}) sur la grille Ex", p.ch.name, scroll.tid);
    st.players.get_mut(&sid).unwrap().trade.as_mut().unwrap().option_item = Some(iid);
}

/// Op 10 : l'objet revient dans le sac, sur la case `wanted` si elle est libre (sinon à sa case d'origine).
async fn take_back(ctx: &WorldCtx, st: &mut WorldState, sid: u32, iid: u32, wanted: Option<Grid>) -> anyhow::Result<()> {
    let Some((mine, _)) = sides(st, sid) else {
        cancel(st, sid);
        return Ok(());
    };
    if mine.locked || !mine.items.contains_key(&iid) {
        return Ok(());
    }
    let p = st.players.get_mut(&sid).unwrap();
    let t = p.trade.as_mut().unwrap();
    t.items.remove(&iid);
    t.offered.remove(&iid);
    if t.bound_item == Some(iid) {
        t.bound_item = None;
    }
    let moved = match wanted.filter(|g| p.inventory.is_free(*g)) {
        Some(g) => p.inventory.move_to(&ctx.db, iid, g).await?,
        None => None,
    };
    let p = &st.players[&sid];
    if let Some(item) = moved.as_ref().or_else(|| p.inventory.get(iid)) {
        p.send_raw(item_update(item, p.ch.id, item_status::INVENTORY));
    }
    if let Some(peer) = st.players.get(&mine.peer) {
        peer.send_raw(item_remove(iid, Some(p.ch.id)));
    }
    Ok(())
}

fn money(st: &mut WorldState, sid: u32, amount: i64) {
    let Some((mine, _)) = sides(st, sid) else { return cancel(st, sid) };
    if mine.locked {
        return;
    }
    let amount = amount.clamp(0, st.players[&sid].ch.money as i64);
    st.players.get_mut(&sid).unwrap().trade.as_mut().unwrap().gold = amount;
    let s = &schema::PRIVATE_TRADE;
    send(st, sid, Fields::new(s).with(0, op::UPDATE_MONEY).with(5, amount));
    send(st, mine.peer, Fields::new(s).with(0, op::TARGET_UPDATE_MONEY).with(5, amount));
}

fn lock(st: &mut WorldState, sid: u32) {
    let Some((mine, theirs)) = sides(st, sid) else { return cancel(st, sid) };
    if mine.locked {
        return;
    }
    st.players.get_mut(&sid).unwrap().trade.as_mut().unwrap().locked = true;
    // Le client pose son propre verrou sur le personnage du champ 1.
    let cid = st.players[&sid].ch.id;
    send(st, sid, packet(op::LOCK, Some(cid), None));
    send(st, mine.peer, packet(op::TARGET_LOCKED, None, None));
    if theirs.locked {
        send(st, sid, packet(op::ACTIVE_READY, None, None));
        send(st, mine.peer, packet(op::ACTIVE_READY, None, None));
    }
}

fn unlock(st: &mut WorldState, sid: u32) {
    let Some((mine, theirs)) = sides(st, sid) else { return cancel(st, sid) };
    if !mine.locked || mine.ready || theirs.ready {
        return;
    }
    st.players.get_mut(&sid).unwrap().trade.as_mut().unwrap().locked = false;
    let cid = st.players[&sid].ch.id;
    send(st, sid, packet(op::UNLOCK, Some(cid), None));
    send(st, mine.peer, packet(op::TARGET_UNLOCKED, None, None));
    // Le verrou du partenaire reste posé : son client ne relâcherait pas le bouton « Lock » (relâché au clic
    // seulement). Il voit l'offre changer, et l'échange attend que les deux soient de nouveau verrouillés.
}

async fn ready(ctx: &WorldCtx, st: &mut WorldState, sid: u32) -> anyhow::Result<()> {
    let Some((mine, theirs)) = sides(st, sid) else {
        cancel(st, sid);
        return Ok(());
    };
    if !mine.locked || !theirs.locked || mine.ready {
        return Ok(());
    }
    st.players.get_mut(&sid).unwrap().trade.as_mut().unwrap().ready = true;
    send(st, sid, packet(op::READY, None, None));
    send(st, mine.peer, packet(op::TARGET_READY, None, None));
    if theirs.ready {
        execute(ctx, st, sid, mine.peer).await?;
    }
    Ok(())
}

/// Objet proposé encore tel qu'au moment de sa proposition (sa case peut avoir changé).
pub fn same_offer(offered: Option<&super::items::InventoryItem>, now: Option<&super::items::InventoryItem>) -> bool {
    match (offered, now) {
        (Some(o), Some(n)) => super::items::InventoryItem { grid: o.grid, ..n.clone() } == *o,
        _ => false,
    }
}

/// Ce qui empêche l'échange, côté `a` qui reçoit ce que `b` propose.
fn check(st: &WorldState, a: u32, b: u32) -> Option<&'static str> {
    let (pa, pb) = (&st.players[&a], &st.players[&b]);
    let (ta, tb) = (pa.trade.as_ref()?, pb.trade.as_ref()?);
    if pa.dead || pb.dead {
        return Some("un joueur est mort");
    }
    if (pa.ch.money as i64) < ta.gold || pa.ch.money as i64 - ta.gold + tb.gold > super::npc_trade::MAX_MONEY {
        return Some("argent");
    }
    if ta.items.keys().chain(tb.items.keys()).any(|iid| pa.inventory.get(*iid).is_none() && pb.inventory.get(*iid).is_none()) {
        return Some("objet disparu");
    }
    // Ce que `b` donne doit être resté tel qu'il l'a proposé (quantité, conversion, options, durabilité).
    if tb.items.keys().any(|iid| !same_offer(tb.offered.get(iid), pb.inventory.get(*iid))) {
        return Some("objet modifié depuis sa proposition");
    }
    if pa.inventory.free_count() + ta.items.len() < tb.items.len() {
        return Some("inventaire plein");
    }
    if ta.option_item.is_some_and(|iid| !pa.inventory.cash.contains_key(&iid)) {
        return Some("parchemin disparu");
    }
    None
}

async fn execute(ctx: &WorldCtx, st: &mut WorldState, a: u32, b: u32) -> anyhow::Result<()> {
    if let Some(why) = check(st, a, b).or_else(|| check(st, b, a)) {
        info!("[échange] annulé : {why}");
        send_system(st, a, &format!("Échange annulé : {why}."));
        send_system(st, b, &format!("Échange annulé : {why}."));
        cancel(st, a);
        return Ok(());
    }
    let (ta, tb) = (st.players[&a].trade.clone().unwrap(), st.players[&b].trade.clone().unwrap());
    let (cid_a, cid_b) = (st.players[&a].ch.id, st.players[&b].ch.id);
    // L'échange est préparé sur des copies des deux inventaires, enregistré en une transaction, puis reporté
    // en mémoire : une erreur de la base n'en laisse rien, ni en base ni en jeu.
    let mut inv = [st.players[&a].inventory.clone(), st.players[&b].inventory.clone()];
    // Les objets quittent d'abord les deux inventaires, pour libérer leurs cases.
    let mut outgoing: Vec<(usize, super::items::InventoryItem)> = Vec::new();
    for (from, trade) in [(0, &ta), (1, &tb)] {
        for iid in trade.items.keys() {
            if let Some(item) = inv[from].items.remove(iid) {
                outgoing.push((from, item));
            }
        }
    }
    let cids = [cid_a, cid_b];
    let mut stmts: Vec<(&str, mysql_async::Params)> = Vec::new();
    let mut moved: Vec<(usize, super::items::InventoryItem)> = Vec::new();
    for (from, item) in outgoing {
        let to = 1 - from;
        let grid = inv[to].free_slot().unwrap_or([0, 0, 0]);
        stmts.push((
            "UPDATE character_items SET character_id = ?, status = ?, layer = ?, row_idx = ?, col_idx = ? WHERE id = ?",
            (cids[to], stored_status::INVENTORY, grid[0], grid[1], grid[2], item.iid).into(),
        ));
        let item = super::items::InventoryItem { grid, ..item };
        inv[to].items.insert(item.iid, item.clone());
        moved.push((from, item));
    }
    let money = [
        st.players[&a].ch.money as i64 - ta.gold + tb.gold,
        st.players[&b].ch.money as i64 - tb.gold + ta.gold,
    ];
    for i in 0..2 {
        stmts.push((super::SAVE_MONEY, (money[i], cids[i]).into()));
    }
    // Marvel Scroll of Release posé : un exemplaire consommé.
    let scrolls = [ta.option_item, tb.option_item];
    for i in 0..2 {
        if let Some(sql) = scrolls[i].and_then(|iid| inv[i].consume_sql(iid, 1)) {
            stmts.push(sql);
        }
    }
    if let Err(e) = ctx.db.characters.atomic(stmts).await {
        send_system(st, a, "Échange annulé : erreur du serveur.");
        send_system(st, b, "Échange annulé : erreur du serveur.");
        cancel(st, a);
        return Err(e);
    }
    let sids = [a, b];
    // Trace dans la base des journaux : ce que chacun a donné.
    let given = |side: usize| moved.iter().filter(|(from, _)| *from == side).map(|(_, i)| (i.iid, i.tid, i.count)).collect::<Vec<_>>();
    crate::journal::trade(&ctx.db, (cid_a, &st.players[&a].ch.name), (cid_b, &st.players[&b].ch.name), ta.gold, tb.gold, &given(0), &given(1));
    let mut scroll_left: Vec<(usize, u32, Option<InventoryItem>)> = Vec::new();
    for i in 0..2 {
        if let Some(iid) = scrolls[i] {
            scroll_left.push((i, iid, inv[i].consumed(iid, 1)));
        }
    }
    let [inv_a, inv_b] = inv;
    for (sid, inventory) in [(a, inv_a), (b, inv_b)] {
        let p = st.players.get_mut(&sid).unwrap();
        p.trade = None;
        p.inventory = inventory;
    }
    for (from, item) in moved {
        let to = 1 - from;
        let receiver = &st.players[&sids[to]];
        receiver.send_raw(item_remove(item.iid, Some(cids[from])));
        receiver.send_raw(super::items::item_add_of(&item, cids[to], item_status::INVENTORY, Some(item.grid)));
        if let Some(c) = super::items::convert_add(&item, cids[to]) {
            receiver.send_raw(c);
        }
        st.players[&sids[from]].send_raw(item_remove(item.iid, Some(cids[from])));
    }
    for (i, iid, left) in scroll_left {
        let p = &st.players[&sids[i]];
        p.send_raw(item_remove(iid, Some(cids[i])));
        if let Some(left) = left {
            p.send_raw(super::items::item_add_of(&left, cids[i], item_status::CASH_INVENTORY, None));
        }
        info!("[échange] {} : Marvel Scroll of Release consommé", p.ch.name);
    }
    for (i, me) in sids.into_iter().enumerate() {
        let p = st.players.get_mut(&me).unwrap();
        let money = money[i];
        p.ch.money = money as u64;
        let c = &schema::CHARACTER;
        p.send(c, Fields::new(c).with(0, 1).with(1, p.ch.id).with(8, money));
        let s = &schema::PRIVATE_TRADE;
        p.send(s, Fields::new(s).with(0, op::RESULT).with(1, p.ch.id).with(8, RESULT_DONE));
        action_status(st, me, 0);
    }
    info!(
        "[échange] {} ↔ {} : {} objet(s) et {} or contre {} objet(s) et {} or",
        st.players[&a].ch.name,
        st.players[&b].ch.name,
        ta.items.len(),
        ta.gold,
        tb.items.len(),
        tb.gold
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grille_d_echange() {
        let mut t = Trade::default();
        for i in 0..12u32 {
            let g = t.free_slot().unwrap();
            t.items.insert(i, g);
        }
        assert_eq!(t.items[&11], [0, 2, 3]);
        assert!(t.free_slot().is_none(), "12 cases");
    }

    #[test]
    fn case_visee() {
        use archcore_net::packet::mini_payload;
        use archcore_net::schema::mini;
        let mut t = Trade::default();
        let cell = |g: [i64; 3]| {
            let s = &mini::ITEM_GRID;
            archcore_net::build_mini(s, &Fields::new(s).with(0, g[0]).with(1, g[1]).with(2, g[2]))
        };
        // Case visée de l'op 9 (champ 7) : prise si elle est dans la grille et libre.
        let wanted = super::super::bank::grid_of(Some(mini_payload(&cell([0, 2, 1]))));
        assert_eq!(t.slot_for(wanted), Some([0, 2, 1]));
        t.items.insert(1, [0, 2, 1]);
        assert_eq!(t.slot_for(wanted), Some([0, 0, 0]), "case occupée : première libre");
        assert_eq!(t.slot_for(Some([0, 3, 0])), Some([0, 0, 0]), "hors grille (3 lignes)");
        assert_eq!(t.slot_for(Some([1, 0, 0])), Some([0, 0, 0]), "un seul sac");
        assert_eq!(super::super::bank::grid_of(Some(mini_payload(&cell([-1, -1, -1])))), None, "0xFFFF : au choix du serveur");
        assert_eq!(t.slot_for(None), Some([0, 0, 0]));
    }

    #[test]
    fn objets_lies_et_grille_ex() {
        let mut t = Trade::default();
        assert_eq!(bound_refusal(&t, false, false), None, "objet ordinaire");
        assert_eq!(bound_refusal(&t, false, true), None, "objet d'événement non lié");
        assert!(bound_refusal(&t, true, false).is_some_and(|w| w.contains("Marvel")), "sans parchemin");
        t.option_item = Some(9);
        assert_eq!(bound_refusal(&t, true, false), None, "parchemin posé");
        assert!(bound_refusal(&t, true, true).is_some(), "objet d'événement lié");
        t.bound_item = Some(3);
        assert!(bound_refusal(&t, true, false).is_some_and(|w| w.contains("un seul")));
        assert_eq!(bound_refusal(&t, false, false), None);
        let p = archcore_net::decode_packet(
            &schema::ITEM,
            &super::super::items::item_add_of(&InventoryItem { iid: 9, tid: 11256, count: 2, grid: OPTION_GRID_CELL, conv: Default::default(), options: vec![], durability: -1 }, 42, STATUS_TRADE_OPTION_GRID, Some(OPTION_GRID_CELL)),
        )
        .unwrap();
        assert_eq!((p.int(0), p.int(1), p.int(2), p.int(3), p.int(4)), (Some(0), Some(14), Some(9), Some(11256), Some(42)));
        assert!(p.bytes(7).is_some(), "case de la grille Ex");
    }

    #[test]
    fn meurtriers() {
        // Paliers : 40, 60 et 100 points.
        assert!(!murderer_refuses(39, 59), "palier 1 de la cible seulement : demande admise");
        assert!(murderer_refuses(40, 0), "palier 1 du demandeur");
        assert!(murderer_refuses(0, 60), "palier 2 de la cible : menu caché");
        assert!(murderer_refuses(100, 100));
        assert!(!murderer_refuses(0, 0));
    }

    #[test]
    fn objet_propose_inchange() {
        let it = super::super::items::InventoryItem { iid: 7, tid: 190, count: 50, grid: [0, 1, 2], conv: Default::default(), options: vec![], durability: -1 };
        assert!(same_offer(Some(&it), Some(&super::super::items::InventoryItem { grid: [0, 3, 3], ..it.clone() })), "case changée : même objet");
        assert!(!same_offer(Some(&it), Some(&super::super::items::InventoryItem { count: 1, ..it.clone() })), "pierres consommées après la proposition");
        assert!(!same_offer(Some(&it), Some(&super::super::items::InventoryItem { durability: 3, ..it.clone() })));
        assert!(!same_offer(Some(&it), None) && !same_offer(None, Some(&it)));
    }
}
