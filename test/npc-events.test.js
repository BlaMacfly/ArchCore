import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseEvents, EventFunction, NPC_CID_BASE } from '../src/world/npcs.js';
import { NpcEvents, DialogOp } from '../src/world/npc-events.js';
import { buildMini, buildPacket, decodePacket, decodeMini, sparse } from '../src/net/packet.js';
import { Main, Mini } from '../src/net/schema.js';

// Section npc.ini de <Merchant> Darryl (Anchorville), telle qu'importée dans npc_spawn.raw.
const DARRYL_RAW = [
  ['ModuleData1', 'AgpmCharacter'], ['Name', '<Merchant> Darryl'], ['TID', '584'],
  ['ModuleData2', 'ApmEventManager'],
  ['Function', '21'], ['EventID', '1073741841'], ['CondStart', '0'], ['CondEnd', '0'], ['NPCDialogStart', '0'], ['Template', '1'], ['NPCDialogEnd', '0'],
  ['Function', '9'], ['EventID', '1073741842'], ['NPCTradeStart', '0'], ['Template', '104'], ['NPCTradeEnd', '0'],
  ['Function', '27'], ['EventID', '1073741843'], ['QuestGroupID', '1'], ['QuestEnd', '0'],
  ['EnumEnd', '0'],
];

test('PNJ : lecture des fonctions d’événement de npc.ini', () => {
  const ev = parseEvents(JSON.stringify(DARRYL_RAW));
  assert.deepEqual(ev.map((e) => e.function), [EventFunction.NPC_DIALOG, EventFunction.NPC_TRADE, EventFunction.QUEST]);
  assert.equal(ev[0].eventId, 0x40000011);
  assert.equal(ev[1].params.Template, '104');
  assert.equal(ev[2].params.QuestGroupID, '1');
});

function setup() {
  const npc = { cid: NPC_CID_BASE + 3, name: '<Marchand> Darryl', pos: { x: -479097.9, y: 4753.4, z: -66021.9 }, events: parseEvents(DARRYL_RAW) };
  const registry = { npcs: [npc], get: (cid) => (cid === npc.cid ? npc : null) };
  const sent = [];
  const session = { id: 1, state: { character: { id: 1, name: 'Testeur', pos_x: -479000, pos_z: -66000 } }, send: (schema, values) => sent.push({ schema, values }) };
  return { npc, events: new NpcEvents({ npcs: registry }), session, sent };
}

const request = (base) => decodePacket(Main.EVENT_NPC_DIALOG, buildPacket(Main.EVENT_NPC_DIALOG, sparse(Main.EVENT_NPC_DIALOG, { 0: DialogOp.REQUEST, 1: buildMini(Mini.EVENT_BASE, sparse(Mini.EVENT_BASE, base)), 2: 1 })));

test('PNJ : une demande de dialogue près du PNJ est accordée avec le même événement', () => {
  const { npc, events, session, sent } = setup();
  events.onDialog(session, request({ 0: 1, 1: npc.cid, 3: EventFunction.NPC_DIALOG }));
  assert.equal(sent.length, 1);
  assert.equal(sent[0].schema, Main.EVENT_NPC_DIALOG);
  // Aller-retour complet par l'encodeur, comme sur le réseau.
  const wire = decodePacket(Main.EVENT_NPC_DIALOG, buildPacket(Main.EVENT_NPC_DIALOG, sent[0].values));
  assert.equal(wire.values[0], DialogOp.GRANT);
  assert.equal(wire.values[2], 1);
  const base = decodeMini(Mini.EVENT_BASE, wire.values[1]).values;
  assert.equal(base[1], npc.cid);
  assert.equal(base[3], EventFunction.NPC_DIALOG);
});

test('PNJ : demande par identifiant d’événement', () => {
  const { events, session, sent } = setup();
  events.onDialog(session, request({ 2: 0x40000011 }));
  assert.equal(sent.length, 1);
});

test('PNJ : refus si le joueur est trop loin ou si le PNJ est inconnu', () => {
  const { npc, events, session, sent } = setup();
  session.state.character.pos_x = -470000;
  events.onDialog(session, request({ 0: 1, 1: npc.cid, 3: EventFunction.NPC_DIALOG }));
  session.state.character.pos_x = -479000;
  events.onDialog(session, request({ 0: 1, 1: 12345, 3: EventFunction.NPC_DIALOG }));
  assert.equal(sent.length, 0);
});

test('PNJ : la vue contient les fonctions sous forme de paquets ApmEventManager (champ 4)', async () => {
  const { NpcRegistry, BASE_TYPE_CHARACTER } = await import('../src/world/npcs.js');
  const { parseHeader } = await import('../src/net/packet.js');
  const npc = { cid: NPC_CID_BASE + 3, tid: 584, name: '<Marchand> Darryl', pos: { x: 1, y: 2, z: 3 }, degreeX: 0, degreeY: 81, events: parseEvents(DARRYL_RAW) };
  const view = decodePacket(Main.OPT_VIEW, NpcRegistry.viewPacket(npc));
  const block = view.values[4];
  assert.ok(Buffer.isBuffer(block) && block.length > 0, 'champ 4 présent');
  const found = [];
  for (let off = 0; off < block.length;) {
    const h = parseHeader(block.subarray(off));
    const pkt = decodePacket(Main.EVENT_MANAGER, block.subarray(off, off + h.size));
    const base = decodeMini(Mini.EVENT_BASE, pkt.values[0]).values;
    const data = decodeMini(Mini.EVENT_DATA_INT32, pkt.values[1]).values;
    found.push([base[0], base[1], base[3], data[0]]);
    off += h.size;
  }
  assert.deepEqual(found, [
    [BASE_TYPE_CHARACTER, npc.cid, EventFunction.NPC_DIALOG, 1],
    [BASE_TYPE_CHARACTER, npc.cid, EventFunction.NPC_TRADE, 104],
    [BASE_TYPE_CHARACTER, npc.cid, EventFunction.QUEST, 1],
  ]);
});
