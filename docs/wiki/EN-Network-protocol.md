# Network protocol

The 64-bit ArchLord client is a rebuild of the original client. Its protocol follows the original
32-bit client, with wider lengths. Everything below was found in the client executable and checked
against the real client.

## AuPacket

| Bytes | Content |
|---|---|
| 0 | `D6`, start marker |
| 1–4 | total size (u32) |
| 5 | packet type (u8) |
| 6 | header flags (u8) |
| 7–10 | owner (u32) |
| 11–18 | timestamp (8 bytes) |
| 19… | field flags (1, 2 or 4 bytes depending on the module), then the present fields |
| end | `6B`, end marker |

- A bit set in the field flags means the field is present.
- Mini-packets, memory blocks and lists are prefixed with their length as a **u32**
  (u16 in the 32-bit client).
- Type 3 (`AgcmCharacter`) uses **4-byte** field flags.

## Encrypted frames

After the handshake, every packet is wrapped:

```text
A1 (server) / B1 (client) | size u32 | counter u32 | Blowfish ECB data | AF (server) / BF (client)
```

Blowfish works on little-endian 32-bit words, and data is zero-padded to a multiple of 8 bytes.

## Handshake (type 0x48)

1. The client sends `REQ`.
2. The server answers `SET_ALGO`, then `PUBLIC_KEY` (32 bytes).
3. The client sends `PRIVATE_KEY`: its own key, encrypted with the server's.
4. The server confirms with `COMPLETE`. Everything after that uses the client's key.

## Login (type 0x0D, AgpmLogin)

| Op | Direction | Purpose |
|---|---|---|
| 0 | ↔ | Request and delivery of an 8-character key that encrypts the password (CryptoAPI RC4) |
| 1 | ↔ | User name and password (65 bytes), then the answer |
| 3 | ↔ | World choice; the answer carries the current race (field 4 of the `CHAR_INFO` mini) |
| 4, 5 | → | Character names, then end of list |
| 6, 7 | ↔ | List request; one character packet (type 2) per character, then end |
| 10 | ↔ | Creation screen: base models, then confirmation |
| 11, 12 | ↔ | Character creation |
| 13 | ↔ | Character deletion |
| 8 | ↔ | World entry: CID in field 6, `CHAR_INFO` in field 8, world address in field 9 |
| 15 | → | Error result (wrong password, name taken…) |

The race sent with op 3 matters: the client's "Create" button only sends op 10 for race 1 or 2.
With 0, the creation screen keeps loading forever. Creation-screen models get their own CIDs,
starting at `0x7F000000`, so they never collide with a real character.

The world list goes through type 0x18 (AgpmWorld), formatted as `Name=Group=0=Status=Flag;…`.

## World entry (type 0x03, AgcmCharacter)

1. The client sends **op 2**: name in field 3, authentication key in field 6.
2. The server sends the configuration, the character, then **op 8** with the position.
3. The client answers **op 9** with its CID.
4. The server sends nearby NPCs, then **op 7**: the player is in game.
5. The client sends **op 13** every 15 seconds as a keepalive.

## NPC spawning (type 0x42, AgpmOptimizedView)

Op 1 carries, in field 1, a 26-field "character view" mini-packet: CID, TID, name, position,
destination, orientation, and so on. ArchCore gives NPCs CIDs starting at `0x40000000`.

## Chat (type 0x0F, AgpmChatting)

1-byte field flags: op, sender CID, channel type, target CID, sender name (49), target name (49),
message (memory block).

- The client sends op 0 with its CID, the type (0 normal, 5 whisper with the target name) and the
  message **without a trailing zero**, under 256 bytes. A GM command is a normal message starting
  with `/`.
- If the player picked a chat color, the client appends `\x01RRGGBB` to the message. ArchCore strips
  it before parsing a command and keeps it for display.
- On receipt, the client rejects a message over 512 bytes. Type 7 (system message) is shown without
  a sender; ArchCore uses it to answer the GM, and type 10 for announcements.

## GM level (type 0x12, AgpmAdmin)

2-byte field flags: op, CID, then 13 mini-packets. Op 0 with, in field 2, a "GM data" mini-packet of
op 1 (CID, name, GM level as INT16, account name) records the character's level. The CID must be a
character the client already knows: ArchCore sends it right after the world-entry character packet.

## Known types

| Type | Module | Type | Module |
|---|---|---|---|
| 0x00 | System | 0x15 | Guild |
| 0x01 | Configuration | 0x18 | Worlds |
| 0x02 | Character | 0x2F | Return to login |
| 0x03 | Character manager | 0x3C | Events (NPC functions) |
| 0x04 | Items | 0x40 | Movement |
| 0x05 | Skills | 0x41 | Actions |
| 0x08 | Party | 0x42 | Optimized view |
| 0x0D | Login | 0x44 | System message |
| 0x0F | Chat | 0x48 | Startup encryption |
| 0x11 | Timer | 0x60 | Titles |
| 0x13 | Quests | 0x63 | Reconnect |

Full schemas for the 86 modules are in `data/re/packet-schemas.json`.
