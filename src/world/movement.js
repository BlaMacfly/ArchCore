// Déplacement des joueurs (AGPMOPTIMIZEDCHARMOVE, type 0x40).
//
// Le client envoie un paquet 0x40 à chaque ordre de déplacement (clic au sol) et à l'arrêt :
//   0 CID, 1 position courante, 2 destination, 3 cible suivie, 4 distance de suivi,
//   5 drapeaux (0x04 arrêt, 0x08 synchronisation), 6 direction, 7, 8.
// Le serveur :
//   1. vérifie que le CID est celui de la session et que le saut de position reste plausible ;
//   2. mémorise la position et la destination, et enregistre la position en base au plus toutes
//      les SAVE_INTERVAL ms (et à la déconnexion) ;
//   3. fait apparaître les joueurs les uns aux autres quand ils entrent à portée de vue
//      (vue optimisée op 1, comme les PNJ) ;
//   4. rediffuse le paquet de déplacement aux joueurs à portée, CID de l'émetteur inclus.

import { buildMini, buildPacket, sparse } from '../net/packet.js';
import { Main, Mini } from '../net/schema.js';
import { log } from '../common/log.js';

/** Délai minimal entre deux écritures de position en base pour un même personnage. */
export const SAVE_INTERVAL = 2000;
/** Écart toléré entre la position annoncée par le client et la dernière connue (unités du monde). */
export const MAX_JUMP = 30000;

const dist2 = (a, b) => {
  const dx = (a.x ?? 0) - (b.x ?? 0);
  const dz = (a.z ?? 0) - (b.z ?? 0);
  return dx * dx + dz * dz;
};

export class Movement {
  /**
   * @param {{db:any, players:Map<number, any>, viewRadius:number, now?:() => number}} ctx
   */
  constructor(ctx) {
    this.db = ctx.db;
    this.players = ctx.players;
    this.viewRadius = ctx.viewRadius ?? 8000;
    this.now = ctx.now ?? (() => Date.now());
    this.lastSave = new Map(); // id personnage → horodatage de la dernière écriture
    this.pendingSave = new Map(); // id personnage → minuterie d'écriture différée
  }

  /** Paquet d'apparition d'un joueur pour les autres (vue optimisée op 1). */
  static viewPacket(ch, move = {}) {
    const pos = { x: ch.pos_x, y: ch.pos_y, z: ch.pos_z };
    const view = buildMini(Mini.OPT_VIEW_CHAR, sparse(Mini.OPT_VIEW_CHAR, {
      0: ch.id,
      1: ch.template_id,
      2: Buffer.from(String(ch.name), 'latin1'),
      6: move.flags ?? 0,
      7: move.direction ?? 0,
      8: pos,
      9: move.dest ?? pos,
    }));
    return buildPacket(Main.OPT_VIEW, sparse(Main.OPT_VIEW, { 0: 1, 1: view }));
  }

  /** Paquet de déplacement rediffusé (mêmes champs que ceux reçus du client). */
  static movePacket(cid, v) {
    return buildPacket(Main.OPT_CHAR_MOVE, sparse(Main.OPT_CHAR_MOVE, {
      0: cid, 1: v[1], 2: v[2], 3: v[3], 4: v[4], 5: v[5], 6: v[6], 7: v[7], 8: v[8],
    }));
  }

  /** Fait voir `target` à `viewer` s'il est à portée et pas encore connu. Renvoie true si envoyé. */
  reveal(viewer, target) {
    if (viewer === target) return false;
    const vc = viewer.state.character;
    const tc = target.state.character;
    if (!vc || !tc) return false;
    viewer.state.seenPlayers ??= new Set();
    if (viewer.state.seenPlayers.has(tc.id)) return false;
    if (dist2({ x: vc.pos_x, z: vc.pos_z }, { x: tc.pos_x, z: tc.pos_z }) > this.viewRadius * this.viewRadius) return false;
    viewer.sendRaw(Movement.viewPacket(tc, target.state.move));
    viewer.state.seenPlayers.add(tc.id);
    return true;
  }

  /** À l'entrée en jeu : chaque joueur à portée voit le nouveau venu et réciproquement. */
  onEnter(session) {
    let shown = 0;
    for (const other of this.players.values()) {
      if (this.reveal(session, other)) shown++;
      this.reveal(other, session);
    }
    return shown;
  }

  /** Traite un paquet 0x40 reçu d'un client. */
  async onMove(session, packet) {
    const v = packet.values;
    const ch = session.state.character;
    if (!ch) {
      log.warn(`[world] #${session.id} déplacement sans personnage`);
      return;
    }
    const cid = v[0];
    if (cid !== ch.id) {
      log.warn(`[world] #${session.id} déplacement pour le CID ${cid} ≠ ${ch.id} (ignoré)`);
      return;
    }
    const pos = v[1];
    if (!pos) {
      log.debug(`[world] #${session.id} déplacement sans position (drapeaux ${v[5]})`);
      return;
    }
    const known = { x: ch.pos_x, z: ch.pos_z };
    // Une position proche de la dernière destination annoncée (arrivée) est toujours plausible.
    const arrived = session.state.move?.dest && dist2(session.state.move.dest, pos) <= 500 * 500;
    if (!arrived && dist2(known, pos) > MAX_JUMP * MAX_JUMP) {
      log.warn(`[world] #${session.id} ${ch.name} annonce une position trop éloignée (${Math.round(pos.x)}, ${Math.round(pos.z)}) de (${Math.round(known.x)}, ${Math.round(known.z)}) : recalage`);
      session.sendRaw(buildPacket(Main.OPT_CHAR_MOVE, sparse(Main.OPT_CHAR_MOVE, { 0: ch.id, 1: { x: ch.pos_x, y: ch.pos_y, z: ch.pos_z }, 2: { x: ch.pos_x, y: ch.pos_y, z: ch.pos_z }, 5: 0x0c, 6: 0 })));
      return;
    }
    ch.pos_x = pos.x; ch.pos_y = pos.y; ch.pos_z = pos.z;
    session.state.move = { dest: v[2] ?? pos, flags: v[5] ?? 0, direction: v[6] ?? 0, at: this.now() };
    log.debug(`[world] #${session.id} ${ch.name} → (${Math.round(pos.x)}, ${Math.round(pos.z)}) vers (${Math.round(v[2]?.x ?? pos.x)}, ${Math.round(v[2]?.z ?? pos.z)}) drapeaux ${v[5] ?? 0}`);

    this.#schedulePositionSave(ch, (v[5] ?? 0) & 0x04);

    // Apparitions mutuelles puis rediffusion aux joueurs à portée.
    const pkt = Movement.movePacket(ch.id, v);
    const r2 = this.viewRadius * this.viewRadius;
    let relayed = 0;
    for (const other of this.players.values()) {
      if (other === session) continue;
      const oc = other.state.character;
      if (!oc) continue;
      if (dist2({ x: oc.pos_x, z: oc.pos_z }, pos) > r2) continue;
      this.reveal(other, session);
      this.reveal(session, other);
      other.sendRaw(pkt);
      relayed++;
    }
    return relayed;
  }

  #schedulePositionSave(ch, immediate) {
    const last = this.lastSave.get(ch.id) ?? 0;
    const now = this.now();
    const due = immediate ? 0 : Math.max(0, SAVE_INTERVAL - (now - last));
    if (this.pendingSave.has(ch.id)) {
      if (!immediate) return;
      clearTimeout(this.pendingSave.get(ch.id));
      this.pendingSave.delete(ch.id);
    }
    const run = () => {
      this.pendingSave.delete(ch.id);
      this.lastSave.set(ch.id, this.now());
      this.savePosition(ch).catch((err) => log.warn(`[world] sauvegarde de la position de ${ch.name} : ${err.message}`));
    };
    if (due === 0) run();
    else {
      const timer = setTimeout(run, due);
      timer.unref?.();
      this.pendingSave.set(ch.id, timer);
    }
  }

  /** Écrit la position courante en base. */
  async savePosition(ch) {
    await this.db.characters.query('UPDATE characters SET pos_x = ?, pos_y = ?, pos_z = ? WHERE id = ?', [ch.pos_x, ch.pos_y, ch.pos_z, ch.id]);
  }

  /** À la déconnexion : écriture immédiate et annulation des écritures différées. */
  async onLeave(session) {
    const ch = session.state.character;
    if (!ch) return;
    const timer = this.pendingSave.get(ch.id);
    if (timer) {
      clearTimeout(timer);
      this.pendingSave.delete(ch.id);
    }
    this.lastSave.delete(ch.id);
    await this.savePosition(ch);
    for (const other of this.players.values()) other.state.seenPlayers?.delete(ch.id);
  }
}
