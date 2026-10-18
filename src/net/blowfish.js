// Blowfish tel qu'utilisé par le protocole ArchLord.
//
// L'algorithme est le Blowfish standard (P-array / S-boxes de Schneier, planification de
// clé standard). La seule particularité du jeu est la disposition des mots : le client
// caste directement les octets du tampon en uint32 natifs x86, donc chaque bloc de 8 octets
// est lu comme deux mots **little-endian** (L = octets 0..3, R = octets 4..7).
//
// Référence : AlefCrypto.h (ALEmu) qui reproduit le code du client.

import { P_INIT, S_INIT } from './blowfish-tables.js';

export class Blowfish {
  /** @param {Buffer|Uint8Array|string} key clé de 4 à 56 octets (32 octets dans le jeu) */
  constructor(key) {
    const k = Buffer.isBuffer(key) ? key : Buffer.from(key, 'latin1');
    if (k.length < 4 || k.length > 56) {
      throw new RangeError(`Blowfish : clé de 4 à 56 octets attendue, reçu ${k.length}`);
    }
    this.P = Uint32Array.from(P_INIT);
    this.S = S_INIT.map((box) => Uint32Array.from(box));

    let j = 0;
    for (let i = 0; i < 18; i++) {
      let data = 0;
      for (let n = 0; n < 4; n++) {
        data = ((data << 8) | k[j++]) >>> 0;
        if (j >= k.length) j = 0;
      }
      this.P[i] = (this.P[i] ^ data) >>> 0;
    }

    let l = 0;
    let r = 0;
    for (let i = 0; i < 18; i += 2) {
      [l, r] = this.encryptWords(l, r);
      this.P[i] = l;
      this.P[i + 1] = r;
    }
    for (let b = 0; b < 4; b++) {
      for (let i = 0; i < 256; i += 2) {
        [l, r] = this.encryptWords(l, r);
        this.S[b][i] = l;
        this.S[b][i + 1] = r;
      }
    }
  }

  #f(x) {
    const S = this.S;
    const a = (S[0][x >>> 24] + S[1][(x >>> 16) & 0xff]) >>> 0;
    const b = (a ^ S[2][(x >>> 8) & 0xff]) >>> 0;
    return (b + S[3][x & 0xff]) >>> 0;
  }

  /** Chiffre un bloc exprimé en deux mots 32 bits (xL, xR). */
  encryptWords(l, r) {
    const P = this.P;
    for (let i = 0; i < 16; i += 2) {
      l = (l ^ P[i]) >>> 0;
      r = (r ^ this.#f(l)) >>> 0;
      r = (r ^ P[i + 1]) >>> 0;
      l = (l ^ this.#f(r)) >>> 0;
    }
    l = (l ^ P[16]) >>> 0;
    r = (r ^ P[17]) >>> 0;
    return [r, l];
  }

  /** Déchiffre un bloc exprimé en deux mots 32 bits (xL, xR). */
  decryptWords(l, r) {
    const P = this.P;
    for (let i = 17; i > 1; i -= 2) {
      l = (l ^ P[i]) >>> 0;
      r = (r ^ this.#f(l)) >>> 0;
      r = (r ^ P[i - 1]) >>> 0;
      l = (l ^ this.#f(r)) >>> 0;
    }
    l = (l ^ P[1]) >>> 0;
    r = (r ^ P[0]) >>> 0;
    return [r, l];
  }

  /** Chiffre en place `buf[off..off+8)` avec la disposition little-endian du jeu. */
  encryptBlockLE(buf, off = 0) {
    const [l, r] = this.encryptWords(buf.readUInt32LE(off), buf.readUInt32LE(off + 4));
    buf.writeUInt32LE(l, off);
    buf.writeUInt32LE(r, off + 4);
  }

  /** Déchiffre en place `buf[off..off+8)` avec la disposition little-endian du jeu. */
  decryptBlockLE(buf, off = 0) {
    const [l, r] = this.decryptWords(buf.readUInt32LE(off), buf.readUInt32LE(off + 4));
    buf.writeUInt32LE(l, off);
    buf.writeUInt32LE(r, off + 4);
  }

  /** Chiffre en place tout un tampon (longueur multiple de 8), mode ECB du jeu. */
  encryptBufferLE(buf) {
    if (buf.length % 8) throw new RangeError('Blowfish : longueur non multiple de 8');
    for (let off = 0; off < buf.length; off += 8) this.encryptBlockLE(buf, off);
    return buf;
  }

  /** Déchiffre en place tout un tampon (longueur multiple de 8), mode ECB du jeu. */
  decryptBufferLE(buf) {
    if (buf.length % 8) throw new RangeError('Blowfish : longueur non multiple de 8');
    for (let off = 0; off < buf.length; off += 8) this.decryptBlockLE(buf, off);
    return buf;
  }
}
