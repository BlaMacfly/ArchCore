// Framing réseau ArchLord : encapsulation chiffrée et découpage des trames TCP.
//
// Trame chiffrée (après le handshake AGPMSTARTUPENCRYPTION), relevée dans Archonia.exe
// (FUN_14014f5e0, AuPacket::Encrypt) :
//   0x00  u8   0xA1 (serveur → client) ou 0xB1 (client → serveur)
//   0x01  u32  taille totale de la trame (en-tête 9 + données + pied 1)
//   0x05  u32  compteur de paquets
//   0x09  données : paquet en clair (0xD6 … 0x6B) complété de zéros à un multiple de 8 puis
//         chiffré Blowfish ECB (clé du serveur pour 0xA1, clé du client pour 0xB1)
//   fin   u8   0xAF (serveur) ou 0xBF (client)
//
// Avant le handshake, les paquets circulent en clair (0xD6 … 0x6B).

import { Blowfish } from './blowfish.js';
import { GUARD_HEAD, GUARD_TAIL, HEADER_SIZE } from './packet.js';

export const FRAME_SERVER = 0xa1;
export const FRAME_CLIENT = 0xb1;
export const FRAME_TAIL_SERVER = 0xaf;
export const FRAME_TAIL_CLIENT = 0xbf;
export const FRAME_HEADER = 9;
export const FRAME_OVERHEAD = 10;

export class CryptoSession {
  constructor() {
    this.serverCipher = null; // Blowfish avec la clé annoncée par le serveur
    this.clientCipher = null; // Blowfish avec la clé envoyée (chiffrée) par le client
    this.counter = 0;
    this.ready = false; // true une fois CRYPT_COMPLETE envoyé
  }

  setServerKey(key) { this.serverCipher = new Blowfish(key); }
  setClientKey(key) { this.clientCipher = new Blowfish(key); }

  /** Chiffre un paquet en clair en une trame 0xA1. */
  encodeFrame(plain) {
    if (!this.serverCipher) throw new Error('CryptoSession : clé serveur absente');
    if (plain[0] !== GUARD_HEAD || plain.length < HEADER_SIZE + 3) throw new RangeError('paquet en clair invalide');
    const padded = plain.length % 8 ? plain.length + (8 - (plain.length % 8)) : plain.length;
    const frame = Buffer.alloc(FRAME_OVERHEAD + padded);
    plain.copy(frame, FRAME_HEADER);
    this.serverCipher.encryptBufferLE(frame.subarray(FRAME_HEADER, FRAME_HEADER + padded));
    frame[0] = FRAME_SERVER;
    frame.writeUInt32LE(frame.length, 1);
    frame.writeUInt32LE(++this.counter >>> 0, 5);
    frame[frame.length - 1] = FRAME_TAIL_SERVER;
    return frame;
  }

  /** Déchiffre une trame 0xB1/0xA1 complète et rend le paquet en clair. */
  decodeFrame(frame) {
    const fromServer = frame[0] === FRAME_SERVER;
    const cipher = fromServer ? this.serverCipher : this.clientCipher;
    if (!cipher) throw new Error('CryptoSession : clé absente pour déchiffrer');
    if (frame[frame.length - 1] !== (fromServer ? FRAME_TAIL_SERVER : FRAME_TAIL_CLIENT)) throw new RangeError('pied de trame invalide');
    const body = Buffer.from(frame.subarray(FRAME_HEADER, frame.length - 1));
    if (body.length % 8) throw new RangeError('corps chiffré non aligné sur 8');
    cipher.decryptBufferLE(body);
    if (body[0] !== GUARD_HEAD) throw new RangeError('déchiffrement incorrect (pas de 0xD6)');
    const size = body.readUInt32LE(1);
    if (size < HEADER_SIZE + 2 || size > body.length) throw new RangeError(`taille de paquet incohérente ${size}`);
    if (body[size - 1] !== GUARD_TAIL) throw new RangeError('pied de paquet invalide après déchiffrement');
    return body.subarray(0, size);
  }
}

/**
 * Découpeur incrémental : accumule les octets reçus et rend les trames complètes.
 * Chaque élément rendu est { encrypted:boolean, data:Buffer } où `data` est soit un
 * paquet en clair, soit une trame chiffrée complète.
 */
export class FrameSplitter {
  constructor() { this.pending = Buffer.alloc(0); }

  push(chunk) {
    this.pending = this.pending.length ? Buffer.concat([this.pending, chunk]) : Buffer.from(chunk);
    const out = [];
    for (;;) {
      if (this.pending.length < 5) break;
      const head = this.pending[0];
      const size = this.pending.readUInt32LE(1);
      if (head === GUARD_HEAD) {
        if (size < HEADER_SIZE + 2) throw new RangeError(`paquet en clair trop court (${size})`);
        if (this.pending.length < size) break;
        out.push({ encrypted: false, data: Buffer.from(this.pending.subarray(0, size)) });
        this.pending = this.pending.subarray(size);
      } else if (head === FRAME_CLIENT || head === FRAME_SERVER) {
        if (size < FRAME_OVERHEAD + 24) throw new RangeError(`trame chiffrée trop courte (${size})`);
        if (this.pending.length < size) break;
        out.push({ encrypted: true, data: Buffer.from(this.pending.subarray(0, size)) });
        this.pending = this.pending.subarray(size);
      } else {
        throw new RangeError(`octet de garde inconnu 0x${head.toString(16)}`);
      }
    }
    return out;
  }
}
