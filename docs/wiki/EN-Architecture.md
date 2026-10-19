# Code architecture

ArchCore is written in **Rust**. A Cargo workspace holds two crates that build a single executable:
`archcore [login|world|all]` starts the server, the other subcommands are the tools (see
[[Tools|EN-Tools]]). The server runs on `tokio` (async networking) and `mysql_async` (MariaDB).

## Layout

```text
Cargo.toml                 workspace
crates/archcore-net/       protocol, independent of the server
  src/packet.rs            AuPacket codec: 19-byte header, flags, fields, mini-packets
  src/schema.rs            packet types, field schemas read from the client, operation codes
  src/blowfish.rs          standard Blowfish (little-endian words, like the client)
  src/framing.rs           encrypted A1/B1 … AF/BF frames and TCP stream splitting
  src/rc4.rs               RC4 as derived by CryptoAPI (MD5 of the text key)
  src/text.rs              protocol text in Windows-1252
crates/archcore/           server and tools
  src/main.rs              entry point and subcommand dispatch
  src/server.rs            loads config, templates, NPCs, starts the servers
  src/net.rs               one task per connection: encryption, 0x48 handshake, dispatch
  src/ext.rs               ArchRust extensions: HELLO / HELLO_ACK negotiation (type 0xAC)
  src/login.rs             accounts, world list, characters, world entry
  src/character.rs         templates, `characters` rows, factors, AGPMCHARACTER packet
  src/config.rs, db.rs, log.rs
  src/world/               world server
    mod.rs                 shared state, world entry, GM command actions
    presence.rs            movement, player visibility, position saving
    npcs.rs                NPCs (npc_spawn), npc.ini functions, view packet, title translation
    npc_events.rs          NPC dialog
    npc_trade.rs           shops: list, buy, sell
    monsters.rs            creatures (monster_spawn): placement, stats, life cycle, view packet
    combat.rs              attacking monsters: approach, hits, death, experience, respawn; player death and resurrection
    ai.rs                  monster AI: attackers, retaliation, pursuit, going home
    loot.rs                drops: rolls (gold, groups, own items), ground items, pick-up
    equipment.rs           equipment: restrictions, replacement, factors, moving items in the inventory
    recovery.rs            life and mana potions, out-of-combat regeneration
    map.rs                 server map (map_segment): blocked ground, regions
    items.rs               item catalog, inventory, AGPMITEM packets
    chat.rs, gm.rs         chat and GM commands
    addon.rs               AddOn messages of the ArchRust extensions
  src/tools/               tools: db_init, decrypt, import, i18n, sim, re
sql/                       schemas of the three databases
data/i18n/                 translation memory
data/re/                   reverse engineering findings (packet schemas)
```

## Life of a packet

1. `net.rs` accepts the connection and starts a task that reads the TCP stream.
2. `framing.rs` splits the stream into frames and decrypts them once the handshake is done; `net.rs`
   handles the handshake itself (type 0x48).
3. `packet.rs` decodes the header and fields with the schema from `schema.rs`.
4. The service connection calls the right handler, for example `login.rs` for AGPMLOGIN.
5. The handler answers with `session.send(&schema, fields)`, which encodes, encrypts and sends.

Packets of one connection are handled one at a time, in arrival order. On the world side, the shared
state (players in game, mutes, rates) is behind a lock: packets from all players are handled one after
another.

## Two servers

- **Login (11002)**: handshake, world list, authentication (SHA-256 in the database), character
  list, creation and deletion. On world entry, it hands out an authentication key and the world
  server address.
- **World (11008)**: checks the key, sends the character, its inventory, its position, the game time
  and nearby NPCs, then handles movement, chat, GM commands, dialogs and shops.

The details of the exchanges are on the [[Network protocol|EN-Network-protocol]] page.

## Tests

```bash
cargo test
```

The tests cover the packet codec (against reference bytes), Blowfish and RC4, frame splitting, the
configuration, NPC functions, the shop, the inventory, GM commands, the ArchRust extensions and the import and translation
tools. Full scenarios are replayed with `archcore sim`.
