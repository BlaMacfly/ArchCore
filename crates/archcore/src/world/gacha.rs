//! Gacha de l'épisode 8 (« Fortune Gate », AgpmEventGacha 0x57) : socles des villes, objets du monde de fonction 38.
//!
//! ```text
//!   Socles : `gacha_object` (archcore import) : objet des obj*.ini, type de tirage de ses données (`GachaStart`,
//!     `Type=N`, `GachaEnd`) : 1 armes (modèle 9081), 2 armures (5780), 3 accessoires (5891), 5 charisme (13173).
//!   Types : gachatypetemplate.ini (`RaceCheck`, `ClassCheck`, `LevelCheck`, `LevelLimit`, `DropTable`) ; tables
//!     gachadroptable*.txt par niveau du joueur (1 à 99) : objet demandé (`Gacha_ItemTID` × `Gacha_Item_Number`),
//!     or (`Gacha_Cost`), charisme (`CharismaPoint`), poids des rangs `Rank1` à `Rank4` sur 10 000. Tables de
//!     l'épisode 8, gardées dans `client_table`.
//!   Objets tirables : colonnes `Gacha_Type_Numer` (type), `GachaRank` (rang 1 à 4), `GachaMinLv` et `GachaMaxLv`
//!     (niveaux du joueur), `Race` et `Class` de la table des objets.
//!   Tirage : rang par les seuils cumulés du niveau du joueur, lus comme le code partagé d'Archonia (`R4`, `R3+R4`,
//!     `R2+R3+R4`, le rang 1 en reste) ; puis un objet au hasard, équiprobable, parmi ceux du type et du rang qui
//!     conviennent au niveau, à la race et à la classe (selon les contrôles du type). Choix d'ArchCore (le client
//!     ne le montre pas) : rang sans objet → le rang inférieur, puis le supérieur.
//!   Op 0 (clic, à 600 unités au plus comme le code partagé d'Archonia) → op 1 : code (0 à 4 : la fenêtre UI_Gacha
//!     s'ouvre ; 5 erreur inconnue, 6 niveau hors table : message seul), objet, nombre, or, charisme, et la bande
//!     des objets tirables (TID u32 à la suite, 200 au plus) que le client fait défiler.
//!   Op 2 (« Start ») → op 3 : TID tiré (champ 3) ou refus (champ 4) : 1 objets, 2 or, 3 charisme manquants,
//!     4 inventaire plein (rien de pris). Le coût est pris au tirage (objets de l'inventaire ou de l'inventaire Cash),
//!     l'objet rangé dans l'inventaire (Cash pour un objet Cash) avec des options tirées comme celles d'un objet créé.
//! ```

use super::{WorldCtx, WorldState};
use crate::{info, warn};
use archcore_net::schema::{self, gacha_op, gacha_result, mini};
use archcore_net::{build_mini, build_packet, decode_mini, Fields, Packet, Vec3};
use rand::Rng;
use std::collections::{BTreeMap, HashMap};

/// Fonction d'événement des socles (`Function=38`).
pub const EVENT_FUNCTION: i64 = 38;
/// Portée du code partagé d'Archonia (`FUN_140304580(…, 600)` à la réception de l'op 0).
pub const RANGE: f64 = 600.0;
/// Objets de la bande au plus (choix d'ArchCore : un tirage hors de la bande arrête le défilement aussitôt).
const BAND_MAX: usize = 200;
/// Poids des rangs : sur 10 000 (`× 0,0001` chez Archonia).
const RANK_SCALE: u64 = 10_000;

/// Une section de gachatypetemplate.ini.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GachaType {
    pub id: u32,
    pub name: String,
    pub race_check: bool,
    pub class_check: bool,
    pub level_check: bool,
    /// Niveau sans ligne dans la table : refus (code 6) plutôt que la dernière ligne.
    pub level_limit: bool,
    pub drop_table: String,
    /// Lignes de la table, par niveau du joueur.
    pub rows: BTreeMap<i64, Row>,
}

/// Une ligne d'une table de tirage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Row {
    pub item_tid: u32,
    pub item_count: i64,
    pub gold: i64,
    pub charisma: i64,
    /// Poids des rangs 1 à 4.
    pub ranks: [u64; 4],
}

/// Un objet tirable.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GachaItem {
    pub tid: u32,
    pub kind: u32,
    pub rank: usize,
    pub min_level: i64,
    pub max_level: i64,
    pub races: Vec<i64>,
    pub classes: Vec<i64>,
}

#[derive(Clone, Debug, Default)]
pub struct GachaCatalog {
    pub types: HashMap<u32, GachaType>,
    pub items: Vec<GachaItem>,
    /// Socles : objet du monde → (type, position).
    pub objects: HashMap<u32, (u32, Vec3)>,
}

/// Sections de gachatypetemplate.ini, dans l'ordre.
pub fn parse_types(text: &str) -> Vec<GachaType> {
    let mut out: Vec<GachaType> = Vec::new();
    for line in text.lines().map(|l| l.trim_start_matches('\u{feff}').trim()) {
        if let Some(id) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            out.extend(id.trim().parse::<u32>().ok().map(|id| GachaType { id, ..Default::default() }));
            continue;
        }
        let (Some(t), Some((k, v))) = (out.last_mut(), line.split_once('=')) else { continue };
        let (k, v) = (k.trim(), v.trim());
        let on = v.parse::<i64>().unwrap_or(0) != 0;
        match k {
            "Name" => t.name = v.to_string(),
            "RaceCheck" => t.race_check = on,
            "ClassCheck" => t.class_check = on,
            "LevelCheck" => t.level_check = on,
            "LevelLimit" => t.level_limit = on,
            "DropTable" => t.drop_table = v.to_string(),
            _ => {}
        }
    }
    out
}

/// Tables de tirage citées par gachatypetemplate.ini (`archcore import`).
pub fn drop_table_names(text: &str) -> Vec<String> {
    let mut names: Vec<String> = parse_types(text).into_iter().map(|t| t.drop_table).filter(|n| !n.is_empty()).collect();
    names.dedup();
    names
}

/// Lignes d'une table gachadroptable*.txt, par niveau (`Character_Lv`) ; les colonnes se repèrent par leur nom
/// (`CharismaPoint` n'existe que dans la table du charisme).
pub fn parse_drop_table(text: &str) -> BTreeMap<i64, Row> {
    let mut lines = text.lines();
    let Some(header) = lines.next() else { return BTreeMap::new() };
    let columns: Vec<&str> = header.trim_start_matches('\u{feff}').split('\t').map(str::trim).collect();
    let at = |name: &str| columns.iter().position(|c| *c == name);
    let Some(level) = at("Character_Lv") else { return BTreeMap::new() };
    let (tid, count, gold, charisma) = (at("Gacha_ItemTID"), at("Gacha_Item_Number"), at("Gacha_Cost"), at("CharismaPoint"));
    let ranks = [at("Rank1"), at("Rank2"), at("Rank3"), at("Rank4")];
    let mut out = BTreeMap::new();
    for line in lines {
        let c: Vec<&str> = line.split('\t').map(str::trim).collect();
        let num = |i: Option<usize>| i.and_then(|i| c.get(i)).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0).max(0.0) as i64;
        let Some(lv) = c.get(level).and_then(|v| v.parse::<i64>().ok()).filter(|v| *v > 0) else { continue };
        let row = Row { item_tid: num(tid) as u32, item_count: num(count), gold: num(gold), charisma: num(charisma), ranks: ranks.map(|r| num(r) as u64) };
        out.insert(lv, row);
    }
    out
}

/// Objet tirable d'après ses colonnes de la table des objets ; `None` hors gacha.
pub fn item_of(tid: u32, raw: &serde_json::Value) -> Option<GachaItem> {
    let text = |k: &str| raw.get(k).and_then(|v| v.as_str()).map(str::trim).unwrap_or("");
    let num = |k: &str| text(k).parse::<f64>().ok().unwrap_or(0.0) as i64;
    let list = |k: &str| text(k).split(';').filter_map(|v| v.trim().parse::<i64>().ok()).filter(|v| *v > 0).collect::<Vec<_>>();
    let (kind, rank) = (num("Gacha_Type_Numer"), num("GachaRank"));
    if kind <= 0 || !(1..=4).contains(&rank) {
        return None;
    }
    let min_level = num("GachaMinLv").max(1);
    let max_level = match num("GachaMaxLv") {
        0 => i64::MAX,
        m => m.max(min_level),
    };
    Some(GachaItem { tid, kind: kind as u32, rank: rank as usize, min_level, max_level, races: list("Race"), classes: list("Class") })
}

impl GachaType {
    /// Ligne du niveau `level`, sinon le code du refus.
    pub fn row(&self, level: i64) -> Result<&Row, i64> {
        if self.rows.is_empty() {
            return Err(gacha_result::UNKNOWN);
        }
        match self.rows.get(&level) {
            Some(r) => Ok(r),
            None if self.level_limit => Err(gacha_result::LEVEL_LIMIT),
            // Sans limite : la ligne la plus proche.
            None => Ok(self.rows.range(..=level).next_back().or_else(|| self.rows.iter().next()).map(|(_, r)| r).unwrap()),
        }
    }
}

/// Objets tirables d'un type pour un joueur, par rang (indice 0 : rang 1).
pub fn pools(items: &[GachaItem], t: &GachaType, level: i64, race: i64, class: i64) -> [Vec<u32>; 4] {
    let mut out: [Vec<u32>; 4] = Default::default();
    for i in items.iter().filter(|i| i.kind == t.id) {
        if t.level_check && !(i.min_level..=i.max_level).contains(&level) {
            continue;
        }
        if t.race_check && !i.races.is_empty() && !i.races.contains(&race) {
            continue;
        }
        if t.class_check && !i.classes.is_empty() && !i.classes.contains(&class) {
            continue;
        }
        out[i.rank - 1].push(i.tid);
    }
    for p in &mut out {
        p.sort_unstable();
        p.dedup();
    }
    out
}

/// Rang tiré (1 à 4) : seuils cumulés depuis le rang 4, le rang 1 prend le reste des 10 000.
pub fn draw_rank(rng: &mut impl Rng, ranks: [u64; 4]) -> usize {
    let roll = rng.gen_range(0..RANK_SCALE);
    let mut bound = 0;
    for rank in [4, 3, 2] {
        bound += ranks[rank - 1];
        if roll < bound {
            return rank;
        }
    }
    1
}

/// Objet tiré au rang `rank` ; rang vide : les rangs inférieurs, puis les supérieurs.
pub fn pick(rng: &mut impl Rng, pools: &[Vec<u32>; 4], rank: usize) -> Option<u32> {
    let order = (1..=rank).rev().chain(rank + 1..=4);
    let pool = order.map(|r| &pools[r - 1]).find(|p| !p.is_empty())?;
    Some(pool[rng.gen_range(0..pool.len())])
}

/// Bande que le client fait défiler : tous les objets tirables, du rang 1 au rang 4, 200 au plus.
pub fn band(pools: &[Vec<u32>; 4]) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    for t in pools.iter().flatten() {
        if out.len() < BAND_MAX && !out.contains(t) {
            out.push(*t);
        }
    }
    out
}

/// Code du contrôle du coût (`gacha_result`) : objets, puis or, puis charisme.
pub fn afford(row: &Row, items_held: i64, money: i64, charisma: i64) -> i64 {
    if row.item_tid > 0 && row.item_count > 0 && items_held < row.item_count {
        gacha_result::NOT_ENOUGH_ITEM
    } else if money < row.gold {
        gacha_result::NOT_ENOUGH_GOLD
    } else if charisma < row.charisma {
        gacha_result::NOT_ENOUGH_CHARISMA
    } else {
        gacha_result::SUCCESS
    }
}

/// Op 1 : ouverture (ou refus seul), coût et bande.
pub fn open_packet(base: &[u8], cid: u32, code: i64, row: Option<&Row>, band: &[u32]) -> Vec<u8> {
    let s = &schema::EVENT_GACHA;
    let mut f = Fields::new(s).with(0, gacha_op::OPEN).with(1, base.to_vec()).with(2, cid).with(3, code);
    if let Some(r) = row {
        f = f.with(4, r.item_tid).with(5, r.item_count).with(6, r.gold).with(7, r.charisma);
    }
    let bytes: Vec<u8> = band.iter().flat_map(|t| t.to_le_bytes()).collect();
    if !bytes.is_empty() {
        f = f.with(8, bytes);
    }
    build_packet(s, &f)
}

/// Op 3 : objet tiré (`tid`) ou refus (`code`).
pub fn result_packet(base: &[u8], cid: u32, tid: u32, code: i64) -> Vec<u8> {
    let s = &schema::EVENT_GACHA;
    build_packet(s, &Fields::new(s).with(0, gacha_op::RESULT).with(1, base.to_vec()).with(2, cid).with(3, tid).with(4, code))
}

impl GachaCatalog {
    /// Types et tables (`client_table`), socles (`gacha_object`), objets tirables (`item_data`).
    pub async fn load(db: &crate::db::Databases) -> Self {
        use crate::db::col;
        let text_of = |name: String| async move {
            let row = db.world.first("SELECT data FROM client_table WHERE row_idx = 0 AND name = ?", (name,)).await.ok().flatten();
            row.and_then(|r| serde_json::from_str::<String>(&col::<String>(&r, "data")).ok()).unwrap_or_default()
        };
        let mut out = GachaCatalog::default();
        for mut t in parse_types(&text_of("gachatypetemplate.ini".into()).await) {
            if !t.drop_table.is_empty() {
                t.rows = parse_drop_table(&text_of(t.drop_table.clone()).await);
            }
            out.types.insert(t.id, t);
        }
        match db.world.rows("SELECT object_id, gacha_type, pos_x, pos_y, pos_z FROM gacha_object", ()).await {
            Ok(rows) => {
                for r in rows {
                    let at = Vec3::new(col::<f64>(&r, "pos_x") as f32, col::<f64>(&r, "pos_y") as f32, col::<f64>(&r, "pos_z") as f32);
                    out.objects.insert(col(&r, "object_id"), (col(&r, "gacha_type"), at));
                }
            }
            Err(e) => warn!("gacha : socles non chargés ({e}) : lancer `archcore db-init` puis `archcore import`"),
        }
        if let Ok(rows) = db.world.rows("SELECT tid, raw FROM item_data", ()).await {
            out.items = rows
                .iter()
                .filter_map(|r| {
                    let raw = serde_json::from_str::<serde_json::Value>(&col::<String>(r, "raw")).ok()?;
                    item_of(col(r, "tid"), &raw)
                })
                .collect();
        }
        if out.types.is_empty() || out.objects.is_empty() {
            warn!("gacha : {} type(s), {} socle(s) : lancer `archcore import`", out.types.len(), out.objects.len());
        } else {
            info!("gacha : {} type(s) de tirage, {} socle(s), {} objet(s) tirable(s)", out.types.len(), out.objects.len(), out.items.len());
        }
        out
    }
}

/// Socle désigné par l'EVENT_BASE, à portée du joueur : son type de tirage.
fn socket_of(ctx: &WorldCtx, st: &WorldState, sid: u32, base: &Fields) -> Option<u32> {
    let player = st.players.get(&sid)?;
    if base.int(3) != Some(EVENT_FUNCTION) || base.int(0) != Some(super::npcs::BASE_TYPE_OBJECT) {
        warn!("[gacha] {} : événement {:?} de fonction {:?} : pas un socle", player.ch.name, base.int(1), base.int(3));
        return None;
    }
    let object = base.int(1)? as u32;
    let Some((kind, at)) = ctx.gacha.objects.get(&object).copied() else {
        warn!("[gacha] {} : objet {object} : pas un socle de gacha", player.ch.name);
        return None;
    };
    let me = super::presence::position_now(player, std::time::Instant::now());
    let distance = super::combat::dist2d(me, at);
    if distance > RANGE + super::npc_events::RANGE_TOLERANCE {
        warn!("[gacha] {} trop loin du socle {object} ({distance:.0})", player.ch.name);
        return None;
    }
    Some(kind)
}

/// Exemplaires de l'objet `tid` dans l'inventaire et l'inventaire Cash (le Fortune Gate Pass est un objet Cash), hors
/// objets proposés à l'échange.
fn held(p: &super::Player, tid: u32) -> Vec<(u32, i64)> {
    let offered = |i: &u32| p.trade.as_ref().is_some_and(|t| t.items.contains_key(i));
    p.inventory.items.values().chain(p.inventory.cash.values()).filter(|i| i.tid == tid && !offered(&i.iid)).map(|i| (i.iid, i.count)).collect()
}

/// État d'un joueur devant un type : ligne, objets tirables, code du contrôle.
fn prepare<'a>(ctx: &'a WorldCtx, p: &super::Player, kind: u32) -> (Option<&'a Row>, [Vec<u32>; 4], i64) {
    let Some(t) = ctx.gacha.types.get(&kind) else { return (None, Default::default(), gacha_result::UNKNOWN) };
    let f = &p.factors;
    let (level, race, class) = (f.status[7] as i64, f.ftype[0] as i64, f.ftype[2] as i64);
    let row = match t.row(level) {
        Ok(r) => r,
        Err(code) => return (None, Default::default(), code),
    };
    let pools = pools(&ctx.gacha.items, t, level, race, class);
    if pools.iter().all(Vec::is_empty) {
        return (Some(row), pools, gacha_result::UNKNOWN);
    }
    let items: i64 = held(p, row.item_tid).iter().map(|(_, n)| n).sum();
    let mut code = afford(row, items, p.ch.money as i64, p.ch.charisma as i64);
    if code == gacha_result::SUCCESS && p.inventory.free_count() == 0 {
        code = gacha_result::INVENTORY_FULL;
    }
    (Some(row), pools, code)
}

/// AGPMEVENTGACHA reçu du client.
pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let base = p.bytes(1).and_then(|b| decode_mini(&mini::EVENT_BASE, b).ok()).unwrap_or_else(|| Fields::new(&mini::EVENT_BASE));
    let raw_base = build_mini(&mini::EVENT_BASE, &base);
    match p.int(0) {
        Some(gacha_op::REQUEST) => {
            let Some(kind) = socket_of(ctx, st, sid, &base) else { return Ok(()) };
            let Some(player) = st.players.get(&sid) else { return Ok(()) };
            let (row, pools, code) = prepare(ctx, player, kind);
            let shown = if matches!(code, gacha_result::UNKNOWN | gacha_result::LEVEL_LIMIT) { Vec::new() } else { band(&pools) };
            player.send_raw(open_packet(&raw_base, player.ch.id, code, row, &shown));
            info!("[gacha] {} ouvre le socle {:?} (type {kind}) : code {code}, {} objet(s) dans la bande", player.ch.name, base.int(1), shown.len());
            Ok(())
        }
        Some(gacha_op::ROLL) => roll(ctx, st, sid, &base, &raw_base).await,
        other => {
            warn!("[gacha] #{sid} op {other:?} inattendue");
            Ok(())
        }
    }
}

/// Bouton « Start » : coût pris, objet tiré et rangé.
async fn roll(ctx: &WorldCtx, st: &mut WorldState, sid: u32, base: &Fields, raw_base: &[u8]) -> anyhow::Result<()> {
    use super::items::{item_add_of, item_remove, item_status};
    let Some(kind) = socket_of(ctx, st, sid, base) else { return Ok(()) };
    let Some(p) = st.players.get(&sid) else { return Ok(()) };
    let cid = p.ch.id;
    let (row, pools, mut code) = prepare(ctx, p, kind);
    let rank = row.map(|r| draw_rank(&mut rand::thread_rng(), r.ranks)).unwrap_or(1);
    let won = pick(&mut rand::thread_rng(), &pools, rank);
    let (Some(row), Some(tid)) = (row.copied(), won) else {
        p.send_raw(result_packet(raw_base, cid, 0, if code == gacha_result::SUCCESS { gacha_result::UNKNOWN } else { code }));
        return Ok(());
    };
    // Place : l'objet tiré, ou la case d'une pile de l'objet demandé entièrement prise (dans l'inventaire ordinaire).
    let stack = ctx.catalog.get(tid).map(|d| d.stack).unwrap_or(1).max(1);
    let piles = held(p, row.item_tid);
    let cash = ctx.catalog.is_cash(tid);
    // Une pile de l'objet demandé entièrement prise libère sa case (inventaire ordinaire ou Cash, selon l'objet tiré).
    let frees = |iid: &u32, n: &i64| {
        (if cash { p.inventory.cash.contains_key(iid) } else { p.inventory.items.contains_key(iid) }) && *n <= row.item_count
    };
    let room = if cash { p.inventory.cash_has_room(tid, 1, stack) } else { p.inventory.placement(tid, 1, stack, None).is_some() };
    let freed = row.item_count > 0 && piles.iter().any(|(iid, n)| frees(iid, n));
    if code == gacha_result::INVENTORY_FULL && (room || freed) {
        code = gacha_result::SUCCESS;
    } else if code == gacha_result::SUCCESS && !room && !freed {
        // Inventaire Cash plein : refus avant de prendre le coût.
        code = gacha_result::INVENTORY_FULL;
    }
    if code != gacha_result::SUCCESS {
        info!("[gacha] {} : tirage refusé (code {code})", p.ch.name);
        p.send_raw(result_packet(raw_base, cid, 0, code));
        return Ok(());
    }
    // Coût : objets demandés (piles dans l'ordre de l'inventaire), puis or et charisme.
    let p = st.players.get_mut(&sid).unwrap();
    let mut left = if row.item_tid > 0 { row.item_count } else { 0 };
    for (iid, n) in piles {
        if left <= 0 {
            break;
        }
        let take = n.min(left);
        left -= take;
        match p.inventory.consume(&ctx.db, iid, take).await? {
            Some(rest) => p.send_raw(p.inventory.count_packet(&rest, cid)),
            None => p.send_raw(item_remove(iid, Some(cid))),
        }
    }
    if row.charisma > 0 {
        let charisma = (p.ch.charisma as i64 - row.charisma).max(0) as u32;
        ctx.db.characters.exec("UPDATE characters SET charisma = ? WHERE id = ?", (charisma, cid)).await?;
        p.ch.charisma = charisma;
        p.factors.status[4] = charisma as i32;
        super::skills::send_points(p);
    }
    let (added, status) = if cash {
        (p.inventory.add_cash(&ctx.db, tid, 1, stack).await?, item_status::CASH_INVENTORY)
    } else {
        (p.inventory.add(&ctx.db, tid, 1, stack, None).await?, item_status::INVENTORY)
    };
    let Some((it, created)) = added else {
        // Place prise entre-temps : rien d'autre à rendre que le message.
        p.send_raw(result_packet(raw_base, cid, 0, gacha_result::INVENTORY_FULL));
        return Ok(());
    };
    if created {
        let level = ctx.catalog.get(tid).and_then(|d| d.equip.as_ref()).map(|e| e.level.max(1)).unwrap_or(1);
        let (options, sockets) = super::options::generate(&ctx.catalog, &ctx.options, tid, level, &mut rand::thread_rng());
        p.inventory.set_generated(&ctx.db, it.iid, &options, sockets).await?;
    }
    let it = p.inventory.get_usable(it.iid).cloned().unwrap_or(it);
    p.send_raw(if created { item_add_of(&it, cid, status, Some(it.grid)) } else { p.inventory.count_packet(&it, cid) });
    if let Some(c) = super::items::convert_add(&it, cid) {
        p.send_raw(c);
    }
    let name = p.ch.name.clone();
    let money = p.ch.money as i64;
    if row.gold > 0 {
        super::set_money(ctx, st, sid, money - row.gold).await?;
    }
    if let Some(p) = st.players.get(&sid) {
        p.send_raw(result_packet(raw_base, cid, tid, gacha_result::SUCCESS));
    }
    let item = ctx.catalog.get(tid).map(|d| d.name.clone()).unwrap_or_default();
    info!("[gacha] {name} : type {kind}, rang {rank} → {item} (TID {tid}) ; coût {} × {}, {} or, {} charisme", row.item_count, row.item_tid, row.gold, row.charisma);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    const TYPES: &str = "[1]\r\nName=Weapon\r\nComment=\r\nRaceCheck=1\r\nClassCheck=1\r\nLevelCheck=1\r\nLevelLimit=1\r\nDropTable=gachadroptableweapon.txt\r\nEnumEnd=0\r\n[5]\r\nName=Charisma\r\nRaceCheck=0\r\nClassCheck=1\r\nLevelCheck=1\r\nLevelLimit=1\r\nDropTable=gachadroptablecharisma.txt\r\nEnumEnd=0\r\n";
    const WEAPON: &str = "Character_Lv\tGacha_ItemTID\tGacha_Item_Number\tGacha_Cost\tRank1\tRank2\tRank3\tRank4\r\n1\t7436\t1\t\t1000\t2200\t5300\t1500\r\n2\t7436\t1\t\t1000\t2200\t5300\t1500\r\n";
    const CHARISMA: &str = "Character_Lv\tGacha_ItemTID\tGacha_Item_Number\tGacha_Cost\tCharismaPoint\tRank1\tRank2\tRank3\tRank4\r\n1\t\t\t\t100\t6390\t3100\t500\t10\r\n";

    #[test]
    fn types_et_tables() {
        let t = parse_types(TYPES);
        assert_eq!(t.len(), 2);
        assert_eq!((t[0].id, t[0].name.as_str(), t[0].race_check, t[0].level_limit), (1, "Weapon", true, true));
        assert_eq!((t[1].id, t[1].race_check, t[1].drop_table.as_str()), (5, false, "gachadroptablecharisma.txt"));
        assert_eq!(drop_table_names(TYPES), vec!["gachadroptableweapon.txt", "gachadroptablecharisma.txt"]);
        let w = parse_drop_table(WEAPON);
        assert_eq!(w[&1], Row { item_tid: 7436, item_count: 1, gold: 0, charisma: 0, ranks: [1000, 2200, 5300, 1500] });
        let c = parse_drop_table(CHARISMA);
        assert_eq!(c[&1], Row { item_tid: 0, item_count: 0, gold: 0, charisma: 100, ranks: [6390, 3100, 500, 10] });
        let mut weapon = t[0].clone();
        weapon.rows = w;
        assert!(weapon.row(2).is_ok());
        assert_eq!(weapon.row(3), Err(gacha_result::LEVEL_LIMIT));
        weapon.level_limit = false;
        assert_eq!(weapon.row(50).map(|r| r.item_tid), Ok(7436), "sans limite : la ligne la plus proche");
        assert_eq!(GachaType::default().row(1), Err(gacha_result::UNKNOWN));
    }

    #[test]
    fn objets_et_bassins() {
        let raw = |kind: &str, rank: &str, lo: &str, hi: &str, race: &str, class: &str| {
            serde_json::json!({"Gacha_Type_Numer": kind, "GachaRank": rank, "GachaMinLv": lo, "GachaMaxLv": hi, "Race": race, "Class": class})
        };
        assert_eq!(item_of(1, &raw("", "", "", "", "", "")), None);
        assert_eq!(item_of(1, &raw("1", "5", "1", "9", "", "")), None, "rang hors de 1 à 4");
        let bow = item_of(537, &raw("1", "3", "5", "16", "1;2;3", "2")).unwrap();
        assert_eq!((bow.kind, bow.rank, bow.min_level, bow.max_level, bow.races.clone(), bow.classes.clone()), (1, 3, 5, 16, vec![1, 2, 3], vec![2]));
        let items = vec![
            bow,
            item_of(4001, &raw("1", "4", "5", "16", "1;2;3", "1")).unwrap(),
            item_of(7830, &raw("1", "1", "1", "6", "", "")).unwrap(),
            item_of(46, &raw("2", "1", "1", "13", "1", "2")).unwrap(),
        ];
        let mut t = parse_types(TYPES).remove(0);
        let p = pools(&items, &t, 10, 1, 2);
        assert_eq!(p, [vec![], vec![], vec![537], vec![]], "niveau, classe et type");
        assert_eq!(pools(&items, &t, 3, 4, 1), [vec![7830], vec![], vec![], vec![]], "race 4 : seul l'objet sans race");
        t.class_check = false;
        assert_eq!(pools(&items, &t, 10, 1, 2)[3], vec![4001]);
        assert_eq!(band(&pools(&items, &t, 10, 1, 2)), vec![537, 4001]);
    }

    #[test]
    fn tirage_des_rangs() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let mut seen = [0usize; 4];
        for _ in 0..20_000 {
            seen[draw_rank(&mut rng, [1000, 2200, 5300, 1500]) - 1] += 1;
        }
        // 10 %, 22 %, 53 %, 15 %.
        for (got, want) in seen.iter().zip([2000, 4400, 10600, 3000]) {
            assert!((*got as i64 - want).abs() < 400, "{seen:?}");
        }
        assert_eq!(draw_rank(&mut rng, [10_000, 0, 0, 0]), 1);
        assert_eq!(draw_rank(&mut rng, [0, 0, 0, 10_000]), 4);
        // Rang vide : inférieur d'abord, puis supérieur.
        let p: [Vec<u32>; 4] = [vec![1], vec![], vec![], vec![9]];
        assert_eq!(pick(&mut rng, &p, 3), Some(1));
        assert_eq!(pick(&mut rng, &[vec![], vec![], vec![], vec![9]], 1), Some(9));
        assert_eq!(pick(&mut rng, &Default::default(), 2), None);
    }

    #[test]
    fn cout_et_paquets() {
        let row = Row { item_tid: 7436, item_count: 1, gold: 500, charisma: 0, ranks: [0; 4] };
        assert_eq!(afford(&row, 0, 1000, 0), gacha_result::NOT_ENOUGH_ITEM);
        assert_eq!(afford(&row, 1, 100, 0), gacha_result::NOT_ENOUGH_GOLD);
        assert_eq!(afford(&row, 1, 500, 0), gacha_result::SUCCESS);
        let charisma = Row { charisma: 100, ..Default::default() };
        assert_eq!(afford(&charisma, 0, 0, 99), gacha_result::NOT_ENOUGH_CHARISMA);
        let base = build_mini(&mini::EVENT_BASE, &Fields::new(&mini::EVENT_BASE).with(0, 1).with(1, 126_159_030u32).with(3, EVENT_FUNCTION));
        let open = archcore_net::decode_packet(&schema::EVENT_GACHA, &open_packet(&base, 7, 0, Some(&row), &[537, 4001])).unwrap();
        assert_eq!((open.int(0), open.int(2), open.int(3), open.int(4), open.int(5), open.int(6), open.int(7)), (Some(1), Some(7), Some(0), Some(7436), Some(1), Some(500), Some(0)));
        assert_eq!(open.bytes(8), Some(&[0x19, 2, 0, 0, 0xa1, 0x0f, 0, 0][..]));
        let b = decode_mini(&mini::EVENT_BASE, open.bytes(1).unwrap()).unwrap();
        assert_eq!((b.int(1), b.int(3)), (Some(126_159_030), Some(38)));
        let refused = archcore_net::decode_packet(&schema::EVENT_GACHA, &open_packet(&base, 7, gacha_result::LEVEL_LIMIT, None, &[])).unwrap();
        assert_eq!((refused.int(3), refused.int(4), refused.bytes(8)), (Some(6), None, None));
        let done = archcore_net::decode_packet(&schema::EVENT_GACHA, &result_packet(&base, 7, 537, 0)).unwrap();
        assert_eq!((done.int(0), done.int(3), done.int(4)), (Some(3), Some(537), Some(0)));
    }
}
