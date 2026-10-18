// Session client : enveloppe une socket TCP, découpe les trames, gère le chiffrement
// et dispatche les paquets décodés vers des gestionnaires par type.

import { EventEmitter } from 'node:events';
import { CryptoSession, FrameSplitter } from './framing.js';
import { MainById } from './schema.js';
import { buildPacket, decodePacket, hexDump, parseHeader } from './packet.js';
import { log } from '../common/log.js';

let nextSessionId = 1;

export class ClientSession extends EventEmitter {
  /**
   * @param {import('node:net').Socket} socket
   * @param {{name:string, handlers:Map<number, Function>}} server
   */
  constructor(socket, server) {
    super();
    this.id = nextSessionId++;
    this.socket = socket;
    this.server = server;
    this.remote = `${socket.remoteAddress}:${socket.remotePort}`;
    this.crypto = new CryptoSession();
    this.splitter = new FrameSplitter();
    this.state = {}; // état libre pour les gestionnaires (compte, personnage…)
    this.closed = false;
    this.pending = Promise.resolve(); // file de traitement des paquets reçus

    socket.on('data', (chunk) => this.#onData(chunk));
    socket.on('error', (err) => log.warn(`[${server.name}] #${this.id} erreur socket : ${err.message}`));
    socket.on('close', () => {
      this.closed = true;
      log.info(`[${server.name}] #${this.id} déconnecté (${this.remote})`);
      this.emit('close');
    });
    log.info(`[${server.name}] #${this.id} connecté depuis ${this.remote}`);
  }

  #onData(chunk) {
    let frames;
    try {
      frames = this.splitter.push(chunk);
    } catch (err) {
      log.warn(`[${this.server.name}] #${this.id} flux invalide : ${err.message} — ${hexDump(chunk)}`);
      this.close();
      return;
    }
    // Un paquet à la fois, dans l'ordre d'arrivée : les gestionnaires asynchrones (base de données)
    // ne doivent pas se doubler (réponses MJ dans le désordre, ENTER_WORLD avant la fin de GAME_ENTER).
    for (const frame of frames) {
      this.pending = this.pending.then(() => this.#onFrame(frame)).catch((err) => {
        log.error(`[${this.server.name}] #${this.id} gestionnaire : ${err.stack || err}`);
      });
    }
  }

  async #onFrame({ encrypted, data }) {
    let plain = data;
    if (encrypted) {
      try {
        plain = this.crypto.decodeFrame(data);
      } catch (err) {
        log.warn(`[${this.server.name}] #${this.id} déchiffrement : ${err.message} — ${hexDump(data)}`);
        return;
      }
    }
    const header = parseHeader(plain);
    if (!header) {
      log.warn(`[${this.server.name}] #${this.id} paquet sans en-tête valide — ${hexDump(plain)}`);
      return;
    }
    const schema = MainById[header.type];
    const handler = this.server.handlers.get(header.type);
    if (!schema || !handler) {
      log.warn(`[${this.server.name}] #${this.id} type 0x${header.type.toString(16)} non géré (${plain.length} o) — ${hexDump(plain)}`);
      return;
    }
    let packet;
    try {
      packet = decodePacket(schema, plain);
    } catch (err) {
      log.warn(`[${this.server.name}] #${this.id} décodage ${schema.name} : ${err.message} — ${hexDump(plain)}`);
      return;
    }
    if (log.trace) log.debug(`[${this.server.name}] #${this.id} ← ${schema.name} op=${packet.values[0]} flags=0x${packet.flags.toString(16)}`);
    await handler(this, packet, plain);
  }

  /** Envoie un paquet principal déjà construit (chiffré si la session est prête). */
  sendRaw(plain, { encrypt = this.crypto.ready } = {}) {
    if (this.closed) return;
    const out = encrypt ? this.crypto.encodeFrame(plain) : plain;
    if (log.trace) {
      const h = parseHeader(plain);
      log.debug(`[${this.server.name}] #${this.id} → type=0x${h.type.toString(16)} ${plain.length} o${encrypt ? ' (chiffré)' : ''}`);
    }
    this.socket.write(out);
  }

  /** Construit et envoie un paquet principal selon un schéma. */
  send(schema, values, opts = {}) {
    this.sendRaw(buildPacket(schema, values, opts), opts);
  }

  close() {
    if (!this.closed) this.socket.destroy();
  }
}
