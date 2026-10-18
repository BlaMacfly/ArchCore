// Présence des joueurs dans le monde : déplacement, visibilité mutuelle, sauvegarde de la position.
//
// Relevé dans le client (FUN_140264010) : le déplacement est un paquet AGPMOPTIMIZEDCHARMOVE (0x40),
// drapeaux d'en-tête sur 1 octet :
//   0 CID, 1 position actuelle, 2 destination, 3 cible suivie, 4 distance de suivi (u16),
//   5 drapeaux de déplacement, 6 direction.
// Drapeaux (champ 5) : 0x04 arrêt (aucune destination), 0x01 direction, 0x02, 0x08, 0x10, 0x20, 0x40.
//
// Visibilité : AGPMCHARACTER op 0 ajoute un personnage chez le client, op 2 le retire
// (AgpmCharacter::OnReceive, FUN_140173f10, cas 0 et 2 ; CID au champ 1).

import { sparse } from '../net/packet.js';
import { Main } from '../net/schema.js';
import { buildCharacterPacket } from '../common/character.js';
import { log } from '../common/log.js';

export const MoveFlag = Object.freeze({ DIRECTION: 0x01, STOP: 0x04, SYNC: 0x08 });
const CHAR_OP_ADD = 0;
const CHAR_OP_REMOVE = 2;
const SAVE_INTERVAL_MS = 60_000;

const posOf = (ch) => ({ x: ch.pos_x, y: ch.pos_y, z: ch.pos_z });
const finite = (p) => p && Number.isFinite(p.x) && Number.isFinite(p.y) && Number.isFinite(p.z);

export class Presence {
  /**
   * @param {object} ctx
   * @param {Map<number, import('../net/session.js').ClientSession>} ctx.players  joueurs en jeu (CID → session)
   * @param {object} ctx.db
   * @param {number} ctx.viewRadius
   * @param {(session: object, pos: object) => number} [ctx.onArea]  appelé quand un joueur change de zone (PNJ proches…)
   */
  constructor({ players, db, viewRadius, onArea }) {
    this.players = players;
    this.db = db;
    this.viewRadius = viewRadius;
    this.onArea = onArea ?? (() => 0);
    this.saveTimer = setInterval(() => this.saveAll().catch((e) => log.warn(`[world] sauvegarde des positions : ${e.message}`)), SAVE_INTERVAL_MS);
    this.saveTimer.unref?.();
  }

  /** Le joueur vient d'entrer en jeu : il voit les joueurs proches, et eux le voient. */
  enter(session) {
    session.state.seenPlayers = new Set();
    session.state.inGame = true;
    session.once('close', () => this.leave(session).catch((e) => log.warn(`[world] départ de #${session.id} : ${e.message}`)));
    this.refresh(session);
  }

  /** Le joueur quitte le monde : on le retire chez les autres et on sauvegarde sa position. */
  async leave(session) {
    const ch = session.state.character;
    if (!ch || !session.state.inGame) return;
    if (session.state.dest) { const d = session.state.dest; ch.pos_x = d.x; ch.pos_y = d.y; ch.pos_z = d.z; }
    log.info(`[world] #${session.id} ${ch.name} quitte le monde en ${ch.pos_x.toFixed(0)}, ${ch.pos_z.toFixed(0)} (${session.state.moveCount ?? 0} déplacement(s))`);
    session.state.inGame = false;
    for (const other of this.players.values()) {
      if (other === session || !other.state.seenPlayers?.has(ch.id)) continue;
      this.#hide(other, ch.id);
    }
    await this.save(session);
  }

  /** Paquet de déplacement reçu du client. */
  onMove(session, packet) {
    const ch = session.state.character;
    if (!ch || !session.state.inGame) return;
    const v = packet.values;
    if (v[0] && v[0] !== ch.id) {
      log.warn(`[world] #${session.id} déplacement pour le CID ${v[0]} refusé (joueur ${ch.id})`);
      return;
    }
    const flags = v[5] ?? 0;
    const dest = finite(v[2]) ? v[2] : null;
    // Le client n'envoie pas toujours sa position actuelle (FUN_140264010 l'omet selon son état) :
    // on prend alors la destination précédente, supposée atteinte, sinon la dernière position connue.
    const pos = finite(v[1]) ? v[1] : (session.state.dest ?? posOf(ch));
    // Le client envoie sa position actuelle ; les autres clients interpolent eux-mêmes vers la destination.
    ch.pos_x = pos.x; ch.pos_y = pos.y; ch.pos_z = pos.z;
    session.state.dest = flags & MoveFlag.STOP ? null : dest;
    session.state.moveFlags = flags;
    session.state.direction = v[6] ?? 0;
    session.state.followTarget = v[3] ?? 0;
    session.state.followDistance = v[4] ?? 0;
    session.state.dirty = true;
    session.state.moveCount = (session.state.moveCount ?? 0) + 1;
    if (session.state.moveCount === 1) log.info(`[world] #${session.id} premier déplacement reçu de ${ch.name} : position ${finite(v[1]) ? 'fournie' : 'absente'}, destination ${dest ? 'fournie' : 'absente'}, drapeaux 0x${flags.toString(16)}`);
    log.debug(`[world] #${session.id} ${ch.name} → ${dest ? `${dest.x.toFixed(0)}, ${dest.z.toFixed(0)}` : 'arrêt'} (drapeaux 0x${flags.toString(16)})`);

    for (const other of this.players.values()) {
      if (other === session || !other.state.seenPlayers?.has(ch.id)) continue;
      other.send(Main.OPT_CHAR_MOVE, this.#movePacket(session));
    }
    this.refresh(session);
    this.onArea(session, dest ?? pos);
  }

  /**
   * Recalcule ce que voit `session` et qui la voit : ajoute les joueurs entrés dans le rayon,
   * retire ceux qui en sont sortis. Symétrique.
   */
  refresh(session) {
    const ch = session.state.character;
    if (!ch || !session.state.inGame) return;
    for (const other of this.players.values()) {
      if (other === session || !other.state.inGame || !other.state.character) continue;
      const oc = other.state.character;
      const near = this.#near(posOf(ch), posOf(oc));
      this.#setVisible(session, other, near);
      this.#setVisible(other, session, near);
    }
  }

  /** Joueurs actuellement visibles par `session` (sessions). */
  visibleTo(session) {
    const ids = session.state.seenPlayers ?? new Set();
    return [...ids].map((id) => this.players.get(id)).filter(Boolean);
  }

  async save(session) {
    const ch = session.state.character;
    if (!ch) return;
    await this.db.characters.query('UPDATE characters SET pos_x = ?, pos_y = ?, pos_z = ? WHERE id = ?', [ch.pos_x, ch.pos_y, ch.pos_z, ch.id]);
    session.state.dirty = false;
  }

  async saveAll() {
    for (const s of this.players.values()) if (s.state.dirty) await this.save(s);
  }

  close() {
    clearInterval(this.saveTimer);
  }

  #near(a, b) {
    const dx = a.x - b.x;
    const dz = a.z - b.z;
    return dx * dx + dz * dz <= this.viewRadius * this.viewRadius;
  }

  /** Rend `target` visible (ou non) pour `viewer`. */
  #setVisible(viewer, target, visible) {
    const seen = viewer.state.seenPlayers;
    const id = target.state.character.id;
    if (!seen) return;
    if (visible && !seen.has(id)) {
      seen.add(id);
      const tc = target.state.character;
      viewer.sendRaw(buildCharacterPacket({ ...tc, pos: posOf(tc) }, target.state.factors, { op: CHAR_OP_ADD }));
      if (target.state.dest) viewer.send(Main.OPT_CHAR_MOVE, this.#movePacket(target));
    } else if (!visible && seen.has(id)) {
      this.#hide(viewer, id);
    }
  }

  #hide(viewer, id) {
    viewer.state.seenPlayers?.delete(id);
    viewer.send(Main.CHARACTER, sparse(Main.CHARACTER, { 0: CHAR_OP_REMOVE, 1: id }));
  }

  #movePacket(session) {
    const ch = session.state.character;
    const pos = posOf(ch);
    const dest = session.state.dest;
    return sparse(Main.OPT_CHAR_MOVE, {
      0: ch.id,
      1: pos,
      2: dest ?? pos,
      3: session.state.followTarget || null,
      4: session.state.followTarget ? session.state.followDistance : null,
      5: dest ? (session.state.moveFlags ?? 0) & ~MoveFlag.STOP : MoveFlag.STOP | MoveFlag.SYNC,
      6: session.state.direction ?? 0,
    });
  }
}
