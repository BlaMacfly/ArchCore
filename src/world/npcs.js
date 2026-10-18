// PNJ placés dans le monde (importés de npc.ini dans archcore_world.npc_spawn).
//
// Apparition côté client : paquet AGPMOPTIMIZEDVIEW (type 0x42) opération 1, dont le champ 1
// est un mini-paquet « vue de personnage » (AGPMOPTIMIZEDPACKET_VIEW, drapeaux 4 octets).
// Relevé dans l'exécutable du client (FUN_140260d40) : le client lit
//   0 CID, 1 TID, 2 nom (bloc mémoire, 48 octets max), 3-4 INT8, 5 mini action,
//   6 drapeaux de déplacement, 7 direction, 8 position, 9 destination (utilisée si non nulle),
//   10 cible suivie, 11 distance de suivi, 12-13 axes X/Y (INT16, convertis en flottants),
//   14 statut spécial, 15-25 divers (valeurs par défaut du client si absents)
// puis crée le personnage via AgpmCharacter (FUN_140175bb0).

import { buildMini, buildPacket, sparse } from '../net/packet.js';
import { Main, Mini } from '../net/schema.js';
import { readFileSync, existsSync } from 'node:fs';
import path from 'node:path';
import { log } from '../common/log.js';
import { ROOT } from '../common/config.js';

/** Traduit « <Titre> Nom » avec data/i18n/npc-titles-<langue>.json (les noms propres restent tels quels). */
export function loadNpcTranslator(language) {
  const file = path.join(ROOT, 'data', 'i18n', `npc-titles-${language}.json`);
  if (!language || language === 'en' || !existsSync(file)) return (name) => name;
  const { titles = {}, names = {}, suffixes = {} } = JSON.parse(readFileSync(file, 'utf8'));
  return (name) => {
    if (names[name]) return names[name];
    const m = name.match(/^<([^>]+)>\s*(.*)$/);
    if (!m) return name;
    return `<${titles[m[1]] ?? m[1]}> ${suffixes[m[2]] ?? m[2]}`;
  };
}

/** Fonctions d'événement (ApmEventManager) des PNJ, numéros relevés dans npc.ini. */
export const EventFunction = Object.freeze({ NPC_TRADE: 9, NPC_DIALOG: 21, QUEST: 27 });

/**
 * Lit les fonctions d'un PNJ dans la section npc.ini brute (liste de paires clé/valeur) :
 * chaque « Function » ouvre un événement, suivi de son « EventID » et de ses paramètres.
 */
export function parseEvents(raw) {
  let pairs = raw;
  if (typeof raw === 'string') { try { pairs = JSON.parse(raw); } catch { return []; } }
  if (!Array.isArray(pairs)) return [];
  const events = [];
  let cur = null;
  for (const [k, v] of pairs) {
    if (k === 'Function') { cur = { function: Number(v), params: {} }; events.push(cur); continue; }
    if (!cur) continue;
    if (k === 'EventID') cur.eventId = Number(v);
    else if (k !== 'EnumEnd') cur.params[k] = v;
  }
  return events;
}

/** Les PNJ reçoivent des CID dans une plage séparée de celle des joueurs. */
export const NPC_CID_BASE = 0x40000000;

export class NpcRegistry {
  constructor(db, language = 'en') {
    this.db = db;
    this.translate = loadNpcTranslator(language);
    this.npcs = [];
  }

  async load() {
    const rows = await this.db.world.query('SELECT id, tid, name, pos_x, pos_y, pos_z, degree_x, degree_y, raw FROM npc_spawn');
    this.npcs = rows.map((r) => ({
      cid: NPC_CID_BASE + Number(r.id),
      id: Number(r.id),
      tid: Number(r.tid),
      name: this.translate(r.name),
      pos: { x: r.pos_x, y: r.pos_y, z: r.pos_z },
      degreeX: r.degree_x,
      degreeY: r.degree_y,
      events: parseEvents(r.raw),
    }));
    this.byCid = new Map(this.npcs.map((n) => [n.cid, n]));
    log.info(`${this.npcs.length} PNJ chargé(s)`);
  }

  /** PNJ par CID. */
  get(cid) {
    return this.byCid?.get(cid) ?? null;
  }

  /** PNJ dont la distance horizontale (X/Z) à `pos` est inférieure à `radius`. */
  near(pos, radius) {
    const r2 = radius * radius;
    return this.npcs.filter((n) => {
      const dx = n.pos.x - pos.x;
      const dz = n.pos.z - pos.z;
      return dx * dx + dz * dz <= r2;
    });
  }

  /** Paquet d'apparition d'un PNJ (vue optimisée, opération 1). */
  static viewPacket(npc) {
    const clamp16 = (v) => Math.max(-32768, Math.min(32767, Math.round(v)));
    const view = buildMini(Mini.OPT_VIEW_CHAR, sparse(Mini.OPT_VIEW_CHAR, {
      0: npc.cid,
      1: npc.tid,
      2: Buffer.from(npc.name, 'latin1'),
      6: 0, // drapeaux de déplacement
      7: 0, // direction
      8: npc.pos,
      9: npc.pos,
      12: clamp16(npc.degreeX),
      13: clamp16(npc.degreeY),
    }));
    return buildPacket(Main.OPT_VIEW, sparse(Main.OPT_VIEW, { 0: 1, 1: view }));
  }
}
