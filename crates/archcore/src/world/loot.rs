//! Butin des monstres : or et objets posés au sol, à ramasser.
//!
//! À la mort d'un monstre, le tueur obtient :
//! - de l'or : entre `Geld_Min` et `Geld_Max` du monstre (CharacterDataTable.txt), × le taux `gold` ;
//! - des objets de groupe : pour chaque groupe (groupdroprate.txt), un tirage sur 100 000 avec le taux du
//!   groupe (× taux `drop`, × `DropMeditation` du monstre si le groupe y est sensible, + bonus selon le niveau
//!   du tueur) ; puis un rang d'objet selon le rang du monstre (droprankrate.txt), puis un objet du groupe et
//!   du rang (GroupID, DropRank, DropRate d'itemdatatable.txt) adapté au niveau du monstre ;
//! - les objets propres au monstre (itemdroptable.txt) : toujours à 100 000, sinon selon leur taux.
//!
//! Tout tombe au sol autour du cadavre (AGPMITEM op 0, statut 1 « au sol », champ 6 = position), réservé
//! au tueur pendant 30 s, et disparaît au bout de 45 s. Le client demande le ramassage par AGPMITEM op 11
//! (champ 2 = IID) ; le serveur répond op 12 (champ 1 = 0 réussi, 1 échec) puis crédite l'or ou range l'objet.

use super::items::{item_add, item_count, item_remove, item_status, ItemCatalog};
use super::monsters::MonsterStats;
use super::{set_money, WorldCtx, WorldState};
use crate::db::{col, Databases};
use crate::{debug, info};
use archcore_net::schema::{self, mini};
use archcore_net::{build_mini, build_packet, Fields, Packet, Vec3};
use rand::Rng;
use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

/// Modèle de l'objet « or » ; sa quantité est la somme.
pub const GOLD_TID: u32 = 268;
/// IID des objets au sol (plage 0x30000000–0x3fffffff, jamais en base).
pub const GROUND_IID_BASE: u32 = 0x3000_0000;
/// Dispersion autour du cadavre.
pub const SCATTER: f64 = 150.0;
/// Durée de vie d'un objet au sol, et durée pendant laquelle il est réservé au tueur.
pub const EXPIRE: Duration = Duration::from_secs(45);
pub const OWNERSHIP: Duration = Duration::from_secs(30);
/// Distance maximale de ramassage.
pub const PICKUP_DISTANCE: f64 = 2000.0;
/// Statut AGPMITEM « au sol ».
pub const FIELD: i64 = 1;
/// Opérations AGPMITEM du ramassage.
pub const PICKUP_ITEM: i64 = 11;
pub const PICKUP_ITEM_RESULT: i64 = 12;
pub mod pickup_result {
    pub const SUCCESS: i64 = 0;
    pub const FAIL: i64 = 1;
}

/// Objet propre à un monstre.
#[derive(Clone, Debug, PartialEq)]
pub struct MonsterDrop {
    pub tid: u32,
    pub min: i64,
    pub max: i64,
    /// Sur 100 000.
    pub rate: i64,
}

/// Objet d'un groupe de butin.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupItem {
    pub tid: u32,
    /// Rang 1 à 10.
    pub rank: usize,
    /// Poids dans le tirage.
    pub rate: i64,
    pub level_min: i64,
    pub level_max: i64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DropGroup {
    pub id: u32,
    pub meditation: bool,
    /// Sur 100 000.
    pub rate: i64,
    /// Bonus de taux par tranche de 10 niveaux du tueur.
    pub level_bonus: [i64; 10],
    pub items: Vec<GroupItem>,
}

/// Ce que le monstre apporte au tirage.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Source {
    pub tid: u32,
    pub level: i64,
    pub geld_min: i64,
    pub geld_max: i64,
    pub drop_rank: i64,
    pub meditation: f64,
}

impl Source {
    pub fn of(tid: u32, level: i64, s: &MonsterStats) -> Self {
        Source { tid, level, geld_min: s.geld_min as i64, geld_max: s.geld_max as i64, drop_rank: s.drop_rank as i64, meditation: s.drop_meditation }
    }
}

/// Taux du serveur appliqués au tirage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rates {
    pub gold: f64,
    pub drop: f64,
}

#[derive(Default)]
pub struct LootTables {
    pub per_monster: HashMap<u32, Vec<MonsterDrop>>,
    pub groups: Vec<DropGroup>,
    /// Rang du monstre → pour cent par rang d'objet (1 à 10).
    pub rank_rates: HashMap<i64, [i64; 10]>,
}

fn json_array<const N: usize>(text: &str) -> [i64; N] {
    let v: Vec<i64> = serde_json::from_str(text).unwrap_or_default();
    std::array::from_fn(|i| v.get(i).copied().unwrap_or(0))
}

impl LootTables {
    pub async fn load(db: &Databases) -> anyhow::Result<Self> {
        let mut t = LootTables::default();
        // Les tables peuvent manquer si l'import n'a pas eu les tables du serveur.
        let Ok(rows) = db.world.rows("SELECT monster_tid, item_tid, min_count, max_count, rate FROM monster_drop ORDER BY id", ()).await else {
            crate::warn!("butin non chargé : lancer `archcore db-init` puis `archcore import --server …`");
            return Ok(t);
        };
        for r in &rows {
            t.per_monster.entry(col(r, "monster_tid")).or_default().push(MonsterDrop {
                tid: col(r, "item_tid"),
                min: col(r, "min_count"),
                max: col(r, "max_count"),
                rate: col(r, "rate"),
            });
        }
        for r in db.world.rows("SELECT monster_rank, rates FROM drop_rank_rate", ()).await? {
            t.rank_rates.insert(col(&r, "monster_rank"), json_array(&col::<String>(&r, "rates")));
        }
        let mut by_id: BTreeMap<u32, DropGroup> = BTreeMap::new();
        for r in db.world.rows("SELECT group_id, meditation, rate, level_bonus FROM drop_group", ()).await? {
            let id: u32 = col(&r, "group_id");
            by_id.insert(
                id,
                DropGroup { id, meditation: col::<i64>(&r, "meditation") != 0, rate: col(&r, "rate"), level_bonus: json_array(&col::<String>(&r, "level_bonus")), items: Vec::new() },
            );
        }
        let sql = "SELECT tid, JSON_VALUE(raw, '$.GroupID') AS g, JSON_VALUE(raw, '$.DropRank') AS rk, JSON_VALUE(raw, '$.DropRate') AS rt, \
                   JSON_VALUE(raw, '$.SuitableLevelMin') AS lmin, JSON_VALUE(raw, '$.SuitableLevelMax') AS lmax FROM item_data";
        let num = |r: &mysql_async::Row, k: &str| col::<Option<String>>(r, k).and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(0.0) as i64;
        for r in db.world.rows(sql, ()).await? {
            let (g, rank, rate) = (num(&r, "g"), num(&r, "rk"), num(&r, "rt"));
            if g <= 0 || !(1..=10).contains(&rank) || rate <= 0 {
                continue;
            }
            if let Some(group) = by_id.get_mut(&(g as u32)) {
                group.items.push(GroupItem { tid: col(&r, "tid"), rank: rank as usize, rate, level_min: num(&r, "lmin"), level_max: num(&r, "lmax") });
            }
        }
        t.groups = by_id.into_values().collect();
        let items: usize = t.groups.iter().map(|g| g.items.len()).sum();
        info!("butin : {} monstre(s) à butin propre, {} groupe(s) ({items} objet(s)), {} rang(s)", t.per_monster.len(), t.groups.len(), t.rank_rates.len());
        Ok(t)
    }

    /// Butin d'un monstre tué par un personnage de niveau `killer_level` : (TID, quantité), l'or en premier.
    pub fn roll(&self, rng: &mut impl Rng, m: &Source, killer_level: i32, rates: Rates) -> Vec<(u32, i64)> {
        let mut out = Vec::new();
        if m.geld_max > 0 {
            let lo = m.geld_min.clamp(0, m.geld_max);
            let gold = (rng.gen_range(lo..=m.geld_max) as f64 * rates.gold).round() as i64;
            if gold > 0 {
                out.push((GOLD_TID, gold));
            }
        }
        if let Some(ranks) = self.rank_rates.get(&m.drop_rank) {
            for g in &self.groups {
                if let Some(tid) = self.roll_group(rng, g, ranks, m, killer_level, rates) {
                    out.push((tid, 1));
                }
            }
        }
        for d in self.per_monster.get(&m.tid).map(Vec::as_slice).unwrap_or(&[]) {
            let hit = d.rate >= 100_000 || (rng.gen_range(0..100_000) as f64) < d.rate as f64 * rates.drop;
            if hit {
                out.push((d.tid, rng.gen_range(d.min..=d.max.max(d.min))));
            }
        }
        out
    }

    fn roll_group(&self, rng: &mut impl Rng, g: &DropGroup, ranks: &[i64; 10], m: &Source, killer_level: i32, rates: Rates) -> Option<u32> {
        let mut rate = g.rate as f64 * rates.drop;
        if g.meditation {
            rate *= m.meditation;
        }
        let bonus = g.level_bonus[((killer_level.max(1) - 1) / 10).clamp(0, 9) as usize];
        let rate = (rate + bonus as f64).max(0.0);
        if rng.gen_range(0..100_000) as f64 >= rate {
            return None;
        }
        // Objets adaptés au niveau du monstre, dans un rang que ce monstre peut donner.
        let fits = |i: &&GroupItem| (i.level_max <= 1 || (i.level_min..=i.level_max).contains(&m.level)) && ranks[i.rank - 1] > 0;
        let candidates: Vec<&GroupItem> = g.items.iter().filter(fits).collect();
        if candidates.is_empty() {
            return None;
        }
        let roll = rng.gen_range(0..100);
        let (mut total, mut chosen) = (0, 0);
        for rank in 1..=10 {
            if candidates.iter().any(|i| i.rank == rank) {
                chosen = rank;
                total += ranks[rank - 1];
                if roll < total {
                    break;
                }
            }
        }
        let in_rank: Vec<&&GroupItem> = candidates.iter().filter(|i| i.rank == chosen).collect();
        let weight: i64 = in_rank.iter().map(|i| i.rate).sum();
        let mut pick = rng.gen_range(0..weight.max(1));
        for i in in_rank {
            if pick < i.rate {
                return Some(i.tid);
            }
            pick -= i.rate;
        }
        None
    }
}

/// Objet posé au sol.
#[derive(Clone, Debug, PartialEq)]
pub struct GroundItem {
    pub iid: u32,
    pub tid: u32,
    pub count: i64,
    pub pos: Vec3,
    /// CID du tueur, seul à pouvoir le ramasser avant `owner_until`.
    pub owner: u32,
    pub owner_until: Instant,
    pub expire_at: Instant,
}

/// Objets au sol du monde.
#[derive(Default)]
pub struct Ground {
    pub items: BTreeMap<u32, GroundItem>,
    next: u32,
}

impl Ground {
    fn alloc(&mut self) -> u32 {
        self.next = self.next.wrapping_add(1) % 0x1000_0000;
        GROUND_IID_BASE + self.next
    }
}

/// AGPMITEM op 0 d'un objet au sol.
pub fn ground_packet(g: &GroundItem) -> Vec<u8> {
    let s = &schema::ITEM;
    let fm = &mini::ITEM_FIELD;
    let field = build_mini(fm, &Fields::new(fm).with(0, g.pos));
    build_packet(s, &Fields::new(s).with(0, super::items::item_op::ADD).with(1, FIELD).with(2, g.iid).with(3, g.tid).with(4, g.owner).with(5, g.count).with(6, field))
}

/// AGPMITEM op 12 : résultat d'un ramassage.
pub fn pickup_result_packet(outcome: i64, iid: u32, tid: u32, count: i64) -> Vec<u8> {
    let s = &schema::ITEM;
    build_packet(s, &Fields::new(s).with(0, PICKUP_ITEM_RESULT).with(1, outcome).with(2, iid).with(3, tid).with(5, count))
}


/// Position qui compte pour ramasser : la destination de la marche (le client marche jusqu'à l'objet).
fn reach_pos(p: &super::Player) -> Vec3 {
    p.dest.unwrap_or_else(|| p.ch.pos())
}

fn dist2d(a: Vec3, b: Vec3) -> f64 {
    super::combat::dist2d(a, b)
}

/// Pose le butin du monstre tué autour de `center`, au nom du joueur `killer`.
pub fn drop_loot(_ctx: &WorldCtx, st: &mut WorldState, killer: u32, center: Vec3, loot: &[(u32, i64)], catalog: &ItemCatalog, now: Instant) {
    let Some(owner) = st.players.get(&killer).map(|p| p.ch.id) else { return };
    let mut rng = rand::thread_rng();
    let mut placed = Vec::new();
    for &(tid, count) in loot {
        // Un objet qui ne s'empile pas tombe en autant d'exemplaires.
        let stack = if tid == GOLD_TID { i64::MAX } else { catalog.get(tid).map(|d| d.stack).unwrap_or(1) };
        let mut left = count.max(1);
        while left > 0 {
            let n = left.min(stack.max(1));
            left -= n;
            let pos = Vec3::new(center.x + rng.gen_range(-SCATTER..SCATTER) as f32, center.y, center.z + rng.gen_range(-SCATTER..SCATTER) as f32);
            let iid = st.ground.alloc();
            let item = GroundItem { iid, tid, count: n, pos, owner, owner_until: now + OWNERSHIP, expire_at: now + EXPIRE };
            st.ground.items.insert(iid, item.clone());
            placed.push(item);
        }
    }
    for item in &placed {
        let plain = ground_packet(item);
        for p in st.players.values_mut().filter(|p| p.in_game) {
            if super::sector::sees(p.view_pos, item.pos) && p.seen_items.insert(item.iid) {
                p.send_raw(plain.clone());
            }
        }
    }
    if !placed.is_empty() {
        let list: Vec<String> = placed.iter().map(|i| format!("{} × {}", i.count, catalog.get(i.tid).map(|d| d.name.as_str()).unwrap_or("?"))).collect();
        debug!("[butin] {}", list.join(", "));
    }
}

/// Objets au sol des secteurs vus depuis `pos` : montrés s'ils sont nouveaux, retirés s'ils en sont sortis.
pub fn sync_ground(_ctx: &WorldCtx, st: &mut WorldState, sid: u32, pos: Vec3) {
    let Some(p) = st.players.get_mut(&sid) else { return };
    for item in st.ground.items.values() {
        if super::sector::sees(pos, item.pos) && p.seen_items.insert(item.iid) {
            p.send_raw(ground_packet(item));
        } else if !super::sector::keeps(pos, item.pos) && p.seen_items.remove(&item.iid) {
            p.send_raw(item_remove(item.iid, Some(item.owner)));
        }
    }
}

/// Retire un objet du sol chez ceux qui le voient.
fn take_from_ground(st: &mut WorldState, iid: u32) -> Option<GroundItem> {
    let item = st.ground.items.remove(&iid)?;
    let gone = item_remove(iid, Some(item.owner));
    for p in st.players.values_mut() {
        if p.seen_items.remove(&iid) {
            p.send_raw(gone.clone());
        }
    }
    Some(item)
}

/// Objets au sol arrivés à expiration.
pub fn expire(st: &mut WorldState, now: Instant) {
    let due: Vec<u32> = st.ground.items.values().filter(|i| i.expire_at <= now).map(|i| i.iid).collect();
    for iid in due {
        take_from_ground(st, iid);
    }
}

/// AGPMITEM reçu d'un joueur : ramassage (op 11) ici, changement de place (op 2) dans `equipment.rs`.
pub async fn on_item_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    if super::trade::is_offered(st, sid, p.int(2).unwrap_or(0) as u32) {
        debug!("[échange] #{sid} AGPMITEM op {:?} sur un objet proposé à l'échange : ignoré", p.int(0));
        return Ok(());
    }
    match p.int(0) {
        Some(PICKUP_ITEM) => pick_up(ctx, st, sid, p.int(2).unwrap_or(0) as u32, Instant::now()).await,
        Some(super::items::item_op::UPDATE) => super::equipment::on_item_update(ctx, st, sid, p).await,
        Some(super::recovery::USE_ITEM) => super::recovery::use_item(ctx, st, sid, p, &ctx.catalog).await,
        Some(super::travel::USE_RETURN_SCROLL) => super::travel::use_return(ctx, st, sid, Instant::now()).await,
        Some(super::equipment::SPLIT_ITEM) => super::equipment::split(ctx, st, sid, p).await,
        Some(super::equipment::REQUEST_DESTROY_ITEM) => super::equipment::destroy(ctx, st, sid, p.int(2).unwrap_or(0) as u32).await,
        op => {
            debug!("[world] #{sid} AGPMITEM op {op:?} (ignoré)");
            Ok(())
        }
    }
}

async fn pick_up(ctx: &WorldCtx, st: &mut WorldState, sid: u32, iid: u32, now: Instant) -> anyhow::Result<()> {
    let Some(p) = st.players.get(&sid) else { return Ok(()) };
    let Some(item) = st.ground.items.get(&iid).cloned() else { return Ok(()) };
    let fail = |st: &WorldState| st.players[&sid].send_raw(pickup_result_packet(pickup_result::FAIL, iid, item.tid, item.count));
    if !p.in_game || p.dead || (item.owner != p.ch.id && now < item.owner_until) || dist2d(reach_pos(p), item.pos) > PICKUP_DISTANCE {
        fail(st);
        return Ok(());
    }
    let (cid, name, money) = (p.ch.id, p.ch.name.clone(), p.ch.money as i64);
    if item.tid == GOLD_TID {
        take_from_ground(st, iid);
        st.players[&sid].send_raw(pickup_result_packet(pickup_result::SUCCESS, iid, item.tid, item.count));
        set_money(ctx, st, sid, money + item.count).await?;
        info!("[butin] {name} ramasse {} pièce(s) d'or", item.count);
        return Ok(());
    }
    let stack = ctx.catalog.get(item.tid).map(|d| d.stack).unwrap_or(1);
    let player = st.players.get_mut(&sid).unwrap();
    let Some((stored, created)) = player.inventory.add(&ctx.db, item.tid, item.count, stack, None).await? else {
        fail(st);
        return Ok(());
    };
    player.send_raw(if created { item_add(stored.iid, stored.tid, Some(cid), stored.count, item_status::INVENTORY, Some(stored.grid)) } else { item_count(&stored, cid) });
    take_from_ground(st, iid);
    st.players[&sid].send_raw(pickup_result_packet(pickup_result::SUCCESS, iid, item.tid, item.count));
    let label = ctx.catalog.get(item.tid).map(|d| d.name.clone()).unwrap_or_else(|| format!("TID {}", item.tid));
    info!("[butin] {name} ramasse {} × {label}", item.count);
    Ok(())
}

/// Mort d'un monstre : tirage et dépôt du butin.
pub fn on_kill(ctx: &WorldCtx, st: &mut WorldState, killer: u32, monster: u32, now: Instant) {
    let Some(m) = st.monsters.get(monster) else { return };
    let (pos, source) = (m.pos, Source::of(m.tid, m.level, &st.monsters.stats_of(m.tid)));
    let level = st.players.get(&killer).map(|p| p.factors.status[7]).unwrap_or(1);
    let rates = Rates { gold: st.rates.get("gold"), drop: st.rates.get("drop") };
    let loot = ctx.loot.roll(&mut rand::thread_rng(), &source, level, rates);
    drop_loot(ctx, st, killer, pos, &loot, &ctx.catalog, now);
}

#[cfg(test)]
mod tests {
    use super::*;
    use archcore_net::{decode_mini, decode_packet};
    use rand::SeedableRng;

    const ONE: Rates = Rates { gold: 1.0, drop: 1.0 };

    fn worm() -> Source {
        Source { tid: 450, level: 1, geld_min: 10, geld_max: 12, drop_rank: 1, meditation: 1.0 }
    }

    fn tables() -> LootTables {
        let mut t = LootTables::default();
        t.rank_rates.insert(1, [90, 7, 3, 0, 0, 0, 0, 0, 0, 0]);
        t.groups.push(DropGroup {
            id: 1,
            meditation: false,
            rate: 100_000,
            level_bonus: [0; 10],
            items: vec![
                GroupItem { tid: 10, rank: 1, rate: 1, level_min: 1, level_max: 10 },
                GroupItem { tid: 11, rank: 1, rate: 1, level_min: 50, level_max: 60 },
                GroupItem { tid: 12, rank: 5, rate: 1, level_min: 1, level_max: 10 },
            ],
        });
        t.per_monster.insert(450, vec![MonsterDrop { tid: 99, min: 2, max: 2, rate: 100_000 }, MonsterDrop { tid: 98, min: 1, max: 1, rate: 0 }]);
        t
    }

    #[test]
    fn or_objets_de_groupe_et_propres() {
        let t = tables();
        let mut rng = rand::rngs::StdRng::seed_from_u64(3);
        for _ in 0..50 {
            let loot = t.roll(&mut rng, &worm(), 1, ONE);
            let gold = loot[0];
            assert!(gold.0 == GOLD_TID && (10..=12).contains(&gold.1), "or : {gold:?}");
            // Seul le TID 10 convient : niveau du monstre et rang donnable (le rang 5 vaut 0 %).
            assert!(loot.contains(&(10, 1)));
            assert!(!loot.iter().any(|(t, _)| *t == 11 || *t == 12));
            assert!(loot.contains(&(99, 2)), "objet garanti");
            assert!(!loot.iter().any(|(t, _)| *t == 98), "taux nul");
        }
        // Taux d'or doublé, butin coupé.
        let loot = t.roll(&mut rng, &Source { geld_min: 10, geld_max: 10, ..worm() }, 1, Rates { gold: 2.0, drop: 0.0 });
        assert_eq!(loot, vec![(GOLD_TID, 20), (99, 2)]);
        // Sans or ni rang de butin : seulement les objets propres.
        assert_eq!(t.roll(&mut rng, &Source { geld_max: 0, drop_rank: 0, ..worm() }, 1, ONE), vec![(99, 2)]);
    }

    #[test]
    fn bonus_de_niveau() {
        let mut t = tables();
        t.groups[0].rate = 0;
        t.groups[0].level_bonus = [100_000, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let mut rng = rand::rngs::StdRng::seed_from_u64(9);
        let src = Source { geld_max: 0, tid: 1, ..worm() };
        assert_eq!(t.roll(&mut rng, &src, 5, ONE), vec![(10, 1)], "niveau 1 à 10 : bonus");
        assert!(t.roll(&mut rng, &src, 15, ONE).is_empty(), "niveau 11 à 20 : rien");
    }

    #[test]
    fn paquet_au_sol() {
        let now = Instant::now();
        let g = GroundItem { iid: GROUND_IID_BASE + 1, tid: GOLD_TID, count: 11, pos: Vec3::new(1.0, 2.0, 3.0), owner: 7, owner_until: now, expire_at: now };
        let p = decode_packet(&schema::ITEM, &ground_packet(&g)).unwrap();
        assert_eq!((p.int(0), p.int(1), p.int(2), p.int(3), p.int(4), p.int(5)), (Some(0), Some(FIELD), Some(GROUND_IID_BASE as i64 + 1), Some(268), Some(7), Some(11)));
        let f = decode_mini(&mini::ITEM_FIELD, p.bytes(6).unwrap()).unwrap();
        assert_eq!(f.vec3(0), Some(Vec3::new(1.0, 2.0, 3.0)));
        let r = decode_packet(&schema::ITEM, &pickup_result_packet(pickup_result::SUCCESS, 5, 268, 11)).unwrap();
        assert_eq!((r.int(0), r.int(1), r.int(2), r.int(5)), (Some(PICKUP_ITEM_RESULT), Some(0), Some(5), Some(11)));
    }
}
