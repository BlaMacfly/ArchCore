// Serveur TCP générique : accepte les connexions et crée une ClientSession par socket.

import net from 'node:net';
import { ClientSession } from './session.js';
import { log } from '../common/log.js';

export class PacketServer {
  /**
   * @param {string} name nom affiché dans les journaux
   * @param {{bind:string, port:number}} opts
   */
  constructor(name, opts) {
    this.name = name;
    this.opts = opts;
    this.handlers = new Map();
    this.sessions = new Set();
    this.server = net.createServer({ noDelay: true }, (socket) => this.#accept(socket));
  }

  /** Associe un type de paquet à un gestionnaire async (session, packet, plain) => void. */
  on(packetType, handler) {
    this.handlers.set(packetType, handler);
    return this;
  }

  #accept(socket) {
    socket.setNoDelay(true);
    const session = new ClientSession(socket, this);
    this.sessions.add(session);
    session.on('close', () => this.sessions.delete(session));
    this.emitConnect?.(session);
  }

  listen() {
    return new Promise((resolve, reject) => {
      this.server.once('error', reject);
      this.server.listen(this.opts.port, this.opts.bind, () => {
        this.server.off('error', reject);
        log.info(`[${this.name}] en écoute sur ${this.opts.bind}:${this.opts.port}`);
        resolve();
      });
    });
  }

  async close() {
    for (const s of this.sessions) s.close();
    await new Promise((resolve) => this.server.close(resolve));
  }
}
