//! Objets : catalogue (`item_data`, `npc_trade_list`), inventaire des personnages et paquets AGPMITEM.
//!
//! Le client ne connaît un objet qu'après un AGPMITEM op 0 (ajout) qui lui donne son IID, son modèle
//! (TID), sa quantité et sa case. Il le crée à partir de son propre modèle (`ItemTemplateAll.ini`) : un
//! TID qu'il ne connaît pas est ignoré. Les objets d'inventaire gardent comme IID leur identifiant en
//! base ; les objets montrés par une boutique reçoivent des IID d'une plage à part, jamais enregistrés.

use crate::db::{col, Databases};
use crate::info;
use archcore_net::schema::{self, mini};
use archcore_net::{build_mini, build_packet, Fields};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU32, Ordering};

pub mod item_op {
    pub const ADD: i64 = 0;
    pub const REMOVE: i64 = 1;
    pub const UPDATE: i64 = 2;
}

/// Statut d'un objet (champ 1 d'AGPMITEM).
pub mod item_status {
    pub const INVENTORY: i64 = 2;
    pub const NPC_TRADE: i64 = 10;
}

/// Statut enregistré en base (colonne `character_items.status`), indépendant du protocole.
pub mod stored_status {
    pub const INVENTORY: i64 = 0;
}

/// Inventaire : 4 sacs de 4 × 4 cases utilisés (la grille du client en a 5 × 5 × 5).
pub const INVENTORY: (i64, i64, i64) = (4, 4, 4);
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

fn grid_mini(g: Grid) -> Vec<u8> {
    let s = &mini::ITEM_GRID;
    build_mini(s, &Fields::new(s).with(0, g[0]).with(1, g[1]).with(2, g[2]))
}

/// AGPMITEM op 0 (ajout). `grid` = [sac, ligne, colonne] pour l'inventaire ou une boutique.
pub fn item_add(iid: u32, tid: u32, owner: Option<u32>, count: i64, status: i64, grid: Option<Grid>) -> Vec<u8> {
    let s = &schema::ITEM;
    let f = Fields::new(s).with(0, item_op::ADD).with(1, status).with(2, iid).with(3, tid).with_opt(4, owner).with(5, count);
    build_packet(s, &f.with_opt(7, grid.map(grid_mini)))
}

/// AGPMITEM op 1 (retrait).
pub fn item_remove(iid: u32, owner: Option<u32>) -> Vec<u8> {
    let s = &schema::ITEM;
    build_packet(s, &Fields::new(s).with(0, item_op::REMOVE).with(2, iid).with_opt(4, owner))
}

/// AGPMITEM op 2 (mise à jour) de la quantité.
pub fn item_count(item: &InventoryItem, owner: u32) -> Vec<u8> {
    let s = &schema::ITEM;
    build_packet(
        s,
        &Fields::new(s)
            .with(0, item_op::UPDATE)
            .with(1, item_status::INVENTORY)
            .with(2, item.iid)
            .with(3, item.tid)
            .with(4, owner)
            .with(5, item.count),
    )
}

#[derive(Clone, Debug)]
pub struct ItemData {
    pub tid: u32,
    pub name: String,
    pub npc_price: i64,
    pub pc_price: i64,
    pub stack: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShopEntry {
    pub tid: u32,
    pub count: i64,
}

/// Catalogue des objets (prix, piles) et listes des boutiques.
#[derive(Default)]
pub struct ItemCatalog {
    pub items: HashMap<u32, ItemData>,
    pub shops: HashMap<i64, Vec<ShopEntry>>,
}

impl ItemCatalog {
    pub async fn load(db: &Databases) -> anyhow::Result<Self> {
        let rows = db.world.rows("SELECT tid, name, npc_price, pc_price, stack FROM item_data", ()).await?;
        let items: HashMap<u32, ItemData> = rows
            .iter()
            .map(|r| {
                let tid: u32 = col(r, "tid");
                (tid, ItemData { tid, name: col(r, "name"), npc_price: col(r, "npc_price"), pc_price: col(r, "pc_price"), stack: col::<i64>(r, "stack").max(1) })
            })
            .collect();
        let lists = db.world.rows("SELECT template_id, tid, count FROM npc_trade_list ORDER BY template_id, idx", ()).await?;
        let mut shops: HashMap<i64, Vec<ShopEntry>> = HashMap::new();
        for r in &lists {
            shops.entry(col(r, "template_id")).or_default().push(ShopEntry { tid: col(r, "tid"), count: col::<i64>(r, "count").max(1) });
        }
        info!("{} objet(s) et {} boutique(s) chargé(s)", items.len(), shops.len());
        Ok(ItemCatalog { items, shops })
    }

    pub fn get(&self, tid: u32) -> Option<&ItemData> {
        self.items.get(&tid)
    }

    pub fn shop(&self, template: i64) -> &[ShopEntry] {
        self.shops.get(&template).map(Vec::as_slice).unwrap_or(&[])
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InventoryItem {
    pub iid: u32,
    pub tid: u32,
    pub count: i64,
    pub grid: Grid,
}

/// Inventaire d'un personnage en jeu, synchronisé avec `character_items`.
#[derive(Clone, Debug, Default)]
pub struct Inventory {
    pub character_id: u32,
    pub items: BTreeMap<u32, InventoryItem>,
}

impl Inventory {
    pub async fn load(db: &Databases, character_id: u32) -> anyhow::Result<Self> {
        let rows = db
            .characters
            .rows(
                "SELECT id, template_id, layer, row_idx, col_idx, count FROM character_items WHERE character_id = ? AND status = ?",
                (character_id, stored_status::INVENTORY),
            )
            .await?;
        let items = rows
            .iter()
            .map(|r| {
                let iid: u32 = col(r, "id");
                (iid, InventoryItem { iid, tid: col(r, "template_id"), count: col(r, "count"), grid: [col(r, "layer"), col(r, "row_idx"), col(r, "col_idx")] })
            })
            .collect();
        Ok(Inventory { character_id, items })
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
        if let Some(iid) = self.stack_for(tid, count, max_stack) {
            let item = self.items.get_mut(&iid).unwrap();
            item.count += count;
            db.characters.exec("UPDATE character_items SET count = ? WHERE id = ?", (item.count, iid)).await?;
            return Ok(Some((item.clone(), false)));
        }
        let Some(grid) = wanted.filter(|g| self.is_free(*g)).or_else(|| self.free_slot()) else { return Ok(None) };
        let iid = db
            .characters
            .exec(
                "INSERT INTO character_items (character_id, template_id, status, layer, row_idx, col_idx, count) VALUES (?, ?, ?, ?, ?, ?, ?)",
                (self.character_id, tid, stored_status::INVENTORY, grid[0], grid[1], grid[2], count),
            )
            .await? as u32;
        let item = InventoryItem { iid, tid, count, grid };
        self.items.insert(iid, item.clone());
        Ok(Some((item, true)))
    }

    pub async fn remove(&mut self, db: &Databases, iid: u32) -> anyhow::Result<()> {
        self.items.remove(&iid);
        db.characters.exec("DELETE FROM character_items WHERE id = ? AND character_id = ?", (iid, self.character_id)).await?;
        Ok(())
    }

    /// Paquets AGPMITEM d'ajout de tout l'inventaire (entrée en jeu).
    pub fn add_packets(&self) -> Vec<Vec<u8>> {
        self.items
            .values()
            .map(|i| item_add(i.iid, i.tid, Some(self.character_id), i.count, item_status::INVENTORY, Some(i.grid)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premiere_case_libre() {
        let mut inv = Inventory { character_id: 1, ..Default::default() };
        assert_eq!(inv.free_slot(), Some([0, 0, 0]));
        inv.items.insert(3, InventoryItem { iid: 3, tid: 190, count: 5, grid: [0, 0, 0] });
        assert_eq!(inv.free_slot(), Some([0, 0, 1]));
        assert_eq!(inv.stack_for(190, 10, 9999), Some(3));
        assert_eq!(inv.stack_for(190, 10, 1), None);
        assert!(!inv.is_free([4, 0, 0]));
    }

    #[test]
    fn iid_de_boutique_hors_de_la_base() {
        let a = alloc_shop_iid();
        assert!(a > SHOP_IID_BASE && a < SHOP_IID_BASE + SHOP_IID_SPAN);
        assert_ne!(a, alloc_shop_iid());
    }
}
