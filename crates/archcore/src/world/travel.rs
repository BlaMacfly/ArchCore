//! Parchemins de téléportation (« Passport ») : retour au village, aller-retour, destination fixe.
//!
//! Un parchemin est un objet utilisable (ItemType 1) de Type 4 dans itemdatatable.txt ; son SubType donne sa
//! sorte : 0 retour au village, 1 aller-retour, 2 destination fixe (colonne « Destination » : x;y;z),
//! 3 retour au village immédiat. Il s'utilise par AGPMITEM op 3 comme une potion : un exemplaire est
//! consommé et l'op 4 confirme. Le retour au village (0 et 1) est refusé en combat. Le « village » est le
//! point de résurrection de la région de rattachement le plus proche (`regions.rs`), comme la résurrection ;
//! à défaut, le village de départ de la race.
//!
//! L'aller-retour mémorise l'endroit quitté et l'annonce au client par AGPMITEM op 9 ; le client demande à
//! y revenir par op 6, refusé en combat ; le serveur répond op 10 (retour désactivé) et y ramène le joueur.
//! Un retour refusé est signalé par l'op 7 (champ 4 = CID) : ni Archonia ni le client de Webzen n'en montrent rien
//! (seul abonné, AgcmUIItem, ne fait rien : Archonia 0x1400f62d0, Webzen 0x67ac90) ; le texte reste celui
//! d'ArchCore. Le client renonce au retour par op 8 (AgcmItem FUN_1406480c0, seulement si le retour est possible) :
//! le point est oublié et l'op 10 masque le bouton, comme après un retour.

use super::items::{item_remove, InventoryItem};
use super::recovery::used_packet;
use super::{send_system, Player, WorldCtx, WorldState};
use crate::info;
use archcore_net::schema;
use archcore_net::{build_packet, Fields, Vec3};
use std::time::{Duration, Instant};

/// Opérations AGPMITEM du retour par parchemin.
pub const USE_RETURN_SCROLL: i64 = 6;
pub const USE_RETURN_SCROLL_FAILED: i64 = 7;
pub const CANCEL_RETURN_SCROLL: i64 = 8;
pub const ENABLE_RETURN_SCROLL: i64 = 9;
pub const DISABLE_RETURN_SCROLL: i64 = 10;

/// Sorte de parchemin (SubType).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScrollKind {
    GoTown,
    Roundtrip,
    Destination(Vec3),
    GoTownNow,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scroll {
    pub kind: ScrollKind,
    pub delay: Duration,
}

impl Scroll {
    /// Lit une ligne d'itemdatatable.txt : ItemType 1, Type 4.
    pub fn from_raw(raw: &serde_json::Value) -> Option<Self> {
        let text = |k: &str| raw.get(k).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let num = |k: &str| text(k).parse::<f64>().unwrap_or(-1.0) as i64;
        if num("ItemType") != 1 || num("Type") != 4 {
            return None;
        }
        let kind = match num("SubType") {
            1 => ScrollKind::Roundtrip,
            2 => {
                let v: Vec<f32> = text("Destination").split(';').filter_map(|x| x.trim().parse().ok()).collect();
                if v.len() != 3 {
                    return None;
                }
                ScrollKind::Destination(Vec3::new(v[0], v[1], v[2]))
            }
            3 => ScrollKind::GoTownNow,
            _ => ScrollKind::GoTown,
        };
        let ms = num("Use Interval").max(0) as u64;
        Some(Scroll { kind, delay: Duration::from_millis(ms) })
    }
}

/// Point de résurrection de la région de rattachement le plus proche du joueur ; à défaut, le village de départ
/// de sa race (ou celui des humains), sinon sa position.
pub fn home_village(ctx: &WorldCtx, p: &Player) -> (f64, Option<f64>, f64) {
    let here = p.dest.unwrap_or_else(|| p.ch.pos());
    let f = &p.factors;
    village_point(ctx, p.bound_region, here, f.ftype[0], f.ftype[2])
}

/// Point de résurrection de la région `bound` le plus proche de `here` permis à la race et à la classe ; à défaut, le
/// village de départ de la race (ou celui des humains), sinon `here`.
pub fn village_point(ctx: &WorldCtx, bound: u16, here: Vec3, race: i32, class: i32) -> (f64, Option<f64>, f64) {
    if let Some(pt) = ctx.regions.resurrection(bound, here, race as i64, class as i64) {
        return (pt.x as f64, Some(pt.y as f64), pt.z as f64);
    }
    let villages = &ctx.config.world.start_positions;
    match villages.get(&race.to_string()).or_else(|| villages.get("1")) {
        Some(v) => (v.x, Some(v.y), v.z),
        None => (here.x as f64, None, here.z as f64),
    }
}

/// Région à niveau limité (« LevelLimit » de la région ou de sa parente).
fn level_limited(ctx: &WorldCtx, region: Option<u16>) -> bool {
    region.and_then(|r| ctx.regions.level_range(r)).is_some_and(|(_, limit)| limit > 0)
}

fn return_packet(op: i64, cid: u32) -> Vec<u8> {
    let s = &schema::ITEM;
    build_packet(s, &Fields::new(s).with(0, op).with(4, cid))
}

/// Utilise un parchemin de l'inventaire.
pub async fn use_scroll(ctx: &WorldCtx, st: &mut WorldState, sid: u32, item: &InventoryItem, scroll: Scroll, now: Instant) -> anyhow::Result<()> {
    let p = &st.players[&sid];
    // Zone de combat dangereuse : aucun parchemin (« Passports can not be used at Dangerous Battlefields »,
    // FUN_14053d0b0 d'Archonia, qui le refuse aussi de lui-même).
    if super::pvp::zone_mode(ctx, st, sid) == super::pvp::mode::COMBAT {
        p.send_raw(super::pvp::message(super::pvp::code::CANNOT_USE_TELEPORT, None, None));
        return Ok(());
    }
    // Région à niveau limité (Battle Square…) : aucun parchemin.
    let region = ctx.map.region(p.ch.pos_x, p.ch.pos_z);
    if level_limited(ctx, region) {
        send_system(st, sid, "Les parchemins ne s'utilisent pas dans cette région.");
        return Ok(());
    }
    // Aller-retour depuis une région sûre : refusé (rien à quitter).
    if scroll.kind == ScrollKind::Roundtrip && region.is_none_or(|r| ctx.regions.is_safe(r)) {
        send_system(st, sid, "Un aller-retour ne s'utilise pas en région sûre.");
        return Ok(());
    }
    let fighting = now < p.timers.combat_until;
    if fighting && matches!(scroll.kind, ScrollKind::GoTown | ScrollKind::Roundtrip) {
        send_system(st, sid, "Impossible de rentrer au village pendant un combat.");
        return Ok(());
    }
    let (x, y, z) = match scroll.kind {
        ScrollKind::Destination(d) => (d.x as f64, Some(d.y as f64), d.z as f64),
        _ => home_village(ctx, p),
    };
    let player = st.players.get_mut(&sid).unwrap();
    let (cid, name, here) = (player.ch.id, player.ch.name.clone(), player.dest.unwrap_or_else(|| player.ch.pos()));
    match player.inventory.consume(&ctx.db, item.iid, 1).await? {
        Some(left) => player.send_raw(player.inventory.count_packet(&left, cid)),
        None => player.send_raw(item_remove(item.iid, Some(cid))),
    }
    player.send_raw(used_packet(item.tid, cid, scroll.delay));
    player.attack = None;
    player.skills.cast = None;
    if scroll.kind == ScrollKind::Roundtrip {
        player.return_point = Some(here);
        player.send_raw(return_packet(ENABLE_RETURN_SCROLL, cid));
    }
    let label = ctx.catalog.get(item.tid).map(|d| d.name.clone()).unwrap_or_default();
    info!("[parchemin] {name} utilise {label} : vers {x:.0}, {z:.0}");
    super::teleport(ctx, st, sid, x, y, z).await?;
    Ok(())
}

/// AGPMITEM op 6 : revenir à l'endroit quitté par un aller-retour.
pub async fn use_return(ctx: &WorldCtx, st: &mut WorldState, sid: u32, now: Instant) -> anyhow::Result<()> {
    let Some(p) = st.players.get_mut(&sid) else { return Ok(()) };
    if !p.in_game || p.dead {
        return Ok(());
    }
    let Some(back) = p.return_point else { return Ok(()) };
    // Ni en zone de combat dangereuse ni en région à niveau limité.
    let region = ctx.map.region(p.ch.pos_x, p.ch.pos_z);
    if super::pvp::zone_mode(ctx, st, sid) == super::pvp::mode::COMBAT || level_limited(ctx, region) {
        let p = &st.players[&sid];
        p.send_raw(return_packet(USE_RETURN_SCROLL_FAILED, p.ch.id));
        send_system(st, sid, "Impossible de revenir depuis cette région.");
        return Ok(());
    }
    let Some(p) = st.players.get_mut(&sid) else { return Ok(()) };
    if now < p.timers.combat_until {
        p.send_raw(return_packet(USE_RETURN_SCROLL_FAILED, p.ch.id));
        send_system(st, sid, "Impossible de revenir pendant un combat.");
        return Ok(());
    }
    p.return_point = None;
    let (cid, name) = (p.ch.id, p.ch.name.clone());
    p.send_raw(return_packet(DISABLE_RETURN_SCROLL, cid));
    info!("[parchemin] {name} revient en {:.0}, {:.0}", back.x, back.z);
    super::teleport(ctx, st, sid, back.x as f64, Some(back.y as f64), back.z as f64).await?;
    Ok(())
}

/// AGPMITEM op 8 : renoncer au retour d'un aller-retour.
pub fn cancel_return(st: &mut WorldState, sid: u32) {
    let Some(p) = st.players.get_mut(&sid) else { return };
    if p.return_point.take().is_some() {
        p.send_raw(return_packet(DISABLE_RETURN_SCROLL, p.ch.id));
        info!("[parchemin] {} renonce au retour", p.ch.name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lecture_des_parchemins() {
        let p = |t: &str, st: &str, d: &str| serde_json::json!({"ItemType":"1","Type":t,"SubType":st,"Destination":d,"Use Interval":"1000"});
        assert_eq!(Scroll::from_raw(&p("4", "0", "")).unwrap().kind, ScrollKind::GoTown);
        let r = Scroll::from_raw(&p("4", "1", "")).unwrap();
        assert_eq!((r.kind, r.delay), (ScrollKind::Roundtrip, Duration::from_secs(1)));
        assert_eq!(Scroll::from_raw(&p("4", "2", "-353400;2125;81200")).unwrap().kind, ScrollKind::Destination(Vec3::new(-353400.0, 2125.0, 81200.0)));
        assert_eq!(Scroll::from_raw(&p("4", "3", "")).unwrap().kind, ScrollKind::GoTownNow);
        assert!(Scroll::from_raw(&p("4", "2", "abc")).is_none(), "destination illisible");
        assert!(Scroll::from_raw(&p("0", "0", "")).is_none(), "pas un parchemin");
    }

    #[test]
    fn retour_refuse_ou_abandonne() {
        let s = &schema::ITEM;
        let p = archcore_net::decode_packet(s, &return_packet(USE_RETURN_SCROLL_FAILED, 42)).unwrap();
        assert_eq!((p.int(0), p.int(4), p.int(2)), (Some(7), Some(42), None));
        let p = archcore_net::decode_packet(s, &return_packet(DISABLE_RETURN_SCROLL, 42)).unwrap();
        assert_eq!((p.int(0), p.int(4)), (Some(10), Some(42)));
        assert_eq!((USE_RETURN_SCROLL_FAILED, CANCEL_RETURN_SCROLL), (7, 8));
    }
}
