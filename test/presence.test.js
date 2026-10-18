import { test } from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { Presence, MoveFlag } from '../src/world/presence.js';
import { buildPacket, decodePacket, parseHeader, sparse } from '../src/net/packet.js';
import { Main, PacketType } from '../src/net/schema.js';
import { makeFactors } from '../src/common/character.js';

/** Session factice : enregistre ce qui est envoyé, sous forme de paquets décodés. */
class FakeSession extends EventEmitter {
  constructor(id, ch) {
    super();
    this.id = id;
    this.sent = [];
    this.state = { character: ch, factors: makeFactors({ race: 1, class: 1 }, ch), seenPlayers: new Set(), inGame: true };
  }
  send(schema, values) { this.sent.push({ type: schema.id, values }); }
  sendRaw(buf) { this.sent.push({ type: parseHeader(buf).type, raw: buf }); }
  take() { const s = this.sent; this.sent = []; return s; }
}

const character = (id, x, z) => ({ id, name: `Perso${id}`, template_id: 96, pos_x: x, pos_y: 0, pos_z: z, level: 1 });
const fakeDb = () => {
  const saved = [];
  return { saved, characters: { query: async (_sql, args) => { saved.push(args); return []; } } };
};

function setup() {
  const players = new Map();
  const db = fakeDb();
  const presence = new Presence({ players, db, viewRadius: 1000 });
  presence.close();
  const join = (id, x, z) => {
    const s = new FakeSession(id, character(id, x, z));
    players.set(id, s);
    return s;
  };
  return { players, db, presence, join };
}

const added = (s) => s.sent.filter((m) => m.type === PacketType.CHARACTER && m.raw).length;
const removed = (s) => s.sent.filter((m) => m.type === PacketType.CHARACTER && m.values?.[0] === 2).map((m) => m.values[1]);
const moves = (s) => s.sent.filter((m) => m.type === PacketType.OPT_CHAR_MOVE).map((m) => m.values);

test('présence : deux joueurs proches se voient mutuellement, un joueur lointain non', () => {
  const { presence, join } = setup();
  const a = join(1, 0, 0);
  const b = join(2, 500, 0);
  const c = join(3, 5000, 0);
  presence.refresh(a);
  presence.refresh(c);
  assert.ok(a.state.seenPlayers.has(2), 'A voit B');
  assert.ok(b.state.seenPlayers.has(1), 'B voit A');
  assert.ok(!a.state.seenPlayers.has(3) && !c.state.seenPlayers.has(1), 'A et C ne se voient pas');
  assert.equal(added(a), 1, 'A reçoit le paquet personnage de B');
  assert.equal(added(b), 1, 'B reçoit le paquet personnage de A');
  assert.equal(added(c), 0);
  // Un second rafraîchissement ne renvoie rien.
  a.take(); b.take();
  presence.refresh(a);
  assert.equal(added(a) + added(b), 0);
});

test('présence : un déplacement est relayé aux joueurs qui voient le personnage, et la visibilité suit', () => {
  const { presence, join, db } = setup();
  const a = join(1, 0, 0);
  const b = join(2, 500, 0);
  for (const s of [a, b]) s.state.seenPlayers = new Set([s === a ? 2 : 1]);

  // A part vers (300, 0, 200) depuis (10, 0, 10).
  const packet = decodePacket(Main.OPT_CHAR_MOVE, buildPacket(Main.OPT_CHAR_MOVE, sparse(Main.OPT_CHAR_MOVE, { 0: 1, 1: { x: 10, y: 0, z: 10 }, 2: { x: 300, y: 0, z: 200 }, 5: 0, 6: 3 })));
  presence.onMove(a, packet);
  const relayed = moves(b);
  assert.equal(relayed.length, 1, 'B reçoit le déplacement de A');
  assert.equal(relayed[0][0], 1);
  assert.deepEqual(relayed[0][2], { x: 300, y: 0, z: 200 });
  assert.equal(relayed[0][5] & MoveFlag.STOP, 0, 'pas de drapeau d’arrêt pendant la marche');
  assert.equal(a.state.character.pos_x, 10);
  assert.equal(moves(a).length, 0, 'A ne reçoit pas son propre déplacement');

  // A s'éloigne hors du rayon et s'arrête : B le retire de sa vue, et réciproquement.
  b.take(); a.take();
  const far = decodePacket(Main.OPT_CHAR_MOVE, buildPacket(Main.OPT_CHAR_MOVE, sparse(Main.OPT_CHAR_MOVE, { 0: 1, 1: { x: 4000, y: 0, z: 0 }, 2: { x: 4000, y: 0, z: 0 }, 5: MoveFlag.STOP })));
  presence.onMove(a, far);
  assert.deepEqual(removed(b), [1], 'B retire A');
  assert.deepEqual(removed(a), [2], 'A retire B');
  assert.equal(a.state.dirty, true);
  return presence.saveAll().then(() => assert.deepEqual(db.saved.at(-1), [4000, 0, 0, 1]));
});

test('présence : un déplacement pour un autre CID est refusé', () => {
  const { presence, join } = setup();
  const a = join(1, 0, 0);
  const b = join(2, 100, 0);
  for (const s of [a, b]) s.state.seenPlayers = new Set([s === a ? 2 : 1]);
  const packet = decodePacket(Main.OPT_CHAR_MOVE, buildPacket(Main.OPT_CHAR_MOVE, sparse(Main.OPT_CHAR_MOVE, { 0: 2, 1: { x: 999, y: 0, z: 999 }, 5: MoveFlag.STOP })));
  presence.onMove(a, packet);
  assert.equal(b.state.character.pos_x, 100);
  assert.equal(moves(b).length, 0);
});

test('présence : au départ, les autres joueurs retirent le personnage et la position est sauvegardée', async () => {
  const { presence, join, db } = setup();
  const a = join(1, 0, 0);
  const b = join(2, 100, 0);
  for (const s of [a, b]) s.state.seenPlayers = new Set([s === a ? 2 : 1]);
  a.state.character.pos_x = 42;
  await presence.leave(a);
  assert.deepEqual(removed(b), [1]);
  assert.deepEqual(db.saved.at(-1), [42, 0, 0, 1]);
});
