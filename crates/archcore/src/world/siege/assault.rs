//! Objets d'assaut des assiégeants : catapultes (type 10) et camps de résurrection (type 9), SiegeWarObject.txt.
//!
//! ```text
//!   C op 0x10 (ouvrir, champ 5 objet)      ← op 0x13 résultat 0x0E + fiche (champ 6) : fenêtre UI_SiegeWarCatapult ou
//!                                            UI_SiegeWarAttackRes
//!   C op 0x16 (charger, champ 7 IID)       ← op 0x18 (objet chargé), op 0x1A (fiche) ; op 0x17 (retirer) ← op 0x1A
//!   C op 0x12 (activer)                    objets requis consommés ; camp : résurrection des assaillants (op 0x1C codes
//!                                            12/13, op 0 champ 7) ; catapulte : tir sur une porte extérieure ou une tour
//!                                            de garde ; erreurs par op 2 : 0x13 déjà actif, 1 objets manquants
//!   C op 0x14 (réparer)                    or pris, PV rendus au bout de la durée (op 0x1C codes 16/17) ; erreurs 0x0D,
//!                                            0x0F, 0x14
//! ```
//! Seul un membre d'une guilde assaillante s'en sert, à moins de 1500 unités, pendant le combat. Choix d'ArchCore (le
//! client ne dit pas l'effet) : un tir de catapulte retire `Siege.CatapultDamagePercent` pour cent des PV maximaux de la
//! porte extérieure ou de la tour de garde debout la plus proche ; un camp activé le reste jusqu'à sa destruction.

use super::{assault_def, broadcast_castle, mb, result_packet, send_all, status, status_msg_packet, war, Side};
use super::kind;
use crate::world::{combat, WorldCtx, WorldState};
use crate::{info, warn};
use archcore_net::schema::{self, mini, siege_msg, siege_op, siege_result};
use archcore_net::{build_mini, build_packet, Fields};
use std::time::{Duration, Instant};

/// Distance à l'objet pour s'en servir.
const USE_RANGE: f64 = 1500.0;
/// Cases de chargement de la fenêtre (2 × 4).
const SLOTS: usize = 8;

/// Fiche d'un objet (mini SIEGE_OBJECT_INFO).
pub fn object_info(st: &WorldState, ctx: &WorldCtx, cid: u32, now: Instant) -> Vec<u8> {
    let o = st.siege.objects.get(&cid).cloned().unwrap_or_default();
    let def = assault_def(ctx, o.kind);
    let m = &mini::SIEGE_OBJECT_INFO;
    let mut f = Fields::new(m).with(0, o.repairs_left.clamp(0, 255)).with(1, def.repair_cost).with(2, def.repair_minutes.clamp(0, 255)).with(3, 0);
    for i in 0..SLOTS {
        f = f.with(4 + i, o.loaded.get(i).map(|(_, tid)| *tid as i64).unwrap_or(-1));
    }
    if let Some((from, _, who)) = &o.repair {
        f = f.with(12, now.saturating_duration_since(*from).as_secs().min(255)).with(13, mb(who));
    }
    build_mini(m, &f)
}

fn packet(op: i64, cid: u32, target: u32) -> Fields {
    Fields::new(&schema::SIEGE_WAR).with(0, op).with(3, cid).with(5, target)
}

/// Côté d'un camp parmi ceux de son château : 0 gauche (le plus à l'ouest), 1 droite.
fn camp_side(st: &WorldState, castle: u8, cid: u32) -> i64 {
    let x = |c: u32| st.monsters.get(c).map(|m| m.home.x).unwrap_or(0.0);
    let others = st.siege.objects_of(castle).filter(|(c, o)| o.kind == kind::CAMP && **c != cid).map(|(c, _)| x(*c));
    if others.into_iter().any(|ox| ox < x(cid)) { 1 } else { 0 }
}

/// Paquets AgpmSiegeWar des objets d'assaut (ops 0x10, 0x12, 0x14, 0x16, 0x17).
pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, op: i64, target: u32, iid: u32) -> anyhow::Result<()> {
    let now = Instant::now();
    let Some(o) = st.siege.objects.get(&target).filter(|o| matches!(o.kind, kind::CAMP | kind::CATAPULT)).cloned() else {
        warn!("[siège] #{sid} op {op} sur {target:#x}, qui n'est ni un camp ni une catapulte");
        return Ok(());
    };
    let Some(def) = ctx.sieges.get(o.castle).cloned() else { return Ok(()) };
    let Some(p) = st.players.get(&sid).filter(|p| p.in_game && !p.dead) else { return Ok(()) };
    let (cid, name, pos, money) = (p.ch.id, p.ch.name.clone(), p.ch.pos(), p.ch.money as i64);
    let c = &st.siege.castles[&o.castle];
    let side = st.guilds.of_cid.get(&cid).and_then(|g| c.side_of(*g));
    let reply = |st: &WorldState, r: i64| {
        if let Some(p) = st.players.get(&sid) {
            p.send_raw(result_packet(&def.name, r, cid));
        }
    };
    if !status::at_war(c.status) || c.status == status::ENDED || side != Some(Side::Attack) || o.destroyed {
        reply(st, siege_result::FAILED);
        return Ok(());
    }
    if st.monsters.get(target).is_none_or(|m| combat::dist2d(m.pos, pos) > USE_RANGE) {
        return Ok(());
    }
    let info = |st: &WorldState| build_packet(&schema::SIEGE_WAR, &packet(siege_op::OBJECT_INFO, cid, target).with(6, object_info(st, ctx, target, now)));
    let adef = assault_def(ctx, o.kind);
    match op {
        siege_op::OBJECT_OPEN => {
            let pkt = build_packet(&schema::SIEGE_WAR, &packet(siege_op::OBJECT_ANSWER, cid, target).with(1, siege_result::OPEN_OBJECT).with(6, object_info(st, ctx, target, now)));
            st.players[&sid].send_raw(pkt);
        }
        siege_op::OBJECT_LOAD => {
            let Some(item) = st.players[&sid].inventory.get(iid).cloned() else { return Ok(()) };
            let wanted = adef.items.iter().any(|(t, _)| *t == item.tid);
            if !wanted || o.loaded.len() >= SLOTS || o.loaded.iter().any(|(i, _)| *i == iid) {
                reply(st, siege_result::FAILED);
                return Ok(());
            }
            st.siege.objects.get_mut(&target).unwrap().loaded.push((iid, item.tid));
            st.players[&sid].send_raw(build_packet(&schema::SIEGE_WAR, &packet(siege_op::OBJECT_LOADED, cid, target).with(7, iid)));
            st.players[&sid].send_raw(info(st));
        }
        siege_op::OBJECT_UNLOAD => {
            st.siege.objects.get_mut(&target).unwrap().loaded.retain(|(i, _)| *i != iid);
            st.players[&sid].send_raw(info(st));
        }
        siege_op::OBJECT_ACTIVATE => {
            if o.kind == kind::CAMP && o.active {
                reply(st, siege_result::ALREADY_ACTIVE);
                return Ok(());
            }
            // Chaque objet requis doit être chargé et encore dans l'inventaire.
            let inv = &st.players[&sid].inventory;
            let ready = adef.items.iter().all(|(t, n)| o.loaded.iter().filter(|(i, lt)| lt == t && inv.get(*i).is_some()).map(|(i, _)| inv.get(*i).map(|x| x.count).unwrap_or(0)).sum::<i64>() >= *n);
            if !ready {
                reply(st, siege_result::FAILED);
                return Ok(());
            }
            for (t, n) in &adef.items {
                let mut left = *n;
                for (i, _) in o.loaded.iter().filter(|(_, lt)| lt == t) {
                    if left <= 0 {
                        break;
                    }
                    let p = st.players.get_mut(&sid).unwrap();
                    let have = p.inventory.get(*i).map(|x| x.count).unwrap_or(0).min(left);
                    match p.inventory.consume(&ctx.db, *i, have).await? {
                        Some(rest) => p.send_raw(crate::world::items::item_count(&rest, cid)),
                        None => p.send_raw(crate::world::items::item_remove(*i, Some(cid))),
                    }
                    left -= have;
                }
            }
            st.siege.objects.get_mut(&target).unwrap().loaded.clear();
            if o.kind == kind::CAMP {
                st.siege.objects.get_mut(&target).unwrap().active = true;
                let side = camp_side(st, o.castle, target);
                send_all(st, &status_msg_packet(siege_msg::CAMP_ACTIVE_LEFT + side, &def.name, "", &name, kind::CAMP, side));
                broadcast_castle(ctx, st, o.castle);
                info!("[siège] {name} active un camp de résurrection de {}", def.name);
            } else {
                fire(ctx, st, sid, target, now).await?;
            }
            st.players[&sid].send_raw(info(st));
        }
        siege_op::OBJECT_REPAIR => {
            let max = st.monsters.get(target).map(|m| (m.hp, m.max_hp)).unwrap_or((0, 0));
            let r = if o.repair.is_some() {
                siege_result::ALREADY_REPAIRING
            } else if o.repairs_left <= 0 {
                siege_result::NO_MORE_REPAIR
            } else if money < adef.repair_cost {
                siege_result::NOT_ENOUGH_GOLD
            } else if max.0 >= max.1 {
                siege_result::FAILED
            } else {
                siege_result::OK
            };
            if r != siege_result::OK {
                reply(st, r);
                return Ok(());
            }
            crate::world::set_money(ctx, st, sid, money - adef.repair_cost).await?;
            let minutes = Duration::from_secs(adef.repair_minutes.max(0) as u64 * 60);
            {
                let ob = st.siege.objects.get_mut(&target).unwrap();
                ob.repairs_left -= 1;
                ob.repair = Some((now, now + minutes, name.clone()));
            }
            let side = if o.kind == kind::CAMP { camp_side(st, o.castle, target) } else { 0 };
            send_all(st, &status_msg_packet(siege_msg::REPAIR_LEFT + side, &def.name, "", &name, o.kind, side));
            st.players[&sid].send_raw(info(st));
            info!("[siège] {name} répare un objet de {} ({} min)", def.name, minutes.as_secs() / 60);
        }
        _ => {}
    }
    Ok(())
}

/// Tir de catapulte : la porte extérieure ou la tour de garde debout la plus proche perd une part de ses PV.
async fn fire(ctx: &WorldCtx, st: &mut WorldState, sid: u32, catapult: u32, now: Instant) -> anyhow::Result<()> {
    let Some(o) = st.siege.objects.get(&catapult).cloned() else { return Ok(()) };
    let Some(from) = st.monsters.get(catapult).map(|m| m.pos) else { return Ok(()) };
    let target = st
        .siege
        .objects_of(o.castle)
        .filter(|(_, x)| matches!(x.kind, kind::OUTER_GATE | kind::GUARD_TOWER) && !x.destroyed)
        .filter_map(|(c, _)| st.monsters.get(*c).filter(|m| m.is_alive()).map(|m| (*c, combat::dist2d(m.pos, from))))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(c, _)| c);
    let Some(tcid) = target else { return Ok(()) };
    let pct = ctx.config.world.siege_catapult_damage_percent;
    let m = st.monsters.get_mut(tcid).unwrap();
    let damage = ((m.max_hp as f64 * pct / 100.0).round() as i32).max(1);
    m.hp = (m.hp - damage).max(0);
    let (hp, name) = (m.hp, m.name.clone());
    let watchers = combat::viewers(st, tcid);
    combat::broadcast(st, &watchers, &combat::action_packet(catapult, tcid, combat::result::SUCCESS, Some(combat::damage_factors(hp, 0, damage))));
    info!("[siège] tir de catapulte sur {name} : {damage} dégât(s), {hp} PV");
    if hp == 0 {
        combat::kill(ctx, st, sid, tcid, now).await?;
    }
    Ok(())
}

/// Réparations terminées : PV rendus, objet remontré.
pub fn step(st: &mut WorldState, castle: u8, now: Instant) {
    let done: Vec<u32> = st.siege.objects.iter().filter(|(_, o)| o.castle == castle && o.repair.as_ref().is_some_and(|r| now >= r.1)).map(|(c, _)| *c).collect();
    for cid in done {
        st.siege.objects.get_mut(&cid).unwrap().repair = None;
        if let Some(m) = st.monsters.get_mut(cid).filter(|m| m.is_alive()) {
            m.hp = m.max_hp;
        }
        war::refresh_view(st, cid);
    }
}
