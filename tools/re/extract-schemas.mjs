// Extrait, depuis les pseudocodes Ghidra (sorties batch_decompile de REA) des constructeurs de
// modules Agpm*/Agcm*, le nom du module, les types de paquets enregistrés, et chaque appel à
// AuPacket::SetFieldType (FUN_14014e850) : (type, taille)... terminé par 0xff.
import { readFileSync, writeFileSync } from 'node:fs';

const TYPE_NAMES = ['CHAR','INT8','UINT8','INT16','UINT16','INT32','UINT32','INT64','UINT64','FLOAT','VEC3F','MATRIX','PACKET','MEMORY_BLOCK','POS_BASEMETER','WCHAR'];
const files = process.argv.slice(2);
const modules = [];

function num(tok) {
  tok = tok.trim();
  if (/^0x[0-9a-f]+$/i.test(tok)) return parseInt(tok, 16);
  if (/^-?\d+$/.test(tok)) return parseInt(tok, 10);
  return null;
}

function parseFunction(code) {
  const lines = code.split('\n');
  // table des affectations simples de variables : uVarN = CONCAT44(x, V) | uVarN = V
  const vars = new Map();
  const resolve = (tok, depth = 0) => {
    tok = tok.trim();
    const m = tok.match(/^CONCAT44\([^,]+,\s*(.+)\)$/);
    if (m) return resolve(m[1], depth + 1);
    const n = num(tok);
    if (n !== null) return n;
    if (vars.has(tok) && depth < 5) return resolve(vars.get(tok), depth + 1);
    return tok; // non résolu
  };
  let name = null;
  const packetTypes = [];
  const flagLens = [];
  const fieldCalls = [];
  for (const raw of lines) {
    const line = raw.trim();
    let m;
    if ((m = line.match(/^(\w+) = (.+);$/)) && !line.includes('FUN_')) vars.set(m[1], m[2]);
    if ((m = line.match(/strncpy\(\w+,"([^"]+)"/))) name = m[1];
    if ((m = line.match(/\+ 0x1264\) = (0x[0-9a-f]+|\d+);/))) packetTypes.push(num(m[1]));
    if ((m = line.match(/\*\(undefined2 \*\)\(.*\) = (\d+);/))) flagLens.push({ target: line.split(' = ')[0], value: num(m[1]) });
  }
  // appels SetFieldType : possiblement sur plusieurs lignes
  const joined = lines.map((l) => l.trim()).join(' ');
  const re = /FUN_14014e850\(([^;]*?)\);/g;
  let c;
  while ((c = re.exec(joined))) {
    // découpe des arguments en respectant les parenthèses
    const args = [];
    let depth = 0, cur = '';
    for (const ch of c[1]) {
      if (ch === '(') depth++;
      if (ch === ')') depth--;
      if (ch === ',' && depth === 0) { args.push(cur); cur = ''; } else cur += ch;
    }
    args.push(cur);
    const target = args[0].trim();
    const vals = args.slice(1).map((a) => resolve(a));
    const fields = [];
    let unresolved = false;
    for (let i = 0; i + 1 < vals.length; i += 2) {
      if (vals[i] === 0xff) break;
      if (typeof vals[i] !== 'number' || typeof vals[i + 1] !== 'number') { unresolved = true; fields.push([vals[i], vals[i + 1]]); continue; }
      fields.push([vals[i], vals[i + 1]]);
    }
    fieldCalls.push({ target, fields, unresolved, pretty: fields.map(([t, s]) => (typeof t === 'number' ? `${TYPE_NAMES[t] ?? t}${s !== 1 ? `[${s}]` : ''}` : `?${t}:${s}`)).join(' ') });
  }
  return { name, packetTypes, flagLens, fieldCalls };
}

for (const f of files) {
  const d = JSON.parse(readFileSync(f, 'utf8'));
  for (const it of d.normalized_result.items) {
    const code = typeof it.pseudocode === 'string' ? it.pseudocode : (it.pseudocode?.value ?? '');
    const head = (code.split('\n').find((l) => /FUN_[0-9a-f]+\(/.test(l)) || '').trim();
    const info = parseFunction(code);
    modules.push({ address: it.address, fn: head.slice(0, 60), ...info });
  }
}
modules.sort((a, b) => (a.packetTypes[0] ?? 999) - (b.packetTypes[0] ?? 999));
writeFileSync('data/re/packet-schemas.json', JSON.stringify(modules, null, 1));
for (const m of modules) {
  console.log(`== ${m.name ?? '(?)'} @${m.address} types=[${m.packetTypes.map((t) => '0x' + t.toString(16)).join(',')}] flagLens=[${m.flagLens.map((f) => f.value).join(',')}]`);
  m.fieldCalls.forEach((fc, i) => console.log(`   #${i} ${fc.unresolved ? '(!)' : '   '} ${fc.pretty}`));
}
