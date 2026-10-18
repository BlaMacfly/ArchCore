import { test } from 'node:test';
import assert from 'node:assert/strict';
import { tokenize, parseCommand, parseArgs, parseCoordinates, GmCommands, GmError, ServerRates, formatDuration } from '../src/world/gm-commands.js';
import { buildChatPacket, buildAdminLevelPacket, chatText, chatColor } from '../src/world/chat.js';
import { buildPacket, decodePacket, decodeMini, sparse } from '../src/net/packet.js';
import { Main, Mini, ChatType } from '../src/net/schema.js';

test('Commandes MJ : découpage des arguments entre guillemets', () => {
  assert.deepEqual(tokenize('"Nom du Joueur" 30'), ['Nom du Joueur', '30']);
  assert.deepEqual(tokenize('  2.0  '), ['2.0']);
  assert.deepEqual(tokenize('"ancien""nouveau"'), ['ancien', 'nouveau']);
  assert.deepEqual(tokenize(''), []);
  assert.throws(() => tokenize('"sans fin'), GmError);
});

test('Commandes MJ : nom de commande et arguments', () => {
  assert.deepEqual(parseCommand('/chatban "Testeur" 30'), { name: 'chatban', args: ['Testeur', '30'] });
  assert.deepEqual(parseCommand('/GetExp'), { name: 'GetExp', args: [] });
  assert.deepEqual(parseCommand('/move -475955,-66219'), { name: 'move', args: ['-475955,-66219'] });
  assert.equal(parseCommand('bonjour'), null);
  assert.equal(parseCommand('/'), null);
});

test('Commandes MJ : conversion des arguments', () => {
  assert.deepEqual(parseArgs(['float'], ['2.5']), [2.5]);
  assert.deepEqual(parseArgs(['float'], ['1,5']), [1.5]);
  assert.deepEqual(parseArgs(['name', 'int'], ['Testeur', '30']), ['Testeur', 30]);
  assert.deepEqual(parseArgs(['name', 'int', 'int?', 'int?'], ['Testeur', '1234', '5']), ['Testeur', 1234, 5, null]);
  assert.throws(() => parseArgs(['int'], ['1.5']), GmError);
  assert.throws(() => parseArgs(['float'], ['abc']), GmError);
  assert.throws(() => parseArgs(['name', 'int'], ['Testeur']), GmError);
  assert.throws(() => parseArgs([], ['x']), GmError);
});

test('Commandes MJ : coordonnées de /move', () => {
  assert.deepEqual(parseCoordinates(['-475955,-66219']), { x: -475955, y: null, z: -66219 });
  assert.deepEqual(parseCoordinates(['100', '200']), { x: 100, y: null, z: 200 });
  assert.deepEqual(parseCoordinates(['1,2,3']), { x: 1, y: 2, z: 3 });
  assert.throws(() => parseCoordinates(['abc']), GmError);
  assert.throws(() => parseCoordinates([]), GmError);
});

test('Commandes MJ : durées', () => {
  assert.equal(formatDuration(5), '5 secondes');
  assert.equal(formatDuration(120), '2 minutes');
  assert.equal(formatDuration(90), '1 min 30 s');
});

/** Monde factice : enregistre les appels des commandes. */
function fakeWorld() {
  const calls = [];
  const online = new Map();
  const world = {
    calls,
    addPlayer(name) { const s = { state: { character: { id: online.size + 1, name } } }; online.set(name.toLowerCase(), s); return s; },
    findOnline: (name) => online.get(String(name).toLowerCase()) ?? null,
    async chatBan(name, minutes) { calls.push(['chatBan', name, minutes]); return online.has(name.toLowerCase()) ? name : null; },
    async killCharacter(s) { calls.push(['kill', s.state.character.name]); },
    async renameCharacter(a, b) { calls.push(['rename', a, b]); return b === 'Pris' ? 'taken' : 'offline'; },
    async createItem(name, tid, count, bound) { calls.push(['create', name, tid, count, bound]); return tid === 1 ? { error: 'item' } : { itemName: 'Épée', characterName: name }; },
    scheduleRestart(total, final) { calls.push(['restart', total, final]); },
    cancelRestart() { calls.push(['cancel']); return true; },
    async teleport(s, pos) { calls.push(['teleport', pos]); return { ...pos, y: 0 }; },
  };
  return world;
}

async function run(gm, level, text) {
  const replies = [];
  const session = { state: { gmLevel: level, character: { id: 99, name: 'MJ' } } };
  const handled = await gm.execute(session, text, (m) => replies.push(m));
  return { handled, replies };
}

test('Commandes MJ : taux SetExp / GetExp en mémoire', async () => {
  const gm = new GmCommands({ world: fakeWorld(), rates: new ServerRates({ exp: 1.5 }) });
  let r = await run(gm, 3, '/GetExp');
  assert.deepEqual(r.replies, ["Taux d'expérience : 1.5"]);
  r = await run(gm, 3, '/SetExp 2');
  assert.deepEqual(r.replies, ["Taux d'expérience réglé à 2.0."]);
  assert.equal(gm.rates.get('exp'), 2);
  r = await run(gm, 3, '/SetDrop 3.5');
  assert.equal(gm.rates.get('drop'), 3.5);
  r = await run(gm, 1, '/GetDrop');
  assert.deepEqual(r.replies, ['Taux de butin : 3.5']);
  r = await run(gm, 3, '/SetGold -1');
  assert.match(r.replies[0], /entre 0 et 100/);
  r = await run(gm, 3, '/SetPrestigeRate x');
  assert.match(r.replies[0], /Syntaxe : \/SetPrestigeRate <taux>/);
});

test('Commandes MJ : niveau requis et joueurs sans droits', async () => {
  const gm = new GmCommands({ world: fakeWorld(), levels: { GetExp: 2 } });
  let r = await run(gm, 0, '/SetExp 2');
  assert.deepEqual(r.replies, ['Commande inconnue : /SetExp']);
  assert.equal(gm.rates.get('exp'), 1);
  r = await run(gm, 1, '/SetExp 2');
  assert.match(r.replies[0], /Niveau MJ 3 requis/);
  r = await run(gm, 1, '/GetExp');
  assert.match(r.replies[0], /Niveau MJ 2 requis/);
  r = await run(gm, 3, '/setexp 2');
  assert.deepEqual(r.replies, ['Commande inconnue : /setexp']);
  r = await run(gm, 3, 'bonjour');
  assert.equal(r.handled, false);
});

test('Commandes MJ : modération, objets, annonce, téléportation', async () => {
  const world = fakeWorld();
  world.addPlayer('Testeur');
  const gm = new GmCommands({ world });
  let r = await run(gm, 3, '/chatban "Testeur" 30');
  assert.deepEqual(world.calls.at(-1), ['chatBan', 'Testeur', 30]);
  assert.match(r.replies[0], /pendant 30 min/);
  r = await run(gm, 3, '/chatban "Inconnu" 5');
  assert.match(r.replies[0], /introuvable/);
  r = await run(gm, 3, '/kill "testeur"');
  assert.deepEqual(world.calls.at(-1), ['kill', 'Testeur']);
  r = await run(gm, 3, '/kill "Absent"');
  assert.match(r.replies[0], /absent du monde/);
  r = await run(gm, 3, '/rename_char "Testeur" "Pris"');
  assert.match(r.replies[0], /déjà pris/);
  r = await run(gm, 3, '/rename_char "Testeur" "a b"');
  assert.match(r.replies[0], /nom invalide/);
  r = await run(gm, 3, '/create "Testeur" 1234 5 1');
  assert.deepEqual(world.calls.at(-1), ['create', 'Testeur', 1234, 5, true]);
  assert.match(r.replies[0], /5 × Épée/);
  r = await run(gm, 3, '/create "Testeur" 1 1 0');
  assert.match(r.replies[0], /objet inconnu/);
  r = await run(gm, 3, '/announce_restart 5 30');
  assert.deepEqual(world.calls.at(-1), ['restart', 330, 30]);
  r = await run(gm, 3, '/announce_restart 0 0');
  assert.deepEqual(world.calls.at(-1), ['cancel']);
  r = await run(gm, 1, '/move 100,200');
  assert.deepEqual(world.calls.at(-1), ['teleport', { x: 100, y: null, z: 200 }]);
  assert.deepEqual(r.replies, ['Téléporté en 100, 200.']);
});

test('Discussion : texte reçu sans suffixe de couleur, paquets envoyés', () => {
  const block = Buffer.from('/SetExp 2.0\x01FF8000', 'latin1');
  assert.equal(chatText(block), '/SetExp 2.0');
  assert.equal(chatColor(block), '\x01FF8000');
  assert.equal(chatText(Buffer.from('bonjour\0', 'latin1')), 'bonjour');

  // Message tel que l'envoie le client : op 0, CID, type 0, message sans zéro final
  const fromClient = buildPacket(Main.CHATTING, sparse(Main.CHATTING, { 0: 0, 1: 7, 2: 0, 6: Buffer.from('/GetExp') }));
  const dec = decodePacket(Main.CHATTING, fromClient);
  assert.equal(dec.flags, 0b1000111);
  assert.equal(chatText(dec.values[6]), '/GetExp');

  const sys = decodePacket(Main.CHATTING, buildChatPacket({ type: ChatType.SYSTEM, text: 'Taux : 1.0' }));
  assert.equal(sys.values[1], null); // pas d'émetteur pour un message système
  assert.equal(sys.values[2], ChatType.SYSTEM);
  assert.equal(sys.values[6].toString('latin1'), 'Taux : 1.0');
});

test('AgpmAdmin : niveau MJ du personnage (drapeaux 2, mini ADMIN_CHAR)', () => {
  const pkt = buildAdminLevelPacket({ cid: 12, name: 'Testeur', level: 3, account: 'test' });
  assert.equal(pkt.readUInt16LE(19), 0b111);
  const dec = decodePacket(Main.ADMIN, pkt);
  assert.equal(dec.values[0], 0);
  assert.equal(dec.values[1], 12);
  const info = decodeMini(Mini.ADMIN_CHAR, dec.values[2]);
  assert.equal(info.values[0], 1);
  assert.equal(info.values[1], 12);
  assert.equal(info.values[2].text, 'Testeur');
  assert.equal(info.values[3], 3);
  assert.equal(info.values[5].text, 'test');
});
