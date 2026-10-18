# Code architecture

ArchCore is a single Node.js program (ES modules) that can run the login server, the world server,
or both: `node src/index.js [login|world|all]`. Its only dependency is the `mariadb` driver.

## Layout

```text
src/
  index.js            entry point: loads config and templates, starts the servers
  net/                shared network layer
    blowfish.js       standard Blowfish (little-endian words)
    packet.js         AuPacket codec: 19-byte header, fields, mini-packets
    framing.js        encrypted A1/B1 … AF/BF frames and TCP stream splitting
    schema.js         packet types, field schemas, operation codes
    session.js        one connection: encryption, sending, state
    server.js         TCP server and routing by packet type
    startup-encryption.js  handshake (type 0x48)
  login/handlers.js   accounts, world list, characters, world entry
  world/handlers.js   game entry, keepalive, sending the character and NPCs
  world/npcs.js       NPCs loaded from npc_spawn, distance lookup, view packet
  common/             config, logging, CryptoAPI RC4, character packets, templates
  db/pool.js          one MariaDB pool per database
sql/                  schemas of the three databases
tools/                offline tools (see [[Tools|EN-Tools]])
test/                 protocol tests (node --test)
data/i18n/            translation memory
data/re/              reverse engineering notes (packet schemas)
```

## How a packet flows

1. `server.js` accepts the connection and creates a `Session`.
2. `framing.js` splits the TCP stream into frames and decrypts them once the handshake is done.
3. `packet.js` decodes the header and fields using the schema in `schema.js`.
4. `server.js` calls the handler registered for that packet type, for example `LoginHandlers.onLogin`.
5. The handler replies with `session.send(schema, values)`, which encodes, encrypts and sends.

## Two servers

- **Login (11002)**: handshake, world list, authentication (SHA-256 in the database), character
  list, creation and deletion. On world entry it hands out an authentication key and the world
  server address.
- **World (11008)**: checks the key, sends the character, its position, the game time and nearby
  NPCs, then confirms the game entry.

The full exchange is described on the [[Network protocol|EN-Network-protocol]] page.

## Tests

```bash
npm test
```

The tests cover the packet codec, Blowfish, frame splitting, the 4-byte flags of type 3 and the
NPC spawn packet.
