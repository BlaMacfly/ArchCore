//! Conversion des objets : renforcement physique, sockets et pierres d'esprit (`ItemConvertTable.txt`).
//!
//! ```text
//!   Table : sections séparées par une ligne vide, chacune avec sa ligne d'en-tête.
//!     1 Reinforcement Level  niveau 1–16 : bonus d'arme et d'armure du niveau, rang (lettre), pierres
//!                            admises (colonnes Spirit1–5 : rang de pierre 1 à 5)
//!     2 Success              par rang d'objet (ligne) et niveau visé (colonne) : chance de réussite
//!     3 Unchanged            … d'échec sans perte
//!     4 Reset Reinforcement  … de retour au niveau précédent
//!     5 Destroy              … de destruction
//!     6 Elemental Stone      rang de pierre 1–5 : valeur sur une arme, chance sur une arme, valeur sur une
//!                            armure (résistance en %), chance sur une armure
//!     7 Elemental Stone Bonus  bonus d'arme et d'armure
//!     8 Socket Slot Success  n-ième socket : chance (arme, armure, autre)
//!     9 Socket Slot Fail     n-ième socket, échec : sans perte, sockets vidés, remis à zéro, détruit
//!                            (arme, armure, autre)
//!   État d'un objet : niveau de renforcement, nombre de sockets, TID des pierres serties (8 au plus).
//! ```
//! Le renforcement d'une arme ajoute aux dégâts physiques min et max la somme des bonus d'arme des niveaux
//! atteints ; celui d'une armure ou d'un bouclier, à la défense physique. Une pierre d'esprit ajoute sa
//! valeur aux dégâts de son élément (arme) ou à la résistance de son élément (armure).

/// Nombre de niveaux de renforcement et de rangs de pierre.
pub const MAX_LEVEL: usize = 16;
pub const MAX_STONE_RANK: usize = 5;
/// Sockets au plus sur un objet.
pub const MAX_SOCKETS: usize = 8;

use super::items::EquipData;

/// Une ligne de la première section.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Level {
    pub weapon: i32,
    pub armor: i32,
    pub rank: String,
    /// Rangs de pierre admis (indice 1 à 5).
    pub stones: [bool; MAX_STONE_RANK + 1],
}

/// Un rang de pierre d'esprit.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stone {
    pub weapon_value: i32,
    pub weapon_rate: u32,
    pub armor_value: i32,
    pub armor_rate: u32,
}

/// Catégorie d'objet pour les sockets : arme, armure, autre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Weapon = 0,
    Armor = 1,
    Other = 2,
}

/// Issue d'un échec d'ajout de socket.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SocketFail {
    pub keep: u32,
    pub clear: u32,
    pub reset: u32,
    pub destroy: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConvertTable {
    /// Indice = niveau (0 inutilisé).
    pub levels: Vec<Level>,
    /// [rang de l'objet][niveau visé], en pour cent.
    pub success: Vec<Vec<u32>>,
    pub unchanged: Vec<Vec<u32>>,
    pub reset: Vec<Vec<u32>>,
    pub destroy: Vec<Vec<u32>>,
    /// Indice = rang de pierre (0 inutilisé).
    pub stones: [Stone; MAX_STONE_RANK + 1],
    pub stone_bonus: (i32, i32),
    /// Indice = numéro du socket ajouté (0 inutilisé) ; chance par catégorie.
    pub socket_success: Vec<[u32; 3]>,
    pub socket_fail: Vec<[SocketFail; 3]>,
}

fn num(cells: &[String], i: usize) -> i64 {
    cells.get(i).and_then(|c| c.trim().parse::<f64>().ok()).map(|v| v as i64).unwrap_or(0)
}

/// Sections du fichier, sans leur ligne d'en-tête.
fn sections(text: &str) -> Vec<Vec<Vec<String>>> {
    let mut out = Vec::new();
    let mut cur: Option<Vec<Vec<String>>> = None;
    for line in text.lines() {
        let cells: Vec<String> = line.trim_end_matches('\r').split('\t').map(str::to_string).collect();
        if cells.iter().all(|c| c.trim().is_empty()) {
            if let Some(s) = cur.take() {
                out.push(s);
            }
            continue;
        }
        match cur.as_mut() {
            None => cur = Some(Vec::new()),
            Some(s) => s.push(cells),
        }
    }
    out.extend(cur);
    out
}

impl ConvertTable {
    pub fn parse(text: &str) -> Self {
        let s = sections(text);
        let get = |i: usize| s.get(i).cloned().unwrap_or_default();
        let mut t = ConvertTable { levels: vec![Level::default(); MAX_LEVEL + 1], ..Default::default() };
        for r in get(0) {
            let lv = num(&r, 0) as usize;
            if (1..=MAX_LEVEL).contains(&lv) {
                let mut stones = [false; MAX_STONE_RANK + 1];
                for (k, ok) in stones.iter_mut().enumerate().skip(1) {
                    *ok = r.get(3 + k).is_some_and(|c| !c.trim().is_empty());
                }
                t.levels[lv] = Level { weapon: num(&r, 1) as i32, armor: num(&r, 2) as i32, rank: r.get(3).map(|c| c.trim().to_string()).unwrap_or_default(), stones };
            }
        }
        let grid = |rows: Vec<Vec<String>>| {
            let mut g = vec![vec![0u32; MAX_LEVEL + 1]; MAX_LEVEL + 1];
            for r in rows {
                let rank = num(&r, 0) as usize;
                if rank <= MAX_LEVEL {
                    for (lv, slot) in g[rank].iter_mut().enumerate().skip(1) {
                        *slot = num(&r, lv) as u32;
                    }
                }
            }
            g
        };
        t.success = grid(get(1));
        t.unchanged = grid(get(2));
        t.reset = grid(get(3));
        t.destroy = grid(get(4));
        for r in get(5) {
            let rank = num(&r, 0) as usize;
            if (1..=MAX_STONE_RANK).contains(&rank) {
                t.stones[rank] = Stone { weapon_value: num(&r, 1) as i32, weapon_rate: num(&r, 2) as u32, armor_value: num(&r, 3) as i32, armor_rate: num(&r, 4) as u32 };
            }
        }
        if let Some(r) = get(6).first() {
            t.stone_bonus = (num(r, 1) as i32, num(r, 2) as i32);
        }
        t.socket_success = vec![[0; 3]; MAX_SOCKETS + 1];
        for r in get(7) {
            let n = num(&r, 0) as usize;
            if (1..=MAX_SOCKETS).contains(&n) {
                t.socket_success[n] = [num(&r, 1) as u32, num(&r, 2) as u32, num(&r, 3) as u32];
            }
        }
        t.socket_fail = vec![[SocketFail::default(); 3]; MAX_SOCKETS + 1];
        for r in get(8) {
            let n = num(&r, 0) as usize;
            if (1..=MAX_SOCKETS).contains(&n) {
                for c in 0..3 {
                    let b = 1 + 4 * c;
                    t.socket_fail[n][c] = SocketFail { keep: num(&r, b) as u32, clear: num(&r, b + 1) as u32, reset: num(&r, b + 2) as u32, destroy: num(&r, b + 3) as u32 };
                }
            }
        }
        t
    }

    /// Bonus cumulé des niveaux 1 à `level` : (arme, armure).
    pub fn level_bonus(&self, level: u8) -> (i32, i32) {
        self.levels.iter().skip(1).take(level as usize).fold((0, 0), |(w, a), l| (w + l.weapon, a + l.armor))
    }
}

impl ConvertTable {
    /// Table importée dans `client_table` (`archcore import`) ; vide si elle manque.
    pub async fn load(db: &crate::db::Databases) -> anyhow::Result<Self> {
        let row = db.world.first("SELECT data FROM client_table WHERE name = 'ItemConvertTable.txt' AND row_idx = 0", ()).await?;
        let text = row.and_then(|r| serde_json::from_str::<String>(&crate::db::col::<String>(&r, "data")).ok()).unwrap_or_default();
        let t = Self::parse(&text);
        if text.is_empty() {
            crate::warn!("conversion : ItemConvertTable.txt absente de client_table (lancer archcore import)");
        } else {
            crate::info!("conversion : {} niveau(x) de renforcement, {} rang(s) de pierre", t.levels.iter().filter(|l| l.weapon > 0 || l.armor > 0).count(), t.stones.iter().filter(|s| s.weapon_value > 0).count());
        }
        Ok(t)
    }
}

/// État de conversion d'un objet (colonnes `convert_level`, `socket_count`, `socket_tids` de `character_items`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ItemConvert {
    pub level: u8,
    pub sockets: u8,
    /// TID de ce qui est serti, dans l'ordre (pierres d'esprit, runes).
    pub filled: Vec<u32>,
}

impl ItemConvert {
    pub fn is_plain(&self) -> bool {
        self.level == 0 && self.sockets == 0 && self.filled.is_empty()
    }

    /// Colonne `socket_tids` : TID séparés par des virgules.
    pub fn filled_column(&self) -> String {
        self.filled.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
    }

    pub fn from_columns(level: i64, sockets: i64, filled: &str) -> Self {
        ItemConvert {
            level: level.clamp(0, MAX_LEVEL as i64) as u8,
            sockets: sockets.clamp(0, MAX_SOCKETS as i64) as u8,
            filled: filled.split(',').filter_map(|t| t.trim().parse().ok()).take(MAX_SOCKETS).collect(),
        }
    }
}

/// « Kind » d'une armure ; type d'arme d'une lance (colonne « Type »), qui ne se convertit pas.
pub const KIND_ARMOR: i64 = 1;
pub const WEAPON_LANCE: i64 = 17;
/// Type de consommable (colonne « Type ») d'une potion de renforcement.
pub const USABLE_REINFORCEMENT: i64 = 8;
/// Parties du corps qu'une potion de renforcement accepte en plus des armes : corps et jambes.
const PHYSICAL_PARTS: [i64; 2] = [2, 6];
/// Sockets au plus par catégorie (arme, armure, autre), fixés dans le client.
pub const SOCKET_CAP: [u8; 3] = [8, 6, 5];

fn is_weapon(e: &EquipData) -> bool {
    e.kind == super::equipment::KIND_WEAPON && e.weapon_type != WEAPON_LANCE
}

/// Catégorie d'un équipement pour les sockets : arme (hors lance), armure, autre (bouclier, anneau,
/// collier, genre 64) ; `None` s'il n'en reçoit pas.
pub fn socket_category(e: &EquipData) -> Option<Category> {
    match e.kind {
        _ if is_weapon(e) => Some(Category::Weapon),
        KIND_ARMOR => Some(Category::Armor),
        4 | 8 | 16 | 64 => Some(Category::Other),
        _ => None,
    }
}

/// Sockets d'un objet : ceux enregistrés ou, pour un objet qui en reçoit et n'en a pas encore, max(1,
/// MinSocket), comme le client.
pub fn sockets_of(e: &EquipData, c: &ItemConvert) -> u8 {
    if c.sockets == 0 && socket_category(e).is_some() {
        e.min_socket.clamp(1, MAX_SOCKETS as i64) as u8
    } else {
        c.sockets
    }
}

/// Caractéristiques d'un équipement avec sa conversion. Renforcement : somme des « Weapon Increase » des
/// niveaux atteints aux dégâts physiques d'une arme, des « Armor Increase » à la défense physique du reste.
/// Pierres d'esprit (« Spirit Type » 1 à 5) : « Weapon Value Increase » de leur rang aux dégâts de leur
/// élément sur une arme, « Reinforcement Level% » à la résistance à leur élément sur une armure ; chaque
/// pierre en plus du même élément ajoute « Elemental Stone Bonus ». `stone` donne (type, rang) d'un TID serti.
pub fn effective(t: &ConvertTable, e: &EquipData, c: &ItemConvert, stone: impl Fn(u32) -> Option<(i64, i64)>) -> EquipData {
    let mut e = e.clone();
    let weapon = e.kind == super::equipment::KIND_WEAPON;
    let (w, a) = t.level_bonus(c.level);
    if weapon {
        e.min_damage += w;
        e.max_damage += w;
    } else {
        e.defense += a;
    }
    let mut count = [0i32; 8];
    for tid in &c.filled {
        let Some((kind, rank)) = stone(*tid) else { continue };
        let (Ok(i @ 1..=5), Ok(r @ 1..=MAX_STONE_RANK)) = (usize::try_from(kind), usize::try_from(rank)) else { continue };
        let s = t.stones[r];
        let bonus = if count[i - 1] > 0 { if weapon { t.stone_bonus.0 } else { t.stone_bonus.1 } } else { 0 };
        count[i - 1] += 1;
        if weapon {
            e.spirit_damage[i - 1].0 += s.weapon_value + bonus;
            e.spirit_damage[i - 1].1 += s.weapon_value + bonus;
        } else {
            e.spirit_defense[i - 1] += s.armor_value + bonus;
        }
    }
    e
}

/// Opérations d'AGPMITEMCONVERT.
pub mod op {
    pub const ADD: i64 = 0;
    pub const REQUEST_PHYSICAL: i64 = 1;
    pub const REQUEST_SOCKET: i64 = 2;
    pub const REQUEST_SPIRIT_STONE: i64 = 4;
    pub const RESPONSE_PHYSICAL: i64 = 5;
    pub const RESPONSE_SOCKET: i64 = 6;
    pub const RESPONSE_SPIRIT_STONE: i64 = 7;
}

/// AGPMITEMCONVERT de l'état d'un objet : op 0 (ajout) ou une réponse, avec son résultat et le TID ajouté.
pub fn state_packet(op: i64, owner: u32, iid: u32, c: &ItemConvert, result: Option<i64>, added: Option<u32>) -> Vec<u8> {
    use archcore_net::schema::{self, mini};
    use archcore_net::{build_mini, build_packet, Fields};
    let s = &schema::ITEM_CONVERT;
    let mut f = Fields::new(s).with(0, op).with(1, owner).with(2, iid).with(5, c.level as i64).with(6, c.sockets as i64).with(7, c.filled.len() as i64);
    if let Some(r) = result {
        f.set(4, r);
    }
    if let Some(t) = added {
        f.set(8, t);
    }
    if !c.filled.is_empty() {
        let m = &mini::ITEM_CONVERT_TIDS;
        let mut tids = Fields::new(m);
        for (i, t) in c.filled.iter().take(MAX_SOCKETS).enumerate() {
            tids.set(i, *t);
        }
        f.set(9, build_mini(m, &tids));
    }
    build_packet(s, &f)
}

/// Résultats d'un sertissage de pierre d'esprit (op 7) : ici, 0 est la réussite.
pub mod stone_result {
    pub const SUCCESS: i64 = 0;
    pub const FAILED: i64 = 1;
    pub const INVALID_STONE: i64 = 2;
    pub const FULL: i64 = 3;
    pub const IMPROPER_ITEM: i64 = 5;
}

/// Résultats d'un ajout de socket (op 6) et d'un renforcement (op 5).
pub mod result {
    pub const SUCCESS: i64 = 1;
    pub const FULL: i64 = 2;
    pub const IMPROPER_ITEM: i64 = 3;
    pub const KEEP: i64 = 5;
    /// Socket : sertissages perdus ; renforcement : retour à +0.
    pub const INIT_SAME: i64 = 6;
    /// Socket : objet remis à son état d'origine.
    pub const INIT: i64 = 7;
    pub const DESTROY: i64 = 8;
    /// Socket : objet qui n'en reçoit pas ; renforcement : mauvais catalyseur.
    pub const IMPROPER: i64 = 9;
}

/// Rangs de pierre qu'accepte un objet : ligne `PhysicalRank` de la première section (la dernière au-delà).
pub fn stone_rank_allowed(t: &ConvertTable, physical_rank: i64, stone_rank: usize) -> bool {
    let row = (physical_rank.max(1) as usize).min(MAX_LEVEL);
    t.levels.get(row).is_some_and(|l| l.stones.get(stone_rank).copied().unwrap_or(false))
}

/// Vérifie qu'une pierre peut être sertie, dans l'ordre du client ; rend son rang, ou le refus.
pub fn check_stone(t: &ConvertTable, e: &EquipData, c: &ItemConvert, stone: (i64, i64)) -> Result<usize, i64> {
    let (kind, rank) = stone;
    let Ok(rank @ 1..=MAX_STONE_RANK) = usize::try_from(rank) else { return Err(stone_result::INVALID_STONE) };
    if !(1..=5).contains(&kind) {
        return Err(stone_result::INVALID_STONE);
    }
    if !is_weapon(e) && e.kind != KIND_ARMOR {
        return Err(stone_result::IMPROPER_ITEM);
    }
    let sockets = sockets_of(e, c) as usize;
    if sockets == 0 {
        return Err(stone_result::IMPROPER_ITEM);
    }
    if c.filled.len() >= sockets {
        return Err(stone_result::FULL);
    }
    if !stone_rank_allowed(t, e.physical_rank, rank) {
        return Err(stone_result::INVALID_STONE);
    }
    Ok(rank)
}

/// Tirage d'un ajout de socket (le n-ième, `n` = sockets actuels + 1) : réussite selon « Socket Slot
/// Success », sinon un second tirage dans « Socket Slot Fail » (sans perte, sertissages perdus, objet remis
/// à zéro, détruit). `roll` donne deux nombres de 0 à 99.
pub fn socket_roll(t: &ConvertTable, cat: Category, n: usize, mut roll: impl FnMut() -> u32) -> i64 {
    let c = cat as usize;
    if roll() < t.socket_success.get(n).map(|r| r[c]).unwrap_or(0) {
        return result::SUCCESS;
    }
    let f = t.socket_fail.get(n).map(|r| r[c]).unwrap_or_default();
    let r = roll();
    let mut acc = f.keep;
    for (part, code) in [(0, result::KEEP), (f.clear, result::INIT_SAME), (f.reset, result::INIT), (f.destroy, result::DESTROY)] {
        acc += part;
        if r < acc {
            return code;
        }
    }
    result::KEEP
}

/// Vérifie qu'un objet peut être renforcé ; rend la ligne de chances (`PhysicalRank`), ou le refus.
pub fn check_physical(e: &EquipData, c: &ItemConvert) -> Result<usize, i64> {
    if !is_weapon(e) && !(e.kind == KIND_ARMOR && PHYSICAL_PARTS.contains(&e.part)) {
        return Err(result::IMPROPER_ITEM);
    }
    if e.physical_rank < 1 {
        return Err(result::IMPROPER_ITEM);
    }
    let rank = (e.physical_rank as usize).min(MAX_LEVEL);
    if c.level as usize >= rank {
        return Err(result::FULL);
    }
    Ok(rank)
}

/// Tirage d'un renforcement : ligne `rank`, colonne niveau visé. Réussite selon « Success », sinon un second
/// tirage : « Unchanged », « Reset Reinforcement » (retour à +0), « Destroy ».
pub fn physical_roll(t: &ConvertTable, rank: usize, next: usize, mut roll: impl FnMut() -> u32) -> i64 {
    let at = |g: &Vec<Vec<u32>>| g.get(rank).and_then(|r| r.get(next)).copied().unwrap_or(0);
    if roll() < at(&t.success) {
        return result::SUCCESS;
    }
    let r = roll();
    let keep = at(&t.unchanged);
    let reset = keep + at(&t.reset);
    if r < keep {
        result::KEEP
    } else if r < reset {
        result::INIT_SAME
    } else if r < reset + at(&t.destroy) {
        result::DESTROY
    } else {
        result::KEEP
    }
}

/// AGPMITEMCONVERT reçu du client.
pub async fn on_packet(ctx: &super::WorldCtx, st: &mut super::WorldState, sid: u32, p: &archcore_net::Packet) -> anyhow::Result<()> {
    let (iid, other) = (p.int(2).unwrap_or(0) as u32, p.int(3).unwrap_or(0) as u32);
    match p.int(0) {
        Some(op::REQUEST_SPIRIT_STONE) => spirit_stone(ctx, st, sid, iid, other).await,
        Some(op::REQUEST_SOCKET) => add_socket(ctx, st, sid, iid).await,
        Some(op::REQUEST_PHYSICAL) => physical(ctx, st, sid, iid, other).await,
        other => {
            crate::debug!("[conversion] #{sid} AGPMITEMCONVERT op {other:?} (ignoré)");
            Ok(())
        }
    }
}

/// AGPMEVENTITEMCONVERT reçu : op 0 demande la fenêtre de conversion d'un forgeron (fonction 22), op 1
/// l'accorde.
pub fn on_event(ctx: &super::WorldCtx, st: &super::WorldState, sid: u32, p: &archcore_net::Packet) {
    use super::npc_events::{npc_distance, resolve, MAX_NPC_DISTANCE};
    use archcore_net::schema::{self, mini};
    use archcore_net::{build_mini, decode_mini, Fields};
    if p.int(0) != Some(0) {
        return;
    }
    let Some(player) = st.players.get(&sid) else { return };
    let base = p.bytes(1).and_then(|b| decode_mini(&mini::EVENT_BASE, b).ok()).unwrap_or_else(|| Fields::new(&mini::EVENT_BASE));
    let Some((npc, _)) = resolve(&ctx.npcs, &base) else { return };
    if !npc.events.iter().any(|e| e.function == super::npcs::event_function::ITEM_CONVERT) || npc_distance(npc, player.ch.pos_x, player.ch.pos_z) > MAX_NPC_DISTANCE {
        crate::warn!("[conversion] #{sid} {} : conversion refusée auprès de {}", player.ch.name, npc.name);
        return;
    }
    let s = &schema::EVENT_ITEM_CONVERT;
    player.send(s, Fields::new(s).with(0, 1).with(1, build_mini(&mini::EVENT_BASE, &base)).with(2, player.ch.id));
}

/// Un forgeron (fonction 22) est à portée.
fn near_blacksmith(ctx: &super::WorldCtx, p: &super::Player) -> bool {
    let f = super::npcs::event_function::ITEM_CONVERT;
    ctx.npcs.near(p.ch.pos(), super::npc_events::MAX_NPC_DISTANCE * 2.0).any(|n| n.events.iter().any(|e| e.function == f))
}

/// Enregistre l'état de conversion d'un objet de l'inventaire.
async fn save(ctx: &super::WorldCtx, p: &mut super::Player, iid: u32, conv: &ItemConvert) -> anyhow::Result<()> {
    ctx.db
        .characters
        .exec("UPDATE character_items SET convert_level = ?, socket_count = ?, socket_tids = ? WHERE id = ?", (conv.level, conv.sockets, conv.filled_column(), iid))
        .await?;
    if let Some(i) = p.inventory.items.get_mut(&iid) {
        i.conv = conv.clone();
    }
    Ok(())
}

/// Consomme un exemplaire de `iid` (pierre, potion) et le signale au client.
async fn consume_one(ctx: &super::WorldCtx, p: &mut super::Player, iid: u32) -> anyhow::Result<()> {
    use super::items::{item_count, item_remove};
    let cid = p.ch.id;
    match p.inventory.consume(&ctx.db, iid, 1).await? {
        Some(left) => p.send_raw(item_count(&left, cid)),
        None => p.send_raw(item_remove(iid, Some(cid))),
    }
    Ok(())
}

/// Sertit la pierre `stone_iid` dans l'objet `iid` de l'inventaire. La pierre est consommée, que le
/// sertissage réussisse ou non (chance « Weapon Probability » ou « Armor Probability » de son rang).
async fn spirit_stone(ctx: &super::WorldCtx, st: &mut super::WorldState, sid: u32, iid: u32, stone_iid: u32) -> anyhow::Result<()> {
    use rand::Rng;
    let Some(p) = st.players.get_mut(&sid) else { return Ok(()) };
    let cid = p.ch.id;
    let (Some(item), Some(stone)) = (p.inventory.get(iid).cloned(), p.inventory.get(stone_iid).cloned()) else { return Ok(()) };
    let Some(e) = ctx.catalog.get(item.tid).and_then(|d| d.equip.as_ref()) else { return Ok(()) };
    let checked = ctx.catalog.get(stone.tid).and_then(|d| d.stone).ok_or(stone_result::INVALID_STONE).and_then(|s| check_stone(&ctx.convert, e, &item.conv, s));
    let rank = match checked {
        Ok(r) => r,
        Err(why) => {
            p.send_raw(state_packet(op::RESPONSE_SPIRIT_STONE, cid, iid, &item.conv, Some(why), Some(stone.tid)));
            return Ok(());
        }
    };
    let s = ctx.convert.stones[rank];
    let rate = if is_weapon(e) { s.weapon_rate } else { s.armor_rate };
    let ok = rand::thread_rng().gen_range(0..100) < rate;
    let mut conv = item.conv.clone();
    if ok {
        conv.sockets = sockets_of(e, &conv);
        conv.filled.push(stone.tid);
        save(ctx, p, iid, &conv).await?;
    }
    let result = if ok { stone_result::SUCCESS } else { stone_result::FAILED };
    p.send_raw(state_packet(op::RESPONSE_SPIRIT_STONE, cid, iid, &conv, Some(result), Some(stone.tid)));
    consume_one(ctx, p, stone_iid).await?;
    crate::info!("[conversion] {} sertit la pierre {} dans l'objet {iid} : {}", p.ch.name, stone.tid, if ok { "réussi" } else { "échec" });
    Ok(())
}

/// Ajoute un socket à l'objet `iid` de l'inventaire, chez un forgeron. Pas de prix : la table des coûts
/// du client est vide et il affiche 0.
async fn add_socket(ctx: &super::WorldCtx, st: &mut super::WorldState, sid: u32, iid: u32) -> anyhow::Result<()> {
    use rand::Rng;
    let Some(p) = st.players.get_mut(&sid) else { return Ok(()) };
    let cid = p.ch.id;
    let Some(item) = p.inventory.get(iid).cloned() else { return Ok(()) };
    if !near_blacksmith(ctx, p) {
        crate::warn!("[conversion] {} : ajout de socket loin d'un forgeron", p.ch.name);
        return Ok(());
    }
    let Some(e) = ctx.catalog.get(item.tid).and_then(|d| d.equip.as_ref()) else { return Ok(()) };
    let mut conv = item.conv.clone();
    conv.sockets = sockets_of(e, &conv);
    let Some(cat) = socket_category(e) else {
        p.send_raw(state_packet(op::RESPONSE_SOCKET, cid, iid, &conv, Some(result::IMPROPER), None));
        return Ok(());
    };
    if conv.sockets >= SOCKET_CAP[cat as usize] {
        p.send_raw(state_packet(op::RESPONSE_SOCKET, cid, iid, &conv, Some(result::FULL), None));
        return Ok(());
    }
    let outcome = {
        let mut rng = rand::thread_rng();
        socket_roll(&ctx.convert, cat, conv.sockets as usize + 1, || rng.gen_range(0..100))
    };
    match outcome {
        result::SUCCESS => conv.sockets += 1,
        result::INIT_SAME => conv.filled.clear(),
        result::INIT => conv = ItemConvert { level: conv.level, sockets: sockets_of(e, &ItemConvert::default()), filled: Vec::new() },
        _ => {}
    }
    p.send_raw(state_packet(op::RESPONSE_SOCKET, cid, iid, &conv, Some(outcome), None));
    if outcome == result::DESTROY {
        p.inventory.remove(&ctx.db, iid).await?;
        p.send_raw(super::items::item_remove(iid, Some(cid)));
    } else {
        save(ctx, p, iid, &conv).await?;
        if matches!(outcome, result::INIT_SAME | result::INIT) {
            // Le client ne relit pas les sertissages dans la réponse.
            p.send_raw(state_packet(op::ADD, cid, iid, &conv, None, None));
        }
    }
    crate::info!("[conversion] {} : socket sur l'objet {iid}, résultat {outcome} ({} socket(s))", p.ch.name, conv.sockets);
    Ok(())
}

/// Renforce l'objet `iid` de l'inventaire avec la potion `catalyst_iid`, consommée dans tous les cas.
async fn physical(ctx: &super::WorldCtx, st: &mut super::WorldState, sid: u32, iid: u32, catalyst_iid: u32) -> anyhow::Result<()> {
    use rand::Rng;
    let Some(p) = st.players.get_mut(&sid) else { return Ok(()) };
    let cid = p.ch.id;
    let (Some(item), Some(potion)) = (p.inventory.get(iid).cloned(), p.inventory.get(catalyst_iid).cloned()) else { return Ok(()) };
    if !near_blacksmith(ctx, p) {
        crate::warn!("[conversion] {} : renforcement loin d'un forgeron", p.ch.name);
        return Ok(());
    }
    let Some(e) = ctx.catalog.get(item.tid).and_then(|d| d.equip.as_ref()) else { return Ok(()) };
    if ctx.catalog.get(potion.tid).map(|d| d.usable_type) != Some(USABLE_REINFORCEMENT) {
        p.send_raw(state_packet(op::RESPONSE_PHYSICAL, cid, iid, &item.conv, Some(result::IMPROPER), None));
        return Ok(());
    }
    let rank = match check_physical(e, &item.conv) {
        Ok(r) => r,
        Err(why) => {
            p.send_raw(state_packet(op::RESPONSE_PHYSICAL, cid, iid, &item.conv, Some(why), None));
            return Ok(());
        }
    };
    consume_one(ctx, p, catalyst_iid).await?;
    let outcome = {
        let mut rng = rand::thread_rng();
        physical_roll(&ctx.convert, rank, item.conv.level as usize + 1, || rng.gen_range(0..100))
    };
    let mut conv = item.conv.clone();
    match outcome {
        result::SUCCESS => conv.level += 1,
        result::INIT_SAME => conv.level = 0,
        _ => {}
    }
    p.send_raw(state_packet(op::RESPONSE_PHYSICAL, cid, iid, &conv, Some(outcome), None));
    if outcome == result::DESTROY {
        p.inventory.remove(&ctx.db, iid).await?;
        p.send_raw(super::items::item_remove(iid, Some(cid)));
    } else if conv != item.conv {
        save(ctx, p, iid, &conv).await?;
    }
    crate::info!("[conversion] {} : renforcement de l'objet {iid}, résultat {outcome} (+{})", p.ch.name, conv.level);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn weapon(rank: i64) -> EquipData {
        EquipData { kind: super::super::equipment::KIND_WEAPON, physical_rank: rank, min_socket: 1, min_damage: 10, max_damage: 12, ..Default::default() }
    }

    #[test]
    fn pierres_admises() {
        let t = ConvertTable::parse(TABLE);
        let open = ItemConvert { sockets: 2, ..Default::default() };
        assert_eq!(check_stone(&t, &weapon(3), &open, (2, 2)), Ok(2));
        assert_eq!(check_stone(&t, &weapon(3), &open, (2, 3)), Err(stone_result::INVALID_STONE), "rang refusé par la ligne 3");
        assert_eq!(check_stone(&t, &weapon(3), &open, (7, 1)), Err(stone_result::INVALID_STONE), "foudre : le client refuse");
        // Objet neuf : un socket (max(1, MinSocket)), puis plein.
        assert_eq!(check_stone(&t, &weapon(3), &ItemConvert::default(), (2, 1)), Ok(1));
        assert_eq!(check_stone(&t, &weapon(3), &ItemConvert { filled: vec![201], ..Default::default() }, (2, 1)), Err(stone_result::FULL));
        let ring = EquipData { kind: 8, physical_rank: 3, ..Default::default() };
        assert_eq!(check_stone(&t, &ring, &open, (2, 1)), Err(stone_result::IMPROPER_ITEM));
        let lance = EquipData { weapon_type: WEAPON_LANCE, ..weapon(3) };
        assert_eq!(check_stone(&t, &lance, &open, (2, 1)), Err(stone_result::IMPROPER_ITEM));
    }

    #[test]
    fn objet_converti() {
        let t = ConvertTable::parse(TABLE);
        // Niveau 3 (+7), deux pierres d'eau de rang 2 (+4, puis +4 et le bonus 3) et une pierre inconnue.
        let c = ItemConvert { level: 3, sockets: 3, filled: vec![201, 201, 999] };
        let stone = |tid: u32| (tid == 201).then_some((2, 2));
        let e = effective(&t, &weapon(3), &c, stone);
        assert_eq!((e.min_damage, e.max_damage, e.spirit_damage[1]), (17, 19, (11, 11)));
        let mail = EquipData { kind: KIND_ARMOR, defense: 5, ..Default::default() };
        let e = effective(&t, &mail, &c, stone);
        assert_eq!((e.defense, e.spirit_defense[1]), (11, 21));
    }

    #[test]
    fn sockets_et_renforcement() {
        let t = ConvertTable::parse(TABLE);
        let seq = |v: Vec<u32>| { let mut it = v.into_iter(); move || it.next().unwrap_or(99) };
        // Deuxième socket d'un « autre » : 90 % ; échec puis 50 % sans perte, 50 % sertissages perdus.
        assert_eq!(socket_roll(&t, Category::Other, 2, seq(vec![89])), result::SUCCESS);
        assert_eq!(socket_roll(&t, Category::Other, 2, seq(vec![90, 49])), result::KEEP);
        assert_eq!(socket_roll(&t, Category::Other, 2, seq(vec![90, 50])), result::INIT_SAME);
        assert_eq!(socket_category(&EquipData { kind: 32, ..Default::default() }), None);
        // Renforcement, ligne 3 : +3 à 50 % ; échec : 40 % sans perte, 7 % retour à +0, 3 % détruit.
        assert_eq!(check_physical(&weapon(3), &ItemConvert { level: 2, ..Default::default() }), Ok(3));
        assert_eq!(check_physical(&weapon(3), &ItemConvert { level: 3, ..Default::default() }), Err(result::FULL));
        let helm = EquipData { kind: KIND_ARMOR, part: 3, physical_rank: 9, ..Default::default() };
        assert_eq!(check_physical(&helm, &ItemConvert::default()), Err(result::IMPROPER_ITEM));
        assert_eq!(physical_roll(&t, 3, 3, seq(vec![49])), result::SUCCESS);
        assert_eq!(physical_roll(&t, 3, 3, seq(vec![50, 39])), result::KEEP);
        assert_eq!(physical_roll(&t, 3, 3, seq(vec![50, 46])), result::INIT_SAME);
        assert_eq!(physical_roll(&t, 3, 3, seq(vec![50, 49])), result::DESTROY);
    }

    const TABLE: &str = "Reinforcement Level\tWeapon Increase\tArmor Increase\tRank\tSpirit1\tSpirit2\tSpirit3\tSpirit4\tSpirit5\r\n\
1\t2\t2\t\tO\t\t\t\t\r\n\
2\t2\t2\t\tO\t\t\t\t\r\n\
3\t3\t2\tL\tO\tO\t\t\t\r\n\
\t\t\t\t\t\t\t\t\r\n\
Success\t1\t2\t3\r\n\
3\t100\t90\t50\r\n\
\t\t\t\r\n\
Unchanged\t1\t2\t3\r\n\
3\t100\t100\t40\r\n\
\t\t\t\r\n\
Reset Reinforcement\t1\t2\t3\r\n\
3\t0\t0\t7\r\n\
\t\t\t\r\n\
Destroy\t1\t2\t3\r\n\
3\t0\t0\t3\r\n\
\t\t\t\r\n\
Elemental Stone Level\tWeapon Value Increase\tWeapon Probability\tReinforcement Level%\tArmor Probability\r\n\
1\t3\t100\t7\t100\r\n\
2\t4\t70\t9\t70\r\n\
\t\t\t\t\r\n\
Elemental Stone Bonus\tWeapon Bonus\tArmor Bonus%\r\n\
\t3\t3\r\n\
\t\t\r\n\
Socket Slot Success\tWeapon Chance\tArmor Chance\tOther Chance\r\n\
1\t100\t100\t100\r\n\
2\t90\t90\t90\r\n\
\t\t\t\r\n\
Socket Slot Fail\tUnchanged (Weapon)\tReset Sockets (Weapon)\tReset (Weapon)\tDestroy (Weapon)\tUnchanged (Armor)\tReset Sockets (Armor)\tReset (Armor)\tDestroy (Armor)\tUnchanged (Other)\tReset Sockets (Other)\r\n\
2\t95\t5\t0\t0\t95\t5\t0\t0\t50\t50\t0\t0\r\n";

    #[test]
    fn table_de_conversion() {
        let t = ConvertTable::parse(TABLE);
        assert_eq!((t.levels[3].weapon, t.levels[3].armor, t.levels[3].rank.as_str()), (3, 2, "L"));
        assert_eq!(t.levels[3].stones, [false, true, true, false, false, false]);
        assert_eq!(t.level_bonus(3), (7, 6));
        assert_eq!((t.success[3][2], t.unchanged[3][3], t.reset[3][3], t.destroy[3][3]), (90, 40, 7, 3));
        assert_eq!(t.stones[2], Stone { weapon_value: 4, weapon_rate: 70, armor_value: 9, armor_rate: 70 });
        assert_eq!(t.stone_bonus, (3, 3));
        assert_eq!(t.socket_success[2], [90, 90, 90]);
        assert_eq!(t.socket_fail[2][2], SocketFail { keep: 50, clear: 50, reset: 0, destroy: 0 });
    }

    #[test]
    fn colonnes_de_l_objet() {
        let c = ItemConvert::from_columns(5, 3, "201, 203");
        assert_eq!(c, ItemConvert { level: 5, sockets: 3, filled: vec![201, 203] });
        assert_eq!(c.filled_column(), "201,203");
        assert!(ItemConvert::default().is_plain());
    }
}
