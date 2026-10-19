//! Présence des joueurs dans le monde : déplacement, visibilité mutuelle, sauvegarde de la position.
//!
//! Le déplacement est un paquet AGPMOPTIMIZEDCHARMOVE (0x40), drapeaux sur 1 octet :
//! 0 CID, 1 position actuelle, 2 destination, 3 cible suivie, 4 distance de suivi (u16),
//! 5 drapeaux de déplacement, 6 direction. Drapeaux (champ 5) : 0x04 arrêt, 0x01 direction, 0x08 synchro.
//! Visibilité : AGPMCHARACTER op 0 ajoute un personnage chez le client, op 2 le retire.

use super::{character_add_packet, character_remove_packet, send_nearby_npcs, WorldCtx, WorldState};
use crate::{debug, info, warn};
use archcore_net::schema;
use archcore_net::{Fields, Packet, Vec3};
use std::sync::Arc;
use std::time::Duration;

pub mod move_flag {
    pub const STOP: i64 = 0x04;
    pub const SYNC: i64 = 0x08;
}

const SAVE_INTERVAL: Duration = Duration::from_secs(60);

/// Le joueur vient d'entrer en jeu : il voit les joueurs proches, et eux le voient.
pub fn enter(_ctx: &WorldCtx, st: &mut WorldState, sid: u32) {
    if let Some(p) = st.players.get_mut(&sid) {
        p.seen_players.clear();
        p.in_game = true;
    }
    refresh(st, sid);
}

/// Le joueur quitte le monde : on le retire chez les autres et on sauvegarde sa position.
pub async fn leave(ctx: &WorldCtx, st: &mut WorldState, sid: u32) -> anyhow::Result<()> {
    let Some(p) = st.players.get_mut(&sid) else { return Ok(()) };
    if !p.in_game {
        return Ok(());
    }
    if let Some(d) = p.dest {
        p.ch.set_pos(d);
    }
    info!("[world] #{sid} {} quitte le monde en {:.0}, {:.0} ({} déplacement(s))", p.ch.name, p.ch.pos_x, p.ch.pos_z, p.move_count);
    p.in_game = false;
    let cid = p.ch.id;
    for other in st.sids() {
        if other == sid {
            continue;
        }
        let o = st.players.get_mut(&other).unwrap();
        if o.seen_players.remove(&cid) {
            o.send_raw(character_remove_packet(cid));
        }
    }
    save(ctx, st, sid).await
}

/// Paquet de déplacement reçu du client.
pub fn on_move(ctx: &WorldCtx, st: &mut WorldState, sid: u32, packet: &Packet) {
    let Some(p) = st.players.get_mut(&sid) else { return };
    if !p.in_game {
        return;
    }
    if let Some(cid) = packet.int(0).filter(|&c| c != 0) {
        if cid as u32 != p.ch.id {
            warn!("[world] #{sid} déplacement pour le CID {cid} refusé (joueur {})", p.ch.id);
            return;
        }
    }
    if p.dead {
        return;
    }
    // Un déplacement demandé par le joueur interrompt son attaque.
    p.attack = None;
    let flags = packet.int(5).unwrap_or(0);
    let dest = packet.vec3(2).filter(Vec3::is_finite);
    // Le client n'envoie pas toujours sa position actuelle : on prend alors la destination précédente,
    // supposée atteinte, sinon la dernière position connue.
    let given = packet.vec3(1).filter(Vec3::is_finite);
    let pos = given.or(p.dest).unwrap_or_else(|| p.ch.pos());
    p.ch.set_pos(pos);
    p.dest = if flags & move_flag::STOP != 0 { None } else { dest };
    p.move_flags = flags;
    p.direction = packet.int(6).unwrap_or(0);
    p.follow_target = packet.int(3).unwrap_or(0);
    p.follow_distance = packet.int(4).unwrap_or(0);
    p.dirty = true;
    p.move_count += 1;
    if p.move_count == 1 {
        info!(
            "[world] #{sid} premier déplacement reçu de {} : position {}, destination {}, drapeaux 0x{flags:x}",
            p.ch.name,
            if given.is_some() { "fournie" } else { "absente" },
            if dest.is_some() { "fournie" } else { "absente" }
        );
    }
    match dest {
        Some(d) => debug!("[world] #{sid} {} → {:.0}, {:.0} (drapeaux 0x{flags:x})", p.ch.name, d.x, d.z),
        None => debug!("[world] #{sid} {} → arrêt (drapeaux 0x{flags:x})", p.ch.name),
    }
    let cid = p.ch.id;
    let mv = move_fields(st, sid);
    for other in st.sids() {
        if other == sid {
            continue;
        }
        let o = &st.players[&other];
        if o.seen_players.contains(&cid) {
            o.send(&schema::OPT_CHAR_MOVE, mv.clone());
        }
    }
    // Rattachement à la dernière région sûre traversée.
    let at = dest.unwrap_or(pos);
    if let Some(region) = ctx.map.region(at.x as f64, at.z as f64).filter(|r| ctx.regions.is_safe(*r)) {
        st.players.get_mut(&sid).unwrap().bound_region = region;
    }
    refresh(st, sid);
    send_nearby_npcs(ctx, st, sid, dest.unwrap_or(pos));
}

/// Recalcule ce que voit `sid` et qui le voit : ajoute les joueurs entrés dans le rayon, retire ceux
/// qui en sont sortis. Symétrique.
pub fn refresh(st: &mut WorldState, sid: u32) {
    let radius = st.view_radius();
    let Some(p) = st.players.get(&sid) else { return };
    if !p.in_game {
        return;
    }
    let me = p.ch.pos();
    for other in st.sids() {
        if other == sid {
            continue;
        }
        let o = &st.players[&other];
        if !o.in_game {
            continue;
        }
        let near = near(me, o.ch.pos(), radius);
        set_visible(st, sid, other, near);
        set_visible(st, other, sid, near);
    }
}

fn near(a: Vec3, b: Vec3, radius: f64) -> bool {
    let dx = (a.x - b.x) as f64;
    let dz = (a.z - b.z) as f64;
    dx * dx + dz * dz <= radius * radius
}

/// Rend `target` visible (ou non) pour `viewer`.
fn set_visible(st: &mut WorldState, viewer: u32, target: u32, visible: bool) {
    let t = &st.players[&target];
    let tid = t.ch.id;
    let add = character_add_packet(t);
    let gear = super::equipment::view_packets(t);
    let has_dest = t.dest.is_some();
    let mv = if has_dest { Some(move_fields(st, target)) } else { None };
    let v = st.players.get_mut(&viewer).unwrap();
    if visible && v.seen_players.insert(tid) {
        v.send_raw(add);
        // Équipement porté, après le personnage auquel il se rattache.
        for g in gear {
            v.send_raw(g);
        }
        if let Some(mv) = mv {
            v.send(&schema::OPT_CHAR_MOVE, mv);
        }
    } else if !visible && v.seen_players.remove(&tid) {
        v.send_raw(character_remove_packet(tid));
    }
}

/// Champs du paquet de déplacement d'un joueur, relayé aux autres.
pub fn move_fields(st: &WorldState, sid: u32) -> Fields {
    let p = &st.players[&sid];
    let pos = p.ch.pos();
    let s = &schema::OPT_CHAR_MOVE;
    let follow = p.follow_target != 0;
    Fields::new(s)
        .with(0, p.ch.id)
        .with(1, pos)
        .with(2, p.dest.unwrap_or(pos))
        .with_opt(3, follow.then_some(p.follow_target))
        .with_opt(4, follow.then_some(p.follow_distance))
        .with(5, if p.dest.is_some() { p.move_flags & !move_flag::STOP } else { move_flag::STOP | move_flag::SYNC })
        .with(6, p.direction)
}

pub async fn save(ctx: &WorldCtx, st: &mut WorldState, sid: u32) -> anyhow::Result<()> {
    let Some(p) = st.players.get_mut(&sid) else { return Ok(()) };
    ctx.db
        .characters
        .exec("UPDATE characters SET pos_x = ?, pos_y = ?, pos_z = ? WHERE id = ?", (p.ch.pos_x, p.ch.pos_y, p.ch.pos_z, p.ch.id))
        .await?;
    p.dirty = false;
    Ok(())
}

/// Sauvegarde périodique des positions des joueurs qui ont bougé.
pub fn spawn_autosave(world: Arc<super::World>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(SAVE_INTERVAL);
        tick.tick().await;
        loop {
            tick.tick().await;
            let mut st = world.state.lock().await;
            for sid in st.sids() {
                if st.players[&sid].dirty {
                    if let Err(e) = save(&world.ctx, &mut st, sid).await {
                        warn!("[world] sauvegarde des positions : {e:#}");
                    }
                }
            }
        }
    });
}

impl WorldState {
    /// Rayon de vue, recopié dans l'état pour les calculs de visibilité (voir `World::new`).
    pub fn view_radius(&self) -> f64 {
        self.view_radius
    }
}
