// RC4 tel que dérivé par CryptoAPI dans le client ArchLord 64 bits :
//   CryptCreateHash(MD5) → CryptHashData(clé texte) → CryptDeriveKey(CALG_RC4, CRYPT_CREATE_SALT)
// Sur le « Microsoft Base Cryptographic Provider », la clé RC4 effective est constituée des
// 16 octets du MD5 (5 octets de clé + 11 octets de sel dérivés du hachage).
// Vérifié : déchiffre le fichier de serveurs et toutes les tables ini du client avec la clé "1111".
// Le même mécanisme est utilisé par le client pour chiffrer compte et mot de passe au login,
// avec pour clé la chaîne de 8 caractères envoyée par le serveur (opération HASHKEY).

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

export function capiRc4Key(keyString) {
  return createHash('md5').update(Buffer.isBuffer(keyString) ? keyString : Buffer.from(keyString, 'latin1')).digest();
}

/** Chiffre ou déchiffre (RC4 est symétrique) comme CryptEncrypt/CryptDecrypt du client. */
export function capiCrypt(keyString, data) {
  return rc4(capiRc4Key(keyString), data);
}
