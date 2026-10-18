// Déchiffre les tables de données du client Archonia (dossier Game/ini) vers data/client-ini/.
// Chiffrement constaté dans Archonia.exe (FUN_14015ae00) : CryptoAPI MD5("1111") → RC4.
// Les fichiers déjà en clair sont copiés tels quels. Les sous-dossiers sont parcourus.
import { readFileSync, writeFileSync, mkdirSync, readdirSync, statSync } from 'node:fs';
import path from 'node:path';
import { capiDecrypt } from './capi-rc4.js';
import { loadConfig } from '../src/common/config.js';

const KEY = process.env.ARCHONIA_INI_KEY || '1111';
const src = process.argv[2] || path.join(loadConfig().client.dataDir, 'ini');
const dst = process.argv[3] || path.resolve('data/client-ini');

function looksPlain(buf) {
  const n = Math.min(buf.length, 256);
  if (n === 0) return true;
  let ok = 0;
  for (let i = 0; i < n; i++) {
    const c = buf[i];
    if ((c >= 0x20 && c < 0x7f) || c === 0x0d || c === 0x0a || c === 0x09 || c >= 0xa1) ok++;
  }
  // un fichier chiffré contient beaucoup d'octets de contrôle < 0x20
  const ctrl = n - ok;
  return ctrl / n < 0.05;
}

let done = 0, copied = 0, failed = 0;
function walk(dirSrc, dirDst) {
  mkdirSync(dirDst, { recursive: true });
  for (const name of readdirSync(dirSrc)) {
    const s = path.join(dirSrc, name);
    const d = path.join(dirDst, name);
    if (statSync(s).isDirectory()) { walk(s, d); continue; }
    const data = readFileSync(s);
    if (looksPlain(data)) { writeFileSync(d, data); copied++; continue; }
    const out = capiDecrypt(KEY, data);
    if (!looksPlain(out)) { failed++; console.warn(`?? ${path.relative(src, s)} : toujours illisible après déchiffrement`); }
    writeFileSync(d, out);
    done++;
  }
}
walk(src, dst);
console.log(`déchiffrés : ${done}, copiés en clair : ${copied}, douteux : ${failed} → ${dst}`);
