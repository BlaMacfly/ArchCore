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

use super::items::{item_count, item_remove, InventoryItem};
use super::recovery::used_packet;
use super::{send_system, Player, WorldCtx, WorldState};
use crate::info;
use archcore_net::schema;
use archcore_net::{build_packet, Fields, Vec3};
use std::time::{Duration, Instant};

/// Opérations AGPMITEM du retour par parchemin.
pub const USE_RETURN_SCROLL: i64 = 6;
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
    if let Some(pt) = ctx.regions.resurrection(p.bound_region, here, f.ftype[0] as i64, f.ftype[2] as i64) {
        return (pt.x as f64, Some(pt.y as f64), pt.z as f64);
    }
    let villages = &ctx.config.world.start_positions;
    match villages.get(&p.factors.ftype[0].to_string()).or_else(|| villages.get("1")) {
        Some(v) => (v.x, Some(v.y), v.z),
        None => (p.ch.pos_x, None, p.ch.pos_z),
    }
}

fn return_packet(op: i64, cid: u32) -> Vec<u8> {
    let s = &schema::ITEM;
    build_packet(s, &Fields::new(s).with(0, op).with(4, cid))
}

/// Utilise un parchemin de l'inventaire.
pub async fn use_scroll(ctx: &WorldCtx, st: &mut WorldState, sid: u32, item: &InventoryItem, scroll: Scroll, now: Instant) -> anyhow::Result<()> {
    let p = &st.players[&sid];
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
        Some(left) => player.send_raw(item_count(&left, cid)),
        None => player.send_raw(item_remove(item.iid, Some(cid))),
    }
    player.send_raw(used_packet(item.tid, cid, scroll.delay));
    player.attack = None;
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
    if now < p.timers.combat_until {
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
}
