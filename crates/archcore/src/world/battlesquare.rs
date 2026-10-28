//! Battle Square et Battleground : les zones de combat entre joueurs du jeu d'origine.
//!
//! **Battle Square** (régions 189 à 234, six étages ; régions racines 198, 208, 209, 215, 223, 229) : zone de combat
//! (sûreté 2 de RegionTemplate.ini, déjà lue par `regions.rs`), sans restriction de race. Un meurtre n'y donne ni points
//! de meurtrier ni avis de recherche (`pvp.rs`) et aucune perte d'objet (il n'y en a nulle part dans ArchCore). Le
//! joueur tué peut lâcher un crâne au sol (`pvpskulldroptable.txt` : chance selon l'écart de niveau tueur − victime,
//! multipliée par `Rate.Drop` ; niveau du crâne selon celui de la victime ; objets de `BattleSquare.SkullTids`).
//! Accès : chaque étage est une île du relief serveur, reliée à rien, et les portails d'origine n'ont aucun point dans
//! TeleportPoint.ini. Choix d'ArchCore (`BattleSquare.SupervisorTeleport`) : parler à un superviseur (PNJ 551 à 554,
//! « Battle Square Supervisor », fonction 21 seulement) envoie à la porte de l'étage le plus haut que le niveau
//! permet (LevelMin de RegionTemplate.ini : 20, 30… 70) ; sous le niveau du premier étage, AgpmPvP op 18 code 18 et
//! pas de voyage. Arrivée dans la Battle Square (par ce voyage ou une commande MJ) : codes 2 (zone de combat) puis 13
//! (« You moved to a Battle Square »).
//!
//! **Battleground** (régions 110 à 114, 123 à 125, 175, 176, liste en dur du client) : races différentes ennemies sans
//! Ctrl (`pvp.rs`), pas de points de meurtrier ; un mort y ressuscite dans le fort de sa race (111 humain, 112 orc,
//! 113 elfe de lune, 176 Dragon Scion) au point de résurrection de sa race. Toujours ouvert ; avec un horaire
//! (`Battleground.Schedule`), son début et sa fin sont annoncés à tous par le paquet brut 0x58 (types 2 et 4).

use super::{WorldCtx, WorldState};
use crate::db::{col, Databases};
use crate::{debug, info};
use archcore_net::schema::{mini, ptype};
use archcore_net::{decode_mini, Fields, Packet, Vec3};
use chrono::Timelike;
use rand::Rng;
use std::collections::HashMap;
use std::ops::RangeInclusive;
use std::time::{Duration, Instant};

/// Régions de la Battle Square (étage 1 : 189 à 198 ; étage 6 : 229 à 234).
pub const BATTLE_SQUARE: RangeInclusive<u16> = 189..=234;
/// Étages : région racine et porte d'arrivée (centre d'une région « porte » de l'étage, sur le relief serveur).
pub const FLOORS: [(u16, f32, f32); 6] = [
    (198, -268_600.0, -237_000.0),
    (208, -150_600.0, -276_600.0),
    (209, -22_600.0, -292_600.0),
    (215, -81_800.0, -257_400.0),
    (223, 9_400.0, -239_800.0),
    (229, 77_000.0, -276_200.0),
];
/// Niveau du premier étage si RegionTemplate.ini n'en donne pas.
const FIRST_FLOOR_LEVEL: i32 = 20;
/// Sections de npc.ini des superviseurs de la Battle Square.
pub const SUPERVISORS: RangeInclusive<u32> = 551..=554;
/// Régions des Battlegrounds, en dur dans le client (`FUN_1402ea830`).
pub const BATTLEGROUNDS: [u16; 10] = [110, 111, 112, 113, 114, 123, 124, 125, 175, 176];
/// Cadence de l'horaire du Battleground.
const BG_STEP: Duration = Duration::from_secs(1);

/// Fort d'une race dans le Battleground (`FUN_1403288a0` du client).
pub fn fort(race: i32) -> u16 {
    match race {
        2 => 112,
        3 => 113,
        4 => 176,
        _ => 111,
    }
}

pub fn is_battle_square(region: u16) -> bool {
    BATTLE_SQUARE.contains(&region)
}

fn region_of(ctx: &WorldCtx, st: &WorldState, sid: u32) -> Option<u16> {
    let pos = st.players.get(&sid)?.ch.pos();
    ctx.map.region(pos.x as f64, pos.z as f64)
}

pub fn in_battle_square(ctx: &WorldCtx, st: &WorldState, sid: u32) -> bool {
    region_of(ctx, st, sid).is_some_and(is_battle_square)
}

pub fn in_battleground(ctx: &WorldCtx, st: &WorldState, sid: u32) -> bool {
    region_of(ctx, st, sid).is_some_and(|r| BATTLEGROUNDS.contains(&r))
}

/// Un meurtre ici ne donne ni points de meurtrier, ni statut de premier attaquant, ni droit de mise à prix :
/// Battleground, Battle Square, Shrine en combat, château en siège (siege/war.rs).
pub fn free_kill_zone(ctx: &WorldCtx, st: &WorldState, sid: u32) -> bool {
    in_battleground(ctx, st, sid)
        || in_battle_square(ctx, st, sid)
        || super::shrine::active_shrine(ctx, st, sid).is_some()
        || super::siege::war::war_zone(ctx, st, sid).is_some()
}

/// Étage (rang dans `FLOORS`) le plus haut permis à ce niveau ; sinon le niveau exigé par le premier.
pub fn floor_for_level(levels: impl Fn(u16) -> Option<(i32, i32)>, level: i32) -> Result<usize, i32> {
    let min = |i: usize| levels(FLOORS[i].0).map(|l| l.0).filter(|m| *m > 0).unwrap_or(FIRST_FLOOR_LEVEL + 10 * i as i32);
    (0..FLOORS.len()).rev().find(|i| level >= min(*i)).ok_or(min(0))
}

/// Point de résurrection imposé par la zone où le joueur est mort : zone de sa race d'un Shrine en combat, fort de sa
/// race dans le Battleground ; `None` : règle normale (village de rattachement).
pub fn resurrection_point(ctx: &WorldCtx, st: &WorldState, sid: u32) -> Option<Vec3> {
    if let Some(pos) = super::shrine::resurrection_point(ctx, st, sid) {
        return Some(pos);
    }
    if !in_battleground(ctx, st, sid) {
        return None;
    }
    let p = st.players.get(&sid)?;
    let race = p.factors.ftype[0];
    ctx.regions.race_point(fort(race), p.ch.pos(), race as i64)
}

// ---------------------------------------------------------------------------------------------
// Crânes
// ---------------------------------------------------------------------------------------------

/// pvpskulldroptable.txt : chance (pour cent) par écart de niveau, niveau du crâne par niveau de la victime.
#[derive(Default)]
pub struct SkullTable {
    pub chances: HashMap<i64, f64>,
    pub levels: Vec<(i64, i64, i64)>,
}

impl SkullTable {
    pub async fn load(db: &Databases) -> Self {
        let mut t = SkullTable::default();
        if let Ok(rows) = db.world.rows("SELECT level_gap, chance FROM pvp_skull_chance", ()).await {
            t.chances = rows.iter().map(|r| (col::<i64>(r, "level_gap"), col::<f64>(r, "chance"))).collect();
        }
        if let Ok(rows) = db.world.rows("SELECT victim_level_min, victim_level_max, skull_level FROM pvp_skull_level ORDER BY victim_level_min", ()).await {
            t.levels = rows.iter().map(|r| (col(r, "victim_level_min"), col(r, "victim_level_max"), col(r, "skull_level"))).collect();
        }
        info!("crânes de la Battle Square : {} écart(s) de niveau, {} tranche(s)", t.chances.len(), t.levels.len());
        t
    }

    /// Chance (pour cent) qu'un joueur tué lâche un crâne : 0 hors de la table.
    pub fn chance(&self, killer_level: i32, victim_level: i32) -> f64 {
        self.chances.get(&(killer_level as i64 - victim_level as i64)).copied().unwrap_or(0.0)
    }

    /// Niveau du crâne (1 à 3) selon le niveau de la victime.
    pub fn skull_level(&self, victim_level: i32) -> Option<i64> {
        let l = victim_level as i64;
        self.levels.iter().find(|(a, b, _)| (*a..=*b).contains(&l)).map(|x| x.2)
    }
}

/// Un joueur en tue un autre : dans la Battle Square, la victime peut lâcher un crâne au sol, au nom du tueur.
pub fn on_pvp_kill(ctx: &WorldCtx, st: &mut WorldState, killer: u32, victim: u32, killer_level: i32, victim_level: i32) {
    if !in_battle_square(ctx, st, victim) {
        return;
    }
    let chance = ctx.skulls.chance(killer_level, victim_level) * st.rates.get("drop");
    let Some(level) = ctx.skulls.skull_level(victim_level) else { return };
    let Some(&tid) = ctx.config.world.battle_square_skull_tids.get((level - 1).max(0) as usize) else { return };
    let roll = rand::thread_rng().gen_range(0.0..100.0);
    debug!("[bs] crâne de niveau {level} : tirage {roll:.1} pour {chance:.1} %");
    if roll >= chance {
        return;
    }
    let Some(pos) = st.players.get(&victim).map(|p| p.ch.pos()) else { return };
    info!("[bs] {} lâche un crâne (TID {tid})", st.players[&victim].ch.name);
    super::loot::drop_loot(ctx, st, killer, pos, &[(tid, 1)], &ctx.catalog, victim_level as i64, Instant::now());
}

// ---------------------------------------------------------------------------------------------
// Accès
// ---------------------------------------------------------------------------------------------

/// Après une téléportation : en arrivant de l'extérieur dans la Battle Square, codes 2 puis 13 d'AgpmPvP op 18.
pub fn on_arrival(st: &WorldState, sid: u32, before: Option<u16>, after: Option<u16>) {
    if !after.is_some_and(is_battle_square) || before.is_some_and(is_battle_square) {
        return;
    }
    if let Some(p) = st.players.get(&sid) {
        p.send_raw(super::pvp::message(super::pvp::code::ENTER_COMBAT_AREA, None, None));
        p.send_raw(super::pvp::message(super::pvp::code::MOVE_BATTLE_SQUARE, None, None));
        info!("[bs] {} entre dans la Battle Square", p.ch.name);
    }
}

/// Dialogue avec un superviseur de la Battle Square : voyage à l'étage du niveau du joueur.
pub async fn on_dialog(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    if !ctx.config.world.battle_square_supervisor_teleport || p.int(0) != Some(super::npc_events::dialog_op::REQUEST) {
        return Ok(());
    }
    let base = p.bytes(1).and_then(|b| decode_mini(&mini::EVENT_BASE, b).ok()).unwrap_or_else(|| Fields::new(&mini::EVENT_BASE));
    let Some((npc, _)) = super::npc_events::resolve(&ctx.npcs, &base) else { return Ok(()) };
    if !SUPERVISORS.contains(&npc.cid.wrapping_sub(super::npcs::NPC_CID_BASE)) {
        return Ok(());
    }
    let Some(pl) = st.players.get(&sid).filter(|pl| pl.in_game && !pl.dead) else { return Ok(()) };
    if super::npc_events::npc_distance(npc, pl.ch.pos_x, pl.ch.pos_z) > super::npc_events::MAX_NPC_DISTANCE {
        return Ok(());
    }
    let level = pl.factors.status[7];
    let floor = match floor_for_level(|r| ctx.regions.level_range(r), level) {
        Ok(f) => f,
        Err(min) => {
            pl.send_raw(super::pvp::message(super::pvp::code::NOT_ENOUGH_LEVEL, Some(min as i64), None));
            info!("[bs] {} (niveau {level}) ne peut pas entrer dans la Battle Square ({min} exigé)", pl.ch.name);
            return Ok(());
        }
    };
    let (_, x, z) = FLOORS[floor];
    info!("[bs] {} (niveau {level}) part pour l'étage {} de la Battle Square", pl.ch.name, floor + 1);
    super::teleport(ctx, st, sid, x as f64, None, z as f64).await?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Annonces du Battleground (paquet brut 0x58)
// ---------------------------------------------------------------------------------------------

/// Types d'annonce (champ +0x418) : 2 début, 4 fin.
pub mod announce {
    pub const START: i32 = 2;
    pub const END: i32 = 4;
}

/// Paquet 0x58 : opération 1 (annonce), texte, couleur, type et variante (0 : combat des races).
pub fn announce_packet(kind: i32, variant: i32, text: &str, color: u32) -> Vec<u8> {
    let mut out = vec![0u8; 0x420];
    out[0] = archcore_net::packet::GUARD_HEAD;
    out[5] = ptype::BATTLEGROUND;
    out[0x13] = 1;
    super::guild::society::put_text(&mut out, 0x14, 1024, text);
    out[0x414..0x418].copy_from_slice(&color.to_le_bytes());
    out[0x418..0x41c].copy_from_slice(&kind.to_le_bytes());
    out[0x41c..0x420].copy_from_slice(&variant.to_le_bytes());
    out.push(archcore_net::packet::GUARD_TAIL);
    let len = out.len() as u32;
    out[1..5].copy_from_slice(&len.to_le_bytes());
    out
}

/// Plages « HH:MM-HH:MM » séparées par des « ; » : (début, fin) en minutes depuis minuit.
pub fn parse_schedule(text: &str) -> Vec<(u32, u32)> {
    let hm = |s: &str| -> Option<u32> {
        let (h, m) = s.trim().split_once(':')?;
        let (h, m): (u32, u32) = (h.trim().parse().ok()?, m.trim().parse().ok()?);
        (h < 24 && m < 60).then_some(h * 60 + m)
    };
    text.split(';').filter_map(|r| r.split_once('-')).filter_map(|(a, b)| Some((hm(a)?, hm(b)?))).collect()
}

/// L'instant (minutes depuis minuit) tombe-t-il dans une plage (une plage peut passer minuit) ?
pub fn in_schedule(ranges: &[(u32, u32)], minute: u32) -> bool {
    ranges.iter().any(|&(a, b)| if a <= b { (a..b).contains(&minute) } else { minute >= a || minute < b })
}

#[derive(Default)]
pub struct BattlegroundClock {
    /// Plage en cours au dernier contrôle (`None` : pas encore contrôlé).
    on: Option<bool>,
    next: Option<Instant>,
}

/// Horaire du Battleground : début et fin annoncés à tous (rien sans `Battleground.Schedule`).
pub fn step(ctx: &WorldCtx, st: &mut WorldState, now: Instant) {
    let ranges = parse_schedule(&ctx.config.world.battleground_schedule);
    if ranges.is_empty() || st.battleground.next.is_some_and(|t| now < t) {
        return;
    }
    st.battleground.next = Some(now + BG_STEP);
    let local = match ctx.config.world.time_zone {
        crate::config::TimeZone::Utc => chrono::Utc::now().naive_utc(),
        crate::config::TimeZone::Local => chrono::Local::now().naive_local(),
    };
    let on = in_schedule(&ranges, local.hour() * 60 + local.minute());
    let before = st.battleground.on.replace(on);
    if before.is_none() || before == Some(on) {
        return;
    }
    let (kind, text) = if on { (announce::START, "Début du combat des races au Battleground") } else { (announce::END, "Fin du combat des races au Battleground") };
    info!("[bg] annonce : {text}");
    let plain = announce_packet(kind, 0, text, 0xff00_ff00);
    for p in st.players.values().filter(|p| p.in_game) {
        p.send_raw(plain.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etage_selon_le_niveau() {
        let levels = |r: u16| FLOORS.iter().position(|f| f.0 == r).map(|i| (20 + 10 * i as i32, 30 + 10 * i as i32));
        assert_eq!(floor_for_level(levels, 19), Err(20));
        assert_eq!(floor_for_level(levels, 20), Ok(0));
        assert_eq!(floor_for_level(levels, 45), Ok(2));
        assert_eq!(floor_for_level(levels, 120), Ok(5));
        assert_eq!(floor_for_level(|_| None, 35), Ok(1), "à défaut : 20, 30, 40…");
        assert_eq!((fort(1), fort(2), fort(3), fort(4)), (111, 112, 113, 176));
        assert!(is_battle_square(189) && is_battle_square(234) && !is_battle_square(235));
    }

    #[test]
    fn table_des_cranes() {
        let t = SkullTable { chances: HashMap::from([(0, 10.0), (40, 0.0), (-35, 1.0)]), levels: vec![(1, 40, 1), (41, 70, 2), (71, 100, 3)] };
        assert_eq!((t.chance(50, 50), t.chance(90, 50), t.chance(15, 50), t.chance(10, 60)), (10.0, 0.0, 1.0, 0.0));
        assert_eq!((t.skull_level(40), t.skull_level(41), t.skull_level(100), t.skull_level(101)), (Some(1), Some(2), Some(3), None));
    }

    #[test]
    fn horaire_du_battleground() {
        let r = parse_schedule("20:00-21:30; 23:00-01:00;faux");
        assert_eq!(r, vec![(1200, 1290), (1380, 60)]);
        assert!(in_schedule(&r, 1200) && !in_schedule(&r, 1290) && in_schedule(&r, 30) && in_schedule(&r, 1439) && !in_schedule(&r, 600));
        let p = announce_packet(announce::START, 0, "Go", 0xff00ff00);
        assert_eq!((p.len(), p[5], p[0x13], p[0x14], p[0x418]), (0x421, 0x58, 1, b'G', 2));
    }
}
