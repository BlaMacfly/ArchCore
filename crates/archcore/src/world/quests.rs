//! Quêtes : catalogue (questtemplate.ini, questgroup.ini), journal des personnages, acceptation,
//! progression, fin et récompenses.
//!
//! ```text
//!   AGPMEVENTQUEST (0x3E) : client → op 0 (événement du PNJ, fonction 27) ← op 0 : fenêtre des quêtes.
//!   AGPMQUEST (0x13), relevé dans le client x64 :
//!     client → op 0 accepter (champ 2 = PNJ, absent pour une quête « personnelle », champ 4 = TID)
//!            ← op 1 (champ 4 = TID, champ 6 = 1 réussi / 0 refusé) ; le client l'inscrit au journal
//!     client → op 2 (ou 15) terminer (champ 4 = TID, champ 7 = récompense choisie)
//!            ← op 3 (champ 4, champ 6) ; récompenses (expérience, argent, objets)
//!     client → op 6 abandonner (champ 4) ← op 7 (champ 4, champ 6)
//!     ← op 10 progression : mini QUEST_INFO (TID, monstres A et B tués, objectifs remplis) ; le client
//!       affiche « nom n/total »
//!     ← à l'entrée en jeu : op 5 (drapeaux des quêtes terminées, champs 8 à 10 : 3 × 0x800 octets, bit
//!       TID % 16 384 du bloc TID / 16 384, octet bit / 8, bit de poids faible d'abord), puis op 4 (mini
//!       QUEST_INFO) pour chaque quête en cours
//! ```
//! Une quête se prend auprès d'un PNJ dont le groupe (`QuestGroupID` de npc.ini → questgroup.ini) la
//! propose, ou depuis le journal si elle est « personnelle » (`Self_Quest`) ; elle se rend de même.

use super::items::{item_add, item_count, item_remove, item_status};
use super::npc_events::{npc_distance, resolve, MAX_NPC_DISTANCE};
use super::npcs::event_function;
use super::{send_system, WorldCtx, WorldState};
use crate::db::{col, Databases};
use crate::{info, warn};
use archcore_net::schema::{self, mini};
use archcore_net::{build_mini, decode_mini, Fields, Packet};
use rand::Rng;
use std::collections::{BTreeSet, HashMap};

/// Quêtes en cours au plus (taille du journal du client).
pub const MAX_ACTIVE: usize = 60;
/// Taille d'un bloc de drapeaux et TID couverts par bloc.
const FLAG_BLOCK: usize = 0x800;
const TIDS_PER_BLOCK: u32 = 0x4000;

pub mod op {
    pub const ACCEPT: i64 = 0;
    pub const ACCEPT_RESULT: i64 = 1;
    pub const COMPLETE: i64 = 2;
    pub const COMPLETE_RESULT: i64 = 3;
    pub const ADD: i64 = 4;
    pub const FLAGS: i64 = 5;
    pub const CANCEL: i64 = 6;
    pub const CANCEL_RESULT: i64 = 7;
    pub const PROGRESS: i64 = 10;
    pub const COMPLETE_SELECT: i64 = 15;
}

/// Monstres à tuer.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hunt {
    pub monster: u32,
    pub count: i64,
}

/// Objet laissé par un monstre de la quête : pour cent de chances, nombre à réunir.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct QuestDrop {
    pub monster: u32,
    pub item: u32,
    pub rate: i64,
    pub count: i64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct QuestTemplate {
    pub tid: u32,
    pub name: String,
    pub level_min: i32,
    pub level_max: i32,
    pub races: Vec<i64>,
    pub classes: Vec<i64>,
    pub previous: Option<u32>,
    pub self_quest: bool,
    pub complete_level: i32,
    /// Objets à rapporter (pris à la fin).
    pub item: Option<(u32, i64)>,
    pub hunts: [Option<Hunt>; 2],
    pub drops: Vec<QuestDrop>,
    pub gifts: Vec<(u32, i64)>,
    /// Récompenses au choix.
    pub choices: Vec<(u32, i64)>,
    pub gift_money: i64,
    pub gift_exp: u64,
}

fn text<'a>(d: &'a serde_json::Value, k: &str) -> &'a str {
    d.get(k).and_then(|v| v.as_str()).unwrap_or("").trim()
}

/// Premier entier d'une cellule (« 5007;1 » → 5007).
fn int(d: &serde_json::Value, k: &str) -> i64 {
    text(d, k).split(';').next().and_then(|s| s.trim().parse::<i64>().ok()).unwrap_or(0)
}

fn list(d: &serde_json::Value, k: &str) -> Vec<i64> {
    text(d, k).split(';').filter_map(|s| s.trim().parse::<i64>().ok()).filter(|x| *x != 0).collect()
}

impl QuestTemplate {
    pub fn from_data(tid: u32, name: &str, d: &serde_json::Value) -> Self {
        let pair = |t: &str, c: &str| (int(d, t) > 0).then(|| (int(d, t) as u32, int(d, c).max(1)));
        let hunt = |k: &str| (int(d, &format!("Complete_Monster{k}_Tid")) > 0).then(|| Hunt { monster: int(d, &format!("Complete_Monster{k}_Tid")) as u32, count: int(d, &format!("Complete_Monster{k}_Count")) });
        let drops = ["A", "B"]
            .iter()
            .filter_map(|k| {
                let item = int(d, &format!("DropItem{k}_TID"));
                let monster = int(d, &format!("Complete_Monster{k}_Tid"));
                (item > 0 && monster > 0).then(|| QuestDrop {
                    monster: monster as u32,
                    item: item as u32,
                    rate: match int(d, &format!("DropItem{k}_Property")) {
                        0 => 100,
                        r => r,
                    },
                    count: int(d, &format!("DropItem{k}_Count")).max(1),
                })
            })
            .collect();
        QuestTemplate {
            tid,
            name: name.to_string(),
            level_min: int(d, "Need_Level_Min") as i32,
            level_max: int(d, "Need_Level_Max") as i32,
            races: list(d, "Need_Race"),
            classes: list(d, "Need_Class"),
            previous: (int(d, "Need_PreviousQuestTid") > 0).then(|| int(d, "Need_PreviousQuestTid") as u32),
            self_quest: int(d, "Self_Quest") != 0,
            complete_level: int(d, "Complete_Level") as i32,
            item: pair("Complete_ItemTid", "Complete_ItemCount"),
            hunts: [hunt("A"), hunt("B")],
            drops,
            gifts: ["A", "B", "C", "D"].iter().filter_map(|k| pair(&format!("Gift_Item{k}_Tid"), &format!("Gift_Item{k}_Count"))).collect(),
            choices: (1..=8).filter_map(|i| pair(&format!("SelectItem{i}_Tid"), &format!("SelectItem{i}_Count"))).collect(),
            gift_money: int(d, "Gift_Money"),
            gift_exp: int(d, "Gift_Exp").max(0) as u64,
        }
    }

    /// Raison d'un refus d'acceptation, sinon `None`.
    pub fn refusal(&self, level: i32, race: i64, class: i64, quests: &PlayerQuests) -> Option<&'static str> {
        if quests.done.contains(&self.tid) {
            return Some("quête déjà terminée");
        }
        if quests.active(self.tid).is_some() {
            return Some("quête déjà en cours");
        }
        if quests.active.len() >= MAX_ACTIVE {
            return Some("journal plein");
        }
        if (self.level_min > 0 && level < self.level_min) || (self.level_max > 0 && level > self.level_max) {
            return Some("niveau");
        }
        if (!self.races.is_empty() && !self.races.contains(&race)) || (!self.classes.is_empty() && !self.classes.contains(&class)) {
            return Some("race ou classe");
        }
        if self.previous.is_some_and(|p| !quests.done.contains(&p)) {
            return Some("quête précédente non terminée");
        }
        None
    }

    /// Objectifs remplis : monstres tués et objets réunis (`held` = nombre de l'objet demandé).
    pub fn objectives_met(&self, a: &ActiveQuest, held: i64) -> bool {
        let killed = [a.count_a, a.count_b];
        self.hunts.iter().zip(killed).all(|(h, k)| h.is_none_or(|h| k >= h.count)) && self.item.is_none_or(|(_, n)| held >= n)
    }
}

/// Quêtes et groupes de PNJ.
#[derive(Default)]
pub struct QuestCatalog {
    quests: HashMap<u32, QuestTemplate>,
    /// Groupe → (quêtes proposées, quêtes terminées auprès de lui).
    groups: HashMap<u32, (Vec<u32>, Vec<u32>)>,
}

impl QuestCatalog {
    pub async fn load(db: &Databases) -> anyhow::Result<Self> {
        let Ok(rows) = db.world.rows("SELECT tid, name, data FROM quest_template", ()).await else {
            warn!("quêtes non chargées : lancer `archcore db-init` puis `archcore import`");
            return Ok(QuestCatalog::default());
        };
        let quests: HashMap<u32, QuestTemplate> = rows
            .iter()
            .map(|r| {
                let tid: u32 = col(r, "tid");
                let data = serde_json::from_str::<serde_json::Value>(&col::<String>(r, "data")).unwrap_or_default();
                (tid, QuestTemplate::from_data(tid, &col::<String>(r, "name"), &data))
            })
            .collect();
        let mut groups = HashMap::new();
        for r in db.world.rows("SELECT id, give, confirm FROM quest_group", ()).await.unwrap_or_default() {
            let parse = |k: &str| serde_json::from_str::<Vec<u32>>(&col::<String>(&r, k)).unwrap_or_default();
            groups.insert(col::<u32>(&r, "id"), (parse("give"), parse("confirm")));
        }
        info!("quêtes : {} ({} groupe(s) de PNJ)", quests.len(), groups.len());
        Ok(QuestCatalog { quests, groups })
    }

    pub fn get(&self, tid: u32) -> Option<&QuestTemplate> {
        self.quests.get(&tid)
    }

    pub fn group(&self, id: u32) -> Option<&(Vec<u32>, Vec<u32>)> {
        self.groups.get(&id)
    }

}

/// Quête en cours.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActiveQuest {
    pub tid: u32,
    pub count_a: i64,
    pub count_b: i64,
}

/// Journal d'un personnage.
#[derive(Clone, Debug, Default)]
pub struct PlayerQuests {
    pub active: Vec<ActiveQuest>,
    pub done: BTreeSet<u32>,
}

impl PlayerQuests {
    pub async fn load(db: &Databases, character_id: u32) -> anyhow::Result<Self> {
        let mut q = PlayerQuests::default();
        for r in db.characters.rows("SELECT quest_tid, status, count_a, count_b FROM character_quests WHERE character_id = ?", (character_id,)).await? {
            let tid: u32 = col(&r, "quest_tid");
            if col::<i64>(&r, "status") == 1 {
                q.done.insert(tid);
            } else {
                q.active.push(ActiveQuest { tid, count_a: col(&r, "count_a"), count_b: col(&r, "count_b") });
            }
        }
        Ok(q)
    }

    pub fn active(&self, tid: u32) -> Option<&ActiveQuest> {
        self.active.iter().find(|a| a.tid == tid)
    }

    /// Les trois blocs de drapeaux des quêtes terminées.
    pub fn flag_blocks(&self) -> [Vec<u8>; 3] {
        let mut blocks = [vec![0u8; FLAG_BLOCK], vec![0u8; FLAG_BLOCK], vec![0u8; FLAG_BLOCK]];
        for tid in &self.done {
            let (block, bit) = ((tid / TIDS_PER_BLOCK) as usize, (tid % TIDS_PER_BLOCK) as usize);
            if *tid > 0 && block < 3 {
                blocks[block][bit / 8] |= 1 << (bit % 8);
            }
        }
        blocks
    }
}

/// Mini QUEST_INFO.
pub fn info_mini(a: &ActiveQuest, done: bool) -> Vec<u8> {
    let s = &mini::QUEST_INFO;
    build_mini(s, &Fields::new(s).with(0, a.tid).with(1, a.count_a).with(2, a.count_b).with(3, done as i64).with(4, 0))
}

fn quest(op: i64, cid: u32) -> Fields {
    let s = &schema::QUEST;
    Fields::new(s).with(0, op).with(1, cid)
}

/// Paquets de l'entrée en jeu : drapeaux des quêtes terminées, puis chaque quête en cours.
pub fn login_packets(cid: u32, q: &PlayerQuests) -> Vec<Vec<u8>> {
    let s = &schema::QUEST;
    let [a, b, c] = q.flag_blocks();
    let mut out = vec![archcore_net::build_packet(s, &quest(op::FLAGS, cid).with(8, a).with(9, b).with(10, c))];
    for aq in &q.active {
        out.push(archcore_net::build_packet(s, &quest(op::ADD, cid).with(3, info_mini(aq, false)).with(4, aq.tid)));
    }
    out
}

/// Nombre d'exemplaires d'un objet dans l'inventaire.
fn held(p: &super::Player, tid: u32) -> i64 {
    p.inventory.items.values().filter(|i| i.tid == tid).map(|i| i.count).sum()
}

// ---------------------------------------------------------------------------------------------
// Fenêtre auprès d'un PNJ
// ---------------------------------------------------------------------------------------------

/// Groupe de quêtes du PNJ désigné par un mini EVENT_BASE, s'il est à portée.
fn npc_group(ctx: &WorldCtx, st: &WorldState, sid: u32, event: Option<&[u8]>) -> Option<u32> {
    let base = decode_mini(&mini::EVENT_BASE, event?).ok()?;
    let (npc, _) = resolve(&ctx.npcs, &base)?;
    let ev = npc.events.iter().find(|e| e.function == event_function::QUEST)?;
    let p = st.players.get(&sid)?;
    let pos = p.dest.unwrap_or_else(|| p.ch.pos());
    (npc_distance(npc, pos.x as f64, pos.z as f64) <= MAX_NPC_DISTANCE * 2.0).then_some(())?;
    ev.param("QuestGroupID").and_then(|g| g.trim().parse::<u32>().ok())
}

/// AGPMEVENTQUEST reçu : fenêtre des quêtes d'un PNJ.
pub fn on_event(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) {
    if p.int(0) != Some(0) {
        return;
    }
    let Some(group) = npc_group(ctx, st, sid, p.bytes(1)) else {
        warn!("[quête] #{sid} fenêtre de quêtes refusée (PNJ inconnu, sans quêtes ou trop loin)");
        return;
    };
    let player = &st.players[&sid];
    info!("[quête] {} ouvre les quêtes du groupe {group}", player.ch.name);
    let s = &schema::EVENT_QUEST;
    player.send(s, Fields::new(s).with(0, 0).with(1, p.bytes(1).map(|b| build_mini(&mini::EVENT_BASE, &decode_mini(&mini::EVENT_BASE, b).unwrap_or_else(|_| Fields::new(&mini::EVENT_BASE)))).unwrap_or_default()).with(2, player.ch.id));
}

// ---------------------------------------------------------------------------------------------
// Acceptation, abandon, fin
// ---------------------------------------------------------------------------------------------

/// AGPMQUEST reçu.
pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    if !st.players.get(&sid).is_some_and(|p| p.in_game && !p.dead) {
        return Ok(());
    }
    let tid = p.int(4).unwrap_or(0) as u32;
    match p.int(0).unwrap_or(-1) {
        op::ACCEPT => accept(ctx, st, sid, tid, p.bytes(2)).await?,
        op::COMPLETE | op::COMPLETE_SELECT => complete(ctx, st, sid, tid, p.bytes(2), p.int(7).unwrap_or(0)).await?,
        op::CANCEL => cancel(ctx, st, sid, tid).await?,
        other => warn!("[quête] #{sid} op {other} non géré"),
    }
    Ok(())
}

/// Le PNJ (ou le journal, pour une quête personnelle) permet de prendre (`give`) ou de rendre la quête.
fn place_allows(ctx: &WorldCtx, st: &WorldState, sid: u32, q: &QuestTemplate, event: Option<&[u8]>, give: bool) -> bool {
    match npc_group(ctx, st, sid, event).and_then(|g| ctx.quests.group(g)) {
        Some((offered, confirmed)) => (if give { offered } else { confirmed }).contains(&q.tid) || q.self_quest,
        None => q.self_quest,
    }
}

fn reply(st: &WorldState, sid: u32, op: i64, tid: u32, ok: bool) {
    let p = &st.players[&sid];
    p.send(&schema::QUEST, quest(op, p.ch.id).with(4, tid).with(6, ok as i64));
}

async fn accept(ctx: &WorldCtx, st: &mut WorldState, sid: u32, tid: u32, event: Option<&[u8]>) -> anyhow::Result<()> {
    let Some(q) = ctx.quests.get(tid) else {
        reply(st, sid, op::ACCEPT_RESULT, tid, false);
        return Ok(());
    };
    let p = &st.players[&sid];
    let why = q.refusal(p.ch.level, p.factors.ftype[0] as i64, p.factors.ftype[2] as i64, &p.quests).or_else(|| (!place_allows(ctx, st, sid, q, event, true)).then_some("ce PNJ ne la propose pas"));
    if let Some(why) = why {
        info!("[quête] {} ne peut pas prendre « {} » : {why}", p.ch.name, q.name);
        send_system(st, sid, &format!("Quête « {} » impossible : {why}.", q.name));
        reply(st, sid, op::ACCEPT_RESULT, tid, false);
        return Ok(());
    }
    let character_id = p.ch.id;
    ctx.db
        .characters
        .exec("REPLACE INTO character_quests (character_id, quest_tid, status, count_a, count_b) VALUES (?, ?, 0, 0, 0)", (character_id, tid))
        .await?;
    let p = st.players.get_mut(&sid).unwrap();
    p.quests.active.push(ActiveQuest { tid, ..Default::default() });
    info!("[quête] {} prend « {} »", p.ch.name, q.name);
    let a = ActiveQuest { tid, ..Default::default() };
    p.send(&schema::QUEST, quest(op::ACCEPT_RESULT, character_id).with(3, info_mini(&a, false)).with(4, tid).with(6, 1));
    // Objets déjà réunis : la progression le dit tout de suite.
    send_progress(ctx, st, sid, tid);
    Ok(())
}

async fn cancel(ctx: &WorldCtx, st: &mut WorldState, sid: u32, tid: u32) -> anyhow::Result<()> {
    let p = st.players.get_mut(&sid).unwrap();
    let Some(i) = p.quests.active.iter().position(|a| a.tid == tid) else {
        reply(st, sid, op::CANCEL_RESULT, tid, false);
        return Ok(());
    };
    p.quests.active.remove(i);
    ctx.db.characters.exec("DELETE FROM character_quests WHERE character_id = ? AND quest_tid = ? AND status = 0", (p.ch.id, tid)).await?;
    info!("[quête] {} abandonne la quête {tid}", p.ch.name);
    reply(st, sid, op::CANCEL_RESULT, tid, true);
    Ok(())
}

async fn complete(ctx: &WorldCtx, st: &mut WorldState, sid: u32, tid: u32, event: Option<&[u8]>, choice: i64) -> anyhow::Result<()> {
    let p = &st.players[&sid];
    let (Some(q), Some(a)) = (ctx.quests.get(tid), p.quests.active(tid).copied()) else {
        reply(st, sid, op::COMPLETE_RESULT, tid, false);
        return Ok(());
    };
    let have = q.item.map(|(item, _)| held(p, item)).unwrap_or(0);
    let chosen = if q.choices.is_empty() { None } else { q.choices.get(if choice >= 1 { choice as usize - 1 } else { 0 }).copied() };
    let rewards: Vec<(u32, i64)> = q.gifts.iter().copied().chain(chosen).collect();
    let why = if !q.objectives_met(&a, have) {
        Some("objectifs non remplis")
    } else if q.complete_level > 0 && p.ch.level < q.complete_level {
        Some("niveau")
    } else if !place_allows(ctx, st, sid, q, event, false) {
        Some("rendez-la au bon PNJ")
    } else if p.inventory.free_count() < rewards.len() {
        Some("inventaire plein")
    } else {
        None
    };
    if let Some(why) = why {
        info!("[quête] {} ne peut pas terminer « {} » : {why}", p.ch.name, q.name);
        send_system(st, sid, &format!("Quête « {} » : {why}.", q.name));
        reply(st, sid, op::COMPLETE_RESULT, tid, false);
        return Ok(());
    }
    let (cid, name, money) = (p.ch.id, p.ch.name.clone(), p.ch.money as i64);
    // Objets rapportés, pris dans l'inventaire.
    if let Some((item, mut need)) = q.item {
        let stacks: Vec<u32> = st.players[&sid].inventory.items.values().filter(|i| i.tid == item).map(|i| i.iid).collect();
        for iid in stacks {
            if need <= 0 {
                break;
            }
            let p = st.players.get_mut(&sid).unwrap();
            let n = p.inventory.get(iid).map(|i| i.count).unwrap_or(0).min(need);
            need -= n;
            match p.inventory.consume(&ctx.db, iid, n).await? {
                Some(left) => p.send_raw(item_count(&left, cid)),
                None => p.send_raw(item_remove(iid, Some(cid))),
            }
        }
    }
    let p = st.players.get_mut(&sid).unwrap();
    p.quests.active.retain(|x| x.tid != tid);
    p.quests.done.insert(tid);
    ctx.db.characters.exec("UPDATE character_quests SET status = 1 WHERE character_id = ? AND quest_tid = ?", (cid, tid)).await?;
    reply(st, sid, op::COMPLETE_RESULT, tid, true);
    for (item, count) in rewards {
        let stack = ctx.catalog.get(item).map(|d| d.stack).unwrap_or(1);
        let p = st.players.get_mut(&sid).unwrap();
        match p.inventory.add(&ctx.db, item, count, stack, None).await? {
            Some((it, true)) => p.send_raw(item_add(it.iid, it.tid, Some(cid), it.count, item_status::INVENTORY, Some(it.grid))),
            Some((it, false)) => p.send_raw(item_count(&it, cid)),
            None => warn!("[quête] {name} : récompense {item} perdue (inventaire plein)"),
        }
    }
    if q.gift_money > 0 {
        super::set_money(ctx, st, sid, (money + q.gift_money).min(super::npc_trade::MAX_MONEY)).await?;
    }
    if q.gift_exp > 0 {
        super::combat::reward(ctx, st, sid, q.gift_exp).await?;
    }
    info!("[quête] {name} termine « {} »", q.name);
    send_system(st, sid, &format!("Quête « {} » terminée.", q.name));
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Progression
// ---------------------------------------------------------------------------------------------

/// Envoie la progression d'une quête en cours (op 10).
fn send_progress(ctx: &WorldCtx, st: &WorldState, sid: u32, tid: u32) {
    let p = &st.players[&sid];
    let (Some(q), Some(a)) = (ctx.quests.get(tid), p.quests.active(tid)) else { return };
    let have = q.item.map(|(item, _)| held(p, item)).unwrap_or(0);
    p.send(&schema::QUEST, quest(op::PROGRESS, p.ch.id).with(3, info_mini(a, q.objectives_met(a, have))).with(4, tid));
}

/// Un monstre tué par `sid` : compteurs des quêtes, objets de quête.
pub async fn on_kill(ctx: &WorldCtx, st: &mut WorldState, sid: u32, monster: u32) -> anyhow::Result<()> {
    let Some(p) = st.players.get(&sid) else { return Ok(()) };
    let cid = p.ch.id;
    let concerned: Vec<u32> = p
        .quests
        .active
        .iter()
        .filter(|a| ctx.quests.get(a.tid).is_some_and(|q| q.hunts.iter().flatten().any(|h| h.monster == monster) || q.drops.iter().any(|d| d.monster == monster)))
        .map(|a| a.tid)
        .collect();
    for tid in concerned {
        let q = ctx.quests.get(tid).unwrap();
        let p = st.players.get_mut(&sid).unwrap();
        let Some(a) = p.quests.active.iter_mut().find(|a| a.tid == tid) else { continue };
        let mut changed = false;
        for (i, h) in q.hunts.iter().enumerate() {
            let Some(h) = h.filter(|h| h.monster == monster && h.count > 0) else { continue };
            let counter = if i == 0 { &mut a.count_a } else { &mut a.count_b };
            if *counter < h.count {
                *counter += 1;
                changed = true;
            }
        }
        let (ca, cb) = (a.count_a, a.count_b);
        if changed {
            ctx.db.characters.exec("UPDATE character_quests SET count_a = ?, count_b = ? WHERE character_id = ? AND quest_tid = ?", (ca, cb, cid, tid)).await?;
        }
        for d in q.drops.iter().filter(|d| d.monster == monster) {
            let p = st.players.get_mut(&sid).unwrap();
            if held(p, d.item) >= d.count || rand::thread_rng().gen_range(0..100) >= d.rate {
                continue;
            }
            let stack = ctx.catalog.get(d.item).map(|x| x.stack).unwrap_or(1);
            match p.inventory.add(&ctx.db, d.item, 1, stack, None).await? {
                Some((it, true)) => p.send_raw(item_add(it.iid, it.tid, Some(cid), it.count, item_status::INVENTORY, Some(it.grid))),
                Some((it, false)) => p.send_raw(item_count(&it, cid)),
                None => send_system(st, sid, "Inventaire plein : objet de quête perdu."),
            }
            changed = true;
        }
        if changed {
            send_progress(ctx, st, sid, tid);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shattered_roots() -> QuestTemplate {
        let d = serde_json::json!({ "Need_Level_Min": "120", "Need_Level_Max": "120", "Complete_ItemTid": "15513", "Complete_ItemCount": "50",
            "Complete_MonsterA_Tid": "5783", "DropItemA_TID": "15513", "DropItemA_Property": "100", "DropItemA_Count": "50",
            "Gift_ItemA_Tid": "65461;1", "Gift_ItemA_Count": "1", "Gift_Money": "0", "Self_Quest": "1" });
        QuestTemplate::from_data(4632, "Shattered Roots", &d)
    }

    #[test]
    fn lecture_d_une_quete() {
        let q = shattered_roots();
        assert_eq!((q.level_min, q.level_max, q.item, q.self_quest), (120, 120, Some((15513, 50)), true));
        assert_eq!(q.hunts[0], Some(Hunt { monster: 5783, count: 0 }));
        assert_eq!(q.drops, vec![QuestDrop { monster: 5783, item: 15513, rate: 100, count: 50 }]);
        assert_eq!(q.gifts, vec![(65461, 1)]);
        let hunt = QuestTemplate::from_data(4599, "Winged Menace", &serde_json::json!({ "Complete_MonsterA_Tid": "1575", "Complete_MonsterA_Count": "200", "Need_PreviousQuestTid": "4598" }));
        assert_eq!((hunt.hunts[0], hunt.previous, hunt.self_quest), (Some(Hunt { monster: 1575, count: 200 }), Some(4598), false));
    }

    #[test]
    fn conditions_et_objectifs() {
        let q = shattered_roots();
        let mut j = PlayerQuests::default();
        assert_eq!(q.refusal(119, 1, 1, &j), Some("niveau"));
        assert_eq!(q.refusal(120, 1, 1, &j), None);
        j.active.push(ActiveQuest { tid: 4632, ..Default::default() });
        assert_eq!(q.refusal(120, 1, 1, &j), Some("quête déjà en cours"));
        assert!(!q.objectives_met(&j.active[0], 49) && q.objectives_met(&j.active[0], 50));
        let mut hunt = QuestTemplate { tid: 1, hunts: [Some(Hunt { monster: 7, count: 3 }), None], previous: Some(4632), ..Default::default() };
        assert_eq!(hunt.refusal(1, 1, 1, &j), Some("quête précédente non terminée"));
        j.done.insert(4632);
        assert_eq!(hunt.refusal(1, 1, 1, &j), None);
        assert!(!hunt.objectives_met(&ActiveQuest { tid: 1, count_a: 2, count_b: 0 }, 0));
        assert!(hunt.objectives_met(&ActiveQuest { tid: 1, count_a: 3, count_b: 0 }, 0));
        hunt.races = vec![2];
        assert_eq!(hunt.refusal(1, 1, 1, &j), Some("race ou classe"));
    }

    #[test]
    fn drapeaux_des_quetes_terminees() {
        let j = PlayerQuests { done: [1, 8, 4632, 16384 + 3].into_iter().collect(), ..Default::default() };
        let [a, b, c] = j.flag_blocks();
        assert_eq!((a.len(), a[0], a[1], a[4632 / 8], b[0], c.iter().any(|x| *x != 0)), (0x800, 0b10, 0b1, 1 << (4632 % 8), 0b1000, false));
    }

    #[test]
    fn paquets_de_quete() {
        let j = PlayerQuests { active: vec![ActiveQuest { tid: 4611, count_a: 12, count_b: 0 }], done: [4632].into_iter().collect() };
        let pk = login_packets(7, &j);
        assert_eq!(pk.len(), 2);
        let flags = archcore_net::decode_packet(&schema::QUEST, &pk[0]).unwrap();
        assert_eq!((flags.int(0), flags.bytes(8).map(|b| b.len())), (Some(op::FLAGS), Some(0x800)));
        let add = archcore_net::decode_packet(&schema::QUEST, &pk[1]).unwrap();
        let info = decode_mini(&mini::QUEST_INFO, add.bytes(3).unwrap()).unwrap();
        assert_eq!((add.int(0), info.int(0), info.int(1)), (Some(op::ADD), Some(4611), Some(12)));
    }
}
