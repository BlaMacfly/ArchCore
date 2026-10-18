// Schémas des paquets ArchLord connus d'ArchCore.
//
// Définitions relevées dans Archonia.exe (Ghidra via REA) : chaque module Agpm*/Agcm* appelle
// AuPacket::SetFieldType (FUN_14014e850) dans son constructeur avec la liste (type, taille)…
// Le relevé complet de tous les modules est dans data/re/packet-schemas.json
// (tools/re/extract-schemas.mjs). `verified: true` = confronté à ce relevé.
//
// Un champ = [type, taille]. Pour CHAR la taille est le nombre d'octets ; pour les
// scalaires c'est un nombre de répétitions (toujours 1 en pratique) ; pour PACKET > 1
// signifie « liste de mini-paquets précédée d'un u8 compteur ».

import { T } from './packet.js';

const S = (name, id, flagLen, fields, extra = {}) =>
  Object.freeze({ name, id, flagLen, fields: fields.map((f) => Object.freeze(f)), verified: false, ...extra });
const V = { verified: true };
const ints = (n) => Array.from({ length: n }, () => [T.INT32, 1]);

// ---------------------------------------------------------------------------------------
// Types de paquets principaux (enregistrés par chaque module dans le client)
// ---------------------------------------------------------------------------------------
export const PacketType = Object.freeze({
  SYSTEM: 0x00,
  CONFIG: 0x01,
  CHARACTER: 0x02,
  CHAR_MANAGER: 0x03,
  ITEM: 0x04,
  SKILL: 0x05,
  PARTY: 0x08,
  LOGIN: 0x0d,
  CHATTING: 0x0f,
  TIMER: 0x11,
  ADMIN: 0x12,
  QUEST: 0x13,
  UI_STATUS: 0x14,
  GUILD: 0x15,
  WORLD: 0x18,
  RETURN_TO_LOGIN: 0x2f,
  EVENT_MANAGER: 0x3c,
  OPT_CHAR_MOVE: 0x40,
  OPT_CHAR_ACTION: 0x41,
  OPT_VIEW: 0x42,
  SYSTEM_MESSAGE: 0x44,
  STARTUP_ENCRYPTION: 0x48,
  TITLE: 0x60,
  RECONNECT: 0x63,
});

export const Main = Object.freeze({
  // AgpmConfig — drapeaux 4
  CONFIG: S('AGPMCONFIG', PacketType.CONFIG, 4, [
    [T.INT32, 1], // 0 drop d'objet à la mort (PC)
    [T.INT32, 1], // 1 pénalité d'XP à la mort
    [T.INT8, 1], // 2 serveur de test ?
  ], V),

  // AgpmCharacter — drapeaux 4
  CHARACTER: S('AGPMCHARACTER', PacketType.CHARACTER, 4, [
    [T.INT8, 1], // 0 opération
    [T.INT32, 1], // 1 CID
    [T.INT32, 1], // 2 TID (template)
    [T.MEMORY_BLOCK, 1], // 3 nom (u16 longueur + octets)
    [T.INT8, 1], // 4 statut
    [T.PACKET, 1], // 5 mini déplacement (AGPMCHAR_MOVE)
    [T.PACKET, 1], // 6 mini action
    [T.PACKET, 1], // 7 mini AGPMCHAR_FACTORS
    [T.INT64, 1], // 8 argent inventaire
    [T.INT64, 1], // 9 argent banque
    [T.INT64, 1], // 10 cash
    [T.INT8, 1], // 11 statut d'action
    [T.INT8, 1], // 12 statut criminel
    [T.INT32, 1], // 13 attaquant
    [T.INT8, 1], // 14 nouveau perso
    [T.UINT8, 1], // 15 dernier point de liaison
    [T.UINT8, 1], // 16 animation sociale
    [T.UINT64, 1], // 17 statut spécial
    [T.INT8, 1], // 18 transformation
    [T.CHAR, 32], // 19 skill init
    [T.INT8, 1], // 20 visage
    [T.INT8, 1], // 21 cheveux
    [T.INT32, 1], // 22 option flag
    [T.INT8, 1], // 23 taille de banque
    [T.UINT16, 1], // 24 event status flag
    [T.INT64, 1], // 25 temps criminel restant (INT64 dans Archonia, INT32 dans ALEmu)
    [T.INT64, 1], // 26 temps meurtrier restant
    [T.CHAR, 16], // 27 signature
    [T.MEMORY_BLOCK, 1], // 28 données GameGuard
    [T.UINT64, 1], // 29 dernière mort en Battle Square (UINT64 dans Archonia)
  ], V),

  // AgcmCharacter (type 3, « AgsmCharManager » côté serveur) — drapeaux sur 4 octets
  // (réglés par AuPacket::SetFlagLength(4), FUN_14014f540, dans le constructeur AgcmCharacter)
  CHAR_MANAGER: S('AGSMCHARMANAGER', PacketType.CHAR_MANAGER, 4, [
    [T.INT8, 1], // 0 opération
    [T.CHAR, 12], // 1
    [T.INT32, 1], // 2
    [T.CHAR, 49], // 3 nom de personnage ("Nom#Monde" côté login)
    [T.INT32, 1], // 4 CID
    [T.VEC3F, 1], // 5 position
    [T.INT32, 1], // 6 clé d'authentification
    [T.CHAR, 248], // 7 (nouveau dans Archonia)
  ], V),

  // AgpmLogin — drapeaux 2
  LOGIN: S('AGPMLOGIN', PacketType.LOGIN, 2, [
    [T.INT8, 1], // 0 opération
    [T.CHAR, 32], // 1 clé de hachage (8 octets utiles)
    [T.CHAR, 49], // 2 compte (chiffré RC4 côté client)
    [T.INT8, 1], // 3 longueur du compte
    [T.CHAR, 65], // 4 mot de passe (chiffré RC4) — 65 dans Archonia (33 dans ALEmu)
    [T.INT8, 1], // 5 longueur du mot de passe
    [T.INT32, 1], // 6 CID
    [T.CHAR, 32], // 7 nom du monde
    [T.PACKET, 1], // 8 mini AGPMLOGIN_CHAR_INFO
    [T.PACKET, 1], // 9 mini AGPMLOGIN_VERSION_INFO
    [T.INT32, 1], // 10 résultat
    [T.PACKET, 1], // 11 mini AGPMLOGIN_SERVER_INFO
    [T.CHAR, 2049], // 12 extra (étranger)
    [T.CHAR, 5], // 13 code / challenge
    [T.INT32, 1], // 14 compte limité
    [T.INT32, 1], // 15 compte protégé
  ], V),

  // AgpmTimer — drapeaux 1
  TIMER: S('AGPMTIMER', PacketType.TIMER, 1, [
    [T.UINT8, 1], // 0 opération
    [T.UINT64, 1], // 1 horodatage
    [T.UINT32, 1], // 2
    [T.UINT32, 1], // 3
  ], V),

  // AgpmWorld — drapeaux 1
  WORLD: S('AGPMWORLD', PacketType.WORLD, 1, [
    [T.INT8, 1], // 0 opération
    [T.CHAR, 32], // 1
    [T.INT16, 1], // 2
    [T.MEMORY_BLOCK, 1], // 3 liste des mondes encodée
    [T.CHAR, 32], // 4
  ], V),

  // AgpmChatting — drapeaux 1
  CHATTING: S('AGPMCHATTING', PacketType.CHATTING, 1, [
    [T.INT8, 1], // 0 opération
    [T.INT32, 1], // 1 CID émetteur
    [T.INT8, 1], // 2 type de canal
    [T.INT32, 1], // 3 CID cible
    [T.CHAR, 49], // 4 nom émetteur
    [T.CHAR, 49], // 5 nom cible
    [T.MEMORY_BLOCK, 1], // 6 message
  ], V),

  // AgpmSystemMessage — drapeaux 1
  SYSTEM_MESSAGE: S('AGPMSYSTEMMESSAGE', PacketType.SYSTEM_MESSAGE, 1, [
    [T.INT8, 1], [T.INT32, 1], [T.INT32, 1], [T.INT32, 1], [T.CHAR, 65], [T.CHAR, 65],
  ], V),

  // AgpmReturnToLogin — drapeaux 1
  RETURN_TO_LOGIN: S('AGPMRETURNTOLOGIN', PacketType.RETURN_TO_LOGIN, 1, [
    [T.INT8, 1], [T.INT32, 1], [T.CHAR, 49], [T.CHAR, 32], [T.CHAR, 25], [T.INT32, 1],
  ], V),

  // AgpmOptimizedPacket2 (0x40) — drapeaux 1
  OPT_CHAR_MOVE: S('AGPMOPTIMIZEDCHARMOVE', PacketType.OPT_CHAR_MOVE, 1, [
    [T.INT32, 1], // 0 CID
    [T.VEC3F, 1], // 1 position
    [T.VEC3F, 1], // 2 destination
    [T.INT32, 1], // 3 cible suivie
    [T.UINT16, 1], // 4 distance de suivi
    [T.INT8, 1], // 5 drapeaux de déplacement
    [T.INT8, 1], // 6 direction
    [T.INT8, 1], // 7
    [T.INT32, 1], // 8
  ], V),

  // AgpmOptimizedPacket2 (0x41) — drapeaux 2
  OPT_CHAR_ACTION: S('AGPMOPTIMIZEDCHARACTION', PacketType.OPT_CHAR_ACTION, 2, [
    [T.INT32, 1], [T.INT32, 1], [T.INT8, 1], [T.PACKET, 1], [T.INT32, 1], [T.INT8, 1], [T.INT8, 1], [T.UINT32, 1], [T.UINT8, 1],
  ], V),

  // AgpmOptimizedPacket2 (0x42) — drapeaux 2
  OPT_VIEW: S('AGPMOPTIMIZEDVIEW', PacketType.OPT_VIEW, 2, [
    [T.INT8, 1], // 0 opération
    [T.PACKET, 1], // 1
    [T.PACKET, 32], // 2 liste de vues (u8 compteur + minis) — 32 dans Archonia
    [T.MEMORY_BLOCK, 1], // 3
    [T.MEMORY_BLOCK, 1], // 4
    [T.CHAR, 33], // 5
    [T.MEMORY_BLOCK, 1], // 6
    [T.MEMORY_BLOCK, 1], // 7
    [T.MEMORY_BLOCK, 1], // 8
    [T.INT32, 1], // 9
    [T.INT32, 1], // 10
    [T.INT32, 1], // 11
    [T.INT32, 1], // 12
    [T.MEMORY_BLOCK, 1], // 13
  ], V),

  // AgpmStartupEncryption — drapeaux 1
  STARTUP_ENCRYPTION: S('AGPMSTARTUPENCRYPTION', PacketType.STARTUP_ENCRYPTION, 1, [
    [T.INT8, 1], // 0 opération
    [T.MEMORY_BLOCK, 1], // 1 clé
  ], V),
});

// ---------------------------------------------------------------------------------------
// Mini-paquets (identifiants internes à ArchCore, jamais transmis)
// ---------------------------------------------------------------------------------------
export const Mini = Object.freeze({
  LOGIN_SERVER_ADDR: S('AGPMLOGIN_SERVER_ADDR', 100, 1, [[T.CHAR, 23], [T.CHAR, 23], [T.CHAR, 23]], V),

  LOGIN_CHAR_INFO: S('AGPMLOGIN_CHAR_INFO', 101, 2, [
    [T.INT32, 1], // 0 TID
    [T.CHAR, 49], // 1 nom
    [T.INT32, 1], // 2 nombre de personnages / max
    [T.INT32, 1], // 3 index de slot
    [T.INT32, 1], // 4 union
    [T.INT32, 1], // 5 race
    [T.INT32, 1], // 6 cheveux
    [T.INT32, 1], // 7 visage
    [T.CHAR, 49], // 8
    [T.INT32, 1], // 9 (nouveau dans Archonia)
  ], V),

  LOGIN_SERVER_INFO: S('AGPMLOGIN_SERVER_INFO', 102, 1, [[T.CHAR, 23]], V),

  LOGIN_VERSION_INFO: S('AGPMLOGIN_VERSION_INFO', 103, 1, [[T.INT32, 1], [T.INT32, 1]], V),

  LOGIN_COMPEN_INFO: S('AGPMLOGIN_COMPEN_INFO', 104, 1, [[T.INT32, 1], [T.INT32, 1], [T.CHAR, 49], [T.CHAR, 100], [T.MEMORY_BLOCK, 1]], V),

  // Déplacement — confirmé dans le constructeur AgpmCharacter (drapeaux 1)
  CHAR_MOVE: S('AGPMCHAR_MOVE', 105, 1, [
    [T.VEC3F, 1], // 0 position
    [T.VEC3F, 1], // 1 destination
    [T.INT32, 1], // 2 cible suivie
    [T.INT32, 1], // 3 distance de suivi
    [T.FLOAT, 1], // 4 axe X
    [T.FLOAT, 1], // 5 axe Y
    [T.INT8, 1], // 6 drapeaux
    [T.INT8, 1], // 7 direction
  ], V),

  // AgpmCharacter : action/compétence — drapeaux 2
  CHAR_ACTION: S('AGPMCHAR_ACTION', 106, 2, [
    [T.INT8, 1], [T.INT32, 1], [T.INT32, 1], [T.INT8, 1], [T.PACKET, 1], [T.VEC3F, 1], [T.UINT8, 1], [T.INT8, 1], [T.UINT32, 1], [T.UINT8, 1],
  ], V),

  // AgpmFactors (toutes vérifiées, ordre : résultat, statut, type, points, max, récup, attributs, dégâts, défense, attaque, objet, dirt, prix)
  CHAR_FACTORS: S('AGPMCHAR_FACTORS', 107, 2, Array.from({ length: 13 }, () => [T.PACKET, 1]), V),
  FACTOR_STATUS: S('AGPMCHAR_FACTOR_STATUS', 108, 2, ints(13), V),
  FACTOR_TYPE: S('AGPMCHAR_FACTOR_TYPE', 109, 1, ints(3), V),
  FACTOR_POINT: S('AGPMCHAR_FACTOR_POINT', 110, 4, ints(20), V),
  FACTOR_POINTMAX: S('AGPMCHAR_FACTOR_POINTMAX', 111, 2, ints(11), V),
  FACTOR_RECOVERY: S('AGPMCHAR_FACTOR_RECOVERY', 112, 1, ints(3), V),
  FACTOR_ATTRIBUTE: S('AGPMCHAR_FACTOR_ATTRIBUTE', 113, 2, ints(15), V),
  FACTOR_DMG: S('AGPMCHAR_FACTOR_DMG', 114, 1, [[T.PACKET, 1], [T.PACKET, 1]], V),
  FACTOR_DEFENSE: S('AGPMCHAR_FACTOR_DEFENSE', 115, 1, [[T.PACKET, 1], [T.PACKET, 1]], V),
  FACTOR_ATTACK: S('AGPMCHAR_FACTOR_ATTACK', 116, 1, ints(8), V),
  FACTOR_ITEM: S('AGPMCHAR_FACTOR_ITEM', 117, 1, ints(5), V),
  FACTOR_DIRT: S('AGPMCHAR_FACTOR_DIRT', 118, 1, ints(8), V),
  FACTOR_PRICE: S('AGPMCHAR_FACTOR_PRICE', 119, 1, ints(4), V),

  // AgpmOptimizedPacket2 : vue d'un personnage — drapeaux 4
  OPT_VIEW_CHAR: S('AGPMOPTIMIZEDPACKET_VIEW', 120, 4, [
    [T.INT32, 1], [T.INT32, 1], [T.MEMORY_BLOCK, 1], [T.INT8, 1], [T.INT8, 1], [T.PACKET, 1], [T.INT8, 1], [T.INT8, 1], [T.VEC3F, 1],
    [T.VEC3F, 1], [T.INT32, 1], [T.UINT16, 1], [T.INT16, 1], [T.INT16, 1], [T.UINT64, 1], [T.INT8, 1], [T.INT8, 1], [T.INT8, 1],
    [T.INT8, 1], [T.UINT16, 1], [T.INT8, 1], [T.INT32, 1], [T.CHAR, 16], [T.INT32, 1], [T.INT32, 1], [T.INT32, 1],
  ], V),

  // AgpmOptimizedPacket2 : vue d'un objet — drapeaux 4
  OPT_VIEW_ITEM: S('AGPMOPTIMIZEDPACKET_ITEMVIEW', 121, 4, [
    [T.INT32, 1], [T.INT32, 1], [T.INT16, 1], [T.INT8, 1], [T.INT8, 1], [T.INT8, 1], ...ints(9),
    [T.UINT16, 1], [T.UINT16, 1], [T.UINT16, 1], [T.UINT16, 1], [T.UINT16, 1], [T.MEMORY_BLOCK, 1],
  ], V),
});

/** Index type de paquet → schéma principal. */
export const MainById = Object.freeze(Object.fromEntries(Object.values(Main).map((s) => [s.id, s])));

// Opérations (champ 0) des paquets de login
export const LoginOp = Object.freeze({
  HASHKEY: 0,
  USER_LOGIN: 1,
  UNION_INFO: 3,
  CHAR_NAME: 4,
  CHAR_NAME_FINISH: 5,
  CHAR_LIST: 6,
  CHAR_LIST_FINISH: 7,
  WORLD_ENTER: 8,
  CHAR_CREATION_ENTER: 10,
  CREATE_CHAR: 11,
  CREATE_CHAR_FINISH: 12,
  DELETE_CHAR: 13,
  LOGIN_RESULT: 15,
});

export const LoginResult = Object.freeze({
  INCORRECT_USER: 0,
  GIBBERISH: 1,
  TOO_MANY_ATTEMPTS: 2,
  TOS_AGREEMENT: 3,
  NAME_TAKEN: 7,
});

export const CryptOp = Object.freeze({
  REQUEST: 0,
  PUBLIC_KEY: 1,
  PRIVATE_KEY: 2,
  COMPLETE: 3,
  DYNCODE_PUB: 4,
  DYNCODE_PRV: 5,
  SET_ALGO: 6,
});

export const WorldOp = Object.freeze({ SERVER_LIST_REQ: 0, SERVER_LIST_ANS: 2, SERVER_EXTRA: 3 });

export const CharMgrOp = Object.freeze({
  GAME_ENTER: 2, // client → monde : champ 2 = 17, nom (3), clé d'auth (6), CHAR[248] (7) — FUN_1404b8100
  FINISH_ENTER: 7,
  LOADING_POS: 8,
  ENTER_WORLD: 9, // client → monde après LOADING_POS : CID au champ 4 — FUN_1404b75d0
  AUTH_KEY: 11, // login → client : clé pour le serveur monde
  KEEPALIVE: 13, // client → monde toutes les 15 s, champ 2 — FUN_1404b38b0
});
