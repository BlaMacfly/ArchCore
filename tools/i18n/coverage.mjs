// Mesure la couverture de la traduction française (ini_fr) par rapport à l'anglais (ini).
// Compare les lignes : une ligne de ini_fr identique à celle de ini contenant du texte = non traduite.
import { readFileSync, existsSync, readdirSync } from 'node:fs';
import path from 'node:path';
import { capiCrypt } from '../../src/common/capi-rc4.js';
import { loadConfig } from '../../src/common/config.js';

const GAME = process.argv[2] || loadConfig().client.dataDir;
const isPlain = (b) => { let c = 0; const n = Math.min(b.length, 256); for (let i = 0; i < n; i++) { const x = b[i]; if (x < 0x20 && x !== 9 && x !== 10 && x !== 13) c++; } return c / n < 0.05; };
export const loadIni = (p) => { const b = readFileSync(p); return { encrypted: !isPlain(b), text: (isPlain(b) ? b : capiCrypt('1111', b)).toString('latin1') }; };
const hasWords = (s) => /[A-Za-z]{3,}/.test(s.replace(/\b(ModuleData\d*|Agp\w+|Agc\w+|Apm\w+|TID|ID|Name|0x[0-9a-f]+)\b/g, ''));

const rows = [];
let totalText = 0, totalSame = 0;
for (const f of readdirSync(path.join(GAME, 'ini_fr')).sort()) {
  const en = path.join(GAME, 'ini', f);
  if (!existsSync(en)) { rows.push([f, 'absent de ini', '', '', '']); continue; }
  const a = loadIni(en).text.split(/\r?\n/);
  const b = loadIni(path.join(GAME, 'ini_fr', f)).text.split(/\r?\n/);
  const enSet = new Set(a.filter(hasWords));
  let text = 0, same = 0;
  for (const l of b) { if (!hasWords(l)) continue; text++; if (enSet.has(l)) same++; }
  totalText += text; totalSame += same;
  rows.push([f, String(a.length), String(b.length), String(text), `${same} (${text ? Math.round((100 * same) / text) : 0} %)`]);
}
const w = [26, 8, 8, 8, 14];
console.log(['fichier', 'lignes EN', 'lignes FR', 'texte FR', 'encore anglais'].map((s, i) => s.padEnd(w[i])).join(''));
for (const r of rows) console.log(r.map((s, i) => s.padEnd(w[i])).join(''));
console.log(`TOTAL : ${totalSame} lignes encore en anglais sur ${totalText} (${Math.round((100 * totalSame) / totalText)} %)`);
