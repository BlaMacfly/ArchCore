// Handshake AGPMSTARTUPENCRYPTION (type 0x48), commun au login et au monde.
//
//   client → CRYPT_REQUEST (0)              en clair
//   serveur → CRYPT_SET_ALGO (6)            en clair (bloc mémoire vide)
//   serveur → CRYPT_PUBLIC_KEY (1) + clé    en clair, 32 octets aléatoires ; clé Blowfish « serveur »
//   client → CRYPT_PRIVATE_KEY (2) + clé    en clair, 32 octets chiffrés Blowfish avec la clé serveur
//   serveur → CRYPT_COMPLETE (3)            chiffré ; tout le reste de la session est chiffré
//
// Schéma confirmé dans Archonia.exe (constructeur AgpmStartupEncryption : drapeau 1 octet,
// champs INT8 + MEMORY_BLOCK, type 0x48).

import { randomBytes } from 'node:crypto';
import { Blowfish } from './blowfish.js';
import { Main, CryptOp } from './schema.js';
import { log } from '../common/log.js';

export async function handleStartupEncryption(session, packet) {
  const op = packet.values[0];
  switch (op) {
    case CryptOp.REQUEST: {
      session.send(Main.STARTUP_ENCRYPTION, [CryptOp.SET_ALGO, Buffer.alloc(0)], { encrypt: false });
      const serverKey = randomBytes(32);
      session.crypto.setServerKey(serverKey);
      session.send(Main.STARTUP_ENCRYPTION, [CryptOp.PUBLIC_KEY, serverKey], { encrypt: false });
      return;
    }
    case CryptOp.PRIVATE_KEY: {
      const block = packet.values[1];
      if (!block || block.length !== 32) {
        log.warn(`[${session.server.name}] #${session.id} clé client invalide (${block?.length ?? 0} octets)`);
        session.close();
        return;
      }
      if (!session.crypto.serverCipher) {
        log.warn(`[${session.server.name}] #${session.id} clé client reçue avant la clé serveur`);
        session.close();
        return;
      }
      const clientKey = Buffer.from(block);
      session.crypto.serverCipher.decryptBufferLE(clientKey);
      session.crypto.setClientKey(clientKey);
      session.crypto.ready = true;
      session.send(Main.STARTUP_ENCRYPTION, [CryptOp.COMPLETE, null], { encrypt: true });
      log.info(`[${session.server.name}] #${session.id} chiffrement Blowfish établi`);
      return;
    }
    default:
      log.warn(`[${session.server.name}] #${session.id} opération de chiffrement inconnue ${op}`);
  }
}

export { Blowfish };
