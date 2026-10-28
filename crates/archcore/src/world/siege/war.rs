//! Combat de siège (statuts 3 à 6) : objets, ordre d'attaque, trône et Yeux d'Archon, carving, fin, et règles du monde
//! pendant le siège (combat entre joueurs, résurrection, portails).
//!
//! ```text
//!   début (statut 3)        objets et gardes du château montrés, intacts ; op 0, 0xE, 0xF ; op 0x1C code 1 ;
//!                           WarInfo étape 1 (op 4 : 0 et la durée en secondes), op 0x1D genre 2 chaque minute
//!   objet détruit           variante « en destruction » puis, 20 s plus tard, ruine (même CID) ; op 0x1C code 4 + type ;
//!                           WarInfo op 5 (objets détruits, total) ; porte extérieure → étape 2, intérieure → étape 3
//!   trône (type 6) détruit  statut 4, op 0x1C code 5, étape 4 (pas à Rensphere)
//!   Œil d'Archon détruit    Œil éveillé (ouvert) : op 0x1C code 6 + côté, étape 5 puis 6 ; seul ouvert plus
//!                           d'ArchonEyeOpenTime : refermé (code 7). Les deux ouverts au statut 4 → statut 5
//!   C op 0x1B (trône)       ← op 2 0x10, op 0x1C code 8, étape 7 ; bouger ou mourir : op 2 0x11, code 9 ;
//!                           StampingTime plus tard : op 2 0x12, code 10, carve guild (op 0), étape 8
//!   fin (durée écoulée)     statut 6 : carve guild propriétaire (sinon l'ancien le reste) ; code 11, étape 9 ; guildes
//!                           vaincues présentes renvoyées en ville ; objets cachés ; 30 s plus tard statut 0, WarInfo op 6
//! ```
//! Ordre d'attaque des objets (règle du client) : un assaillant frappe tours de garde (1) et portes extérieures (3) ;
//! portes intérieures (2) et tour de résurrection (5) une fois une porte extérieure tombée (sans porte extérieure : une
//! tour de garde tombée, ou aucune tour) ; Tour de vie (4), Yeux (7) et trône (6, jamais à Rensphere) une fois une porte
//! intérieure tombée (ou s'il n'y en a pas). Un défenseur ne frappe que les camps (9) et catapultes (10). Les gardes
//! (`siege_war` 2) se battent comme des monstres ordinaires, que les défenseurs ne frappent pas. Tours de garde et gardes
//! n'attaquent d'eux-mêmes que les assaillants, et ne visent jamais un défenseur ([`may_target`]).
//!
//! Choix d'ArchCore : un Œil d'Archon « s'active » réduit à 0 PV ; un objet détruit ne rapporte ni expérience ni butin ;
//! pendant le siège (statuts 3 à 5, 7, 8), dans les régions du château, les guildes des deux camps se battent sans Ctrl,
//! un même camp (ou des guildes alliées) jamais, et un meurtre ne donne ni points de meurtrier ni avis de recherche ;
//! résurrection de siège (AgpmCharacter op 0x13 intérieur, 0x14 extérieur) aux points « binding » de son camp ;
//! portails `SpecialType` 2 (trajets du château) réservés à la guilde propriétaire hors siège (libres sans propriétaire),
//! aux guildes du siège pendant le siège ; 3 (terrain de chasse du seigneur) à la guilde propriétaire ; 4 (entrée de
//! Rensphere) aux guildes propriétaires d'un château pendant la guerre de Rensphere.

use super::{
    broadcast_castle, broadcast_lists, concerned, duration, mastered_guild_of, region_of, revive_flags, save, send_all, send_to, status, status_msg_packet,
    time_packet, variants, Carving, Side, War, NEVER, RENSPHERE,
};
use super::{kind, Castle};
use crate::world::monsters::{Life, MonsterRegistry};
use crate::world::{combat, sector, send_system, WorldCtx, WorldState};
use crate::{info, warn};
use archcore_net::schema::{self, siege_msg, siege_result, war_info_op};
use archcore_net::{build_packet, Fields, Vec3};
use std::time::{Duration, Instant};

/// Opérations AGPMCHARACTER du client : résurrection de siège au point intérieur (0x13) ou extérieur (0x14).
pub const RESURRECT_INNER: i64 = 0x13;
pub const RESURRECT_OUTER: i64 = 0x14;
/// Délai entre la fin du combat (statut 6) et le retour au statut 0.
pub const CLOSE_DELAY: Duration = Duration::from_secs(30);
/// Carving sans durée connue.
const DEFAULT_STAMPING: Duration = Duration::from_secs(180);
/// Le graveur ne doit pas s'éloigner de plus de cette distance du point où il a commencé.
const CARVE_STILL: f64 = 100.0;
/// Distance au trône pour graver.
const CARVE_RANGE: f64 = 1500.0;
/// Cadence du compte à rebours (op 0x1D) et du compteur de WarInfo.
const TIME_EVERY: Duration = Duration::from_secs(60);

/// Paquet AGPMCHARACTER op 2 : suppression immédiate de la vue (avant de remontrer l'objet sous un autre modèle).
fn delete_packet(cid: u32) -> Vec<u8> {
    let c = &schema::CHARACTER;
    build_packet(c, &Fields::new(c).with(0, 2).with(1, cid))
}

/// Montre l'objet tel qu'il est à ceux qui voient son secteur (le retire d'abord s'ils le voyaient).
pub fn refresh_view(st: &mut WorldState, cid: u32) {
    let Some(m) = st.monsters.get(cid) else { return };
    let (pos, visible, view) = (m.pos, m.is_visible(), MonsterRegistry::view_packet(&st.monsters, m));
    let gone = delete_packet(cid);
    for p in st.players.values_mut().filter(|p| p.in_game) {
        if p.seen_monsters.remove(&cid) {
            p.send_raw(gone.clone());
        }
        if visible && sector::sees(p.view_pos, pos) {
            p.seen_monsters.insert(cid);
            p.send_raw(view.clone());
        }
    }
}

/// Montre (début du siège) ou cache (fin) les objets et gardes d'un château, remis à neuf.
fn show_objects(ctx: &WorldCtx, st: &mut WorldState, id: u8, now: Instant, on: bool) {
    let cids: Vec<u32> = st.siege.objects.iter().filter(|(_, o)| o.castle == id).map(|(c, _)| *c).collect();
    for cid in cids {
        let o = st.siege.objects.get_mut(&cid).unwrap();
        o.destroyed = false;
        o.ruin_at = None;
        o.loaded.clear();
        o.active = false;
        o.repair = None;
        o.repairs_left = ctx.sieges.assault.get(&o.kind).map(|a| a.repair_count).unwrap_or(0);
        let base = o.base_tid;
        let Some(m) = st.monsters.get_mut(cid) else { continue };
        m.tid = base;
        m.hp = m.max_hp;
        m.aggro.clear();
        m.target = None;
        m.life = if on { Life::Alive } else { Life::Gone { respawn_at: now + NEVER } };
        let home = m.home;
        st.monsters.relocate(cid, home);
        for set in [&mut st.monsters.pending, &mut st.monsters.engaged] {
            set.remove(&cid);
        }
        st.monsters.walking.remove(&cid);
        st.monsters.effects.remove(&cid);
        st.monsters.skill_state.remove(&cid);
        st.monsters.casts.remove(&cid);
        refresh_view(st, cid);
    }
}

/// Objets d'un type (pas les gardes) d'un château : (CID, détruit).
fn objects(st: &WorldState, id: u8, k: u8) -> Vec<(u32, bool)> {
    st.siege.objects_of(id).filter(|(_, o)| o.kind == k).map(|(c, o)| (*c, o.destroyed)).collect()
}

/// Un objet de ce type est tombé, ou il n'y en a aucun.
fn fallen_or_none(st: &WorldState, id: u8, k: u8) -> (bool, bool) {
    let v = objects(st, id, k);
    (v.iter().any(|(_, d)| *d), v.is_empty())
}

/// Le joueur peut frapper ce monstre : toujours hors des sièges ; un objet ou un garde de siège seulement pendant le
/// combat, selon son camp et l'ordre d'attaque.
pub fn may_hit(_ctx: &WorldCtx, st: &WorldState, sid: u32, cid: u32) -> bool {
    let Some(o) = st.siege.objects.get(&cid) else { return true };
    let Some(c) = st.siege.castles.get(&o.castle) else { return false };
    let side = st.players.get(&sid).and_then(|p| st.guilds.of_cid.get(&p.ch.id)).and_then(|g| c.side_of(*g));
    allows(st, side, cid)
}

/// Ordre d'attaque des objets pour un joueur du camp `side` (`None` : hors du siège).
pub fn allows(st: &WorldState, side: Option<Side>, cid: u32) -> bool {
    let Some(o) = st.siege.objects.get(&cid) else { return true };
    if !st.siege.castles.get(&o.castle).is_some_and(|c| status::at_war(c.status) && c.status != status::ENDED) {
        return false;
    }
    if o.guard {
        return side != Some(Side::Defense);
    }
    // Objet détruit (ruine, Œil éveillé) : plus de coups.
    if o.destroyed || st.monsters.get(cid).is_some_and(|m| m.tid != o.base_tid) {
        return false;
    }
    match side {
        Some(Side::Defense) => matches!(o.kind, kind::CAMP | kind::CATAPULT),
        Some(Side::Attack) => {
            let id = o.castle;
            let (outer_down, no_outer) = fallen_or_none(st, id, kind::OUTER_GATE);
            let (tower_down, no_tower) = fallen_or_none(st, id, kind::GUARD_TOWER);
            let (inner_down, no_inner) = fallen_or_none(st, id, kind::INNER_GATE);
            let past_outer = outer_down || (no_outer && (tower_down || no_tower));
            match o.kind {
                kind::GUARD_TOWER | kind::OUTER_GATE => true,
                kind::INNER_GATE | kind::REVIVAL_TOWER => past_outer,
                kind::LIFE_TOWER | kind::EYE => inner_down || no_inner,
                kind::THRONE => id != RENSPHERE && (inner_down || no_inner),
                _ => false,
            }
        }
        None => false,
    }
}

/// Étape de WarInfo (avec ses arguments « %s ») aux joueurs concernés.
fn set_step(ctx: &WorldCtx, st: &mut WorldState, id: u8, step: u8, args: Option<[&str; 3]>) {
    let Some(w) = st.siege.castles.get_mut(&id).and_then(|c| c.war.as_mut()) else { return };
    if step < w.step && step < 7 {
        return;
    }
    w.step = step;
    let who = concerned(ctx, st, id);
    if let Some(a) = args {
        send_to(st, &who, &crate::world::warinfo::args(war_info_op::SIEGE_WAR, a));
    }
    send_to(st, &who, &crate::world::warinfo::step(war_info_op::SIEGE_WAR, step as i64, id as i64));
}

fn names(st: &WorldState, gid: Option<u32>) -> (String, String) {
    (super::guild_name(st, gid), super::master_name(st, gid))
}

/// Début du combat (statut 3).
pub async fn start(ctx: &WorldCtx, st: &mut WorldState, id: u8, now: Instant) -> anyhow::Result<()> {
    let Some(def) = ctx.sieges.get(id).cloned() else { return Ok(()) };
    let total = duration(ctx, &def);
    // Guerre de Rensphere : les guildes propriétaires d'un autre château en sont les assaillants (archlord.rs).
    if id == RENSPHERE {
        crate::world::archlord::on_war_start(ctx, st, now);
    }
    {
        let c = st.siege.castles.entry(id).or_insert_with(|| Castle::new(id));
        c.status = status::WAR;
        c.carve = None;
        c.close_at = None;
        c.war = Some(War { ends_at: now + total, step: 0, eyes: Vec::new(), carving: None, next_time: now + TIME_EVERY });
    }
    show_objects(ctx, st, id, now, true);
    save(ctx, st, id).await?;
    broadcast_castle(ctx, st, id);
    broadcast_lists(ctx, st, id);
    send_all(st, &status_msg_packet(siege_msg::START, &def.name, "", "", 0, 0));
    set_step(ctx, st, id, 1, None);
    let who = concerned(ctx, st, id);
    send_to(st, &who, &crate::world::warinfo::values(war_info_op::VALUES_4, war_info_op::SIEGE_WAR, 0, total.as_secs() as i64));
    send_to(st, &who, &time_packet(2, total.as_secs()));
    let c = &st.siege.castles[&id];
    info!("[siège] {} : siège commencé pour {} min, {} assaillant(s), {} défenseur(s)", def.name, total.as_secs() / 60, c.selected(Side::Attack).len(), c.defenders().len());
    Ok(())
}

/// Un tour du combat : fin, ruines, Yeux, carving, réparations, compte à rebours.
pub async fn advance(ctx: &WorldCtx, st: &mut WorldState, id: u8, now: Instant) -> anyhow::Result<()> {
    let Some(c) = st.siege.castles.get(&id).cloned() else { return Ok(()) };
    if c.status == status::ENDED {
        if c.close_at.is_some_and(|t| now >= t) {
            close(ctx, st, id).await?;
        }
        return Ok(());
    }
    let Some(w) = c.war.clone() else { return Ok(()) };
    if now >= w.ends_at {
        return finish(ctx, st, id, now).await;
    }
    // Variantes « en destruction » remplacées par la ruine.
    let ruins: Vec<u32> = st.siege.objects.iter().filter(|(_, o)| o.castle == id && o.ruin_at.is_some_and(|t| now >= t)).map(|(c, _)| *c).collect();
    for cid in ruins {
        let o = st.siege.objects.get_mut(&cid).unwrap();
        o.ruin_at = None;
        let Some((_, ruin)) = variants(o.base_tid) else { continue };
        if let Some(m) = st.monsters.get_mut(cid) {
            m.tid = ruin;
        }
        refresh_view(st, cid);
    }
    eyes_step(ctx, st, id, now);
    carving_step(ctx, st, id, now);
    super::assault::step(st, id, now);
    if now >= w.next_time {
        if let Some(w) = st.siege.castles.get_mut(&id).and_then(|c| c.war.as_mut()) {
            w.next_time = now + TIME_EVERY;
        }
        let left = w.ends_at.saturating_duration_since(now).as_secs();
        let who = concerned(ctx, st, id);
        send_to(st, &who, &time_packet(2, left));
        send_to(st, &who, &crate::world::warinfo::counter(war_info_op::SIEGE_WAR, left.div_ceil(60) as i64));
    }
    Ok(())
}

/// Côté d'un Œil (0 ouest, 1 est), par rapport au trône (à défaut, à l'autre Œil).
fn eye_side(st: &WorldState, id: u8, cid: u32) -> i64 {
    let x = |c: u32| st.monsters.get(c).map(|m| m.home.x).unwrap_or(0.0);
    let reference = objects(st, id, kind::THRONE).first().map(|(c, _)| x(*c));
    let reference = reference.unwrap_or_else(|| {
        let eyes = objects(st, id, kind::EYE);
        eyes.iter().map(|(c, _)| x(*c)).sum::<f32>() / eyes.len().max(1) as f32
    });
    if x(cid) < reference { 0 } else { 1 }
}

/// Mort d'un monstre : rend vrai si c'était un objet de siège (pris en charge ici : ni expérience ni butin).
pub fn on_kill(ctx: &WorldCtx, st: &mut WorldState, killer_sid: u32, cid: u32, now: Instant) -> bool {
    let Some(o) = st.siege.objects.get(&cid).cloned().filter(|o| !o.guard) else { return false };
    let Some(def) = ctx.sieges.get(o.castle).cloned() else { return false };
    let id = o.castle;
    let killer = st.players.get(&killer_sid).map(|p| p.ch.name.clone()).unwrap_or_default();
    let killer_guild = st.players.get(&killer_sid).and_then(|p| st.guilds.of_cid.get(&p.ch.id)).copied();
    {
        let ob = st.siege.objects.get_mut(&cid).unwrap();
        ob.destroyed = true;
        ob.repair = None;
        if o.kind != kind::EYE && variants(o.base_tid).is_some_and(|(a, b)| a != b) {
            ob.ruin_at = Some(now + combat::CORPSE_TIME);
        }
    }
    if let Some(m) = st.monsters.get_mut(cid) {
        if let Some((destroying, _)) = variants(o.base_tid) {
            m.tid = destroying;
        }
        // L'Œil éveillé reste debout ; le reste demeure au sol jusqu'à la fin du siège.
        m.life = if o.kind == kind::EYE { Life::Alive } else { Life::Dead { remove_at: now + NEVER } };
        if o.kind == kind::EYE {
            m.hp = m.max_hp;
        }
    }
    refresh_view(st, cid);
    let total = st.siege.objects_of(id).filter(|(_, x)| x.kind != kind::PROTECTOR).count();
    let destroyed = st.siege.objects_of(id).filter(|(_, x)| x.destroyed && x.kind != kind::EYE).count();
    let (gname, _) = names(st, killer_guild);
    info!("[siège] {} : {} (type {}) détruit par {killer}", def.name, st.monsters.get(cid).map(|m| m.name.clone()).unwrap_or_default(), o.kind);
    let who = concerned(ctx, st, id);
    send_to(st, &who, &crate::world::warinfo::values(war_info_op::VALUES_5, war_info_op::SIEGE_WAR, destroyed as i64, total as i64));
    match o.kind {
        kind::EYE => {
            let side = eye_side(st, id, cid);
            let open = {
                let w = st.siege.castles.get_mut(&id).and_then(|c| c.war.as_mut());
                match w {
                    Some(w) => {
                        w.eyes.push((cid, now));
                        w.eyes.len()
                    }
                    None => 0,
                }
            };
            send_all(st, &status_msg_packet(siege_msg::EYE_OPENED, &def.name, &gname, &killer, kind::EYE, side));
            set_step(ctx, st, id, if open >= 2 { 6 } else { 5 }, None);
            check_eyes(ctx, st, id);
        }
        kind::THRONE if id != RENSPHERE => {
            send_all(st, &status_msg_packet(siege_msg::OBJECT_DESTROYED, &def.name, &gname, &killer, o.kind, 0));
            let c = st.siege.castles.get_mut(&id).unwrap();
            if c.status == status::WAR {
                c.status = status::THRONE_RELEASED;
                send_all(st, &status_msg_packet(siege_msg::THRONE_RELEASED, &def.name, &gname, &killer, kind::THRONE, 0));
                set_step(ctx, st, id, 4, None);
                broadcast_castle(ctx, st, id);
                check_eyes(ctx, st, id);
            }
        }
        k => {
            send_all(st, &status_msg_packet(siege_msg::OBJECT_DESTROYED, &def.name, &gname, &killer, k, 0));
            match k {
                kind::OUTER_GATE => set_step(ctx, st, id, 2, None),
                kind::INNER_GATE => set_step(ctx, st, id, 3, None),
                kind::REVIVAL_TOWER | kind::CAMP => broadcast_castle(ctx, st, id),
                _ => {}
            }
        }
    }
    true
}

/// Les deux Yeux ouverts après la levée du sceau : statut 5, carving permis.
fn check_eyes(ctx: &WorldCtx, st: &mut WorldState, id: u8) {
    let total = objects(st, id, kind::EYE).len();
    let Some(c) = st.siege.castles.get_mut(&id) else { return };
    let open = c.war.as_ref().map(|w| w.eyes.len()).unwrap_or(0);
    if c.status == status::THRONE_RELEASED && total > 0 && open >= total {
        c.status = status::EYES_OPEN;
        info!("[siège] château {id} : Yeux d'Archon ouverts, carving permis");
        broadcast_castle(ctx, st, id);
    }
}

/// Un Œil ouvert seul plus d'ArchonEyeOpenTime se referme (code 7) et redevient un Œil intact.
fn eyes_step(ctx: &WorldCtx, st: &mut WorldState, id: u8, now: Instant) {
    let Some(def) = ctx.sieges.get(id) else { return };
    let total = objects(st, id, kind::EYE).len();
    let Some(c) = st.siege.castles.get(&id) else { return };
    let Some(w) = c.war.as_ref() else { return };
    if def.eye_open.is_zero() || c.status >= status::EYES_OPEN || w.eyes.is_empty() || w.eyes.len() >= total {
        return;
    }
    let expired: Vec<u32> = w.eyes.iter().filter(|(_, at)| now >= *at + def.eye_open).map(|(c, _)| *c).collect();
    for cid in expired {
        let side = eye_side(st, id, cid);
        if let Some(w) = st.siege.castles.get_mut(&id).and_then(|c| c.war.as_mut()) {
            w.eyes.retain(|(c, _)| *c != cid);
        }
        let base = st.siege.objects.get(&cid).map(|o| o.base_tid).unwrap_or(0);
        if let Some(o) = st.siege.objects.get_mut(&cid) {
            o.destroyed = false;
        }
        if let Some(m) = st.monsters.get_mut(cid) {
            m.tid = base;
            m.hp = m.max_hp;
            m.life = Life::Alive;
        }
        refresh_view(st, cid);
        send_all(st, &status_msg_packet(siege_msg::EYE_CLOSED, &def.name, "", "", kind::EYE, side));
        info!("[siège] {} : un Œil d'Archon s'est refermé", def.name);
    }
}

/// Op 0x1B : carving du trône par le chef d'une guilde du siège.
pub fn carve(ctx: &WorldCtx, st: &mut WorldState, sid: u32, throne: u32, now: Instant) {
    let Some(p) = st.players.get(&sid).filter(|p| p.in_game) else { return };
    let (cid, pos, dead, name) = (p.ch.id, p.ch.pos(), p.dead, p.ch.name.clone());
    let Some(o) = st.siege.objects.get(&throne).filter(|o| o.kind == kind::THRONE).cloned() else {
        warn!("[siège] {name} : carving sur {throne:#x}, qui n'est pas un trône");
        return;
    };
    let Some(def) = ctx.sieges.get(o.castle).cloned() else { return };
    let id = def.id;
    let refuse = |st: &WorldState, why: &str| {
        info!("[siège] {name} : carving refusé à {} ({why})", def.name);
        if let Some(p) = st.players.get(&sid) {
            p.send_raw(super::result_packet(&def.name, siege_result::FAILED, cid));
        }
    };
    let Some(c) = st.siege.castles.get(&id) else { return };
    let ready = if id == RENSPHERE { c.status == status::WAR && c.carve.is_none() } else { c.status == status::EYES_OPEN && o.destroyed };
    if !ready || dead {
        return refuse(st, "trône pas prêt");
    }
    let Some(gid) = mastered_guild_of(st, cid) else { return refuse(st, "pas chef de guilde") };
    let Some(side) = c.side_of(gid) else { return refuse(st, "guilde hors du siège") };
    if c.war.as_ref().is_some_and(|w| w.carving.is_some()) {
        return refuse(st, "carving déjà en cours");
    }
    let allowed = match c.carve {
        None => c.owner != Some(gid) && side == Side::Attack,
        Some(g) => g != gid,
    };
    if !allowed {
        return refuse(st, "camp ou guilde");
    }
    if st.monsters.get(throne).is_none_or(|m| combat::dist2d(m.pos, pos) > CARVE_RANGE) {
        return refuse(st, "trop loin du trône");
    }
    let stamping = match ctx.config.world.siege_carve_seconds {
        s if s > 0 => Duration::from_secs(s as u64),
        _ if !def.stamping.is_zero() => def.stamping,
        _ => DEFAULT_STAMPING,
    };
    if let Some(w) = st.siege.castles.get_mut(&id).and_then(|c| c.war.as_mut()) {
        w.carving = Some(Carving { cid, guild: gid, pos, until: now + stamping });
    }
    let (gname, master) = names(st, Some(gid));
    if let Some(p) = st.players.get(&sid) {
        p.send_raw(super::result_packet(&def.name, siege_result::CARVE_START, cid));
    }
    send_all(st, &status_msg_packet(siege_msg::CARVE_START, &def.name, &gname, &master, kind::THRONE, 0));
    set_step(ctx, st, id, 7, None);
    info!("[siège] {name} ({gname}) commence le carving du trône de {} ({} s)", def.name, stamping.as_secs());
}

/// Carving en cours : annulé si le graveur meurt, part ou bouge ; réussi au bout de sa durée.
fn carving_step(ctx: &WorldCtx, st: &mut WorldState, id: u8, now: Instant) {
    let Some(def) = ctx.sieges.get(id) else { return };
    let Some(k) = st.siege.castles.get(&id).and_then(|c| c.war.as_ref()).and_then(|w| w.carving.clone()) else { return };
    let sid = st.by_cid.get(&k.cid).copied();
    let still = sid.and_then(|s| st.players.get(&s)).is_some_and(|p| p.in_game && !p.dead && p.dest.is_none() && combat::dist2d(p.ch.pos(), k.pos) <= CARVE_STILL);
    let (gname, master) = names(st, Some(k.guild));
    if !still {
        if let Some(w) = st.siege.castles.get_mut(&id).and_then(|c| c.war.as_mut()) {
            w.carving = None;
        }
        if let Some(p) = sid.and_then(|s| st.players.get(&s)) {
            p.send_raw(super::result_packet(&def.name, siege_result::CARVE_CANCEL, k.cid));
        }
        send_all(st, &status_msg_packet(siege_msg::CARVE_CANCEL, &def.name, &gname, &master, kind::THRONE, 0));
        info!("[siège] {} : carving de {gname} annulé", def.name);
        return;
    }
    if now < k.until {
        return;
    }
    if let Some(c) = st.siege.castles.get_mut(&id) {
        c.carve = Some(k.guild);
        if let Some(w) = c.war.as_mut() {
            w.carving = None;
        }
    }
    if let Some(p) = sid.and_then(|s| st.players.get(&s)) {
        p.send_raw(super::result_packet(&def.name, siege_result::CARVE_DONE, k.cid));
    }
    send_all(st, &status_msg_packet(siege_msg::CARVE_DONE, &def.name, &gname, &master, kind::THRONE, 0));
    broadcast_castle(ctx, st, id);
    set_step(ctx, st, id, 8, Some([&gname, &master, &def.name]));
    info!("[siège] {} : la guilde {gname} a gravé le sceau du trône", def.name);
}

/// Fin du combat (statut 6) : la guilde du dernier carving prend le château, sinon le propriétaire le garde.
pub async fn finish(ctx: &WorldCtx, st: &mut WorldState, id: u8, now: Instant) -> anyhow::Result<()> {
    let Some(def) = ctx.sieges.get(id).cloned() else { return Ok(()) };
    let who = concerned(ctx, st, id);
    let (before, after, winners, participants) = {
        let c = st.siege.castles.get_mut(&id).unwrap();
        let before = c.owner;
        let after = c.carve.or(before);
        // Vainqueurs : la nouvelle guilde propriétaire ; si le château n'a pas changé de mains, tous les défenseurs.
        let winners = if after != before { after.into_iter().collect::<Vec<u32>>() } else { c.defenders() };
        let participants = c.participants();
        c.owner = after;
        c.status = status::ENDED;
        c.close_at = Some(now + CLOSE_DELAY);
        c.last_siege = if c.next_siege > 0 { c.next_siege } else { crate::character::unix_now() };
        c.next_siege = 0;
        c.apply_until = 0;
        if let Some(w) = c.war.as_mut() {
            w.carving = None;
        }
        (before, after, winners, participants)
    };
    let (gname, master) = names(st, after);
    send_all(st, &status_msg_packet(siege_msg::END, &def.name, &gname, &master, 0, 0));
    send_to(st, &who, &crate::world::warinfo::args(war_info_op::SIEGE_WAR, [&gname, &master, &def.name]));
    send_to(st, &who, &crate::world::warinfo::step(war_info_op::SIEGE_WAR, 9, id as i64));
    if let Some(w) = st.siege.castles.get_mut(&id).and_then(|c| c.war.as_mut()) {
        w.step = 9;
    }
    // Guildes vaincues présentes dans les régions du château : renvoyées en ville.
    let losers: Vec<u32> = st
        .sids()
        .into_iter()
        .filter(|sid| {
            let p = &st.players[sid];
            let g = st.guilds.of_cid.get(&p.ch.id).copied();
            p.in_game && g.is_some_and(|g| participants.contains(&g) && !winners.contains(&g)) && region_of(ctx, st, *sid).is_some_and(|r| def.has_region(r))
        })
        .collect();
    for sid in losers {
        if st.players.get(&sid).is_some_and(|p| p.dead) {
            combat::resurrect(ctx, st, sid).await?;
            continue;
        }
        let (x, y, z) = crate::world::travel::home_village(ctx, &st.players[&sid]);
        crate::world::teleport(ctx, st, sid, x, y, z).await?;
    }
    show_objects(ctx, st, id, now, false);
    save(ctx, st, id).await?;
    if after != before {
        super::on_owner_change(ctx, st, id, before, after);
    }
    broadcast_castle(ctx, st, id);
    info!("[siège] {} : siège terminé, propriétaire {}", def.name, if gname.is_empty() { "aucun" } else { &gname });
    Ok(())
}

/// Retour au statut 0 : candidatures oubliées, fenêtre de déclaration du propriétaire ouverte, tableau effacé.
async fn close(ctx: &WorldCtx, st: &mut WorldState, id: u8) -> anyhow::Result<()> {
    let Some(def) = ctx.sieges.get(id) else { return Ok(()) };
    let who = concerned(ctx, st, id);
    {
        let c = st.siege.castles.get_mut(&id).unwrap();
        c.status = status::NONE;
        c.war = None;
        c.close_at = None;
        c.applications.clear();
        c.declare_until = crate::character::unix_now() + def.declare_hours * 3600;
    }
    send_to(st, &who, &crate::world::warinfo::clear(war_info_op::SIEGE_WAR));
    save(ctx, st, id).await?;
    broadcast_castle(ctx, st, id);
    broadcast_lists(ctx, st, id);
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Règles du monde pendant le siège
// ---------------------------------------------------------------------------------------------

/// Château en combat (statuts 3 à 5, 7, 8) dont le joueur occupe une région.
pub fn war_zone(ctx: &WorldCtx, st: &WorldState, sid: u32) -> Option<u8> {
    let def = ctx.sieges.by_region(region_of(ctx, st, sid)?)?;
    st.siege.castles.get(&def.id).filter(|c| status::at_war(c.status) && c.status != status::ENDED).map(|_| def.id)
}

/// Règle de combat entre deux joueurs dans la même zone de siège : `Some(true)` permis sans Ctrl (camps opposés),
/// `Some(false)` refusé (même camp, alliés, ou Rensphere gravée), `None` : règles ordinaires.
pub fn pvp_rule(ctx: &WorldCtx, st: &WorldState, sid: u32, tsid: u32) -> Option<bool> {
    let id = war_zone(ctx, st, sid)?;
    if war_zone(ctx, st, tsid) != Some(id) {
        return None;
    }
    let c = &st.siege.castles[&id];
    // Rensphere gravée : combat suspendu jusqu'à la fin.
    if id == RENSPHERE && c.status == status::WAR && c.carve.is_some() {
        return Some(false);
    }
    let guild = |s: u32| st.players.get(&s).and_then(|p| st.guilds.of_cid.get(&p.ch.id)).copied();
    let (ga, gb) = (guild(sid)?, guild(tsid)?);
    match (c.side_of(ga), c.side_of(gb)) {
        (Some(a), Some(b)) => Some(a != b && ga != gb && !crate::world::guild::relation::allied(st, ga, gb)),
        _ => None,
    }
}

/// AgpmCharacter op 0x13 (intérieur) ou 0x14 (extérieur) : résurrection au point de son camp.
pub async fn resurrect(ctx: &WorldCtx, st: &mut WorldState, sid: u32, inner: bool) -> anyhow::Result<()> {
    let Some(p) = st.players.get(&sid).filter(|p| p.dead) else { return Ok(()) };
    let (cid, pos, name) = (p.ch.id, p.ch.pos(), p.ch.name.clone());
    let Some(id) = war_zone(ctx, st, sid) else {
        send_system(st, sid, "Aucun siège en cours ici.");
        return Ok(());
    };
    let def = ctx.sieges.get(id).cloned().unwrap();
    let side = st.guilds.of_cid.get(&cid).and_then(|g| st.siege.castles[&id].side_of(*g));
    let (attack_ok, defense_ok) = revive_flags(st, id);
    let kinds: &[u8] = match side {
        Some(Side::Attack) if attack_ok => {
            if id == RENSPHERE { &[3, 6] } else { &[3] }
        }
        Some(Side::Defense) if defense_ok => {
            if id == RENSPHERE { &[4, 5] } else if inner { &[4] } else { &[7] }
        }
        _ => &[],
    };
    let d = |b: &Vec3| ((b.x - pos.x) as f64).powi(2) + ((b.z - pos.z) as f64).powi(2);
    let at = ctx.sieges.bindings.iter().filter(|b| def.has_region(b.region) && kinds.contains(&b.kind)).map(|b| b.pos).min_by(|a, b| d(a).total_cmp(&d(b)));
    let Some(at) = at else {
        info!("[siège] {name} : résurrection de siège impossible ({side:?}, assaillants {attack_ok}, défenseurs {defense_ok})");
        send_system(st, sid, "Aucun point de résurrection de votre camp n'est disponible.");
        return Ok(());
    };
    info!("[siège] {name} ressuscite au point de son camp ({})", if inner { "intérieur" } else { "extérieur" });
    combat::resurrect_at(ctx, st, sid, Some(at)).await
}

/// Portails des châteaux (`SpecialType` 2, 3 et 4) : `Err` donne le message de refus.
pub fn portal_check(ctx: &WorldCtx, st: &WorldState, sid: u32, from: (i64, Vec3), to: (i64, Option<Vec3>)) -> Result<(), String> {
    let special = [from.0, to.0].into_iter().find(|s| matches!(s, 2..=4)).unwrap_or(0);
    if special == 0 {
        return Ok(());
    }
    let castle_at = |at: Vec3| ctx.map.region(at.x as f64, at.z as f64).and_then(|r| ctx.sieges.by_region(r));
    let Some(def) = castle_at(from.1).or_else(|| to.1.and_then(castle_at)) else { return Ok(()) };
    let Some(c) = st.siege.castles.get(&def.id) else { return Ok(()) };
    let gid = st.players.get(&sid).and_then(|p| st.guilds.of_cid.get(&p.ch.id)).copied();
    let owner = gid.is_some() && c.owner == gid;
    let fighting = status::at_war(c.status) && c.status != status::ENDED;
    match special {
        3 if owner => Ok(()),
        3 => Err("Ce passage est réservé à la guilde qui tient le château.".into()),
        2 if fighting => match gid.and_then(|g| c.side_of(g)) {
            Some(_) => Ok(()),
            None => Err("Pendant le siège, ces passages sont réservés aux guildes du siège.".into()),
        },
        2 if owner || c.owner.is_none() => Ok(()),
        2 => Err("Ce passage est réservé à la guilde qui tient le château.".into()),
        _ => {
            let rensphere = st.siege.castles.get(&RENSPHERE).is_some_and(|r| status::at_war(r.status) && r.status != status::ENDED);
            let lord = gid.is_some_and(|g| st.siege.castles.values().any(|x| x.id != RENSPHERE && x.owner == Some(g)));
            if rensphere && lord { Ok(()) } else { Err("L'entrée de Rensphere n'est ouverte qu'aux guildes propriétaires d'un château, pendant la guerre de Rensphere.".into()) }
        }
    }
}

/// Un objet ou un garde de siège peut-il prendre pour cible le joueur `pcid` ? Jamais un défenseur de son château ; de
/// lui-même (attaque préventive), seulement un membre d'une guilde assaillante, pendant le combat ; frappé
/// (`provoked`), tout autre agresseur. Toujours pour un monstre ordinaire.
pub fn may_target(st: &WorldState, cid: u32, pcid: u32, provoked: bool) -> bool {
    let Some(o) = st.siege.objects.get(&cid) else { return true };
    let Some(c) = st.siege.castles.get(&o.castle).filter(|c| status::at_war(c.status) && c.status != status::ENDED) else { return false };
    let side = st.guilds.of_cid.get(&pcid).and_then(|g| c.side_of(*g));
    if provoked { side != Some(Side::Defense) } else { side == Some(Side::Attack) }
}

/// Cible choisie d'office (pièges des invocations) : jamais un objet de siège, les gardes oui.
pub fn may_hit_auto(st: &WorldState, cid: u32) -> bool {
    !st.siege.objects.get(&cid).is_some_and(|o| !o.guard)
}

/// `/siege hp <château> <PV>` (essais) : PV des objets intacts du château en guerre ramenés à `hp` au plus.
pub fn set_hp(ctx: &WorldCtx, st: &mut WorldState, castle: &str, hp: i32) -> Result<String, String> {
    let Some(def) = ctx.sieges.named(castle).cloned() else { return Err(format!("château inconnu : {castle}")) };
    if !st.siege.castles.get(&def.id).is_some_and(|c| status::at_war(c.status) && c.status != status::ENDED) {
        return Err(format!("{} n'est pas en guerre", def.name));
    }
    let cids: Vec<u32> = st.siege.objects_of(def.id).filter(|(_, o)| !o.destroyed).map(|(c, _)| *c).collect();
    for cid in &cids {
        if let Some(m) = st.monsters.get_mut(*cid).filter(|m| m.is_alive()) {
            m.hp = m.hp.min(hp.max(1));
        }
        refresh_view(st, *cid);
    }
    Ok(format!("{} : {} objet(s) à {} PV au plus.", def.name, cids.len(), hp.max(1)))
}
