//! Objets : catalogue (`item_data`, `npc_trade_list`), inventaire des personnages et paquets AGPMITEM.
//!
//! Le client ne connaît un objet qu'après un AGPMITEM op 0 (ajout) qui lui donne son IID, son modèle
//! (TID), sa quantité et sa case. Il le crée à partir de son propre modèle (`ItemTemplateAll.ini`) : un
//! TID qu'il ne connaît pas est ignoré. Les objets d'inventaire gardent comme IID leur identifiant en
//! base ; les objets montrés par une boutique reçoivent des IID d'une plage à part, jamais enregistrés.
//! Un objet équipé (statut 3) est enregistré avec le statut 1 et sa partie du corps dans `layer`.
//!
//! Inventaire Cash (statut 51) : les objets dont la colonne « Cash » d'itemdatatable n'est pas nulle (familiers compris,
//! « Cash » = 6) y sont rangés, sans case envoyée (le client les place lui-même dans sa liste) et avec le mini Cash au
//! champ 23 (octet « en cours d'utilisation »…, `mini::ITEM_CASH`). En base : statut 6, rang dans la liste en
//! `row_idx`. Les objets Cash encore enregistrés dans l'inventaire ordinaire y sont déplacés à l'entrée en jeu
//! (`sort_cash`). Choix d'ArchCore : les 9 têtes Cash qui se portent (partie 17) restent dans l'inventaire ordinaire,
//! l'équipement depuis le statut 51 n'étant pas relevé. Sac du familier (statut 13, en base 7) : voir `pet_bag.rs`.

use crate::db::{col, Databases};
use crate::info;
use mysql_async::Params;
use archcore_net::schema::{self, mini};
use archcore_net::{build_mini, build_packet, Fields};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicU32, Ordering};

pub mod item_op {
    pub const ADD: i64 = 0;
    pub const REMOVE: i64 = 1;
    pub const UPDATE: i64 = 2;
    /// Arrêt d'un objet Cash en cours d'utilisation (client → serveur).
    pub const UNUSE: i64 = 23;
    /// Mini Cash d'un objet mis à jour (champ 23) : le client rafraîchit son inventaire Cash.
    pub const UPDATE_USE_TIME: i64 = 26;
    /// Ramassage automatique du sac du familier (champ 20, `mini::ITEM_AUTO_PICK`).
    pub const CHANGE_AUTO_PICK: i64 = 30;
}

/// Statut d'un objet (champ 1 d'AGPMITEM).
pub mod item_status {
    pub const INVENTORY: i64 = 2;
    pub const EQUIP: i64 = 3;
    /// Grille d'échange entre joueurs.
    pub const TRADE_GRID: i64 = 6;
    pub const BANK: i64 = 100;
    pub const NPC_TRADE: i64 = 10;
    /// Sac du familier (`pet_bag.rs`).
    pub const SUB_INVENTORY: i64 = 13;
    /// Inventaire Cash.
    pub const CASH_INVENTORY: i64 = 51;
}

/// Statut enregistré en base (colonne `character_items.status`), indépendant du protocole.
pub mod stored_status {
    pub const INVENTORY: i64 = 0;
    pub const EQUIP: i64 = 1;
    pub const BANK: i64 = 2;
    /// Entrepôt de guilde (`guild_id` renseigné).
    pub const GUILD_WAREHOUSE: i64 = 3;
    /// Pièce jointe d'un courrier (`mail.item_id`), rattachée au destinataire jusqu'à sa récupération.
    pub const MAILBOX: i64 = 4;
    /// En vente dans la maison des ventes (`auction_sale.item_id`), rattachée au vendeur jusqu'à l'achat.
    pub const AUCTION: i64 = 5;
    /// Inventaire Cash (statut 51) : rang dans la liste en `row_idx`.
    pub const CASH_INVENTORY: i64 = 6;
    /// Sac du familier (statut 13) : sac, ligne, colonne.
    pub const SUB_INVENTORY: i64 = 7;
}

/// Inventaire : 5 sacs de 5 × 5 cases, comme les onglets et la grille de la fenêtre `UI_Inventory` du client.
pub const INVENTORY: (i64, i64, i64) = (5, 5, 5);
/// Inventaire Cash : une liste de 200 places (1 × 200 × 1).
pub const CASH_INVENTORY: (i64, i64, i64) = (1, 200, 1);
/// Sac du familier : 4 sacs de 2 lignes × 4 colonnes (fenêtre `UI_SubInventory`).
pub const SUB_INVENTORY: (i64, i64, i64) = (4, 2, 4);
/// Grille d'une boutique : 1 page de 20 lignes × 4 colonnes (type de grille 6 du client).
pub const SHOP_GRID: (i64, i64, i64) = (1, 20, 4);
pub const SHOP_IID_BASE: u32 = 0x2000_0000;
const SHOP_IID_SPAN: u32 = 0x1000_0000;

static SHOP_IID_COUNTER: AtomicU32 = AtomicU32::new(0);

/// IID d'un objet de boutique (plage 0x20000000–0x2fffffff, jamais en base).
pub fn alloc_shop_iid() -> u32 {
    SHOP_IID_BASE + (SHOP_IID_COUNTER.fetch_add(1, Ordering::Relaxed).wrapping_add(1) % SHOP_IID_SPAN)
}

pub type Grid = [i64; 3];

/// Insertion d'un exemplaire d'objet : personnage, modèle, statut, case (sac, ligne, colonne), quantité.
pub const INSERT_ITEM: &str =
    "INSERT INTO character_items (character_id, template_id, status, layer, row_idx, col_idx, count) VALUES (?, ?, ?, ?, ?, ?, ?)";

/// Place prévue pour des objets ajoutés à l'inventaire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// Pile existante (IID) et sa nouvelle quantité.
    Stack(u32, i64),
    /// Nouvelle pile dans cette case.
    Slot(Grid),
}

pub fn grid_mini(g: Grid) -> Vec<u8> {
    let s = &mini::ITEM_GRID;
    build_mini(s, &Fields::new(s).with(0, g[0]).with(1, g[1]).with(2, g[2]))
}

/// Champ AGPMITEM de la case selon le statut : 8 pour la banque, 7 sinon (inventaire, boutique, échange, sac du
/// familier).
fn grid_field(status: i64) -> usize {
    if status == item_status::BANK { 8 } else { 7 }
}

/// La case part avec l'objet : ni pour un objet porté, ni pour l'inventaire Cash (le client choisit sa place).
fn sends_grid(status: i64) -> bool {
    status != item_status::EQUIP && status != item_status::CASH_INVENTORY
}

/// Champ 23 d'AGPMITEM (mini Cash) réduit à l'octet « en cours d'utilisation ».
pub fn cash_mini(in_use: bool) -> Vec<u8> {
    let s = &mini::ITEM_CASH;
    build_mini(s, &Fields::new(s).with(0, i64::from(in_use)))
}

/// Mini Cash complet d'un objet à durée (`lifetime.rs`) : 0 « en cours d'utilisation » (0 non, 1 oui, 2 en pause), 1 temps
/// de jeu restant (ms), 2 date d'expiration (secondes Unix).
pub fn cash_mini_full(in_use: i64, remain_ms: i64, expire_at: i64) -> Vec<u8> {
    let s = &mini::ITEM_CASH;
    build_mini(s, &Fields::new(s).with(0, in_use).with(1, remain_ms).with(2, expire_at.max(0) as u64))
}

/// AGPMITEM op 26 : mini Cash de l'objet `iid` (« en cours d'utilisation » ou non) ; le client rafraîchit son
/// inventaire Cash, et avec lui la disponibilité du sac du familier.
pub fn use_time_packet(iid: u32, owner: u32, in_use: bool) -> Vec<u8> {
    let s = &schema::ITEM;
    build_packet(s, &Fields::new(s).with(0, item_op::UPDATE_USE_TIME).with(2, iid).with(4, owner).with(23, cash_mini(in_use)))
}

/// AGPMITEM op 0 (ajout). `grid` = [sac, ligne, colonne] pour l'inventaire, la banque, une boutique ou
/// l'échange. Champ 26 : identifiant stable de l'objet, l'IID (voir `item_add_of`).
pub fn item_add(iid: u32, tid: u32, owner: Option<u32>, count: i64, status: i64, grid: Option<Grid>) -> Vec<u8> {
    let s = &schema::ITEM;
    let f = Fields::new(s).with(0, item_op::ADD).with(1, status).with(2, iid).with(3, tid).with_opt(4, owner).with(5, count).with(26, iid as u64);
    build_packet(s, &f.with_opt(grid_field(status), grid.map(grid_mini)))
}

/// AGPMITEM op 0 d'un objet connu, avec ses options (champ 20) et son identifiant stable (champ 26 : l'IID, qui est
/// l'identifiant en base ; le commutateur d'équipement, `itemset.rs`, désigne les objets par lui).
pub fn item_add_of(i: &InventoryItem, owner: u32, status: i64, grid: Option<Grid>) -> Vec<u8> {
    item_add_with_cash(i, owner, status, grid, (status == item_status::CASH_INVENTORY).then(|| cash_mini(false)))
}

/// Comme `item_add_of`, avec le mini Cash donné (champ 23 : objet à durée, `lifetime.rs`).
pub fn item_add_with_cash(i: &InventoryItem, owner: u32, status: i64, grid: Option<Grid>, cash: Option<Vec<u8>>) -> Vec<u8> {
    let s = &schema::ITEM;
    let f = Fields::new(s).with(0, item_op::ADD).with(1, status).with(2, i.iid).with(3, i.tid).with(4, owner).with(5, i.count).with(26, i.iid as u64);
    let f = f.with_opt(grid_field(status), grid.filter(|_| sends_grid(status)).map(grid_mini));
    let f = f.with_opt(23, cash);
    // Durabilité entamée : facteur « objet » (champ 10) ; pleine, le client garde celle du modèle.
    let f = f.with_opt(10, (i.durability >= 0).then(|| durability_factors(i.durability)));
    build_packet(s, &f.with_opt(20, (!i.options.is_empty()).then(|| super::options::options_mini(&i.options))))
}

/// Champ 10 d'AGPMITEM (mini CHAR_FACTORS) réduit au facteur « objet » (sous-paquet 10) : durabilité actuelle au
/// champ 0. Le maximum affiché par le client (sous-valeur 6) ne vient que de son modèle.
pub fn durability_factors(current: i32) -> Vec<u8> {
    let (fi, cf) = (&mini::FACTOR_ITEM, &mini::CHAR_FACTORS);
    build_mini(cf, &Fields::new(cf).with(10, build_mini(fi, &Fields::new(fi).with(0, current.max(0)))))
}

/// AGPMITEM op 2 (mise à jour) de la seule durabilité d'un objet, sans statut ni case : le client ne le déplace pas.
pub fn item_durability(iid: u32, tid: u32, owner: u32, current: i32) -> Vec<u8> {
    let s = &schema::ITEM;
    build_packet(s, &Fields::new(s).with(0, item_op::UPDATE).with(2, iid).with(3, tid).with(4, owner).with(10, durability_factors(current)))
}

/// AGPMITEM op 1 (retrait).
pub fn item_remove(iid: u32, owner: Option<u32>) -> Vec<u8> {
    let s = &schema::ITEM;
    build_packet(s, &Fields::new(s).with(0, item_op::REMOVE).with(2, iid).with_opt(4, owner))
}

/// AGPMITEM op 2 (mise à jour) du statut et de la place : case d'inventaire, de banque ou d'échange, ou
/// aucune pour un objet équipé.
pub fn item_update(item: &InventoryItem, owner: u32, status: i64) -> Vec<u8> {
    let s = &schema::ITEM;
    let f = Fields::new(s).with(0, item_op::UPDATE).with(1, status).with(2, item.iid).with(3, item.tid).with(4, owner).with(5, item.count);
    build_packet(s, &f.with_opt(grid_field(status), sends_grid(status).then(|| grid_mini(item.grid))))
}

/// AGPMITEM op 2 (mise à jour) de la quantité d'un objet de l'inventaire.
pub fn item_count(item: &InventoryItem, owner: u32) -> Vec<u8> {
    item_count_in(item, owner, item_status::INVENTORY)
}

/// AGPMITEM op 2 (mise à jour) de la quantité d'un objet au statut `status` (inventaire, Cash, sac du familier).
pub fn item_count_in(item: &InventoryItem, owner: u32, status: i64) -> Vec<u8> {
    let s = &schema::ITEM;
    build_packet(s, &Fields::new(s).with(0, item_op::UPDATE).with(1, status).with(2, item.iid).with(3, item.tid).with(4, owner).with(5, item.count))
}

/// AGPMITEMCONVERT op 0 d'un objet converti, à envoyer après son AGPMITEM op 0 ; rien pour un objet
/// ordinaire.
pub fn convert_add(item: &InventoryItem, owner: u32) -> Option<Vec<u8>> {
    use super::convert::{op, state_packet};
    (!item.conv.is_plain()).then(|| state_packet(op::ADD, owner, item.iid, &item.conv, None, None))
}

#[derive(Clone, Debug)]
pub struct ItemData {
    pub tid: u32,
    pub name: String,
    pub npc_price: i64,
    pub pc_price: i64,
    /// Boutique des Shrines : prix chez un PNJ en points de charisme (« NPC Charisma Price ») et en Shrine Coins
    /// (« NPC Shrine Coin », objets `npc_trade::SHRINE_COIN`), en plus de l'or ; 0 : aucun.
    pub charisma_price: i64,
    pub coin_price: i64,
    pub stack: i64,
    /// Caractéristiques d'équipement, pour un objet qui se porte.
    pub equip: Option<EquipData>,
    /// Effet d'une potion.
    pub potion: Option<super::recovery::Potion>,
    /// Parchemin de téléportation.
    pub scroll: Option<super::travel::Scroll>,
    /// Pierre d'esprit : (« Spirit Type », « Rank »).
    pub stone: Option<(i64, i64)>,
    /// Type d'un consommable (« Type » quand « ItemType » vaut 1 : 2 pierre d'esprit, 8 potion de
    /// renforcement, 9 rune), 0 sinon.
    pub usable_type: i64,
    /// Options fixes (« OptionTID ») : celles qu'une rune apporte à l'objet où elle est sertie.
    pub option_tids: Vec<u16>,
    /// Genre (« Kind ») ; le bit 0x20 marque les objets qu'un meurtrier de palier 2 ne peut pas acheter.
    pub kind: i64,
    /// Liaison (« BoundType ») : 1 lié dès l'obtention, 2 lié à l'usage ; 0 aucune.
    pub bound_type: i64,
    /// Type d'objet de la boutique payante (« Cash » : 6 familier) ; 0 pour un objet ordinaire.
    pub cash: i64,
    /// Consommable utilisable à cheval (« EnableOnRide »).
    pub enable_on_ride: bool,
    /// Catégories de la maison des ventes (« FirstCategory », « SecondCategory ») ; un objet sans les deux ne peut pas y
    /// être mis en vente.
    pub auction_category: (i64, i64),
    /// Objet d'événement (« EventItem »).
    pub event_item: bool,
}

/// Options fixes d'un objet (colonne « OptionTID », séparées par « ; »).
pub fn option_tids_of(raw: &serde_json::Value) -> Vec<u16> {
    let text = raw.get("OptionTID").and_then(|v| v.as_str()).unwrap_or("");
    text.split(';').filter_map(|t| t.trim().parse::<u16>().ok()).filter(|t| *t > 0).take(super::options::MAX_OPTIONS).collect()
}

/// Type d'un consommable (colonnes « ItemType » 1 et « Type »).
pub fn usable_type_of(raw: &serde_json::Value) -> i64 {
    let num = |k: &str| raw.get(k).and_then(|v| v.as_str()).and_then(|t| t.trim().parse::<f64>().ok()).map(|v| v as i64).unwrap_or(0);
    if num("ItemType") == 1 { num("Type") } else { 0 }
}

/// Type et rang d'une pierre d'esprit (colonnes « Spirit Type » et « Rank » d'itemdatatable.txt).
pub fn stone_of(raw: &serde_json::Value) -> Option<(i64, i64)> {
    let num = |k: &str| raw.get(k).and_then(|v| v.as_str()).and_then(|t| t.trim().parse::<f64>().ok()).map(|v| v as i64).unwrap_or(0);
    let kind = num("Spirit Type");
    (kind > 0).then(|| (kind, num("Rank")))
}

/// Caractéristiques d'un équipement (colonnes d'itemdatatable.txt).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EquipData {
    /// Partie du corps (2 corps, 3 tête, 5 mains, 6 jambes, 7 pieds, 9 main gauche, 10 main droite…).
    pub part: i64,
    /// 1 armure, 2 arme, 4 bouclier…
    pub kind: i64,
    /// 2 = à deux mains.
    pub hand: i64,
    pub level: i64,
    /// Races et classes admises (vide : toutes), sexe exigé (0 : les deux).
    pub races: Vec<i64>,
    pub classes: Vec<i64>,
    pub gender: i64,
    pub min_damage: i32,
    pub max_damage: i32,
    pub defense: i32,
    /// Précision (« Physical_ATKRate »), PV en plus.
    pub ar: i32,
    pub hp: i32,
    /// Vitesse et portée d'attaque d'une arme.
    pub atk_speed: i32,
    pub atk_range: i32,
    /// Dégâts (min, max) et défense de chaque élément, dans l'ordre des attributs 1 à 8 (magie, eau, feu,
    /// terre, air, poison, foudre, glace).
    pub spirit_damage: [(i32, i32); 8],
    pub spirit_defense: [i32; 8],
    /// Rang de renforcement (« PhysicalRank », ligne des tables de chance) et sockets au départ et au plus.
    pub physical_rank: i64,
    pub min_socket: i64,
    pub max_socket: i64,
    /// Type d'arme (colonne « Type » : 17 lance).
    pub weapon_type: i64,
    /// Rang de l'objet (« Rank ») et nombre d'options tirées à sa création (« MinOption », « MaxOption »).
    pub rank: i64,
    pub min_option: i64,
    pub max_option: i64,
    /// Groupes d'options liées (« Link_id », séparés par « ; ») d'un objet unique.
    pub link_ids: Vec<i64>,
    /// Durabilité maximale (« DUR ») ; 0 : l'objet ne s'use pas. Une valeur commençant par « @ » marque un objet
    /// inusable (`unbreakable`, le client affiche alors 50 000) : jamais usé, jamais réparé.
    pub durability: i32,
    pub unbreakable: bool,
    /// Monture (partie 14) : vitesse de course à cheval (« Run_Buff ») et modèle de la monture (« SubType »).
    pub run_buff: i32,
    pub ride_tid: i64,
    /// Section d'objets (« Item_section_num ») : l'objet ne compte pas dans les régions de même « ItemSection »
    /// (`regions.rs`) ; 0 : aucune.
    pub section: i64,
    /// Dégâts héroïques (min, max) et défense héroïque (« Heroic_Min_Damage », « Heroic_Max_Damage »,
    /// « Heroic_Defense_Point » : `heroic.rs`).
    pub heroic_min: i32,
    pub heroic_max: i32,
    pub heroic_def: i32,
    /// Esquive en plus (« PDR » des bottes : DR, facteur « points max » 9) ; le client ne lit pas la colonne mais
    /// affiche le DR reçu.
    pub pdr: i32,
    /// Caractéristiques en plus (« CON_Bonus_Point », « STR_… », « INT_… », « DEX_… », « Wis_… » des anneaux), dans
    /// l'ordre constitution, force, intelligence, dextérité, sagesse.
    pub stat_bonus: [i32; 5],
}

impl EquipData {
    /// L'objet s'use et se répare : durabilité maximale positive et pas inusable.
    pub fn wears(&self) -> bool {
        self.durability > 0 && !self.unbreakable
    }

    /// Durabilité actuelle d'un exemplaire : la valeur enregistrée, ou le maximum pour `DURABILITY_FULL`.
    pub fn current_durability(&self, stored: i32) -> i32 {
        if stored < 0 { self.durability } else { stored.min(self.durability) }
    }

    /// Exemplaire cassé (durabilité 0) : il ne compte plus dans les facteurs de son porteur.
    pub fn is_broken(&self, stored: i32) -> bool {
        self.wears() && self.current_durability(stored) == 0
    }
}

/// Préfixe des colonnes élémentaires d'itemdatatable.txt, dans l'ordre des attributs 1 à 8 (« Linghtning »
/// est l'orthographe du fichier).
pub const SPIRIT_COLUMNS: [&str; 8] = ["Magic", "Water", "Fire", "Earth", "Air", "Poison", "Linghtning", "Ice"];

impl EquipData {
    /// Lit les colonnes d'une ligne d'itemdatatable.txt ; `None` si l'objet ne se porte pas.
    pub fn from_raw(raw: &serde_json::Value) -> Option<Self> {
        let text = |k: &str| raw.get(k).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let num = |k: &str| text(k).parse::<f64>().map(|v| v as i64).unwrap_or(0);
        let list = |k: &str| text(k).split(';').filter_map(|x| x.trim().parse::<i64>().ok()).filter(|&x| x > 0).collect::<Vec<_>>();
        let (part, kind) = (num("Part"), num("Kind"));
        if part < 2 || kind <= 0 {
            return None;
        }
        let dur = text("DUR");
        let unbreakable = dur.starts_with('@');
        Some(EquipData {
            part,
            kind,
            hand: num("Hand"),
            level: num("Level"),
            races: list("Race"),
            classes: list("Class"),
            gender: num("Gender"),
            min_damage: num("Physical_Min_Damage") as i32,
            max_damage: num("Physical_Max_Damage") as i32,
            defense: num("Physical_Defense_Point") as i32,
            ar: num("Physical_ATKRate") as i32,
            hp: num("HP") as i32,
            atk_speed: num("ATK Speed") as i32,
            atk_range: num("ATK Range") as i32,
            spirit_damage: SPIRIT_COLUMNS.map(|e| (num(&format!("{e}_Min_Damage")) as i32, num(&format!("{e}_Max_Damage")) as i32)),
            spirit_defense: SPIRIT_COLUMNS.map(|e| num(&format!("{e}_Defense_Point")) as i32),
            physical_rank: num("PhysicalRank"),
            min_socket: num("MinSocket"),
            max_socket: num("MaxSocket"),
            weapon_type: num("Type"),
            rank: num("Rank"),
            min_option: num("MinOption"),
            max_option: num("MaxOption"),
            link_ids: text("Link_id").split(';').filter_map(|t| t.trim().parse::<i64>().ok()).filter(|t| *t > 0).collect(),
            durability: if unbreakable { 0 } else { dur.parse::<f64>().map(|v| v.max(0.0) as i32).unwrap_or(0) },
            unbreakable,
            run_buff: num("Run_Buff") as i32,
            ride_tid: num("SubType"),
            section: num("Item_section_num"),
            heroic_min: num("Heroic_Min_Damage") as i32,
            heroic_max: num("Heroic_Max_Damage") as i32,
            heroic_def: num("Heroic_Defense_Point") as i32,
            pdr: num("PDR") as i32,
            stat_bonus: ["CON_Bonus_Point", "STR_Bonus_Point", "INT_Bonus_Point", "DEX_Bonus_Point", "Wis_Bonus_Point"].map(|k| num(k) as i32),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShopEntry {
    pub tid: u32,
    pub count: i64,
}

/// Compétence lancée par l'utilisation d'un objet (« Use skill id », « Use skill level ») ; « CanStopUsingItem » : son
/// effet s'arrête en réutilisant l'objet (familiers, `summons.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UseSkill {
    pub skill_tid: u32,
    pub level: u32,
    pub can_stop: bool,
}

impl UseSkill {
    /// Première compétence de l'objet (les colonnes sont des listes « ; », voir `consumables::ItemUse`).
    pub fn from_raw(raw: &serde_json::Value) -> Option<Self> {
        let num = |k: &str| raw.get(k).and_then(|v| v.as_str()).and_then(|t| t.split(';').next()?.trim().parse::<f64>().ok()).unwrap_or(0.0);
        let skill_tid = num("Use skill id");
        (skill_tid > 0.0).then(|| UseSkill { skill_tid: skill_tid as u32, level: (num("Use skill level") as u32).max(1), can_stop: num("CanStopUsingItem") != 0.0 })
    }
}

/// Catalogue des objets (prix, piles) et listes des boutiques.
#[derive(Default)]
pub struct ItemCatalog {
    pub items: HashMap<u32, ItemData>,
    pub shops: HashMap<i64, Vec<ShopEntry>>,
    /// Objets qui lancent une compétence à l'utilisation, par TID.
    pub use_skill: HashMap<u32, UseSkill>,
    /// Boîtes de loterie (`lottery.rs`), par TID, et « SubType » des boîtes et des clés.
    pub lottery: HashMap<u32, Vec<super::lottery::Pot>>,
    pub lottery_sub_types: HashMap<u32, i64>,
    /// Costumes (`avatar.rs`) : objet → pièces.
    pub avatars: HashMap<u32, Vec<u32>>,
    /// Potions de métamorphose (type d'usage 7) et leurs bonus (table `item_transform`), par TID (`morph.rs`).
    pub morph: HashMap<u32, super::morph::Potion>,
    /// Potions of Quickness et Beam (type d'usage 12, sous-type 1 ou 2), refusées sous métamorphose.
    pub speed_potions: std::collections::HashSet<u32>,
    /// Consommables à compétence, orbes de résurrection et objets d'oubli des compétences, par TID (`consumables.rs`).
    pub uses: HashMap<u32, super::consumables::ItemUse>,
    /// Objets Cash à durée (temps de jeu ou date d'expiration), par TID (`lifetime.rs`).
    pub lifespans: HashMap<u32, super::lifetime::Lifespan>,
    /// Numéro de quête des objets de quête (« QuestItem »), par TID (`quests.rs`).
    pub quest_items: HashMap<u32, u32>,
}

/// Bonus d'une ligne de la table `item_transform` (ItemTransformTable.txt).
fn transform_bonus(r: &mysql_async::Row) -> super::morph::Bonus {
    super::morph::Bonus {
        movement_run: col(r, "movement_run"),
        atk_speed: col(r, "atk_speed"),
        max_hp: col(r, "max_hp"),
        max_mp: col(r, "max_mp"),
        defense: col(r, "defense"),
        min_damage: col(r, "min_damage"),
        max_damage: col(r, "max_damage"),
    }
}

impl ItemCatalog {
    pub async fn load(db: &Databases) -> anyhow::Result<Self> {
        let rows = db.world.rows("SELECT tid, name, npc_price, pc_price, npc_charisma_price, npc_shrine_coin, stack, raw FROM item_data", ()).await?;
        let use_skill: HashMap<u32, UseSkill> = rows
            .iter()
            .filter_map(|r| {
                let raw = serde_json::from_str::<serde_json::Value>(&col::<String>(r, "raw")).ok()?;
                UseSkill::from_raw(&raw).map(|u| (col::<u32>(r, "tid"), u))
            })
            .collect();
        let items: HashMap<u32, ItemData> = rows
            .iter()
            .map(|r| {
                let tid: u32 = col(r, "tid");
                let raw = serde_json::from_str::<serde_json::Value>(&col::<String>(r, "raw")).ok();
                let equip = raw.as_ref().and_then(EquipData::from_raw);
                let potion = raw.as_ref().and_then(super::recovery::Potion::from_raw);
                let scroll = raw.as_ref().and_then(super::travel::Scroll::from_raw);
                let stone = raw.as_ref().and_then(stone_of);
                let usable_type = raw.as_ref().map(usable_type_of).unwrap_or(0);
                let option_tids = raw.as_ref().map(option_tids_of).unwrap_or_default();
                let column = |k: &str| raw.as_ref().and_then(|v| v.get(k)?.as_str()?.trim().parse::<i64>().ok()).unwrap_or(0);
                let (kind, bound_type, cash, enable_on_ride) = (column("Kind"), column("BoundType"), column("Cash"), column("EnableOnRide") != 0);
                let (auction_category, event_item) = ((column("FirstCategory"), column("SecondCategory")), column("EventItem") != 0);
                (tid, ItemData { tid, name: col(r, "name"), npc_price: col(r, "npc_price"), pc_price: col(r, "pc_price"), charisma_price: col(r, "npc_charisma_price"), coin_price: col(r, "npc_shrine_coin"), stack: col::<i64>(r, "stack").max(1), equip, potion, scroll, stone, usable_type, option_tids, kind, bound_type, cash, enable_on_ride, auction_category, event_item })
            })
            .collect();
        let raws: Vec<(u32, serde_json::Value)> =
            rows.iter().filter_map(|r| Some((col::<u32>(r, "tid"), serde_json::from_str::<serde_json::Value>(&col::<String>(r, "raw")).ok()?))).collect();
        let mut morph: HashMap<u32, super::morph::Potion> = raws.iter().filter_map(|(tid, raw)| super::morph::Potion::from_raw(raw).map(|p| (*tid, p))).collect();
        let uses: HashMap<u32, super::consumables::ItemUse> = raws.iter().filter_map(|(tid, raw)| super::consumables::ItemUse::from_raw(raw).map(|u| (*tid, u))).collect();
        let lifespans: HashMap<u32, super::lifetime::Lifespan> = raws.iter().filter_map(|(tid, raw)| super::lifetime::Lifespan::from_raw(raw).map(|l| (*tid, l))).collect();
        let quest_items: HashMap<u32, u32> = raws
            .iter()
            .filter_map(|(tid, raw)| raw.get("QuestItem")?.as_str()?.trim().parse::<u32>().ok().filter(|q| *q > 0).map(|q| (*tid, q)))
            .collect();
        let speed_potions = raws
            .iter()
            .filter(|(_, raw)| usable_type_of(raw) == super::morph::USABLE_SPEED && matches!(raw.get("SubType").and_then(|v| v.as_str()).map(str::trim), Some("1" | "2")))
            .map(|(tid, _)| *tid)
            .collect();
        match db.world.rows("SELECT * FROM item_transform", ()).await {
            Ok(rows) => {
                for r in &rows {
                    if let Some(p) = morph.get_mut(&col::<u32>(r, "item_tid")) {
                        p.bonus = transform_bonus(r);
                    }
                }
            }
            Err(_) => crate::warn!("bonus des potions de métamorphose non chargés : lancer `archcore db-init` puis `archcore import`"),
        }
        let lists = db.world.rows("SELECT template_id, tid, count FROM npc_trade_list ORDER BY template_id, idx", ()).await?;
        let mut shops: HashMap<i64, Vec<ShopEntry>> = HashMap::new();
        for r in &lists {
            shops.entry(col(r, "template_id")).or_default().push(ShopEntry { tid: col(r, "tid"), count: col::<i64>(r, "count").max(1) });
        }
        info!(
            "{} objet(s) et {} boutique(s) chargé(s), {} objet(s) à compétence, {} potion(s) de métamorphose, {} consommable(s) à compétence ou de résurrection",
            items.len(),
            shops.len(),
            use_skill.len(),
            morph.len(),
            uses.len()
        );
        info!("{} objet(s) à durée, {} objet(s) de quête", lifespans.len(), quest_items.len());
        // Boîtes de loterie et costumes : tables de l'épisode 8, textes gardés dans `client_table`.
        let text_of = |name: &'static str| async move {
            let row = db.world.first("SELECT data FROM client_table WHERE row_idx = 0 AND name = ?", (name,)).await.ok().flatten();
            row.and_then(|r| serde_json::from_str::<String>(&col::<String>(&r, "data")).ok()).unwrap_or_default()
        };
        let mut lottery = super::lottery::parse(&text_of("itemlotterybox.txt").await);
        lottery.retain(|tid, pots| {
            pots.retain(|p| items.contains_key(&p.tid));
            items.contains_key(tid) && !pots.is_empty()
        });
        let mut avatars = super::avatar::parse(&text_of("avatarset.ini").await);
        avatars.retain(|tid, pieces| {
            pieces.retain(|p| items.contains_key(p));
            items.contains_key(tid) && !pieces.is_empty()
        });
        let lottery_sub_types = raws
            .iter()
            .filter(|(_, raw)| matches!(usable_type_of(raw), super::lottery::USABLE_BOX | super::lottery::USABLE_KEY))
            .filter_map(|(tid, raw)| Some((*tid, raw.get("SubType")?.as_str()?.trim().parse::<i64>().ok()?)))
            .collect();
        info!("{} boîte(s) de loterie, {} costume(s)", lottery.len(), avatars.len());
        Ok(ItemCatalog { items, shops, use_skill, lottery, lottery_sub_types, avatars, morph, speed_potions, uses, lifespans, quest_items })
    }

    pub fn get(&self, tid: u32) -> Option<&ItemData> {
        self.items.get(&tid)
    }

    pub fn shop(&self, template: i64) -> &[ShopEntry] {
        self.shops.get(&template).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Objet rangé dans l'inventaire Cash : « Cash » non nul, sauf un objet qui se porte (choix d'ArchCore, voir l'en-tête).
    pub fn is_cash(&self, tid: u32) -> bool {
        self.get(tid).is_some_and(|d| d.cash != 0 && d.equip.is_none())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InventoryItem {
    pub iid: u32,
    pub tid: u32,
    pub count: i64,
    pub grid: Grid,
    /// Renforcement, sockets et pierres serties.
    pub conv: super::convert::ItemConvert,
    /// TID des options (5 au plus).
    pub options: Vec<u16>,
    /// Durabilité enregistrée : `DURABILITY_FULL` (-1) pour « pleine » (colonne DUR du modèle), sinon la valeur
    /// actuelle (voir `repair.rs`).
    pub durability: i32,
}

/// Colonnes d'état lues avec un objet : conversion, options et durabilité.
pub const CONVERT_COLUMNS: &str = "convert_level, socket_count, socket_tids, option_tids, durability";

/// Durabilité « pleine » en base (`character_items.durability`) : la colonne DUR du modèle.
pub const DURABILITY_FULL: i32 = -1;

/// Durabilité d'une ligne de `character_items` (colonne `durability`).
pub fn durability_of(r: &mysql_async::Row) -> i32 {
    col(r, "durability")
}

/// Options d'une ligne de `character_items` (colonne `option_tids`).
pub fn options_of(r: &mysql_async::Row) -> Vec<u16> {
    super::options::options_from_column(&col::<String>(r, "option_tids"))
}

/// État de conversion d'une ligne de `character_items` (colonnes `CONVERT_COLUMNS`).
pub fn convert_of(r: &mysql_async::Row) -> super::convert::ItemConvert {
    super::convert::ItemConvert::from_columns(col(r, "convert_level"), col(r, "socket_count"), &col::<String>(r, "socket_tids"))
}

/// Inventaire d'un personnage en jeu, synchronisé avec `character_items`.
#[derive(Clone, Debug, Default)]
pub struct Inventory {
    pub character_id: u32,
    pub items: BTreeMap<u32, InventoryItem>,
    /// Objets équipés, par partie du corps (`grid` = [partie, 0, 0]).
    pub equipment: BTreeMap<i64, InventoryItem>,
    /// Inventaire Cash (statut 51) : `grid` = [0, rang, 0].
    pub cash: BTreeMap<u32, InventoryItem>,
    /// Sac du familier (statut 13) : `grid` = [sac, ligne, colonne].
    pub sub: BTreeMap<u32, InventoryItem>,
    /// Objets Cash en cours d'utilisation (octet « inuse » du mini Cash) ; jamais enregistré : le familier ne revient
    /// pas de lui-même à l'entrée en jeu.
    pub in_use: BTreeSet<u32>,
    /// Costume porté (`avatar.rs`) : l'objet de l'inventaire Cash, et ses pièces, qui ne comptent pas dans les facteurs.
    pub avatar_item: Option<u32>,
    pub avatar: Vec<InventoryItem>,
    /// Le sac du familier a été envoyé au client pendant cette session (`pet_bag.rs`).
    pub sub_sent: bool,
    /// Ramassage automatique du sac du familier coupé (op 30 à 0 ; `characters.pet_auto_pick`).
    pub no_auto_pick: bool,
    /// Durée de vie commencée des objets à durée (colonnes `remain_ms`, `expire_at` : `lifetime.rs`), par IID.
    pub life: BTreeMap<u32, super::lifetime::Life>,
}

/// Toutes les cases d'une grille (sac, ligne, colonne), dans l'ordre.
fn cells(dims: (i64, i64, i64)) -> impl Iterator<Item = Grid> {
    let (l, r, c) = dims;
    (0..l).flat_map(move |a| (0..r).flat_map(move |b| (0..c).map(move |d| [a, b, d])))
}

/// Case libre dans `dims` parmi `items`.
pub fn is_free_in(items: &BTreeMap<u32, InventoryItem>, dims: (i64, i64, i64), g: Grid) -> bool {
    let (l, r, c) = dims;
    (0..l).contains(&g[0]) && (0..r).contains(&g[1]) && (0..c).contains(&g[2]) && !items.values().any(|i| i.grid == g)
}

/// Première case libre de `items` dans `dims`, dans le sac `layer` s'il est donné.
pub fn free_in(items: &BTreeMap<u32, InventoryItem>, dims: (i64, i64, i64), layer: Option<i64>) -> Option<Grid> {
    cells(dims).filter(|g| layer.is_none_or(|l| g[0] == l)).find(|g| is_free_in(items, dims, *g))
}

impl Inventory {
    pub async fn load(db: &Databases, character_id: u32) -> anyhow::Result<Self> {
        let rows = db
            .characters
            .rows(
                &format!("SELECT id, template_id, status, layer, row_idx, col_idx, count, {CONVERT_COLUMNS} FROM character_items WHERE character_id = ? AND status IN (?, ?, ?, ?)"),
                (character_id, stored_status::INVENTORY, stored_status::EQUIP, stored_status::CASH_INVENTORY, stored_status::SUB_INVENTORY),
            )
            .await?;
        let mut inv = Inventory { character_id, ..Default::default() };
        for r in &rows {
            let iid: u32 = col(r, "id");
            let item = InventoryItem { iid, tid: col(r, "template_id"), count: col(r, "count"), grid: [col(r, "layer"), col(r, "row_idx"), col(r, "col_idx")], conv: convert_of(r), options: options_of(r), durability: durability_of(r) };
            match col::<i64>(r, "status") {
                stored_status::EQUIP => inv.equipment.insert(item.grid[0], item),
                stored_status::CASH_INVENTORY => inv.cash.insert(iid, item),
                stored_status::SUB_INVENTORY => inv.sub.insert(iid, item),
                _ => inv.items.insert(iid, item),
            };
        }
        // Objets à durée commencés (lifetime.rs) ; rien si les colonnes manquent (base pas encore migrée).
        if let Ok(rows) = db.characters.rows("SELECT id, remain_ms, expire_at FROM character_items WHERE character_id = ? AND (remain_ms <> 0 OR expire_at <> 0)", (character_id,)).await {
            for r in &rows {
                inv.life.insert(col(r, "id"), super::lifetime::Life { remain_ms: col(r, "remain_ms"), expire_at: col(r, "expire_at") });
            }
        }
        // Ramassage automatique du sac du familier : actif si la colonne manque (base pas encore migrée).
        if let Ok(Some(r)) = db.characters.first("SELECT pet_auto_pick FROM characters WHERE id = ?", (character_id,)).await {
            inv.no_auto_pick = col::<i64>(&r, "pet_auto_pick") == 0;
        }
        Ok(inv)
    }

    /// Range dans l'inventaire Cash les objets Cash encore enregistrés dans l'inventaire ordinaire (objets d'avant
    /// l'inventaire Cash, reçus hors ligne, par courrier ou par échange) ; leur case de l'inventaire se libère. Rend
    /// leur nombre.
    pub async fn sort_cash(&mut self, db: &Databases, catalog: &ItemCatalog) -> anyhow::Result<usize> {
        let moved: Vec<u32> = self.items.values().filter(|i| catalog.is_cash(i.tid)).map(|i| i.iid).collect();
        let mut n = 0;
        for iid in moved {
            let Some(g) = free_in(&self.cash, CASH_INVENTORY, None) else { break };
            db.characters
                .exec("UPDATE character_items SET status = ?, layer = ?, row_idx = ?, col_idx = ? WHERE id = ?", (stored_status::CASH_INVENTORY, g[0], g[1], g[2], iid))
                .await?;
            let mut item = self.items.remove(&iid).unwrap();
            item.grid = g;
            self.cash.insert(iid, item);
            n += 1;
        }
        Ok(n)
    }

    /// Ajoute `count` objets Cash à l'inventaire Cash : sur une pile existante si possible, sinon à la première place
    /// libre. Rend (objet, créé ?) ou `None` s'il est plein.
    pub async fn add_cash(&mut self, db: &Databases, tid: u32, count: i64, max_stack: i64) -> anyhow::Result<Option<(InventoryItem, bool)>> {
        self.add_to(db, tid, count, max_stack, stored_status::CASH_INVENTORY).await
    }

    /// L'inventaire Cash peut-il recevoir `count` objets `tid` (pile existante ou place libre) ?
    pub fn cash_has_room(&self, tid: u32, count: i64, max_stack: i64) -> bool {
        self.cash.values().any(|i| max_stack > 1 && i.tid == tid && i.count + count <= max_stack) || free_in(&self.cash, CASH_INVENTORY, None).is_some()
    }

    /// Ajoute `count` objets au sac du familier (pile existante, sinon première case libre). `None` s'il est plein.
    pub async fn add_sub(&mut self, db: &Databases, tid: u32, count: i64, max_stack: i64) -> anyhow::Result<Option<(InventoryItem, bool)>> {
        self.add_to(db, tid, count, max_stack, stored_status::SUB_INVENTORY).await
    }

    /// Ajout à la grille du statut enregistré `stored` (Cash ou sac du familier).
    async fn add_to(&mut self, db: &Databases, tid: u32, count: i64, max_stack: i64, stored: i64) -> anyhow::Result<Option<(InventoryItem, bool)>> {
        let character_id = self.character_id;
        let (map, dims) = if stored == stored_status::CASH_INVENTORY { (&mut self.cash, CASH_INVENTORY) } else { (&mut self.sub, SUB_INVENTORY) };
        if let Some(pile) = map.values_mut().find(|i| max_stack > 1 && i.tid == tid && i.count + count <= max_stack) {
            pile.count += count;
            db.characters.exec("UPDATE character_items SET count = ? WHERE id = ?", (pile.count, pile.iid)).await?;
            return Ok(Some((pile.clone(), false)));
        }
        let Some(g) = free_in(map, dims, None) else { return Ok(None) };
        let iid = db.characters.exec(INSERT_ITEM, (character_id, tid, stored, g[0], g[1], g[2], count)).await? as u32;
        let item = InventoryItem { iid, tid, count, grid: g, conv: Default::default(), options: Vec::new(), durability: DURABILITY_FULL };
        map.insert(iid, item.clone());
        Ok(Some((item, true)))
    }

    /// Objet que le joueur peut utiliser (AGPMITEM op 3) : de l'inventaire, de l'inventaire Cash ou du sac du familier.
    pub fn get_usable(&self, iid: u32) -> Option<&InventoryItem> {
        self.items.get(&iid).or_else(|| self.cash.get(&iid)).or_else(|| self.sub.get(&iid))
    }

    /// Statut AGPMITEM d'un objet de l'inventaire, de l'inventaire Cash ou du sac du familier.
    pub fn status_of(&self, iid: u32) -> Option<i64> {
        if self.items.contains_key(&iid) {
            Some(item_status::INVENTORY)
        } else if self.cash.contains_key(&iid) {
            Some(item_status::CASH_INVENTORY)
        } else if self.sub.contains_key(&iid) {
            Some(item_status::SUB_INVENTORY)
        } else {
            None
        }
    }

    /// AGPMITEM op 2 de la quantité d'un objet, avec le statut de sa grille.
    pub fn count_packet(&self, item: &InventoryItem, owner: u32) -> Vec<u8> {
        item_count_in(item, owner, self.status_of(item.iid).unwrap_or(item_status::INVENTORY))
    }

    /// Grille qui tient l'objet `iid` (inventaire, Cash, sac du familier).
    fn holder(&mut self, iid: u32) -> Option<&mut BTreeMap<u32, InventoryItem>> {
        if self.items.contains_key(&iid) {
            Some(&mut self.items)
        } else if self.cash.contains_key(&iid) {
            Some(&mut self.cash)
        } else if self.sub.contains_key(&iid) {
            Some(&mut self.sub)
        } else {
            None
        }
    }

    pub fn get(&self, iid: u32) -> Option<&InventoryItem> {
        self.items.get(&iid)
    }

    pub fn is_free(&self, g: Grid) -> bool {
        let (l, r, c) = INVENTORY;
        if g[0] < 0 || g[0] >= l || g[1] < 0 || g[1] >= r || g[2] < 0 || g[2] >= c {
            return false;
        }
        !self.items.values().any(|i| i.grid == g)
    }

    pub fn free_slot(&self) -> Option<Grid> {
        let (l, r, c) = INVENTORY;
        (0..l).flat_map(|a| (0..r).flat_map(move |b| (0..c).map(move |d| [a, b, d]))).find(|g| self.is_free(*g))
    }

    /// Pile existante du même objet pouvant recevoir `count` de plus.
    fn stack_for(&self, tid: u32, count: i64, max_stack: i64) -> Option<u32> {
        if max_stack <= 1 {
            return None;
        }
        self.items.values().find(|i| i.tid == tid && i.count + count <= max_stack).map(|i| i.iid)
    }

    /// Ajoute `count` objets : sur une pile existante si possible, sinon dans la case voulue (si libre)
    /// ou la première case libre. Rend (objet, créé ?) ou `None` si l'inventaire est plein.
    pub async fn add(&mut self, db: &Databases, tid: u32, count: i64, max_stack: i64, wanted: Option<Grid>) -> anyhow::Result<Option<(InventoryItem, bool)>> {
        let Some(place) = self.placement(tid, count, max_stack, wanted) else { return Ok(None) };
        let (sql, params) = self.placement_sql(tid, count, place);
        let iid = db.characters.exec(sql, params).await? as u32;
        Ok(Some(self.apply_placement(tid, count, place, iid)))
    }

    /// Place de `count` objets ajoutés : pile existante si possible, sinon case voulue (si libre) ou première
    /// case libre ; `None` si l'inventaire est plein. Rien n'est encore changé.
    pub fn placement(&self, tid: u32, count: i64, max_stack: i64, wanted: Option<Grid>) -> Option<Placement> {
        if let Some(iid) = self.stack_for(tid, count, max_stack) {
            return Some(Placement::Stack(iid, self.items[&iid].count + count));
        }
        wanted.filter(|g| self.is_free(*g)).or_else(|| self.free_slot()).map(Placement::Slot)
    }

    /// Requête qui enregistre une place ; pour une nouvelle case, c'est une insertion (à mettre en dernier
    /// dans une transaction pour en avoir l'identifiant).
    pub fn placement_sql(&self, tid: u32, count: i64, place: Placement) -> (&'static str, Params) {
        match place {
            Placement::Stack(iid, total) => ("UPDATE character_items SET count = ? WHERE id = ?", (total, iid).into()),
            Placement::Slot(g) => (INSERT_ITEM, (self.character_id, tid, stored_status::INVENTORY, g[0], g[1], g[2], count).into()),
        }
    }

    /// Reporte en mémoire une place enregistrée (`new_iid` : identifiant inséré, pour une nouvelle case).
    /// Rend (objet, créé ?).
    pub fn apply_placement(&mut self, tid: u32, count: i64, place: Placement, new_iid: u32) -> (InventoryItem, bool) {
        match place {
            Placement::Stack(iid, total) => {
                let item = self.items.get_mut(&iid).unwrap();
                item.count = total;
                (item.clone(), false)
            }
            Placement::Slot(grid) => {
                let item = InventoryItem { iid: new_iid, tid, count, grid, conv: Default::default(), options: Vec::new(), durability: DURABILITY_FULL };
                self.items.insert(new_iid, item.clone());
                (item, true)
            }
        }
    }

    /// Retire un objet de l'inventaire, de l'inventaire Cash ou du sac du familier.
    pub async fn remove(&mut self, db: &Databases, iid: u32) -> anyhow::Result<()> {
        if let Some(m) = self.holder(iid) {
            m.remove(&iid);
        }
        self.in_use.remove(&iid);
        self.life.remove(&iid);
        db.characters.exec("DELETE FROM character_items WHERE id = ? AND character_id = ?", (iid, self.character_id)).await?;
        Ok(())
    }

    /// Paquets AGPMITEM d'ajout de tout l'inventaire, de l'inventaire Cash et de l'équipement (entrée en jeu). Le sac
    /// du familier n'en est pas : le client n'en range les objets que si le sac est disponible (`pet_bag.rs`).
    pub fn add_packets(&self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        for i in self.items.values() {
            out.push(item_add_of(i, self.character_id, item_status::INVENTORY, Some(i.grid)));
            out.extend(convert_add(i, self.character_id));
        }
        for i in self.cash.values() {
            let cash = super::lifetime::cash_mini_of(self, i.iid);
            out.push(item_add_with_cash(i, self.character_id, item_status::CASH_INVENTORY, None, Some(cash)));
            out.extend(convert_add(i, self.character_id));
        }
        for i in self.equipment.values() {
            out.push(item_add_of(i, self.character_id, item_status::EQUIP, None));
            out.extend(convert_add(i, self.character_id));
        }
        out.extend(self.avatar.iter().map(|i| item_add_of(i, self.character_id, item_status::EQUIP, None)));
        out
    }

    /// Paquets AGPMITEM d'ajout de l'équipement porté (pour le porteur ou ceux qui le voient).
    pub fn equip_packets(&self) -> Vec<Vec<u8>> {
        self.equipment.values().chain(self.avatar.iter()).map(|i| item_add_of(i, self.character_id, item_status::EQUIP, None)).collect()
    }

    pub fn free_count(&self) -> usize {
        let (l, r, c) = INVENTORY;
        (l * r * c) as usize - self.items.len()
    }

    /// Place un objet de l'inventaire sur la partie `part`, après avoir rangé dans l'inventaire les objets
    /// portés sur `displaced`. Rend (objet équipé, objets rangés) ; `None` si l'objet n'est pas dans
    /// l'inventaire ou s'il n'y a pas assez de place pour ranger.
    pub async fn equip(&mut self, db: &Databases, iid: u32, part: i64, displaced: &[i64]) -> anyhow::Result<Option<(InventoryItem, Vec<InventoryItem>)>> {
        if !self.items.contains_key(&iid) || self.free_count() + 1 < displaced.len() {
            return Ok(None);
        }
        let mut item = self.items.remove(&iid).unwrap();
        let mut stored = Vec::new();
        for p in displaced {
            if let Some(moved) = self.unequip(db, *p, None).await? {
                stored.push(moved);
            }
        }
        item.grid = [part, 0, 0];
        db.characters
            .exec("UPDATE character_items SET status = ?, layer = ?, row_idx = 0, col_idx = 0 WHERE id = ?", (stored_status::EQUIP, part, iid))
            .await?;
        self.equipment.insert(part, item.clone());
        Ok(Some((item, stored)))
    }

    /// Range dans l'inventaire l'objet porté sur `part` : dans la case voulue si elle est libre, sinon la
    /// première libre. `None` si rien n'est porté là ou si l'inventaire est plein.
    pub async fn unequip(&mut self, db: &Databases, part: i64, wanted: Option<Grid>) -> anyhow::Result<Option<InventoryItem>> {
        let Some(grid) = wanted.filter(|g| self.is_free(*g)).or_else(|| self.free_slot()) else { return Ok(None) };
        let Some(mut item) = self.equipment.remove(&part) else { return Ok(None) };
        item.grid = grid;
        db.characters
            .exec(
                "UPDATE character_items SET status = ?, layer = ?, row_idx = ?, col_idx = ? WHERE id = ?",
                (stored_status::INVENTORY, grid[0], grid[1], grid[2], item.iid),
            )
            .await?;
        self.items.insert(item.iid, item.clone());
        Ok(Some(item))
    }

    /// Retire `n` exemplaires d'un objet de l'inventaire, de l'inventaire Cash ou du sac du familier : rend la pile
    /// restante, ou `None` s'il n'en reste rien.
    pub async fn consume(&mut self, db: &Databases, iid: u32, n: i64) -> anyhow::Result<Option<InventoryItem>> {
        let Some(item) = self.holder(iid).and_then(|m| m.get_mut(&iid)) else { return Ok(None) };
        if item.count > n {
            item.count -= n;
            db.characters.exec("UPDATE character_items SET count = ? WHERE id = ?", (item.count, iid)).await?;
            return Ok(Some(item.clone()));
        }
        self.remove(db, iid).await?;
        Ok(None)
    }

    /// Requête qui retire `n` exemplaires de l'objet `iid` (inventaire, Cash, sac du familier) : quantité diminuée, ou
    /// ligne supprimée s'il n'en reste rien. À joindre à une transaction, puis à reporter par `consumed`.
    pub fn consume_sql(&self, iid: u32, n: i64) -> Option<(&'static str, Params)> {
        let item = self.get_usable(iid)?;
        Some(if item.count > n {
            ("UPDATE character_items SET count = ? WHERE id = ?", (item.count - n, iid).into())
        } else {
            ("DELETE FROM character_items WHERE id = ? AND character_id = ?", (iid, self.character_id).into())
        })
    }

    /// Reporte en mémoire un retrait enregistré par `consume_sql` : rend la pile restante, ou `None` s'il n'en reste rien.
    pub fn consumed(&mut self, iid: u32, n: i64) -> Option<InventoryItem> {
        let item = self.holder(iid)?.get_mut(&iid)?;
        if item.count > n {
            item.count -= n;
            return Some(item.clone());
        }
        self.holder(iid).map(|m| m.remove(&iid));
        self.in_use.remove(&iid);
        self.life.remove(&iid);
        None
    }

    /// Enregistre les options et les sockets tirés pour l'objet `iid` de l'inventaire (ou de l'inventaire Cash, du sac
    /// du familier).
    pub async fn set_generated(&mut self, db: &Databases, iid: u32, options: &[u16], sockets: u8) -> anyhow::Result<()> {
        if options.is_empty() && sockets == 0 {
            return Ok(());
        }
        let column = super::options::options_column(options);
        db.characters.exec("UPDATE character_items SET option_tids = ?, socket_count = ? WHERE id = ?", (column, sockets, iid)).await?;
        if let Some(i) = self.holder(iid).and_then(|m| m.get_mut(&iid)) {
            i.options = options.to_vec();
            i.conv.sockets = sockets;
        }
        Ok(())
    }

    /// Objet de l'inventaire posé sur cette case.
    pub fn at(&self, g: Grid) -> Option<&InventoryItem> {
        self.items.values().find(|i| i.grid == g)
    }

    /// Verse la pile `from` dans la pile `into` du même objet, si le total tient dans `max_stack` : rend la
    /// pile réunie (`from` disparaît).
    pub async fn merge(&mut self, db: &Databases, from: u32, into: u32, max_stack: i64) -> anyhow::Result<Option<InventoryItem>> {
        let (Some(a), Some(b)) = (self.items.get(&from), self.items.get(&into)) else { return Ok(None) };
        if from == into || a.tid != b.tid || a.count + b.count > max_stack {
            return Ok(None);
        }
        let total = a.count + b.count;
        db.characters
            .atomic(vec![
                ("DELETE FROM character_items WHERE id = ? AND character_id = ?", (from, self.character_id).into()),
                ("UPDATE character_items SET count = ? WHERE id = ?", (total, into).into()),
            ])
            .await?;
        self.items.remove(&from);
        let target = self.items.get_mut(&into).unwrap();
        target.count = total;
        Ok(Some(target.clone()))
    }

    /// Sépare `n` exemplaires d'une pile vers une case libre : rend (pile d'origine, nouvelle pile).
    pub async fn split(&mut self, db: &Databases, iid: u32, n: i64, grid: Grid) -> anyhow::Result<Option<(InventoryItem, InventoryItem)>> {
        let Some(item) = self.items.get(&iid) else { return Ok(None) };
        if n <= 0 || n >= item.count || !self.is_free(grid) {
            return Ok(None);
        }
        let (tid, left) = (item.tid, item.count - n);
        let new_iid = db
            .characters
            .atomic(vec![
                ("UPDATE character_items SET count = ? WHERE id = ?", (left, iid).into()),
                (INSERT_ITEM, (self.character_id, tid, stored_status::INVENTORY, grid[0], grid[1], grid[2], n).into()),
            ])
            .await? as u32;
        let part = InventoryItem { iid: new_iid, tid, count: n, grid, conv: Default::default(), options: Vec::new(), durability: DURABILITY_FULL };
        self.items.insert(new_iid, part.clone());
        let origin = self.items.get_mut(&iid).unwrap();
        origin.count = left;
        Ok(Some((origin.clone(), part)))
    }

    /// Déplace un objet de l'inventaire vers une case libre.
    pub async fn move_to(&mut self, db: &Databases, iid: u32, grid: Grid) -> anyhow::Result<Option<InventoryItem>> {
        if !self.items.contains_key(&iid) || !self.is_free(grid) {
            return Ok(None);
        }
        let item = self.items.get_mut(&iid).unwrap();
        item.grid = grid;
        db.characters.exec("UPDATE character_items SET layer = ?, row_idx = ?, col_idx = ? WHERE id = ?", (grid[0], grid[1], grid[2], iid)).await?;
        Ok(Some(item.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premiere_case_libre() {
        let mut inv = Inventory { character_id: 1, ..Default::default() };
        assert_eq!(inv.free_slot(), Some([0, 0, 0]));
        inv.items.insert(3, InventoryItem { iid: 3, tid: 190, count: 5, grid: [0, 0, 0], conv: Default::default(), options: Vec::new(), durability: DURABILITY_FULL });
        assert_eq!(inv.free_slot(), Some([0, 0, 1]));
        assert_eq!(inv.stack_for(190, 10, 9999), Some(3));
        assert_eq!(inv.stack_for(190, 10, 1), None);
        assert!(inv.is_free([4, 4, 4]));
        assert!(!inv.is_free([5, 0, 0]));
        assert!(!inv.is_free([0, 5, 0]));
        assert!(!inv.is_free([0, 0, 5]));
        assert_eq!(inv.free_count(), 124);
    }

    #[test]
    fn place_des_objets_ajoutes() {
        let mut inv = Inventory { character_id: 1, ..Default::default() };
        inv.items.insert(3, InventoryItem { iid: 3, tid: 190, count: 5, grid: [0, 0, 0], conv: Default::default(), options: Vec::new(), durability: DURABILITY_FULL });
        assert_eq!(inv.placement(190, 10, 9999, None), Some(Placement::Stack(3, 15)));
        assert_eq!(inv.placement(190, 10, 1, Some([0, 2, 2])), Some(Placement::Slot([0, 2, 2])));
        assert_eq!(inv.placement(191, 1, 1, Some([0, 0, 0])), Some(Placement::Slot([0, 0, 1])), "case voulue occupée");
        assert_eq!(inv.items[&3].count, 5, "rien n'est changé avant l'enregistrement");
        let (item, created) = inv.apply_placement(190, 10, Placement::Stack(3, 15), 0);
        assert_eq!((item.count, created), (15, false));
        let (item, created) = inv.apply_placement(191, 1, Placement::Slot([0, 0, 1]), 42);
        assert_eq!((item.iid, item.grid, created), (42, [0, 0, 1], true));
        assert_eq!(inv.items.len(), 2);
    }

    #[test]
    fn retrait_en_transaction() {
        let mut inv = Inventory { character_id: 1, ..Default::default() };
        let item = |iid: u32, count: i64| InventoryItem { iid, tid: 11256, count, grid: [0, 0, 0], conv: Default::default(), options: Vec::new(), durability: DURABILITY_FULL };
        inv.cash.insert(5, item(5, 2));
        inv.in_use.insert(5);
        assert!(inv.consume_sql(5, 1).is_some_and(|(q, _)| q.starts_with("UPDATE")));
        assert_eq!(inv.consumed(5, 1).map(|i| i.count), Some(1), "pile de l'inventaire Cash diminuée");
        assert!(inv.consume_sql(5, 1).is_some_and(|(q, _)| q.starts_with("DELETE")));
        assert_eq!(inv.consumed(5, 1), None);
        assert!(inv.cash.is_empty() && inv.in_use.is_empty(), "objet retiré, plus allumé");
        assert!(inv.consume_sql(5, 1).is_none());
    }

    #[test]
    fn case_occupee() {
        let mut inv = Inventory { character_id: 1, ..Default::default() };
        inv.items.insert(3, InventoryItem { iid: 3, tid: 190, count: 5, grid: [0, 1, 2], conv: Default::default(), options: Vec::new(), durability: DURABILITY_FULL });
        assert_eq!(inv.at([0, 1, 2]).map(|i| i.iid), Some(3));
        assert!(inv.at([0, 0, 0]).is_none());
    }

    #[test]
    fn caracteristiques_d_equipement() {
        let sword = serde_json::json!({"Part":"10","Kind":"2","Hand":"1","Level":"1","Race":"1;2;3","Class":"1","Gender":"",
            "Physical_Min_Damage":"11","Physical_Max_Damage":"13","ATK Speed":"71","ATK Range":"180"});
        let e = EquipData::from_raw(&sword).unwrap();
        assert_eq!((e.part, e.kind, e.level, e.races.clone(), e.classes.clone(), e.gender), (10, 2, 1, vec![1, 2, 3], vec![1], 0));
        assert_eq!((e.min_damage, e.max_damage, e.atk_speed, e.atk_range), (11, 13, 71, 180));
        let armor = serde_json::json!({"Part":"2","Kind":"1","Level":"2","Race":"1","Class":"1","Physical_Defense_Point":"4"});
        assert_eq!(EquipData::from_raw(&armor).unwrap().defense, 4);
        assert!(EquipData::from_raw(&serde_json::json!({"Part":"0","Kind":"0"})).is_none(), "potion");
    }

    #[test]
    fn iid_de_boutique_hors_de_la_base() {
        let a = alloc_shop_iid();
        assert!(a > SHOP_IID_BASE && a < SHOP_IID_BASE + SHOP_IID_SPAN);
        assert_ne!(a, alloc_shop_iid());
    }
}
