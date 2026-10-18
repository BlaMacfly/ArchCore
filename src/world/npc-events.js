// Événements des PNJ (ApmEventManager) : dialogue pour l'instant.
//
// Le client demande l'ouverture d'un dialogue par AGPMEVENTNPCDIALOG (0x37) op 0, avec au champ 1 un
// mini EVENT_BASE désignant l'événement, au champ 2 son CID. Pour un PNJ de npc.ini (identifiants
// d'événement à bit 30 levé), le mini contient le type de l'objet source, le CID du PNJ et la fonction.
// Le serveur accorde par l'op 1 en renvoyant les mêmes champs ; le texte du dialogue est dans le client
// (npcdialog.txt).

import { buildMini, decodeMini, sparse } from '../net/packet.js';
import { Main, Mini } from '../net/schema.js';
import { EventFunction } from './npcs.js';
import { log } from '../common/log.js';

export const DialogOp = Object.freeze({ REQUEST: 0, GRANT: 1 });
// Distance maximale (unités du jeu, ~1 cm) entre le joueur et le PNJ pour ouvrir un dialogue.
const MAX_DIALOG_DISTANCE = 1500;

export class NpcEvents {
  constructor({ npcs }) {
    this.npcs = npcs;
  }

  /** Désigne l'événement visé : PNJ trouvé par CID ou par identifiant d'événement. */
  resolve(base) {
    const v = base.values;
    if (v[1]) {
      const npc = this.npcs.get(v[1]);
      if (npc) return { npc, event: npc.events.find((e) => e.function === v[3]) ?? null };
    }
    if (v[2]) {
      for (const npc of this.npcs.npcs) {
        const event = npc.events.find((e) => e.eventId === v[2]);
        if (event) return { npc, event };
      }
    }
    return { npc: null, event: null };
  }

  onDialog(session, packet) {
    const ch = session.state.character;
    if (!ch) return;
    const v = packet.values;
    if (v[0] !== DialogOp.REQUEST) {
      log.warn(`[pnj] #${session.id} AGPMEVENTNPCDIALOG op ${v[0]} inattendu`);
      return;
    }
    const raw = v[1];
    const base = raw ? decodeMini(Mini.EVENT_BASE, raw) : { values: [] };
    const { npc, event } = this.resolve(base);
    const desc = `source ${base.values[0] ?? '-'}/${base.values[1] ?? '-'}, événement ${base.values[2] ?? '-'}, fonction ${base.values[3] ?? '-'}`;
    if (!npc) {
      log.warn(`[pnj] #${session.id} dialogue avec un PNJ inconnu (${desc})`);
      return;
    }
    const dx = npc.pos.x - ch.pos_x;
    const dz = npc.pos.z - ch.pos_z;
    const dist = Math.sqrt(dx * dx + dz * dz);
    if (dist > MAX_DIALOG_DISTANCE) {
      log.warn(`[pnj] #${session.id} ${ch.name} trop loin de ${npc.name} (${dist.toFixed(0)}) pour lui parler`);
      return;
    }
    if (event && event.function !== EventFunction.NPC_DIALOG) {
      log.info(`[pnj] ${ch.name} → ${npc.name} : fonction ${event.function} demandée par le dialogue (${desc})`);
    }
    log.info(`[pnj] ${ch.name} parle à ${npc.name} (${desc})`);
    // Accord : mêmes données que la demande. Le décodeur rend un mini sans son préfixe de longueur u32,
    // il faut donc le reconstruire (renvoyer le tampon brut produirait un paquet invalide pour le client).
    const grantBase = raw
      ? buildMini(Mini.EVENT_BASE, base.values)
      : buildMini(Mini.EVENT_BASE, sparse(Mini.EVENT_BASE, { 0: 3, 1: npc.cid, 3: EventFunction.NPC_DIALOG }));
    session.send(Main.EVENT_NPC_DIALOG, sparse(Main.EVENT_NPC_DIALOG, { 0: DialogOp.GRANT, 1: grantBase, 2: v[2] ?? ch.id }));
  }
}
