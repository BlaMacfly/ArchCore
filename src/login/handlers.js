// Gestionnaires du serveur de login ArchCore.
//
// Flux (d'après ALEmu, à confronter au client ArchLord 64 bits) :
//   AGPMSTARTUPENCRYPTION  handshake Blowfish (startup-encryption.js)
//   AGPMWORLD      op 0  → op 2 : liste des mondes "Nom=Groupe=0=Statut=Drapeau;…"
//   AGPMLOGIN      op 0  HASHKEY     → clé de 8 caractères (chiffrement RC4 du compte/mot de passe)
//                  op 1  USER_LOGIN  → op 1 (succès) ou op 15 LOGIN_RESULT (échec)
//                  op 3  UNION_INFO  → op 3, puis op 4 par personnage, puis op 5
//                  op 6  CHAR_LIST   → paquet AGPMCHARACTER par personnage, puis op 7
//                  op 10 CHAR_CREATION_ENTER → personnages de base, puis op 10
//                  op 11 CREATE_CHAR → op 11, paquet personnage, op 12
//                  op 13 DELETE_CHAR → op 13
//                  op 8  WORLD_ENTER → AGSMCHARMANAGER op 11 (clé d'auth) puis op 8 (adresse du monde)

import { randomBytes, randomInt, createHash } from 'node:crypto';
import { buildMini, decodeMini, sparse } from '../net/packet.js';
import { Main, Mini, LoginOp, LoginResult, WorldOp, CharMgrOp } from '../net/schema.js';
import { capiCrypt } from '../common/capi-rc4.js';
import { buildCharacterPacket, makeFactors } from '../common/character.js';
import { log } from '../common/log.js';

const MAX_CHARACTERS = 3;
// Race de la scène de création envoyée au client (1 = Humain ; le client n'accepte que 1 ou 2).
const CREATION_RACE = 1;
const CREATION_CID_BASE = 0x7f000000;
const HASHKEY_ALPHABET = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';

function makeHashKey() {
  const bytes = randomBytes(8);
  return Array.from(bytes, (b) => HASHKEY_ALPHABET[b % HASHKEY_ALPHABET.length]).join('');
}

export const sha256hex = (s) => createHash('sha256').update(s, 'latin1').digest('hex');

export class LoginHandlers {
  /**
   * @param {{db:import('../db/pool.js').Databases, templates:Map<number,object>, baseTemplates:number[], config:object}} ctx
   */
  constructor(ctx) {
    this.db = ctx.db;
    this.templates = ctx.templates;
    this.baseTemplates = ctx.baseTemplates;
    this.config = ctx.config;
    this.worldList = [];
  }

  async refreshWorldList() {
    const rows = await this.db.auth.query(
      'SELECT w.id, w.name, w.address, w.status, w.flag, g.name AS group_name FROM world w JOIN server_group g ON g.id = w.group_id ORDER BY g.id, w.id',
    );
    this.worldList = rows;
    log.info(`[login] ${rows.length} monde(s) : ${rows.map((r) => `${r.name}@${r.address}`).join(', ')}`);
  }

  worldListString() {
    return this.worldList.map((w) => `${w.name}=${w.group_name}=0=${w.status}=${w.flag};`).join('');
  }

  worldAddress(name) {
    return this.worldList.find((w) => w.name === name)?.address ?? null;
  }

  // ------------------------------------------------------------------ AGPMWORLD
  async onWorld(session, packet) {
    const op = packet.values[0];
    if (op === WorldOp.SERVER_LIST_REQ) {
      const list = Buffer.from(`${this.worldListString()}\0`, 'latin1');
      session.send(Main.WORLD, sparse(Main.WORLD, { 0: WorldOp.SERVER_LIST_ANS, 3: list }));
      return;
    }
    if (op === WorldOp.SERVER_EXTRA) return;
    log.warn(`[login] #${session.id} AGPMWORLD op ${op} non géré`);
  }

  // ------------------------------------------------------------------ AGPMLOGIN
  async onLogin(session, packet) {
    const op = packet.values[0];
    const st = session.state;
    switch (op) {
      case LoginOp.HASHKEY: {
        st.hashKey = makeHashKey();
        session.send(Main.LOGIN, sparse(Main.LOGIN, { 0: LoginOp.HASHKEY, 1: st.hashKey }));
        return;
      }
      case LoginOp.USER_LOGIN:
        return this.#userLogin(session, packet);
      case LoginOp.UNION_INFO:
        return this.#unionInfo(session, packet);
      case LoginOp.CHAR_LIST:
        return this.#charList(session);
      case LoginOp.CHAR_CREATION_ENTER:
        return this.#charCreationEnter(session, packet);
      case LoginOp.CREATE_CHAR:
        return this.#createChar(session, packet);
      case LoginOp.DELETE_CHAR:
        return this.#deleteChar(session, packet);
      case LoginOp.WORLD_ENTER:
        return this.#worldEnter(session, packet);
      default:
        log.warn(`[login] #${session.id} AGPMLOGIN op ${op} non géré`);
    }
  }

  #sendLoginResult(session, result) {
    const info = buildMini(Mini.LOGIN_CHAR_INFO, sparse(Mini.LOGIN_CHAR_INFO, {}));
    session.send(Main.LOGIN, sparse(Main.LOGIN, { 0: LoginOp.LOGIN_RESULT, 8: info, 10: result }));
  }

  async #userLogin(session, packet) {
    const st = session.state;
    const v = packet.values;
    if (!st.hashKey) {
      log.warn(`[login] #${session.id} login sans clé de hachage`);
      this.#sendLoginResult(session, LoginResult.GIBBERISH);
      return;
    }
    const acctLen = v[3] ?? 0;
    const pwLen = v[5] ?? 0;
    const acctRaw = v[2]?.raw ?? Buffer.alloc(0);
    const pwRaw = v[4]?.raw ?? Buffer.alloc(0);
    if (acctLen <= 0 || acctLen > 48 || pwLen <= 0 || pwLen > 64) {
      this.#sendLoginResult(session, LoginResult.GIBBERISH);
      return;
    }
    const username = capiCrypt(st.hashKey, acctRaw.subarray(0, acctLen)).toString('latin1');
    const password = capiCrypt(st.hashKey, pwRaw.subarray(0, pwLen)).toString('latin1');
    log.info(`[login] #${session.id} tentative de connexion : ${JSON.stringify(username)}`);

    const rows = await this.db.auth.query('SELECT id, username, password_hash, banned_until FROM account WHERE username = ?', [username]);
    const account = rows[0];
    if (!account || account.password_hash.toLowerCase() !== sha256hex(password)) {
      log.info(`[login] #${session.id} identifiants refusés pour ${JSON.stringify(username)}`);
      this.#sendLoginResult(session, LoginResult.INCORRECT_USER);
      return;
    }
    if (account.banned_until && new Date(account.banned_until) > new Date()) {
      this.#sendLoginResult(session, LoginResult.TOO_MANY_ATTEMPTS);
      return;
    }
    st.account = { id: account.id, username: account.username };
    await this.db.auth.query('UPDATE account SET last_login = NOW(), last_ip = ? WHERE id = ?', [session.socket.remoteAddress, account.id]);
    await this.#loadCharacters(session);
    log.info(`[login] #${session.id} ${account.username} authentifié (${st.characters.length} personnage(s))`);
    session.send(Main.LOGIN, sparse(Main.LOGIN, { 0: LoginOp.USER_LOGIN, 2: acctRaw, 14: 0, 15: 0 }));
  }

  async #loadCharacters(session) {
    const st = session.state;
    st.characters = await this.db.characters.query(
      'SELECT * FROM characters WHERE account_id = ? AND deleted_at IS NULL ORDER BY slot',
      [st.account.id],
    );
  }

  #requireAccount(session) {
    if (!session.state.account) {
      log.warn(`[login] #${session.id} requête avant authentification`);
      session.close();
      return false;
    }
    return true;
  }

  #charInfoMini(map) {
    return buildMini(Mini.LOGIN_CHAR_INFO, sparse(Mini.LOGIN_CHAR_INFO, map));
  }

  async #unionInfo(session, packet) {
    if (!this.#requireAccount(session)) return;
    const st = session.state;
    st.world = packet.values[7]?.text ?? '';
    // Le client (FUN_1405a4fc0) range ce champ comme race courante de l'écran de connexion. Le bouton
    // « Créer » (FUN_1405a5650) n'envoie la demande op 10 que pour la race 1 ou 2 et ouvre la scène
    // de création de cette race : avec 0, aucune demande ne part et le chargement ne finit jamais.
    session.send(Main.LOGIN, sparse(Main.LOGIN, { 0: LoginOp.UNION_INFO, 8: this.#charInfoMini({ 4: CREATION_RACE }) }));
    const count = st.characters.length;
    for (const ch of st.characters) {
      session.send(Main.LOGIN, sparse(Main.LOGIN, { 0: LoginOp.CHAR_NAME, 8: this.#charInfoMini({ 1: ch.name, 2: count, 3: ch.slot }) }));
    }
    session.send(Main.LOGIN, sparse(Main.LOGIN, { 0: LoginOp.CHAR_NAME_FINISH, 2: st.account.username }));
  }

  #characterPacket(ch, extra = {}) {
    const tpl = this.templates.get(ch.template_id);
    if (!tpl) throw new Error(`template ${ch.template_id} inconnu pour ${ch.name}`);
    const factors = makeFactors(tpl, ch);
    return buildCharacterPacket({ ...ch, pos: { x: ch.pos_x, y: ch.pos_y, z: ch.pos_z }, ...extra }, factors);
  }

  async #charList(session) {
    if (!this.#requireAccount(session)) return;
    const st = session.state;
    for (const ch of st.characters) session.sendRaw(this.#characterPacket(ch));
    session.send(Main.LOGIN, sparse(Main.LOGIN, { 0: LoginOp.CHAR_LIST_FINISH, 2: st.account.username, 6: 0 }));
  }

  async #charCreationEnter(session, packet) {
    if (!this.#requireAccount(session)) return;
    // CID réservés aux modèles de l'écran de création : ils ne doivent jamais croiser ceux des vrais
    // personnages (sinon le client met à jour le personnage existant et le fait courir vers la scène).
    let fakeId = CREATION_CID_BASE;
    for (const tid of this.baseTemplates) {
      const tpl = this.templates.get(tid);
      if (!tpl) { log.warn(`[login] template de base ${tid} absent`); continue; }
      const ch = { id: fakeId++, template_id: tid, name: tpl.name, status: 2, pos: { x: 0, y: 0, z: 0 } };
      session.sendRaw(buildCharacterPacket(ch, makeFactors(tpl)));
    }
    const asked = packet?.values?.[8] ? decodeMini(Mini.LOGIN_CHAR_INFO, packet.values[8]).values[5] : undefined;
    log.info(`[login] #${session.id} écran de création (race ${asked ?? '?'}) : ${this.baseTemplates.length} modèle(s) de base`);
    session.send(Main.LOGIN, sparse(Main.LOGIN, { 0: LoginOp.CHAR_CREATION_ENTER, 8: this.#charInfoMini({ 2: 15, 5: asked || CREATION_RACE }) }));
  }

  async #createChar(session, packet) {
    if (!this.#requireAccount(session)) return;
    const st = session.state;
    const info = packet.values[8] ? decodeMini(Mini.LOGIN_CHAR_INFO, packet.values[8]).values : [];
    const tid = info[0] ?? 0;
    const name = (info[1]?.text ?? '').trim();
    const hair = info[6] ?? 0;
    const face = info[7] ?? 0;
    log.info(`[login] #${session.id} création : ${JSON.stringify(name)} tid=${tid} hair=${hair} face=${face}`);
    if (st.characters.length >= MAX_CHARACTERS || !name || name.length > 48 || !this.templates.has(tid)) {
      this.#sendLoginResult(session, LoginResult.NAME_TAKEN);
      return;
    }
    const exists = await this.db.characters.query('SELECT id FROM characters WHERE name = ?', [name]);
    if (exists.length) {
      this.#sendLoginResult(session, LoginResult.NAME_TAKEN);
      return;
    }
    const used = new Set(st.characters.map((c) => c.slot));
    let slot = 0;
    while (used.has(slot)) slot++;
    const tpl = this.templates.get(tid);
    const start = this.config.world.startPositions?.[String(tpl.race)] ?? this.config.world.startPositions?.['1'] ?? { x: 0, y: 0, z: 0 };
    log.info(`[login] #${session.id} ${name} apparaîtra à ${start.village ?? '?'} (race ${tpl.race})`);
    const res = await this.db.characters.query(
      'INSERT INTO characters (account_id, slot, name, template_id, level, hp, mp, sp, pos_x, pos_y, pos_z, hair, face) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)',
      [st.account.id, slot, name, tid, tpl.level ?? 1, tpl.hp ?? 0, tpl.mp ?? 0, tpl.sp ?? 0, start.x, start.y, start.z, hair, face],
    );
    await this.#loadCharacters(session);
    const ch = st.characters.find((c) => c.id === Number(res.insertId));
    session.send(Main.LOGIN, sparse(Main.LOGIN, { 0: LoginOp.CREATE_CHAR, 8: this.#charInfoMini({ 1: name, 2: st.characters.length, 3: slot }) }));
    session.sendRaw(this.#characterPacket(ch, { status: 1, isNew: 1 }));
    session.send(Main.LOGIN, sparse(Main.LOGIN, { 0: LoginOp.CREATE_CHAR_FINISH, 6: ch.id }));
  }

  async #deleteChar(session, packet) {
    if (!this.#requireAccount(session)) return;
    const st = session.state;
    const info = packet.values[8] ? decodeMini(Mini.LOGIN_CHAR_INFO, packet.values[8]).values : [];
    const name = (info[1]?.text ?? '').trim();
    const ch = st.characters.find((c) => c.name === name);
    if (!ch) return;
    await this.db.characters.query('UPDATE characters SET deleted_at = NOW(), name = CONCAT(name, "#", id) WHERE id = ?', [ch.id]);
    await this.#loadCharacters(session);
    log.info(`[login] #${session.id} personnage supprimé : ${name}`);
    session.send(Main.LOGIN, sparse(Main.LOGIN, { 0: LoginOp.DELETE_CHAR, 8: this.#charInfoMini({ 1: name }) }));
  }

  async #worldEnter(session, packet) {
    if (!this.#requireAccount(session)) return;
    const st = session.state;
    const info = packet.values[8] ? decodeMini(Mini.LOGIN_CHAR_INFO, packet.values[8]).values : [];
    const name = (info[1]?.text ?? '').trim();
    const ch = st.characters.find((c) => c.name === name);
    if (!ch) {
      log.warn(`[login] #${session.id} entrée monde pour un personnage inconnu ${JSON.stringify(name)}`);
      return;
    }
    const address = this.worldAddress(st.world) ?? this.worldList[0]?.address;
    if (!address) {
      log.error(`[login] aucune adresse de monde pour ${JSON.stringify(st.world)}`);
      return;
    }
    const authKey = randomInt(1, 2 ** 31 - 1);
    await this.db.auth.query('UPDATE account SET auth_token = ? WHERE id = ?', [authKey, st.account.id]);
    const fullName = `${ch.name}#${st.world}`;
    session.send(Main.CHAR_MANAGER, sparse(Main.CHAR_MANAGER, { 0: CharMgrOp.AUTH_KEY, 3: fullName, 6: authKey }));
    const charInfo = this.#charInfoMini({ 0: ch.template_id, 1: fullName });
    const serverInfo = buildMini(Mini.LOGIN_SERVER_INFO, [address]);
    // Le client (AgcmLogin::OnReceive, cas 8) : CHAR_INFO au champ 8, SERVER_INFO au champ 9
    // (champ 11 dans ALEmu), CID du personnage au champ 6 ; les trois sont transmis à
    // AgcmCharacter pour la connexion au serveur monde.
    session.send(Main.LOGIN, sparse(Main.LOGIN, { 0: LoginOp.WORLD_ENTER, 6: ch.id, 8: charInfo, 9: serverInfo }));
    log.info(`[login] #${session.id} ${st.account.username} → monde ${st.world} (${address}) avec ${ch.name}`);
  }
}
