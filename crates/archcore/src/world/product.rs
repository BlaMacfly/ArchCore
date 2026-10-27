//! Production : récolte sur cadavre (Collection Skill), cuisine et alchimie (recettes), compétences de production.
//!
//! ```text
//!   Données : product_category (compétence de chaque catégorie : 0 Hunt 663, 1 Cook 203, 2 Alchemy 285),
//!             product_compose (recettes), product_exp (niveaux 1 à 15), product_gather (monstres récoltables),
//!             product_result_item (objets récoltés) : tables Product*.txt du client (`archcore import`). Outil exigé
//!             par une compétence : `arg1` de SKILL_SPEC quand `requirement` vaut 4 (654, 655, 1016).
//!   PNJ (AGPMEVENTPRODUCT 0x3A, fonction 24 ; donnée = catégorie : 0 Adventurer, 1 Cook, 2 Alchemist) :
//!     client → op 0 (événement) ← op 1 : ouvre UI_PDSkillBuy
//!     client → op 2 (a = TID de la compétence) ← AGPMSKILL op 0 puis op 3 (b : 0 apprise, 2 déjà apprise,
//!             3 or insuffisant, 4 niveau trop bas, 1 autre refus)
//!     client → op 4 (a = recette) ← op 5 (b : 0 achetée, 2 déjà connue, 3 or insuffisant, 4 niveau de compétence
//!             trop bas, 5 compétence de la catégorie absente, 1 autre refus)
//!   Récolte : AGPMSKILL op 7 de Collection Skill sur un cadavre ← op 7, action 1 (lancement) ou refus (action 16
//!             cible invalide, 17 plus rien à récolter) ; le client montre une barre de 5,4 s puis envoie
//!             AGPMPRODUCT op 2 (champ 3 : cadavre) ← op 3.
//!   Fabrication : Cooking et Alchemy ne se lancent pas ; la fenêtre UI_Compose envoie AGPMPRODUCT op 0 (champ 3
//!             recette, champ 4 IID de l'objet recette ou 0) ← op 3.
//!   Résultat (op 3) : champ 3 TID obtenu, 4 quantité, 5 code, 6 expérience gagnée. Codes : 0 réussite, 1 échec,
//!             2 outil absent, 3 cible invalide, 4 trop loin, 5 inventaire plein, 6 matériaux insuffisants,
//!             7 plus rien à récolter.
//!   Entrée en jeu : op 1 (champ 7 : recettes connues, INT32 bout à bout).
//!   Niveau d'une compétence de production : facteur « dirt » 4 d'AGPMSKILL ; expérience cumulée : facteur 6.
//! ```
//! Le serveur d'origine n'a pas laissé ses règles ; celles d'ArchCore, choisies d'après les données :
//! - réussite si un tirage dans 0..100 est sous min(100, chance de la ligne + facteur de product_exp au niveau
//!   courant) ; la chance est Probability (récolte), SucessProb plus Recipe_addProb si l'objet recette est fourni
//!   (fabrication) ;
//! - récolte : objet tiré selon les poids du bloc (ensemble, niveau de compétence) de product_result_item, quantité 1 ;
//!   un cadavre ne se récolte qu'une fois, tous joueurs confondus ; le lancement le réserve à son lanceur et le
//!   garde visible `HARVEST_HOLD` de plus (le client met environ 9 s, le cadavre disparaît 20 s après la mort) ;
//!   inventaire plein : code 5, le cadavre reste à récolter ;
//! - fabrication : matériaux consommés en cas de réussite comme d'échec (quantités additionnées quand un matériau
//!   revient), objet recette consommé s'il est fourni et valide (sinon ignoré, sans bonus), produit 1 × Result_TID ;
//!   recette inconnue, d'une autre catégorie, d'un niveau trop haut ou exigeant un titre (ArchCore n'en gère pas) :
//!   code 1 sans rien consommer, avec un message système ;
//! - expérience tirée dans [min, max] de product_exp au niveau courant, sur une réussite seulement ; niveau suivant
//!   quand l'expérience cumulée atteint le cumul du niveau (15 au plus, expérience bornée au cumul final) ;
//! - apprentissage d'une compétence : niveau et prix du niveau 1 de SKILL_CONST, sans point de compétence ni taxe ;
//!   achat d'une recette : MakingPrice, compétence de la catégorie au niveau ItemLv.

use super::items::{item_add_of, item_count, item_remove, item_status};
use super::monsters::Life;
use super::npc_events::{npc_distance, resolve};
use super::npcs::event_function;
use super::skills::{self, Cast, Owned, Stage};
use super::{send_system, Player, WorldCtx, WorldState};
use crate::db::{col, Databases};
use crate::{debug, info, warn};
use archcore_net::schema::{self, mini};
use archcore_net::{build_mini, build_packet, decode_mini, Fields, Packet, Vec3};
use rand::Rng;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Catégorie de la récolte (les autres, 1 cuisine et 2 alchimie, fabriquent).
pub const CATEGORY_HUNT: i64 = 0;
/// Niveau maximal d'une compétence de production (product_exp).
pub const MAX_LEVEL: u32 = 15;
/// Distance au sol (x, z) jusqu'à laquelle on récolte un cadavre, comme le client.
pub const GATHER_RANGE: f64 = 250.0;
/// Marge ajoutée aux distances, pour les positions arrondies du client.
const REACH_TOLERANCE: f64 = 50.0;
/// Distance au PNJ : le client s'en approche à 1600 unités avant d'envoyer sa demande.
const NPC_RANGE: f64 = 1700.0;
/// Durée de la réservation d'un cadavre par le lancement de la récolte, et de sa prolongation.
pub const HARVEST_HOLD: Duration = Duration::from_secs(15);

pub mod op {
    pub const COMPOSE: i64 = 0;
    pub const COMPOSE_INFO: i64 = 1;
    pub const GATHER: i64 = 2;
    pub const RESULT: i64 = 3;
}

/// Codes de résultat (AGPMPRODUCT op 3, champ 5).
pub mod result {
    pub const SUCCESS: i64 = 0;
    pub const FAIL: i64 = 1;
    pub const NO_TOOL: i64 = 2;
    pub const INVALID_TARGET: i64 = 3;
    pub const TOO_FAR: i64 = 4;
    pub const FULL_INVENTORY: i64 = 5;
    pub const NO_MATERIAL: i64 = 6;
    pub const NOTHING_LEFT: i64 = 7;
}

pub mod event_op {
    pub const REQUEST: i64 = 0;
    pub const GRANT: i64 = 1;
    pub const LEARN_SKILL: i64 = 2;
    pub const LEARN_SKILL_RESULT: i64 = 3;
    pub const BUY_COMPOSE: i64 = 4;
    pub const BUY_COMPOSE_RESULT: i64 = 5;
}

/// Codes des op 3 et 5 d'AGPMEVENTPRODUCT (ceux du contrôle du client).
pub mod learn_result {
    pub const SUCCESS: i64 = 0;
    pub const FAIL: i64 = 1;
    pub const ALREADY: i64 = 2;
    pub const NO_MONEY: i64 = 3;
    pub const LOW_LEVEL: i64 = 4;
    pub const NO_SKILL: i64 = 5;
}

/// Refus du lancement de la récolte (type d'action du mini SKILL_ACTION).
pub mod cast_refusal {
    pub const INVALID_TARGET: i64 = 16;
    pub const NOTHING_LEFT: i64 = 17;
}

/// Une recette (product_compose).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Compose {
    pub result_tid: u32,
    pub category: i64,
    pub name: String,
    pub title_tid: u32,
    pub item_level: u32,
    pub price: i64,
    pub success_rate: i64,
    pub recipe_tid: u32,
    pub recipe_bonus: i64,
    /// Matériaux (TID, quantité), un matériau répété additionné.
    pub materials: Vec<(u32, i64)>,
}

/// Un niveau d'une compétence de production (product_exp).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ExpLevel {
    pub min: u32,
    pub max: u32,
    /// Ajouté à la chance de réussite.
    pub rate_bonus: i64,
    pub level_exp: u32,
    /// Expérience cumulée à la fin du niveau.
    pub total: u32,
}

/// Un monstre récoltable (product_gather).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Gather {
    /// Bit n : classe n (1 guerrier, 2 archer, 3 scion, 4 mage).
    pub classes: u8,
    pub skill_tid: u32,
    pub rate: i64,
    pub item_set: i64,
}

#[derive(Default)]
pub struct ProductCatalog {
    /// Compétence → catégorie, et l'inverse.
    pub categories: HashMap<u32, i64>,
    pub skill_of: HashMap<i64, u32>,
    /// Compétence → outil exigé.
    pub tools: HashMap<u32, u32>,
    pub composes: HashMap<u32, Compose>,
    /// Compétence → niveaux (niveau n à l'indice n - 1).
    pub exp: HashMap<u32, Vec<ExpLevel>>,
    /// TID du monstre → récolte.
    pub gather: HashMap<u32, Gather>,
    /// (ensemble, niveau) → objets (TID, poids).
    pub results: HashMap<(i64, u32), Vec<(u32, i64)>>,
}

/// Matériaux additionnés par TID, dans l'ordre de leur première mention.
pub fn sum_materials(list: &[(u32, i64)]) -> Vec<(u32, i64)> {
    let mut out: Vec<(u32, i64)> = Vec::new();
    for &(tid, n) in list {
        match out.iter_mut().find(|(t, _)| *t == tid) {
            Some(e) => e.1 += n,
            None => out.push((tid, n)),
        }
    }
    out
}

impl ProductCatalog {
    pub async fn load(db: &Databases) -> anyhow::Result<Self> {
        let mut c = ProductCatalog::default();
        let Ok(rows) = db.world.rows("SELECT category, skill_tid FROM product_category", ()).await else {
            warn!("production non chargée : lancer `archcore db-init` puis `archcore import`");
            return Ok(c);
        };
        for r in &rows {
            let (category, tid): (i64, u32) = (col(r, "category"), col(r, "skill_tid"));
            c.categories.insert(tid, category);
            c.skill_of.insert(category, tid);
        }
        // Outil : SKILL_SPEC `requirement` 4 (EQUIP_PRODUCT_TOOL), objet `arg1`.
        for r in db.world.rows("SELECT tid, spec FROM skill_spec", ()).await? {
            let tid: u32 = col(&r, "tid");
            if !c.categories.contains_key(&tid) {
                continue;
            }
            let spec = serde_json::from_str::<serde_json::Value>(&col::<String>(&r, "spec")).unwrap_or_default();
            let int = |k: &str| spec.get(k).and_then(|v| v.as_str()).and_then(|s| s.trim().parse::<u32>().ok()).unwrap_or(0);
            if int("requirement") == 4 && int("arg1") > 0 {
                c.tools.insert(tid, int("arg1"));
            }
        }
        for r in db.world.rows("SELECT result_tid, category, name, title_tid, item_level, price, success_rate, recipe_tid, recipe_bonus, materials FROM product_compose", ()).await? {
            let materials: Vec<(u32, i64)> = serde_json::from_str(&col::<String>(&r, "materials")).unwrap_or_default();
            let tid: u32 = col(&r, "result_tid");
            c.composes.insert(
                tid,
                Compose {
                    result_tid: tid,
                    category: col(&r, "category"),
                    name: col(&r, "name"),
                    title_tid: col(&r, "title_tid"),
                    item_level: col::<i64>(&r, "item_level").max(1) as u32,
                    price: col(&r, "price"),
                    success_rate: col(&r, "success_rate"),
                    recipe_tid: col(&r, "recipe_tid"),
                    recipe_bonus: col(&r, "recipe_bonus"),
                    materials: sum_materials(&materials),
                },
            );
        }
        for r in db.world.rows("SELECT skill_tid, level, exp_min, exp_max, rate_bonus, level_exp, total_exp FROM product_exp ORDER BY skill_tid, level", ()).await? {
            let list = c.exp.entry(col(&r, "skill_tid")).or_default();
            if col::<i64>(&r, "level") as usize == list.len() + 1 {
                let n = |k: &str| col::<i64>(&r, k).max(0) as u32;
                list.push(ExpLevel { min: n("exp_min"), max: n("exp_max"), rate_bonus: col(&r, "rate_bonus"), level_exp: n("level_exp"), total: n("total_exp") });
            }
        }
        for r in db.world.rows("SELECT monster_tid, classes, skill_tid, rate, item_set FROM product_gather", ()).await? {
            c.gather.insert(col(&r, "monster_tid"), Gather { classes: col(&r, "classes"), skill_tid: col(&r, "skill_tid"), rate: col(&r, "rate"), item_set: col(&r, "item_set") });
        }
        for r in db.world.rows("SELECT item_set, skill_level, item_tid, weight FROM product_result_item ORDER BY item_set, skill_level, item_tid", ()).await? {
            c.results.entry((col(&r, "item_set"), col(&r, "skill_level"))).or_default().push((col(&r, "item_tid"), col(&r, "weight")));
        }
        info!(
            "production : {} compétence(s), {} recette(s), {} monstre(s) récoltable(s), {} bloc(s) d'objets",
            c.categories.len(),
            c.composes.len(),
            c.gather.len(),
            c.results.len()
        );
        Ok(c)
    }

    pub fn category(&self, skill_tid: u32) -> Option<i64> {
        self.categories.get(&skill_tid).copied()
    }

    pub fn level(&self, skill_tid: u32, level: u32) -> Option<&ExpLevel> {
        self.exp.get(&skill_tid)?.get((level as usize).checked_sub(1)?)
    }

    /// Objets récoltés au niveau `level` : le bloc de ce niveau, sinon celui du plus haut niveau inférieur.
    pub fn results(&self, item_set: i64, level: u32) -> Option<&[(u32, i64)]> {
        (1..=level).rev().find_map(|l| self.results.get(&(item_set, l))).map(Vec::as_slice)
    }
}

// ---------------------------------------------------------------------------------------------
// Tirages et expérience
// ---------------------------------------------------------------------------------------------

/// Chance de réussite (pour cent), bornée à 0..=100.
pub fn chance(base: i64, bonus: i64) -> i64 {
    (base + bonus).clamp(0, 100)
}

/// Réussite : tirage dans 0..100 sous la chance.
pub fn roll_success(rng: &mut impl Rng, chance: i64) -> bool {
    rng.gen_range(0..100) < chance
}

/// Objet tiré selon les poids (ceux à 0 ou moins ne sortent jamais).
pub fn pick_weighted(rng: &mut impl Rng, items: &[(u32, i64)]) -> Option<u32> {
    let total: i64 = items.iter().map(|(_, w)| (*w).max(0)).sum();
    if total <= 0 {
        return None;
    }
    let mut x = rng.gen_range(0..total);
    for &(tid, w) in items {
        let w = w.max(0);
        if x < w {
            return Some(tid);
        }
        x -= w;
    }
    None
}

/// Expérience gagnée par une action réussie : tirée dans [min, max].
pub fn roll_exp(rng: &mut impl Rng, lv: &ExpLevel) -> u32 {
    if lv.max > lv.min { rng.gen_range(lv.min..=lv.max) } else { lv.min }
}

/// Niveau et expérience cumulée après un gain : niveau suivant tant que l'expérience atteint le cumul du niveau
/// courant ; au dernier niveau, l'expérience s'arrête à son cumul.
pub fn apply_exp(levels: &[ExpLevel], level: u32, exp: u32, gain: u32) -> (u32, u32) {
    let top = (levels.len() as u32).clamp(1, MAX_LEVEL);
    let (mut level, exp) = (level.clamp(1, top), exp.saturating_add(gain));
    while level < top && exp >= levels[level as usize - 1].total {
        level += 1;
    }
    let cap = levels.get(top as usize - 1).map(|l| l.total).filter(|t| *t > 0).unwrap_or(u32::MAX);
    (level, exp.min(cap))
}

// ---------------------------------------------------------------------------------------------
// Paquets
// ---------------------------------------------------------------------------------------------

/// AGPMPRODUCT op 3 : résultat d'une récolte ou d'une fabrication.
pub fn result_packet(cid: u32, skill_id: u32, tid: u32, count: i64, code: i64, exp: u32) -> Vec<u8> {
    let s = &schema::PRODUCT;
    build_packet(s, &Fields::new(s).with(0, op::RESULT).with(1, cid).with(2, skill_id).with(3, tid).with(4, count).with(5, code).with(6, exp))
}

/// AGPMPRODUCT op 1 : recettes connues.
pub fn recipes_packet(cid: u32, recipes: &[u32]) -> Vec<u8> {
    let s = &schema::PRODUCT;
    let list: Vec<u8> = recipes.iter().flat_map(|r| (*r as i32).to_le_bytes()).collect();
    build_packet(s, &Fields::new(s).with(0, op::COMPOSE_INFO).with(1, cid).with(7, list))
}

/// Recettes connues d'un personnage.
pub async fn load_recipes(db: &Databases, character_id: u32) -> anyhow::Result<Vec<u32>> {
    let rows = db.characters.rows("SELECT recipe_tid FROM character_recipes WHERE character_id = ? ORDER BY recipe_tid", (character_id,)).await?;
    Ok(rows.iter().map(|r| col::<u32>(r, "recipe_tid")).collect())
}

/// Entrée en jeu : recettes connues (AGPMPRODUCT op 1).
pub fn on_enter(st: &WorldState, sid: u32) {
    if let Some(p) = st.players.get(&sid) {
        p.send_raw(recipes_packet(p.ch.id, &p.recipes));
    }
}

// ---------------------------------------------------------------------------------------------
// Cadavres
// ---------------------------------------------------------------------------------------------

/// Récolte d'un cadavre : réservée par un lancement jusqu'à `until`, faite (`done`) ensuite.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Harvest {
    pub sid: u32,
    pub done: bool,
    pub until: Instant,
}

/// Le joueur `sid` peut-il récolter ce cadavre : pas encore récolté, ni réservé par un autre ?
pub fn may_harvest(st: &WorldState, sid: u32, corpse: u32, now: Instant) -> bool {
    st.harvests.get(&corpse).is_none_or(|h| !h.done && (h.sid == sid || h.until <= now))
}

/// Cadavre récoltable par ce joueur avec cette compétence : monstre mort et ciblable, dont le modèle a une
/// récolte pour sa classe et cette compétence. Rend (position, récolte).
fn corpse(ctx: &WorldCtx, st: &WorldState, p: &Player, cid: u32, skill_tid: u32) -> Option<(Vec3, Gather)> {
    let m = st.monsters.get(cid).filter(|m| matches!(m.life, Life::Dead { .. }))?;
    if !st.monsters.stats_of(m.tid).is_attackable() {
        return None;
    }
    let class = p.factors.ftype[2];
    let g = ctx.product.gather.get(&m.tid).filter(|g| g.skill_tid == skill_tid && (1..=4).contains(&class) && g.classes & (1 << class) != 0)?;
    Some((m.pos, *g))
}

/// Monstre mort, pour le lancement d'une compétence de production.
pub fn is_corpse(m: &super::monsters::Monster) -> bool {
    matches!(m.life, Life::Dead { .. })
}

/// Le lancement de la récolte commence : cadavre réservé et gardé visible `HARVEST_HOLD` de plus.
pub fn on_cast_begin(st: &mut WorldState, sid: u32, cid: u32, now: Instant) {
    st.harvests.insert(cid, Harvest { sid, done: false, until: now + HARVEST_HOLD });
    if let Some(m) = st.monsters.get_mut(cid) {
        if let Life::Dead { remove_at } = m.life {
            m.life = Life::Dead { remove_at: remove_at.max(now + HARVEST_HOLD) };
        }
    }
}

fn has_item(p: &Player, tid: u32) -> bool {
    p.inventory.items.values().any(|i| i.tid == tid)
}

/// Outil exigé par la compétence et absent de l'inventaire (le client vérifie la possession, pas l'équipement).
fn missing_tool(ctx: &WorldCtx, p: &Player, skill_tid: u32) -> Option<u32> {
    ctx.product.tools.get(&skill_tid).copied().filter(|t| !has_item(p, *t))
}

// ---------------------------------------------------------------------------------------------
// Lancement de la récolte
// ---------------------------------------------------------------------------------------------

/// Lancement d'une compétence de production (AGPMSKILL op 7) : seule la récolte en passe par là ; cible = cadavre.
/// Accepté, le lancement suit le cours des autres compétences (`skills::begin` : approche, action 1).
pub fn request_cast(ctx: &WorldCtx, st: &mut WorldState, sid: u32, owned: Owned, target: u32, now: Instant) {
    let Some(p) = st.players.get(&sid) else { return };
    if ctx.product.category(owned.tid) != Some(CATEGORY_HUNT) {
        debug!("[production] {} lance la compétence {} : pas une récolte, ignoré", p.ch.name, owned.tid);
        return;
    }
    if let Some(tool) = missing_tool(ctx, p, owned.tid) {
        debug!("[production] {} n'a pas l'outil {tool}", p.ch.name);
        p.send_raw(result_packet(p.ch.id, owned.id, 0, 0, result::NO_TOOL, 0));
        return;
    }
    let refusal = if corpse(ctx, st, p, target, owned.tid).is_none() {
        Some(cast_refusal::INVALID_TARGET)
    } else if !may_harvest(st, sid, target, now) {
        Some(cast_refusal::NOTHING_LEFT)
    } else {
        None
    };
    if let Some(code) = refusal {
        debug!("[production] {} : récolte de {target:#x} refusée (action {code})", p.ch.name);
        p.send_raw(skills::refusal_packet(p.ch.id, &owned, target, code));
        return;
    }
    let p = st.players.get_mut(&sid).unwrap();
    p.skills.cast = Some(Cast { skill_id: owned.id, target, stage: Stage::Approach { check_at: now }, forced: false });
    debug!("[production] {} prépare la récolte de {target:#x}", p.ch.name);
}

// ---------------------------------------------------------------------------------------------
// AGPMPRODUCT
// ---------------------------------------------------------------------------------------------

/// AGPMPRODUCT reçu du client.
pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some(player) = st.players.get(&sid) else { return Ok(()) };
    if !player.in_game || player.dead {
        return Ok(());
    }
    let skill_id = p.int(2).unwrap_or(0) as u32;
    let target = p.int(3).unwrap_or(0);
    match p.int(0) {
        Some(op::GATHER) => gather(ctx, st, sid, skill_id, target as u32, Instant::now()).await,
        Some(op::COMPOSE) => compose(ctx, st, sid, skill_id, target as u32, p.int(4).unwrap_or(0).max(0) as u32).await,
        other => {
            debug!("[production] #{sid} AGPMPRODUCT op {other:?} (ignoré)");
            Ok(())
        }
    }
}

/// Récolte d'un cadavre (op 2).
async fn gather(ctx: &WorldCtx, st: &mut WorldState, sid: u32, skill_id: u32, cid: u32, now: Instant) -> anyhow::Result<()> {
    let p = &st.players[&sid];
    let me = p.ch.id;
    let Some(owned) = p.skills.by_id(skill_id).copied().filter(|s| ctx.product.category(s.tid) == Some(CATEGORY_HUNT)) else {
        warn!("[production] {} récolte avec la compétence {skill_id}, qui n'est pas une récolte connue", p.ch.name);
        return Ok(());
    };
    let reply = |code: i64| result_packet(me, skill_id, 0, 0, code, 0);
    if missing_tool(ctx, p, owned.tid).is_some() {
        p.send_raw(reply(result::NO_TOOL));
        return Ok(());
    }
    let Some((pos, g)) = corpse(ctx, st, p, cid, owned.tid) else {
        p.send_raw(reply(result::INVALID_TARGET));
        return Ok(());
    };
    if super::combat::dist2d(p.ch.pos(), pos) > GATHER_RANGE + REACH_TOLERANCE {
        p.send_raw(reply(result::TOO_FAR));
        return Ok(());
    }
    if !may_harvest(st, sid, cid, now) {
        p.send_raw(reply(result::NOTHING_LEFT));
        return Ok(());
    }
    let bonus = ctx.product.level(owned.tid, owned.level).map(|l| l.rate_bonus).unwrap_or(0);
    let (success, item) = {
        let mut rng = rand::thread_rng();
        let success = roll_success(&mut rng, chance(g.rate, bonus));
        let item = if success { ctx.product.results(g.item_set, owned.level).and_then(|list| pick_weighted(&mut rng, list)) } else { None };
        (success, item)
    };
    if success && item.is_none() {
        warn!("[production] ensemble {} sans objet au niveau {} : récolte comptée comme un échec", g.item_set, owned.level);
    }
    // Inventaire plein : rien n'est fait, le cadavre reste à récolter.
    if let Some(tid) = item {
        let stack = ctx.catalog.get(tid).map(|d| d.stack).unwrap_or(1).max(1);
        if p.inventory.placement(tid, 1, stack, None).is_none() {
            p.send_raw(reply(result::FULL_INVENTORY));
            return Ok(());
        }
    }
    st.harvests.insert(cid, Harvest { sid, done: true, until: now });
    let name = st.players[&sid].ch.name.clone();
    let Some(tid) = item else {
        info!("[production] {name} échoue à récolter {cid:#x}");
        st.players[&sid].send_raw(reply(result::FAIL));
        return Ok(());
    };
    give(ctx, st, sid, tid).await?;
    let exp = gain_exp(ctx, st, sid, owned.tid).await?;
    info!("[production] {name} récolte {tid} sur {cid:#x} (+{exp} d'expérience)");
    if let Some(p) = st.players.get(&sid) {
        p.send_raw(result_packet(me, skill_id, tid, 1, result::SUCCESS, exp));
    }
    Ok(())
}

/// Range un exemplaire de `tid` dans l'inventaire (place déjà vérifiée) et le montre au joueur.
async fn give(ctx: &WorldCtx, st: &mut WorldState, sid: u32, tid: u32) -> anyhow::Result<()> {
    let Some(p) = st.players.get_mut(&sid) else { return Ok(()) };
    let stack = ctx.catalog.get(tid).map(|d| d.stack).unwrap_or(1).max(1);
    let Some((it, created)) = p.inventory.add(&ctx.db, tid, 1, stack, None).await? else { return Ok(()) };
    let cid = p.ch.id;
    p.send_raw(if created { item_add_of(&it, cid, item_status::INVENTORY, Some(it.grid)) } else { item_count(&it, cid) });
    Ok(())
}

/// Expérience d'une action réussie : ajoutée, niveau monté au besoin, enregistrée, compétence renvoyée (AGPMSKILL
/// op 2). Rend le gain.
async fn gain_exp(ctx: &WorldCtx, st: &mut WorldState, sid: u32, skill_tid: u32) -> anyhow::Result<u32> {
    let Some(levels) = ctx.product.exp.get(&skill_tid).filter(|l| !l.is_empty()) else { return Ok(0) };
    let Some(p) = st.players.get_mut(&sid) else { return Ok(0) };
    let Some(s) = p.skills.list.iter_mut().find(|s| s.tid == skill_tid) else { return Ok(0) };
    let Some(lv) = levels.get((s.level.max(1) as usize - 1).min(levels.len() - 1)) else { return Ok(0) };
    let gain = roll_exp(&mut rand::thread_rng(), lv);
    let before = s.level;
    let (level, exp) = apply_exp(levels, s.level, s.exp, gain);
    s.level = level;
    s.exp = exp;
    let updated = *s;
    let cid = p.ch.id;
    p.send_raw(skills::update_packet(cid, &updated));
    ctx.db.characters.exec("UPDATE character_skills SET level = ?, exp = ? WHERE character_id = ? AND skill_tid = ?", (level, exp, cid, skill_tid)).await?;
    if level > before {
        let name = ctx.skills.get(skill_tid).map(|t| t.name.clone()).unwrap_or_default();
        info!("[production] {} passe {name} au niveau {level}", p.ch.name);
        send_system(st, sid, &format!("{name} passe au niveau {level}."));
    }
    Ok(gain)
}

/// Fabrication (op 0) : recette `recipe`, objet recette `recipe_iid` (0 : aucun).
async fn compose(ctx: &WorldCtx, st: &mut WorldState, sid: u32, skill_id: u32, recipe: u32, recipe_iid: u32) -> anyhow::Result<()> {
    let p = &st.players[&sid];
    let me = p.ch.id;
    let reply = |code: i64| result_packet(me, skill_id, recipe, 0, code, 0);
    let Some((owned, category)) = p.skills.by_id(skill_id).copied().and_then(|s| Some((s, ctx.product.category(s.tid).filter(|c| *c != CATEGORY_HUNT)?))) else {
        warn!("[production] {} fabrique avec la compétence {skill_id}, qui n'est pas une fabrication connue", p.ch.name);
        p.send_raw(reply(result::FAIL));
        return Ok(());
    };
    // Refus sans rien consommer : code 1 (le client n'en a pas d'autre pour ces cas) et la raison.
    let refused = match ctx.product.composes.get(&recipe) {
        None => Some("recette inconnue".to_string()),
        Some(c) if c.category != category => Some(format!("{} n'est pas une recette de cette compétence", c.name)),
        Some(c) if !p.recipes.contains(&recipe) => Some(format!("{} : recette pas encore apprise", c.name)),
        Some(c) if owned.level < c.item_level => Some(format!("{} demande le niveau {} de la compétence", c.name, c.item_level)),
        Some(c) if c.title_tid != 0 => Some(format!("{} demande un titre", c.name)),
        Some(_) => None,
    };
    if let Some(why) = refused {
        info!("[production] {} ne peut pas fabriquer {recipe} : {why}", p.ch.name);
        p.send_raw(reply(result::FAIL));
        send_system(st, sid, &format!("Fabrication impossible : {why}."));
        return Ok(());
    }
    let c = &ctx.product.composes[&recipe];
    if missing_tool(ctx, p, owned.tid).is_some() {
        p.send_raw(reply(result::NO_TOOL));
        return Ok(());
    }
    // Matériaux pris dans les piles de l'inventaire, autant qu'il en faut.
    let mut take: Vec<(u32, i64)> = Vec::new();
    for &(tid, need) in &c.materials {
        let mut left = need;
        for it in p.inventory.items.values().filter(|i| i.tid == tid) {
            if left == 0 {
                break;
            }
            let n = it.count.min(left);
            take.push((it.iid, n));
            left -= n;
        }
        if left > 0 {
            p.send_raw(reply(result::NO_MATERIAL));
            return Ok(());
        }
    }
    // Objet recette : celui désigné, s'il est bien dans l'inventaire et du bon modèle.
    let with_recipe = c.recipe_tid != 0 && recipe_iid != 0 && p.inventory.get(recipe_iid).is_some_and(|i| i.tid == c.recipe_tid);
    if recipe_iid != 0 && !with_recipe {
        debug!("[production] {} : objet recette {recipe_iid} ignoré", p.ch.name);
    }
    if with_recipe {
        take.push((recipe_iid, 1));
    }
    // Place du produit : pile existante, case libre, ou case libérée par une pile entièrement consommée.
    let stack = ctx.catalog.get(c.result_tid).map(|d| d.stack).unwrap_or(1).max(1);
    let freed = take.iter().any(|(iid, n)| p.inventory.get(*iid).is_some_and(|i| i.count <= *n));
    if p.inventory.placement(c.result_tid, 1, stack, None).is_none() && !freed {
        p.send_raw(reply(result::FULL_INVENTORY));
        return Ok(());
    }
    let bonus = if with_recipe { c.recipe_bonus } else { 0 } + ctx.product.level(owned.tid, owned.level).map(|l| l.rate_bonus).unwrap_or(0);
    let success = roll_success(&mut rand::thread_rng(), chance(c.success_rate, bonus));
    let (result_tid, name) = (c.result_tid, c.name.clone());
    let p = st.players.get_mut(&sid).unwrap();
    for (iid, n) in &take {
        match p.inventory.consume(&ctx.db, *iid, *n).await? {
            Some(left) => p.send_raw(item_count(&left, me)),
            None => p.send_raw(item_remove(*iid, Some(me))),
        }
    }
    let who = p.ch.name.clone();
    if !success {
        info!("[production] {who} échoue à fabriquer {name} ({result_tid})");
        p.send_raw(reply(result::FAIL));
        return Ok(());
    }
    give(ctx, st, sid, result_tid).await?;
    let exp = gain_exp(ctx, st, sid, owned.tid).await?;
    info!("[production] {who} fabrique {name} ({result_tid}{}, +{exp} d'expérience)", if with_recipe { ", avec la recette" } else { "" });
    if let Some(p) = st.players.get(&sid) {
        p.send_raw(result_packet(me, skill_id, result_tid, 1, result::SUCCESS, exp));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// PNJ (AGPMEVENTPRODUCT)
// ---------------------------------------------------------------------------------------------

/// AGPMEVENTPRODUCT reçu du client.
pub async fn on_event(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some(player) = st.players.get(&sid) else { return Ok(()) };
    let base = p.bytes(1).and_then(|b| decode_mini(&mini::EVENT_BASE, b).ok()).unwrap_or_else(|| Fields::new(&mini::EVENT_BASE));
    let Some((npc, _)) = resolve(&ctx.npcs, &base) else {
        warn!("[production] #{sid} PNJ inconnu");
        return Ok(());
    };
    let Some(ev) = npc.events.iter().find(|e| e.function == event_function::PRODUCT) else {
        warn!("[production] #{sid} {} n'enseigne pas la production", npc.name);
        return Ok(());
    };
    if npc_distance(npc, player.ch.pos_x, player.ch.pos_z) > NPC_RANGE {
        warn!("[production] #{sid} {} trop loin de {}", player.ch.name, npc.name);
        return Ok(());
    }
    let category: i64 = ev.param("ProductCategory").and_then(|v| v.trim().parse().ok()).unwrap_or(-1);
    let event = build_mini(&mini::EVENT_BASE, &base);
    let cid = player.ch.id;
    let s = &schema::EVENT_PRODUCT;
    let reply = |op: i64, a: Option<i64>, b: Option<i64>| Fields::new(s).with(0, op).with(1, event.clone()).with(2, cid).with_opt(3, a).with_opt(4, b);
    let a = p.int(3).unwrap_or(-1);
    match p.int(0).unwrap_or(-1) {
        event_op::REQUEST => {
            info!("[production] {} consulte {} (catégorie {category})", player.ch.name, npc.name);
            player.send(s, reply(event_op::GRANT, None, None));
        }
        event_op::LEARN_SKILL => {
            let code = learn_skill(ctx, st, sid, category, a.max(0) as u32).await?;
            if let Some(player) = st.players.get(&sid) {
                player.send(s, reply(event_op::LEARN_SKILL_RESULT, Some(a), Some(code)));
            }
        }
        event_op::BUY_COMPOSE => {
            let code = buy_recipe(ctx, st, sid, category, a.max(0) as u32).await?;
            if let Some(player) = st.players.get(&sid) {
                player.send(s, reply(event_op::BUY_COMPOSE_RESULT, Some(a), Some(code)));
            }
        }
        op => warn!("[production] #{sid} AGPMEVENTPRODUCT op {op} inattendu"),
    }
    Ok(())
}

/// Apprend la compétence de production `tid` chez un PNJ de la catégorie `category` : rend le code de l'op 3.
pub async fn learn_skill(ctx: &WorldCtx, st: &mut WorldState, sid: u32, category: i64, tid: u32) -> anyhow::Result<i64> {
    let p = &st.players[&sid];
    let Some(t) = ctx.skills.get(tid).filter(|_| ctx.product.category(tid) == Some(category)) else {
        info!("[production] {} demande la compétence {tid}, qu'un PNJ de la catégorie {category} n'enseigne pas", p.ch.name);
        return Ok(learn_result::FAIL);
    };
    if p.skills.by_tid(tid).is_some() {
        return Ok(learn_result::ALREADY);
    }
    let lv = t.level(1).cloned().unwrap_or_default();
    if p.ch.level < lv.require_level {
        return Ok(learn_result::LOW_LEVEL);
    }
    if (p.ch.money as i64) < lv.learn_cost {
        return Ok(learn_result::NO_MONEY);
    }
    if p.skills.list.len() >= skills::MAX_SKILLS {
        return Ok(learn_result::FAIL);
    }
    let money = p.ch.money as i64 - lv.learn_cost;
    ctx.db.characters.exec("INSERT INTO character_skills (character_id, skill_tid, level, exp) VALUES (?, ?, 1, 0) ON DUPLICATE KEY UPDATE level = 1, exp = 0", (p.ch.id, tid)).await?;
    st.next_skill_id += 1;
    let owned = Owned { id: st.next_skill_id, tid, level: 1, ready_at: None, exp: 0 };
    let p = st.players.get_mut(&sid).unwrap();
    p.skills.list.push(owned);
    p.send_raw(skills::add_packet(p.ch.id, &owned));
    info!("[production] {} apprend {} ({} or)", p.ch.name, t.name, lv.learn_cost);
    super::set_money(ctx, st, sid, money).await?;
    Ok(learn_result::SUCCESS)
}

/// Achète la recette `recipe` chez un PNJ de la catégorie `category` : rend le code de l'op 5.
pub async fn buy_recipe(ctx: &WorldCtx, st: &mut WorldState, sid: u32, category: i64, recipe: u32) -> anyhow::Result<i64> {
    let p = &st.players[&sid];
    let Some(c) = ctx.product.composes.get(&recipe).filter(|c| c.category == category) else {
        info!("[production] {} demande la recette {recipe}, absente de la catégorie {category}", p.ch.name);
        return Ok(learn_result::FAIL);
    };
    if p.recipes.contains(&recipe) {
        return Ok(learn_result::ALREADY);
    }
    let Some(skill) = ctx.product.skill_of.get(&category).and_then(|t| p.skills.by_tid(*t)) else {
        return Ok(learn_result::NO_SKILL);
    };
    if skill.level < c.item_level {
        return Ok(learn_result::LOW_LEVEL);
    }
    if (p.ch.money as i64) < c.price {
        return Ok(learn_result::NO_MONEY);
    }
    let money = p.ch.money as i64 - c.price;
    ctx.db.characters.exec("INSERT IGNORE INTO character_recipes (character_id, recipe_tid) VALUES (?, ?)", (p.ch.id, recipe)).await?;
    let p = st.players.get_mut(&sid).unwrap();
    p.recipes.push(recipe);
    info!("[production] {} achète la recette {} ({} or)", p.ch.name, c.name, c.price);
    super::set_money(ctx, st, sid, money).await?;
    Ok(learn_result::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    fn levels() -> Vec<ExpLevel> {
        [(5, 7, 0, 103, 103), (7, 10, 2, 260, 363), (10, 13, 4, 597, 960)]
            .iter()
            .map(|&(min, max, rate_bonus, level_exp, total)| ExpLevel { min, max, rate_bonus, level_exp, total })
            .collect()
    }

    #[test]
    fn chance_et_tirage() {
        assert_eq!((chance(90, 4), chance(90, 28), chance(-5, 0)), (94, 100, 0));
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        assert!((0..500).all(|_| roll_success(&mut rng, 100)), "100 % : toujours");
        assert!((0..500).all(|_| !roll_success(&mut rng, 0)), "0 % : jamais");
        let wins = (0..2000).filter(|_| roll_success(&mut rng, 70)).count();
        assert!((1250..1550).contains(&wins), "{wins} réussites sur 2000 à 70 %");
    }

    #[test]
    fn tirage_pondere() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(3);
        let block = [(4963, 40), (4960, 30), (4927, 30), (1, 0)];
        let mut seen: HashMap<u32, u32> = HashMap::new();
        for _ in 0..3000 {
            *seen.entry(pick_weighted(&mut rng, &block).unwrap()).or_default() += 1;
        }
        assert!(!seen.contains_key(&1), "poids nul jamais tiré");
        assert!((1050..1350).contains(&seen[&4963]), "{seen:?}");
        assert!((750..1050).contains(&seen[&4960]), "{seen:?}");
        assert_eq!(pick_weighted(&mut rng, &[]), None);
        assert_eq!(pick_weighted(&mut rng, &[(5, 0)]), None);
    }

    #[test]
    fn experience_et_niveaux() {
        let l = levels();
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        for _ in 0..100 {
            assert!((5..=7).contains(&roll_exp(&mut rng, &l[0])));
        }
        assert_eq!(roll_exp(&mut rng, &ExpLevel { min: 4, max: 2, ..Default::default() }), 4);
        assert_eq!(apply_exp(&l, 1, 100, 2), (1, 102));
        assert_eq!(apply_exp(&l, 1, 100, 3), (2, 103), "cumul du niveau 1 atteint");
        assert_eq!(apply_exp(&l, 1, 0, 400), (3, 400), "deux niveaux d'un coup");
        assert_eq!(apply_exp(&l, 3, 950, 50), (3, 960), "dernier niveau : expérience bornée");
        assert_eq!(apply_exp(&[], 1, 0, 5), (1, 5));
    }

    #[test]
    fn materiaux_additionnes() {
        assert_eq!(sum_materials(&[(4938, 2), (4960, 1), (4938, 2)]), vec![(4938, 4), (4960, 1)]);
    }

    #[test]
    fn bloc_d_objets_du_niveau() {
        let mut c = ProductCatalog::default();
        c.results.insert((2, 1), vec![(4963, 100)]);
        c.results.insert((2, 3), vec![(4964, 100)]);
        assert_eq!(c.results(2, 2), Some(&[(4963, 100)][..]), "niveau sans bloc : le précédent");
        assert_eq!(c.results(2, 5), Some(&[(4964, 100)][..]));
        assert_eq!(c.results(9, 1), None);
    }

    #[test]
    fn paquets() {
        let raw = result_packet(5, 9, 4963, 1, result::SUCCESS, 6);
        let p = archcore_net::decode_packet(&schema::PRODUCT, &raw).unwrap();
        assert_eq!((p.int(0), p.int(1), p.int(2), p.int(3), p.int(4), p.int(5), p.int(6)), (Some(3), Some(5), Some(9), Some(4963), Some(1), Some(0), Some(6)));
        let raw = recipes_packet(5, &[644, 406]);
        let p = archcore_net::decode_packet(&schema::PRODUCT, &raw).unwrap();
        let list = p.bytes(7).unwrap();
        let ids: Vec<i32> = list.chunks(4).map(|b| i32::from_le_bytes(b.try_into().unwrap())).collect();
        assert_eq!((p.int(0), ids), (Some(op::COMPOSE_INFO), vec![644, 406]));
        // Demande du client : op 2, cadavre au champ 3.
        let s = &schema::PRODUCT;
        let raw = build_packet(s, &Fields::new(s).with(0, op::GATHER).with(1, 5).with(2, 9).with(3, 0x1000_0042i64));
        let p = archcore_net::decode_packet(s, &raw).unwrap();
        assert_eq!((p.int(0), p.int(3), p.int(4)), (Some(2), Some(0x1000_0042), None));
        let e = &schema::EVENT_PRODUCT;
        let base = super::super::npcs::event_base(0x4000_000f, event_function::PRODUCT);
        let raw = build_packet(e, &Fields::new(e).with(0, event_op::LEARN_SKILL).with(1, base).with(2, 5).with(3, 663));
        let p = archcore_net::decode_packet(e, &raw).unwrap();
        let b = decode_mini(&mini::EVENT_BASE, p.bytes(1).unwrap()).unwrap();
        assert_eq!((p.int(0), p.int(2), p.int(3), b.int(1), b.int(3)), (Some(2), Some(5), Some(663), Some(0x4000_000f), Some(24)));
    }
}
