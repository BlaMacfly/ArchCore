import { test } from 'node:test';
import assert from 'node:assert/strict';
import { Movement, SAVE_INTERVAL, MAX_JUMP } from '../src/world/movement.js';
import { buildPacket, decodePacket, decodeMini, parseHeader, sparse } from '../src/net/packet.js';
import { Main, Mini, PacketType } from '../src/net/schema.js';

/** Session factice : enregistre les paquets envoyés (décodés par type). */
function fakeSession(id, ch) {
  const sent = [];
  return {
    id,
    state: { character: ch, seenPlayers: new Set() },
    sent,
    sendRaw(buf) {
      const h = parseHeader(buf);
      const schema = h.type === PacketType.OPT_VIEW ? Main.OPT_VIEW : h.type === PacketType.OPT_CHAR_MOVE ? Main.OPT_CHAR_MOVE : null;
      sent.push(schema ? { type: h.type, ...decodePacket(schema, buf) } : { type: h.type });
    },
    send(schema, values) { this.sendRaw(buildPacket(schema, values)); },
    close() {},
  };
}

const ch = (id, name, x, z) => ({ id, name, template_id: 96, pos_x: x, pos_y: 100, pos_z: z });

function movePacket(cid, pos, dest, flags = 0) {
  const plain = buildPacket(Main.OPT_CHAR_MOVE, sparse(Main.OPT_CHAR_MOVE, { 0: cid, 1: pos, 2: dest, 5: flags, 6: 0 }));
  return decodePacket(Main.OPT_CHAR_MOVE, plain);
}

function setup() {
  const saves = [];
  const db = { characters: { query: async (sql, params) => { saves.push(params); return []; } } };
  const players = new Map();
  let clock = 1_000_000;
  const movement = new Movement({ db, players, viewRadius: 8000, now: () => clock });
  const tick = (ms) => { clock += ms; };
  return { db, players, movement, saves, tick };
}

test('déplacement : position mémorisée, apparition et rediffusion aux joueurs à portée', async () => {
  const { players, movement, saves } = setup();
  const a = fakeSession(1, ch(1, 'A', 0, 0));
  const b = fakeSession(2, ch(2, 'B', 3000, 0)); // à portée
  const c = fakeSession(3, ch(3, 'C', 50000, 0)); // trop loin
  for (const s of [a, b, c]) players.set(s.state.character.id, s);

  const relayed = await movement.onMove(a, movePacket(1, { x: 100, y: 100, z: 0 }, { x: 2000, y: 100, z: 0 }));
  assert.equal(relayed, 1);
  assert.equal(a.state.character.pos_x, 100);
  assert.deepEqual(a.state.move.dest, { x: 2000, y: 100, z: 0 });

  // B voit apparaître A (vue op 1 avec le CID et le nom) puis reçoit son déplacement.
  const view = b.sent.find((p) => p.type === PacketType.OPT_VIEW);
  assert.ok(view, 'apparition attendue');
  const mini = decodeMini(Mini.OPT_VIEW_CHAR, view.values[1]);
  assert.equal(mini.values[0], 1);
  assert.equal(mini.values[2].toString('latin1'), 'A');
  const move = b.sent.find((p) => p.type === PacketType.OPT_CHAR_MOVE);
  assert.ok(move);
  assert.equal(move.values[0], 1);
  assert.deepEqual(move.values[2], { x: 2000, y: 100, z: 0 });
  // A voit aussi B ; C ne reçoit rien ; A ne reçoit pas son propre mouvement.
  assert.ok(a.sent.some((p) => p.type === PacketType.OPT_VIEW));
  assert.ok(!a.sent.some((p) => p.type === PacketType.OPT_CHAR_MOVE));
  assert.equal(c.sent.length, 0);

  // Deuxième déplacement : plus d'apparition (déjà vue), une seule rediffusion de plus.
  await movement.onMove(a, movePacket(1, { x: 200, y: 100, z: 0 }, { x: 2000, y: 100, z: 0 }));
  assert.equal(b.sent.filter((p) => p.type === PacketType.OPT_VIEW).length, 1);
  assert.equal(b.sent.filter((p) => p.type === PacketType.OPT_CHAR_MOVE).length, 2);
  // Première écriture immédiate en base, la seconde est différée.
  assert.equal(saves.length, 1);
  assert.deepEqual(saves[0], [100, 100, 0, 1]);
});

test('déplacement : CID étranger ignoré, saut trop grand recalé', async () => {
  const { players, movement, saves } = setup();
  const a = fakeSession(1, ch(1, 'A', 0, 0));
  players.set(1, a);
  await movement.onMove(a, movePacket(99, { x: 10, y: 0, z: 0 }, { x: 10, y: 0, z: 0 }));
  assert.equal(a.state.character.pos_x, 0);
  assert.equal(saves.length, 0);

  await movement.onMove(a, movePacket(1, { x: MAX_JUMP + 1, y: 0, z: 0 }, { x: 0, y: 0, z: 0 }));
  assert.equal(a.state.character.pos_x, 0, 'position inchangée');
  const fix = a.sent.find((p) => p.type === PacketType.OPT_CHAR_MOVE);
  assert.ok(fix, 'paquet de recalage attendu');
  assert.deepEqual(fix.values[1], { x: 0, y: 100, z: 0 });
  assert.equal(fix.values[5], 0x0c);
});

test('déplacement : écriture en base différée puis immédiate à l\'arrêt et à la déconnexion', async () => {
  const { players, movement, saves, tick } = setup();
  const a = fakeSession(1, ch(1, 'A', 0, 0));
  players.set(1, a);
  await movement.onMove(a, movePacket(1, { x: 1, y: 0, z: 0 }, { x: 5, y: 0, z: 0 }));
  assert.equal(saves.length, 1);
  tick(500);
  await movement.onMove(a, movePacket(1, { x: 2, y: 0, z: 0 }, { x: 5, y: 0, z: 0 }));
  assert.equal(saves.length, 1, 'écriture différée');
  // L'arrêt (drapeau 0x04) force l'écriture.
  await movement.onMove(a, movePacket(1, { x: 3, y: 0, z: 0 }, { x: 3, y: 0, z: 0 }, 0x04));
  assert.equal(saves.length, 2);
  assert.equal(saves[1][0], 3);
  tick(SAVE_INTERVAL + 1);
  await movement.onLeave(a);
  assert.equal(saves.length, 3);
});

test('entrée en jeu : les joueurs à portée se voient mutuellement', () => {
  const { players, movement } = setup();
  const a = fakeSession(1, ch(1, 'A', 0, 0));
  const b = fakeSession(2, ch(2, 'B', 1000, 1000));
  players.set(1, a);
  players.set(2, b);
  const shown = movement.onEnter(a);
  assert.equal(shown, 1);
  assert.ok(b.state.seenPlayers.has(1));
  assert.ok(a.state.seenPlayers.has(2));
  assert.equal(movement.onEnter(a), 0, 'pas de doublon');
});
