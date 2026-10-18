// Gestionnaires du serveur monde ArchCore (entrée en jeu, discussion, commandes MJ).
//
//   AGSMCHARMANAGER (type 3, drapeaux 4 octets) op 2 GAME_ENTER : nom + clé d'auth ; op 13 = keepalive
//       → AGPMCONFIG, AGPMCHARACTER op 25 (effet), AGSMCHARMANAGER op 8 (position de chargement), AGPMTIMER
//   AGSMCHARMANAGER op 9 ENTER_WORLD
//       → paquet personnage complet, AGPMADMIN (niveau MJ), vues optimisées, déplacement,
//         AGSMCHARMANAGER op 7, mises à jour
//   AGPMCHATTING (type 0x0F) op 0 : message ; « /… » = commande (gm-commands.js)

import { sparse, buildMini } from '../net/packet.js';
import { Main, CharMgrOp, ChatType } from '../net/schema.js';
import { buildCharacterPacket, buildMoveMini, buildFactorsMini, makeFactors } from '../common/character.js';
import { log } from '../common/log.js';
import { NpcRegistry } from './npcs.js';
import { buildChatPacket, buildAdminLevelPacket, chatText, chatColor } from './chat.js';
import { GmCommands, ServerRates, formatDuration } from './gm-commands.js';
import { Presence } from './presence.js';

// Inventaire : 4 sacs (InventorySmall_Layer0..3 dans le client) de 4 × 4 cases (à confirmer).
const INVENTORY = { layers: 4, rows: 4, columns: 4 };
// AGPMCHARACTER champ 11 (statut d'action) : 2 = mort, d'après le client d'origine (à confirmer en jeu).
const ACTION_STATUS_DEAD = 2;
// Drapeaux de déplacement : 0x04 arrêt + 0x08 synchronisation (comme à l'entrée en jeu).
const MOVE_STOP_SYNC = 0x0c;

export class WorldHandlers {
  constructor(ctx) {
    this.db = ctx.db;
    this.templates = ctx.templates;
    this.config = ctx.config;
    this.players = new Map(); // id personnage → session
    this.npcs = ctx.npcs; // NpcRegistry
    this.viewRadius = ctx.config.world.viewRadius ?? 8000;
    this.chatBans = new Map(); // id personnage → fin de l'interdiction (ms)
    this.restart = null; // { at, timer }
    this.presence = new Presence({ players: this.players, db: this.db, viewRadius: this.viewRadius, onArea: (s, p) => this.#sendNearbyNpcs(s, p) });
    this.gm = new GmCommands({
      world: this,
      rates: new ServerRates(ctx.config.world.rates ?? {}),
      levels: ctx.config.world.gmCommandLevels ?? {},
    });
  }

  async onCharManager(session, packet) {
    const op = packet.values[0];
    const v = packet.values;
    log.debug(`[world] #${session.id} AGSMCHARMANAGER op=${op} f1=${v[1]?.text ?? '-'} f2=${v[2] ?? '-'} f3=${v[3]?.text ?? '-'} f4=${v[4] ?? '-'} f5=${v[5] ? JSON.stringify(v[5]) : '-'} f6=${v[6] ?? '-'} f7=${v[7] ? JSON.stringify(v[7].text) : '-'}`);
    switch (op) {
      case CharMgrOp.GAME_ENTER:
        return this.#gameEnter(session, packet);
      case CharMgrOp.KEEPALIVE:
        return;
      case CharMgrOp.ENTER_WORLD:
        return this.#enterWorld(session);
      default:
        log.warn(`[world] #${session.id} AGSMCHARMANAGER op ${op} non géré`);
    }
  }

  async onCharacter(session, packet) {
    const op = packet.values[0];
    log.debug(`[world] #${session.id} AGPMCHARACTER op ${op} (ignoré)`);
  }

  /** AGPMOPTIMIZEDCHARMOVE (0x40) : le joueur se déplace. */
  onCharMove(session, packet) {
    this.presence.onMove(session, packet);
  }

  async #gameEnter(session, packet) {
    const v = packet.values;
    const rawName = v[3]?.text ?? '';
    const name = rawName.split('#')[0].trim();
    const authKey = v[6] ?? 0;
    const accounts = await this.db.auth.query('SELECT id, username, gm_level FROM account WHERE auth_token = ?', [authKey]);
    const account = accounts[0];
    if (!account) {
      log.warn(`[world] #${session.id} clé d'auth ${authKey} inconnue pour ${JSON.stringify(rawName)}`);
      session.close();
      return;
    }
    const chars = await this.db.characters.query('SELECT * FROM characters WHERE account_id = ? AND name = ? AND deleted_at IS NULL', [account.id, name]);
    const ch = chars[0];
    if (!ch) {
      log.warn(`[world] #${session.id} personnage ${JSON.stringify(name)} absent pour le compte ${account.username}`);
      session.close();
      return;
    }
    const tpl = this.templates.get(ch.template_id);
    if (!tpl) {
      log.error(`[world] template ${ch.template_id} inconnu`);
      session.close();
      return;
    }
    session.state.account = account;
    session.state.character = ch;
    session.state.gmLevel = Number(account.gm_level ?? 0);
    session.state.factors = makeFactors(tpl, ch);
    session.state.seenNpcs = new Set();
    this.players.set(ch.id, session);
    session.once('close', () => { if (this.players.get(ch.id) === session) this.players.delete(ch.id); });
    log.info(`[world] #${session.id} ${account.username} entre avec ${ch.name} (cid ${ch.id}${session.state.gmLevel ? `, MJ ${session.state.gmLevel}` : ''})`);

    const pos = { x: ch.pos_x, y: ch.pos_y, z: ch.pos_z };
    session.send(Main.CONFIG, [0, 0, 0]);
    session.send(Main.CHARACTER, sparse(Main.CHARACTER, { 0: 25, 1: ch.id, 23: 0 }));
    session.send(Main.CHAR_MANAGER, sparse(Main.CHAR_MANAGER, { 0: CharMgrOp.LOADING_POS, 4: ch.id, 5: pos }));
    session.send(Main.TIMER, sparse(Main.TIMER, { 0: 6, 1: BigInt(Date.now()) }));
    await this.db.characters.query('UPDATE characters SET last_played = NOW() WHERE id = ?', [ch.id]);
  }

  async #enterWorld(session) {
    const ch = session.state.character;
    if (!ch) {
      log.warn(`[world] #${session.id} ENTER_WORLD sans personnage`);
      session.close();
      return;
    }
    const pos = { x: ch.pos_x, y: ch.pos_y, z: ch.pos_z };
    const factors = session.state.factors;
    session.sendRaw(buildCharacterPacket({ ...ch, pos }, factors));
    // Niveau MJ : le client l'accroche à un personnage déjà connu (CID), d'où l'envoi après le paquet personnage.
    if (session.state.gmLevel > 0) {
      session.sendRaw(buildAdminLevelPacket({ cid: ch.id, name: ch.name, level: session.state.gmLevel, account: session.state.account.username }));
    }
    const empty = Buffer.alloc(0);
    session.send(Main.OPT_VIEW, sparse(Main.OPT_VIEW, { 0: 2, 6: empty, 7: empty, 8: empty }));
    session.send(Main.OPT_VIEW, sparse(Main.OPT_VIEW, { 0: 2, 6: empty, 7: empty }));
    session.send(Main.OPT_CHAR_MOVE, sparse(Main.OPT_CHAR_MOVE, { 0: ch.id, 1: pos, 2: pos, 5: MOVE_STOP_SYNC, 6: 0 }));
    const sent = this.#sendNearbyNpcs(session, pos);
    log.info(`[world] #${session.id} ${sent} PNJ envoyé(s) dans un rayon de ${this.viewRadius}`);
    session.send(Main.CHAR_MANAGER, sparse(Main.CHAR_MANAGER, { 0: CharMgrOp.FINISH_ENTER, 4: ch.id }));
    this.presence.enter(session);
    session.send(Main.CHARACTER, sparse(Main.CHARACTER, { 0: 1, 1: ch.id, 4: 2, 5: buildMoveMini({ pos }) }));
    session.send(Main.CHARACTER, sparse(Main.CHARACTER, { 0: 1, 1: ch.id, 4: 2, 7: buildFactorsMini(factors) }));
    log.info(`[world] #${session.id} ${ch.name} est en jeu`);
  }

  /** PNJ autour de `pos` pas encore envoyés à cette session (vue optimisée op 1, un paquet par PNJ). */
  #sendNearbyNpcs(session, pos) {
    const nearby = this.npcs ? this.npcs.near(pos, this.viewRadius) : [];
    let sent = 0;
    for (const npc of nearby) {
      if (session.state.seenNpcs.has(npc.cid)) continue;
      session.sendRaw(NpcRegistry.viewPacket(npc));
      session.state.seenNpcs.add(npc.cid);
      sent++;
    }
    return sent;
  }

  // -------------------------------------------------------------------------------------
  // Discussion
  // -------------------------------------------------------------------------------------

  async onChatting(session, packet) {
    const v = packet.values;
    const ch = session.state.character;
    if (!ch || v[0] !== 0) {
      log.debug(`[world] #${session.id} AGPMCHATTING op ${v[0]} ignoré`);
      return;
    }
    const type = v[2] ?? ChatType.NORMAL;
    const text = chatText(v[6]);
    if (!text) return;
    log.info(`[chat] ${ch.name} (type ${type}) : ${text}`);

    if (text.startsWith('/') && await this.gm.execute(session, text, (msg) => this.sendSystem(session, msg))) return;

    const ban = this.chatBans.get(ch.id);
    if (ban && ban > Date.now()) {
      this.sendSystem(session, `Vous ne pouvez pas parler encore ${formatDuration(Math.ceil((ban - Date.now()) / 1000))}.`);
      return;
    }
    const colored = text + (chatColor(v[6]) ?? '');
    if (type === ChatType.WHISPER) {
      const targetName = v[5]?.text ?? '';
      const target = this.findOnline(targetName);
      if (!target) {
        this.sendSystem(session, `${targetName} n'est pas connecté.`);
        return;
      }
      const pkt = buildChatPacket({ type, senderCid: ch.id, senderName: ch.name, targetName: target.state.character.name, text: colored });
      target.sendRaw(pkt);
      if (target !== session) session.sendRaw(pkt);
      return;
    }
    // Discussion normale : tous les joueurs à portée de vue, émetteur compris (le client n'affiche
    // son propre message qu'au retour du serveur).
    const pkt = buildChatPacket({ type: ChatType.NORMAL, senderCid: ch.id, senderName: ch.name, text: colored });
    const r2 = this.viewRadius * this.viewRadius;
    for (const other of this.players.values()) {
      const oc = other.state.character;
      const dx = oc.pos_x - ch.pos_x;
      const dz = oc.pos_z - ch.pos_z;
      if (dx * dx + dz * dz <= r2) other.sendRaw(pkt);
    }
  }

  /** Message système (type 7) à un joueur, une ligne par paquet. */
  sendSystem(session, text) {
    for (const line of String(text).split('\n')) session.sendRaw(buildChatPacket({ type: ChatType.SYSTEM, text: line }));
  }

  /** Annonce (type 10) à tous les joueurs en jeu. */
  broadcastNotice(text) {
    const pkt = buildChatPacket({ type: ChatType.NOTICE, text });
    for (const s of this.players.values()) s.sendRaw(pkt);
    log.info(`[annonce] ${text}`);
  }

  // -------------------------------------------------------------------------------------
  // Actions des commandes MJ (gm-commands.js)
  // -------------------------------------------------------------------------------------

  findOnline(name) {
    const key = String(name ?? '').toLowerCase();
    for (const s of this.players.values()) if (s.state.character.name.toLowerCase() === key) return s;
    return null;
  }

  async #findCharacter(name) {
    const online = this.findOnline(name);
    if (online) return online.state.character;
    const rows = await this.db.characters.query('SELECT id, name FROM characters WHERE name = ? AND deleted_at IS NULL', [name]);
    return rows[0] ?? null;
  }

  /** Interdit la discussion pendant `minutes` (0 = lève l'interdiction). Renvoie le nom exact ou null. */
  async chatBan(name, minutes) {
    const ch = await this.#findCharacter(name);
    if (!ch) return null;
    if (minutes > 0) this.chatBans.set(ch.id, Date.now() + minutes * 60_000);
    else this.chatBans.delete(ch.id);
    const target = this.players.get(ch.id);
    if (target) this.sendSystem(target, minutes > 0 ? `Un MJ vous interdit de parler pendant ${minutes} min.` : 'Vous pouvez de nouveau parler.');
    log.info(`[mj] interdiction de discussion ${ch.name} : ${minutes} min`);
    return ch.name;
  }

  async killCharacter(session) {
    const ch = session.state.character;
    session.state.factors.point.hp = 0;
    session.send(Main.CHARACTER, sparse(Main.CHARACTER, { 0: 1, 1: ch.id, 7: buildFactorsMini(session.state.factors), 11: ACTION_STATUS_DEAD }));
    log.info(`[mj] ${ch.name} tué`);
  }

  /** Renomme un personnage. Renvoie 'missing', 'taken', 'online' ou 'offline'. */
  async renameCharacter(oldName, newName) {
    const ch = await this.#findCharacter(oldName);
    if (!ch) return 'missing';
    const taken = await this.db.characters.query('SELECT id FROM characters WHERE name = ? AND id <> ?', [newName, ch.id]);
    if (taken.length) return 'taken';
    try {
      await this.db.characters.query('UPDATE characters SET name = ? WHERE id = ?', [newName, ch.id]);
    } catch (err) {
      if (err.code === 'ER_DUP_ENTRY') return 'taken';
      throw err;
    }
    log.info(`[mj] ${ch.name} renommé en ${newName}`);
    const online = this.players.get(ch.id);
    if (online) {
      online.state.character.name = newName;
      this.sendSystem(online, `Votre personnage s'appelle désormais ${newName}.`);
      return 'online';
    }
    return 'offline';
  }

  /** Ajoute un objet à l'inventaire (archcore_characters.character_items, première case libre). */
  async createItem(name, tid, count, bound) {
    const ch = await this.#findCharacter(name);
    if (!ch) return { error: 'character' };
    const items = await this.db.world.query('SELECT tid, name FROM item_template WHERE tid = ?', [tid]);
    if (!items.length) return { error: 'item' };
    const used = await this.db.characters.query('SELECT layer, row_idx, col_idx FROM character_items WHERE character_id = ? AND status = 0', [ch.id]);
    const taken = new Set(used.map((r) => `${r.layer}:${r.row_idx}:${r.col_idx}`));
    let slot = null;
    for (let l = 0; l < INVENTORY.layers && !slot; l++) {
      for (let r = 0; r < INVENTORY.rows && !slot; r++) {
        for (let c = 0; c < INVENTORY.columns && !slot; c++) if (!taken.has(`${l}:${r}:${c}`)) slot = [l, r, c];
      }
    }
    if (!slot) return { error: 'full' };
    await this.db.characters.query(
      'INSERT INTO character_items (character_id, template_id, status, layer, row_idx, col_idx, count) VALUES (?, ?, 0, ?, ?, ?, ?)',
      [ch.id, tid, ...slot, count],
    );
    log.info(`[mj] ${count} × objet ${tid} (${items[0].name}) donné à ${ch.name}${bound ? ' (non échangeable : non enregistré)' : ''}`);
    return { itemName: items[0].name, characterName: ch.name };
  }

  /** Annonce un redémarrage dans `total` secondes ; compte à rebours sur les `final` dernières secondes. */
  scheduleRestart(total, final) {
    if (this.restart) clearTimeout(this.restart.timer);
    const at = Date.now() + total * 1000;
    const announce = (left) => this.broadcastNotice(`Redémarrage du serveur dans ${formatDuration(left)}.`);
    announce(total);
    const tick = () => {
      const left = Math.round((at - Date.now()) / 1000);
      if (left <= 0) {
        this.restart = null;
        this.broadcastNotice('Redémarrage du serveur.');
        if (this.config.world.exitOnRestart) setTimeout(() => process.exit(0), 2000).unref();
        return;
      }
      const countdown = left <= final && (left <= 10 || left % 10 === 0);
      if (countdown || (left % 60 === 0 && left !== total)) announce(left);
      this.restart.timer = setTimeout(tick, 1000);
    };
    this.restart = { at, timer: setTimeout(tick, 1000) };
  }

  cancelRestart() {
    if (!this.restart) return false;
    clearTimeout(this.restart.timer);
    this.restart = null;
    this.broadcastNotice('Redémarrage du serveur annulé.');
    return true;
  }

  /** Téléporte un joueur en (x, z). Sans y, garde l'altitude actuelle (recalage sur le terrain à confirmer). */
  async teleport(session, { x, y, z }) {
    const ch = session.state.character;
    const pos = { x, y: y ?? ch.pos_y, z };
    ch.pos_x = pos.x; ch.pos_y = pos.y; ch.pos_z = pos.z;
    session.send(Main.OPT_CHAR_MOVE, sparse(Main.OPT_CHAR_MOVE, { 0: ch.id, 1: pos, 2: pos, 5: MOVE_STOP_SYNC, 6: 0 }));
    session.send(Main.CHARACTER, sparse(Main.CHARACTER, { 0: 1, 1: ch.id, 5: buildMoveMini({ pos, flags: MOVE_STOP_SYNC }) }));
    const sent = this.#sendNearbyNpcs(session, pos);
    session.state.dest = null;
    this.presence.refresh(session);
    await this.db.characters.query('UPDATE characters SET pos_x = ?, pos_y = ?, pos_z = ? WHERE id = ?', [pos.x, pos.y, pos.z, ch.id]);
    log.info(`[mj] ${ch.name} téléporté en ${pos.x}, ${pos.y}, ${pos.z} (${sent} PNJ envoyé(s))`);
    return pos;
  }
}

export { buildMini };
