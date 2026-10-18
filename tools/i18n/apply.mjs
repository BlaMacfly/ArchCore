// Applique la mémoire de traduction data/i18n/fr.json aux fichiers de langue du client (Game/ini_fr).
//
//   node tools/i18n/apply.mjs            → applique (accents conservés, encodage Windows-1252)
//   node tools/i18n/apply.mjs --ascii    → retire les accents (si la police du jeu ne les affiche pas)
//   node tools/i18n/apply.mjs --dry      → n'écrit rien, affiche seulement le nombre de remplacements
//
// La première exécution copie ini_fr vers ini_fr.orig ; chaque application repart de ini_fr.orig,
// donc l'outil est rejouable sans empiler les remplacements. Les fichiers chiffrés à l'origine
// (RC4 / MD5("1111")) sont rechiffrés de la même façon.
//
// Format de fr.json :
//   { "files": { "<fichier>": { "<texte actuel exact>": "<traduction>", "ko:<coréen décodé>": "<traduction>" } },
//     "global": { "<texte actuel exact>": "<traduction>" } }
// Les cellules comparées sont les colonnes séparées par des tabulations, sinon la valeur après « = ».

import { readFileSync, writeFileSync, existsSync, mkdirSync, readdirSync, copyFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { capiCrypt } from '../../src/common/capi-rc4.js';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const GAME = path.resolve(ROOT, '..', 'client', 'ArchLord', 'Game');
const SRC = path.join(GAME, 'ini_fr.orig');
const DST = path.join(GAME, 'ini_fr');
const args = new Set(process.argv.slice(2));
const ASCII = args.has('--ascii');
const DRY = args.has('--dry');

const isPlain = (b) => { let c = 0; const n = Math.min(b.length, 256); for (let i = 0; i < n; i++) { const x = b[i]; if (x < 0x20 && x !== 9 && x !== 10 && x !== 13) c++; } return c / n < 0.05; };
const korean = new TextDecoder('euc-kr');
const stripAccents = (s) => s.normalize('NFD').replace(/[̀-ͯ]/g, '').replace(/œ/g, 'oe').replace(/Œ/g, 'OE').replace(/[«»]/g, '"').replace(/’/g, "'");

// Encodage Windows-1252 (latin1 + quelques signes typographiques)
const CP1252 = { '€': 0x80, '‚': 0x82, '„': 0x84, '…': 0x85, '‘': 0x91, '’': 0x92, '“': 0x93, '”': 0x94, '–': 0x96, '—': 0x97, 'œ': 0x9c, 'Œ': 0x8c };
function toCp1252(s) {
  const out = Buffer.alloc(s.length);
  let n = 0;
  for (const ch of s) {
    const code = ch.codePointAt(0);
    if (code < 0x100) out[n++] = code;
    else if (CP1252[ch] !== undefined) out[n++] = CP1252[ch];
    else out[n++] = 0x3f; // « ? » pour un caractère non représentable
  }
  return out.subarray(0, n);
}

const tm = JSON.parse(readFileSync(path.join(ROOT, 'data', 'i18n', 'fr.json'), 'utf8'));
const global = tm.global ?? {};

if (!existsSync(SRC)) {
  mkdirSync(SRC, { recursive: true });
  for (const f of readdirSync(DST)) copyFileSync(path.join(DST, f), path.join(SRC, f));
  console.log(`copie d'origine créée : ${SRC}`);
}

let total = 0;
for (const f of readdirSync(SRC).sort()) {
  const map = { ...global, ...(tm.files?.[f] ?? {}) };
  const koMap = Object.fromEntries(Object.entries(map).filter(([k]) => k.startsWith('ko:')).map(([k, v]) => [k.slice(3), v]));
  if (!Object.keys(map).length) { if (!DRY) copyFileSync(path.join(SRC, f), path.join(DST, f)); continue; }
  const raw = readFileSync(path.join(SRC, f));
  const encrypted = !isPlain(raw);
  const text = (encrypted ? capiCrypt('1111', raw) : raw).toString('latin1');
  let count = 0;
  const translate = (cell) => {
    const key = cell.trim();
    let out = map[key];
    if (out === undefined && /[\x80-\xff]/.test(key)) out = koMap[korean.decode(Buffer.from(key, 'latin1'))];
    if (out === undefined) return cell;
    count++;
    return cell.replace(key, ASCII ? stripAccents(out) : out);
  };
  const lines = text.split(/(\r?\n)/);
  for (let i = 0; i < lines.length; i += 2) {
    const line = lines[i];
    if (line.includes('\t')) lines[i] = line.split('\t').map(translate).join('\t');
    else if (line.includes('=')) { const eq = line.indexOf('='); lines[i] = line.slice(0, eq + 1) + translate(line.slice(eq + 1)); }
  }
  total += count;
  console.log(`${f.padEnd(28)} ${String(count).padStart(6)} remplacement(s)${encrypted ? ' (rechiffré)' : ''}`);
  if (DRY) continue;
  const plain = toCp1252(lines.join(''));
  writeFileSync(path.join(DST, f), encrypted ? capiCrypt('1111', plain) : plain);
}
console.log(`${DRY ? '[essai] ' : ''}${total} remplacement(s) au total${ASCII ? ' (sans accents)' : ''}`);
