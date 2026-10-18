// Crée les bases ArchCore et applique les scripts sql/*.sql.
//   node tools/db-init.mjs            → crée/actualise le schéma
//   node tools/db-init.mjs --account user pass [gm]   → crée aussi un compte

import { readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import mariadb from 'mariadb';
import { loadConfig, ROOT } from '../src/common/config.js';

const cfg = loadConfig().database;
const conn = await mariadb.createConnection({ host: cfg.host, port: cfg.port, user: cfg.user, password: cfg.password, multipleStatements: true });

const dbs = { '001_auth.sql': cfg.auth, '002_characters.sql': cfg.characters, '003_world.sql': cfg.world };
for (const name of Object.values(dbs)) await conn.query(`CREATE DATABASE IF NOT EXISTS \`${name}\` CHARACTER SET utf8mb4`);

const sqlDir = path.join(ROOT, 'sql');
for (const file of readdirSync(sqlDir).filter((f) => f.endsWith('.sql')).sort()) {
  const target = dbs[file];
  if (!target) { console.warn(`? ${file} : base cible inconnue, ignoré`); continue; }
  await conn.query(`USE \`${target}\``);
  await conn.query(readFileSync(path.join(sqlDir, file), 'utf8'));
  console.log(`✓ ${file} → ${target}`);
}

const i = process.argv.indexOf('--account');
if (i > 0) {
  const [user, pass, gm = '0'] = process.argv.slice(i + 1);
  if (!user || !pass) throw new Error('--account <utilisateur> <mot de passe> [gm]');
  const hash = createHash('sha256').update(pass, 'latin1').digest('hex');
  await conn.query(`USE \`${cfg.auth}\``);
  await conn.query('INSERT INTO account (username, password_hash, gm_level) VALUES (?,?,?) ON DUPLICATE KEY UPDATE password_hash = VALUES(password_hash), gm_level = VALUES(gm_level)', [user, hash, Number(gm)]);
  console.log(`✓ compte ${user} (gm ${gm})`);
}
await conn.end();
