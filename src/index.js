// Point d'entrée ArchCore : `node src/index.js [login|world|all]`.

import { loadConfig } from './common/config.js';
import { Databases } from './db/pool.js';
import { PacketServer } from './net/server.js';
import { handleStartupEncryption } from './net/startup-encryption.js';
import { PacketType } from './net/schema.js';
import { LoginHandlers } from './login/handlers.js';
import { WorldHandlers } from './world/handlers.js';
import { NpcRegistry } from './world/npcs.js';
import { loadTemplates } from './common/templates.js';
import { log } from './common/log.js';

const mode = process.argv[2] ?? 'all';
const config = loadConfig();
const db = new Databases(config.database);

async function main() {
  await db.check();
  const { templates, baseTemplates } = await loadTemplates(db);
  const servers = [];

  if (mode === 'login' || mode === 'all') {
    const handlers = new LoginHandlers({ db, templates, baseTemplates, config });
    await handlers.refreshWorldList();
    const server = new PacketServer('login', config.login);
    server.on(PacketType.STARTUP_ENCRYPTION, handleStartupEncryption);
    server.on(PacketType.WORLD, (s, p) => handlers.onWorld(s, p));
    server.on(PacketType.LOGIN, (s, p) => handlers.onLogin(s, p));
    await server.listen();
    servers.push(server);
    setInterval(() => handlers.refreshWorldList().catch((e) => log.warn(`liste des mondes : ${e.message}`)), 60_000).unref();
  }

  if (mode === 'world' || mode === 'all') {
    const npcs = new NpcRegistry(db, config.world.language);
    await npcs.load();
    const handlers = new WorldHandlers({ db, templates, config, npcs });
    const server = new PacketServer('world', config.world);
    server.on(PacketType.STARTUP_ENCRYPTION, handleStartupEncryption);
    server.on(PacketType.CHAR_MANAGER, (s, p) => handlers.onCharManager(s, p));
    server.on(PacketType.CHARACTER, (s, p) => handlers.onCharacter(s, p));
    server.on(PacketType.CHATTING, (s, p) => handlers.onChatting(s, p));
    server.on(PacketType.OPT_CHAR_MOVE, (s, p) => handlers.onCharMove(s, p));
    await server.listen();
    servers.push(server);
  }

  const shutdown = async () => {
    log.info('arrêt…');
    for (const s of servers) await s.close();
    await db.close();
    process.exit(0);
  };
  process.on('SIGINT', shutdown);
  process.on('SIGTERM', shutdown);
}

main().catch((err) => {
  log.error(err.stack || String(err));
  process.exit(1);
});
