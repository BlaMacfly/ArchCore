//! Portails de téléportation : objets du décor (obj*.ini) qui portent un point de TeleportPoint.ini.
//!
//! ```text
//!   AGPMEVENTTELEPORT (0x31) :
//!   client → op 4 (demande, EVENT_BASE de l'objet). À moins de 1600 unités ← op 5 (accord : le client ouvre
//!            la liste des destinations ; champ 5 : numéros des points désactivés) ; plus loin, le serveur fait
//!            courir le joueur vers l'objet et accorde dès qu'il est à portée.
//!   client → op 0 (EVENT_BASE de l'objet de départ, nom du point d'arrivée)
//!            ← op 1 (CID, position d'arrivée), déplacement, ← op 8 (chargement terminé).
//!   client → op 7 (EVENT_BASE, CID) : « Retour » depuis un point de zone de combat entre joueurs
//!            ← op 1, déplacement, ← op 8.
//! ```
//! Un trajet est permis si l'un des groupes visés par le point de départ (`TargetGroup`) compte le point
//! d'arrivée (`Group`), et si la race du joueur est admise aux deux bouts (`UseType`, 0 : toutes). L'arrivée
//! est l'objet qui porte le point d'arrivée (ou sa position fixe, `PointTargetType=1`) : on se pose sur un cercle
//! de rayon `RadiusMin`..`RadiusMax` autour de lui, au premier des huit points espacés de 45° (à partir d'un angle
//! tiré au hasard) qui n'est pas sur un sol infranchissable. Le prix du trajet (TeleportFee.txt, que le client affiche aussi), plus la
//! taxe de la région du départ (`tax.rs`), est pris sur l'argent porté.
//!
//! Points désactivés (champ 5 de l'op 5, `FUN_1406ea050` d'Archonia) : au plus 20 numéros de point (entiers sur 4
//! octets ; le numéro d'un point est son rang dans TeleportPoint.ini, à partir de 1, comme au chargement du client
//! `FUN_14022f0d0`). Le client grise ces destinations (statut « Disable ») ; si le départ lui-même y figure, toutes. ArchCore
//! y met les destinations que le client proposerait et qu'il refuserait de toute façon : point sans objet d'arrivée
//! dans les obj*.ini (entrées des forts de la Battle Ground, par exemple) ou qui n'admet pas la race du joueur.
//!
//! Zones de combat entre joueurs (`RegionType=1`, toujours avec `SpecialType=1` dans TeleportPoint.ini) :
//! comme dans le module AgpmEventTeleport d'Archonia (FUN_1402e8780), le point de départ de chaque trajet
//! réussi est noté sur le personnage ; depuis un point `SpecialType=1`, le client ne propose que « Retour »
//! (op 7), qui ramène à ce point de départ (à défaut, au village de rattachement), sans frais. Le client
//! demande lui-même confirmation avant d'entrer dans une telle zone (`TELEPORT_ConfirmPvPZone`).
//!
//! Demande (op 4), trajet (op 0) et retour (op 7) sont ignorés pour un joueur mort ou en combat (10 s
//! après le dernier coup) : le client d'origine ne les envoie pas et affiche `Teleport_CombatMode`.

use super::combat::dist2d;
use super::npcs::BASE_TYPE_OBJECT;
use super::{send_system, WorldCtx, WorldState};
use crate::db::{col, Databases};
use crate::{info, warn};
use archcore_net::schema::{self, mini};
use archcore_net::{build_mini, decode_mini, Fields, Packet, Vec3};
use rand::Rng;
use std::collections::HashMap;
use std::time::Instant;

/// Distance (au sol) à laquelle un portail répond.
pub const MAX_USE_RANGE: f64 = super::npc_events::range::TELEPORT;

pub mod op {
    pub const TELEPORT_POINT: i64 = 0;
    pub const TELEPORT_START: i64 = 1;
    pub const REQUEST: i64 = 4;
    pub const GRANTED: i64 = 5;
    pub const RETURN: i64 = 7;
    pub const LOADING: i64 = 8;
}

/// `RegionType` d'un point en zone de combat entre joueurs.
pub const REGION_PVP: i64 = 1;
/// `SpecialType` d'un point d'où l'on ne peut que revenir (op 7).
pub const SPECIAL_RETURN_ONLY: i64 = 1;

/// Races admises (bits de `UseType`), dans l'ordre des races du jeu (1 humain, 2 orc, 3 elfe de lune,
/// 4 dragonien).
fn race_bit(race: i64) -> i64 {
    match race {
        1..=4 => 1 << (race - 1),
        _ => 0,
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Point {
    /// Numéro du point : son rang dans TeleportPoint.ini, à partir de 1.
    pub id: i64,
    pub radius_min: f64,
    pub radius_max: f64,
    pub use_type: i64,
    /// `RegionType` (1 : zone de combat entre joueurs) et `SpecialType` (1 : retour seul).
    pub region_type: i64,
    pub special_type: i64,
    pub groups: Vec<String>,
    pub target_groups: Vec<String>,
    /// Arrivée : position fixe du point (`PointTargetType=1`) ou de l'objet qui le porte ; sans l'une ni l'autre, le
    /// point ne peut pas servir d'arrivée.
    pub at: Option<Vec3>,
}

impl Point {
    /// La race du joueur est admise (0 : toutes).
    pub fn admits(&self, race: i64) -> bool {
        self.use_type == 0 || self.use_type & race_bit(race) != 0
    }
    /// Un des groupes visés depuis ce point compte `to` parmi ses arrivées.
    pub fn leads_to(&self, to: &Point) -> bool {
        self.target_groups.iter().any(|g| to.groups.contains(g))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Portal {
    pub point: String,
    pub pos: Vec3,
}

/// Destinations proposées (et donc points désactivés) : 20 au plus, comme la liste du client.
pub const MAX_LISTED: usize = 20;

/// Points, portails et prix des trajets, chargés au démarrage.
#[derive(Default)]
pub struct Portals {
    pub points: HashMap<String, Point>,
    /// Noms des points dans l'ordre de TeleportPoint.ini.
    pub order: Vec<String>,
    pub objects: HashMap<u32, Portal>,
    pub fees: HashMap<(String, String), i64>,
}

impl Portals {
    pub async fn load(db: &Databases) -> anyhow::Result<Self> {
        let mut r = Portals::default();
        let Ok(rows) = db.world.rows("SELECT point_id, name, target_type, pos_x, pos_y, pos_z, radius_min, radius_max, use_type, region_type, special_type, point_groups, target_groups FROM teleport_point ORDER BY point_id", ()).await else {
            warn!("portails non chargés : lancer `archcore db-init` puis `archcore import`");
            return Ok(r);
        };
        let list = |s: String| serde_json::from_str::<Vec<String>>(&s).unwrap_or_default();
        for row in &rows {
            let point = Point {
                id: col(row, "point_id"),
                radius_min: col::<f32>(row, "radius_min") as f64,
                radius_max: col::<f32>(row, "radius_max") as f64,
                use_type: col(row, "use_type"),
                region_type: col(row, "region_type"),
                special_type: col(row, "special_type"),
                groups: list(col(row, "point_groups")),
                target_groups: list(col(row, "target_groups")),
                // Point à position fixe (`PointTargetType=1`, `PointPos`) : c'est son arrivée ; sinon l'objet qui le porte.
                at: (col::<i64>(row, "target_type") == 1).then(|| Vec3::new(col::<f64>(row, "pos_x") as f32, col::<f64>(row, "pos_y") as f32, col::<f64>(row, "pos_z") as f32)),
            };
            let name: String = col(row, "name");
            r.order.push(name.clone());
            r.points.insert(name, point);
        }
        // L'objet qui porte un point en est l'arrivée ; si plusieurs le portent, le dernier compte.
        for row in db.world.rows("SELECT object_id, point, pos_x, pos_y, pos_z FROM teleport_object ORDER BY object_id", ()).await? {
            let pos = Vec3::new(col::<f64>(&row, "pos_x") as f32, col::<f64>(&row, "pos_y") as f32, col::<f64>(&row, "pos_z") as f32);
            let point: String = col(&row, "point");
            if let Some(p) = r.points.get_mut(&point) {
                p.at = Some(pos);
            }
            r.objects.insert(col(&row, "object_id"), Portal { point, pos });
        }
        for row in db.world.rows("SELECT source, destination, fee FROM teleport_fee", ()).await? {
            r.fees.insert((col(&row, "source"), col(&row, "destination")), col(&row, "fee"));
        }
        info!("portails : {} point(s), {} objet(s), {} prix de trajet", r.points.len(), r.objects.len(), r.fees.len());
        Ok(r)
    }

    /// Destinations que le client propose depuis `from` (`FUN_1402e71e0` d'Archonia) : les points des groupes visés,
    /// dans l'ordre des groupes puis de TeleportPoint.ini, sans doublon ni le départ, 20 au plus.
    pub fn destinations(&self, from: &Point) -> Vec<&Point> {
        let mut out: Vec<&Point> = Vec::new();
        for g in &from.target_groups {
            for p in self.order.iter().filter_map(|n| self.points.get(n)) {
                if out.len() < MAX_LISTED && p.id != from.id && p.groups.contains(g) && !out.iter().any(|q| q.id == p.id) {
                    out.push(p);
                }
            }
        }
        out
    }

    /// Champ 5 de l'accord : numéros (entiers sur 4 octets) des destinations sans objet d'arrivée ou qui n'admettent
    /// pas la race `race`.
    pub fn disabled(&self, from: &Point, race: i64) -> Vec<u8> {
        self.destinations(from).iter().filter(|p| p.at.is_none() || !p.admits(race)).flat_map(|p| (p.id as i32).to_le_bytes()).collect()
    }

    /// Prix du trajet de `from` à `to` (gratuit s'il n'est pas dans la table).
    pub fn fee(&self, from: &str, to: &str) -> i64 {
        self.fees.get(&(from.to_string(), to.to_string())).copied().unwrap_or(0).max(0)
    }
}

/// Demande en attente : le joueur court vers le portail (une marche demandée par le client l'annule).
#[derive(Clone, Debug, PartialEq)]
pub struct Pending {
    pub object: u32,
    pub pos: Vec3,
    /// EVENT_BASE de la demande, renvoyé tel quel avec l'accord.
    pub base: Vec<u8>,
    /// Points désactivés envoyés avec l'accord (champ 5).
    pub disabled: Vec<u8>,
}

/// Point visé : un nom de point en CP949, terminé par un zéro.
pub fn point_name(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    encoding_rs::EUC_KR.decode(&bytes[..end]).0.trim().to_string()
}

fn packet(op: i64, base: Option<&[u8]>, cid: u32) -> Fields {
    let s = &schema::EVENT_TELEPORT;
    let f = Fields::new(s).with(0, op).with(2, cid);
    match base {
        Some(b) => f.with(1, b.to_vec()),
        None => f,
    }
}

/// Portail désigné par une mini EVENT_BASE (objet), avec la base décodée.
fn portal_of<'a>(ctx: &'a WorldCtx, p: &Packet) -> Option<(u32, &'a Portal, Vec<u8>)> {
    let raw = p.bytes(1)?;
    let base = decode_mini(&mini::EVENT_BASE, raw).ok()?;
    if base.int(0) != Some(BASE_TYPE_OBJECT) {
        return None;
    }
    let id = base.int(1)? as u32;
    ctx.portals.objects.get(&id).map(|portal| (id, portal, build_mini(&mini::EVENT_BASE, &base)))
}

pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    // Sous métamorphose (sauf l'ArchLord) : « Portals may not be used while under metamorphosis. » (morph.rs).
    if matches!(p.int(0), Some(op::REQUEST | op::TELEPORT_POINT | op::RETURN)) && super::morph::restricted(st, sid) {
        super::send_system(st, sid, "Les portails ne s'utilisent pas sous métamorphose.");
        return Ok(());
    }
    match p.int(0) {
        Some(op::REQUEST) => request(ctx, st, sid, p),
        Some(op::TELEPORT_POINT) => teleport(ctx, st, sid, p).await?,
        Some(op::RETURN) => go_back(ctx, st, sid, p).await?,
        op => crate::debug!("[portail] #{sid} AGPMEVENTTELEPORT op {op:?} (ignoré)"),
    }
    Ok(())
}

/// Le joueur ne peut pas se servir d'un portail : mort, ou en combat (comme le module d'Archonia, qui
/// ignore alors la demande).
fn unable(player: &super::Player, now: Instant) -> bool {
    player.dead || now < player.timers.combat_until
}

/// Départ effectif vers `at` : op 1 (position), déplacement, op 8.
async fn travel_to(ctx: &WorldCtx, st: &mut WorldState, sid: u32, at: Vec3) -> anyhow::Result<()> {
    let cid = st.players[&sid].ch.id;
    let start = packet(op::TELEPORT_START, None, cid).with(4, at);
    st.players[&sid].send(&schema::EVENT_TELEPORT, start);
    super::teleport(ctx, st, sid, at.x as f64, Some(at.y as f64), at.z as f64).await?;
    st.players[&sid].send(&schema::EVENT_TELEPORT, packet(op::LOADING, None, cid));
    Ok(())
}

/// Op 7 : « Retour » depuis un point d'où l'on ne peut que revenir (zones de combat entre joueurs).
async fn go_back(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some((_, portal, _)) = portal_of(ctx, p) else { return Ok(()) };
    let Some(here) = ctx.portals.points.get(&portal.point) else { return Ok(()) };
    let Some(player) = st.players.get(&sid) else { return Ok(()) };
    if here.special_type != SPECIAL_RETURN_ONLY {
        warn!("[portail] {} : retour demandé depuis « {} », qui n'est pas un point de retour", player.ch.name, portal.point);
        return Ok(());
    }
    if unable(player, Instant::now()) || dist2d(player.ch.pos(), portal.pos) > MAX_USE_RANGE {
        return Ok(());
    }
    // Arrivée : autour du point de départ du dernier trajet, sinon au village de rattachement.
    let from = player.trip_from.clone();
    let back = from.as_deref().and_then(|n| ctx.portals.points.get(n)).and_then(|pt| pt.at.and_then(|c| landing(ctx, pt, c, &mut rand::thread_rng())));
    let at = match back {
        Some(at) => at,
        None => {
            let (x, y, z) = super::travel::home_village(ctx, player);
            Vec3::new(x as f32, y.unwrap_or(player.ch.pos_y) as f32, z as f32)
        }
    };
    let name = player.ch.name.clone();
    st.players.get_mut(&sid).unwrap().trip_from = None;
    travel_to(ctx, st, sid, at).await?;
    info!("[portail] {name} : retour depuis « {} » vers « {} »", portal.point, from.as_deref().unwrap_or("village"));
    Ok(())
}

/// Op 4 : le joueur s'adresse à un portail.
fn request(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) {
    let Some((object, portal, base)) = portal_of(ctx, p) else {
        warn!("[portail] #{sid} demande sur un objet sans portail");
        return;
    };
    let Some(point) = ctx.portals.points.get(&portal.point) else { return };
    let Some(player) = st.players.get_mut(&sid) else { return };
    if unable(player, Instant::now()) || !point.admits(player.factors.ftype[0] as i64) {
        return;
    }
    player.attack = None;
    let disabled = ctx.portals.disabled(point, player.factors.ftype[0] as i64);
    let pending = Pending { object, pos: portal.pos, base, disabled };
    let distance = dist2d(player.ch.pos(), portal.pos);
    if distance <= MAX_USE_RANGE {
        grant(st, sid, &pending);
        return;
    }
    // Trop loin : le joueur court jusqu'à bonne distance du portail, l'accord suit à l'arrivée à portée.
    let from = player.ch.pos();
    let k = ((distance - 0.8 * MAX_USE_RANGE) / distance) as f32;
    let dest = Vec3::new(from.x + (portal.pos.x - from.x) * k, from.y, from.z + (portal.pos.z - from.z) * k);
    super::presence::run_to(st, sid, dest, Instant::now());
    info!("[portail] {} court vers « {} » ({distance:.0} unités)", st.players[&sid].ch.name, portal.point);
    st.players.get_mut(&sid).unwrap().portal = Some(pending);
}

/// Accord (op 5) : le client ouvre la liste des destinations ; le joueur s'arrête.
fn grant(st: &mut WorldState, sid: u32, pending: &Pending) {
    let Some(player) = st.players.get_mut(&sid) else { return };
    player.portal = None;
    let cid = player.ch.id;
    let mut grant = packet(op::GRANTED, Some(&pending.base), cid);
    if !pending.disabled.is_empty() {
        grant = grant.with(5, pending.disabled.clone());
    }
    player.send(&schema::EVENT_TELEPORT, grant);
    if player.dest.is_some() {
        super::presence::stop(st, sid);
    }
}

/// Accorde les demandes des joueurs arrivés à portée de leur portail.
pub fn step(st: &mut WorldState) {
    let ready: Vec<(u32, Pending)> = st
        .players
        .iter()
        .filter_map(|(sid, p)| p.portal.as_ref().filter(|w| dist2d(p.ch.pos(), w.pos) <= MAX_USE_RANGE).map(|w| (*sid, w.clone())))
        .collect();
    for (sid, pending) in ready {
        grant(st, sid, &pending);
    }
}

/// Lieu d'arrivée : premier des huit points du cercle autour de `center` qui n'est pas infranchissable.
pub fn landing(ctx: &WorldCtx, point: &Point, center: Vec3, rng: &mut impl Rng) -> Option<Vec3> {
    let (lo, hi) = (point.radius_min.min(point.radius_max), point.radius_min.max(point.radius_max));
    let radius = if hi > lo { rng.gen_range(lo..hi) } else { lo };
    let start = rng.gen_range(-4.0..4.0f64) * 45.0;
    (0..8).find_map(|i| {
        let a = (start + i as f64 * 45.0).to_radians();
        let (x, z) = (center.x as f64 + radius * a.cos(), center.z as f64 + radius * a.sin());
        (!ctx.map.is_blocked(x, z)).then(|| Vec3::new(x as f32, ctx.map.height(x, z).unwrap_or(center.y), z as f32))
    })
}

/// Op 0 : le joueur choisit sa destination.
async fn teleport(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some((_, portal, _)) = portal_of(ctx, p) else { return Ok(()) };
    let target_name = point_name(p.bytes(3).unwrap_or_default());
    let Some(player) = st.players.get(&sid) else { return Ok(()) };
    let race = player.factors.ftype[0] as i64;
    let (Some(src), Some(dst)) = (ctx.portals.points.get(&portal.point), ctx.portals.points.get(&target_name)) else {
        warn!("[portail] {} : point « {target_name} » inconnu", player.ch.name);
        return Ok(());
    };
    if unable(player, Instant::now()) {
        return Ok(());
    }
    // Le joueur doit être au portail de départ (l'accord a été donné à moins de 1600 unités).
    if dist2d(player.ch.pos(), portal.pos) > MAX_USE_RANGE * 2.0 {
        warn!("[portail] {} trop loin de « {} »", player.ch.name, portal.point);
        return Ok(());
    }
    if !src.admits(race) || !dst.admits(race) || !src.leads_to(dst) {
        warn!("[portail] {} : trajet « {} » → « {target_name} » refusé", player.ch.name, portal.point);
        return Ok(());
    }
    // Trajets des châteaux (SpecialType 2 à 4) : selon le propriétaire et le siège (siege/war.rs).
    if let Err(why) = super::siege::war::portal_check(ctx, st, sid, (src.special_type, portal.pos), (dst.special_type, dst.at)) {
        info!("[portail] {} : trajet « {} » → « {target_name} » refusé ({why})", player.ch.name, portal.point);
        send_system(st, sid, &why);
        return Ok(());
    }
    let Some(center) = dst.at else {
        warn!("[portail] « {target_name} » n'a pas d'objet d'arrivée");
        return Ok(());
    };
    // Prix du trajet taxé dans la région du départ (tax.rs).
    let taxed = super::tax::quote(ctx, st, sid, ctx.portals.fee(&portal.point, &target_name));
    let fee = taxed.total;
    let money = player.ch.money as i64;
    if fee > money {
        send_system(st, sid, &format!("Il faut {fee} pièce(s) d'or pour ce trajet."));
        return Ok(());
    }
    let Some(at) = landing(ctx, dst, center, &mut rand::thread_rng()) else {
        warn!("[portail] pas de place libre autour de « {target_name} »");
        return Ok(());
    };
    let name = player.ch.name.clone();
    if fee > 0 {
        super::set_money(ctx, st, sid, money - fee).await?;
        super::tax::collect(ctx, st, &taxed).await?;
    }
    // Point de départ noté, pour un « Retour » depuis l'arrivée (module d'Archonia, op 0 réussie).
    st.players.get_mut(&sid).unwrap().trip_from = Some(portal.point.clone());
    travel_to(ctx, st, sid, at).await?;
    let zone = if dst.region_type == REGION_PVP { " (zone de combat entre joueurs)" } else { "" };
    info!("[portail] {name} : « {} » → « {target_name} »{zone} pour {fee} pièce(s)", portal.point);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(use_type: i64, groups: &[&str], targets: &[&str]) -> Point {
        let s = |l: &[&str]| l.iter().map(|g| g.to_string()).collect();
        Point { id: 0, radius_min: 400.0, radius_max: 600.0, use_type, region_type: 0, special_type: 0, groups: s(groups), target_groups: s(targets), at: None }
    }

    #[test]
    fn races_et_groupes() {
        let village = point(0, &["1.A"], &["1.A", "2.B"]);
        let mine = point(4, &["2.B"], &["2.B"]);
        assert!(village.admits(2) && mine.admits(3) && !mine.admits(1));
        assert!(village.leads_to(&mine) && !mine.leads_to(&village));
        assert_eq!(point_name(b"Village\0\0\0"), "Village");
        assert_eq!(point_name(&[0xB8, 0xB6, 0xC0, 0xBB, 0]), "마을");
    }

    #[test]
    fn points_desactives() {
        let at = Some(Vec3::new(0.0, 0.0, 0.0));
        let mut r = Portals::default();
        let mut add = |name: &str, id: i64, p: Point| {
            r.order.push(name.to_string());
            r.points.insert(name.to_string(), Point { id, ..p });
        };
        add("Tullan", 1, Point { at, ..point(0, &["1.A"], &["2.B", "1.A"]) });
        add("Fort", 2, point(1, &["2.B"], &[]));
        add("Mine", 3, Point { at, ..point(4, &["2.B"], &[]) });
        add("Village", 4, Point { at, ..point(0, &["1.A", "2.B"], &[]) });
        let from = r.points["Tullan"].clone();
        let ids: Vec<i64> = r.destinations(&from).iter().map(|p| p.id).collect();
        assert_eq!(ids, vec![2, 3, 4], "groupe 2.B puis 1.A, sans doublon ni départ");
        // Humain : le fort n'a pas d'objet d'arrivée, la mine n'admet que les elfes de lune.
        assert_eq!(r.disabled(&from, 1), [2i32.to_le_bytes(), 3i32.to_le_bytes()].concat());
        assert_eq!(r.disabled(&from, 3), 2i32.to_le_bytes().to_vec());
    }
}
