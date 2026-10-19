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

## Clock (type 0x11, AgpmTimer)

Between the loading position (op 8) and the world entry, the server sends an AGPMTIMER packet: op 6
in field 0, and in field 1 the server time in **milliseconds** (u64). The client advances it locally
and derives the game hour as `(ms / 3 600 000) % 24`, **without any time-zone correction**
(`AgpmEventNature`): that hour drives the day/night cycle, the music and the zone ambience
(`skyset.ini`). The original Korean server most likely sent its local time.

ArchCore therefore sends by default the Unix epoch **shifted by the machine's local offset** (local
midnight of the epoch = 0), so that the game hour is the server's hour; `world.timeZone = "utc"` in
the [[configuration|EN-Configuration]] sends the raw UTC Unix epoch instead. Fields 2 and 3 (u32) are
unused. `archcore sim` prints the received value and the matching game hour.

## NPC spawning (type 0x42, AgpmOptimizedView)

Op 1 carries, in field 1, a 26-field "character view" mini-packet: CID, TID, name, position,
destination, orientation, and so on. ArchCore gives NPCs CIDs starting at `0x40000000`.

Monsters use the same packet, with CIDs starting at `0x50000000`. They also need field 5 (factors: level
and speeds, type, current and maximum HP/MP/SP, attack), otherwise the client believes they have 0 HP;
field 6 is `0x04` (stopped) and field 9 (destination) is absent. The height sent is the one from the
tables (often 0): the client puts the creature on the ground. A monster leaving the view radius (× 1.2) is
removed with AGPMCHARACTER op 2.

## NPC functions (types 0x3C and 0x37)

The client only knows an NPC has functions if they come with its view: **field 4** of the view packet
(0x42) holds a sequence of full **ApmEventManager (0x3C)** packets, one per function.

| 0x3C field | Content |
|---|---|
| 0 | event designation (EVENT_BASE mini): object type **3** (character), NPC CID, function |
| 1 | function data (single-integer mini): dialog template, shop template, quest group |

Functions read from `npc.ini`: **21** dialog, **9** shop, **27** quests.

On click, the client sends **AgpmEventNPCDialog (0x37) op 0**: event in field 1 (type 3, NPC CID,
function 21), player CID in field 2. The server answers **op 1** with the same fields, and the client
opens the window; the text comes from the client (`npcdialog.txt`). ArchCore grants the dialog when
the player is within 1500 units of the NPC.

## Items (type 0x04, AgpmItem)

**4-byte** field flags, 27 fields. The main ones:

| Field | Content |
|---|---|
| 0 | op (INT8): 0 add, 1 remove, 2 update |
| 1 | status (INT8): 2 inventory, 10 NPC shop |
| 2 | IID, item ID |
| 3 | TID, item template |
| 4 | owner CID |
| 5 | count |
| 7 | slot: mini with 3 INT16 (bag, row, column) |

The client builds the item from its own template, read from `ItemTemplateAll.ini`. If it does not
know the TID, it silently ignores the item.

On world entry, the inventory is not sent as separate packets. The full AGPMITEM packets are
concatenated in **field 6** of the 0x42 view packet **op 2**. The client's inventory grid has 5 bags
of 5 × 5 slots (grid type 1).

## NPC shops (type 0x32, AgcmEventNPCTrade)

**2-byte** field flags:

| Field | Content |
|---|---|
| 0 | event (EVENT_BASE mini) |
| 1 | op |
| 2 | player CID |
| 3 | item IID |
| 4 | detail (mini, 2-byte flags): TID, price (integer), count, status, bag, row, column… |
| 5 | timestamp (INT64) |
| 6 | result |
| 7–9 | target inventory slot (buying) |

1. When "Trade" is clicked, the client opens the window at once, before any answer, and sends
   **op 0** to request the list.
2. For each item, the server sends an AGPMITEM with **status 10** and the NPC's CID as owner, then a
   0x32 **op 0** carrying the IID and the detail. **Op 1** ends the list. A shop grid has
   1 × 20 × 4 slots.
3. Buying: the client sends **op 2**, with the count in field 2 of the detail. The server answers
   op 2 with the result: **2** success, **3** not enough gold, **4** inventory full.
4. Selling: the client sends **op 3** with the IID of an inventory item. The server answers op 3 with
   the result: **0** success, **1** too much gold.

The character's money is updated by a character packet (type 2) op 1, field 8.

## Movement (type 0x40, AgpmOptimizedCharMove)

The client sends a 0x40 packet on every movement click (`FUN_140264010`), with 1-byte header flags:

| Field | Content |
|---|---|
| 0 | Character CID |
| 1 | current position (often missing: ArchCore then uses the previous destination) |
| 2 | destination |
| 3, 4 | followed target and follow distance (u16) |
| 5 | movement flags: `0x04` stop, `0x08` sync, `0x01` direction… |
| 6 | direction |

ArchCore updates the character's position, relays the same packet to the players who see it, then
recomputes visibility: a character packet **op 0** adds a player who entered the view radius, **op 2**
(CID in field 1) removes a player who left it or disconnected. The position is saved on disconnect
and every minute.

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

## Combat (type 0x41, AgpmOptimizedCharAction)

2-byte flags: 0 attacker (I32), 1 target (I32), 2 result (I8), 3 factors (packet), 4 target HP (I32),
5 combo (I8), 6 forced attack (I8), 7 additional effect (U32), 8 hit index (U8).

- **Client → server**: 0, 1 and 6. The client picks its target; it does not send one packet per hit.
- **Server → clients**: one packet per hit, to every player who sees the target. Result 1 = miss, 5 = hit
  (6 critical, 7 evade, 9 block…). Field 3 is only present on a hit; the client reads it with the **full
  AGPMCHAR_FACTORS schema** (checked in the x64 client): sub-packet 0 "result" holds a point block (HP,
  MP, normal damage as a negative number, field 9) used to display the hit, sub-packet 3 the target's
  remaining HP and MP.

Out of reach (player attack range + target `HitRange` radius), the server walks the player to the target
with an AGPMOPTIMIZEDCHARMOVE; a movement requested by the client cancels the attack. Hits follow the
attack speed (60,000 / speed ms).

When a monster dies, AGPMCHARACTER op 1 with field 11 (action status) set to 2; the corpse is removed
20 s later (op 2), then the monster respawns in place with full HP. The killer gets the template's
experience (`Exp` from CharacterDataTable.txt × the `exp` rate) in its factors (op 1, field 7) and a
system message; experience is counted per level (`LevelUpExp.txt`), and a level up is announced with
AGPMCHARACTER **op 5**. Only templates whose type has the "attackable" (0x10000) and "targetable"
(0x20000) bits can be targeted: ambient creatures (type 8: roosters, hens…) cannot.

**Monster retaliation.** A monster that is hit remembers its attacker (cumulative damage, forgotten 30 s
after the last hit) and, every 500 ms, targets whoever dealt it the most damage among living players
within 5000 units. It walks to them with AGPMOPTIMIZEDCHARMOVE (field 0 = the monster's CID), then hits
them with the same 0x41 packet (attacker = monster, target = player), at its attack speed. With no
attacker left it goes back to its spawn point; more than 10,000 units away from that point, it is
brought back with full HP. No spontaneous attack on sight.

**Patrols.** An idle monster ("monster" type, no attacker), seen by at least one player and whose spawn
point has a radius, walks every 15 to 45 s to a random spot within that radius, off blocked ground:
AGPMOPTIMIZEDCHARMOVE with field 5 set to 0 (walking), whereas a pursuit runs (0x10). At most 16
departures per tick.

**Player death.** At 0 HP: AGPMCHARACTER op 1 (factors, field 11 = 2) to the player and to those who see
them; they can no longer move or attack, and monsters drop them. The client asks to be resurrected in
town with AGPMCHARACTER **op 9**: the server restores HP and MP, sends status 0, teleports the character
to the resurrection point of their bound region and makes them invincible for 10 s.

**Regions and resurrection points.** Each segment of the server map has a region number
(RegionTemplate.ini), whose `Type` field says whether it is safe (bits 2-3 at 0), free or dangerous. A
player is bound to the last safe region they crossed; on entering the world, to the one they stand in if it
is safe, otherwise to their race's (9 Anchorville, 5 Golundo, 26 Norine). Resurrection points are the
objects of the client's obj*.ini files carrying an event 19 ("binding") of type 1: region by name
(`TownName`), allowed race and class (-1 or 0: all), object position. The player comes back at the nearest
point of their region they may use; failing that, at their race's starting village.

**Drops.** When a monster dies, its loot falls on the ground around the corpse (± 150 units): AGPMITEM
op 0 with status 1 ("on the ground"), the IID (`0x30000000` range), the TID, the owner (killer's CID),
the count and, in field 6, a mini-packet holding a single VEC3F, the position (read from the x64
client). Gold is the TID 268 item, its count being the amount. The client asks to pick an item up with
AGPMITEM **op 11** (field 2 = IID); the server stores the item (AGPMITEM op 0 status 2, or op 2 for an
existing stack) or credits the gold (AGPMCHARACTER op 1, field 8), then answers **op 12** (field 1: 0
success, 1 failure; 2 IID, 3 TID, 5 count) and removes the item from the ground (op 1) for those who saw
it. An item is reserved for the killer for 30 s, can be picked up from at most 2000 units, and vanishes
after 45 s.

Rolls: gold between the monster's `Geld_Min` and `Geld_Max` × the `gold` rate; for each group of
`groupdroprate.txt`, a chance out of 100,000 (group rate × `drop` rate, × the monster's `DropMeditation`
for groups affected by it, + a bonus by killer level), then an item rank from the monster's drop rank
(`droprankrate.txt`) and an item of that group and rank (`GroupID`, `DropRank`, `DropRate` from
itemdatatable.txt) suited to the monster's level; finally the monster's own items (`itemdroptable.txt`),
always given at 100,000.

## Equipment (type 0x04, AgpmItem)

The client asks to move an item with AGPMITEM **op 2**: status 3 to wear an inventory item, status 2 with
the wanted slot in field 7 (mini-packet of three INT16; -1 = server's choice) to put away a worn item or
move an item within the inventory. The server checks the required level, race, class and gender
(`Level`, `Race`, `Class`, `Gender` from itemdatatable.txt), puts the item already worn on the same body
part back in the inventory (`Part`: 2 body, 3 head, 5 hands, 6 legs, 7 feet, 9 left hand, 10 right hand,
11 and 12 rings, 13 necklace) and frees the other hand for a two-handed weapon (`Hand` = 2). It answers
the wearer with op 2 packets (status 3 without a slot for the worn item, status 2 with its slot for a
stored item), sends players who see them an op 0 (status 3, owner = wearer) or an op 1, then sends the
wearer's factors again (AGPMCHARACTER op 1, field 7). Equipment adds to the template's factors: physical
min and max damage, physical defense, accuracy (`Physical_ATKRate`), HP, and for a weapon its attack speed
and range. A refusal gives a system message and puts the item back. In the database a worn item has
status 1 and its body part in `layer`. A player's equipment is sent (op 0, status 3) to those who start
seeing them.

In the inventory, dropping a stack on a stack of the same item (op 2, status 2, target slot) merges them if
the total fits in one stack: the source disappears (op 1), the target is updated (op 2). AGPMITEM **op 17**
splits a stack (count in field 5, free slot in field 7): the new stack arrives with op 0, the old one is
updated with op 2. **Op 21** destroys an inventory item (op 1 in answer).

## Potions and regeneration (types 0x04 and 0x02)

The client uses an item with AGPMITEM **op 3** (field 2 = IID). A potion is a usable item (`ItemType` 1 in
itemdatatable.txt) that restores `HP` HP or `MP` MP, up to the maximum; it is refused when dead, when the
gauge is full, or while the delay of its kind (life or mana, `Use Interval` in ms) is still running. A use
consumes one unit (AGPMITEM op 2 with the new count, or op 1 for the last one), is confirmed with AGPMITEM
**op 4** (TID in field 3, user in fields 4 and 12, reuse delay in ms in field 18) and sends the factors
again (AGPMCHARACTER op 1, field 7) to the player and to those who see them.

Out of combat (no hit dealt or received for 10 s), a living player regains every 10 s 1.5% of their max HP
and MP, plus their recovery rate, plus 5; the new factors are sent the same way.

## Teleport scrolls (type 0x04)

A scroll ("Passport") is a usable item of `Type` 4; its `SubType` gives its kind: 0 back to town, 1 round
trip, 2 fixed destination (`Destination` column: x;y;z), 3 back to town right away. It is used with
AGPMITEM op 3 like a potion (one unit consumed, op 4 confirmation), then the server teleports the player
(AGPMOPTIMIZEDCHARMOVE and AGPMCHARACTER). The "town" is the resurrection point of the bound region, as for
resurrection.
Going back to town (0 and 1) is refused in combat. The round trip remembers the place left and announces
it with AGPMITEM **op 9** (field 4 = CID); the client asks to go back with **op 6**, refused in combat; the
server answers **op 10** (return disabled) and takes the player back.

## GM level (type 0x12, AgpmAdmin)

2-byte field flags: op, CID, then 13 mini-packets. Op 0 with, in field 2, a "GM data" mini-packet of
op 1 (CID, name, GM level as INT16, account name) records the character's level. The CID must be a
character the client already knows: ArchCore sends it right after the world-entry character packet.

## ArchRust extensions (type 0xAC)

A type of ArchCore's own. The original client only dispatches types 0x01 to 0x77 and ignores the rest;
an ArchRust client uses it after the handshake to negotiate its extensions (`HELLO` / `HELLO_ACK`) and
then exchange AddOn messages. The server never sends one to a connection that did not send `HELLO`.
Full specification: [[ArchRust extensions|EN-ArchRust-extensions]].

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
| 0x32 | NPC shops | 0x37 | NPC dialog |

Full schemas for the 86 modules are in `data/re/packet-schemas.json`.
