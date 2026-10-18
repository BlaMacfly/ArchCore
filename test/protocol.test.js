import { test } from 'node:test';
import assert from 'node:assert/strict';
import { Blowfish } from '../src/net/blowfish.js';
import { buildPacket, decodePacket, buildMini, decodeMini, sparse } from '../src/net/packet.js';
import { Main, Mini, LoginOp } from '../src/net/schema.js';
import { CryptoSession, FrameSplitter } from '../src/net/framing.js';

// Vecteurs de test Blowfish officiels (mots big-endian).
test('Blowfish : vecteurs de test standard', () => {
  const cases = [
    ['0000000000000000', '0000000000000000', '4ef997456198dd78'],
    ['ffffffffffffffff', 'ffffffffffffffff', '51866fd5b85ecb8a'],
    ['0123456789abcdef', '1111111111111111', '61f9c3802281b096'],
    ['fedcba9876543210', '0123456789abcdef', '0aceab0fc6a0a28d'],
  ];
  for (const [key, pt, ct] of cases) {
    const bf = new Blowfish(Buffer.from(key, 'hex'));
    const p = Buffer.from(pt, 'hex');
    const [l, r] = bf.encryptWords(p.readUInt32BE(0), p.readUInt32BE(4));
    const out = Buffer.alloc(8);
    out.writeUInt32BE(l, 0); out.writeUInt32BE(r, 4);
    assert.equal(out.toString('hex'), ct, `chiffrement ${key}`);
    const [dl, dr] = bf.decryptWords(l, r);
    const back = Buffer.alloc(8);
    back.writeUInt32BE(dl, 0); back.writeUInt32BE(dr, 4);
    assert.equal(back.toString('hex'), pt, `déchiffrement ${key}`);
  }
});

test('Blowfish : aller-retour little-endian sur tampon', () => {
  const bf = new Blowfish(Buffer.from('ArchCoreTestKey_0123456789abcdef'));
  const data = Buffer.from('0123456789abcdef0123456789abcdef');
  const enc = Buffer.from(data);
  bf.encryptBufferLE(enc);
  assert.notDeepEqual(enc, data);
  bf.decryptBufferLE(enc);
  assert.deepEqual(enc, data);
});

test('Paquet : en-tête, drapeaux et champs', () => {
  // en-tête relevé sur le client Archonia : d6 | u32 taille | u16 type | u64 owner | u32 tick
  const req = buildPacket(Main.STARTUP_ENCRYPTION, [0, null]);
  assert.equal(req.toString('hex'), 'd616000000' + '48' + '00' + '00000000' + '00000000' + '00000000' + '01' + '00' + '6b');
  const values = sparse(Main.LOGIN, { 0: LoginOp.HASHKEY, 1: 'abcdefgh' });
  const pkt = buildPacket(Main.LOGIN, values, { sessionId: 7, timestamp: 9 });
  assert.equal(pkt[0], 0xd6);
  assert.equal(pkt[pkt.length - 1], 0x6b);
  assert.equal(pkt.readUInt32LE(1), pkt.length);
  assert.equal(pkt.readUInt8(5), Main.LOGIN.id);
  // 19 (en-tête) + 2 (drapeaux) + 1 (op) + 32 (clé) + 1 (pied)
  assert.equal(pkt.length, 19 + 2 + 1 + 32 + 1);
  assert.equal(pkt.readUInt16LE(19), 0b11);
  const dec = decodePacket(Main.LOGIN, pkt);
  assert.equal(dec.sessionId, 7);
  assert.equal(dec.values[0], LoginOp.HASHKEY);
  assert.equal(dec.values[1].text, 'abcdefgh');
  assert.equal(dec.values[2], null);
});

test('Paquet : mini-paquets imbriqués et blocs mémoire', () => {
  const mini = buildMini(Mini.LOGIN_CHAR_INFO, sparse(Mini.LOGIN_CHAR_INFO, { 1: 'Hero', 2: 3, 3: 0 }));
  assert.equal(mini.readUInt32LE(0), mini.length - 4);
  const pkt = buildPacket(Main.LOGIN, sparse(Main.LOGIN, { 0: LoginOp.CHAR_NAME, 8: mini }));
  const dec = decodePacket(Main.LOGIN, pkt);
  assert.ok(Buffer.isBuffer(dec.values[8]));
  const inner = decodeMini(Mini.LOGIN_CHAR_INFO, dec.values[8]);
  assert.equal(inner.values[1].text, 'Hero');
  assert.equal(inner.values[2], 3);
  assert.equal(inner.values[3], 0);

  const world = buildPacket(Main.WORLD, sparse(Main.WORLD, { 0: 2, 3: Buffer.from('A=B=0=4=1;\0', 'latin1') }));
  const wd = decodePacket(Main.WORLD, world);
  assert.equal(wd.values[3].toString('latin1'), 'A=B=0=4=1;\0');
});

test('Framing : chiffrement, découpage et déchiffrement', () => {
  const server = new CryptoSession();
  server.setServerKey('S'.repeat(32));
  server.setClientKey('C'.repeat(32));
  const plain = buildPacket(Main.TIMER, [6, 12345n]);
  const frame = server.encodeFrame(plain);
  assert.equal(frame[0], 0xa1);
  assert.equal(frame[frame.length - 1], 0xaf);
  assert.equal(frame.readUInt32LE(1), frame.length);
  assert.equal(frame.readUInt32LE(5), 1);
  assert.equal(frame.length, 10 + Math.ceil(plain.length / 8) * 8);

  const splitter = new FrameSplitter();
  const clear = buildPacket(Main.STARTUP_ENCRYPTION, [0, null]);
  const stream = Buffer.concat([clear, frame]);
  const parts = [...splitter.push(stream.subarray(0, 5)), ...splitter.push(stream.subarray(5))];
  assert.equal(parts.length, 2);
  assert.equal(parts[0].encrypted, false);
  assert.deepEqual(parts[0].data, clear);
  assert.equal(parts[1].encrypted, true);
  const back = server.decodeFrame(parts[1].data);
  assert.deepEqual(back, plain);
});

test('AGSMCHARMANAGER : drapeaux sur 4 octets, comme le client Archonia', () => {
  // Octets relevés sur le fil (keepalive du client) : drapeaux 05 00 00 00, op 0x0d, champ 2 = 0x0d00?
  const pkt = buildPacket(Main.CHAR_MANAGER, sparse(Main.CHAR_MANAGER, { 0: 2, 2: 17, 3: 'Testeur#ArchCore', 6: 1234, 7: '' }));
  assert.deepEqual([...pkt.subarray(19, 25)], [0xcd, 0, 0, 0, 2, 17]);
  const dec = decodePacket(Main.CHAR_MANAGER, pkt);
  assert.equal(dec.values[0], 2);
  assert.equal(dec.values[2], 17);
  assert.equal(dec.values[3].text, 'Testeur#ArchCore');
  assert.equal(dec.values[6], 1234);
});

test('Vue optimisée : apparition d\'un PNJ (op 1 + mini vue de personnage)', async () => {
  const { NpcRegistry, NPC_CID_BASE } = await import('../src/world/npcs.js');
  const npc = { cid: NPC_CID_BASE + 3, tid: 584, name: '<Merchant> Darryl', pos: { x: -479097.94, y: 4753.4, z: -66021.95 }, degreeX: 0, degreeY: 81 };
  const pkt = NpcRegistry.viewPacket(npc);
  const dec = decodePacket(Main.OPT_VIEW, pkt);
  assert.equal(dec.values[0], 1);
  const view = decodeMini(Mini.OPT_VIEW_CHAR, dec.values[1]);
  assert.equal(view.values[0], NPC_CID_BASE + 3);
  assert.equal(view.values[1], 584);
  assert.equal(view.values[2].toString('latin1'), '<Merchant> Darryl');
  assert.ok(Math.abs(view.values[8].x - npc.pos.x) < 0.1);
  assert.equal(view.values[13], 81);
});
