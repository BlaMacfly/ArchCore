//! Quêtes : catalogue (questtemplate.ini, questgroup.ini), journal des personnages, acceptation,
//! progression, fin et récompenses.
//!
//! ```text
//!   AGPMEVENTQUEST (0x3E) : client → op 0 (événement du PNJ, fonction 27) ← op 0 : fenêtre des quêtes.
//!   AGPMQUEST (0x13), relevé dans le client x64 :
//!     client → op 0 accepter (champ 2 = PNJ, absent pour une quête « personnelle », champ 4 = TID)
//!            ← op 1 (champ 4 = TID, champ 6 = 1 réussi / 0 refusé) ; le client l'inscrit au journal
//!     client → op 2 (ou 15) terminer (champ 4 = TID, champ 7 = rang de la récompense choisie à partir de 0,
//!            -1 sans choix ; un rang hors des choix d'une quête qui en a est refusé)
//!            ← op 3 (champ 4, champ 6) ; récompenses (expérience, argent, objets)
//!     client → op 6 abandonner (champ 4) ← op 7 (champ 4, champ 6)
//!     ← op 10 progression : mini QUEST_INFO (TID, monstres A et B tués, objectifs remplis) ; le client
//!       affiche « nom n/total »
//!     client → op 14 suivi à l'écran (champ 3 = QUEST_INFO { TID, 0, 0, 0, drapeau }, champ 4 = TID) : drapeau 1 à la
//!            prise d'une quête et par la case « suivre », 0 au retrait ; aucune réponse. 10 quêtes suivies au plus
//!            (le client refuse lui-même la onzième : « You can register up to 10 quests »)
//!     ← op 8 (champ 1 CID, champ 4 TID) à la place de l'op 3 refusée quand les récompenses ne tiennent pas dans
//!       l'inventaire (sysstr 447) ; ← op 9 (mêmes champs) à la place de l'op 1 refusée quand l'objet remis au départ
//!       (`Quest_TransItemTid`) ne tient pas (sysstr 440)
//!     ← à l'entrée en jeu : op 5 (drapeaux des quêtes terminées, champs 8 à 10 : 3 × 0x800 octets, bit
//!       TID % 16 384 du bloc TID / 16 384, octet bit / 8, bit de poids faible d'abord), puis op 4 (mini
//!       QUEST_INFO) pour chaque quête en cours ; son 5ᵉ entier est le drapeau du suivi (le 4ᵉ est ignoré par l'op 4)
//! ```
//!     client → op 11 ramassage sur un objet du monde (champ 1 CID, champ 2 EVENT_BASE de l'objet, champ 4 TID,
//!            champ 5 rang 0 à 4 = colonnes `ObjectA` à `ObjectE`), envoyé par le client au bout d'une barre de 5 s
//!            (sysstr 331) ; voir `gather`
//! Les ops 12 et 13 (aucun abonné dans le client) ne sont pas gérées.
//! Une quête se prend auprès d'un PNJ dont le groupe (`QuestGroupID` de npc.ini → questgroup.ini) la
//! propose, ou depuis le journal si elle est « personnelle » (`Self_Quest`) ; elle se rend de même.
//! Un objet du monde peut aussi porter l'événement de quête (fonction 27 de son obj*.ini, table `quest_object`) :
//! le client le désigne par un EVENT_BASE { type 1, numéro de l'objet, -, 27 } dans AGPMEVENTQUEST op 0 (champ 3 :
//! rayon de l'objet + 50, relevé dans les deux clients) puis dans les ops 0 et 2 d'AGPMQUEST ; son groupe (le
//! `QuestGroupID` de l'événement, d'ordinaire le numéro de l'objet) donne et reprend les quêtes comme celui d'un PNJ,
//! à 1600 unités au plus de l'objet (min(champ 3, 1600) pour la fenêtre, comme le code partagé d'Archonia).

use super::items::{item_add, item_count, item_remove, item_status};
use super::npc_events::{in_range, range, resolve};
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
/// Quêtes suivies à l'écran au plus (« JournalMaxCount » du client).
pub const MAX_JOURNAL: usize = 10;
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
    /// Récompenses qui ne tiennent pas dans l'inventaire : la quête n'est pas terminée.
    pub const COMPLETE_INVENTORY_FULL: i64 = 8;
    /// Objet remis au départ qui ne tient pas dans l'inventaire : la quête n'est pas prise.
    pub const ACCEPT_INVENTORY_FULL: i64 = 9;
    pub const PROGRESS: i64 = 10;
    /// Suivi à l'écran (client → serveur).
    /// Ramassage sur un objet du monde (client → serveur).
    pub const GATHER: i64 = 11;
    pub const JOURNAL: i64 = 14;
    pub const COMPLETE_SELECT: i64 = 15;
}

/// Code d'AgpmSystemMessage « NotEnoughInventory » (systemmsgformat.xml du client).
pub const MSG_NOT_ENOUGH_INVENTORY: i64 = 41;

/// Objet à ramasser sur un objet du monde (`Object<rang>_ID`, `ObjectItem<rang>_TID`, `ObjectItem<rang>_Count`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Gather {
    pub object: u32,
    pub item: u32,
    pub count: i64,
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
    /// Objet remis à la prise de la quête (`Quest_TransItemTid`, `Quest_TransItemCount` ; vide dans les données).
    pub trans_item: Option<(u32, i64)>,
    /// Objets à rapporter (pris à la fin).
    pub item: Option<(u32, i64)>,
    pub hunts: [Option<Hunt>; 2],
    pub drops: Vec<QuestDrop>,
    /// Objets à ramasser sur des objets du monde, par rang (A à E).
    pub gathers: [Option<Gather>; 5],
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
            trans_item: pair("Quest_TransItemTid", "Quest_TransItemCount"),
            item: pair("Complete_ItemTid", "Complete_ItemCount"),
            hunts: [hunt("A"), hunt("B")],
            drops,
            gathers: ["A", "B", "C", "D", "E"].map(|k| {
                let (object, item) = (int(d, &format!("Object{k}_ID")), int(d, &format!("ObjectItem{k}_TID")));
                (object > 0 && item > 0).then(|| Gather { object: object as u32, item: item as u32, count: int(d, &format!("ObjectItem{k}_Count")).max(1) })
            }),
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

    /// Objectifs remplis : monstres tués, objets réunis et objets ramassés sur les objets du monde (`held` : nombre
    /// d'exemplaires détenus d'un TID).
    pub fn objectives_met(&self, a: &ActiveQuest, held: impl Fn(u32) -> i64) -> bool {
        let killed = [a.count_a, a.count_b];
        self.hunts.iter().zip(killed).all(|(h, k)| h.is_none_or(|h| k >= h.count))
            && self.item.is_none_or(|(item, n)| held(item) >= n)
            && self.gathers.iter().flatten().all(|g| held(g.item) >= g.count)
    }

    /// Objets repris à la fin : `Complete_ItemTid` et objets ramassés, par TID (le plus grand nombre demandé).
    pub fn taken_items(&self) -> Vec<(u32, i64)> {
        let mut out: Vec<(u32, i64)> = Vec::new();
        for (item, n) in self.item.into_iter().chain(self.gathers.iter().flatten().map(|g| (g.item, g.count))) {
            match out.iter_mut().find(|(t, _)| *t == item) {
                Some(e) => e.1 = e.1.max(n),
                None => out.push((item, n)),
            }
        }
        out
    }
}

/// Quêtes et groupes de PNJ.
#[derive(Default)]
pub struct QuestCatalog {
    quests: HashMap<u32, QuestTemplate>,
    /// Groupe → (quêtes proposées, quêtes terminées auprès de lui).
    groups: HashMap<u32, (Vec<u32>, Vec<u32>)>,
    /// Objets du monde à quêtes : numéro de l'objet → (groupe, position).
    objects: HashMap<u32, (u32, archcore_net::Vec3)>,
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
        // Table absente (base non migrée) : aucun objet à quêtes.
        let mut objects = HashMap::new();
        for r in db.world.rows("SELECT object_id, quest_group, pos_x, pos_y, pos_z FROM quest_object", ()).await.unwrap_or_default() {
            let at = archcore_net::Vec3::new(col::<f64>(&r, "pos_x") as f32, col::<f64>(&r, "pos_y") as f32, col::<f64>(&r, "pos_z") as f32);
            objects.insert(col::<u32>(&r, "object_id"), (col::<u32>(&r, "quest_group"), at));
        }
        info!("quêtes : {} ({} groupe(s) de PNJ et d'objets, {} objet(s) du monde à quêtes)", quests.len(), groups.len(), objects.len());
        Ok(QuestCatalog { quests, groups, objects })
    }

    /// Objet du monde à quêtes : (groupe, position).
    pub fn object(&self, id: u32) -> Option<(u32, archcore_net::Vec3)> {
        self.objects.get(&id).copied()
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
    /// Suivie à l'écran (op 14).
    pub journal: bool,
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
        for r in db.characters.rows("SELECT quest_tid, status, count_a, count_b, journal FROM character_quests WHERE character_id = ?", (character_id,)).await? {
            let tid: u32 = col(&r, "quest_tid");
            if col::<i64>(&r, "status") == 1 {
                q.done.insert(tid);
            } else {
                q.active.push(ActiveQuest { tid, count_a: col(&r, "count_a"), count_b: col(&r, "count_b"), journal: col::<i64>(&r, "journal") != 0 });
            }
        }
        Ok(q)
    }

    pub fn active(&self, tid: u32) -> Option<&ActiveQuest> {
        self.active.iter().find(|a| a.tid == tid)
    }

    /// Op 14 : inscrit (`on`) ou retire la quête en cours `tid` du suivi à l'écran ; rend vrai si le drapeau change.
    /// Au-delà de `MAX_JOURNAL` quêtes suivies, l'inscription est refusée.
    pub fn set_journal(&mut self, tid: u32, on: bool) -> bool {
        let full = self.active.iter().filter(|a| a.journal).count() >= MAX_JOURNAL;
        let Some(a) = self.active.iter_mut().find(|a| a.tid == tid) else { return false };
        if a.journal == on || (on && full) {
            return false;
        }
        a.journal = on;
        true
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
    build_mini(s, &Fields::new(s).with(0, a.tid).with(1, a.count_a).with(2, a.count_b).with(3, done as i64).with(4, a.journal as i64))
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

/// `count` exemplaires de `item` (piles de `stack`) tiennent dans l'inventaire : une case libre, ou une pile du même
/// objet qui a la place.
fn fits(p: &super::Player, item: u32, count: i64, stack: i64) -> bool {
    p.inventory.free_count() > 0 || (stack > 1 && p.inventory.items.values().any(|i| i.tid == item && i.count + count <= stack))
}

/// Nombre d'exemplaires d'un objet dans l'inventaire.
fn held(p: &super::Player, tid: u32) -> i64 {
    p.inventory.items.values().filter(|i| i.tid == tid).map(|i| i.count).sum()
}

// ---------------------------------------------------------------------------------------------
// Fenêtre auprès d'un PNJ
// ---------------------------------------------------------------------------------------------

/// Portée de la fenêtre d'un objet : champ 3 d'AGPMEVENTQUEST (rayon de l'objet + 50 chez le client), borné à 1600 ;
/// 1600 sans valeur positive (FUN_1402a5460 d'Archonia).
pub fn object_range(asked: Option<i64>) -> f64 {
    match asked {
        Some(r) if r > 0 => (r as f64).min(range::QUEST),
        _ => range::QUEST,
    }
}

/// Groupe de quêtes de la source désignée par un mini EVENT_BASE (PNJ, ou objet du monde de type 1, à `object_reach`
/// au plus), si elle est à portée.
fn source_group(ctx: &WorldCtx, st: &WorldState, sid: u32, event: Option<&[u8]>, object_reach: f64) -> Option<u32> {
    let base = decode_mini(&mini::EVENT_BASE, event?).ok()?;
    let p = st.players.get(&sid)?;
    if base.int(0) == Some(super::npcs::BASE_TYPE_OBJECT) {
        base.int(3).is_none_or(|f| f == event_function::QUEST).then_some(())?;
        let (group, at) = ctx.quests.object(base.int(1)? as u32)?;
        let me = super::presence::position_now(p, std::time::Instant::now());
        return (super::combat::dist2d(me, at) <= object_reach + super::npc_events::RANGE_TOLERANCE).then_some(group);
    }
    let (npc, _) = resolve(&ctx.npcs, &base)?;
    let ev = npc.events.iter().find(|e| e.function == event_function::QUEST)?;
    in_range(npc, p, range::QUEST).then_some(())?;
    ev.param("QuestGroupID").and_then(|g| g.trim().parse::<u32>().ok())
}

/// AGPMEVENTQUEST reçu : fenêtre des quêtes d'un PNJ ou d'un objet du monde.
pub fn on_event(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) {
    if p.int(0) != Some(0) {
        return;
    }
    let Some(group) = source_group(ctx, st, sid, p.bytes(1), object_range(p.int(3))) else {
        warn!("[quête] #{sid} fenêtre de quêtes refusée (PNJ ou objet inconnu, sans quêtes ou trop loin)");
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
        op::COMPLETE | op::COMPLETE_SELECT => complete(ctx, st, sid, tid, p.bytes(2), p.int(7).unwrap_or(-1)).await?,
        op::CANCEL => cancel(ctx, st, sid, tid).await?,
        op::JOURNAL => journal(ctx, st, sid, tid, p).await?,
        op::GATHER => gather(ctx, st, sid, tid, p.bytes(2), p.int(5).unwrap_or(-1)).await?,
        other => warn!("[quête] #{sid} op {other} non géré"),
    }
    Ok(())
}

/// Le PNJ ou l'objet du monde (ou le journal, pour une quête personnelle) permet de prendre (`give`) ou de rendre la
/// quête.
fn place_allows(ctx: &WorldCtx, st: &WorldState, sid: u32, q: &QuestTemplate, event: Option<&[u8]>, give: bool) -> bool {
    match source_group(ctx, st, sid, event, range::QUEST).and_then(|g| ctx.quests.group(g)) {
        Some((offered, confirmed)) => (if give { offered } else { confirmed }).contains(&q.tid) || q.self_quest,
        None => q.self_quest,
    }
}

/// Op 14 : suivi à l'écran d'une quête en cours (drapeau au 5ᵉ entier du QUEST_INFO du champ 3), enregistré pour
/// l'entrée en jeu suivante. Pas de réponse.
async fn journal(ctx: &WorldCtx, st: &mut WorldState, sid: u32, tid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some(info) = p.bytes(3).and_then(|b| decode_mini(&mini::QUEST_INFO, b).ok()) else { return Ok(()) };
    let on = info.int(4).unwrap_or(0) != 0;
    let tid = if tid != 0 { tid } else { info.int(0).unwrap_or(0) as u32 };
    let pl = st.players.get_mut(&sid).unwrap();
    if !pl.quests.set_journal(tid, on) {
        crate::debug!("[quête] {} : suivi de la quête {tid} inchangé ({})", pl.ch.name, if on { "inscription" } else { "retrait" });
        return Ok(());
    }
    let character_id = pl.ch.id;
    info!("[quête] {} : quête {tid} {}", pl.ch.name, if on { "suivie à l'écran" } else { "retirée du suivi" });
    ctx.db.characters.exec("UPDATE character_quests SET journal = ? WHERE character_id = ? AND quest_tid = ? AND status = 0", (on as i64, character_id, tid)).await?;
    Ok(())
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
    let why = q.refusal(p.ch.level, p.factors.ftype[0] as i64, p.factors.ftype[2] as i64, &p.quests).or_else(|| (!place_allows(ctx, st, sid, q, event, true)).then_some("ce PNJ ou cet objet ne la propose pas"));
    if let Some(why) = why {
        info!("[quête] {} ne peut pas prendre « {} » : {why}", p.ch.name, q.name);
        send_system(st, sid, &format!("Quête « {} » impossible : {why}.", q.name));
        reply(st, sid, op::ACCEPT_RESULT, tid, false);
        return Ok(());
    }
    let character_id = p.ch.id;
    // Objet remis au départ : il doit tenir dans l'inventaire (une case libre, ou une pile du même objet qui l'accueille),
    // sinon op 9 (le client affiche sysstr 440).
    if let Some((item, count)) = q.trans_item {
        let stack = ctx.catalog.get(item).map(|d| d.stack).unwrap_or(1);
        if !fits(p, item, count, stack) {
            info!("[quête] {} ne peut pas prendre « {} » : inventaire plein (objet de départ {item})", p.ch.name, q.name);
            p.send(&schema::QUEST, quest(op::ACCEPT_INVENTORY_FULL, character_id).with(4, tid));
            return Ok(());
        }
    }
    ctx.db
        .characters
        .exec("REPLACE INTO character_quests (character_id, quest_tid, status, count_a, count_b, journal) VALUES (?, ?, 0, 0, 0, 0)", (character_id, tid))
        .await?;
    if let Some((item, count)) = q.trans_item {
        let stack = ctx.catalog.get(item).map(|d| d.stack).unwrap_or(1);
        let p = st.players.get_mut(&sid).unwrap();
        match p.inventory.add(&ctx.db, item, count, stack, None).await? {
            Some((it, true)) => p.send_raw(item_add(it.iid, it.tid, Some(character_id), it.count, item_status::INVENTORY, Some(it.grid))),
            Some((it, false)) => p.send_raw(item_count(&it, character_id)),
            None => warn!("[quête] {} : objet de départ {item} non remis", p.ch.name),
        }
    }
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
    drop_quest_items(ctx, st, sid, tid).await
}

/// Objets de la quête `tid` (« QuestItem » d'itemdatatable : le numéro de la quête, pas un drapeau) détruits à son
/// abandon ou à sa fin (règle proposée : ni vente ni échange, `npc_trade.rs`, `trade.rs`).
async fn drop_quest_items(ctx: &WorldCtx, st: &mut WorldState, sid: u32, tid: u32) -> anyhow::Result<()> {
    let Some(p) = st.players.get_mut(&sid) else { return Ok(()) };
    let cid = p.ch.id;
    let gone: Vec<u32> = p.inventory.items.values().filter(|i| ctx.catalog.quest_items.get(&i.tid) == Some(&tid)).map(|i| i.iid).collect();
    for iid in gone {
        p.inventory.remove(&ctx.db, iid).await?;
        p.send_raw(item_remove(iid, Some(cid)));
        info!("[quête] {} : objet de quête {iid} retiré (quête {tid})", p.ch.name);
    }
    Ok(())
}

/// Récompense au choix d'une quête : rang `choice` à partir de 0 (champ 7 de l'op 2, -1 sans choix, comme le clic
/// d'Archonia). Quête sans choix : aucune ; rang hors des bornes d'une quête à choix : refus.
pub fn reward_choice(choices: &[(u32, i64)], choice: i64) -> Result<Option<(u32, i64)>, ()> {
    if choices.is_empty() {
        return Ok(None);
    }
    usize::try_from(choice).ok().and_then(|i| choices.get(i)).copied().map(Some).ok_or(())
}

async fn complete(ctx: &WorldCtx, st: &mut WorldState, sid: u32, tid: u32, event: Option<&[u8]>, choice: i64) -> anyhow::Result<()> {
    let p = &st.players[&sid];
    let (Some(q), Some(a)) = (ctx.quests.get(tid), p.quests.active(tid).copied()) else {
        reply(st, sid, op::COMPLETE_RESULT, tid, false);
        return Ok(());
    };
    let chosen = reward_choice(&q.choices, choice);
    let rewards: Vec<(u32, i64)> = q.gifts.iter().copied().chain(chosen.ok().flatten()).collect();
    let why = if chosen.is_err() {
        Some("choix de récompense invalide")
    } else if !q.objectives_met(&a, |t| held(p, t)) {
        Some("objectifs non remplis")
    } else if q.complete_level > 0 && p.ch.level < q.complete_level {
        Some("niveau")
    } else if !place_allows(ctx, st, sid, q, event, false) {
        Some("rendez-la au bon PNJ ou au bon objet")
    } else {
        None
    };
    // Récompenses qui ne tiennent pas : op 8 (sysstr 447 chez le client, qui rafraîchit sa fenêtre), quête gardée.
    if why.is_none() && p.inventory.free_count() < rewards.len() {
        info!("[quête] {} ne peut pas terminer « {} » : inventaire plein", p.ch.name, q.name);
        p.send(&schema::QUEST, quest(op::COMPLETE_INVENTORY_FULL, p.ch.id).with(4, tid));
        return Ok(());
    }
    if let Some(why) = why {
        info!("[quête] {} ne peut pas terminer « {} » : {why}", p.ch.name, q.name);
        send_system(st, sid, &format!("Quête « {} » : {why}.", q.name));
        reply(st, sid, op::COMPLETE_RESULT, tid, false);
        return Ok(());
    }
    let (cid, name, money) = (p.ch.id, p.ch.name.clone(), p.ch.money as i64);
    // Objets rapportés et objets ramassés sur les objets du monde, pris dans l'inventaire.
    for (item, mut need) in q.taken_items() {
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
    drop_quest_items(ctx, st, sid, tid).await?;
    for (item, count) in rewards {
        let stack = ctx.catalog.get(item).map(|d| d.stack).unwrap_or(1);
        let p = st.players.get_mut(&sid).unwrap();
        match p.inventory.add(&ctx.db, item, count, stack, None).await? {
            Some((it, true)) => p.send_raw(item_add(it.iid, it.tid, Some(cid), it.count, item_status::INVENTORY, Some(it.grid))),
            Some((it, false)) => p.send_raw(item_count(&it, cid)),
            None => warn!("[quête] {name} : récompense {item} perdue (inventaire plein)"),
        }
    }
    // Titre Storyteller équipé : expérience et or des récompenses + Value_1 % (effets 1 et 2).
    let (exp_bonus, gold_bonus) = st.players.get(&sid).map_or((0, 0), |p| super::titles::quest_bonus(ctx, p));
    let gift_money = q.gift_money + q.gift_money * gold_bonus / 100;
    let gift_exp = q.gift_exp + q.gift_exp * exp_bonus.max(0) as u64 / 100;
    if gift_money > 0 {
        super::set_money(ctx, st, sid, (money + gift_money).min(super::npc_trade::MAX_MONEY)).await?;
    }
    if gift_exp > 0 {
        super::combat::reward(ctx, st, sid, gift_exp).await?;
    }
    super::titles::on_quest(ctx, st, sid).await?;
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
    p.send(&schema::QUEST, quest(op::PROGRESS, p.ch.id).with(3, info_mini(a, q.objectives_met(a, |t| held(p, t)))).with(4, tid));
}

/// Op 11 : ramassage sur un objet du monde, au bout de la barre de 5 s du client. Accepté (choix d'ArchCore, le
/// serveur d'origine ne se voit pas) si la quête est en cours, que son `Object<rang>_ID` est l'objet désigné, à 1600
/// unités au plus, et que l'objet n'est pas déjà détenu au nombre demandé : un exemplaire est remis, puis la
/// progression (op 10). Refus sans réponse, comme le client qui ne traite pas d'op 11 ; inventaire plein :
/// AgpmSystemMessage `NotEnoughInventory`. L'objet du monde reste disponible pour tous.
async fn gather(ctx: &WorldCtx, st: &mut WorldState, sid: u32, tid: u32, event: Option<&[u8]>, rank: i64) -> anyhow::Result<()> {
    let p = &st.players[&sid];
    let name = p.ch.name.clone();
    let Some(q) = ctx.quests.get(tid) else {
        warn!("[quête] {name} : ramassage pour une quête inconnue ({tid})");
        return Ok(());
    };
    let g = usize::try_from(rank).ok().and_then(|r| q.gathers.get(r).copied().flatten());
    let base = event.and_then(|b| decode_mini(&mini::EVENT_BASE, b).ok());
    let object = base.as_ref().filter(|b| b.int(0) == Some(super::npcs::BASE_TYPE_OBJECT)).and_then(|b| b.int(1)).map(|o| o as u32);
    let near = |g: &Gather| {
        ctx.quests.object(g.object).is_some_and(|(_, at)| {
            super::combat::dist2d(super::presence::position_now(p, std::time::Instant::now()), at) <= range::QUEST + super::npc_events::RANGE_TOLERANCE
        })
    };
    let why = match g {
        _ if p.quests.active(tid).is_none() => Some("quête non en cours"),
        None => Some("rang sans objet à ramasser"),
        Some(g) if object != Some(g.object) => Some("pas l'objet de la quête"),
        Some(g) if !near(&g) => Some("trop loin de l'objet"),
        Some(g) if held(p, g.item) >= g.count => Some("objet déjà détenu"),
        Some(_) => None,
    };
    let (Some(g), None) = (g, why) else {
        info!("[quête] {name} ne ramasse rien pour « {} » (rang {rank}) : {}", q.name, why.unwrap_or(""));
        return Ok(());
    };
    let stack = ctx.catalog.get(g.item).map(|d| d.stack).unwrap_or(1);
    if !fits(p, g.item, 1, stack) {
        info!("[quête] {name} ne ramasse rien pour « {} » : inventaire plein", q.name);
        let s = &schema::SYSTEM_MESSAGE;
        p.send(s, Fields::new(s).with(0, 0).with(1, MSG_NOT_ENOUGH_INVENTORY));
        return Ok(());
    }
    let cid = p.ch.id;
    let p = st.players.get_mut(&sid).unwrap();
    match p.inventory.add(&ctx.db, g.item, 1, stack, None).await? {
        Some((it, true)) => p.send_raw(item_add(it.iid, it.tid, Some(cid), it.count, item_status::INVENTORY, Some(it.grid))),
        Some((it, false)) => p.send_raw(item_count(&it, cid)),
        None => {
            warn!("[quête] {name} : objet ramassé {} non remis", g.item);
            return Ok(());
        }
    }
    info!("[quête] {name} ramasse l'objet {} sur l'objet du monde {} (« {} », rang {rank})", g.item, g.object, q.name);
    send_progress(ctx, st, sid, tid);
    Ok(())
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

    #[test]
    fn recompense_au_choix() {
        let choices = [(10, 1), (11, 2), (12, 3)];
        assert_eq!(reward_choice(&choices, 0), Ok(Some((10, 1))), "rang à partir de 0");
        assert_eq!(reward_choice(&choices, 2), Ok(Some((12, 3))));
        assert_eq!(reward_choice(&choices, 3), Err(()), "hors des bornes");
        assert_eq!(reward_choice(&choices, -1), Err(()), "aucun choix pour une quête qui en a");
        assert_eq!(reward_choice(&[], -1), Ok(None));
        assert_eq!(reward_choice(&[], 5), Ok(None), "quête sans choix : champ ignoré");
    }

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
        assert!(!q.objectives_met(&j.active[0], |_| 49) && q.objectives_met(&j.active[0], |_| 50));
        let mut hunt = QuestTemplate { tid: 1, hunts: [Some(Hunt { monster: 7, count: 3 }), None], previous: Some(4632), ..Default::default() };
        assert_eq!(hunt.refusal(1, 1, 1, &j), Some("quête précédente non terminée"));
        j.done.insert(4632);
        assert_eq!(hunt.refusal(1, 1, 1, &j), None);
        assert!(!hunt.objectives_met(&ActiveQuest { tid: 1, count_a: 2, ..Default::default() }, |_| 0));
        assert!(hunt.objectives_met(&ActiveQuest { tid: 1, count_a: 3, ..Default::default() }, |_| 0));
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
        let j = PlayerQuests { active: vec![ActiveQuest { tid: 4611, count_a: 12, count_b: 0, journal: true }], done: [4632].into_iter().collect() };
        let pk = login_packets(7, &j);
        assert_eq!(pk.len(), 2);
        let flags = archcore_net::decode_packet(&schema::QUEST, &pk[0]).unwrap();
        assert_eq!((flags.int(0), flags.bytes(8).map(|b| b.len())), (Some(op::FLAGS), Some(0x800)));
        let add = archcore_net::decode_packet(&schema::QUEST, &pk[1]).unwrap();
        let info = decode_mini(&mini::QUEST_INFO, add.bytes(3).unwrap()).unwrap();
        assert_eq!((add.int(0), info.int(0), info.int(1), info.int(4)), (Some(op::ADD), Some(4611), Some(12), Some(1)), "5ᵉ entier : suivi à l'écran");
    }

    #[test]
    fn suivi_a_l_ecran() {
        let mut j = PlayerQuests { active: (1..=12).map(|tid| ActiveQuest { tid, ..Default::default() }).collect(), ..Default::default() };
        assert!(!j.set_journal(99, true), "quête absente");
        for tid in 1..=10 {
            assert!(j.set_journal(tid, true));
        }
        assert!(!j.set_journal(3, true), "déjà suivie");
        assert!(!j.set_journal(11, true), "dix quêtes au plus");
        assert!(j.set_journal(3, false) && j.set_journal(11, true));
        assert_eq!(j.active.iter().filter(|a| a.journal).count(), MAX_JOURNAL);
        // Op 14 telle que le client l'envoie : CID, QUEST_INFO { TID, 0, 0, 0, drapeau }, TID.
        let s = &mini::QUEST_INFO;
        let info = build_mini(s, &Fields::new(s).with(0, 4611).with(1, 0).with(2, 0).with(3, 0).with(4, 1));
        let raw = archcore_net::build_packet(&schema::QUEST, &quest(op::JOURNAL, 7).with(3, info).with(4, 4611));
        let p = archcore_net::decode_packet(&schema::QUEST, &raw).unwrap();
        let back = decode_mini(&mini::QUEST_INFO, p.bytes(3).unwrap()).unwrap();
        assert_eq!((p.int(0), p.int(4), back.int(0), back.int(4)), (Some(14), Some(4611), Some(4611), Some(1)));
    }

    #[test]
    fn ramassage_sur_les_objets_du_monde() {
        // « Permission » (2830) : quatre certificats, un par objet du monde (rangs A à D).
        let d = serde_json::json!({ "ObjectA_ID": "178520252", "ObjectItemA_TID": "4830", "ObjectItemA_Count": "1",
            "ObjectB_ID": "178520255", "ObjectItemB_TID": "4831", "ObjectItemB_Count": "1", "ObjectD_ID": "178520253", "ObjectItemD_TID": "4833",
            "ObjectE_ID": "1", "ObjectItemE_TID": "" });
        let q = QuestTemplate::from_data(2830, "Permission", &d);
        assert_eq!(q.gathers[0], Some(Gather { object: 178_520_252, item: 4830, count: 1 }));
        assert_eq!((q.gathers[2], q.gathers[3].map(|g| g.count), q.gathers[4]), (None, Some(1), None), "nombre absent : 1 ; objet sans TID : rien");
        let a = ActiveQuest { tid: 2830, ..Default::default() };
        let held = |have: &'static [u32]| move |t: u32| have.iter().filter(|x| **x == t).count() as i64;
        assert!(!q.objectives_met(&a, held(&[4830, 4831])));
        assert!(q.objectives_met(&a, held(&[4830, 4831, 4833])));
        assert_eq!(q.taken_items(), vec![(4830, 1), (4831, 1), (4833, 1)]);
        let mixed = QuestTemplate::from_data(9, "x", &serde_json::json!({ "Complete_ItemTid": "4830", "Complete_ItemCount": "3", "ObjectA_ID": "5", "ObjectItemA_TID": "4830" }));
        assert_eq!(mixed.taken_items(), vec![(4830, 3)], "même objet : le plus grand nombre");
    }

    #[test]
    fn portee_d_un_objet() {
        assert_eq!(object_range(Some(250)), 250.0, "rayon de l'objet + 50");
        assert_eq!(object_range(Some(5000)), 1600.0, "borné à 1600");
        assert_eq!((object_range(Some(0)), object_range(None)), (1600.0, 1600.0));
    }

    #[test]
    fn objet_de_depart() {
        let q = QuestTemplate::from_data(10, "Départ", &serde_json::json!({ "Quest_TransItemTid": "1234", "Quest_TransItemCount": "2" }));
        assert_eq!(q.trans_item, Some((1234, 2)));
        assert_eq!(shattered_roots().trans_item, None);
    }
}
