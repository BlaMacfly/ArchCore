// Gestionnaires du serveur monde ArchCore (entrée en jeu minimale).
//
//   AGSMCHARMANAGER (type 3, drapeaux 4 octets) op 2 GAME_ENTER : nom + clé d'auth ; op 13 = keepalive
//       → AGPMCONFIG, AGPMCHARACTER op 25 (effet), AGSMCHARMANAGER op 8 (position de chargement), AGPMTIMER
//   AGSMCHARMANAGER op 9 ENTER_WORLD
//       → paquet personnage complet, vues optimisées, déplacement, AGSMCHARMANAGER op 7, mises à jour

import { sparse, buildMini } from '../net/packet.js';
import { Main, CharMgrOp } from '../net/schema.js';
import { buildCharacterPacket, buildMoveMini, buildFactorsMini, makeFactors } from '../common/character.js';
import { log } from '../common/log.js';
import { NpcRegistry } from './npcs.js';

export class WorldHandlers {
  constructor(ctx) {
    this.db = ctx.db;
    this.templates = ctx.templates;
    this.config = ctx.config;
    this.players = new Map(); // id personnage → session
    this.npcs = ctx.npcs; // NpcRegistry
    this.viewRadius = ctx.config.world.viewRadius ?? 8000;
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

  async #gameEnter(session, packet) {
    const v = packet.values;
    const rawName = v[3]?.text ?? '';
    const name = rawName.split('#')[0].trim();
    const authKey = v[6] ?? 0;
    const accounts = await this.db.auth.query('SELECT id, username FROM account WHERE auth_token = ?', [authKey]);
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
    session.state.factors = makeFactors(tpl, ch);
    this.players.set(ch.id, session);
    session.once('close', () => this.players.delete(ch.id));
    log.info(`[world] #${session.id} ${account.username} entre avec ${ch.name} (cid ${ch.id})`);

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
    const empty = Buffer.alloc(0);
    session.send(Main.OPT_VIEW, sparse(Main.OPT_VIEW, { 0: 2, 6: empty, 7: empty, 8: empty }));
    session.send(Main.OPT_VIEW, sparse(Main.OPT_VIEW, { 0: 2, 6: empty, 7: empty }));
    session.send(Main.OPT_CHAR_MOVE, sparse(Main.OPT_CHAR_MOVE, { 0: ch.id, 1: pos, 2: pos, 5: 12, 6: 0 }));
    // PNJ visibles autour du joueur (vue optimisée op 1, un paquet par PNJ)
    const nearby = this.npcs ? this.npcs.near(pos, this.viewRadius) : [];
    for (const npc of nearby) session.sendRaw(NpcRegistry.viewPacket(npc));
    log.info(`[world] #${session.id} ${nearby.length} PNJ envoyé(s) dans un rayon de ${this.viewRadius}`);
    session.send(Main.CHAR_MANAGER, sparse(Main.CHAR_MANAGER, { 0: CharMgrOp.FINISH_ENTER, 4: ch.id }));
    session.send(Main.CHARACTER, sparse(Main.CHARACTER, { 0: 1, 1: ch.id, 4: 2, 5: buildMoveMini({ pos }) }));
    session.send(Main.CHARACTER, sparse(Main.CHARACTER, { 0: 1, 1: ch.id, 4: 2, 7: buildFactorsMini(factors) }));
    log.info(`[world] #${session.id} ${ch.name} est en jeu`);
  }
}

export { buildMini };
