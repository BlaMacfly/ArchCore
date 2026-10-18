// Discussion (AGPMCHATTING, type 0x0F) et données MJ (AGPMADMIN, type 0x12).
//
// Le client (AgcmUIChatting2, FUN_1404d7dc0) envoie un message sans zéro final. Si le joueur a choisi
// une couleur de discussion, il ajoute « \x01RRGGBB » à la fin des messages normaux et des commandes
// /a…, /g…, /p…, /r…, /s…, /w… : ce suffixe est retiré avant d'interpréter le texte.

import { buildMini, buildPacket, sparse } from '../net/packet.js';
import { Main, Mini, ChatType } from '../net/schema.js';

/** Taille maximale d'un message accepté par le client à la réception (FUN_1402e21e0). */
export const CHAT_MAX_BYTES = 512;

const COLOR_SUFFIX = /\x01[0-9A-Fa-f]{6}$/;

/** Texte d'un message reçu : bloc mémoire → chaîne, sans zéro final ni suffixe de couleur. */
export function chatText(block) {
  if (!block) return '';
  const raw = Buffer.isBuffer(block) ? block.toString('latin1') : String(block);
  return raw.replace(/\0+$/, '').replace(COLOR_SUFFIX, '');
}

/** Couleur de discussion choisie par le joueur (suffixe « \x01RRGGBB »), ou null. */
export function chatColor(block) {
  if (!block) return null;
  const raw = Buffer.isBuffer(block) ? block.toString('latin1') : String(block);
  const m = raw.replace(/\0+$/, '').match(COLOR_SUFFIX);
  return m ? m[0] : null;
}

/** Paquet AGPMCHATTING op 0. `senderCid`/`senderName` sont facultatifs pour les messages système. */
export function buildChatPacket({ type = ChatType.NORMAL, senderCid = null, senderName = null, targetName = null, text }) {
  let data = Buffer.from(String(text ?? ''), 'latin1');
  if (data.length > CHAT_MAX_BYTES) data = data.subarray(0, CHAT_MAX_BYTES);
  return buildPacket(Main.CHATTING, sparse(Main.CHATTING, {
    0: 0,
    1: senderCid,
    2: type,
    4: senderName,
    5: targetName,
    6: data,
  }));
}

/** Paquet AGPMADMIN op 0 qui donne au client le niveau MJ d'un personnage (mini ADMIN_CHAR op 1). */
export function buildAdminLevelPacket({ cid, name, level, account = '' }) {
  const info = buildMini(Mini.ADMIN_CHAR, sparse(Mini.ADMIN_CHAR, { 0: 1, 1: cid, 2: name, 3: level, 5: account }));
  return buildPacket(Main.ADMIN, sparse(Main.ADMIN, { 0: 0, 1: cid, 2: info }));
}
