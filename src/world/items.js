// Objets : catalogue (item_data, npc_trade_list), inventaire des personnages et paquets AGPMITEM.
//
// Le client ne connaît un objet qu'après un AGPMITEM op 0 (ajout) qui lui donne son IID, son modèle (TID),
// sa quantité et sa case. Les objets d'inventaire gardent comme IID leur identifiant en base
// (archcore_characters.character_items.id) ; les objets montrés par une boutique reçoivent des IID d'une
// plage à part, qui ne sont jamais enregistrés.

import { buildMini, sparse } from '../net/packet.js';
import { Main, Mini } from '../net/schema.js';
import { log } from '../common/log.js';

export const ItemOp = Object.freeze({ ADD: 0, REMOVE: 1, UPDATE: 2 });

// Statut d'un objet (champ 1 d'AGPMITEM), relevé dans le client : 10 est posé par la boutique (FUN_14073a940).
export const ItemStatus = Object.freeze({ NONE: 0, FIELD: 1, INVENTORY: 2, EQUIP: 3, NPC_TRADE: 10, SUB_INVENTORY: 13, BANK: 100 });

// Statut enregistré en base (colonne character_items.status), indépendant du protocole.
export const StoredStatus = Object.freeze({ INVENTORY: 0, EQUIP: 1, BANK: 2 });

// Inventaire : 4 sacs (InventorySmall_Layer0..3 dans le client) de 4 × 4 cases (à confirmer).
export const INVENTORY = Object.freeze({ layers: 4, rows: 4, columns: 4 });
// Grille d'une boutique : une page de 20 lignes × 4 colonnes (à confirmer dans le client).
export const SHOP_GRID = Object.freeze({ layers: 1, rows: 20, columns: 4 });
export const SHOP_IID_BASE = 0x20000000;
const SHOP_IID_SPAN = 0x10000000;

let shopIidCounter = 0;
/** IID d'un objet de boutique (plage 0x20000000–0x2fffffff, jamais en base). */
export function allocShopIid() {
  shopIidCounter = (shopIidCounter + 1) % SHOP_IID_SPAN;
  return SHOP_IID_BASE + shopIidCounter;
}

/** Valeurs d'un AGPMITEM op 0 (ajout). `grid` = [sac, ligne, colonne] pour l'inventaire ou une boutique. */
export function itemAddValues({ iid, tid, owner = null, count, status, grid = null }) {
  return sparse(Main.ITEM, {
    0: ItemOp.ADD,
    1: status,
    2: iid,
    3: tid,
    4: owner,
    5: count,
    7: grid ? buildMini(Mini.ITEM_GRID, grid) : null,
  });
}

/** Valeurs d'un AGPMITEM op 1 (retrait). */
export function itemRemoveValues(iid, owner = null) {
  return sparse(Main.ITEM, { 0: ItemOp.REMOVE, 2: iid, 4: owner });
}

/** Valeurs d'un AGPMITEM op 2 (mise à jour) de la quantité. */
export function itemCountValues(item, owner) {
  return sparse(Main.ITEM, { 0: ItemOp.UPDATE, 1: ItemStatus.INVENTORY, 2: item.iid, 3: item.tid, 4: owner, 5: item.count });
}

/** Catalogue des objets (prix, piles) et listes des boutiques, chargés depuis archcore_world. */
export class ItemCatalog {
  constructor(db) {
    this.db = db;
    this.items = new Map(); // TID → { tid, name, npcPrice, pcPrice, stack }
    this.shops = new Map(); // modèle de boutique → [{ tid, count }]
  }

  async load() {
    const rows = await this.db.world.query('SELECT tid, name, npc_price, pc_price, stack FROM item_data');
    for (const r of rows) {
      this.items.set(Number(r.tid), { tid: Number(r.tid), name: r.name, npcPrice: Number(r.npc_price), pcPrice: Number(r.pc_price), stack: Math.max(1, Number(r.stack)) });
    }
    const lists = await this.db.world.query('SELECT template_id, tid, count FROM npc_trade_list ORDER BY template_id, idx');
    for (const r of lists) {
      const id = Number(r.template_id);
      if (!this.shops.has(id)) this.shops.set(id, []);
      this.shops.get(id).push({ tid: Number(r.tid), count: Math.max(1, Number(r.count)) });
    }
    log.info(`${this.items.size} objet(s) et ${this.shops.size} boutique(s) chargé(s)`);
  }

  get(tid) {
    return this.items.get(Number(tid)) ?? null;
  }

  shop(templateId) {
    return this.shops.get(Number(templateId)) ?? [];
  }
}

/** Inventaire d'un personnage en jeu, synchronisé avec character_items. */
export class Inventory {
  constructor(db, characterId, items = []) {
    this.db = db;
    this.characterId = characterId;
    this.items = new Map(items.map((i) => [i.iid, i])); // IID → { iid, tid, count, grid: [sac, ligne, colonne] }
  }

  static async load(db, characterId) {
    const rows = await db.characters.query(
      'SELECT id, template_id, layer, row_idx, col_idx, count FROM character_items WHERE character_id = ? AND status = ?',
      [characterId, StoredStatus.INVENTORY],
    );
    return new Inventory(db, characterId, rows.map((r) => ({ iid: Number(r.id), tid: Number(r.template_id), count: Number(r.count), grid: [r.layer, r.row_idx, r.col_idx] })));
  }

  get(iid) {
    return this.items.get(iid) ?? null;
  }

  isFree([l, r, c]) {
    if (l < 0 || l >= INVENTORY.layers || r < 0 || r >= INVENTORY.rows || c < 0 || c >= INVENTORY.columns) return false;
    for (const item of this.items.values()) if (item.grid[0] === l && item.grid[1] === r && item.grid[2] === c) return false;
    return true;
  }

  freeSlot() {
    for (let l = 0; l < INVENTORY.layers; l++) {
      for (let r = 0; r < INVENTORY.rows; r++) {
        for (let c = 0; c < INVENTORY.columns; c++) if (this.isFree([l, r, c])) return [l, r, c];
      }
    }
    return null;
  }

  /** Pile existante du même objet pouvant recevoir `count` de plus. */
  stackFor(tid, count, maxStack) {
    if (maxStack <= 1) return null;
    for (const item of this.items.values()) if (item.tid === tid && item.count + count <= maxStack) return item;
    return null;
  }

  /**
   * Ajoute `count` objets : sur une pile existante si possible, sinon dans la case voulue (si libre) ou la
   * première case libre. Renvoie { item, created } ou null si l'inventaire est plein.
   */
  async add(tid, count, maxStack, wanted = null) {
    const stack = this.stackFor(tid, count, maxStack);
    if (stack) {
      stack.count += count;
      await this.db.characters.query('UPDATE character_items SET count = ? WHERE id = ?', [stack.count, stack.iid]);
      return { item: stack, created: false };
    }
    const grid = wanted && this.isFree(wanted) ? wanted : this.freeSlot();
    if (!grid) return null;
    const res = await this.db.characters.query(
      'INSERT INTO character_items (character_id, template_id, status, layer, row_idx, col_idx, count) VALUES (?, ?, ?, ?, ?, ?, ?)',
      [this.characterId, tid, StoredStatus.INVENTORY, ...grid, count],
    );
    const item = { iid: Number(res.insertId), tid, count, grid };
    this.items.set(item.iid, item);
    return { item, created: true };
  }

  async remove(iid) {
    this.items.delete(iid);
    await this.db.characters.query('DELETE FROM character_items WHERE id = ? AND character_id = ?', [iid, this.characterId]);
  }

  /** Paquets d'ajout de tout l'inventaire (entrée en jeu). */
  addPackets() {
    return [...this.items.values()].map((i) => itemAddValues({ iid: i.iid, tid: i.tid, owner: this.characterId, count: i.count, status: ItemStatus.INVENTORY, grid: i.grid }));
  }
}
