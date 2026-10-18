// Importe les tables déchiffrées du client (data/client-ini) dans archcore_world.
//   node tools/import-client-data.mjs [dossier]
// Les fichiers sont en CP949 (coréen) ; ils sont convertis en UTF-8.

import { readFileSync, existsSync } from 'node:fs';
import path from 'node:path';
import mariadb from 'mariadb';
import { loadConfig, ROOT } from '../src/common/config.js';

const cfg = loadConfig().database;
const dir = process.argv[2] ? path.resolve(process.argv[2]) : path.join(ROOT, 'data', 'client-ini');
const decoder = new TextDecoder('euc-kr');
const read = (name) => decoder.decode(readFileSync(path.join(dir, name)));
const num = (v, d = 0) => { const n = Number(String(v ?? '').trim()); return Number.isFinite(n) ? n : d; };

/** Parse un .ini ArchLord : sections [N] → { key: valeur | [valeurs si répétée] }. */
function parseIni(text) {
  const sections = [];
  let cur = null;
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || line.startsWith('//') || line.startsWith(';')) continue;
    const sec = line.match(/^\[(.+)\]$/);
    if (sec) { cur = { id: sec[1], keys: {}, order: [] }; sections.push(cur); continue; }
    if (!cur) continue;
    const eq = line.indexOf('=');
    if (eq < 0) continue;
    const k = line.slice(0, eq).trim();
    const v = line.slice(eq + 1).trim();
    cur.order.push([k, v]);
    if (k in cur.keys) cur.keys[k] = [].concat(cur.keys[k], v); else cur.keys[k] = v;
  }
  return sections;
}

/** Parse un .txt tabulé avec en-tête. */
function parseTsv(text) {
  const lines = text.split(/\r?\n/).filter((l) => l.trim().length);
  const header = lines[0].split('\t').map((h) => h.trim());
  return { header, rows: lines.slice(1).map((l) => l.split('\t')) };
}

const first = (v) => (Array.isArray(v) ? v[0] : v);
const conn = await mariadb.createConnection({ host: cfg.host, port: cfg.port, user: cfg.user, password: cfg.password, database: cfg.world });
const batch = async (sql, rows) => { for (let i = 0; i < rows.length; i += 500) await conn.batch(sql, rows.slice(i, i + 500)); };

// ---------------------------------------------------------------- charactertemplate.ini
{
  const secs = parseIni(read('charactertemplate.ini'));
  const rows = secs.filter((s) => /^\d+$/.test(s.id)).map((s) => {
    const k = s.keys;
    const dmg = [].concat(k.MinDamage ?? []), dmgMax = [].concat(k.MaxDamage ?? []);
    return [Number(s.id), first(k.Name) ?? '', num(k.CharacterType), num(k.Race), num(k.Gender), num(k.Class), num(k.Level, 1),
      num(k.Con), num(k.Str), num(k.Int), num(k.Dex), num(k.Cha), num(k.Luk), num(k.Wis), num(k.Movement), num(k.MovementFast),
      num(k.Hp), num(k.Mp), num(k.Sp), num(k.AttackPoint), num(k.MagicAttackPoint), num(k.AR), num(k.DR), num(k.BaseEXP),
      num(dmg[dmg.length - 1]), num(dmgMax[dmgMax.length - 1]), num(k.AttackRange), num(k.AttackSpeed), num(k.Sight), JSON.stringify(k)];
  });
  await conn.query('DELETE FROM character_template');
  await batch('INSERT INTO character_template (tid,name,character_type,race,gender,class,level,con,str,intel,dex,cha,luk,wis,movement,movement_fast,hp,mp,sp,attack_point,magic_attack,ar,dr,base_exp,min_damage,max_damage,attack_range,attack_speed,sight,raw) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)', rows);
  console.log(`✓ character_template : ${rows.length}`);
}

// ---------------------------------------------------------------- CharacterDataTable.txt
{
  const { header, rows } = parseTsv(read('CharacterDataTable.txt'));
  const out = rows.filter((r) => /^\d+$/.test(r[0]?.trim())).map((r) => {
    const o = {};
    header.forEach((h, i) => { if (r[i] !== undefined && r[i] !== '') o[h] = r[i]; });
    return [Number(r[0]), r[1] ?? '', JSON.stringify(o)];
  });
  await conn.query('DELETE FROM character_data_table');
  await batch('INSERT INTO character_data_table (tid,name,data) VALUES (?,?,?)', out);
  console.log(`✓ character_data_table : ${out.length}`);
}

// ---------------------------------------------------------------- LevelUpExp.txt
{
  const { rows } = parseTsv(read('LevelUpExp.txt'));
  const out = rows.filter((r) => /^\d+$/.test(r[0]?.trim())).map((r) => [Number(r[0]), BigInt(r[1].trim())]);
  await conn.query('DELETE FROM level_exp');
  await batch('INSERT INTO level_exp (level,exp) VALUES (?,?)', out);
  console.log(`✓ level_exp : ${out.length}`);
}

// ---------------------------------------------------------------- GrowUpFactor.txt
{
  const lines = read('GrowUpFactor.txt').split(/\r?\n/);
  const out = [];
  let tid = null;
  for (const line of lines) {
    const cells = line.split('\t').map((c) => c.trim());
    if (!cells[0]) continue;
    if (/^\d+$/.test(cells[0]) && cells.slice(1).every((c) => c === '')) { tid = Number(cells[0]); continue; }
    if (cells[0] === 'LV' || tid === null) continue;
    if (!/^\d+$/.test(cells[0])) continue;
    const n = cells.map((c) => num(c));
    // LV STR Dex CON WIS INT CHA HP MP SP AC AR DR FIRE WATER EARTH AIR MAGIC MinDmg MaxDmg
    out.push([tid, n[0], n[1], n[2], n[3], n[4], n[5], n[6], n[7], n[8], n[9], n[10], n[11], n[12], n[13], n[14], n[15], n[16], n[17], n[18], n[19]]);
  }
  await conn.query('DELETE FROM growup_factor');
  await batch('INSERT INTO growup_factor (template_id,level,str,dex,con,wis,intel,cha,hp,mp,sp,ac,ar,dr,res_fire,res_water,res_earth,res_air,res_magic,min_damage,max_damage) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)', out);
  console.log(`✓ growup_factor : ${out.length}`);
}

// ---------------------------------------------------------------- logincharpos.ini
{
  const secs = parseIni(read('logincharpos.ini'));
  const out = secs.filter((s) => /^\d+$/.test(s.id)).map((s) => {
    const p = String(first(s.keys.Position) ?? '0,0,0').split(',').map((v) => num(v));
    return [Number(s.id), first(s.keys.Race) ?? '', p[0], p[1], p[2], num(s.keys.DegreeX), num(s.keys.DegreeY)];
  });
  await conn.query('DELETE FROM login_char_pos');
  await batch('INSERT INTO login_char_pos (idx,race,pos_x,pos_y,pos_z,degree_x,degree_y) VALUES (?,?,?,?,?,?,?)', out);
  console.log(`✓ login_char_pos : ${out.length}`);
}

// ---------------------------------------------------------------- itemtemplateentry.ini
if (existsSync(path.join(dir, 'itemtemplateentry.ini'))) {
  const secs = parseIni(read('itemtemplateentry.ini'));
  const out = secs.filter((s) => /^\d+$/.test(s.id)).map((s) => [Number(s.id), first(s.keys['0']) ?? '', first(s.keys['1']) ?? null, JSON.stringify(s.keys)]);
  await conn.query('DELETE FROM item_template');
  await batch('INSERT INTO item_template (tid,name,file,raw) VALUES (?,?,?,?)', out);
  console.log(`✓ item_template : ${out.length}`);
}

// ---------------------------------------------------------------- SKILLTEMPLATE.ini
if (existsSync(path.join(dir, 'SKILLTEMPLATE.ini'))) {
  const secs = parseIni(read('SKILLTEMPLATE.ini'));
  const out = secs.filter((s) => /^\d+$/.test(s.id)).map((s) => [Number(s.id), first(s.keys['1']) ?? first(s.keys.Name) ?? '', JSON.stringify(s.keys)]);
  await conn.query('DELETE FROM skill_template');
  await batch('INSERT INTO skill_template (tid,name,raw) VALUES (?,?,?)', out);
  console.log(`✓ skill_template : ${out.length}`);
}

// ---------------------------------------------------------------- npc.ini (PNJ placés dans le monde)
if (existsSync(path.join(dir, 'npc.ini'))) {
  const secs = parseIni(read('npc.ini'));
  const out = secs.filter((s) => /^\d+$/.test(s.id) && first(s.keys.TID) && first(s.keys.Position)).map((s) => {
    const p = String(first(s.keys.Position)).split(',').map((v) => num(v));
    const d = String(first(s.keys.Degree) ?? '0,0').split(',').map((v) => num(v));
    return [Number(s.id), num(first(s.keys.TID)), first(s.keys.Name) ?? '', p[0], p[1], p[2], d[0] ?? 0, d[1] ?? 0, JSON.stringify(s.order)];
  });
  await conn.query('DELETE FROM npc_spawn');
  await batch('INSERT INTO npc_spawn (id,tid,name,pos_x,pos_y,pos_z,degree_x,degree_y,raw) VALUES (?,?,?,?,?,?,?,?,?)', out);
  console.log(`✓ npc_spawn : ${out.length}`);
}

await conn.end();
