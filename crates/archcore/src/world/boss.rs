//! Fenêtre des boss d'Archonia (AgpmBoss 0x64) : ajout d'Archonia sans effet sur le jeu, qui montre l'état des boss de
//! `UIBoss.xml` (table `boss_window`), leur butin possible et un classement.
//!
//! ```text
//!   entrée en jeu, mort ou réapparition d'un boss suivi   ← op 1 : un mini BOSS_SPAWN par boss présent dans le monde
//!                                                           (TID, vivant, prochaine apparition en secondes Unix, heure)
//!   C op 4 (boss choisi, champ 4)                         ← op 3 (16 premiers objets de sa table de butin), op 5 page 0
//!   C op 6 (champ 4, mini BOSS_RANK : page)               ← op 5 de la page
//!   combat contre un boss suivi                           ← op 7 aux joueurs qui le blessent, toutes les 2 s au plus
//!                                                           (20 lignes triées par dégâts)
//! ```
//! Heure du serveur : AgpmTimer champ 2 (secondes Unix) et 3 (décalage du fuseau), voir `nature.rs`. Choix d'ArchCore
//! (le client ne dit rien du serveur) : le classement (op 5) est celui du **dernier combat** contre ce boss, trié par
//! dégâts ; la valeur de chaque ligne est le nombre de victoires du joueur sur ce boss (table `character_boss_kill` :
//! une victoire pour chaque joueur qui l'a blessé pendant le combat où il meurt). Heure et minute affichées : celles de
//! la prochaine apparition, ou de la dernière mort tant que le cadavre est au sol (« Last Death At »).

use super::monsters::Life;
use super::{WorldCtx, WorldState};
use crate::character::unix_now;
use crate::db::{col, Databases};
use crate::{debug, info};
use archcore_net::schema::{self, boss_op, mini};
use archcore_net::{build_mini, build_packet, decode_mini, Fields, Packet, Value};
use chrono::{TimeZone, Timelike};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Boss au plus dans la liste du client.
pub const MAX_BOSSES: usize = 32;
/// Lignes du compteur de dégâts, et d'une page du classement.
pub const METER_ROWS: usize = 20;
pub const RANK_ROWS: usize = 16;
/// Intervalle minimal entre deux compteurs de dégâts d'un même boss.
pub const METER_EVERY: Duration = Duration::from_secs(2);

/// Un boss de la fenêtre.
#[derive(Clone, Debug, PartialEq)]
pub struct BossEntry {
    pub tid: u32,
    pub category: u8,
    pub name: String,
}

#[derive(Default)]
pub struct BossCatalog {
    pub list: Vec<BossEntry>,
}

impl BossCatalog {
    pub async fn load(db: &Databases) -> Self {
        let mut c = BossCatalog::default();
        match db.world.rows("SELECT tid, category, name FROM boss_window ORDER BY row_order", ()).await {
            Ok(rows) => {
                c.list = rows.iter().take(MAX_BOSSES).map(|r| BossEntry { tid: col(r, "tid"), category: col::<u32>(r, "category") as u8, name: col(r, "name") }).collect();
            }
            Err(e) => crate::warn!("[boss] table boss_window non lue ({e:#}) : lancer `archcore db-init` puis `archcore import`"),
        }
        info!("fenêtre des boss : {} boss suivi(s)", c.list.len());
        c
    }

    pub fn tracks(&self, tid: u32) -> bool {
        self.list.iter().any(|b| b.tid == tid)
    }
}

/// Un combattant du dernier combat contre un boss.
#[derive(Clone, Debug, PartialEq)]
pub struct Fighter {
    pub cid: u32,
    pub name: String,
    pub damage: u64,
}

#[derive(Default)]
pub struct Bosses {
    /// Dernière mort (secondes Unix) par TID.
    pub last_death: HashMap<u32, i64>,
    /// Classement du dernier combat par TID, du plus fort au plus faible.
    pub last_fight: HashMap<u32, Vec<Fighter>>,
    /// Dernier compteur de dégâts envoyé, par CID du monstre.
    meter_at: HashMap<u32, Instant>,
}

/// Heure et minute (fuseau de `World.TimeZone`) d'un instant Unix.
fn hour_minute(ctx: &WorldCtx, unix: i64) -> (i64, i64) {
    let t = match ctx.config.world.time_zone {
        crate::config::TimeZone::Utc => chrono::Utc.timestamp_opt(unix, 0).single().map(|d| d.naive_utc()),
        crate::config::TimeZone::Local => chrono::Local.timestamp_opt(unix, 0).single().map(|d| d.naive_local()),
    };
    t.map(|d| (d.hour() as i64, d.minute() as i64)).unwrap_or((0, 0))
}

/// Mini BOSS_SPAWN : structure brute de 48 octets.
pub fn spawn_mini(tid: u32, next: i64, hour: i64, minute: i64, alive: bool) -> Vec<u8> {
    let mut b = vec![0u8; 48];
    b[0..4].copy_from_slice(&tid.to_le_bytes());
    b[0x18..0x20].copy_from_slice(&(next.max(0) as u64).to_le_bytes());
    b[0x20] = hour as u8;
    b[0x21] = minute as u8;
    b[0x28..0x2c].copy_from_slice(&(alive as i32).to_le_bytes());
    build_mini(&mini::BOSS_SPAWN, &Fields::new(&mini::BOSS_SPAWN).with(0, b))
}

/// État d'un boss dans le monde : (vivant, prochaine apparition en secondes Unix ou 0) ; `None` s'il n'y apparaît pas.
fn status(st: &WorldState, tid: u32, now: Instant) -> Option<(bool, i64)> {
    let mut seen = false;
    let mut next: Option<i64> = None;
    for m in st.monsters.monsters.iter().filter(|m| m.tid == tid && m.owner.is_none()) {
        seen = true;
        match m.life {
            Life::Alive => return Some((true, 0)),
            Life::Gone { respawn_at } => {
                let at = unix_now() + respawn_at.saturating_duration_since(now).as_secs() as i64;
                next = Some(next.map_or(at, |n| n.min(at)));
            }
            Life::Dead { .. } => {}
        }
    }
    seen.then_some((false, next.unwrap_or(0)))
}

/// Op 1 : liste des boss suivis présents dans le monde.
pub fn list_packet(ctx: &WorldCtx, st: &WorldState) -> Vec<u8> {
    let now = Instant::now();
    let mut minis = Vec::new();
    for b in &ctx.bosses.list {
        let Some((alive, next)) = status(st, b.tid, now) else { continue };
        let shown = if next > 0 { next } else { st.bosses.last_death.get(&b.tid).copied().unwrap_or(0) };
        let (h, m) = if shown > 0 { hour_minute(ctx, shown) } else { (0, 0) };
        minis.push(Value::Bytes(spawn_mini(b.tid, next, h, m, alive)));
    }
    let s = &schema::BOSS;
    build_packet(s, &Fields::new(s).with(0, boss_op::LIST).with(1, 0).with(2, minis))
}

/// Entrée en jeu : la liste.
pub fn on_enter(ctx: &WorldCtx, st: &WorldState, sid: u32) {
    if ctx.bosses.list.is_empty() {
        return;
    }
    if let Some(p) = st.players.get(&sid) {
        p.send_raw(list_packet(ctx, st));
    }
}

/// La liste à tous les joueurs en jeu.
fn broadcast_list(ctx: &WorldCtx, st: &WorldState) {
    let plain = list_packet(ctx, st);
    for p in st.players.values().filter(|p| p.in_game) {
        p.send_raw(plain.clone());
    }
}

/// Un monstre suivi réapparaît, ou son délai de réapparition vient d'être tiré : liste renvoyée.
pub fn on_life_change(ctx: &WorldCtx, st: &WorldState, tid: u32) {
    if ctx.bosses.tracks(tid) {
        broadcast_list(ctx, st);
    }
}

/// Mort d'un boss suivi : classement du combat, victoires en base, liste renvoyée. `damage` : dégâts par CID de joueur.
pub fn on_kill(ctx: &WorldCtx, st: &mut WorldState, tid: u32, monster: u32, damage: &HashMap<u32, u64>) {
    if !ctx.bosses.tracks(tid) {
        return;
    }
    let mut fighters: Vec<Fighter> = damage
        .iter()
        .filter(|(_, d)| **d > 0)
        .filter_map(|(cid, d)| st.by_cid.get(cid).and_then(|sid| st.players.get(sid)).map(|p| Fighter { cid: *cid, name: p.ch.name.clone(), damage: *d }))
        .collect();
    fighters.sort_by(|a, b| b.damage.cmp(&a.damage).then(a.cid.cmp(&b.cid)));
    info!("[boss] boss {tid} tué : {} combattant(s)", fighters.len());
    let db = ctx.db.characters.clone();
    let cids: Vec<u32> = fighters.iter().map(|f| f.cid).collect();
    tokio::spawn(async move {
        for cid in cids {
            let sql = "INSERT INTO character_boss_kill (character_id, boss_tid, kills) VALUES (?, ?, 1) ON DUPLICATE KEY UPDATE kills = kills + 1, last_kill_at = CURRENT_TIMESTAMP";
            if let Err(e) = db.exec(sql, (cid, tid)).await {
                crate::warn!("[boss] victoire du personnage {cid} sur {tid} : {e:#}");
            }
        }
    });
    st.bosses.last_fight.insert(tid, fighters);
    st.bosses.last_death.insert(tid, unix_now());
    st.bosses.meter_at.remove(&monster);
    broadcast_list(ctx, st);
}

/// Mini BOSS_DROP : 16 objets que le boss peut donner (tables de butin, `LootTables::possible`) : TID, taux, quantité.
pub fn drops_mini(tid: u32, items: &[(u32, i64, i64)]) -> Vec<u8> {
    let mut b = vec![0u8; 196];
    b[0..4].copy_from_slice(&tid.to_le_bytes());
    for (i, (item, rate, count)) in items.iter().take(16).enumerate() {
        b[4 + i * 4..8 + i * 4].copy_from_slice(&item.to_le_bytes());
        b[0x44 + i * 4..0x48 + i * 4].copy_from_slice(&(*rate as i32).to_le_bytes());
        b[0x84 + i * 4..0x88 + i * 4].copy_from_slice(&(*count as i32).to_le_bytes());
    }
    build_mini(&mini::BOSS_DROP, &Fields::new(&mini::BOSS_DROP).with(0, b))
}

/// Objets possibles d'un boss, au niveau où il apparaît.
fn possible_drops(ctx: &WorldCtx, st: &WorldState, tid: u32) -> Vec<(u32, i64, i64)> {
    let stats = st.monsters.stats_of(tid);
    let level = st.monsters.monsters.iter().find(|m| m.tid == tid).map(|m| m.level).unwrap_or(stats.level as i64);
    ctx.loot.possible(&super::loot::Source::of(tid, level, &stats), 16)
}

/// Mini BOSS_RANK d'une page du dernier combat : `kills` donne les victoires de chaque CID sur ce boss.
pub fn rank_mini(fighters: &[Fighter], page: usize, me: (u32, &str), kills: &HashMap<u32, i64>) -> Vec<u8> {
    let pages = fighters.len().div_ceil(RANK_ROWS).max(1);
    let page = page.min(pages - 1);
    let rows: Vec<Value> = fighters
        .iter()
        .enumerate()
        .skip(page * RANK_ROWS)
        .take(RANK_ROWS)
        .map(|(i, f)| {
            let r = &mini::BOSS_RANK_ROW;
            Value::Bytes(build_mini(r, &Fields::new(r).with(0, (i + 1).min(127)).with(1, f.name.as_str()).with(2, kills.get(&f.cid).copied().unwrap_or(0))))
        })
        .collect();
    let my_rank = fighters.iter().position(|f| f.cid == me.0).map(|i| (i + 1).min(127)).unwrap_or(0);
    let s = &mini::BOSS_RANK;
    let f = Fields::new(s).with(0, page).with(1, pages).with(2, my_rank).with(3, me.1).with(4, kills.get(&me.0).copied().unwrap_or(0)).with(5, rows);
    build_mini(s, &f)
}

/// Mini BOSS_DAMAGE : compteur de dégâts d'un boss en combat.
pub fn damage_mini(st: &WorldState, tid: u32, damage: &HashMap<u32, u64>) -> Vec<u8> {
    let mut rows: Vec<(u32, u64)> = damage.iter().map(|(c, d)| (*c, *d)).filter(|(_, d)| *d > 0).collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let list: Vec<Value> = rows
        .iter()
        .filter_map(|(cid, d)| {
            let p = st.by_cid.get(cid).and_then(|sid| st.players.get(sid))?;
            let guild = st.guilds.of_cid.get(cid).and_then(|g| st.guilds.by_id.get(g));
            let r = &mini::BOSS_DAMAGE_ROW;
            let f = Fields::new(r)
                .with(0, p.ch.name.as_str())
                .with(1, p.ch.template_id)
                .with(2, p.factors.status[7])
                .with(3, guild.map(|g| g.name.as_str()).unwrap_or(""))
                .with(4, guild.map(|g| g.mark_tid).unwrap_or(0))
                .with(5, guild.map(|g| g.mark_color).unwrap_or(0))
                .with(6, *d as i64);
            Some(Value::Bytes(build_mini(r, &f)))
        })
        .take(METER_ROWS)
        .collect();
    let s = &mini::BOSS_DAMAGE;
    build_mini(s, &Fields::new(s).with(0, tid).with(1, list.len()).with(2, list))
}

/// Victoires sur un boss, par CID.
async fn kills_of(ctx: &WorldCtx, tid: u32) -> anyhow::Result<HashMap<u32, i64>> {
    let rows = ctx.db.characters.rows("SELECT character_id, kills FROM character_boss_kill WHERE boss_tid = ?", (tid,)).await?;
    Ok(rows.iter().map(|r| (col::<u32>(r, "character_id"), col::<i64>(r, "kills"))).collect())
}

/// Op 5 au joueur : la page `page` du classement de `tid`.
async fn send_ranking(ctx: &WorldCtx, st: &WorldState, sid: u32, tid: u32, page: usize) -> anyhow::Result<()> {
    let Some(p) = st.players.get(&sid) else { return Ok(()) };
    let kills = kills_of(ctx, tid).await?;
    let fighters = st.bosses.last_fight.get(&tid).map(|v| v.as_slice()).unwrap_or(&[]);
    let s = &schema::BOSS;
    let rank = rank_mini(fighters, page, (p.ch.id, &p.ch.name), &kills);
    p.send(s, Fields::new(s).with(0, boss_op::RANKING).with(4, tid).with(5, rank));
    Ok(())
}

/// Paquet AgpmBoss reçu du client.
pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let tid = p.int(4).unwrap_or(0).max(0) as u32;
    match p.int(0) {
        Some(boss_op::SELECT) => {
            if let Some(pl) = st.players.get(&sid) {
                let s = &schema::BOSS;
                pl.send(s, Fields::new(s).with(0, boss_op::DROPS).with(3, drops_mini(tid, &possible_drops(ctx, st, tid))));
            }
            debug!("[boss] #{sid} consulte le boss {tid}");
            send_ranking(ctx, st, sid, tid, 0).await?;
        }
        Some(boss_op::RANKING_PAGE) => {
            let page = p.bytes(5).and_then(|b| decode_mini(&mini::BOSS_RANK, b).ok()).and_then(|f| f.int(0)).unwrap_or(0).max(0) as usize;
            send_ranking(ctx, st, sid, tid, page).await?;
        }
        op => debug!("[boss] #{sid} op {op:?} ignorée"),
    }
    Ok(())
}

/// Compteur de dégâts (op 7) des boss suivis en combat, toutes les `METER_EVERY` au plus, à ceux qui les blessent.
pub fn step(ctx: &WorldCtx, st: &mut WorldState, now: Instant) {
    if ctx.bosses.list.is_empty() {
        return;
    }
    let fighting: Vec<(u32, u32)> = st
        .monsters
        .monsters
        .iter()
        .filter(|m| m.is_alive() && !m.aggro.is_empty() && m.owner.is_none() && ctx.bosses.tracks(m.tid))
        .map(|m| (m.cid, m.tid))
        .collect();
    for (cid, tid) in fighting {
        if st.bosses.meter_at.get(&cid).is_some_and(|t| now < *t + METER_EVERY) {
            continue;
        }
        st.bosses.meter_at.insert(cid, now);
        let damage = super::summons::damage_by_owner(st, cid);
        let s = &schema::BOSS;
        let plain = build_packet(s, &Fields::new(s).with(0, boss_op::DAMAGE).with(6, damage_mini(st, tid, &damage)));
        for who in damage.keys() {
            if let Some(p) = st.by_cid.get(who).and_then(|sid| st.players.get(sid)) {
                p.send_raw(plain.clone());
            }
        }
    }
    st.bosses.meter_at.retain(|cid, _| st.monsters.get(*cid).is_some_and(|m| m.is_alive()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use archcore_net::packet::mini_payload;
    use archcore_net::{decode_mini, decode_packet};

    #[test]
    fn apparition_d_un_boss() {
        let m = spawn_mini(2073, 1_800_000_000, 21, 5, false);
        let f = decode_mini(&mini::BOSS_SPAWN, mini_payload(&m)).unwrap();
        let b = f.bytes(0).unwrap();
        assert_eq!(b.len(), 48);
        assert_eq!(u32::from_le_bytes(b[0..4].try_into().unwrap()), 2073);
        assert_eq!(u64::from_le_bytes(b[0x18..0x20].try_into().unwrap()), 1_800_000_000);
        assert_eq!((b[0x20], b[0x21], b[0x28]), (21, 5, 0));
        assert_eq!(spawn_mini(1, 0, 0, 0, true)[4 + 0x28 + 1], 1, "vivant (après longueur et drapeaux)");
    }

    #[test]
    fn classement_par_page() {
        let fighters: Vec<Fighter> = (0..20).map(|i| Fighter { cid: i, name: format!("J{i}"), damage: 1000 - i as u64 }).collect();
        let kills = HashMap::from([(3u32, 7i64), (17, 2)]);
        let m = rank_mini(&fighters, 1, (17, "J17"), &kills);
        let f = decode_mini(&mini::BOSS_RANK, mini_payload(&m)).unwrap();
        assert_eq!((f.int(0), f.int(1), f.int(2), f.text(3).as_deref(), f.int(4)), (Some(1), Some(2), Some(18), Some("J17"), Some(2)));
        let rows = f.list(5).unwrap();
        assert_eq!(rows.len(), 4);
        let Value::Bytes(first) = &rows[0] else { panic!() };
        let r = decode_mini(&mini::BOSS_RANK_ROW, first).unwrap();
        assert_eq!((r.int(0), r.text(1).as_deref()), (Some(17), Some("J16")));
        // Page trop loin : la dernière ; sans combat : une page vide.
        let last = decode_mini(&mini::BOSS_RANK, mini_payload(&rank_mini(&fighters, 9, (3, "J3"), &kills))).unwrap();
        assert_eq!((last.int(0), last.int(2), last.int(4)), (Some(1), Some(4), Some(7)));
        let none = decode_mini(&mini::BOSS_RANK, mini_payload(&rank_mini(&[], 0, (3, "J3"), &kills))).unwrap();
        assert_eq!((none.int(1), none.int(2)), (Some(1), Some(0)));
    }

    #[test]
    fn paquet_de_liste() {
        let s = &schema::BOSS;
        let plain = build_packet(s, &Fields::new(s).with(0, boss_op::LIST).with(1, 0).with(2, vec![Value::Bytes(spawn_mini(2073, 0, 0, 0, true))]));
        let p = decode_packet(s, &plain).unwrap();
        assert_eq!((p.int(0), p.int(1), p.list(2).map(|l| l.len())), (Some(1), Some(0), Some(1)));
    }
}
