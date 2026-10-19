# ArchRust ↔ ArchCore extensions

> **Specification, version 1.1 — approved on 2026-10-19** (decisions in section 11). Both sides are
> implemented (section 10). This document is the reference for both projects: the
> [ArchRust](https://github.com/BlaMacfly/ArchRust) client and the ArchCore server.

ArchRust, the client rewritten in Rust, supports **Lua AddOns** modelled on World of Warcraft. Some AddOns
need the server: to exchange data between players, or to query the server. This document defines an
**extension protocol** that sits next to the ArchLord protocol without changing it. Version 1 covers three
features:

1. **capability negotiation**: client and server agree on the extensions they use;
2. the **AddOn message channel**: the equivalent of WoW's `C_ChatInfo.SendAddonMessage` and
   `CHAT_MSG_ADDON` event;
3. since version 1.1, the **client language**: the server serves its texts (NPC, monster, quest and item
   names…) in the language the player chose (section 6).

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
| 15 | UINT8 | `result` | result code (section 7) |
| 16 | MEMORY_BLOCK | `language` | language, BCP 47 code in ASCII (`fr`, `pt-BR`, at most 16 bytes): wished by the client, applied by the server — v1.1 |
| 17 | MEMORY_BLOCK | `languages` | languages offered by the server, separated by `;` — v1.1 |
| 18 | MEMORY_BLOCK | `text_kind` | kind of text requested, ASCII (section 6) — v1.1 |
| 19 | MEMORY_BLOCK | `text_ids` | requested identifiers: little-endian UINT32s back to back — v1.1 |
| 20 | MEMORY_BLOCK | `texts` | returned texts: for each, UINT32 identifier, UINT16 length, UTF-8 — v1.1 |
| 21–31 | — | — | reserved for future versions |

Decoding rules:
- a missing field (flag bit 0) takes its default value: 0 or empty;
- an empty memory block does not set its flag, so an empty payload is a missing field 14;
- a reserved field that is present is ignored, which lets future versions add fields.

## 3. Operations

| `op` | Name | Direction | Fields |
|---|---|---|---|
| 0 | `HELLO` | client → server | 2, 3, 4, 5, 16 (optional, v1.1) |
| 1 | `HELLO_ACK` | server → client | 2, 3, 4, 5, 6, 7, 8, 16 and 17 (when `language` is granted) |
| 2 | `ADDON_SEND` | client → server | 1 (optional), 9, 10, 11 (depending on scope), 14 |
| 3 | `ADDON_RECV` | server → client | 9, 10, 11 (whisper), 12, 13, 14 |
| 4 | `RESULT` | server → client | 1, 15 |
| 5 | `SET_LANGUAGE` | client → server | 1 (optional), 16 — v1.1 |
| 6 | `LANGUAGE_ACK` | server → client | 1, 16 (applied language), 17 — v1.1 |
| 7 | `TEXT_REQUEST` | client → server | 1 (optional), 18, 19 — v1.1 |
| 8 | `TEXT_REPLY` | server → client | 1, 16 (language served), 18, 20 — v1.1 |

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

- **Protocol version** (`version_major.version_minor`), currently **1.1** (1.1 adds fields 16 to 20,
  operations 5 to 8 and result 9; a 1.0 client or server ignores them). A minor version only adds
  optional fields or operations. A different major version makes both sides incompatible: the server then
  answers `HELLO_ACK` with an empty capability list.
- **Capability version**: an integer after the name (`addon-msg/1`), raised when the capability's
  behaviour changes.

### Capabilities

| Capability | Version 1 | Role |
|---|---|---|
| `addon-msg` | defined here | AddOn message channel |
| `language` | defined here (v1.1, section 6) | client language and localized server texts |
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
| 2 | `SAY` | players within the sender's chat range, sender included: the same radius as normal chat (`World.ViewRadius`, 8000 units by default) | ✅ |
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
2. that the player is not chat-muted (otherwise `NOT_ALLOWED`);
3. the prefix;
4. the payload size;
5. the rate limit;
6. the scope and recipient.

It then sends an `ADDON_RECV` to each recipient, filling in `sender_cid` and `sender_name` itself. For a
whisper, the `target` field of `ADDON_RECV` holds the recipient's name.

### Rate limit

- One **token bucket** per connection: at most `rate_burst` tokens (**20** in version 1), refilled at
  `rate_per_sec` tokens per second (**10** in version 1). Each `ADDON_SEND` costs one token.
- With no token left, the message is dropped and the server answers `THROTTLED`.
- The values are announced in `HELLO_ACK`: the client should smooth its sends itself, ChatThrottleLib
  style, rather than rely on server refusals.

## 6. Language and localized texts (`language/1`)

The ArchLord protocol does not carry the player's language: the original client reads its texts from its
language folder (`Game/ini`, `Game/ini_fr`…) and the server knows nothing about that choice. The texts the
server sends itself (NPC and monster names in view packets, and later quests, items…) are therefore in a
single language, that of `World.Language`. The `language` capability fixes this.

### Connection language

- The client attaches its language to `HELLO` (field 16): a **BCP 47** code (ISO 639-1, optional region:
  `fr`, `de`, `pt-BR`, `zh-CN`). The server normalizes it (`FR-ca` → `fr-CA`).
- `HELLO_ACK` returns the **applied language** in field 16 and the **languages offered** by the server in
  field 17 (`language` table of the world database, see [[Translation|EN-Translation]]). The applied
  language is the requested code when offered, otherwise its base language (`fr-CA` → `fr`), otherwise the
  server's default (`World.Language`).
- `SET_LANGUAGE` (field 16) changes the language during the connection, on login as on world; the server
  answers `LANGUAGE_ACK` (applied language, offered languages). A malformed code gets `RESULT`
  `INVALID_REQUEST`; an unknown language is not an error: the server applies its default and says so in
  `LANGUAGE_ACK`.
- The wish **follows the account**: the server stores it (`account.language`) at authentication and on every
  `SET_LANGUAGE`. A connection without a language (original client, ArchRust set to "Automatic") gets the
  last language stored on the account, otherwise `World.Language`. Since the world `HELLO` comes before
  `GAME_ENTER`, its `HELLO_ACK` can only announce the default language: when the account's language ends up
  applying, the world server sends a `LANGUAGE_ACK` (field 1 set to 0) on world entry with the language
  actually applied.
- Texts already sent are not re-sent: a change made in game applies to the NPCs and monsters that appear
  afterwards, and to the following `TEXT_REQUEST`s.

### Text fallback

For every text, the server looks, in order, for: the `localized_text` row in the applied language, in its
base language, the NPC title translation (`data/i18n/npc-titles-<language>.json`, NPC and monster names
only), the English row, and finally the client's original text. A text it does not know at all is returned
empty.

### Texts on demand (`TEXT_REQUEST` → `TEXT_REPLY`)

The client asks for texts by **kind** (field 18) and **identifiers** (field 19, 1 to 64 UINT32s); the server
returns every requested identifier, in order, with its text in the applied language (fields 16, 18, 20).
Version 1 kinds (same names as the `kind` column of `localized_text`):

| `text_kind` | Identifier | Original text |
|---|---|---|
| `npc_name` | `npc_spawn.id` (NPC CID − `0x40000000`) | `npc.ini` |
| `monster_name` | monster TID | `CharacterTemplatePublic.ini` |
| `item_name`, `item_desc` | item TID | `itemdatatable.txt` |
| `quest_name`, `quest_desc` | quest TID | `questtemplate.ini` |
| `skill_name` | skill TID | `SKILLTEMPLATE.ini` |
| `region_name` | region id | `RegionTemplate.ini` |
| `npc_dialog` | reserved | — |

An unknown kind, an empty list or one of more than 64 identifiers, or a request without the `language`
capability, get `RESULT` `INVALID_REQUEST`. On the login connection, `TEXT_REQUEST` answers
`NOT_AVAILABLE`: texts are requested from the world server.

## 7. Results (`RESULT`)

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
| 7 | `NOT_ALLOWED` | reserved prefix, or the player is chat-muted (`/chatban`) |
| 8 | `TARGET_NO_EXTENSION` | the recipient lacks the `addon-msg` capability |
| 9 | `INVALID_REQUEST` | v1.1: malformed language code, unknown text kind, too many identifiers, or `language` capability not granted |

## 8. ArchRust side: Lua API

It reuses WoW's names so existing AddOns are easy to adapt:

```lua
C_ChatInfo.RegisterAddonMessagePrefix(prefix)      -- true if accepted (at most 128 prefixes)
C_ChatInfo.IsAddonMessagePrefixRegistered(prefix)  -- boolean
C_ChatInfo.GetRegisteredAddonMessagePrefixes()     -- list
C_ChatInfo.SendAddonMessage(prefix, text, chatType, target)
  -- chatType: "WHISPER", "SAY", "PARTY", "GUILD"; returns a code (Enum.SendAddonMessageResult)
C_ArchCore.IsAvailable()                           -- extensions negotiated with this server?
C_ArchCore.GetCapabilities()                       -- { ["addon-msg"] = 1, ["language"] = 1, … }
C_ArchCore.GetLimits()                             -- { maxPayload = 255, burst = 20, perSecond = 10 }
C_ArchCore.GetLanguage()                           -- language applied by the server ("fr"), nil without the capability
C_ArchCore.GetLanguages()                          -- offered languages: { "en", "fr", "de", … }
C_ArchCore.RequestTexts(kind, ids)                 -- text request (section 6); returns the request id, nil if refused
```

Events:
- `CHAT_MSG_ADDON(prefix, text, channel, sender, target)`, delivered only for registered prefixes;
- `ARCHCORE_EXTENSIONS_READY(serverSoftware, major, minor)`, once `HELLO_ACK` arrives on the world server;
- `ARCHCORE_ADDON_MESSAGE_FAILED(prefix, result, resultName, requestId)`, when a `RESULT` arrives (for a
  text request, `prefix` is `<texts:kind>`);
- `ARCHCORE_LANGUAGE_CHANGED(language, languages)`, after a `LANGUAGE_ACK` (languages separated by `;`);
- `ARCHCORE_TEXTS(requestId, language, kind, texts)`, when a `TEXT_REPLY` arrives; `texts` is an
  identifier → text table.

`SendAddonMessage` returns success as soon as the message is queued. The client applies the announced rate
limit before sending, and returns `AddonMessageThrottle` if it has to refuse.

On the interface side, ArchRust's **Options** window (the login screen's "Options" button, or Escape in
game) offers the language among those of the [[Translation|EN-Translation]] page, or "Automatic". The choice
is stored in `saved-variables/options.toml`, announced in the `HELLO` of both connections, and sent with
`SET_LANGUAGE` when it changes in game; the window shows the applied language and the ones the server offers.

## 9. Security and privacy

- The server trusts no identity field sent by the client: the sender is always the connection's character.
- `AC.` prefixes are reserved for the server's own messages, so no player can impersonate it.
- Payloads are neither read, stored nor logged by the server.
- A chat mute (`/chatban`) also blocks AddOn messages; otherwise a chat AddOn could bypass it.
- Repeated rate-limit abuse is logged so GMs can step in.

## 10. Implementation

**ArchCore** — done on 2026-10-19
- `ARCHCORE_EXT` schema and constants in `crates/archcore-net/src/schema.rs`; encoding of fields 19 and 20
  and language code normalization in `crates/archcore-net/src/ext_text.rs` (shared with ArchRust).
- `HELLO` handling in `net.rs`, next to the `0x48` handshake, for both login and world, with the agreement
  kept in the session (capabilities and wished language).
- Negotiation in `crates/archcore/src/ext.rs`.
- `crates/archcore/src/world/addon.rs` module: checks, token bucket, relay.
- Language: catalog and fallback in `crates/archcore/src/locale.rs` (`language` and `localized_text` tables,
  `sql/004_locale.sql`); `SET_LANGUAGE` and `TEXT_REQUEST` in `crates/archcore/src/world/lang.rs` (world)
  and `login.rs` (login); NPC and monster names localized in view packets (`world/mod.rs`).
- Unit tests, and the `archcore sim addon` and `archcore sim lang` scenarios ([[Tools|EN-Tools]] page): two
  simulated clients exchange whispers and `SAY` messages, every refusal from section 7 is triggered, a
  simulated original client receives no `0xAC` packet; the same character enters in French then in English
  and receives different NPC names, `TEXT_REQUEST` and `SET_LANGUAGE` are checked, the original client gets
  the account's language.

**ArchRust** — done on 2026-10-19
- `HELLO` after the handshake, with the Options language; `HELLO_ACK` intercepted (3 s at most)
  (`archrust-netclient`, `connection.rs` and `ext.rs`).
- Lua API and events from section 8 in `archrust-addons` (`comm.rs`), send smoothing according to the
  announced limits.
- Language selector in the Options window (`archrust-engine`, `ui.rs`), stored in `options.toml`,
  `SET_LANGUAGE` on change in game, `C_ArchCore.RequestTexts` → `ARCHCORE_TEXTS`.
- `archrust login --port 14002 --lang fr` replays the flow without display and prints the applied language.

**Shared code.** ArchRust depends on the `archcore-net` crate (git dependency on the `Core` branch), which
carries the `0xAC` schema and `ext_text`. The licences allow it: both projects are GPL-3.0-or-later.

## 11. Decisions (2026-10-19)

1. **License**: ArchCore moves to **GPL-3.0-or-later**, like ArchRust, so `archcore-net` can be shared
   between the two projects.
2. **`/chatban` and AddOns**: a muted player cannot send AddOn messages either (`NOT_ALLOWED`).
3. **`SAY` scope**: same radius as normal chat (`World.ViewRadius`). A `SAY` AddOn message reaches exactly
   the players who would see a chat message; one setting for both.
4. **Messages to the server**: the first uses of the `addon-data` capability will be detailed character
   stats, quests and world state. The `SERVER` scope stays closed in version 1.
5. **Official AddOns**: there are none. The protocol neither announces nor distributes AddOns; `AC.`
   prefixes stay reserved for the server's own messages.
6. **Language** (v1.1): a `language` capability rather than a change to the ArchLord protocol, so the
   original client keeps working; BCP 47 codes; the wish follows the account; fallback to the base
   language, then English, then the original text; offered languages drawn from the game's official
   versions and Webzen's multi-language client ([[Translation|EN-Translation]] page).
