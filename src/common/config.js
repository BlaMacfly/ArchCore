// Chargement de la configuration ArchCore (config/archcore.json).

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

export const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');

const DEFAULTS = {
  login: { port: 11000, bind: '0.0.0.0' },
  world: { port: 11008, bind: '0.0.0.0', name: 'ArchCore', group: 'ArchCore', publicAddress: '127.0.0.1' },
  database: { host: '127.0.0.1', port: 3306, user: 'root', password: 'archcore', connectionLimit: 8,
    auth: 'archcore_auth', characters: 'archcore_characters', world: 'archcore_world' },
  client: { dataDir: '../client/ArchLord/Game' },
};

// ARCHCORE_CONFIG=<fichier> permet d'utiliser une autre configuration (serveur de test, autres ports…).
export function loadConfig(file = process.env.ARCHCORE_CONFIG || path.join(ROOT, 'config', 'archcore.json')) {
  let user = {};
  try {
    user = JSON.parse(readFileSync(file, 'utf8'));
  } catch (err) {
    if (err.code !== 'ENOENT') throw err;
  }
  const merged = {};
  for (const key of Object.keys(DEFAULTS)) merged[key] = { ...DEFAULTS[key], ...(user[key] || {}) };
  merged.client.dataDir = path.resolve(ROOT, merged.client.dataDir);
  merged.file = file;
  return merged;
}
