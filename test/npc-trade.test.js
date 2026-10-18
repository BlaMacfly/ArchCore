import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseEvents, EventFunction, NPC_CID_BASE } from '../src/world/npcs.js';
import { NpcEvents } from '../src/world/npc-events.js';
import { NpcTrade, TradeOp, TradeResult } from '../src/world/npc-trade.js';
import { Inventory, ItemOp, ItemStatus, SHOP_IID_BASE } from '../src/world/items.js';
import { buildMini, buildPacket, decodePacket, decodeMini, sparse } from '../src/net/packet.js';
import { Main, Mini } from '../src/net/schema.js';

const DARRYL_RAW = [
  ['Name', '<Merchant> Darryl'], ['TID', '584'],
  ['Function', '21'], ['EventID', '1073741841'], ['NPCDialogStart', '0'], ['Template', '1'], ['NPCDialogEnd', '0'],
  ['Function', '9'], ['EventID', '1073741842'], ['NPCTradeStart', '0'], ['Template', '104'], ['NPCTradeEnd', '0'],
];

// Base factice : garde les objets insérés et rend un identifiant croissant.
function fakeDb() {
  let nextId = 100;
  const rows = [];
  const query = async (sql, params) => {
    if (sql.startsWith('INSERT INTO character_items')) { rows.push(params); return { insertId: nextId++ }; }
    return [];
  };
  return { characters: { query }, rows };
}

function setup({ money = 1000 } = {}) {
  const npc = { cid: NPC_CID_BASE + 3, name: '<Marchand> Darryl', pos: { x: -479097.9, y: 4753.4, z: -66021.9 }, events: parseEvents(DARRYL_RAW) };
  const registry = { npcs: [npc], get: (cid) => (cid === npc.cid ? npc : null) };
  const items = new Map([
    [190, { tid: 190, name: 'Small Life Potion', npcPrice: 10, pcPrice: 5, stack: 9999 }],
    [500, { tid: 500, name: 'Épée', npcPrice: 300, pcPrice: 150, stack: 1 }],
  ]);
  const catalog = { get: (tid) => items.get(tid) ?? null, shop: (id) => (id === 104 ? [{ tid: 190, count: 1 }, { tid: 500, count: 1 }, { tid: 999, count: 1 }] : []) };
  const db = fakeDb();
  const sent = [];
  const ch = { id: 7, name: 'Testeur', pos_x: -479000, pos_z: -66000, money };
  const session = { id: 1, state: { character: ch, inventory: new Inventory(db, ch.id) }, send: (schema, values) => sent.push({ schema, values: wire(schema, values) }) };
  const trade = new NpcTrade({
    npcEvents: new NpcEvents({ npcs: registry }),
    catalog,
    setMoney: async (s, m) => { s.state.character.money = m; },
  });
  return { npc, trade, session, sent, db };
}

// Aller-retour par l'encodeur, comme sur le réseau.
const wire = (schema, values) => decodePacket(schema, buildPacket(schema, values)).values;
const clientPacket = (map) => ({ values: wire(Main.EVENT_NPC_TRADE, sparse(Main.EVENT_NPC_TRADE, map)) });
const openRequest = (npc) => clientPacket({ 0: buildMini(Mini.EVENT_BASE, sparse(Mini.EVENT_BASE, { 0: 3, 1: npc.cid, 3: EventFunction.NPC_TRADE })), 1: TradeOp.ITEM, 2: 7 });

test('boutique : la liste envoie chaque objet (AGPMITEM statut 10 puis détail) et se termine par op 1', async () => {
  const { npc, trade, session, sent } = setup();
  await trade.onPacket(session, openRequest(npc));
  // TID 999 absent du catalogue : ignoré.
  assert.equal(sent.length, 5);
  const [add, detail] = sent;
  assert.equal(add.schema, Main.ITEM);
  assert.equal(add.values[0], ItemOp.ADD);
  assert.equal(add.values[1], ItemStatus.NPC_TRADE);
  assert.ok(add.values[2] > SHOP_IID_BASE);
  assert.equal(add.values[3], 190);
  assert.equal(add.values[4], npc.cid);
  assert.deepEqual(decodeMini(Mini.ITEM_GRID, add.values[7]).values, [0, 0, 0]);
  assert.equal(detail.schema, Main.EVENT_NPC_TRADE);
  assert.equal(detail.values[1], TradeOp.ITEM);
  assert.equal(detail.values[3], add.values[2]);
  const d = decodeMini(Mini.NPC_TRADE_DETAIL, detail.values[4]).values;
  assert.deepEqual([d[0], d[1], d[2], d[4], d[5], d[6]], [190, 10, 1, 0, 0, 0]);
  assert.equal(decodeMini(Mini.EVENT_BASE, detail.values[0]).values[1], npc.cid);
  assert.deepEqual(decodeMini(Mini.ITEM_GRID, sent[2].values[7]).values, [0, 0, 1]);
  const end = sent[4];
  assert.equal(end.values[1], TradeOp.LIST_END);
  assert.equal(end.values[2], 7);
});

test('boutique : achat d’une pile, l’argent baisse et l’objet entre dans l’inventaire', async () => {
  const { npc, trade, session, sent, db } = setup();
  await trade.onPacket(session, openRequest(npc));
  const potion = [...session.state.shop.items.values()].find((i) => i.tid === 190);
  sent.length = 0;
  const buy = (count) => trade.onPacket(session, clientPacket({ 1: TradeOp.BUY, 2: 7, 3: potion.iid, 4: buildMini(Mini.NPC_TRADE_DETAIL, sparse(Mini.NPC_TRADE_DETAIL, { 2: count })) }));
  await buy(20);
  assert.equal(session.state.character.money, 800);
  assert.equal(sent[0].values[0], ItemOp.ADD);
  assert.equal(sent[0].values[1], ItemStatus.INVENTORY);
  assert.equal(sent[0].values[4], 7);
  assert.equal(sent[0].values[5], 20);
  assert.equal(sent.at(-1).values[6], TradeResult.BUY_SUCCEEDED);
  assert.equal(db.rows.length, 1);
  // Deuxième achat : la pile existante grossit (mise à jour, pas de nouvel objet).
  sent.length = 0;
  await buy(5);
  assert.equal(sent[0].values[0], ItemOp.UPDATE);
  assert.equal(sent[0].values[5], 25);
  assert.equal(db.rows.length, 1);
  assert.equal(session.state.character.money, 750);
});

test('boutique : achat refusé sans argent, vente créditée et objet retiré', async () => {
  const { npc, trade, session, sent } = setup({ money: 100 });
  await trade.onPacket(session, openRequest(npc));
  const sword = [...session.state.shop.items.values()].find((i) => i.tid === 500);
  sent.length = 0;
  await trade.onPacket(session, clientPacket({ 1: TradeOp.BUY, 2: 7, 3: sword.iid }));
  assert.equal(sent.length, 1);
  assert.equal(sent[0].values[6], TradeResult.BUY_NOT_ENOUGH_MONEY);
  assert.equal(session.state.character.money, 100);

  const { item } = await session.state.inventory.add(500, 1, 1);
  sent.length = 0;
  await trade.onPacket(session, clientPacket({ 1: TradeOp.SELL, 2: 7, 3: item.iid }));
  assert.equal(session.state.character.money, 250);
  assert.equal(sent[0].values[0], ItemOp.REMOVE);
  assert.equal(sent[0].values[2], item.iid);
  assert.equal(sent.at(-1).values[1], TradeOp.SELL);
  assert.equal(sent.at(-1).values[6], TradeResult.SELL_SUCCEEDED);
  assert.equal(session.state.inventory.get(item.iid), null);
});

test('boutique : rien n’est envoyé si le joueur est loin de Darryl', async () => {
  const { npc, trade, session, sent } = setup();
  session.state.character.pos_x = -470000;
  await trade.onPacket(session, openRequest(npc));
  assert.equal(sent.length, 0);
});
