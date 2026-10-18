// Boutiques des PNJ (AgcmEventNPCTrade, 0x32).
//
//   client → op 0 (événement au champ 0) : demande la liste. Le serveur envoie pour chaque objet un
//            AGPMITEM op 0 (statut 10, case de la boutique), puis un 0x32 op 0 avec son IID et son détail
//            (prix unitaire, quantité, case) ; un 0x32 op 1 termine la liste et ouvre la fenêtre.
//   client → op 2 : achat de l'objet d'IID donné (quantité au champ 2 du détail, case visée aux champs 7-9)
//            ← op 2, résultat 2 (réussi), 3 (pas assez d'argent), 4 (inventaire plein)…
//   client → op 3 : vente de l'objet d'inventaire d'IID donné ← op 3, résultat 0 (réussi), 1 (trop d'argent)
// Les objets d'une boutique viennent de npctradeitemlist.txt (modèle = paramètre Template de l'événement 9).

import { buildMini, decodeMini, sparse } from '../net/packet.js';
import { Main, Mini } from '../net/schema.js';
import { EventFunction } from './npcs.js';
import { ItemStatus, SHOP_GRID, allocShopIid, itemAddValues, itemRemoveValues, itemCountValues } from './items.js';
import { npcDistance, MAX_NPC_DISTANCE } from './npc-events.js';
import { log } from '../common/log.js';

export const TradeOp = Object.freeze({ ITEM: 0, LIST_END: 1, BUY: 2, SELL: 3 });
export const TradeResult = Object.freeze({
  SELL_SUCCEEDED: 0,
  SELL_EXCEED_MONEY: 1,
  BUY_SUCCEEDED: 2,
  BUY_NOT_ENOUGH_MONEY: 3,
  BUY_FULL_INVENTORY: 4,
  INVALID_ITEM: 5,
  INVALID_COUNT: 6,
});
// Argent maximal dans l'inventaire.
export const MAX_MONEY = 1_000_000_000;

export class NpcTrade {
  /**
   * @param {{npcEvents: import('./npc-events.js').NpcEvents, catalog: import('./items.js').ItemCatalog,
   *          setMoney: (session: any, money: number) => Promise<void>}} deps
   */
  constructor({ npcEvents, catalog, setMoney }) {
    this.npcEvents = npcEvents;
    this.catalog = catalog;
    this.setMoney = setMoney;
  }

  async onPacket(session, packet) {
    const ch = session.state.character;
    if (!ch) return;
    const v = packet.values;
    switch (v[1]) {
      case TradeOp.ITEM: return this.#open(session, v);
      case TradeOp.BUY: return this.#buy(session, v);
      case TradeOp.SELL: return this.#sell(session, v);
      default:
        log.warn(`[boutique] #${session.id} op ${v[1]} inattendu`);
    }
  }

  #open(session, v) {
    const ch = session.state.character;
    const base = v[0] ? decodeMini(Mini.EVENT_BASE, v[0]) : { values: [] };
    const { npc } = this.npcEvents.resolve(base);
    const event = npc?.events.find((e) => e.function === EventFunction.NPC_TRADE);
    if (!npc || !event) {
      log.warn(`[boutique] #${session.id} boutique inconnue (source ${base.values[1] ?? '-'}, événement ${base.values[2] ?? '-'})`);
      return;
    }
    if (npcDistance(npc, ch) > MAX_NPC_DISTANCE) {
      log.warn(`[boutique] #${session.id} ${ch.name} trop loin de ${npc.name}`);
      return;
    }
    const template = Number(event.params.Template);
    const list = this.catalog.shop(template);

    // Une boutique déjà ouverte est retirée du client avant d'en envoyer une nouvelle copie.
    this.close(session);
    const eventBase = buildMini(Mini.EVENT_BASE, base.values);
    const perLayer = SHOP_GRID.rows * SHOP_GRID.columns;
    const items = new Map();
    for (const entry of list) {
      const data = this.catalog.get(entry.tid);
      if (!data) continue;
      const slot = items.size;
      const grid = [Math.floor(slot / perLayer), Math.floor((slot % perLayer) / SHOP_GRID.columns), slot % SHOP_GRID.columns];
      if (grid[0] >= SHOP_GRID.layers) break;
      const item = { iid: allocShopIid(), tid: entry.tid, count: entry.count, price: data.npcPrice, grid };
      items.set(item.iid, item);
      // Propriétaire = le PNJ : le client rattache l’objet à ce personnage (FUN_1401b0e10).
      session.send(Main.ITEM, itemAddValues({ iid: item.iid, tid: item.tid, owner: npc.cid, count: item.count, status: ItemStatus.NPC_TRADE, grid }));
      const detail = buildMini(Mini.NPC_TRADE_DETAIL, sparse(Mini.NPC_TRADE_DETAIL, {
        0: item.tid, 1: item.price, 2: item.count, 3: ItemStatus.NPC_TRADE, 4: grid[0], 5: grid[1], 6: grid[2],
      }));
      session.send(Main.EVENT_NPC_TRADE, sparse(Main.EVENT_NPC_TRADE, { 0: eventBase, 1: TradeOp.ITEM, 3: item.iid, 4: detail }));
    }
    session.send(Main.EVENT_NPC_TRADE, sparse(Main.EVENT_NPC_TRADE, { 1: TradeOp.LIST_END, 2: ch.id }));
    session.state.shop = { npc, template, items };
    log.info(`[boutique] ${ch.name} ouvre la boutique de ${npc.name} (modèle ${template}, ${items.size} objet(s), demande : source ${base.values[0] ?? "-"}/${base.values[1] ?? "-"}, événement ${base.values[2] ?? "-"}, fonction ${base.values[3] ?? "-"})`);
  }

  /** Retire du client les objets de la dernière boutique ouverte. */
  close(session) {
    const shop = session.state.shop;
    if (!shop) return;
    for (const iid of shop.items.keys()) session.send(Main.ITEM, itemRemoveValues(iid, shop.npc.cid));
    session.state.shop = null;
  }

  #result(session, op, result) {
    session.send(Main.EVENT_NPC_TRADE, sparse(Main.EVENT_NPC_TRADE, { 1: op, 2: session.state.character.id, 6: result }));
  }

  /** Boutique ouverte et PNJ encore à portée. */
  #activeShop(session) {
    const shop = session.state.shop;
    if (!shop) return null;
    if (npcDistance(shop.npc, session.state.character) > MAX_NPC_DISTANCE) return null;
    return shop;
  }

  async #buy(session, v) {
    const ch = session.state.character;
    const shop = this.#activeShop(session);
    if (!shop) {
      log.warn(`[boutique] #${session.id} achat sans boutique ouverte à portée`);
      return;
    }
    const entry = shop.items.get(v[3]);
    const data = entry && this.catalog.get(entry.tid);
    if (!entry || !data) return this.#result(session, TradeOp.BUY, TradeResult.INVALID_ITEM);
    const detail = v[4] ? decodeMini(Mini.NPC_TRADE_DETAIL, v[4]).values : [];
    const count = detail[2] > 0 ? detail[2] : entry.count;
    if (!Number.isInteger(count) || count < 1 || count > data.stack) return this.#result(session, TradeOp.BUY, TradeResult.INVALID_COUNT);
    const price = data.npcPrice * count;
    const money = Number(ch.money ?? 0);
    if (price > money) return this.#result(session, TradeOp.BUY, TradeResult.BUY_NOT_ENOUGH_MONEY);
    const wanted = [v[7], v[8], v[9]].every(Number.isInteger) ? [v[7], v[8], v[9]] : null;
    const added = await session.state.inventory.add(entry.tid, count, data.stack, wanted);
    if (!added) return this.#result(session, TradeOp.BUY, TradeResult.BUY_FULL_INVENTORY);
    const { item, created } = added;
    session.send(Main.ITEM, created
      ? itemAddValues({ iid: item.iid, tid: item.tid, owner: ch.id, count: item.count, status: ItemStatus.INVENTORY, grid: item.grid })
      : itemCountValues(item, ch.id));
    await this.setMoney(session, money - price);
    this.#result(session, TradeOp.BUY, TradeResult.BUY_SUCCEEDED);
    log.info(`[boutique] ${ch.name} achète ${count} × ${data.name} (TID ${data.tid}) pour ${price} à ${shop.npc.name}`);
  }

  async #sell(session, v) {
    const ch = session.state.character;
    const shop = this.#activeShop(session);
    if (!shop) {
      log.warn(`[boutique] #${session.id} vente sans boutique ouverte à portée`);
      return;
    }
    const item = session.state.inventory.get(v[3]);
    const data = item && this.catalog.get(item.tid);
    if (!item || !data) return this.#result(session, TradeOp.SELL, TradeResult.INVALID_ITEM);
    const price = data.pcPrice * item.count;
    const money = Number(ch.money ?? 0);
    if (money + price > MAX_MONEY) return this.#result(session, TradeOp.SELL, TradeResult.SELL_EXCEED_MONEY);
    await session.state.inventory.remove(item.iid);
    session.send(Main.ITEM, itemRemoveValues(item.iid, ch.id));
    await this.setMoney(session, money + price);
    this.#result(session, TradeOp.SELL, TradeResult.SELL_SUCCEEDED);
    log.info(`[boutique] ${ch.name} vend ${item.count} × ${data.name} (TID ${data.tid}) pour ${price} à ${shop.npc.name}`);
  }
}
