// Client de test ArchCore : rejoue le flux complet d'un client ArchLord contre le login puis le monde.
//   node tools/client-sim.mjs [utilisateur] [motdepasse] [nomPerso]
// Sert à valider le serveur sans lancer le jeu. Les échanges suivent le protocole relevé
// dans l'exécutable du client et ALEmu ; le vrai client peut différer dans l'ordre des opérations.

import net from 'node:net';
import { randomBytes } from 'node:crypto';
import { Blowfish } from '../src/net/blowfish.js';
import { FrameSplitter, FRAME_CLIENT, FRAME_TAIL_CLIENT, FRAME_HEADER, FRAME_OVERHEAD } from '../src/net/framing.js';
import { buildPacket, buildMini, decodePacket, decodeMini, sparse, parseHeader, hexDump, GUARD_HEAD, HEADER_SIZE } from '../src/net/packet.js';
import { Main, Mini, MainById, LoginOp, CryptOp, WorldOp, CharMgrOp, ChatType } from '../src/net/schema.js';
import { capiCrypt } from '../src/common/capi-rc4.js';
import { loadConfig } from '../src/common/config.js';

// Arguments suivants : commandes envoyées dans la discussion une fois en jeu (par défaut /GetExp)
const [,, user = 'test', pass = 'test', charName = 'Testeur', ...extra] = process.argv;
const chatCommands = extra.length ? extra : ['/GetExp'];
const cfg = loadConfig();

class TestClient {
  constructor(label) {
    this.label = label;
    this.splitter = new FrameSplitter();
    this.queue = [];
    this.waiters = [];
    this.serverCipher = null;
    this.clientCipher = null;
    this.counter = 0;
    this.encrypted = false;
  }

  connect(host, port) {
    return new Promise((resolve, reject) => {
      this.sock = net.connect({ host, port }, () => { console.log(`[${this.label}] connecté à ${host}:${port}`); resolve(); });
      this.sock.on('error', reject);
      this.sock.on('data', (chunk) => {
        for (const f of this.splitter.push(chunk)) {
          let plain = f.data;
          if (f.encrypted) {
            const body = Buffer.from(plain.subarray(FRAME_HEADER, plain.length - 1));
            this.serverCipher.decryptBufferLE(body);
            plain = body.subarray(0, body.readUInt32LE(1));
          }
          const h = parseHeader(plain);
          const schema = MainById[h.type];
          const pkt = schema ? decodePacket(schema, plain) : { ...h, values: [] };
          console.log(`[${this.label}] ← ${schema?.name ?? 'type 0x' + h.type.toString(16)} op=${pkt.values[0]} (${plain.length} o)`);
          this.queue.push(pkt);
          this.#flush();
        }
      });
      this.sock.on('close', () => console.log(`[${this.label}] fermé`));
    });
  }

  #flush() {
    while (this.queue.length && this.waiters.length) {
      const pkt = this.queue[0];
      const w = this.waiters[0];
      if (w.match(pkt)) { this.queue.shift(); this.waiters.shift(); w.resolve(pkt); } else { this.queue.shift(); }
    }
  }

  expect(type, op = null, timeout = 5000) {
    return this.expectWhere((p) => p.type === type && (op === null || p.values[0] === op), timeout, `type 0x${type.toString(16)} op ${op}`);
  }

  expectWhere(match, timeout = 5000, what = "paquet") {
    return new Promise((resolve, reject) => {
      const waiter = { match, resolve: (p) => { clearTimeout(timer); resolve(p); } };
      const timer = setTimeout(() => {
        this.waiters.splice(this.waiters.indexOf(waiter), 1);
        reject(new Error(`[${this.label}] attente dépassée : ${what}`));
      }, timeout);
      this.waiters.push(waiter);
      this.#flush();
    });
  }

  send(schema, values) {
    const plain = buildPacket(schema, values);
    console.log(`[${this.label}] → ${schema.name} op=${values[0]} (${plain.length} o)`);
    if (!this.encrypted) { this.sock.write(plain); return; }
    const padded = plain.length % 8 ? plain.length + 8 - (plain.length % 8) : plain.length;
    const frame = Buffer.alloc(FRAME_OVERHEAD + padded);
    plain.copy(frame, FRAME_HEADER);
    this.clientCipher.encryptBufferLE(frame.subarray(FRAME_HEADER, FRAME_HEADER + padded));
    frame[0] = FRAME_CLIENT;
    frame.writeUInt32LE(frame.length, 1);
    frame.writeUInt32LE(++this.counter, 5);
    frame[frame.length - 1] = FRAME_TAIL_CLIENT;
    this.sock.write(frame);
  }

  async handshake() {
    this.send(Main.STARTUP_ENCRYPTION, [CryptOp.REQUEST, null]);
    await this.expect(Main.STARTUP_ENCRYPTION.id, CryptOp.SET_ALGO);
    const pub = await this.expect(Main.STARTUP_ENCRYPTION.id, CryptOp.PUBLIC_KEY);
    const serverKey = pub.values[1];
    if (serverKey.length !== 32) throw new Error('clé serveur invalide');
    this.serverCipher = new Blowfish(serverKey);
    const clientKey = randomBytes(32);
    this.clientCipher = new Blowfish(clientKey);
    const enc = Buffer.from(clientKey);
    this.serverCipher.encryptBufferLE(enc);
    this.send(Main.STARTUP_ENCRYPTION, [CryptOp.PRIVATE_KEY, enc]);
    this.encrypted = true;
    await this.expect(Main.STARTUP_ENCRYPTION.id, CryptOp.COMPLETE);
    console.log(`[${this.label}] chiffrement établi`);
  }

  close() { this.sock.end(); }
}

// ------------------------------------------------------------------------------- login
const login = new TestClient('login');
await login.connect('127.0.0.1', cfg.login.port);
await login.handshake();

login.send(Main.WORLD, sparse(Main.WORLD, { 0: WorldOp.SERVER_LIST_REQ }));
const worlds = await login.expect(Main.WORLD.id, WorldOp.SERVER_LIST_ANS);
const worldStr = worlds.values[3].toString('latin1').replace(/\0+$/, '');
console.log(`  mondes : ${worldStr}`);
const worldName = worldStr.split('=')[0];

login.send(Main.LOGIN, sparse(Main.LOGIN, { 0: LoginOp.HASHKEY }));
const hk = await login.expect(Main.LOGIN.id, LoginOp.HASHKEY);
const hashKey = hk.values[1].text;
console.log(`  clé de hachage : ${hashKey}`);

const encUser = capiCrypt(hashKey, Buffer.from(user, 'latin1'));
const encPass = capiCrypt(hashKey, Buffer.from(pass, 'latin1'));
login.send(Main.LOGIN, sparse(Main.LOGIN, { 0: LoginOp.USER_LOGIN, 2: encUser, 3: encUser.length, 4: encPass, 5: encPass.length }));
const loginAns = await login.expect(Main.LOGIN.id);
if (loginAns.values[0] === LoginOp.LOGIN_RESULT) throw new Error(`login refusé, résultat ${loginAns.values[10]}`);
console.log(`  authentifié (op ${loginAns.values[0]})`);

login.send(Main.LOGIN, sparse(Main.LOGIN, { 0: LoginOp.UNION_INFO, 7: worldName }));
await login.expect(Main.LOGIN.id, LoginOp.UNION_INFO);
const names = [];
for (;;) {
  const p = await login.expect(Main.LOGIN.id);
  if (p.values[0] === LoginOp.CHAR_NAME_FINISH) break;
  if (p.values[0] === LoginOp.CHAR_NAME) names.push(decodeMini(Mini.LOGIN_CHAR_INFO, p.values[8]).values[1].text);
}
console.log(`  personnages : ${names.join(', ') || '(aucun)'}`);

login.send(Main.LOGIN, sparse(Main.LOGIN, { 0: LoginOp.CHAR_LIST }));
let chars = 0;
for (;;) {
  const p = await login.expectWhere((p) => p.type === Main.CHARACTER.id || (p.type === Main.LOGIN.id && p.values[0] === LoginOp.CHAR_LIST_FINISH));
  if (p.type === Main.LOGIN.id) break;
  chars++;
  const factors = decodeMini(Mini.CHAR_FACTORS, p.values[7]);
  const status = decodeMini(Mini.FACTOR_STATUS, factors.values[1]);
  console.log(`  personnage cid=${p.values[1]} tid=${p.values[2]} nom=${p.values[3].toString('latin1').replace(/\0+$/, '')} niveau=${status.values[7]}`);
}

if (!names.includes(charName)) {
  login.send(Main.LOGIN, sparse(Main.LOGIN, { 0: LoginOp.CHAR_CREATION_ENTER }));
  let bases = 0;
  for (;;) {
    const p = await login.expectWhere((p) => p.type === Main.CHARACTER.id || (p.type === Main.LOGIN.id && p.values[0] === LoginOp.CHAR_CREATION_ENTER));
    if (p.type === Main.LOGIN.id) break;
    bases++;
  }
  console.log(`  ${bases} personnage(s) de base reçus`);
  const info = buildMini(Mini.LOGIN_CHAR_INFO, sparse(Mini.LOGIN_CHAR_INFO, { 0: 96, 1: charName, 6: 1, 7: 2 }));
  login.send(Main.LOGIN, sparse(Main.LOGIN, { 0: LoginOp.CREATE_CHAR, 8: info }));
  const created = await login.expect(Main.LOGIN.id);
  if (created.values[0] === LoginOp.LOGIN_RESULT) throw new Error(`création refusée : ${created.values[10]}`);
  await login.expect(Main.CHARACTER.id);
  const fin = await login.expect(Main.LOGIN.id, LoginOp.CREATE_CHAR_FINISH);
  console.log(`  personnage créé, cid=${fin.values[6]}`);
}

const enterInfo = buildMini(Mini.LOGIN_CHAR_INFO, sparse(Mini.LOGIN_CHAR_INFO, { 1: charName }));
login.send(Main.LOGIN, sparse(Main.LOGIN, { 0: LoginOp.WORLD_ENTER, 8: enterInfo }));
const auth = await login.expect(Main.CHAR_MANAGER.id, CharMgrOp.AUTH_KEY);
const authKey = auth.values[6];
const enter = await login.expect(Main.LOGIN.id, LoginOp.WORLD_ENTER);
const address = decodeMini(Mini.LOGIN_SERVER_INFO, enter.values[9]).values[0].text;
console.log(`  clé d'auth ${authKey}, monde ${address}`);
login.close();

// ------------------------------------------------------------------------------- monde
const [host, port] = address.split(':');
const world = new TestClient('world');
await world.connect(host, Number(port));
await world.handshake();
// comme le client (FUN_1404b8100) : op 2, champ 2 = 17, nom, clé d'auth, bloc de 248 octets ; puis un keepalive op 13
world.send(Main.CHAR_MANAGER, sparse(Main.CHAR_MANAGER, { 0: CharMgrOp.GAME_ENTER, 2: 17, 3: `${charName}#${worldName}`, 6: authKey, 7: '' }));
world.send(Main.CHAR_MANAGER, sparse(Main.CHAR_MANAGER, { 0: CharMgrOp.KEEPALIVE, 2: 0 }));
await world.expect(Main.CONFIG.id);
await world.expect(Main.CHARACTER.id, 25);
const loading = await world.expect(Main.CHAR_MANAGER.id, CharMgrOp.LOADING_POS);
console.log(`  position de chargement : ${JSON.stringify(loading.values[5])}`);
await world.expect(Main.TIMER.id);
world.send(Main.CHAR_MANAGER, sparse(Main.CHAR_MANAGER, { 0: CharMgrOp.ENTER_WORLD }));
await world.expect(Main.CHARACTER.id, 0);
const npcNames = [];
for (;;) {
  const p = await world.expectWhere((q) => q.type === Main.ADMIN.id || (q.type === Main.OPT_VIEW.id && q.values[0] === 1) || (q.type === Main.CHAR_MANAGER.id && q.values[0] === CharMgrOp.FINISH_ENTER));
  if (p.type === Main.CHAR_MANAGER.id) break;
  if (p.type === Main.ADMIN.id) {
    console.log(`  niveau MJ reçu : ${decodeMini(Mini.ADMIN_CHAR, p.values[2]).values[3]}`);
    continue;
  }
  npcNames.push(decodeMini(Mini.OPT_VIEW_CHAR, p.values[1]).values[2].toString('latin1'));
}
console.log(`  ${npcNames.length} PNJ reçu(s) : ${npcNames.slice(0, 6).join(', ')}${npcNames.length > 6 ? ', …' : ''}`);
console.log('  entrée en jeu confirmée');

// Commande dans la discussion, comme le client (FUN_1402e3180) : op 0, CID, type 0, message sans zéro final
for (const command of chatCommands) {
  world.send(Main.CHATTING, sparse(Main.CHATTING, { 0: 0, 1: loading.values[4], 2: ChatType.NORMAL, 6: Buffer.from(command, 'latin1') }));
  const replies = [await world.expectWhere((q) => q.type === Main.CHATTING.id, 5000, `réponse à ${command}`)];
  // une commande peut produire plusieurs messages (ex. /chatban sur soi-même) : on les recueille tous
  for (;;) {
    const more = await world.expectWhere((q) => q.type === Main.CHATTING.id, 300).catch(() => null);
    if (!more) break;
    replies.push(more);
  }
  console.log(`  ${command}`);
  for (const r of replies) console.log(`    → [type ${r.values[2]}] ${r.values[6].toString('latin1')}`);
}
console.log('  flux complet OK');
world.close();
