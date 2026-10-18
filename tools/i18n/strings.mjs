// Extrait les chaînes affichables (texte naturel) des fichiers de langue et mesure ce qui reste en anglais.
import { readFileSync, readdirSync, existsSync, writeFileSync, mkdirSync } from 'node:fs';
import path from 'node:path';
import { loadIni } from './coverage.mjs';
import { loadConfig } from '../../src/common/config.js';

const GAME = process.argv[2] || loadConfig().client.dataDir;
// Une chaîne « affichable » : contient au moins une lettre, pas un identifiant technique
const TECH = /^(?:[A-Za-z_]+\d*|[\w.\/-]+\.(?:dds|tga|png|bmp|ini|txt|xml|wav|mp3|ogg|ecl|rws|dff|uvw)|[A-Z0-9_]+|-?[\d.:,;\s-]+|0x[0-9a-f]+)$/i;
export function displayStrings(text) {
  const out = new Set();
  for (const line of text.split(/\r?\n/)) {
    const cells = line.includes('\t') ? line.split('\t') : [line.slice(line.indexOf('=') + 1)];
    for (let c of cells) {
      c = c.trim();
      if (c.length < 2 || !/[A-Za-z]/.test(c) || TECH.test(c)) continue;
      if (!/\s/.test(c) && !/^<.+>/.test(c) && c.length < 4) continue;
      out.add(c);
    }
  }
  return out;
}
const report = [];
let tU = 0, tE = 0, tC = 0;
const pending = {};
for (const f of readdirSync(path.join(GAME, 'ini_fr')).sort()) {
  const en = displayStrings(loadIni(path.join(GAME, 'ini', f)).text);
  const fr = displayStrings(loadIni(path.join(GAME, 'ini_fr', f)).text);
  const still = [...fr].filter((s) => en.has(s));
  const chars = still.reduce((n, s) => n + s.length, 0);
  tU += fr.size; tE += still.length; tC += chars;
  pending[f] = still;
  report.push([f, fr.size, still.length, chars]);
}
console.log('fichier'.padEnd(28) + 'chaînes'.padEnd(10) + 'en anglais'.padEnd(12) + 'caractères');
for (const [f, a, b, c] of report) console.log(f.padEnd(28) + String(a).padEnd(10) + String(b).padEnd(12) + c);
console.log(`TOTAL : ${tE} chaînes distinctes encore en anglais sur ${tU} (${Math.round(100 * tE / tU)} %), ${tC} caractères`);
mkdirSync('data/i18n', { recursive: true });
writeFileSync('data/i18n/pending-en.json', JSON.stringify(pending, null, 1));
for (const f of ['CharacterDataTable.txt', 'ItemTemplateAll2.ini', 'UI_1024X768.ini', 'regionglossary.txt']) console.log(`--- exemples ${f} : ${pending[f].slice(0, 6).map((s) => JSON.stringify(s.slice(0, 60))).join(' | ')}`);
