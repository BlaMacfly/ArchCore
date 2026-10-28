//! Présence des joueurs dans le monde : déplacement, visibilité mutuelle, sauvegarde de la position.
//!
//! Le déplacement est un paquet AGPMOPTIMIZEDCHARMOVE (0x40), drapeaux sur 1 octet :
//! 0 CID, 1 position actuelle, 2 destination, 3 cible suivie, 4 distance de suivi (u16),
//! 5 drapeaux de déplacement, 6 direction. Drapeaux (champ 5) : 0x04 arrêt, 0x01 direction, 0x08 synchro.
//! Visibilité : AGPMCHARACTER op 0 ajoute un personnage chez le client, op 2 le retire. PNJ, monstres et
//! objets au sol se voient dans les 3 × 3 secteurs autour du joueur (`sector.rs`).
//! Marche : le client n'envoie que sa destination ; le serveur avance lui-même le joueur à chaque tour (vitesse
//! de marche ou de course en dixièmes d'unité par seconde), l'arrête à destination et recalcule sa vue quand il
//! change de secteur. Le sol infranchissable n'arrête pas la marche : le client gère lui-même les obstacles,
//! et les tables de blocage arrêteraient le serveur là où le client passe.

use super::{character_add_packet, character_remove_packet, sector, send_nearby_npcs, WorldCtx, WorldState};
use crate::{debug, info, warn};
use archcore_net::schema;
use archcore_net::{Fields, Packet, Vec3};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Drapeau « course » du déplacement.
const MOVE_FAST: i64 = 0x10;

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
    // Un déplacement demandé par le joueur interrompt son attaque, la compétence en préparation et la
    // course vers un portail.
    p.attack = None;
    p.skills.cast = None;
    p.portal = None;
    let flags = packet.int(5).unwrap_or(0);
    let dest = packet.vec3(2).filter(Vec3::is_finite);
    // Position : celle que le serveur tient, la marche en cours avancée jusqu'à maintenant. Le vrai client ne
    // l'envoie pas ; si elle est fournie (simulateur, outils), elle la remplace.
    advance(p, Instant::now());
    let tracked = p.ch.pos();
    let given = packet.vec3(1).filter(Vec3::is_finite);
    let pos = given.unwrap_or_else(|| p.ch.pos());
    p.ch.set_pos(pos);
    p.view_pos = pos;
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
        Some(d) => debug!("[world] #{sid} {} → {:.0}, {:.0} (drapeaux 0x{flags:x}) depuis {:.0}, {:.0} (position suivie {:.0}, {:.0})", p.ch.name, d.x, d.z, pos.x, pos.z, tracked.x, tracked.z),
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
    // Vue autour de la position actuelle, pas de la destination : la marche est suivie par `view_step`.
    send_nearby_npcs(ctx, st, sid, pos);
}

/// Un pas de marche : `speed` en dixièmes d'unité par seconde, `dt` en secondes. Rend la nouvelle position
/// (hauteur inchangée en chemin) et vrai si la destination est atteinte.
pub fn step_toward(pos: Vec3, dest: Vec3, speed: f64, dt: f64) -> (Vec3, bool) {
    let (dx, dz) = ((dest.x - pos.x) as f64, (dest.z - pos.z) as f64);
    let total = (dx * dx + dz * dz).sqrt();
    let done = speed.max(0.0) * dt / 10.0;
    if total <= 0.0 || done >= total {
        return (dest, true);
    }
    (Vec3::new(pos.x + (done * dx / total) as f32, pos.y, pos.z + (done * dz / total) as f32), false)
}

/// Avance la marche d'un joueur jusqu'à `now` ; arrêt à destination.
fn advance(p: &mut super::Player, now: Instant) {
    let dt = now.saturating_duration_since(p.moved_at).as_secs_f64();
    p.moved_at = now;
    let Some(dest) = p.dest else { return };
    if p.dead {
        return;
    }
    let speed = if p.move_flags & MOVE_FAST != 0 { p.factors.status[9] } else { p.factors.status[8] } as f64;
    let (next, arrived) = step_toward(p.ch.pos(), dest, speed, dt);
    p.ch.set_pos(next);
    if arrived {
        p.dest = None;
    }
}

/// Fait marcher les joueurs ; à chaque changement de secteur, leur vue est recalculée (PNJ, monstres, objets
/// au sol, joueurs).
pub fn move_step(ctx: &WorldCtx, st: &mut WorldState, now: Instant) {
    for sid in st.sids() {
        let Some(p) = st.players.get_mut(&sid) else { continue };
        if !p.in_game {
            continue;
        }
        if crate::probe::every_second(sid) {
            let (pos, dest) = (p.ch.pos(), p.dest);
            crate::probe::line(&format!(
                "POS #{sid} {} ({:.0}, {:.0}, {:.0}) secteur {:?} dest {} vus {} monstres, {} PNJ",
                p.ch.name,
                pos.x,
                pos.y,
                pos.z,
                sector::of(pos),
                dest.map(|d| format!("({:.0}, {:.0})", d.x, d.z)).unwrap_or_else(|| "-".into()),
                p.seen_monsters.len(),
                p.seen_npcs.len()
            ));
        }
        if p.dest.is_none() {
            p.moved_at = now;
            continue;
        }
        advance(p, now);
        let at = p.ch.pos();
        let changed = sector::of(at) != sector::of(p.view_pos);
        p.view_pos = at;
        if changed {
            debug!("[vue] #{sid} {} passe au secteur {:?} en {:.0}, {:.0}", p.ch.name, sector::of(at), at.x, at.z);
            send_nearby_npcs(ctx, st, sid, at);
            refresh(st, sid);
        }
    }
}

/// Recalcule ce que voit `sid` et qui le voit : ajoute les joueurs entrés dans les secteurs voisins, retire
/// ceux qui en sont sortis. Symétrique.
pub fn refresh(st: &mut WorldState, sid: u32) {
    let Some(p) = st.players.get(&sid) else { return };
    if !p.in_game {
        return;
    }
    let me = p.view_pos;
    for other in st.sids() {
        if other == sid {
            continue;
        }
        let o = &st.players[&other];
        if !o.in_game {
            continue;
        }
        // Apparition dans les 3 × 3 secteurs, retrait au-delà des 5 × 5 (voir `sector.rs`).
        let (see, keep) = (sector::sees(me, o.view_pos), sector::keeps(me, o.view_pos));
        let (me_cid, o_cid) = (st.players[&sid].ch.id, o.ch.id);
        // Les membres d'un même groupe se voient à toute distance.
        let grouped = st.parties.same(me_cid, o_cid);
        let mine = grouped || if st.players[&sid].seen_players.contains(&o_cid) { keep } else { see };
        let theirs = grouped || if st.players[&other].seen_players.contains(&me_cid) { keep } else { see };
        set_visible(st, sid, other, mine);
        set_visible(st, other, sid, theirs);
    }
}

/// Rend `target` visible (ou non) pour `viewer`.
pub(super) fn set_visible(st: &mut WorldState, viewer: u32, target: u32, visible: bool) {
    let t = &st.players[&target];
    let tid = t.ch.id;
    let add = character_add_packet(t);
    let gear = super::equipment::view_packets(t);
    // Monture portée, ou Alcadia de l'ArchLord (op 6, archlord.rs).
    let ride = super::ride::view_packet(t).or_else(|| super::archlord::dragon_view(st, tid));
    let link = super::guild::view_packet(st, tid);
    let has_dest = t.dest.is_some();
    let mv = if has_dest { Some(move_fields(st, target)) } else { None };
    let v = st.players.get_mut(&viewer).unwrap();
    if visible && v.seen_players.insert(tid) {
        v.send_raw(add);
        // Équipement porté, après le personnage auquel il se rattache.
        for g in gear {
            v.send_raw(g);
        }
        // Monture : le client la crée déjà en voyant l'objet sur un personnage marqué monté ; l'op 2 est sans risque.
        if let Some(r) = ride {
            v.send_raw(r);
        }
        // Guilde à laquelle il appartient.
        if let Some(l) = link {
            v.send_raw(l);
        }
        if let Some(mv) = mv {
            v.send(&schema::OPT_CHAR_MOVE, mv);
        }
    } else if !visible && v.seen_players.remove(&tid) {
        v.send_raw(character_remove_packet(tid));
    }
}

/// Fait courir le joueur jusqu'à `dest` sur ordre du serveur (à lui et aux joueurs qui le voient).
pub fn run_to(st: &mut WorldState, sid: u32, dest: Vec3, now: Instant) {
    let Some(p) = st.players.get_mut(&sid) else { return };
    advance(p, now);
    p.dest = Some(dest);
    p.move_flags = super::combat::MOVE_FAST;
    p.moved_at = now;
    p.dirty = true;
    broadcast_move(st, sid);
}

/// Arrête le joueur là où il est (à lui et aux joueurs qui le voient).
pub fn stop(st: &mut WorldState, sid: u32) {
    let Some(p) = st.players.get_mut(&sid) else { return };
    advance(p, Instant::now());
    p.dest = None;
    p.dirty = true;
    broadcast_move(st, sid);
}

fn broadcast_move(st: &WorldState, sid: u32) {
    let fields = move_fields(st, sid);
    let cid = st.players[&sid].ch.id;
    for (other, o) in &st.players {
        if *other == sid || o.seen_players.contains(&cid) {
            o.send(&schema::OPT_CHAR_MOVE, fields.clone());
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pas_de_marche() {
        let from = Vec3::new(0.0, 0.0, 0.0);
        let to = Vec3::new(3000.0, 10.0, 4000.0);
        // Course à 5000 (500 unités par seconde) : 2 s font 1000 unités sur 5000, hauteur inchangée en chemin.
        assert_eq!(step_toward(from, to, 5000.0, 2.0), (Vec3::new(600.0, 0.0, 800.0), false));
        assert_eq!(step_toward(from, to, 5000.0, 20.0), (to, true), "arrivé");
        assert_eq!(step_toward(to, to, 5000.0, 0.1), (to, true), "déjà là");
    }
}
