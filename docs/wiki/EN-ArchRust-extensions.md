# ArchRust ↔ ArchCore extensions

> **Specification, version 1.0 — draft.** Nothing is implemented yet. This document is the reference for
> both projects: the [ArchRust](https://github.com/BlaMacfly/ArchRust) client and the ArchCore server.

ArchRust, the client rewritten in Rust, supports **Lua AddOns** modelled on World of Warcraft. Some AddOns
need the server: to exchange data between players, or to query the server. This document defines an
**extension protocol** that sits next to the ArchLord protocol without changing it. Version 1 covers two
features:

1. **capability negotiation**: client and server agree on the extensions they use;
2. the **AddOn message channel**: the equivalent of WoW's `C_ChatInfo.SendAddonMessage` and
   `CHAT_MSG_ADDON` event.

## 1. Principles

- **Full compatibility with the original client.** The server never sends an extension packet to a
  connection that did not negotiate extensions. The original client keeps working and sees nothing.
- **Compatibility with other servers.** ArchRust must stay usable on a server without extensions: with no
  answer to its request, it falls back to classic mode.
- **The server is authoritative.** It fills in the sender of a message itself, enforces size and rate
  limits, and refuses what is not allowed. Nothing the client declares is taken at face value.
- **Versioned and extensible.** Each feature is a named, versioned capability. An unknown capability is
  ignored, never treated as an error.

## 2. Transport

Extensions use a **dedicated AuPacket type: `0xAC`** ("ArchCore"). It has the same format as every other
packet (19-byte header, flags, fields, `0x6B` footer). It is Blowfish-encrypted like the rest of the
session and travels over the usual connections (login 11002, world 11008).

Why `0xAC`: the original client's packet dispatcher (`FUN_14012c930`) only accepts types `0x01` to `0x77`
and silently ignores all others. A type above `0x77` therefore cannot collide with any current or future
module of the original client. Even if received by mistake, it would be ignored.

### `ARCHCORE_EXT` schema (type `0xAC`, 4-byte flags)

| # | Type | Name | Role |
|---|---|---|---|
| 0 | UINT8 | `op` | operation (section 3) |
| 1 | UINT32 | `request_id` | number chosen by the client, echoed in the answer or error |
| 2 | UINT16 | `version_major` | extension protocol major version |
| 3 | UINT16 | `version_minor` | minor version |
| 4 | MEMORY_BLOCK | `software` | software name and version, UTF-8, at most 64 bytes (`ArchRust 0.3.0`) |
| 5 | MEMORY_BLOCK | `capabilities` | list of `name/version` capabilities separated by `;`, ASCII |
| 6 | UINT16 | `max_payload` | maximum size of an AddOn message payload, in bytes |
| 7 | UINT16 | `rate_burst` | rate limit: messages allowed at once |
| 8 | UINT16 | `rate_per_sec` | rate limit: messages refilled per second |
| 9 | MEMORY_BLOCK | `prefix` | AddOn prefix (section 5) |
| 10 | UINT8 | `channel` | message scope (section 5) |
| 11 | MEMORY_BLOCK | `target` | recipient: character name (Windows-1252) |
| 12 | INT32 | `sender_cid` | sender CID, filled in by the server |
| 13 | MEMORY_BLOCK | `sender_name` | sender name (Windows-1252), filled in by the server |
| 14 | MEMORY_BLOCK | `payload` | message payload, arbitrary bytes |
| 15 | UINT8 | `result` | result code (section 6) |
| 16–31 | — | — | reserved for future versions |

Decoding rules:
- a missing field (flag bit 0) takes its default value: 0 or empty;
- an empty memory block does not set its flag, so an empty payload is a missing field 14;
- a reserved field that is present is ignored, which lets future versions add fields.

## 3. Operations

| `op` | Name | Direction | Fields |
|---|---|---|---|
| 0 | `HELLO` | client → server | 2, 3, 4, 5 |
| 1 | `HELLO_ACK` | server → client | 2, 3, 4, 5, 6, 7, 8 |
| 2 | `ADDON_SEND` | client → server | 1 (optional), 9, 10, 11 (depending on scope), 14 |
| 3 | `ADDON_RECV` | server → client | 9, 10, 11 (whisper), 12, 13, 14 |
| 4 | `RESULT` | server → client | 1, 15 |

An unknown operation is ignored and logged by its receiver.

## 4. Capability negotiation

### Flow

Negotiation happens **on each connection** (login, then world) independently, right after the encrypted
handshake (type `0x48`) and before any other packet:

```text
client                                   server
  ── 0x48 handshake (unchanged) ─────────▶
  ◀───────────────────── 0x48 COMPLETE ──
  ── 0xAC HELLO (versions, software, requested capabilities) ──▶
  ◀── 0xAC HELLO_ACK (versions, software, granted capabilities, limits) ──
  ── normal protocol flow (AGPMWORLD, GAME_ENTER…) ──▶
```

- The client sends **a single** `HELLO` per connection. A second `HELLO` is ignored.
- The server answers `HELLO_ACK` with the **intersection** of the requested capabilities and those it
  supports. A capability is granted at the highest version both sides know.
- The server **never** sends a `0xAC` packet to a connection that did not send `HELLO`. Original clients,
  which never send one, therefore never see an extension.
- If the client gets no `HELLO_ACK` within **3 seconds**, it treats the server as having no extensions and
  carries on. It may also carry on without waiting: game packets received meanwhile stay valid.
- Extensions last for the connection. On the world server, `HELLO` may arrive before `GAME_ENTER`: the
  agreement is then attached to the character as soon as it enters the world.

### Versions

- **Protocol version** (`version_major.version_minor`), currently **1.0**. A minor version only adds
  optional fields or operations. A different major version makes both sides incompatible: the server then
  answers `HELLO_ACK` with an empty capability list.
- **Capability version**: an integer after the name (`addon-msg/1`), raised when the capability's
  behaviour changes.

### Capabilities

| Capability | Version 1 | Role |
|---|---|---|
| `addon-msg` | defined here | AddOn message channel |
| `addon-data` | reserved | server data queries from AddOns (like WoW's `C_*` APIs) |
| `saved-vars` | reserved | `SavedVariables` kept on the server, shared across machines |
| `server-script` | reserved | AddOn interfaces driven by server-side scripts |

Example `HELLO`: `capabilities = "addon-msg/1;saved-vars/1"`. If the server only supports `addon-msg/1`, it
answers `capabilities = "addon-msg/1"` with its limits.

## 5. AddOn message channel (`addon-msg/1`)

### Prefix

- 1 to 16 printable ASCII bytes (`0x21` to `0x7E`), no space. Case-sensitive.
- Prefixes starting with `AC.` are **reserved for the server**: a client sending one gets `NOT_ALLOWED`.
- The server relays every valid prefix. The client only delivers to an AddOn the prefixes it registered
  (like `C_ChatInfo.RegisterAddonMessagePrefix` in WoW).

### Payload

- 0 to `max_payload` bytes (**255** in version 1, as in WoW). Content is free, binary included.
- The server neither reads nor changes the payload, and does not log it: only the sender, prefix, scope and
  size may be logged.

### Scopes (`channel`)

| Value | Scope | Recipients | Version 1 |
|---|---|---|---|
| 1 | `WHISPER` | the character named in `target` | ✅ |
| 2 | `SAY` | players within the sender's view range, sender included | ✅ |
| 3 | `PARTY` | the sender's party, sender included | answers `NOT_AVAILABLE` until parties exist |
| 4 | `GUILD` | the sender's guild, sender included | same, until guilds exist |
| 5 | `RAID` | reserved | `NOT_AVAILABLE` |
| 6 | `CHANNEL` | reserved (named channels) | `NOT_AVAILABLE` |
| 7 | `SERVER` | the server itself, for future capabilities (`addon-data`, `server-script`) | `NOT_AVAILABLE` in version 1 |

Only connections granted `addon-msg` receive `ADDON_RECV`. A player on the original client never gets one:
a whisper addressed to them returns `TARGET_NO_EXTENSION`.

### Relay

On an `ADDON_SEND`, the server checks, in order:

1. that the connection was granted `addon-msg` and the player is in the world (otherwise the packet is ignored);
2. the prefix;
3. the payload size;
4. the rate limit;
5. the scope and recipient.

It then sends an `ADDON_RECV` to each recipient, filling in `sender_cid` and `sender_name` itself. For a
whisper, the `target` field of `ADDON_RECV` holds the recipient's name.

### Rate limit

- One **token bucket** per connection: at most `rate_burst` tokens (**20** in version 1), refilled at
  `rate_per_sec` tokens per second (**10** in version 1). Each `ADDON_SEND` costs one token.
- With no token left, the message is dropped and the server answers `THROTTLED`.
- The values are announced in `HELLO_ACK`: the client should smooth its sends itself, ChatThrottleLib
  style, rather than rely on server refusals.

## 6. Results (`RESULT`)

To limit traffic, the server **does not confirm** a successful send: it only sends `RESULT` on error, with
the refused message's `request_id` (0 if the client gave none).

| `result` | Name | Cause |
|---|---|---|
| 0 | `OK` | reserved (not sent in version 1) |
| 1 | `INVALID_PREFIX` | empty or too long prefix, or forbidden characters |
| 2 | `MESSAGE_TOO_LONG` | payload larger than `max_payload` |
| 3 | `THROTTLED` | rate limit reached |
| 4 | `TARGET_OFFLINE` | whisper recipient not found or offline |
| 5 | `NOT_AVAILABLE` | scope not supported by this server |
| 6 | `NOT_IN_GROUP` | `PARTY`/`GUILD` scope without a party or guild |
| 7 | `NOT_ALLOWED` | reserved prefix, or the player may not send |
| 8 | `TARGET_NO_EXTENSION` | the recipient lacks the `addon-msg` capability |

## 7. ArchRust side: proposed Lua API

It reuses WoW's names so existing AddOns are easy to adapt:

```lua
C_ChatInfo.RegisterAddonMessagePrefix(prefix)      -- true if accepted (at most 128 prefixes)
C_ChatInfo.IsAddonMessagePrefixRegistered(prefix)  -- boolean
C_ChatInfo.GetRegisteredAddonMessagePrefixes()     -- list
C_ChatInfo.SendAddonMessage(prefix, text, chatType, target)
  -- chatType: "WHISPER", "SAY", "PARTY", "GUILD"; returns a code (Enum.SendAddonMessageResult)
C_ArchCore.IsAvailable()                           -- extensions negotiated with this server?
C_ArchCore.GetCapabilities()                       -- { ["addon-msg"] = 1, … }
C_ArchCore.GetLimits()                             -- { maxPayload = 255, burst = 20, perSecond = 10 }
```

Events:
- `CHAT_MSG_ADDON(prefix, text, channel, sender, target)`, delivered only for registered prefixes;
- `ARCHCORE_EXTENSIONS_READY(serverSoftware, major, minor)`, once `HELLO_ACK` arrives on the world server;
- `ARCHCORE_ADDON_MESSAGE_FAILED(prefix, result)`, when a `RESULT` arrives.

`SendAddonMessage` returns success as soon as the message is queued. The client applies the announced rate
limit before sending, and returns `AddonMessageThrottle` if it has to refuse.

## 8. Security and privacy

- The server trusts no identity field sent by the client: the sender is always the connection's character.
- `AC.` prefixes let the server talk to official AddOns without spoofing risk.
- Payloads are neither read, stored nor logged by the server.
- A chat mute (`/chatban`) does not block AddOn messages in version 1: they are not visible as text. This
  choice is open (section 10).
- Repeated rate-limit abuse is logged so GMs can step in.

## 9. Planned implementation

**ArchCore**
- `ARCHCORE_EXT` schema and constants in `crates/archcore-net/src/schema.rs`.
- `HELLO` handling in `net.rs`, next to the `0x48` handshake, for both login and world, with the agreement
  kept in the session.
- `crates/archcore/src/world/addon.rs` module: checks, token bucket, relay.
- Unit tests, and an `archcore sim addon` scenario: two simulated clients exchange whispers and `SAY`
  messages, and a simulated original client receives none.

**ArchRust**
- Send `HELLO` after the handshake, wait for `HELLO_ACK` (3 s at most).
- Lua API and events from section 7 in `archrust-addons`.
- Send smoothing according to the announced limits.

**Shared code.** Both projects currently describe the protocol separately (`archcore-net`,
`archrust-protocol`). For the extension, a single source is better: ArchRust could depend on the
`archcore-net` crate (git dependency), which would carry the `0xAC` schema. This requires compatible
licences (ArchRust is GPL-3.0).

## 10. Open questions

1. **ArchCore's licence**, a prerequisite for sharing `archcore-net` with ArchRust.
2. **`/chatban` and AddOns**: should a muted player's AddOn messages be blocked too?
3. **`SAY` scope**: full view range (8000 units) or a shorter speech distance?
4. **Messages to the server (`SERVER`)**: first intended use — detailed stats, quests, world state?
5. **Official AddOns**: should the server be able to announce or offer AddOns (list, versions)?
