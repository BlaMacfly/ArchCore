//! Titres de l'épisode 8 (AgpmTitle, 0x60, structure brute dans les deux sens, comme AgpmShrineBattle).
//!
//! ```text
//!   en-tête   0x13 = 1, op U16 en 0x14, nom du personnage concerné CHAR[49] en 0x16, TID du titre en 0x48, champs de
//!             l'op dès 0x4C ; longueur = taille de la structure + 1 (pied 0x6B) ; ops `title_op`, `title_quest`
//!   entrée    ← op 5 par titre possédé (un seul, 0x5C = 0, sans titre), op 6.7 par objectif, op 9 du titre actif ; un
//!             porteur qui entre dans la vue d'un joueur lui envoie son op 9 (`presence.rs`)
//!   PNJ       dialogue (0x37 op 0) d'un « <Title Manager> » ← op 10 au lieu de l'accord (TID 1 si `Title.Schola`)
//!   C 6.0     fixer l'objectif : contrôles du client refaits (ArchLord, chef de guilde, classe, niveau, Con, Str, Int,
//!             Dex, Wis, Cha, or, objets, titres préalables ; 999 objectifs, 500 titres et objectifs) ; or et objets de
//!             coût consommés ← 6.1 (date, 1), puis 6.3 si des conditions sont déjà remplies ; refus : 6.1 résultat 0
//!   suivi     ← 6.3 à chaque changement d'un compteur (récolte, objet utilisé, niveau, niveau de production)
//!   C 6.4     terminer : toutes les conditions remplies ← 6.5 (date, 1) puis op 1 (1) ; refus : 6.5 résultat 0
//!   C op 2    équiper (0x50 = 1) ou retirer : ← op 3 (bUse, 1, maintenant, 0) puis op 9 au joueur et à ses voisins ;
//!             moins de `Title.ChangeDelaySeconds` après le dernier changement : op 3 (bUse, 0, heure du changement, 3)
//!   C op 7    supprimer un titre possédé ou abandonner un objectif ← op 9 bUse 0 si le titre était actif, op 8 (1)
//!   C op 4, 6.6 (sans appelant dans Archonia) ← listes ; op 11 (choix d'une compétence, Archonia) journalisée
//! ```
//! Conditions (`Title_Check`, TID du premier titre de la famille comme dans la table de Webzen ; les numéros de type
//! d'Archonia sont ramenés par `check::normalize`) : 1 objet Set ramassé sur un monstre (Collector), 10001 récolte
//! réussie sur un cadavre de la créature Set (aussi ses morts avec `Title.CollectCountsKills`), 30001 créature Set tuée
//! (Natural Enemy), 40001 utilisation de l'objet Set, 90001 joueur tué (Killer), 140001 niveau, 170001 quête terminée
//! (Storyteller), 200001 / 200002 / 200003 niveau d'Alchimie / Cuisine / Collecte ; remplie quand la valeur atteint
//! Value. Effets du titre **équipé** seulement : compétences `Skill_Tid_*` (passives, `skills::sync_passives`), 1 / 2
//! expérience / or des récompenses de quête + Value_1 %, 3 / 4 / 5 réussite d'Alchimie / Cuisine / Collecte + Value_1 %,
//! 6 sertissage de la pierre Value_1 + Value_2 %, 7 objets récoltés × Value_2 sur la créature Value_1, 8 expérience de la
//! créature Value_1 + Value_2 %, 9 potion Value_1 : + Value_2 points (Set 0) ou % (Set 1). Le charisme et ses rangs
//! (`charisma_title`) restent du ressort du client, qui les déduit du CHA envoyé.
//! Choix d'ArchCore (le serveur d'origine est absent) : listes poussées à l'entrée, coûts pris à l'acceptation (Cost_Cash
//! ignoré, toujours vide), bornes du client seulement, délai compté entre deux changements quels qu'ils soient (équiper
//! ou retirer ; la suppression d'un titre actif le retire sans délai), Schola 1-3 et Defender of Chantra derrière
//! `Title.Schola`. Christmas Event, Schola 4/5 et le choix de compétence (op 11) d'Archonia ne sont pas repris.

use super::items::{item_count, item_remove};
use super::npc_events::{dialog_op, in_range, range, resolve};
use super::{send_system, Player, WorldCtx, WorldState};
use crate::db::{col, Databases};
use crate::{debug, info, warn};
use archcore_net::schema::{mini, ptype, title_op, title_quest};
use archcore_net::{decode_mini, Fields, Packet};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Décalages de l'en-tête (`FUN_1405f4ce0` et les autres émetteurs d'Archonia).
pub const NAME: usize = 0x16;
pub const NAME_LEN: usize = 49;
pub const TID: usize = 0x48;
pub const SUB: usize = 0x4C;
/// Bornes du client (`FUN_1405f4570`) : objectifs en cours, titres possédés et objectifs ensemble.
pub const MAX_GOALS: usize = 999;
pub const MAX_TITLES: usize = 500;
/// Conditions par titre (Title_Check_1 à 5).
pub const CHECKS: usize = 5;
/// Code de refus de l'op 3 : changement trop rapproché (`TitleErrorMsg_CannotExChangeTitleIn2Hour`).
pub const CODE_TOO_SOON: i32 = 3;
/// Préfixe du nom des PNJ qui ouvrent les fenêtres des titres (npc.ini, 18 PNJ à dialogue, fonction 21).
pub const MANAGER_PREFIX: &str = "<Title Manager>";

/// Types de condition (`Title_Check_N`) : TID du premier titre de la famille qui la définit (titledatatable.txt de
/// Webzen : la colonne est lue telle quelle par son client, `FUN_0087bce0`, et décrite par titlestringtable.txt).
pub mod check {
    /// Collector : objet Set ramassé sur un monstre.
    pub const COLLECT_ITEM: i64 = 1;
    /// Expert Collector : récolte sur un cadavre de la créature Set.
    pub const COLLECT: i64 = 10_001;
    /// Natural Enemy : créature Set tuée.
    pub const KILL: i64 = 30_001;
    /// Addict : objet Set utilisé.
    pub const USE_ITEM: i64 = 40_001;
    /// Killer : joueur tué.
    pub const PK: i64 = 90_001;
    /// Advanced : niveau du personnage.
    pub const LEVEL: i64 = 140_001;
    /// Storyteller : quête terminée.
    pub const QUEST: i64 = 170_001;
    /// Researcher : niveaux d'Alchimie, de Cuisine, de Collecte.
    pub const ALCHEMY: i64 = 200_001;
    pub const COOKING: i64 = 200_002;
    pub const GATHERING: i64 = 200_003;

    /// Type d'une condition : numéros de type d'Archonia (2, 5, 15, 25 à 27) ramenés aux TID de Webzen.
    pub fn normalize(kind: i64) -> i64 {
        match kind {
            2 => COLLECT,
            5 => USE_ITEM,
            15 => LEVEL,
            25 => ALCHEMY,
            26 => COOKING,
            27 => GATHERING,
            k => k,
        }
    }
}

/// Types d'effet (`Title_Effect_N`), les mêmes chez Webzen et chez Archonia.
pub mod effect {
    /// Storyteller : expérience et or des récompenses de quête + Value_1 %.
    pub const QUEST_EXP: i64 = 1;
    pub const QUEST_GOLD: i64 = 2;
    pub const ALCHEMY: i64 = 3;
    pub const COOKING: i64 = 4;
    pub const GATHERING: i64 = 5;
    /// Collector : sertissage de la pierre Value_1 + Value_2 % de réussite.
    pub const REFINE: i64 = 6;
    pub const DOUBLE_COLLECT: i64 = 7;
    /// Natural Enemy : expérience de la créature Value_1 + Value_2 %.
    pub const KILL_EXP: i64 = 8;
    pub const POTION: i64 = 9;
}

// ---------------------------------------------------------------------------------------------
// Données (title_template, charisma_title)
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Check {
    pub kind: i64,
    pub set: i64,
    pub value: i64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TitleEffect {
    pub kind: i64,
    pub set: i64,
    pub v1: i64,
    pub v2: i64,
}

/// Conditions pour fixer l'objectif (Require_*).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Require {
    pub level: i64,
    pub class: i64,
    pub guild_master: bool,
    pub archlord: bool,
    /// Con, Str, Int, Dex, Wis, Cha (points).
    pub stats: [i64; 6],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TitleDef {
    pub tid: u32,
    pub category: String,
    pub name: String,
    pub require: Require,
    pub cost_geld: i64,
    pub cost_items: Vec<(u32, i64)>,
    pub first_titles: Vec<u32>,
    pub checks: Vec<Check>,
    pub skills: Vec<u32>,
    pub effects: Vec<TitleEffect>,
}

impl TitleDef {
    /// Somme des Value_1 des effets du type `kind` (réussites de production).
    pub fn bonus(&self, kind: i64) -> i64 {
        self.effects.iter().filter(|e| e.kind == kind).map(|e| e.v1).sum()
    }
    /// Multiplicateur des objets récoltés sur la créature `monster_tid` (effet 7), 1 sans effet.
    pub fn collect_factor(&self, monster_tid: u32) -> i64 {
        self.effects.iter().filter(|e| e.kind == effect::DOUBLE_COLLECT && e.v1 == monster_tid as i64).map(|e| e.v2.max(1)).max().unwrap_or(1)
    }
    /// Bonus (pour cent) de l'effet `kind` dont Value_1 désigne `tid` (6 pierre sertie, 8 créature tuée) : Value_2.
    pub fn bonus_on(&self, kind: i64, tid: u32) -> i64 {
        self.effects.iter().filter(|e| e.kind == kind && e.v1 == tid as i64).map(|e| e.v2).sum()
    }
    /// Récupération en plus d'une potion `item_tid` qui rend `amount` (effet 9 : points si Set 0, pour cent si Set 1).
    pub fn potion_extra(&self, item_tid: u32, amount: i32) -> i32 {
        self.effects
            .iter()
            .filter(|e| e.kind == effect::POTION && e.v1 == item_tid as i64)
            .map(|e| if e.set == 1 { (amount as i64 * e.v2 / 100) as i32 } else { e.v2 as i32 })
            .sum()
    }
}

#[derive(Clone, Debug, Default)]
pub struct TitleCatalog {
    pub titles: HashMap<u32, TitleDef>,
    /// Rangs de charisme : (CHA min, CHA max, titre), dans l'ordre.
    pub charisma: Vec<(i64, i64, String)>,
    /// Defender of Chantra et Schola 1-3 chargés (`Title.Schola`).
    pub schola: bool,
    /// CID des « <Title Manager> » (nom d'origine de npc_spawn : celui du registre peut être traduit).
    pub managers: HashSet<u32>,
}

fn json_list<T: serde::de::DeserializeOwned>(text: &str) -> Vec<T> {
    serde_json::from_str(text).unwrap_or_default()
}

impl TitleCatalog {
    pub async fn load(db: &Databases, schola: bool) -> Self {
        let mut c = TitleCatalog { schola, ..Default::default() };
        let sql = "SELECT tid, category, name, optional, require_level, require_class, require_guild_master, require_archlord, require_con, \
                   require_str, require_int, require_dex, require_wis, require_cha, cost_geld, cost_items, first_titles, checks, skills, effects \
                   FROM title_template";
        match db.world.rows(sql, ()).await {
            Ok(rows) => {
                for r in &rows {
                    if col::<u32>(r, "optional") != 0 && !schola {
                        continue;
                    }
                    let n = |k: &str| col::<i64>(r, k);
                    let def = TitleDef {
                        tid: col(r, "tid"),
                        category: col(r, "category"),
                        name: col(r, "name"),
                        require: Require {
                            level: n("require_level"),
                            class: n("require_class"),
                            guild_master: n("require_guild_master") != 0,
                            archlord: n("require_archlord") != 0,
                            stats: [n("require_con"), n("require_str"), n("require_int"), n("require_dex"), n("require_wis"), n("require_cha")],
                        },
                        cost_geld: n("cost_geld"),
                        cost_items: json_list::<(u32, i64)>(&col::<String>(r, "cost_items")),
                        first_titles: json_list(&col::<String>(r, "first_titles")),
                        checks: json_list::<(i64, i64, i64)>(&col::<String>(r, "checks")).into_iter().take(CHECKS).map(|(kind, set, value)| Check { kind: check::normalize(kind), set, value }).collect(),
                        skills: json_list(&col::<String>(r, "skills")),
                        effects: json_list::<(i64, i64, i64, i64)>(&col::<String>(r, "effects")).into_iter().map(|(kind, set, v1, v2)| TitleEffect { kind, set, v1, v2 }).collect(),
                    };
                    c.titles.insert(def.tid, def);
                }
            }
            Err(e) => warn!("[titre] table title_template non lue ({e:#}) : lancer `archcore db-init` puis `archcore import`"),
        }
        if let Ok(rows) = db.world.rows("SELECT charisma_min, charisma_max, title FROM charisma_title ORDER BY `rank`", ()).await {
            c.charisma = rows.iter().map(|r| (col(r, "charisma_min"), col(r, "charisma_max"), col(r, "title"))).collect();
        }
        if let Ok(rows) = db.world.rows("SELECT id FROM npc_spawn WHERE name LIKE ?", (format!("{MANAGER_PREFIX}%"),)).await {
            c.managers = rows.iter().map(|r| super::npcs::NPC_CID_BASE + col::<u32>(r, "id")).collect();
        }
        info!(
            "titres : {} (Schola {}), {} rang(s) de charisme, {} gestionnaire(s) de titres",
            c.titles.len(),
            if schola { "comprises" } else { "écartées" },
            c.charisma.len(),
            c.managers.len()
        );
        c
    }

    pub fn get(&self, tid: u32) -> Option<&TitleDef> {
        self.titles.get(&tid)
    }

    /// Rang de charisme : première ligne dont le maximum dépasse `cha` (comme le client), son titre (vide au rang 0).
    pub fn charisma_rank(&self, cha: i64) -> Option<(usize, &str)> {
        self.charisma.iter().position(|(_, max, _)| cha <= *max).map(|i| (i, self.charisma[i].2.as_str()))
    }
}

// ---------------------------------------------------------------------------------------------
// État d'un personnage (character_titles, character_title_goals)
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Owned {
    pub skill: i32,
    pub acquired_at: i64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Goal {
    pub accepted_at: i64,
    /// Valeurs des conditions non vides, dans l'ordre du modèle.
    pub counters: [i32; CHECKS],
}

#[derive(Clone, Debug, Default)]
pub struct PlayerTitles {
    pub owned: BTreeMap<u32, Owned>,
    pub goals: BTreeMap<u32, Goal>,
    pub active: Option<u32>,
    /// Dernier changement du titre actif (secondes Unix, 0 : jamais).
    pub changed_at: i64,
}

impl PlayerTitles {
    pub async fn load(db: &Databases, character_id: u32) -> anyhow::Result<Self> {
        let mut t = PlayerTitles::default();
        for r in db.characters.rows("SELECT title_tid, active, skill_tid, acquired_at FROM character_titles WHERE character_id = ?", (character_id,)).await? {
            let tid: u32 = col(&r, "title_tid");
            if col::<u32>(&r, "active") != 0 && t.active.is_none() {
                t.active = Some(tid);
            }
            t.owned.insert(tid, Owned { skill: col(&r, "skill_tid"), acquired_at: col(&r, "acquired_at") });
        }
        for r in db.characters.rows("SELECT title_tid, accepted_at, c1, c2, c3, c4, c5 FROM character_title_goals WHERE character_id = ?", (character_id,)).await? {
            let counters = ["c1", "c2", "c3", "c4", "c5"].map(|k| col::<i32>(&r, k));
            t.goals.insert(col(&r, "title_tid"), Goal { accepted_at: col(&r, "accepted_at"), counters });
        }
        if let Some(r) = db.characters.first("SELECT title_changed_at FROM characters WHERE id = ?", (character_id,)).await? {
            t.changed_at = col(&r, "title_changed_at");
        }
        Ok(t)
    }

    /// Modèle du titre équipé.
    pub fn active_def<'a>(&self, catalog: &'a TitleCatalog) -> Option<&'a TitleDef> {
        self.active.and_then(|t| catalog.get(t))
    }

    /// Compétences du titre équipé (passives tant qu'il l'est).
    pub fn skills(&self, catalog: &TitleCatalog) -> Vec<u32> {
        self.active_def(catalog).map(|d| d.skills.clone()).unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------------------------
// Conditions
// ---------------------------------------------------------------------------------------------

/// Ce dont dépendent les conditions recalculées : niveau, niveaux de production par catégorie (0 Collecte, 1 Cuisine,
/// 2 Alchimie : `product.rs`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub level: i32,
    pub product: [i32; 3],
}

/// Ce qui vient de faire avancer un compteur.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// Récolte réussie sur un cadavre de cette créature (ou sa mort avec `Title.CollectCountsKills`).
    Collect(u32),
    /// Objet utilisé.
    UseItem(u32),
    /// Créature tuée.
    Kill(u32),
    /// Objet ramassé sur un monstre (butin), autant de fois que la quantité.
    Loot(u32, i32),
    /// Joueur tué.
    Pk,
    /// Quête terminée.
    Quest,
    /// Niveau ou niveau de production changé.
    Refresh,
}

/// Valeur d'une condition d'après l'état, pour les conditions de niveau ; `None` : compteur d'actions.
pub fn live_value(c: &Check, s: &Snapshot) -> Option<i32> {
    match c.kind {
        check::LEVEL => Some(s.level),
        check::ALCHEMY => Some(s.product[2]),
        check::COOKING => Some(s.product[1]),
        check::GATHERING => Some(s.product[0]),
        _ => None,
    }
}

/// Compteurs après `ev` ; vrai si l'un a changé. Les conditions de niveau sont toujours recalculées.
pub fn advance(def: &TitleDef, counters: &mut [i32; CHECKS], ev: Event, s: &Snapshot) -> bool {
    let before = *counters;
    for (i, c) in def.checks.iter().enumerate().take(CHECKS) {
        if let Some(v) = live_value(c, s) {
            counters[i] = v;
            continue;
        }
        let hit = match (c.kind, ev) {
            (check::COLLECT, Event::Collect(tid)) | (check::USE_ITEM, Event::UseItem(tid)) | (check::KILL, Event::Kill(tid)) => (c.set == tid as i64) as i32,
            (check::COLLECT_ITEM, Event::Loot(tid, n)) if c.set == tid as i64 => n.max(1),
            (check::PK, Event::Pk) | (check::QUEST, Event::Quest) => 1,
            _ => 0,
        };
        if hit > 0 {
            counters[i] = counters[i].saturating_add(hit);
        }
    }
    *counters != before
}

/// Toutes les conditions sont remplies (un titre sans condition l'est dès l'acceptation).
pub fn complete(def: &TitleDef, counters: &[i32; CHECKS]) -> bool {
    def.checks.iter().enumerate().take(CHECKS).all(|(i, c)| counters[i] as i64 >= c.value)
}

/// La classe exigée (Require_class_Type) admet ce personnage : 0 tous ; 101 à 901 race et classe (chevalier, archer,
/// mage des humains, puis orcs, puis elfes de lune) ; 1001 Dragon Scion ; 1101, 1201, 1301 ses formes évoluées
/// (classes 1, 2, 4). Race et classe : `Factors::ftype` (forme courante).
pub fn class_ok(code: i64, race: i32, class: i32) -> bool {
    const CLASSES: [i32; 3] = [1, 2, 4];
    match code / 100 {
        0 => true,
        n @ 1..=9 => race == (n as i32 - 1) / 3 + 1 && class == CLASSES[(n as usize - 1) % 3],
        10 => race == super::evolution::DRAGON_SCION,
        n @ 11..=13 => race == super::evolution::DRAGON_SCION && class == CLASSES[(n - 11) as usize],
        _ => false,
    }
}

/// Ce que l'on sait du joueur pour fixer un objectif.
#[derive(Clone, Debug, Default)]
pub struct Candidate {
    pub level: i64,
    pub race: i32,
    pub class: i32,
    pub guild_master: bool,
    pub archlord: bool,
    /// Con, Str, Int, Dex, Wis, Cha (points).
    pub stats: [i64; 6],
    pub money: i64,
    /// Quantités possédées par TID (inventaire ordinaire).
    pub items: HashMap<u32, i64>,
}

/// Refus de fixer l'objectif `def`, dans l'ordre des contrôles du client (`FUN_1405f6ea0`), puis les bornes ; `None` :
/// accepté.
pub fn refusal(def: &TitleDef, me: &Candidate, titles: &PlayerTitles) -> Option<String> {
    let r = &def.require;
    if titles.owned.contains_key(&def.tid) {
        return Some("titre déjà obtenu".into());
    }
    if titles.goals.contains_key(&def.tid) {
        return Some("déjà fixé comme objectif".into());
    }
    if r.archlord && !me.archlord {
        return Some("réservé à l'ArchLord".into());
    }
    if r.guild_master && !me.guild_master {
        return Some("réservé aux chefs de guilde".into());
    }
    if !class_ok(r.class, me.race, me.class) {
        return Some(format!("classe {} exigée", r.class));
    }
    if me.level < r.level {
        return Some(format!("niveau {} exigé", r.level));
    }
    for (i, label) in ["Con", "Str", "Int", "Dex", "Wis", "Cha"].iter().enumerate() {
        if me.stats[i] < r.stats[i] {
            return Some(format!("{label} {} exigé", r.stats[i]));
        }
    }
    if me.money < def.cost_geld {
        return Some(format!("{} or exigés", def.cost_geld));
    }
    for (tid, n) in &def.cost_items {
        if me.items.get(tid).copied().unwrap_or(0) < *n {
            return Some(format!("{n} × objet {tid} exigé(s)"));
        }
    }
    if let Some(t) = def.first_titles.iter().find(|t| !titles.owned.contains_key(t)) {
        return Some(format!("titre {t} exigé"));
    }
    if titles.goals.len() >= MAX_GOALS {
        return Some(format!("{MAX_GOALS} objectifs au plus"));
    }
    if titles.goals.len() + titles.owned.len() >= MAX_TITLES {
        return Some(format!("{MAX_TITLES} titres et objectifs au plus"));
    }
    None
}

// ---------------------------------------------------------------------------------------------
// Paquets
// ---------------------------------------------------------------------------------------------

/// Réponses du serveur (taille de la structure, pied exclu, dans `layout`).
#[derive(Clone, Debug, PartialEq)]
pub enum Reply {
    AddResult { result: i32, skill: i32 },
    UseResult { on: bool, result: i32, time: i64, code: i32 },
    ListEntry { changed_at: i64, valid: bool, skill: i32 },
    QuestAddResult { accepted: i64, result: i32 },
    QuestCheck { values: [i32; CHECKS] },
    QuestCompleteResult { completed: i64, result: i32 },
    QuestEntry { accepted: i64, counters: [i32; CHECKS] },
    DeleteResult { result: i32 },
    UseNear { on: bool },
    UiOpen,
}

impl Reply {
    /// Opération, sous-opération (op 6) et taille de la structure.
    pub fn layout(&self) -> (i64, Option<i64>, usize) {
        use Reply::*;
        match self {
            AddResult { .. } => (title_op::ADD_RESULT, None, 0x54),
            UseResult { .. } => (title_op::USE_RESULT, None, 0x64),
            ListEntry { .. } => (title_op::LIST_RESULT, None, 0x68),
            QuestAddResult { .. } => (title_op::QUEST, Some(title_quest::ADD_RESULT), 0x5C),
            QuestCheck { .. } => (title_op::QUEST, Some(title_quest::CHECK), 0x68),
            QuestCompleteResult { .. } => (title_op::QUEST, Some(title_quest::COMPLETE_RESULT), 0x5C),
            QuestEntry { .. } => (title_op::QUEST, Some(title_quest::LIST_RESULT), 0x84),
            DeleteResult { .. } => (title_op::DELETE_RESULT, None, 0x50),
            UseNear { .. } => (title_op::USE_NEAR, None, 0x50),
            UiOpen => (title_op::UI_OPEN, None, 0x4C),
        }
    }
}

fn put32(b: &mut [u8], off: usize, v: i32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

fn put64(b: &mut [u8], off: usize, v: i64) {
    b[off..off + 8].copy_from_slice(&v.to_le_bytes());
}

fn get32(b: &[u8], off: usize) -> Option<i32> {
    b.get(off..off + 4).map(|x| i32::from_le_bytes(x.try_into().unwrap()))
}

/// Paquet 0x60 : personnage concerné `name`, titre `tid`, réponse.
pub fn packet(name: &str, tid: u32, reply: &Reply) -> Vec<u8> {
    use Reply::*;
    let (op, sub, size) = reply.layout();
    super::shrine::raw_packet_of(ptype::TITLE, op, size, |b| {
        super::guild::society::put_text(b, NAME, NAME_LEN, name);
        put32(b, TID, tid as i32);
        if let Some(sub) = sub {
            put32(b, SUB, sub as i32);
        }
        match reply {
            UiOpen => {}
            AddResult { result, skill } => {
                put32(b, 0x4C, *result);
                put32(b, 0x50, *skill);
            }
            UseResult { on, result, time, code } => {
                put32(b, 0x4C, *on as i32);
                put32(b, 0x50, *result);
                put64(b, 0x58, *time);
                put32(b, 0x60, *code);
            }
            ListEntry { changed_at, valid, skill } => {
                put64(b, 0x50, *changed_at);
                put32(b, 0x5C, *valid as i32);
                put32(b, 0x64, *skill);
            }
            QuestAddResult { accepted: t, result } | QuestCompleteResult { completed: t, result } => {
                put64(b, 0x50, *t);
                put32(b, 0x58, *result);
            }
            QuestCheck { values } => {
                for (i, v) in values.iter().enumerate() {
                    put32(b, 0x50 + i * 4, *v);
                }
                put32(b, 0x64, 1);
            }
            QuestEntry { accepted, counters } => {
                put32(b, 0x50, 1);
                put64(b, 0x60, *accepted);
                for (i, v) in counters.iter().enumerate() {
                    put32(b, 0x70 + i * 4, *v);
                }
            }
            DeleteResult { result } => put32(b, 0x4C, *result),
            UseNear { on } => put32(b, 0x4C, *on as i32),
        }
    })
}

/// Demandes du client.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Use { on: bool },
    List,
    QuestAdd,
    QuestComplete { skill: i32 },
    QuestList,
    Delete,
    SkillSelect { skill: i32 },
}

/// Demande reçue : (nom en 0x16, TID, demande) ; `None` : opération inconnue ou du serveur, structure trop courte.
pub fn decode(raw: &[u8]) -> Option<(String, u32, Request)> {
    if raw.len() < SUB + 1 || raw.get(5) != Some(&ptype::TITLE) {
        return None;
    }
    let op = u16::from_le_bytes([raw[0x14], raw[0x15]]) as i64;
    let (req, size) = match op {
        title_op::USE => (Request::Use { on: get32(raw, 0x50)? != 0 }, 0x54),
        title_op::LIST => (Request::List, 0x4C),
        title_op::DELETE => (Request::Delete, 0x4C),
        title_op::SKILL_SELECT => (Request::SkillSelect { skill: get32(raw, 0x4C)? }, 0x50),
        title_op::QUEST => match get32(raw, SUB)? as i64 {
            title_quest::ADD => (Request::QuestAdd, 0x50),
            title_quest::COMPLETE => (Request::QuestComplete { skill: get32(raw, 0x50)? }, 0x54),
            title_quest::LIST => (Request::QuestList, 0x50),
            _ => return None,
        },
        _ => return None,
    };
    // Structure entière, pied compris.
    if raw.len() < size + 1 {
        return None;
    }
    Some((super::guild::society::get_text(raw, NAME, NAME_LEN), get32(raw, TID)? as u32, req))
}

// ---------------------------------------------------------------------------------------------
// Joueurs
// ---------------------------------------------------------------------------------------------

fn now_unix() -> i64 {
    chrono::Utc::now().timestamp()
}

fn send(st: &WorldState, sid: u32, tid: u32, reply: Reply) {
    if let Some(p) = st.players.get(&sid) {
        p.send_raw(packet(&p.ch.name, tid, &reply));
    }
}

/// Op 9 du porteur `sid` (titre `tid`), à lui et aux joueurs qui le voient.
fn send_near(st: &WorldState, sid: u32, tid: u32, on: bool) {
    let Some(p) = st.players.get(&sid) else { return };
    let plain = packet(&p.ch.name, tid, &Reply::UseNear { on });
    p.send_raw(plain.clone());
    let cid = p.ch.id;
    for (_, o) in st.players.iter().filter(|(other, o)| **other != sid && o.seen_players.contains(&cid)) {
        o.send_raw(plain.clone());
    }
}

/// Op 9 du titre actif d'un joueur, pour un observateur qui le voit entrer dans sa vue (`presence.rs`).
pub fn view_packet(p: &Player) -> Option<Vec<u8>> {
    p.titles.active.map(|tid| packet(&p.ch.name, tid, &Reply::UseNear { on: true }))
}

fn snapshot(ctx: &WorldCtx, p: &Player) -> Snapshot {
    let mut product = [0i32; 3];
    for s in &p.skills.list {
        if let Some(c) = ctx.product.category(s.tid).filter(|c| (0..3).contains(c)) {
            product[c as usize] = product[c as usize].max(s.level as i32);
        }
    }
    Snapshot { level: p.ch.level, product }
}

/// Valeurs courantes des conditions d'un objectif (conditions de niveau recalculées).
fn values(ctx: &WorldCtx, p: &Player, def: &TitleDef, goal: &Goal) -> [i32; CHECKS] {
    let mut counters = goal.counters;
    advance(def, &mut counters, Event::Refresh, &snapshot(ctx, p));
    counters
}

const SAVE_COUNTERS: &str = "UPDATE character_title_goals SET c1 = ?, c2 = ?, c3 = ?, c4 = ?, c5 = ? WHERE character_id = ? AND title_tid = ?";

/// Entrée en jeu : titres possédés (op 5), objectifs (op 6.7), titre actif (op 9).
pub fn on_enter(ctx: &WorldCtx, st: &mut WorldState, sid: u32) {
    let Some(p) = st.players.get_mut(&sid) else { return };
    let snap = snapshot(ctx, p);
    for (tid, g) in p.titles.goals.iter_mut() {
        if let Some(def) = ctx.titles.get(*tid) {
            advance(def, &mut g.counters, Event::Refresh, &snap);
        }
    }
    let p = &st.players[&sid];
    let (name, t) = (&p.ch.name, &p.titles);
    if t.owned.is_empty() {
        p.send_raw(packet(name, 0, &Reply::ListEntry { changed_at: t.changed_at, valid: false, skill: 0 }));
    }
    for (tid, o) in &t.owned {
        p.send_raw(packet(name, *tid, &Reply::ListEntry { changed_at: t.changed_at, valid: true, skill: o.skill }));
    }
    for (tid, g) in &t.goals {
        p.send_raw(packet(name, *tid, &Reply::QuestEntry { accepted: g.accepted_at, counters: g.counters }));
    }
    if let Some(tid) = t.active {
        p.send_raw(packet(name, tid, &Reply::UseNear { on: true }));
    }
}

/// Dialogue demandé (AGPMEVENTNPCDIALOG op 0) : un « <Title Manager> » ouvre les fenêtres des titres (op 10) au lieu
/// du dialogue. Rend vrai si la demande est prise ici.
pub fn on_dialog(ctx: &WorldCtx, st: &WorldState, sid: u32, p: &Packet) -> bool {
    if !ctx.config.world.title_manager_window || p.int(0) != Some(dialog_op::REQUEST) {
        return false;
    }
    let base = p.bytes(1).and_then(|b| decode_mini(&mini::EVENT_BASE, b).ok()).unwrap_or_else(|| Fields::new(&mini::EVENT_BASE));
    let Some((npc, _)) = resolve(&ctx.npcs, &base) else { return false };
    if !ctx.titles.managers.contains(&npc.cid) {
        return false;
    }
    let Some(pl) = st.players.get(&sid) else { return true };
    if super::morph::restricted(st, sid) || !in_range(npc, pl, range::DIALOG) {
        debug!("[titre] {} : {} hors de portée ou sous métamorphose", pl.ch.name, npc.name);
        return true;
    }
    info!("[titre] {} ouvre les titres chez {}", pl.ch.name, npc.name);
    // Archonia ne lit pas le TID de l'op 10 : 1 annonce à ArchRust les Schola 1-3 et Defender of Chantra.
    pl.send_raw(packet(&pl.ch.name, ctx.titles.schola as u32, &Reply::UiOpen));
    true
}

/// Paquet AgpmTitle reçu du client.
pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some(me) = st.players.get(&sid).filter(|x| x.in_game) else { return Ok(()) };
    let Some((name, tid, req)) = decode(&p.raw) else {
        let body = p.raw.get(0x14..p.raw.len().saturating_sub(1)).unwrap_or_default();
        info!("[titre] #{sid} demande inconnue ({} o) : {}", p.raw.len(), archcore_net::hex_dump(body, 64));
        return Ok(());
    };
    if !name.eq_ignore_ascii_case(&me.ch.name) {
        debug!("[titre] {} envoie une demande au nom de « {name} » : traitée pour lui-même", me.ch.name);
    }
    match req {
        Request::Use { on } => use_title(ctx, st, sid, tid, on).await,
        Request::QuestAdd => add_goal(ctx, st, sid, tid).await,
        Request::QuestComplete { .. } => complete_goal(ctx, st, sid, tid).await,
        Request::Delete => delete(ctx, st, sid, tid).await,
        Request::List | Request::QuestList => {
            on_enter(ctx, st, sid);
            Ok(())
        }
        Request::SkillSelect { skill } => {
            info!("[titre] {} choisit la compétence {skill} pour le titre {tid} : choix d'Archonia, ignoré", me.ch.name);
            Ok(())
        }
    }
}

/// Équipe ou retire un titre possédé (op 2 → op 3, op 9).
async fn use_title(ctx: &WorldCtx, st: &mut WorldState, sid: u32, tid: u32, on: bool) -> anyhow::Result<()> {
    let p = &st.players[&sid];
    let (cid, name, t) = (p.ch.id, p.ch.name.clone(), &p.titles);
    let refuse = |code: i32| Reply::UseResult { on, result: 0, time: t.changed_at, code };
    if (on && !t.owned.contains_key(&tid)) || (!on && t.active != Some(tid)) {
        info!("[titre] {name} : titre {tid} {} refusé (ni possédé ni actif)", if on { "équipé" } else { "retiré" });
        let r = refuse(0);
        send(st, sid, tid, r);
        return Ok(());
    }
    let now = now_unix();
    if on && t.active == Some(tid) {
        let r = Reply::UseResult { on, result: 1, time: t.changed_at, code: 0 };
        send(st, sid, tid, r);
        return Ok(());
    }
    let delay = ctx.config.world.title_change_delay_secs;
    if t.changed_at > 0 && now - t.changed_at < delay {
        info!("[titre] {name} : changement refusé, {} s depuis le dernier (délai {delay} s)", now - t.changed_at);
        let r = refuse(CODE_TOO_SOON);
        send(st, sid, tid, r);
        return Ok(());
    }
    let old = t.active;
    ctx.db
        .characters
        .atomic(vec![
            ("UPDATE character_titles SET active = (title_tid = ? AND ?) WHERE character_id = ?", (tid, on, cid).into()),
            ("UPDATE characters SET title_changed_at = ? WHERE id = ?", (now, cid).into()),
        ])
        .await?;
    let p = st.players.get_mut(&sid).unwrap();
    p.titles.active = on.then_some(tid);
    p.titles.changed_at = now;
    send(st, sid, tid, Reply::UseResult { on, result: 1, time: now, code: 0 });
    if let Some(old) = old.filter(|o| *o != tid) {
        send_near(st, sid, old, false);
    }
    send_near(st, sid, tid, on);
    refresh_skills(ctx, st, sid);
    let label = ctx.titles.get(tid).map(|d| d.name.as_str()).unwrap_or("?");
    info!("[titre] {name} {} le titre {tid} ({label})", if on { "équipe" } else { "retire" });
    Ok(())
}

/// Compétences du titre équipé recopiées dans les passives ; facteurs renvoyés s'ils ont changé.
pub fn refresh_skills(ctx: &WorldCtx, st: &mut WorldState, sid: u32) {
    let Some(p) = st.players.get_mut(&sid) else { return };
    p.skills.title_skills = p.titles.skills(&ctx.titles);
    let (caster_tid, form) = (p.ch.template_id, super::skills::form_class(&p.factors));
    if super::skills::sync_passives(&ctx.skills, &mut p.skills, caster_tid, form) {
        super::equipment::refresh_factors(ctx, st, sid);
    }
}

fn guild_master(st: &WorldState, cid: u32) -> bool {
    st.guilds.of_cid.get(&cid).and_then(|g| st.guilds.by_id.get(g)).and_then(|g| g.master()).is_some_and(|m| m.cid == cid)
}

fn candidate(st: &WorldState, sid: u32) -> Candidate {
    let p = &st.players[&sid];
    let f = &p.factors;
    let mut items: HashMap<u32, i64> = HashMap::new();
    for it in p.inventory.items.values() {
        *items.entry(it.tid).or_default() += it.count;
    }
    Candidate {
        level: p.ch.level as i64,
        race: f.ftype[0],
        class: f.ftype[2],
        guild_master: guild_master(st, p.ch.id),
        archlord: super::archlord::is_lord(st, sid),
        // Caractéristiques en centièmes dans les facteurs, sauf le charisme (points bruts).
        stats: [f.status[0] / 100, f.status[1] / 100, f.status[2] / 100, f.status[3] / 100, f.status[6] / 100, f.status[4]].map(i64::from),
        money: p.ch.money as i64,
        items,
    }
}

/// Fixe un titre comme objectif (6.0 → 6.1, 6.3) : contrôles, coûts consommés.
async fn add_goal(ctx: &WorldCtx, st: &mut WorldState, sid: u32, tid: u32) -> anyhow::Result<()> {
    let name = st.players[&sid].ch.name.clone();
    let why = match ctx.titles.get(tid) {
        None => Some("titre inconnu".to_string()),
        Some(def) => refusal(def, &candidate(st, sid), &st.players[&sid].titles),
    };
    if let Some(why) = why {
        info!("[titre] {name} ne peut pas viser le titre {tid} : {why}");
        send(st, sid, tid, Reply::QuestAddResult { accepted: 0, result: 0 });
        send_system(st, sid, &format!("Objectif impossible : {why}."));
        return Ok(());
    }
    let def = &ctx.titles.titles[&tid];
    // Objets de coût pris dans les piles de l'inventaire, autant qu'il en faut (présence déjà vérifiée).
    let p = st.players.get_mut(&sid).unwrap();
    let cid = p.ch.id;
    for &(item_tid, need) in &def.cost_items {
        let mut left = need;
        let piles: Vec<(u32, i64)> = p.inventory.items.values().filter(|i| i.tid == item_tid).map(|i| (i.iid, i.count)).collect();
        for (iid, count) in piles {
            if left == 0 {
                break;
            }
            let n = count.min(left);
            left -= n;
            match p.inventory.consume(&ctx.db, iid, n).await? {
                Some(rest) => p.send_raw(item_count(&rest, cid)),
                None => p.send_raw(item_remove(iid, Some(cid))),
            }
        }
    }
    if def.cost_geld > 0 {
        let money = p.ch.money as i64 - def.cost_geld;
        super::set_money(ctx, st, sid, money).await?;
    }
    let now = now_unix();
    let p = st.players.get_mut(&sid).unwrap();
    let mut goal = Goal { accepted_at: now, counters: [0; CHECKS] };
    advance(def, &mut goal.counters, Event::Refresh, &snapshot(ctx, p));
    let c = goal.counters;
    ctx.db
        .characters
        .exec(
            "INSERT INTO character_title_goals (character_id, title_tid, accepted_at, c1, c2, c3, c4, c5) VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
             ON DUPLICATE KEY UPDATE accepted_at = VALUES(accepted_at), c1 = VALUES(c1), c2 = VALUES(c2), c3 = VALUES(c3), c4 = VALUES(c4), c5 = VALUES(c5)",
            vec![mysql_async::Value::from(cid), tid.into(), now.into(), c[0].into(), c[1].into(), c[2].into(), c[3].into(), c[4].into()],
        )
        .await?;
    p.titles.goals.insert(tid, goal);
    send(st, sid, tid, Reply::QuestAddResult { accepted: now, result: 1 });
    if c.iter().any(|v| *v != 0) {
        send(st, sid, tid, Reply::QuestCheck { values: c });
    }
    info!("[titre] {name} vise le titre {tid} ({}) ; conditions {:?}", def.name, &c[..def.checks.len().min(CHECKS)]);
    Ok(())
}

/// Termine un objectif (6.4 → 6.5 puis op 1).
async fn complete_goal(ctx: &WorldCtx, st: &mut WorldState, sid: u32, tid: u32) -> anyhow::Result<()> {
    let p = &st.players[&sid];
    let (cid, name) = (p.ch.id, p.ch.name.clone());
    let why = match (ctx.titles.get(tid), p.titles.goals.get(&tid)) {
        (None, _) => Some("titre inconnu".to_string()),
        (_, None) => Some("pas un objectif en cours".to_string()),
        (Some(def), Some(g)) if !complete(def, &values(ctx, p, def, g)) => Some("conditions pas encore remplies".to_string()),
        _ if p.titles.owned.len() >= MAX_TITLES => Some(format!("{MAX_TITLES} titres au plus")),
        _ => None,
    };
    if let Some(why) = why {
        info!("[titre] {name} ne peut pas obtenir le titre {tid} : {why}");
        send(st, sid, tid, Reply::QuestCompleteResult { completed: 0, result: 0 });
        send_system(st, sid, &format!("Titre impossible à obtenir : {why}."));
        return Ok(());
    }
    let now = now_unix();
    ctx.db
        .characters
        .atomic(vec![
            ("DELETE FROM character_title_goals WHERE character_id = ? AND title_tid = ?", (cid, tid).into()),
            ("INSERT IGNORE INTO character_titles (character_id, title_tid, active, skill_tid, acquired_at) VALUES (?, ?, 0, 0, ?)", (cid, tid, now).into()),
        ])
        .await?;
    let p = st.players.get_mut(&sid).unwrap();
    p.titles.goals.remove(&tid);
    p.titles.owned.insert(tid, Owned { skill: 0, acquired_at: now });
    // 6.5 retire l'objectif sans ajouter le titre ; l'op 1 l'ajoute (« You received the title of %s. »).
    send(st, sid, tid, Reply::QuestCompleteResult { completed: now, result: 1 });
    send(st, sid, tid, Reply::AddResult { result: 1, skill: 0 });
    info!("[titre] {name} obtient le titre {tid} ({})", ctx.titles.get(tid).map(|d| d.name.as_str()).unwrap_or("?"));
    Ok(())
}

/// Supprime un titre possédé ou abandonne un objectif (op 7 → op 9 si actif, op 8). Coûts non rendus.
async fn delete(ctx: &WorldCtx, st: &mut WorldState, sid: u32, tid: u32) -> anyhow::Result<()> {
    let p = &st.players[&sid];
    let (cid, name) = (p.ch.id, p.ch.name.clone());
    if p.titles.owned.contains_key(&tid) {
        let was_active = p.titles.active == Some(tid);
        ctx.db.characters.exec("DELETE FROM character_titles WHERE character_id = ? AND title_tid = ?", (cid, tid)).await?;
        let p = st.players.get_mut(&sid).unwrap();
        p.titles.owned.remove(&tid);
        if was_active {
            p.titles.active = None;
            send_near(st, sid, tid, false);
            refresh_skills(ctx, st, sid);
        }
        info!("[titre] {name} supprime le titre {tid}{}", if was_active { " (actif)" } else { "" });
    } else if p.titles.goals.contains_key(&tid) {
        ctx.db.characters.exec("DELETE FROM character_title_goals WHERE character_id = ? AND title_tid = ?", (cid, tid)).await?;
        st.players.get_mut(&sid).unwrap().titles.goals.remove(&tid);
        info!("[titre] {name} abandonne l'objectif {tid}");
    } else {
        info!("[titre] {name} supprime le titre {tid}, ni possédé ni visé");
        send(st, sid, tid, Reply::DeleteResult { result: 0 });
        return Ok(());
    }
    send(st, sid, tid, Reply::DeleteResult { result: 1 });
    Ok(())
}

/// Fait avancer les objectifs du joueur après `ev` : compteurs enregistrés et envoyés (6.3) s'ils ont changé.
pub async fn progress(ctx: &WorldCtx, st: &mut WorldState, sid: u32, ev: Event) -> anyhow::Result<()> {
    let Some(p) = st.players.get_mut(&sid) else { return Ok(()) };
    if p.titles.goals.is_empty() {
        return Ok(());
    }
    let snap = snapshot(ctx, p);
    let cid = p.ch.id;
    let mut changed = Vec::new();
    for (tid, g) in p.titles.goals.iter_mut() {
        if ctx.titles.get(*tid).is_some_and(|def| advance(def, &mut g.counters, ev, &snap)) {
            changed.push((*tid, g.counters));
        }
    }
    for (tid, c) in changed {
        ctx.db.characters.exec(SAVE_COUNTERS, vec![mysql_async::Value::from(c[0]), c[1].into(), c[2].into(), c[3].into(), c[4].into(), cid.into(), tid.into()]).await?;
        send(st, sid, tid, Reply::QuestCheck { values: c });
        debug!("[titre] #{sid} objectif {tid} : {c:?}");
    }
    Ok(())
}

/// Récolte réussie sur un cadavre de `monster_tid` (`product.rs`).
pub async fn on_collect(ctx: &WorldCtx, st: &mut WorldState, sid: u32, monster_tid: u32) -> anyhow::Result<()> {
    progress(ctx, st, sid, Event::Collect(monster_tid)).await
}

/// Monstre `monster_tid` tué par `sid` (`combat.rs`) : Natural Enemy, et récolte avec `Title.CollectCountsKills`.
pub async fn on_kill(ctx: &WorldCtx, st: &mut WorldState, sid: u32, monster_tid: u32) -> anyhow::Result<()> {
    progress(ctx, st, sid, Event::Kill(monster_tid)).await?;
    if !ctx.config.world.title_collect_counts_kills {
        return Ok(());
    }
    progress(ctx, st, sid, Event::Collect(monster_tid)).await
}

/// Joueur tué par `sid` (`pvp.rs`) : Killer.
pub async fn on_pk(ctx: &WorldCtx, st: &mut WorldState, sid: u32) -> anyhow::Result<()> {
    progress(ctx, st, sid, Event::Pk).await
}

/// Quête terminée par `sid` (`quests.rs`) : Storyteller.
pub async fn on_quest(ctx: &WorldCtx, st: &mut WorldState, sid: u32) -> anyhow::Result<()> {
    progress(ctx, st, sid, Event::Quest).await
}

/// Butin d'un monstre ramassé par `sid` (`loot.rs`) : Collector.
pub async fn on_loot(ctx: &WorldCtx, st: &mut WorldState, sid: u32, item_tid: u32, count: i32) -> anyhow::Result<()> {
    progress(ctx, st, sid, Event::Loot(item_tid, count)).await
}

/// Objet `item_tid` utilisé (`recovery.rs`).
pub async fn on_item_used(ctx: &WorldCtx, st: &mut WorldState, sid: u32, item_tid: u32) -> anyhow::Result<()> {
    progress(ctx, st, sid, Event::UseItem(item_tid)).await
}

/// Niveau ou niveau de production changé (`combat.rs`, `product.rs`).
pub async fn refresh(ctx: &WorldCtx, st: &mut WorldState, sid: u32) -> anyhow::Result<()> {
    progress(ctx, st, sid, Event::Refresh).await
}

/// Réussite de production en plus (pour cent) du titre équipé : catégorie 0 Collecte (effet 5), 1 Cuisine (4), 2
/// Alchimie (3).
pub fn product_bonus(ctx: &WorldCtx, p: &Player, category: i64) -> i64 {
    let kind = match category {
        0 => effect::GATHERING,
        1 => effect::COOKING,
        2 => effect::ALCHEMY,
        _ => return 0,
    };
    p.titles.active_def(&ctx.titles).map_or(0, |d| d.bonus(kind))
}

/// Multiplicateur des objets récoltés sur `monster_tid` par le titre équipé (effet 7).
pub fn collect_factor(ctx: &WorldCtx, p: &Player, monster_tid: u32) -> i64 {
    p.titles.active_def(&ctx.titles).map_or(1, |d| d.collect_factor(monster_tid))
}

/// Expérience en plus (pour cent) de la créature `monster_tid` par le titre équipé (effet 8).
pub fn kill_exp_bonus(ctx: &WorldCtx, p: &Player, monster_tid: u32) -> i64 {
    p.titles.active_def(&ctx.titles).map_or(0, |d| d.bonus_on(effect::KILL_EXP, monster_tid))
}

/// Réussite du sertissage de la pierre `stone_tid` en plus (points de pour cent) par le titre équipé (effet 6).
pub fn refine_bonus(ctx: &WorldCtx, p: &Player, stone_tid: u32) -> i64 {
    p.titles.active_def(&ctx.titles).map_or(0, |d| d.bonus_on(effect::REFINE, stone_tid))
}

/// Expérience et or des récompenses de quête en plus (pour cent) par le titre équipé (effets 1 et 2).
pub fn quest_bonus(ctx: &WorldCtx, p: &Player) -> (i64, i64) {
    p.titles.active_def(&ctx.titles).map_or((0, 0), |d| (d.bonus(effect::QUEST_EXP), d.bonus(effect::QUEST_GOLD)))
}

/// Récupération en plus de la potion `item_tid` (qui rend `amount`) par le titre équipé (effet 9).
pub fn potion_extra(ctx: &WorldCtx, p: &Player, item_tid: u32, amount: i32) -> i32 {
    p.titles.active_def(&ctx.titles).map_or(0, |d| d.potion_extra(item_tid, amount))
}

/// État des titres d'un joueur en jeu, pour la commande MJ `/title`.
pub fn describe(ctx: &WorldCtx, st: &WorldState, sid: u32) -> String {
    let Some(p) = st.players.get(&sid) else { return "Personnage absent du monde.".into() };
    let t = &p.titles;
    let label = |tid: &u32| ctx.titles.get(*tid).map(|d| format!("{tid} {}", d.name)).unwrap_or_else(|| format!("{tid} (inconnu)"));
    let cha = super::skills::charisma_points(&p.factors) as i64;
    let rank = ctx.titles.charisma_rank(cha).map(|(i, n)| format!("rang {i}{}", if n.is_empty() { String::new() } else { format!(" « {n} »") })).unwrap_or_else(|| "rang inconnu".into());
    let mut lines = vec![format!(
        "{} : titre actif {} ; charisme {cha} ({rank}) ; {} titre(s), {} objectif(s)",
        p.ch.name,
        t.active.as_ref().map(label).unwrap_or_else(|| "aucun".into()),
        t.owned.len(),
        t.goals.len()
    )];
    for tid in t.owned.keys() {
        lines.push(format!("· titre {}", label(tid)));
    }
    for (tid, g) in &t.goals {
        let shown = ctx.titles.get(*tid).map(|d| format!("{:?} sur {:?}", &values(ctx, p, d, g)[..d.checks.len().min(CHECKS)], d.checks.iter().map(|c| c.value).collect::<Vec<_>>()));
        lines.push(format!("· objectif {} : {}", label(tid), shown.unwrap_or_default()));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use archcore_net::{decode_packet, parse_header, schema};

    fn i32_at(b: &[u8], o: usize) -> i32 {
        get32(b, o).unwrap()
    }

    fn i64_at(b: &[u8], o: usize) -> i64 {
        i64::from_le_bytes(b[o..o + 8].try_into().unwrap())
    }

    /// Demande comme l'envoie Archonia (`FUN_1404a66a0`) : structure de `size` octets, pied, longueur = taille + 1.
    fn request(op: u16, size: usize, tid: u32, fill: impl FnOnce(&mut [u8])) -> Vec<u8> {
        let mut b = super::super::shrine::raw_packet_of(ptype::TITLE, op as i64, size, fill);
        super::super::guild::society::put_text(&mut b, NAME, NAME_LEN, "Sim");
        b[TID..TID + 4].copy_from_slice(&tid.to_le_bytes());
        b
    }

    fn knight() -> TitleDef {
        TitleDef {
            tid: 140_001,
            category: "Advanced".into(),
            name: "Novice Human Knight".into(),
            require: Require { level: 15, class: 101, ..Default::default() },
            checks: vec![Check { kind: check::LEVEL, set: 0, value: 15 }],
            skills: vec![1555],
            ..Default::default()
        }
    }

    fn collector() -> TitleDef {
        TitleDef {
            tid: 10_001,
            checks: vec![Check { kind: check::COLLECT, set: 5, value: 50 }],
            effects: vec![TitleEffect { kind: effect::DOUBLE_COLLECT, set: 0, v1: 5, v2: 2 }],
            ..Default::default()
        }
    }

    #[test]
    fn tailles_et_offsets_des_reponses() {
        let cases = [
            (Reply::AddResult { result: 1, skill: 0 }, 0x55, 1),
            (Reply::UseResult { on: true, result: 1, time: 1_761_955_200, code: 0 }, 0x65, 3),
            (Reply::ListEntry { changed_at: 5, valid: true, skill: 9 }, 0x69, 5),
            (Reply::QuestAddResult { accepted: 7, result: 1 }, 0x5D, 6),
            (Reply::QuestCheck { values: [15, 0, 0, 0, 0] }, 0x69, 6),
            (Reply::QuestCompleteResult { completed: 8, result: 1 }, 0x5D, 6),
            (Reply::QuestEntry { accepted: 7, counters: [1, 2, 3, 4, 5] }, 0x85, 6),
            (Reply::DeleteResult { result: 1 }, 0x51, 8),
            (Reply::UseNear { on: true }, 0x51, 9),
            (Reply::UiOpen, 0x4D, 10),
        ];
        for (r, len, op) in cases {
            let b = packet("Sim", 140_003, &r);
            assert_eq!((b.len(), u32::from_le_bytes(b[1..5].try_into().unwrap()) as usize), (len, len), "{r:?}");
            assert_eq!((b[0], b[5], b[0x13], *b.last().unwrap()), (0xd6, 0x60, 1, 0x6b));
            assert!(b[6..0x13].iter().all(|&x| x == 0));
            assert_eq!(parse_header(&b).map(|h| (h.ptype, h.size as usize)), Some((0x60, len)));
            assert_eq!(decode_packet(&schema::TITLE, &b).unwrap().int(0), Some(op), "{r:?}");
            assert_eq!((&b[0x16..0x1A], i32_at(&b, TID)), (&b"Sim\0"[..], 140_003));
        }
        let b = packet("Sim", 1, &Reply::UseResult { on: false, result: 0, time: 0x1122_3344, code: CODE_TOO_SOON });
        assert_eq!((i32_at(&b, 0x4C), i32_at(&b, 0x50), i64_at(&b, 0x58), i32_at(&b, 0x60)), (0, 0, 0x1122_3344, 3));
        let b = packet("Sim", 1, &Reply::ListEntry { changed_at: 5, valid: true, skill: 9 });
        assert_eq!((i64_at(&b, 0x50), i32_at(&b, 0x5C), i32_at(&b, 0x60), i32_at(&b, 0x64)), (5, 1, 0, 9));
        let b = packet("Sim", 1, &Reply::QuestEntry { accepted: 7, counters: [1, 2, 3, 4, 5] });
        assert_eq!((i32_at(&b, SUB), i32_at(&b, 0x50), i32_at(&b, 0x54), i32_at(&b, 0x58), i64_at(&b, 0x60), i64_at(&b, 0x68)), (7, 1, 0, 0, 7, 0));
        assert_eq!((i32_at(&b, 0x70), i32_at(&b, 0x80)), (1, 5));
        let b = packet("Sim", 1, &Reply::QuestCheck { values: [50, 0, 0, 0, 0] });
        assert_eq!((i32_at(&b, SUB), i32_at(&b, 0x50), i32_at(&b, 0x64)), (3, 50, 1));
        let b = packet("Sim", 1, &Reply::QuestCompleteResult { completed: 9, result: 1 });
        assert_eq!((i32_at(&b, SUB), i64_at(&b, 0x50), i32_at(&b, 0x58)), (5, 9, 1));
        // Nom de 48 caractères au plus (CHAR[49]).
        let b = packet(&"x".repeat(60), 1, &Reply::UiOpen);
        assert_eq!((b[0x16 + 47], b[0x16 + 48]), (b'x', 0));
    }

    #[test]
    fn demandes_du_client() {
        // Équiper le titre 140001 (`FUN_1405f4ce0`) : longueur 0x55, 2 en 0x4C, 1 en 0x50.
        let on = request(2, 0x54, 140_001, |b| {
            b[0x4C] = 2;
            b[0x50] = 1;
        });
        assert_eq!(on.len(), 0x55);
        assert_eq!(decode(&on), Some(("Sim".into(), 140_001, Request::Use { on: true })));
        assert_eq!(decode(&request(2, 0x54, 7, |b| b[0x4C] = 2)).map(|d| d.2), Some(Request::Use { on: false }));
        assert_eq!(decode(&request(6, 0x50, 10_001, |_| {})).map(|d| d.2), Some(Request::QuestAdd));
        assert_eq!(decode(&request(6, 0x54, 10_001, |b| b[0x4C] = 4)).map(|d| d.2), Some(Request::QuestComplete { skill: 0 }));
        assert_eq!(decode(&request(6, 0x50, 0, |b| b[0x4C] = 6)).map(|d| d.2), Some(Request::QuestList));
        assert_eq!(decode(&request(7, 0x4C, 40_001, |_| {})).map(|d| (d.1, d.2)), Some((40_001, Request::Delete)));
        assert_eq!(decode(&request(4, 0x4C, 0, |_| {})).map(|d| d.2), Some(Request::List));
        assert_eq!(decode(&request(11, 0x50, 36, |b| b[0x4C] = 7)).map(|d| d.2), Some(Request::SkillSelect { skill: 7 }));
        // Rejets : sous-opération 2, op du serveur, structure trop courte, autre type.
        assert_eq!(decode(&request(6, 0x50, 1, |b| b[0x4C] = 2)), None);
        assert_eq!(decode(&request(9, 0x50, 1, |_| {})), None);
        let mut short = request(2, 0x54, 1, |_| {});
        short.truncate(0x52);
        assert_eq!(decode(&short), None);
        let mut other = request(7, 0x4C, 1, |_| {});
        other[5] = 0x62;
        assert_eq!(decode(&other), None);
    }

    #[test]
    fn conditions_de_niveau_et_de_collecte() {
        let (k, c) = (knight(), collector());
        let s = Snapshot { level: 14, product: [0; 3] };
        let mut counters = [0; CHECKS];
        assert!(advance(&k, &mut counters, Event::Refresh, &s));
        assert_eq!(counters[0], 14);
        assert!(!complete(&k, &counters));
        assert!(!advance(&k, &mut counters, Event::Collect(5), &s), "rien ne change");
        assert!(advance(&k, &mut counters, Event::Refresh, &Snapshot { level: 16, ..s }));
        assert!(complete(&k, &counters));
        let mut counters = [49, 0, 0, 0, 0];
        assert!(!advance(&c, &mut counters, Event::Collect(10), &s), "autre créature");
        assert!(!advance(&c, &mut counters, Event::UseItem(5), &s), "objet, pas créature");
        assert!(advance(&c, &mut counters, Event::Collect(5), &s));
        assert!(complete(&c, &counters));
        // Addict (objet 190, 150 fois) et Researcher (Alchimie, catégorie 2, niveau 10).
        let addict = TitleDef { checks: vec![Check { kind: check::USE_ITEM, set: 190, value: 150 }], ..Default::default() };
        let mut counters = [0; CHECKS];
        assert!(advance(&addict, &mut counters, Event::UseItem(190), &s));
        assert_eq!(counters[0], 1);
        let alchemy = TitleDef { checks: vec![Check { kind: check::ALCHEMY, set: 0, value: 10 }], ..Default::default() };
        let mut counters = [0; CHECKS];
        advance(&alchemy, &mut counters, Event::Refresh, &Snapshot { level: 1, product: [3, 5, 10] });
        assert!(complete(&alchemy, &counters));
        // Natural Enemy (créature 33, 6 fois), Collector (objet 463 ramassé, 30), Killer et Storyteller.
        let enemy = TitleDef { checks: vec![Check { kind: check::KILL, set: 33, value: 6 }], ..Default::default() };
        let mut counters = [5, 0, 0, 0, 0];
        assert!(!advance(&enemy, &mut counters, Event::Collect(33), &s), "récolte, pas mort");
        assert!(advance(&enemy, &mut counters, Event::Kill(33), &s) && complete(&enemy, &counters));
        let stones = TitleDef { checks: vec![Check { kind: check::COLLECT_ITEM, set: 463, value: 30 }], ..Default::default() };
        let mut counters = [0; CHECKS];
        assert!(advance(&stones, &mut counters, Event::Loot(463, 3), &s) && counters[0] == 3);
        assert!(!advance(&stones, &mut counters, Event::Loot(466, 1), &s));
        let story = TitleDef { checks: vec![Check { kind: check::QUEST, set: 0, value: 100 }, Check { kind: check::PK, set: 0, value: 30 }], ..Default::default() };
        let mut counters = [0; CHECKS];
        assert!(advance(&story, &mut counters, Event::Quest, &s) && advance(&story, &mut counters, Event::Pk, &s));
        assert_eq!(counters[..2], [1, 1]);
        // Types d'Archonia ramenés aux TID de Webzen.
        assert_eq!([2, 5, 15, 25, 26, 27, 30_001].map(check::normalize), [10_001, 40_001, 140_001, 200_001, 200_002, 200_003, 30_001]);
        // Sans condition : terminable aussitôt.
        assert!(complete(&TitleDef::default(), &[0; CHECKS]));
    }

    #[test]
    fn classes_exigees() {
        assert!(class_ok(0, 3, 2));
        assert!(class_ok(101, 1, 1) && !class_ok(101, 1, 2) && !class_ok(101, 2, 1));
        assert!(class_ok(301, 1, 4) && class_ok(401, 2, 1) && class_ok(801, 3, 2) && class_ok(901, 3, 4));
        assert!(class_ok(1001, 4, 3) && class_ok(1001, 4, 1));
        assert!(class_ok(1101, 4, 1) && class_ok(1201, 4, 2) && class_ok(1301, 4, 4) && !class_ok(1301, 4, 3));
        assert!(!class_ok(1401, 4, 1));
    }

    #[test]
    fn refus_pour_fixer_l_objectif() {
        let k = knight();
        let titles = PlayerTitles::default();
        let me = Candidate { level: 15, race: 1, class: 1, ..Default::default() };
        assert_eq!(refusal(&k, &me, &titles), None);
        assert!(refusal(&k, &Candidate { level: 14, ..me.clone() }, &titles).unwrap().contains("niveau 15"));
        assert!(refusal(&k, &Candidate { class: 2, ..me.clone() }, &titles).unwrap().contains("classe"));
        let next = TitleDef { tid: 140_002, first_titles: vec![140_001], cost_geld: 100, cost_items: vec![(14_531, 2)], ..k.clone() };
        assert!(refusal(&next, &me, &titles).unwrap().contains("or"));
        let rich = Candidate { money: 100, items: [(14_531, 2)].into_iter().collect(), ..me.clone() };
        assert!(refusal(&next, &rich, &titles).unwrap().contains("titre 140001"));
        let mut owner = PlayerTitles::default();
        owner.owned.insert(140_001, Owned::default());
        assert_eq!(refusal(&next, &rich, &owner), None);
        assert!(refusal(&k, &me, &owner).unwrap().contains("déjà obtenu"));
        owner.goals.insert(140_002, Goal::default());
        assert!(refusal(&next, &rich, &owner).unwrap().contains("objectif"));
        let gm = TitleDef { require: Require { guild_master: true, archlord: true, ..Default::default() }, ..Default::default() };
        assert!(refusal(&gm, &me, &titles).unwrap().contains("ArchLord"));
        // Bornes du client : 500 titres et objectifs ensemble.
        let mut full = PlayerTitles::default();
        for t in 0..500 {
            full.owned.insert(t + 1, Owned::default());
        }
        assert!(refusal(&TitleDef { tid: 9_999, ..Default::default() }, &me, &full).unwrap().contains("500"));
    }

    #[test]
    fn effets_du_titre() {
        let c = collector();
        assert_eq!((c.collect_factor(5), c.collect_factor(10)), (2, 1));
        let addict = TitleDef {
            effects: vec![TitleEffect { kind: effect::POTION, set: 0, v1: 190, v2: 25 }, TitleEffect { kind: effect::POTION, set: 1, v1: 1125, v2: 10 }],
            ..Default::default()
        };
        assert_eq!((addict.potion_extra(190, 75), addict.potion_extra(1125, 150), addict.potion_extra(193, 45)), (25, 15, 0));
        let alchemy = TitleDef { effects: vec![TitleEffect { kind: effect::ALCHEMY, set: 0, v1: 10, v2: 0 }], ..Default::default() };
        assert_eq!((alchemy.bonus(effect::ALCHEMY), alchemy.bonus(effect::GATHERING)), (10, 0));
        let enemy = TitleDef { effects: vec![TitleEffect { kind: effect::KILL_EXP, set: 0, v1: 33, v2: 10 }, TitleEffect { kind: effect::REFINE, set: 0, v1: 463, v2: 3 }], ..Default::default() };
        assert_eq!((enemy.bonus_on(effect::KILL_EXP, 33), enemy.bonus_on(effect::KILL_EXP, 450), enemy.bonus_on(effect::REFINE, 463)), (10, 0, 3));
    }

    #[test]
    fn rangs_de_charisme() {
        let cat = TitleCatalog { charisma: vec![(0, 99, String::new()), (100, 199, "Novice".into()), (200, 399, "Tough".into())], ..Default::default() };
        assert_eq!(cat.charisma_rank(0), Some((0, "")));
        assert_eq!(cat.charisma_rank(150), Some((1, "Novice")));
        assert_eq!(cat.charisma_rank(399), Some((2, "Tough")));
        assert_eq!(cat.charisma_rank(400), None);
    }
}
