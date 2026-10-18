// RC4 tel que dérivé par CryptoAPI (Base Provider, CryptDeriveKey MD5 → RC4 40 bits + sel CRYPT_CREATE_SALT).
// Clé RC4 effective = les 16 octets du MD5 de la chaîne de clé (5 octets de clé + 11 octets de sel dérivés du hash).
import { createHash } from 'node:crypto';

export function rc4(key, data) {
  const S = new Uint8Array(256);
  for (let i = 0; i < 256; i++) S[i] = i;
  for (let i = 0, j = 0; i < 256; i++) {
    j = (j + S[i] + key[i % key.length]) & 0xff;
    [S[i], S[j]] = [S[j], S[i]];
  }
  const out = Buffer.alloc(data.length);
  for (let n = 0, i = 0, j = 0; n < data.length; n++) {
    i = (i + 1) & 0xff;
    j = (j + S[i]) & 0xff;
    [S[i], S[j]] = [S[j], S[i]];
    out[n] = data[n] ^ S[(S[i] + S[j]) & 0xff];
  }
  return out;
}

/** Déchiffre comme FUN_14015ae00 du client : CryptDeriveKey(MD5(keyString), RC4, CRYPT_CREATE_SALT). */
export function capiDecrypt(keyString, data, { keyBytes = 16 } = {}) {
  const md5 = createHash('md5').update(Buffer.from(keyString, 'latin1')).digest();
  return rc4(md5.subarray(0, keyBytes), data);
}
