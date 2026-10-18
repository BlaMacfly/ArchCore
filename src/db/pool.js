// Accès MariaDB : un pool par base (auth, characters, world).

import mariadb from 'mariadb';
import { log } from '../common/log.js';

export class Databases {
  constructor(cfg) {
    this.cfg = cfg;
    const base = {
      host: cfg.host,
      port: cfg.port,
      user: cfg.user,
      password: cfg.password,
      connectionLimit: cfg.connectionLimit ?? 8,
      bigIntAsNumber: true,
      decimalAsNumber: true,
    };
    this.auth = mariadb.createPool({ ...base, database: cfg.auth });
    this.characters = mariadb.createPool({ ...base, database: cfg.characters });
    this.world = mariadb.createPool({ ...base, database: cfg.world });
  }

  async check() {
    for (const [name, pool] of [['auth', this.auth], ['characters', this.characters], ['world', this.world]]) {
      const rows = await pool.query('SELECT DATABASE() AS db, VERSION() AS v');
      log.info(`MariaDB ${rows[0].v} — base ${name} = ${rows[0].db}`);
    }
  }

  async close() {
    await Promise.all([this.auth.end(), this.characters.end(), this.world.end()]);
  }
}
