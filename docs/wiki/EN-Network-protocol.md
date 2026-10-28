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

Between the loading position (op 8) and the world entry, the server sends an AGPMTIMER packet (1-byte flags): in
field 0 the **time speed** (u8, not an operation; ignored when 0), in field 1 the game time in **milliseconds** (u64;
ignored when 0). The client advances that time by `speed` ms per real ms and derives the hour as
`(ms / 3 600 000) % 24`, **without any time-zone correction**: it drives the day/night cycle, the music and the zone
ambience (`skyset.ini`). Archonia starts at 12:00 with speed 6 until it receives anything. Fields 2 (server time, Unix
seconds, UTC) and 3 (time-zone offset, seconds) are read by Archonia's boss window ("Server Time", countdowns):
ArchCore fills them from `World.TimeZone` (offset 0 with `utc`).

ArchCore keeps a **global** clock (ArchCore's choice): speed `World.TimeSpeed` (6 by default, a day lasts 4 real
hours); with `World.TimeStart` = h (0 to 23), the time is h o'clock when the server starts; with -1 (default), it is
the machine's clock (`World.TimeZone`: Unix epoch shifted by the local offset, or UTC) multiplied by the speed, the
same from one start to the next. Two players entering at different moments see the same hour. The packet is resent to
every player in game every 10 minutes (the client resynchronises its clock). `archcore sim` prints the received value
and the matching game hour. See the [[configuration|EN-Configuration]].

## NPC spawning (type 0x42, AgpmOptimizedView)

Op 1 carries, in field 1, a 26-field "character view" mini-packet: CID, TID, name, position,
destination, orientation, and so on. ArchCore gives NPCs CIDs starting at `0x40000000`.

Monsters use the same packet, with CIDs starting at `0x50000000`. They also need field 5 (factors: level
and speeds, type, current and maximum HP/MP/SP, attack), otherwise the client believes they have 0 HP;
field 6 is `0x04` (stopped) and field 9 (destination) is absent.

**Height.** Most spawn points in the server tables have no height (0). Sent as is, a creature sat several
thousand units below the ground: the client lifted it while the distant terrain was coarse, then it
vanished under the detailed relief as the player came closer. ArchCore therefore puts these creatures on
the client's ground (`map_height`, read from `world/b*x.ma2`) and makes them follow the terrain as they
walk.

**Sector visibility.** The world is cut into 800 × 800 sectors of 6400 units, starting at
(-2,560,000, -2,560,000) in X and Z. As on the original server, NPCs, monsters, other players and ground
items appear to a player as soon as they enter the 3 × 3 sectors centred on theirs. ArchCore only removes
them beyond the 5 × 5 sectors (at least 12,800 units): the client draws characters up to about 10,000
units, and a removal on leaving the 3 × 3 sectors could happen at 6,400 units, in plain sight. They appear (optimized view op 1, character packet op 0, item
op 0) or disappear (AGPMCHARACTER op 3 "remove from view", item op 1) when the player or they change sector. The server
follows the player's walk between two movement packets, from their speed (tenths of a unit per second:
2000 walking, 5000 running for a human). This is the Archonia client's rule: a trip lasts
`10000 / speed × distance` ms, with status factor 8 when walking and 9 when running (move flag 0x10, capped at
60,000); an unknown speed is 4000 walking and 12,000 running. Removals use op 3 ("remove from view"), like the original server: op 2 deletes the character at once, while op 3 first goes through the client's view functions, which can delay the deletion.

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
recomputes visibility by sector (see above): a character packet **op 0** adds a player who entered the
neighbouring sectors, **op 3** (remove from view, CID in field 1) removes a player who left them or disconnected. The position is saved on disconnect
and every minute.

## Chat (type 0x0F, AgpmChatting)

1-byte field flags: op, sender CID, channel type, target CID, sender name (49), target name (49),
message (memory block).

- The client sends op 0 with its CID, the type (0 normal, 5 whisper with the target name) and the
  message **without a trailing zero**, under 256 bytes. A GM command is a normal message starting
  with `/`.
- Archonia sends **everything** as type 0, the chat bar mode adding its prefix to the text: `/p` party, `/g`
  guild, `/s` local shout; the player also types `/race` (or `/racetrumpet`), `/scream`, `/warn`. ArchCore reads these
  prefixes (case ignored) **before** GM commands; an unknown command from a player without GM rights is ignored with
  no answer. Only whispers come with their own type (5). Types 3 and 4 sent directly are still accepted.
- `/s`: type 1 "[Shout]" to players closer than `Chat.ShoutRange` (0: twice `World.ViewRadius`).
- Rate (values of Archonia's shared code): one message every 100 ms at most, the rest is ignored; GM commands are
  not limited.
- If the player picked a chat color, the client appends `\x01RRGGBB` to the message. ArchCore strips
  it before parsing a command and keeps it for display.
- On receipt, the client rejects a message over 512 bytes. Archonia's types and display: 0 normal, 1 "[Shout]",
  3 guild, 4 party, 5 and 6 whisper, 7 (whole world), 11 and 12 (notices) as "[Notice]", 8 to 10 (system) as
  "[System]" (a line repeated within 500 ms is ignored there), 13 guild notice, 16 siege, 17 yellow, 19 race trumpet,
  20 elemental shout; 17, 19 and 20 have neither header nor name. ArchCore answers the GM and sends its system
  messages as type 7, its announcements as type 10 (names fixed on 28 October 2026: `WHOLE_WORLD`, `SYSTEM_LEVEL3`,
  `NOTICE_LEVEL1`; values sent unchanged).

## Area shouts (type 0x4C, AgpmAreaChatting)

Server to client only (Archonia never sends it). 1-byte flags: 0 op (only op 0 matters), 1 sender CID, 2 shout kind
(I8: 1 race, 2 world, 3 all worlds; ignored by the client), 3 I8 (1, ignored), 4 AGPMCHATTING type of the message
(U32, picks the tab), 5 name (49), 6 text (memory block). ArchCore's rules:

| Command | Items ("usable" of kind 20, SubType) | Recipients | Type |
|---|---|---|---|
| `/race` | Race Trumpet (3905), kind 1 | players in game of the same race | 19 |
| `/scream` | Shout of Elemental (3906, 6182, 11518), kind 2 | all players in game | 20 |
| `/warn` | Warning of Ashtal (3911), kind 3 | all players in game (a single world) | 7 |

One item is used up per message (refused with a system message when missing); one shout per second (refused with a
message). The server builds the text: `[Item] [Name] message` (these tabs have neither header nor name), color suffix
kept; the sender gets it too. Chat bans (`/chatban`) apply.

## Chat rooms (type 0x2C, AgpmChannel)

4-byte flags: 0 op, 1 requester CID, 2 room name (41), 3 password (9), 4 character name (33: member, inviter, target
or author), 5 kind (1 public, 2 private, 3 guild), 6 result code, 7 message (121), 8 member list (memory block). A
CID other than the player's is refused.

| Op | Direction | Fields | Effect |
|---|---|---|---|
| 1 | client | 2, 3, 5 | Create: name taken → 1; 5 rooms → 4; guild room: master only → 5, one per guild → 7; otherwise op 3 |
| 3 | client | 2 | Join: unknown → 2; already a member → 11; other guild → 6; full or 5 rooms → 4; private: op 8 |
| 3 | server | 2, 3, 5, 8 | Joined; list `name1;name2;…;` plus a zero, the newcomer included, **always present** (the client reads it unchecked) |
| 4 | client → server | 2 | Leave (op 4 to the leaver, op 6 to the others); an empty room disappears |
| 5, 6 | server | 2, 4 | Member joined, left |
| 7 | client → target | 2, 4, 5 | Invitation (absent player → 3); it waives the password (ArchCore's choice) |
| 8 | server / client | 2 (3) | Password asked, then typed (wrong → 8) |
| 9 | client → members | 2, 4, 7 | Message relayed to every member, author included (the client shows nothing before it comes back) |
| 10 | client → server | 4 | Player search: op 10 when online, otherwise 3 |
| 11 | server | 6 | Result: 1 name taken, 2 unknown, 3 player not found, 4 full, 5 not master, 6 not a guild member, 7 the guild already has a room, 8 wrong password, 11 already a member |

Client limits: 5 rooms per character, 15 members (100 for a guild room), 40-character name, 8-character password,
120-character message. Rooms live in memory and are left when leaving the world; names are compared regardless of
case. The `<Chat>No</Chat>` region rule (regionperculiarity.xml, message 43) is not applied: Archonia's file has none.

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

**Elemental damage.** A hit that lands adds, for each element (magic, water, fire, earth, air, poison,
lightning, ice) the attacker has damage in, a part that lands with a 70 % chance for a player, 80 % plus the
level difference (at least 60 %) for a monster: a roll between the element's minimum and maximum, doubled,
minus the target's resistance to that element in percent (80 % at most for a player). Each part is shown in
the result's point block, fields 10 (magic) to 17 (ice); the HP lost is their sum. A player's elemental
damage comes from their equipment (`Magic_Min_Damage`… columns of itemdatatable.txt) and from the spirit
stones set in it; a monster's from CharacterDataTable.txt. Monsters have no elemental resistance in the data.

When a monster dies, AGPMCHARACTER op 1 with field 11 (action status) set to 2; the corpse is removed
20 s later (op 3), then the monster respawns in place with full HP. The killer gets the template's
experience (`Exp` from CharacterDataTable.txt × the `exp` rate) in its factors (op 1, field 7) and a
system message; experience is counted per level (`LevelUpExp.txt`), and a level up is announced with
AGPMCHARACTER **op 5**. Only templates whose type has the "attackable" (0x10000) and "targetable"
(0x20000) bits can be targeted: ambient creatures (type 8: roosters, hens…) cannot.

**Growth with level.** A player's factors follow their level according to `GrowUpFactor.txt` (table
`growup_factor`): one block per template (header line = TID, then one line per level, 1 to 121), as
absolute values. The level's line replaces the template's stats (STR, DEX, CON, WIS, INT), maximum HP,
MP and SP, and physical defense (`AC`); charisma, accuracy, evasion and damage replace it only when they
are not zero (they are zero throughout the file), and resistances are not read. Beyond the last level,
the last line applies; a template without a block uses the block of a playable template of the same race
and class. A new character gets the HP, MP and SP of its level. On every level up and on `/level`, the
factors are recomputed (growth, equipment, skill effects), HP and MP are refilled, then sent (op 1,
field 7). A human warrior thus goes from 120 HP and 32 MP at level 1 to 456 HP and 108 MP at level 25
(the template alone gave 90 and 46 at any level).

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
min and max damage, physical defense, accuracy (`Physical_ATKRate`), HP, elemental damage and defense, and
for a weapon its attack speed and range, all with the item's conversion (next section). A refusal gives a system message and puts the item back. In the database a worn item has
status 1 and its body part in `layer`. A player's equipment is sent (op 0, status 3) to those who start
seeing them.

In the inventory, dropping a stack on a stack of the same item (op 2, status 2, target slot) merges them if
the total fits in one stack: the source disappears (op 1), the target is updated (op 2). AGPMITEM **op 17**
splits a stack (count in field 5, free slot in field 7): the new stack arrives with op 0, the old one is
updated with op 2. **Op 21** destroys an inventory item (op 1 in answer).

## Item options (type 0x04, field 20)

A piece of equipment can carry up to 5 options from `itemoptiontable.txt` (imported into `client_table`, with
the original server's `optionnumdroprate.txt` and `socketnumdroprate.txt`). They are rolled when the item is
created: monster loot (source level = the monster's level), `/create` and `/additem` (the item's level).
Stored in `character_items.option_tids`.

- **Count**: between the item's `MinOption` and `MaxOption`, weighted by `optionnumdroprate.txt` (column =
  `MaxOption`). A new item's sockets are rolled the same way between `MinSocket` and `MaxSocket`
  (`socketnumdroprate.txt`).
- **Choice**: a random option "Type", without repeats, among those with an allowed option; then an option of
  that type by weight (`Probability`, 0 above 100,000). Allowed: the item's part is ticked (a left-hand weapon
  takes weapon options), the item's rank is between `Rank_Min` (0 counts as 1) and `Rank_Max`, the source
  level is at least `Level_limit` (and 1 to 120). A non-empty `Refined` or `Gacha` cell (even "0"), or a
  `CategoryName`, puts the option in another family, outside loot.
- **Network**: AGPMITEM field 20, a mini packet of 5 UINT16 (TIDs), in every op 0 of a known item (inventory,
  equipment seen by others, bank, trade, item on the ground). The client applies the options to the item's
  factors itself for the tooltip ("Additional Ability", the table's `Description`).
- **Effect**: the server adds the options of worn equipment to the character's factors. Each column targets a
  factor (min and max damage of an attribute, defense in points or rate, max HP and MP, accuracy, stats, attack
  range and speed, movement speed); `PointType` 0 adds the value, 1 applies it as a percentage of the total,
  as `ATK_Speed_bonus`, `Movement_Run_bonus` and `*_Percent` always do.
- **Linked options**: a unique item lists option groups (`Link_id` in itemdatatable.txt, separated by ";"); for
  each group the wearer gets the option whose `Level_min` ≤ their level ≤ `Level_max`. Nothing is sent: the
  client finds them itself for the tooltip.
- **Triggered options** (`skill_type` column, read by `Archonia.exe` with no client-side computation): 1 critical
  hit (chance `skill_rate`, damage `Critical` as a percentage of a normal hit; without a chance, `Critical` adds to
  critical damage; without `skill_type`, `Critical` alone is a chance), 2 stuns the target of a landed hit (chance,
  `stun_time`, or else `duration`, in ms), 3 turns a received hit into HP (chance, share `damage_convert_hp`),
  4 regeneration (`regen_hp`, `regen_mp` every 10 s, in combat too), 5 wounds the target (chance; damage from the
  column of its element every second for `dot_damage_time`, which overrides `duration`; this damage is not added
  to the factors), 6 evasion (`evade_rate`: chance to avoid a monster hit, result 7). `Ignore_Physical_Defense_Rate`
  and `Ignore_Attribute_Defense_Rate` add to the ignored defense (Skills section). The stun goes out in the
  monster's state (AGPMCHARACTER op 1, field 17), the wound in its HP (op 1); no effect packet announces them.

## Item conversion (types 0x16 and 0x38, AgpmItemConvert and AgpmEventItemConvert)

Rules and codes read in `Archonia.exe`; chances and values from `ItemConvertTable.txt` (imported into
`client_table`). An item's state (reinforcement level, sockets, set TIDs) is stored in `character_items`
(`convert_level`, `socket_count`, `socket_tids`).

AGPMITEMCONVERT (2 flag bytes): 0 op, 1 CID, 2 item IID, 3 IID of what converts it (potion, stone), 4 result,
5 reinforcement level, 6 sockets, 7 filled sockets, 8 added TID, 9 set TIDs (mini packet of 8 INT32).

- **op 0**: an item's state, sent after its AGPMITEM op 0 (entering the game, bank, trade, pick-up, creation)
  for a reinforced or socketed item. A new item gets its sockets by a roll (previous section); an item that
  takes sockets and has none stored has max(1, `MinSocket`), as the client computes it.
- **Blacksmith** (npc.ini function 22): AGPMEVENTITEMCONVERT op 0 (event, CID) asks for the window, op 1
  grants it. Adding a socket and reinforcing require a blacksmith in range.
- **op 2 → op 6, add a socket**: weapon (except lances, `Type` 17), armour, or "other" (kinds 4, 8, 16,
  64), 8, 6 and 5 sockets at most. Chance of the n-th socket: "Socket Slot Success" section (Korean header of
  the original server's file: socket addition success); on failure, a second roll in "Socket Slot Fail":
  5 no loss, 6 set stones lost, 7 item back to its original state, 8 item destroyed. Success 1, full 2,
  unsuitable item 9. No price: the client's cost table is empty and it shows 0. After 6 or 7, an op 0 sends
  the set stones again, which the client does not read from the answer.
- **op 4 → op 7, spirit stone**: a stone (`Type` 2) of element 1 to 5 (`Spirit Type`: magic, water, fire,
  earth, air) and rank 1 to 5 (`Rank`), into a weapon (except lances) or an armour in the inventory with a
  free socket; the item's `PhysicalRank` row in the first section must have the `Spirit<rank>` cell.
  Chance "Weapon Probability" or "Armor Probability" of the rank; the stone is used up either way. Here
  **0 = success**, 1 failure, 2 stone refused, 3 full, 5 unsuitable item. Effect: the rank's "Weapon Value
  Increase" to a weapon's min and max damage of the element, "Reinforcement Level%" to an armour's
  resistance; each further stone of the same element adds "Elemental Stone Bonus" (3).
- **op 1 → op 5, reinforcement**: a reinforcement potion (`Type` 8, used up either way) on a weapon (except
  lances) or a body or legs armour; level at most `PhysicalRank`. `PhysicalRank` row, target level column:
  success from "Success", otherwise a second roll: "Unchanged" (5), "Reset Reinforcement" (6, back to +0),
  "Destroy" (8). Full 2, unsuitable item 3, wrong potion 9. Each level adds "Weapon Increase" to a weapon's
  physical damage, "Armor Increase" to the physical defense of the rest, levels adding up.

- **op 3 → op 8, rune**: a rune (an item of `itemruneattributetable.txt`, imported into `client_table`) into an
  inventory item whose part is ticked for it (same columns as options) and that has a free socket; refusals:
  2 unknown rune, 3 full, 6 item below `Mount_Restriction_Level`, 7 part refused, 8 a rune with the same
  `ANTI Number` already set. Success (1) from `Success_Rate`, always with `AlwaysSuccess`; otherwise a second
  roll in "Socket Slot Fail" at index set items + 1: 9 no loss, 10 set items lost, 11 back to +0 and set items
  lost, 12 item destroyed. The rune is used up either way. A set rune gives the wearer the options of its
  `OptionTID` (itemdatatable.txt). A rune sent with op 4 is recognised by its table and handled as a rune.

- **Skill runes**: the ice, lightning and poison prayer stones (`Skill_Name`, `Skill_Level`, `Skill_Rate` of
  itemruneattributetable.txt; the client links the skill by its name) cast their skill on the target of a landed
  normal hit, with the `Skill_Rate` chance (4 to 6 %): AGPMSKILL **op 8** (caster = player, skill TID, the
  level's fixed elemental damage minus the target's resistance), then **op 4** for the level's effect (Cold Prayer
  Stone: 15 % slow for 5 s; Lightning: stunned 3 s; Deadly Poison: poison every 3 s for 30 s).

Archonia's stones: the client's check (`Spirit Type` 1 to 5, rank 1 to 5) refuses the poison (6), lightning (7),
ice (8) and stat (100 to 106) stones, which have no `Rank`; ArchCore refuses them too (op 7, result 2).

## Refining (types 0x4B and 0x3F, AgpmRefinery and AgpmEventRefinery)

Recipes from `RefineryItem2.txt`, the table `Archonia.exe` reads (5,731 recipes, imported into `client_table`;
otherwise the original server's `refineryrecipetable.txt`). A recipe is recognised by its ingredients
(`Resource TID`, `Resource Count`), in any order; its `MakeItem Total` rows are equally likely outcomes (item
and count made, bonus item, number of options `Item Option Min/Max` and sockets `Socket Inchant Min/Max` of the
item made); `Combine Use Price` is its gold price.

- **Opening**: AGPMEVENTREFINERY op 0 (event, CID) on a world object with function 26 (the alchemy sphere of
  the `obj*.ini` files, not an NPC); op 1 grants it, the client opens `UI_Refinery`.
- **AGPMREFINERY** (2 flag bytes): 0 op, 1 CID, 2 recipe id, 3 IIDs of the grid's 20 cells (INT32[20], 0 = empty
  cell), 4 result, 5 TID made, 6 price (INT64).
  - **op 5 → op 5, quote**: the grid names a recipe, the server returns its price in field 6.
  - **op 2, refine the grid**: the grid's items, whole, are the ingredients.
  - **op 6, refine a recipe**: id in field 2, ingredients taken from the inventory's stacks.
  - **op 1, result**: 0 success (TID made in field 5), 1 missing ingredients, 2 items without a recipe,
    4 not enough gold, 5 inventory full. The client shows uimessage's `REFINE_*_ITEM`.
- **Item made**: equipment gets options (between `Item Option Min` and `Max`, rolled like loot options) and
  sockets (`Socket Inchant Min`/`Max`), or those of a new item; it keeps from the equipment supplied (the same
  model, otherwise the first) its reinforcement, sockets, set items and options according to the `Keep
  Reinforcement Level`, `Keep Item Socket Count`, `Keep Socketed Items` and `Keep Item Options` columns. The
  "failure" outcome (`MakeItem IsFail`) makes its item like the others: the client does not read the column.

## Production (types 0x17 and 0x3A, AgpmProduct and AgpmEventProduct)

Gathering from corpses (Collection Skill, TID 663, tool 654 Gathering Knife), cooking (Cooking 203, tool 655 Cooking
Tools) and alchemy (Alchemy 285, tool 1016 Alchemy Cauldron). Data: `productcategory.txt` (skill of each category:
0 Hunt, 1 Cook, 2 Alchemy), `productcompose.txt` (95 recipes, named by the TID of the item made), `productexp.txt`
(levels 1 to 15: experience per action, success factor, cumulative total), `productgather.txt` (monsters that can
be gathered, per class, chance, item set) and `productresultitem.txt` (items of a set per skill level, weights adding
up to 100 per block; a server table shipped with the client), imported into the `product_*` tables. A skill's tool is
SKILL_SPEC's `arg1` when `requirement` is 4; having it in the inventory is enough.

- **AGPMEVENTPRODUCT** (1 flag byte): 0 op, 1 event (EVENT_BASE), 2 CID, 3 a, 4 b. NPC function 24, sent with the
  NPC (data: category, 0 Adventurer, 1 Cook, 2 Alchemist).
  - **op 0 → op 1**: opens `UI_PDSkillBuy` (NPC within 1,600 units).
  - **op 2 → op 3**: learn skill `a`; AGPMSKILL op 0 (level 1, experience 0) then b = 0 success, 2 already learnt,
    3 not enough gold, 4 level too low, 1 other refusal. Level and price: SKILL_CONST level 1 (7 / 10 / 15,
    300 / 500 / 1,000 gold), no skill point; price taxed in the player's region (see region taxes).
  - **op 4 → op 5**: buy recipe `a`; b = 0 bought (the client adds it to its list), 2 already known, 3 not enough
    gold (`MakingPrice`), 4 skill below `ItemLv`, 5 the category's skill is missing, 1 other.
- **AGPMPRODUCT** (2 flag bytes): 0 op, 1 CID, 2 skill id, 3 recipe / corpse / TID obtained, 4 recipe item IID /
  count, 5 code, 6 experience gained, 7 INT32 list.
  - **op 1** (server): known recipes, sent on entering the game.
  - **Gathering**: the client casts Collection Skill on a corpse (AGPMSKILL op 7); ArchCore accepts a production
    skill on a dead, targetable monster whose template can be gathered by the player's class, approaches to 250
    units and returns action 1, or refuses with action 16 (invalid target) or 17 (already gathered, or held by
    someone else). The client shows a 5.4 s bar, then sends **op 2** (field 3: corpse); the cast keeps the corpse
    15 s longer.
  - **Crafting**: Cooking and Alchemy are not cast; `UI_Compose` sends **op 0** (field 3: recipe, field 4: recipe
    item or 0).
  - **op 3** (server): result; field 3 TID obtained, 4 count, 5 code (0 success, 1 failure, 2 no tool, 3 invalid
    target, 4 too far, 5 inventory full, 6 not enough materials, 7 nothing left to gather), 6 experience. The item
    comes through AGPMITEM, the experience through AGPMSKILL op 2.
- A production skill's **level** is AGPMSKILL "dirt" factor 4; its **cumulative experience** is factor 6, saved in
  the database (`character_skills.exp`).

ArchCore's rules, the original server not having left them: success when a roll in 0..100 is below
min(100, chance + `productexp` factor at the current level), the chance being `Probability` (gathering) or
`SucessProb` plus `Recipe_addProb` when the recipe item is supplied (crafting); gathering: one item rolled from the
(set, level) block's weights, count 1, a single gathering per corpse for all players, a full inventory loses
nothing; crafting: materials used up on success and on failure (a material listed twice counts twice), recipe item
used up when supplied, 1 item made; an unknown recipe, one of another category, above the level or requiring a title
(`UsingMonster`, empty everywhere): code 1 without using anything up, and a system message; experience rolled in the
level's [min, max] on success only, next level when the level's cumulative total is reached (15 at most).

## Potions and regeneration (types 0x04 and 0x02)

The client uses an item with AGPMITEM **op 3** (field 2 = IID). A potion is a usable item (`ItemType` 1 in
itemdatatable.txt) that restores `HP` HP or `MP` MP, up to the maximum; it is refused when dead, when the
gauge is full, or while the delay of its kind (life or mana, `Use Interval` in ms) is still running. A use
consumes one unit (AGPMITEM op 2 with the new count, or op 1 for the last one), is confirmed with AGPMITEM
**op 4** (TID in field 3, user in fields 4 and 12, reuse delay in ms in field 18) and sends the factors
again (AGPMCHARACTER op 1, field 7) to the player and to those who see them.

Out of combat (no hit dealt or received for 10 s), a living player regains every 10 s 1.5% of their max HP
and MP, plus their recovery rate, plus 5; the new factors are sent the same way.

### Automatic use (type 0x14, AgpmUIStatus)

Two `PointScrollBar` sliders laid over the HP and MP gauges of `UI_CharInfo` (`CONTROL_9745`,
`CONTROL_24256`, "Use Life Potion Automatically") set a threshold in percent (position × 100): the client
drinks the potion of the bar's life or mana slot when the gauge falls below it. The server only keeps the
settings (table `character_ui_status`): potions and thresholds come in AGPMUISTATUS **op 1** (fields 6 and
7: life and mana potion TIDs; 9 and 10: thresholds), the helmet in op 3 (field 8); only the fields present
change. On world entry it sends everything back in **op 0**. Archonia's layout, 2-byte flags:
`INT8 INT32 PACKET[50] INT8 PACKET MEMORY_BLOCK INT32 INT32 INT8 INT8 INT8 MEMORY_BLOCK` (operation, CID,
shortcut bar, changed slot, its content, encoded bar, potions, helmet, thresholds, encoded cooldowns).

## Teleport scrolls (type 0x04)

A scroll ("Passport") is a usable item of `Type` 4; its `SubType` gives its kind: 0 back to town, 1 round
trip, 2 fixed destination (`Destination` column: x;y;z), 3 back to town right away. It is used with
AGPMITEM op 3 like a potion (one unit consumed, op 4 confirmation), then the server teleports the player
(AGPMOPTIMIZEDCHARMOVE and AGPMCHARACTER). The "town" is the resurrection point of the bound region, as for
resurrection.
Going back to town (0 and 1) is refused in combat. The round trip remembers the place left and announces
it with AGPMITEM **op 9** (field 4 = CID); the client asks to go back with **op 6**, refused in combat; the
server answers **op 10** (return disabled) and takes the player back.

## Skills (types 0x05 and 0x3B)

**Data.** `SKILL_SPEC.txt` describes each skill's nature (`attribute` field: buff, debuff, physical or
magic damage, passive; `target`; `melee_attack`: instant damage); `SKILL_CONST.txt` its values per level:
allowed classes (`require_class_*`), required level and points, learning (`skill_cost`) and upgrade
(`skill_upgrade_cost`) prices, HP and MP costs, cast, effect and recast times (ms), range (metres, × 100 in
game units), physical and elemental damage (flat and percent). Both tables name the skill, linked to its
TID by `SKILLTEMPLATE.ini`.

**A character's skills.** AGPMSKILL (2 flag bytes): op, ID, owner ("base" mini: type 3 = character, CID),
factors (AGPMCHAR_FACTORS whose "dirt" sub-packet holds the level at index 4), TID, status, action, points,
active effects. On entering the world, each skill is sent as **op 0** in field 8 of the optimized view op 2.
Every character knows "Normal Attack" (TID 222). Skill points are the character's level (plus one per level
from 50 to 70) minus the points required by each learned level; they are sent in the character's "dirt"
factors (index 5).

**Skill master.** "Trainer" NPCs carry function 25, with the race and class they teach (`SkillRaceType`,
`SkillClassType`), sent in their view (INT8, INT8 mini). AGPMEVENTSKILLMASTER (0x3B, 1 flag byte): op,
event, CID, skill ID, TID, result, points. **Op 4** opens the window (answer op 5, result 0); **op 1** learns
the TID of field 4: AGPMSKILL op 0 then op 6 with the result (0 success, 1 failure including money, 2 already
learned, 3 not enough points, 4 level too low, 5 class); **op 2** upgrades the skill of field 3: AGPMSKILL
**op 2** (new level) then op 3.

**Casting.** The client sends AGPMSKILL **op 7**: ID and action (mini: type, target as a "base" mini). The
server checks the skill, its recast delay, MP and HP, walks the player into range, takes the cost and
broadcasts **op 7** (action 1: target, position, cast, effect and recast times, level). When the cast ends,
**op 8** (action 2: target, result factors) goes to the players who see the target: as in combat,
sub-packet 0 of the factors holds the remaining HP and the damage as negative values, at index 9 for the
physical part and 10 to 17 for the elements (magic, water, fire, earth, air, poison, lightning, ice).
Auto-attack then resumes on the target.

Damage: the physical part takes the character's attack roll, multiplied by (100 + percent) / 100, plus the
flat damage; each element takes the character's elemental damage the same way, doubled. A higher-level
target removes 5 % per level of difference (80 % at most); armour only reduces the physical part; each part
is at least 1.

**Targets and areas.** A skill targets enemies if the units digit of its `target` field is 1; otherwise it
applies to its caster. A party skill (`range` field 303: sphere around the caster, restricted to its party
members; Marching Song, Combat Echo, Battle Cry…) also reaches the caster's living, in-game party members
within `area_r` of the caster (700 units for Marching Song): same heal, same effect, announced by AGPMSKILL
op 4 on each of them. The `range` field gives the area: above 1000 it is centred on the
target, otherwise on the caster; hundreds 3: sphere of radius `area_r` (metres), 4: box from the caster
towards the target (`area_f1` long, `area_f2` wide; 600 × 300 by default). Monsters in the area are hit
like the target, nearest first (`max_target_num`, otherwise 12 at most). An area around the caster is cast
without a target.

**Lasting effects.** The import files the `SKILL_CONST.txt` columns following the `FACTOR_POINT`,
`FACTOR_PERCENT` and `FACTOR_MAGNIFY` markers under the `point.`, `percent.` and `magnify.` prefixes
(they repeat the same names). A skill with a `duration` leaves an effect on its targets: stats, max HP and
MP, physical damage and defense, accuracy, evasion, movement and attack speeds (`point.*` adds,
`percent.*` multiplies), and damage over time (`dot_dmg_*`: flat damage per element every
`dot_damage_time` ms, minus 5 % per level of difference). `point.hp` and `percent.hp` (and mp) heal when
applied. A new effect of the same skill replaces the old one. AGPMSKILL **op 4** announces the effect (skill
ID, bearer in field 2, field 8 = list of "buff" minis: TID, duration in ms, caster TID) to the players who see
the bearer; **op 5** removes it (field 8: TID). On a player, effects are added to the factors (template and
equipment) sent back with AGPMCHARACTER op 1; they drop at death. On a monster, they change its defense,
damage and attack and movement speeds; each damage-over-time tick goes out as AGPMCHARACTER op 1 (result
factors). An effect that would kill a monster whose caster left leaves it at 1 HP.

**Passives.** A passive skill (units digit of `attribute` at 9) sets an endless effect while it is known;
it is recomputed on entering the world, on learning and on upgrading.

**Special states.** The `special_status` column of `SKILL_SPEC.txt` holds codes separated by ";" (those
from 1000 up set something else): 1 stun, 2 freeze, 3 slow, 4 invincible, 8 stun protection, 9 halt (no
skill and no attack), 10 invisible, 12 and 13 invincible to normal attacks or to skills, 14 and 15 skills or
attacks forbidden, 16 sleep, 20 hold, 22 fear… They are carried by the skill's effect. On a target they take
hold with the `skill_rate` chance (always without it; stun protection removes a stun with its own chance)
and last `stun_time` ms for a stun, otherwise `duration`; the effect lasts the longer of the two. On the
caster they always take hold. A character's state is the union of its effects' states; on every change,
AGPMCHARACTER **op 1** (ID in field 1, bits in field 17, u64: 0x1 stunned, 0x2 frozen, 0x4 slowed,
0x8 invincible, 0x200 invisible, 0x80000000 skills forbidden, 0x100000000 attacks forbidden,
0x200000000 asleep, 0x10000000000 held…) goes to the players who see it. A stunned, frozen or sleeping
monster neither moves nor strikes; a held one does not chase; under "attacks forbidden" or fear it does not
strike; a hit wakes it up. A player under these states cannot attack or cast; monsters leave an invincible
or invisible player alone.

**Combat effects.** Skills whose `melee_attack` is 106, 109 or 110 add `skill_rate` to the critical hit
chance and `critical` to critical damage (percent of normal damage, at least 100): a critical normal hit goes
out with result 6. `reflect_melee_attack` 2 (shield, like Defense Hold) adds `skill_rate` to the chance of
blocking a monster's hit, as does `point.physical_block` (80 % at most): result 9, no damage. Invincible to
normal attacks, the player evades (result 7). `convert_type` 1 or 2 turns a hit taken into HP or MP given
back (chance `skill_rate`, otherwise `special_status_rate`; share `damage_convert_hp` or
`damage_convert_mp` of the damage): result 1, then the player's factors (op 1, field 7).

**Damage reflection (counter attack).** `defense_melee_attack` 2 (Counter Attack, a berserker passive: 2 % at
level 1, 11 % at level 10) sends the damage of a monster's hit that lands or is blocked back to its author,
with the `skill_rate` chance: AGPMOPTIMIZEDCHARACTION with the player as attacker, the monster as target,
result **12** (counter attack: `Archonia.exe` then shows the "C.Attack!" mention from `IM_attack.png`) and the monster's factors (remaining HP, damage as a negative); the monster may
die of it, the experience then goes to the player. Bit 1 of `reflect_melee_attack` (reflecting the damage
taken) is carried by no skill in the Archonia data.

**Masteries.** `SkillMastery_High.txt` (high skills) and `SkillMastery_Heroic.txt` (heroic skills) arrange
skills in trees, per class template (96 human warrior… 1722 slayer, from `SkillMasteryEvolution.txt`). Each
skill of a tree requires up to three other skills at a given level (ConditionA to C columns, "TID;points"; the
client shows "%s Skill %d Points Requirement") and a total of points spent (ConditionD: skill points in the
high tree, "All Skills %d Points Requirement", heroic points in the heroic tree, "Total Heroic Skill Points
Required"); a skill's points are its level. Otherwise learning is refused: a system message, then
AGPMEVENTSKILLMASTER op 6, result 1. Capacitate (525) thus requires Marching Song (526) at level 4.

**Monster skills.** Each spawn point names an AI (`MobAITID` of `spawndatatable.txt`, `monster_ai` table
taken from `aidatatable.txt` of the original server tables): up to five "SkillN" skills (TID;level), each
with a HP condition ("HP;100%;이하": at most 100 % of the maximum) and a number of casts ("무한" unlimited,
"횟수;1" once per life); "UseNormalAttack" 0 forbids normal hits. A ready offensive skill (recast elapsed,
condition met, target within its reach: `range`, radius of an area around the monster, otherwise the
monster's range) is cast instead of a normal hit, or instead of chasing when the target is already within its
reach: AGPMSKILL **op 7** (cast, TID in field 4) to the players who see the monster, without op 0: the skill list (op 0) only goes to its owner, and the client plays the cast of a character in view from this packet alone;
at the end of the cast time, **op 8** (target = player, remaining HP, damage as negatives) to the players who
see the target. Damage follows the players' skill formula, from the monster's physical and elemental damage
(`CharacterDataTable.txt`); an area hits the nearby players; lasting effect, damage over time (AGPMCHARACTER
op 1 on each tick) and special states take hold on each target. A stunned, frozen or sleeping monster loses its
cast; a target gone more than 5000 units away (the chase limit) escapes it. A skill without SKILL_SPEC, cast time or recast that weakens (`buff_type` 1: "<MOB>Water Elemental
Skill"…) comes with the normal hits that land, with the `skill_rate` chance. The tables do not say how often
the original server cast these skills: ArchCore casts each one as soon as it is ready. Midas (TID 40, AI 403)
thus casts Insanity (181) every 6 s.

**Ignored defense.** Bits 4 and 8 of `reflect_melee_attack` (beyond the exact values 1 and 2) make the wearer's
hits ignore, while the effect lasts, part of the target's physical defense (Lethal Attack, Paint With Blood) or
elemental resistances (Stunner, Testament); the amounts come from `Skill_Const2.txt`
(`ignore_physical_defence_percent2`, `ignore_attribute_defence_percent2`, imported with the `SKILL_CONST.txt`
levels under the `percent2.` prefix), plus the equipment's `Ignore_*_Defense_Rate` options. Monsters' elemental
resistances (`Magic Res.`, `Water Res.`… in `CharacterDataTable.txt`, 100 % at most) now reduce the elemental part
of players' hits and skills. Stunner level 20 (48 %) thus raises the water part of a hit against a Green Worm
(50 %) from 3 to 4.

**Damage sharing.** The `divide` column of `SKILL_SPEC.txt` (bits read from `Archonia.exe`'s parser, meaning from
the tooltips): 1 elemental damage, 2 physical damage, 4 absorbed by summons, 8 heroic damage, 16 taken by the
caster. Divine Protection (17) and Martyr's Mark (18), party skills, put an effect on their caster and the nearby
party members: while it lasts, each monster hit taken by a member makes the caster, when in game and alive, take
`divide_rate` percent (10 to 50) of its elemental or physical part (the client shows "Party Damage Absorption
Rate"). The caster loses those HP (AGPMCHARACTER op 1, result factors) and may die from them. Dangerous Position
(8, heroic damage) and Crown of Madness (6, summons) have nothing to share in ArchCore.

**Dispels.** The `dispel_magic` column (modulo 10000; 10000 and more: repeated dispel): 2 (exact value) debuffs,
4 stuns, 8 slows, 16 every effect, 32 invisibility, 64 effects with a special state. A friendly skill dispels its
targets (Assistance and Disentangle, `target` 10, can aim at another player in range), an offensive one the
monster it hits (Antimagic) and also its caster when the hundreds of `target` are 1 (Bow of Meditation). Cure
(10002) leaves an effect that, every `interval` ms and with the `skill_rate` chance, removes its wearer's debuffs.
Each removed effect goes out in AGPMSKILL **op 5**; passives stay.

**Heroic points and charisma.** Heroic points ("dirt" factors, index 7) are earned with levels: 1 at level 90, then
2 per level up to 110 and 3 up to 120 (71 in all), minus the `require_heroic_point` of the learned levels. Charisma
is status factor 4, in raw points (0 to 1,000,000, `characters.charisma`); `Archonia.exe` holds no rule to earn it:
the original server gave it in player-versus-player fights and, under the `MobCharismaDrop` and `PVPCharismaDrop`
region rules of `iniRegionPerculiarity.xml` read by the client, on monsters; Archonia's file enables none and gives no
amount. Earning it therefore waits for player-versus-player fights; meanwhile ArchCore's own `/charisma` command
grants it (rate `Rate.Charisma` and the `bonus_charisma_add_rate` bonus of the effects that apply to that gain:
Charisma Double yes, Boss Additional Charisma only on a boss), and learning or upgrading
a heroic level spends it (`require_charisma_point`), as the client's rollback shows. Without enough heroic points or
charisma, learning answers 6 or 7 (codes of the client's check, which its window does not show: a system message
comes with them) and upgrading (op 3, field 5) 3 or 4 (1 money, 2 skill points), which the client shows.

Still to do: skill criticals.

## Teleport portals (type 0x31, AgpmEventTeleport)

Portals are scenery objects (`obj*.ini`) carrying event 8 and the name of a `TeleportPoint.ini` point
(`PointName`); no NPC teleports. AGPMEVENTTELEPORT (0x31, 1 flag byte): op, event (EVENT_BASE of type 1,
object), CID, point name (CP949, 33 bytes), position, disabled points.

- **op 4**: the client addresses the portal. Within 1600 units, the server answers **op 5** (the client
  opens the destination list); further away, it makes the player run towards the object and answers as soon
  as it is in range. A move asked by the player cancels the wait.
- **op 0**: the client picks its destination (point name). The trip is allowed if one of the groups targeted
  by the departure point (`TargetGroup`) lists the arrival point (`Group`) and the player's race is allowed
  at both ends (`UseType`: 1 human, 2 orc, 4 moon elf, 8 dragon scion; 0 all). The fee (`TeleportFee.txt`,
  also shown by the client) is taken from the money carried; the server sends **op 1** (CID, arrival
  position), moves the player, then **op 8** (loading done).

The player lands on a circle of radius `RadiusMin`..`RadiusMax` around the object carrying the arrival
point, at the first of eight points 45° apart (from a random angle) that is not on blocked ground.

**Player-versus-player zones.** The 19 `RegionType=1` points (all `SpecialType=1`): guild arena, Battle
Square, dungeon rooms and entrances. In Archonia (AgcmUIEventTeleport):

- picking such an arrival opens the `TELEPORT_ConfirmPvPZone` box (OK / Cancel); OK sends op 0;
- from such a point, the list only offers "Return" (`Teleport_ReturnOnly`), which sends **op 7** (event,
  CID). The AgpmEventTeleport module records on the character the departure point of each successful trip;
  ArchCore brings the player back there (free of charge), or to the bound village if there is none (after a
  reconnection, for instance). Op 7 is ignored from a point that is not `SpecialType=1`.

**Refusals.** The client does not send op 4 in combat mode (10 s after the last hit given or taken, or an
offensive skill): `Teleport_CombatMode` message in the chat; nor while transformed (character field 18):
`Teleport_TransformChar`. ArchCore likewise ignores ops 0, 4 and 7 from a dead player or one in combat. A
cancelled departure is reported by **op 2** (closes the loading screen).

## Bank (types 0x36, 0x04 and 0x02)

The bank is shared by the characters of one account. Keepers ("Safe Keeper", 18 NPCs) carry function 20,
with no data, sent in their view. AGPMEVENTBANK (0x36, 1 flag byte: op, event, CID): the client asks to open
it with **op 0**; the server, if the keeper is in range, answers **op 1**.

The bank has 4 pages of 7 rows × 4 columns; the first is always open, the next ones according to the
character's field 23 (extra pages). Bank items have **status 100**, their slot is in **field 8** of
AGPMITEM (instead of field 7 for the inventory); they are sent with the inventory on entering the world. To
deposit or move, the client sends AGPMITEM op 2 with status 100 and the wanted slot; to withdraw, op 2 with
status 2 and an inventory slot (or none: first free slot). Dropping onto a stack of the same item merges
them. The server answers with op 2 (new status and slot); it refuses while the window is not open at a
nearby keeper.

Money goes through AGPMCHARACTER **op 7**: field 9 holds the amount deposited (negative: withdrawn). The
server answers op 1 with the money carried (field 8) and banked (field 9). In the database, `account_bank`
keeps the account's money and pages; items stay in `character_items` (status 2), attached to the
character who deposited them and handed to the one who withdraws them.

**Extra page.** From the bank window, the client sends AGPMITEM **op 22** (CID in field 4). The next page is
paid from the money carried, at the `bankslotprice.txt` price (1,000,000 coins for the second; -1, the
next ones, cannot be bought with gold). The server answers AGPMCHARACTER op 1 (money in field 8, extra pages
in field 23) and system message 30 (page added), or 29 when the bank already has its 4 pages.

## Guilds (types 0x15, 0x39 and 0x54, AgpmGuild, AgpmEventGuild and AgpmGuildWarehouse)

AGPMGUILD (0x15, 4 flag bytes) has 25 fields: op, CID (or a system message code), guild name (32), master or
player (48), 8 × INT32, rank, creation date (INT64), size, union, password (12), notice, status, wins, draws,
losses, points, 6 sub-packets (including the **member**, field 16: name, rank, join date, level, TID,
online), guild mark (TID, colour), winner. Ranks: 1 member, 2 join request, 3 leave request, 10 master,
11 sub-master.

| Op | Way | Role |
|---|---|---|
| 0 | ↔ | Creation (letters and digits name, password); the server sends back the guild (name, master, date, size, notice) |
| 1 | ↔ | Invitation by the master or sub-master (invitee in field 16); the invitee gets it with the guild and inviter names |
| 2 | → | Invitee refuses; the master gets message 14 |
| 3 | ↔ | Joining: member if invited, otherwise a join request (rank 2) the master accepts with the same op; the server broadcasts the member |
| 4 | ↔ | Leaving: a member asks (rank 3, message 50 with their name, to the whole guild); broadcast when it happens |
| 29 | → | Leave accepted by the master or sub-master; without an answer it happens after one day |
| 5 | ↔ | Kick by the master or sub-master (the master cannot be kicked) |
| 6 | ↔ | Disbanding by the master, with the creation password |
| 8 | ← | Link of a character (CID) to its guild, with its member: on entering the world, to the other members (online, offline) and to players who see it |
| 9 | ↔ | Notice (field 10), by the master or sub-master |
| 10 | ← | System message: code (field 1), texts (2, 3), number (4, first of 8) |
| 7 | ↔ | Enlarging by the master of a guild of at least 20 members: next size (field 7, next ten) at the `guildmaxmember.txt` price; the new size goes to the whole guild |
| 28 | ↔ | Guild list: requested page (field 21, GUILD_LIST mini, page in field 1) or searched guild (field 2); answer: searched name (2) and GUILD_LIST (total, page, count as INT16, 19 GUILD_LIST_ITEM entries) |
| 32, 33 | ↔ | Guild mark (fields 22 TID and 23 colour) chosen by the master of a guild of 10 members; 33 buys it even though another guild has the same mark; broadcast to the guild, and the links (op 8, fields 22-23) to players who see its members |
| 48 | ↔ | Sub-master and handover; only the op is a field, the rest is raw (see below) |
| 11, 12 | ← | Guild status (field 11: 0 none, 1 battle declared, 2 getting ready, 3 battle); record (12 wins, 13 draws, 14 losses, 15 points), on entering the world and after each battle |
| 13 | ← | Battle in progress (field 17, GUILD_BATTLE mini packet), on entering the world |
| 15 to 20, 24 | ↔ | Guild battle: challenge, acceptance, refusal, cancel request, answer, surrender (see below) |
| 21, 22, 23, 25 | ← | Battle start, clock (every second), score, result |
| 34 to 43 | ↔ | Alliances and hostilities: requests, answers, leaving, truce (see below) |
| 44, 45 | ← | Details of the allies, of the hostile guilds (field 21, GUILD_LIST: at most 3 entries per packet) |

Creating a guild takes level 20 and the price of the first `guildmaxmember.txt` row (50 places for
10,000 coins). Codes used: 0 name taken, 1 level, 2 money (number: price), 4 characters, 5 already a
member, 6 guild full, 7 cannot join, 8 cannot leave (master), 9 master only, 10 wrong password, 11 created,
12 disbanded, 13 joined, 14 refused, 15 left, 16 kicked, 17 player already in a guild, 44 not enough gold to
enlarge, 45 not enough skulls, 47 searched guild not found, 48 join request, 50 leave request, 56 to 59 guild
mark (skulls, gold or cash, missing background, same mark as another guild: only 59 has a message in
Archonia, which then offers op 33).

**Guild list** (Archonia's FUN_140203630). Guilds are taken in creation order, 19 per page; an empty page gets
no answer. Entry: name, master, sub-master, level, ranking, members, size, (unknown), mark (TID, colour),
battle points; Archonia writes 0 for the level and the unknown field, and the ranking comes from an
unidentified guild value: ArchCore sends 0. A search returns the page holding the guild.

**Guild mark** (guildmark.txt, `FUN_1402050f0`). TID = background | pattern << 8 | symbol << 16 (item numbers),
colour = a colour index per layer at the same shifts. The price is the sum of the chosen items, in gold and in
cash; a background is required.

**Sub-master and handover (op 48).** Archonia's packet: header, flags 1, op 48 at +0x17, sub-op at +0x18, then a
structure; size = structure + 1, 0x6B footer.

| Sub-op | Way | Size | Content |
|---|---|---|---|
| 2 | → | 0x80 | +0x19 sender, +0x4a member, +0x7c rank (11 appoint sub-master, 1 remove that rank) |
| 3 | ← | 0xa0 | +0x19 guild, +0x4a master, +0x6b member, +0x9c rank: the member confirms |
| 4 | → | 0x84 | +0x19 guild, +0x4a self, +0x7c rank, +0x80 answer (1 accepts) |
| 5 | → | 0x7b | +0x19 sender, +0x4a sub-master: handover |
| 6 | ← | 0x9c | +0x19 guild, +0x4a master, +0x6b sub-master: the sub-master confirms |
| 7 | → | 0x80 | +0x19 guild, +0x4a self, +0x7c answer |
| 8 | ← | 0x70 | +0x19 guild, +0x3a member, +0x6c new rank (1, 10 master, 11 sub-master), to the whole guild |

As with Archonia's buttons: only the master appoints; the member is level 20, online, and the guild has no
sub-master (or it is the sub-master, to remove that rank); handover only goes to the online sub-master, outside
guild battles. On handover the former master gets rank 1, then the sub-master rank 10.

On entering the world, the member gets its guild (op 0), each member (op 3) and its link (op 8). Guild
managers (12 NPCs) carry function 23, with no data; AGPMEVENTGUILD (0x39): op 0 (event) ← op 1 (grant, the
client opens guild creation). Chat: type 3 goes to the guild members, type 4 to the party members.

**Guild battles** (Archonia: FUN_14084de70, FUN_14084ebd0, FUN_14084f610, FUN_14084f400). GUILD_BATTLE mini
packet (field 17, flags 2): enemy guild, enemy master, acceptance, delay, start (seconds), type, duration (s),
fighters, clock, score, enemy score, gain, enemy gain, result, round. Only masters act. Challenge (op 15:
enemy guild; type 0 points, 1 knockout, 2 master's death, 3 group survival, 4 individual survival; duration
from 5 to 120 minutes; fighters for types 3 and 4): both guilds have 10 members and 100 levels in total
(codes 20 to 23; the required number is the first of the eight numbers, the second one for 22 and 23), are not
in a battle (24, 25), the enemy master is online (26) and does not refuse guild battles (option 0x04: code 43).
The enemy master gets op 15, the challenger code 28 and status 1; without an answer within 60 seconds, the
request lapses. Refusal (op 17): code 30. Acceptance (op 16): status 2 and GUILD_BATTLE to both guilds; the
battle starts `Guild.BattleReadySeconds` later (op 21, status 3), then the clock (op 22) goes out every
second. Cancelling (op 18, then 19 or 20; code 33 when refused): impossible less than a minute before the start
(code 31). During the battle, members of both guilds can hit each other; each enemy killed is worth a point
(op 23). End: when the time is up (scores compared), "GG" surrender (op 24, refused before the start: code 35;
results 4 and 5), death of the enemy master (type 2) or no enemy fighter left standing (types 3 and 4,
fighters: the members online at the start, within the chosen limit). Op 25 (1 win, 2 draw, 3 loss) and record
(op 12), saved in `guild`. A guild in a battle cannot be disbanded (code 41).

**Alliances and hostilities** (Archonia: FUN_140202390 to FUN_140203130, FUN_140605a40 to FUN_140606630,
callbacks FUN_1408cc440, FUN_1408cbbf0, FUN_1408cc1c0, FUN_1408cbb00, FUN_1408cb2f0, FUN_1408cba20,
FUN_1408cb890, FUN_1408cbd20, FUN_1408cb400). Fields: guild (2), name (3), date (6), role (11), other guild (17:
GUILD_BATTLE mini packet reduced to its field 0); 37 and 43 also carry the CID (1). Only masters act.

| Op | Dir. | Content |
|---|---|---|
| 34 | → | Alliance request: 2 own guild, 3 own name, 17 the target guild; ← to the target master: 2 its guild, 3 requesting master, 17 the requesting guild |
| 35, 36 | → | Refusal (code 0x44 to the requester), acceptance; ← 36 to every guild of the alliance: 17 the ally, 6 joining date, 11 role (2 founder, 1 other) |
| 37 | ↔ | Leaving the alliance (17 = own guild); ← 37: 17 the leaving guild, 3 the new founder when the founder leaves |
| 38, 39, 40 | ↔ | Hostility declaration, refusal (0x52), acceptance; ← 40 to both guilds: 17 the hostile guild, 6 date |
| 41 | ↔ | Truce request, forwarded to the target master like op 38; Archonia refuses it with op 39 (0x57) |
| 43 | ↔ | Truce accepted; ← 43 to both guilds: 17 the other one |

An alliance has a founder and at most 3 other guilds (the client keeps no more, FUN_1401ff510); 20 members are
required on both sides (0x41, 20 written by Archonia; 0x42), 10 for a hostility (0x4f; 0x50); at most 5
hostilities (0x4d). A guild already in another alliance, or hostile, cannot be invited (0x3c); only the founder
invites (0x3d); 0x3e already allied, 0x40 target master offline, 0x43 waiting, 0x46 alliance made (0x4a and the
newcomer's name to the other allies), 0x47 left, 0x48 and the leaving guild's name to the others, 0x49 alliance
dissolved (a single guild left); 0x4b hostility impossible (ally, target guild at 5 hostilities), 0x4c already
hostile, 0x4e target master offline, 0x51 waiting, 0x54 hostility declared, 0x55 truce impossible, 0x56
waiting, 0x59 truce made. A request without an answer lapses after a minute. A master who refuses guild
relations (option 0x40: the client then does not show the question) is not asked: 0x3c or 0x4b to the
requester. On entering the world, the member gets op 36 for each ally, op 40 for each hostile guild and their
details (44, 45). Disbanding a guild removes it from its alliance and ends its hostilities. What hostility allows
in combat is not established by the client: ArchCore does nothing with it for now. Alliance chat ("[Union]"):
not done yet.

**Guild warehouse** (Archonia: AgpmGuildWarehouse FUN_140309ad0, receiving FUN_140309e50; AgcmUIGuildWarehouse
FUN_1405fca70 and following; AgcmEventGuild FUN_1406a6740). Bank keepers also carry function 35, with no data
(dialog button `sysstr` 611): AGPMEVENTGUILD op 2 (event, CID) ← op 3 (grant), then the client sends op 1 of packet
**0x54** (flags 2: 0 op, 1 CID, 2 money as INT64, 3 extra pages, 4 IID, 5 page, 6 row, 7 column, 8 result).

| Op | Dir. | Content |
|---|---|---|
| 1 | → | Opening; ← the items (AGPMITEM op 0, status 53, cell in field 7), op 0 (money, extra pages) and op 1 (result 2: the window opens) |
| 2 | → | Closing (the player moved more than 149 away from where it was opened) |
| 3, 4 | → | Money deposited, withdrawn (field 2); ← the character's money (AGPMCHARACTER op 1) and op 0 to those who have the window open |
| 5 | → | Bag item deposited (4 IID, 5 to 7 cell); ← AGPMITEM op 2 with status 53, op 0 to the others |
| 6 | → | Item taken back (3 target status: 2, 4 IID, 5 to 7 cell, -1: chosen by the server); ← AGPMITEM op 2 with status 2, op 1 to the others |
| 7 | ↔ | Expansion: Archonia asks 999,999,999,999 coins, ← result 4 (`GWARE_NOT_ENOUGH_MONEY`) |

Grid: 4 pages of 7 rows × 4 columns, the first one open. Rights (Archonia's masks, FUN_14030b830): the master has
them all (0x1f), the other members deposit items and money (5). 10 members are required (AgpmGuild code 0x5f,
`Guild_Store_Member_Fail`); a guild whose warehouse holds items or money cannot be disbanded (0x60,
`Guild_Destory_Keep_Fail`). The warehouse is loaded when first opened; its items are `character_items` rows with
stored status 3 and `guild_id`.

## Parties (types 0x08 and 0x09, AgpmParty)

Two packet types: **0x09** (management, flags 1: operation, author CID, target CID, 49-byte name) and
**0x08** (the party, flags 2: operation, ID, maximum members, member count, one member's CID, CID list as
u32, factors, effect area, experience sharing, bonus, item sharing).

| Management (0x09) | Meaning |
|---|---|
| 0 | invitation: the client sends the target (CID or name); the invitee gets op 0 (author, author's name) |
| 1 | refusal by the invitee; the author gets op 1 (invitee's name) |
| 2 | acceptance; the author gets op 2 (invitee's name) |
| 3 | leaving |
| 4 | kick by the leader; the kicked player gets op 4 |
| 11 | leader handover; the party gets op 11 (author = new leader) |
| 5, 6, 7, 8, 10 | failures: already in another party, already a member, party full, not leader, player absent |

A party has at most five members, leader first; a player keeps at most sixteen pending invitations. On
acceptance the party is formed (two members) or grows. The new member gets every other member's character
packet, **op 0** of type 0x08 (the party and its list), then one **op 3** per member; the others get their
character packet and an op 3 (them). On leaving, the party and the leaver get **op 4** (the leaver), the
leaver **op 1** (party removed); a party down to one member is disbanded. The leader sets experience sharing
(**op 5**, field 8: 0 by damage, 1 by level) and item sharing (**op 6**, field 10).

Party members see each other at any distance; a disconnected member leaves the party. A killed monster's
experience is shared between the living members close enough (5 × 5 sectors around the monster), by damage
dealt or by level: this sharing is an ArchCore addition, the original server does not describe it. Gold and
items still drop on the ground for the killer.

## Trading between players (type 0x07, AgpmPrivateTrade)

2 flag bytes: op, CID, other player's CID, IID, TID, money (INT64), origin and destination slots, argument.

| Op | Direction | Role |
|---|---|---|
| 1 | client | A asks B to trade (field 2); both get action status 7 |
| 3 | server | B receives the request (field 1 = A) |
| 4 / 25 | client | B accepts / refuses |
| 6 | server | Start, to each (field 1 = self, field 2 = the other) |
| 9 / 10 | client | Put / take back the item with the given IID (field 3) |
| 23 / 24 | client, server | Money offered (field 5); the other gets op 24 |
| 16, 17, 18, 19 | | Lock (self ← 16, other ← 17), unlock (18 / 19) |
| 20 | server | Both sides are locked |
| 21, 22 | | Ready (self ← 21, other ← 22); both ready: exchange |
| 0 | server | Result (field 8 = 8: done) |
| 5 | | Cancel, on both sides |

An item put in the trade goes, for its owner, to status 6 (trade grid: 1 × 3 × 4) through AGPMITEM op 2;
the other player receives it through AGPMITEM op 0, status 6, owner = the first one. It stays in the database
and the inventory until the exchange, and can be neither moved, used nor destroyed. On exchange, the server
checks the money and the inventory space, transfers the items (AGPMITEM op 1 for the old owner, op 0
status 2 for the new one) and the money (AGPMCHARACTER field 8). A disconnection cancels the trade.

## Quests (types 0x13 and 0x3E)

**Data.** `questtemplate.ini` (tab-separated) describes each quest: levels, allowed race and class, previous
quest, "self" quest (`Self_Quest`, taken from the journal), objectives (monsters A and B to kill, item to bring),
items dropped by the quest's monsters (`DropItem*`: TID, percent, count), rewards (items, items to choose,
money, experience). `questgroup.ini` gives, for each NPC group (`QuestGroupID` of npc.ini), the quests it
offers and those handed in to it. The French client only knows about sixty of them (event and high-level
quests).

**Protocol**, read from the x64 client. AGPMQUEST (2 flag bytes): op, CID, NPC event (EVENT_BASE), quest (mini:
TID, monsters A and B killed, objectives done, value, 49-byte text), TID, two integers, result, chosen reward,
three flag blocks.

| Op | Direction | Role |
|---|---|---|
| 0 | client | Take the quest of field 4 (NPC event in field 2, absent for a self quest) |
| 1 | server | Answer: field 4 = TID, field 6 = 1 (success) or 0; the client adds it to its journal |
| 2, 15 | client | Complete (field 7 = chosen reward) |
| 3 | server | Answer (fields 4 and 6) |
| 4 | server | Journal quest (mini), on entering the world |
| 5 | server | Flags of completed quests, on entering the world: fields 8, 9, 10 of 0x800 bytes; quest TID is bit TID % 16,384 of block TID / 16,384 (byte bit / 8, lowest bit first) |
| 6 | client | Abandon (field 4) |
| 7 | server | Answer (fields 4 and 6) |
| 10 | server | Progress (mini): the client shows "monster n/total" |

AGPMEVENTQUEST (0x3E, 1 flag byte: op, event, CID, integer): the client asks for an NPC's quest window
(function 27) with op 0; the server opens it by sending op 0 back.

**Rules.** A quest is taken from an NPC whose group offers it, or from the journal if it is a self quest; it is
refused if already in progress or completed, if the journal is full (60), outside the levels, race or class,
or without the previous quest. Each monster killed advances the quests that count it and may drop the quest
item (by its percent, up to the required count). The quest is completed (at an NPC that receives it, or from
the journal) once the monsters are killed and the items gathered: the required items are taken, the rewards
given (items, money, experience). In the database, `character_quests` keeps quests in progress (counters)
and completed.

## Option refusals (type 0x02, op 0x11)

The options' "Function" tab sets refusals that the client sends in AGPMCHARACTER **op 0x11** (field 1: CID,
field 22: `INT32` flags), on world entry and on each checkbox: 0x01 trades, 0x02 party invitations, 0x10 guild
invitations, 0x80 friend requests (0x04 guild battle, 0x08 battle, 0x40 alliance or hostility wait for their system). The
server keeps the value (`characters.option_flag`), sends it back in the same op and stops at the source what
targets a player who refuses; the author gets Archonia's message:

| Request | Reply to the author | Sentence (`uimessage.txt`) |
|---|---|---|
| Party invitation | AGPMPARTYMANAGEMENT op 14 (field 2: invitee, field 3: their name) | `Option_Party_Off_User` |
| Guild invitation | AGPMGUILD op 10, system message 42 | `Option_Guild_Off_User` |
| Trade request | AGPMPRIVATETRADE op 25 (field 1: targeted CID) | `Option_Trade_Off_User` |

## GM level (type 0x12, AgpmAdmin)

2-byte field flags: op, CID, then 13 mini-packets. Op 0 with, in field 2, a "GM data" mini-packet of
op 1 (CID, name, GM level as INT16, account name) records the character's level. The CID must be a
character the client already knows: ArchCore sends it right after the world-entry character packet.

## ArchRust extensions (type 0xAC)

A type of ArchCore's own. The original client only dispatches types 0x01 to 0x77 and ignores the rest;
an ArchRust client uses it after the handshake to negotiate its extensions (`HELLO` / `HELLO_ACK`) and
then exchange AddOn messages. The server never sends one to a connection that did not send `HELLO`.
Full specification: [[ArchRust extensions|EN-ArchRust-extensions]].

## Automatic reconnection (type 0x63, AgpmReconnect)

AGPMRECONNECT (4 flag bytes): op, CID, name (CHAR[48]), account (CHAR[1024]), password (CHAR[1024]), key,
agreement. The original client (AgcmReconnect):

- keeps the key of every **op 2** received (non-zero field 5). ArchCore sends one, picked at random, at the
  end of the world entry (agreement 0);
- when the world connection is lost with the character in game: opens `UI_ReconnectHold`
  (`ReconnectingServerShow`), retries the world address every 5 s for 60 s and shows "Attempting to
  reconnect to the game server...";
- once connected, sends **op 1** (CID, name, account, password, key) instead of the normal entry (type 0x03
  op 2);
- on an **op 2** with agreement 1 (field 6), resumes the game on the new connection, keeps the new key and
  closes the window; after 60 s, gives up ("Failed reconnecting to the server.").

ArchCore keeps a player whose connection drops in the world, standing still (60 s, the client's delay). A
valid resumption (key, CID, name and account; the password is not checked) attaches the character to the new
connection, sends op 2 with a new key and agreement 1, then sends again what the player sees (NPCs,
monsters, players and ground items removed and sent again, position and factors). A refused resumption
closes the connection. After the delay, the character leaves the world as on a disconnection; a new entry
through the login server replaces it at once.

## Player versus player (types 0x4A and 0x2A, AgpmPvP and AgpmRemission)

**AGPMPVP** (flags 2, server to client only): 0 op, 1 CID (message code for op 18), 2 target CID, 3 wins (message
integer), 4 losses, 5 mode (1 safe, 2 free, 3 combat), 6 status (bit 1 invincible), 7 op 0 byte, 8 guild
(CHAR[33], message string 1), 9 name (CHAR[49], string 2). Op 0 wins and losses, op 2 / 4 enemy added / removed,
op 6 list cleared, op 18 system message (`PvP_Msg_*` texts of uimessage.txt: 3 "in combat", 4 "combat over",
5 defeat, 12 victory, 17 target invincible, 20 friend, 24 charisma gained, 25 level too low…). Codes 0 to 2
(entering a safe, free or combat area) are produced by the client itself on region change; ArchCore sends code 2 then
13 ("You moved to a Battle Square") on arrival in the Battle Square, and 18 (level too low) when a supervisor refuses.

ArchCore rules, taken from the client (`FUN_1402d8e80`): an area's safety comes from bits 2-3 of RegionTemplate.ini's
`Type`; never an attack in a safe area; elsewhere, a declared enemy or a forced attack (Ctrl, field 6 of action 0x41),
level 20 on both sides, not one's party in a free area, not a friend (same guild, allied guild) in a combat area;
hostile guilds and guild battles: enemies by default; Battlegrounds (regions 110-114, 123-125, 175, 176) and Shrines
in battle: different races only, without Ctrl; Battleground, Battle Square and Shrine in battle: no murderer points,
no first attacker, no bounty right (see below). A forced attack makes both players enemies for 5 minutes, extended on every hit; in a free area against
an innocent, the attacker becomes a first attacker for 20 minutes (character field 12 = 1, field 25 = seconds left,
purple name). Killing an innocent more than 4 levels below after attacking first gives murderer points (status
factor 11, `PvP.MurdererPointsPerKill`) and opens a 2-hour delay (field 26); thresholds 40 (expelled from the party,
no invitation or trade, NPC prices doubled), 60 (items of kind 0x20 refused by NPCs, code 9), 100 (no forced attack,
monster experience halved). Killing a player of one's level or higher gives as much charisma as their level. The
amounts (points, charisma) do not come from the original server, which did not leave them.

**AGPMREMISSION** (flags 1): 0 op, 1 CID, 2 mini packet. Op 0 request to the pardoner (NPC function 31, EVENT_BASE
mini), op 1 grant (`UI_Remission` window), op 2 remission: mini {type 4 = letters of pardon} from the client, {type,
result} from the server (type 4 result 1 success, 3 no point, 7 not enough letters; type 1 result 0 the 2-hour delay
has not passed). 10 letters (TID 3311 or 1836) remove `PvP.RemissionPoints` points (2, as the client announces).

## Friends and ignore list (type 0x2B, AgpmBuddy)

**AGPMBUDDY** (flags 4, fixed-size fields): 0 op, 1 CID of the list owner (the receiver, from server to client: the
client drops the packet when it does not know that CID), 2 other player's name (CHAR[33]), 3 flags (`UINT32`: 0x1
block chat, 0x2 block trade, 0x4 block party and guild invitations, 0x00100000 online, 0x02000000 friend list —
absent: ignore list), 4 region (op 7), 5 "Buddy" relation, 6 result code (op 8). Op 15 has its own packet, same
type: 0 op, 1 CID, 2 count (`INT16`), 3 30 names of 33 bytes in a row (CHAR[990]), 4 30 flags (`UINT32`).

| Op | Direction | Role |
|---|---|---|
| 1 | client, server | Add: ignored player (flags 0), or accepting a friend request (requester's name, 0x02000000); the server answers with the entry it kept |
| 2 | client, server | Friend request: relayed to the online target (name = requester) |
| 3 | client, server | Refusal: relayed to the requester (name = the target) |
| 4 | client, server | Remove a friend or an ignored player |
| 5 | client, server | Block checkboxes of an ignored player |
| 6 | server | Online state of a listed player (bit 0x00100000); the client only changes the icon |
| 7 | client, server | Friend detail: their region (field 4, RegionTemplate; -1 offline) |
| 8 | server | Result (field 6): 1 already in a list, 2 offline or unknown, 3 list full, 4 oneself or invalid name, 12 invalid "Buddy" request |
| 9 | server | End of the initial list: the client reads `Ini\IgnoreList.txt` again and sends op 15 |
| 10 to 14 | | "Buddy" relation (mentor and disciple): postponed; op 10 gets code 12 |
| 15 | client | Ignored players from the client's local file (30 at most) |

On world entry ArchCore sends one op 1 per entry (friends with their online state, ignored players with their
blocks), then op 9; the following op 15 is merged into the database (known names that are missing, with their
checkboxes), with no reply. Players in game who have the newcomer among their friends get op 6, and likewise when
they leave the world.

ArchCore's choices (the original server is unknown): 30 friends and 30 ignored players, names compared without case,
a given character in only one of the two lists; friend requests only towards a player in game, ignoring works online
or offline; accepting adds each player to the other's list, removing only applies to the requester; the "refuse
friend requests" option (bit 0x80 of op 0x11) sends op 3 back at once. Blocks are applied by the server: a whisper
from an ignored player with "Block Chat" is not delivered (the author sees their message as usual, like with the
client's filter); trade requests ("Block Trade") and party and guild invitations ("Block Invites") are refused with
the options' message (see above). Database: `character_buddy`.

## Mail (type 0x2D, AgpmMailBox)

**AGPMMAILBOX** (flags 2): 0 op, 1 player's CID, 2 mail (mini packet), 3 IID of the attached item (op 3), 4 result
(0 success; the client tells no other code apart). **Mail** (flags 2): 0 id, 1 sender (CHAR[33]), 2 recipient
(CHAR[33], the character's exact name: compared with case), 3 subject (CHAR[51]), 4 date (Unix seconds), 5 flags
(0x01 read, 0x10 item collected), 6 TID and 7 count (`INT16`) of the attached item, 8 text (memory block, final zero
included, 1 to 1000 bytes). For an existing mail the client only fills in the id.

| Op | Direction | Role |
|---|---|---|
| 0 | server | Mail arrived ("YouHaveMail"), inserted at the top of the client's list |
| 1 | client, server | Delete; on result 0 the client removes the mail |
| 2 | client | Mail opened: marked read, no reply |
| 3 | client, server | Write (recipient, subject, text; field 3: attached IID or 0); reply: `MAIL_WRITE_SUCCESS` or `MAIL_WRITE_FAIL` |
| 4 | client, server | Collect the attached item; reply with the id (flag 0x10) |
| 5 | server | Mailbox emptied (unused) |

The client never asks for the list: on world entry ArchCore sends one op 0 per mail, from oldest to newest. It
removes nothing itself: on sending, the server takes one "Ordinary Letter Paper" sheet (TID 3601, AGPMITEM op 2 or
op 1) and removes the attached item (op 1); on collection it adds it to the inventory (op 0, status 2, first free
slot). ArchCore's choices: one sheet per letter, no gold, a single attached item (the whole stack) that stays the
same `character_items` row (status 4, options and conversion kept); recipient looked up in the database, offline
included, and served at once when in game; 30 mails per mailbox; no expiry. Refusal codes (with a system message):
1 failure (oneself, malformed mail), 2 unknown recipient, 3 mailbox full, 4 no paper, 5 forbidden item (papers
3601-3602, TIDs 4609 to 4623, like the client), 6 inventory full, 7 bound item (`BoundType` 1), 8 not in the
inventory, 10 shop item (`Cash`), 11 deleting a mail whose item was not collected. Database: `mail`.

## Wear and repair (types 0x04 and 0x34, AgpmItem and AgpmEventItemRepair)

An item's maximum durability is the `DUR` column of itemdatatable.txt (a value starting with `@` marks an
unbreakable item); the client reads the maximum from its own template only. The current durability is the **"item"
factor** (sub-packet 10 of the CHAR_FACTORS mini, 5 `INT32`, 1-byte flags), field 0, in AGPMITEM field 10: sent with
the item (op 0) once it is worn down, and on every change by an op 2 without status or slot (fields 0, 2 IID, 3 TID,
4 owner, 10). The client only shows the percentage: `UI_EquipStatus` lights up below 21 %, the slot is marked at 0 %.

Wear (ArchCore's rules, the client contains none): every hit that wounds, skills included, wears the weapon that
dealt it by one point with probability `Item.WeaponWearChance` (10 %) — mounted weapon on horseback, otherwise right
hand, otherwise a left-hand weapon; every wounding hit received wears one worn armour piece picked at random (body,
head, hands, legs, feet, shield) by one point with `Item.ArmorWearChance` (10 %). No wear on death. At 0 the item
stays worn but no longer counts in the factors (damage, defence, options, runes) until repaired. GM command
`/durability <value|-1>`: durability of the worn equipment.

**AGPMEVENTITEMREPAIR** (0x34, 1-byte flags): 0 op, 1 event (EVENT_BASE, function 17), 2 player CID, 3 IIDs of the 20
repair grid cells (`INT32[20]`, row × 4 + column, 0: empty), 4 cost computed by the client (`INT64`), 5 result code.
NPC function 17 (25 blacksmiths, 4 animal breeders) is sent without data, like the bank.

| Op | Direction | Role |
|---|---|---|
| 3 | client | Opening requested from the NPC (fields 0 to 2) |
| 4 | server | Granted: `UI_Repair` and inventory windows |
| 0 | client | Repair of the grid items (inventory items only: a worn item must be taken off first) |
| 1 | server | Result: 1 success (window closed), 2 not enough gold, 3 already full, 4 failure, 5 item cannot be repaired |

Price, like the client: `trunc((NPC price / DUR) × (DUR − current) × 0.5)` per item, plus the player's region tax on each item; a mismatch with
the cost sent gives code 4. Success: gold debited (AGPMCHARACTER field 8), every item made full again (op 2, field
10), then code 1; logged in `log_shop` (action `repair`). Refusals: NPC out of range or without function 17 (code 4),
item without `DUR` or unbreakable (code 5), all items full (code 3). A murderer of tier 2 or more is refused before
the window opens, by a system message (the client has the `CharacterPenalty_CantConvert` text, penalty 3, but the
packet that triggers it is not established).

## Mounts (types 0x04 and 0x4D, AgpmItem and AgpmRide)

A mount is a **part 14** item (Kind 32); part 15 holds the mounted weapon (lance, banner). The client never sends
AgpmRide: it mounts by equipping the item (AGPMITEM op 2, status 3) and dismounts by taking it off (status 2).
**AGPMRIDE** (0x4D, 1-byte flags): 0 op, 1 rider CID, 2 mount item IID (op 6: mount template TID), 3 time left
(ignored). Server ops: 2 mounted, 4 dismounted, 5 time out (no effect in the client), 6 mounted on a template
(unused).

Mounting: item equipped (to the player and those who see him), factors recomputed (to the player and his
neighbours), then op 2 (CID, IID, 0) to the player and his neighbours; the client builds the mount from the item's
`SubType`. Dismounting: item stored, factors, op 4. On horseback the item's `Run_Buff` replaces walk and run speed
(the client then lifts its 60,000 cap). View: bit 1 of AGPMCHARACTER field 18 ("transformation"); a player coming into
view receives the character, his equipment, then op 2; on entering the game the rider gets op 2 after his items.

Mounting refused (the item stays in the inventory): in combat (hit dealt or received less than 10 s ago,
AgpmSystemMessage code 24 "CannotRideWhileCombat"), murderer of tier 2 or more (system message), region without bit
0x10 of RegionTemplate `Type` (code 42), dead character. Forced dismount on death and in a region without mounts
(walking or teleport); a full inventory prevents it (system message, retried every tick). On horseback: combat
allowed, the mounted weapon replaces the hand weapons (on foot it does not count); skills refused (only those marked
`auto_attack`, which the client never casts, escape this); consumables refused without `EnableOnRide`. ArchCore's
choices: buffs are not removed when mounting, no expiry (no mount in the data has `RemainTime` or `ExpireTime`).

## Auction house (type 0x0B, AgpmAuction)

Instant purchase, at a fixed price, of **the whole stack** (no bidding). **AGPMAUCTION** (2-byte flags): 0 op, 1
player CID, 2 sub-packet depending on the op, 3 result (0 success). Sub-packets: **SALES** (2-byte flags: 0 sale id, 1
docID, 2 item number in the database, 3 IID, 4 gold price, 5 CC price, 6 count, 7 status 0 on sale / 1 sold, 8 date,
9 TID), **SELL** (0 IID, 1 count, 2 gold, 3 CC, 4 date, 5 IID of a Marvel scroll), **CANCEL** (0 id, 1 TID, 2 count),
**CONFIRM** (0 id, 1 TID, 2 count, 3 gold earned, 4 CC earned), **BUY** (0 docID, 1 count, 2 TID, 3 gold, 4 CC),
**SELECT2** (0 TID, 1 docID cursor, 2 direction, 3 total, 4 results), **ROWSET** (2-byte flags: 0 index, 1 query
text, 2 headers, 3 rows, 4 columns, 5 row size, 6 buffer, 7 U32 column offsets; every value is a zero-terminated
string). Prices and ids are I64/U64; Archonia has no "SELECT" op.

| Op | Direction | Role |
|---|---|---|
| 1, 2, 3 | server | Sale added to the seller's box (SALES), removed (id), status changed (1: sold, "Confirm" button) |
| 4 | client, server | Listing; answer 0 (sysstr 451) before the item is removed, 6: no more room (279) |
| 5 | client, server | Cancel (id); 0 (452), 7: inventory full (280) |
| 6 | client, server | Collect (id); 0 (453) with the earnings |
| 7 | client, server | Purchase; 0 (454), 5 count (274), 7 inventory full (275), 12 no longer available (276) |
| 9, 10 | client, server | Opening at a function 28 NPC (EVENT_BASE), granted with the same EVENT_BASE |
| 12 | client, server | Search by TID |

Opening: the client sends op 9 with the "Item Trader" EVENT_BASE (function 28, without data in its view, like the
bank); ArchCore answers op 10 when the NPC is in range (1500), except at murderer level 3. Search: the client starts
from the cursor 9,999,999,999,999 in direction 0 and asks again with the last row's docID while it gets 15 rows;
ArchCore returns at most 15 sales of the TID (direction 0: lower docIDs, descending; direction 1: higher ones) and the
total, without expired sales. Row columns: 0 docID, 1 seller, 2 sale id, 3 gold, 4 CC, 5 IID, 6 TID, 7 count, 8 date,
9 conversion (empty), 10 durability, 11 maximum durability, 12 options "a,b,c", 13 to 15 empty. The client sorts and
pages by itself.

ArchCore's choices (the original server is unknown): the sale id is also the docID; gold and CC (`characters.cash`)
at the database price, never the packet's (the client sends a zero CC); a 3 % fee on gold and CC taken when listing,
not refunded on cancel, no tax when collecting; prices as the client allows (gold 0 or 100,000 to 1,000,000,000, CC
up to 1,000,000, not both zero); only an item of the ordinary inventory, not in a trade, classified for the auction
house (`FirstCategory`, `SecondCategory`), neither an event item nor bound on pick-up; Marvel scroll ignored;
`Auction.MaxSales` sales per character; after `Auction.ExpireDays` days a sale leaves the search but can still be
cancelled. Opening, searching and buying need the NPC; listing, cancelling and collecting are done from the sales box
(`UI_SalesBox2` window), anywhere. A seller in game gets op 3 on purchase; offline, they find the sale "sold" on
entering the game (one op 1 per sale). A refusal without a code known to the client comes with a system message. The
item stays a `character_items` row (status 5), then goes to the buyer. In the database: `auction_sale`, log
`log_auction`.

## Equipment switch (type 0x6A, AgpmItemSet)

Equipment sets of Archonia's `UI_SwitchEditor` window; despite its name, AgpmItemSet has **no set bonus** (no table
in the data). **AGPMITEMSET** (1-byte flags): 0 op, 1 slot (0 to 99; op 4: active slot, -1 none), 2 argument (op 3:
filled 1 / empty 0; op 6: `(from << 4) | to`; op 7: position), 3 items (CHAR[4000]): `E,<part>,<id>` tokens separated
by `:`, 10 at most, the id being AGPMITEM field 26 (U64). Slot = `(profile × 10 + config) × 2 + set` (set A 0, B 1),
computed by the client.

| Op | Direction | Role |
|---|---|---|
| 0 | client | Save the worn equipment; answer op 3 then op 5 |
| 1 | client | Wear the set (empty: take everything off); answer: equipment changes (AGPMITEM), op 4 |
| 2 | client | Clear; op 3 (0), empty op 5 |
| 3, 4, 5 | server | Slot filled or empty, active slot, items |
| 6, 7 | client | Move, remove an item of the set; op 5 |

On entering the game, ArchCore sends ops 3 and 5 for each filled slot then op 4, after the items (AGPMITEM now carries
field 26 = IID, the database id, for every item; field 19 "flags", where the client reads binding, is not sent).
ArchCore's choices: free; wearing a set goes through the manual equipment path (level, race, class, gender, mount,
room to store), item by item, with no message on refusal; items no longer in the inventory are skipped; no op 4 after
saving; moving past the last item puts it at the end of the list. Profiles, names and hotkeys stay in the client's
local file. In the database: `character_item_set`, `characters.item_set_active`.

## Weather (types 0x30 and 0x47, AgpmEventNature and AgpmNatureEffect)

Server to client only. **AGPMEVENTNATURE** (0x30, 1-byte flags): 0 weather (I8), 1 and 2 (I64) with no known reader.
The client only reacts to 0 (effect fades out, 5 s), 3 (rain: nature effect 1, sky 99) and 5 (snow: effect 0, sky
98). **AGPMNATUREEFFECT** (0x47, 2-byte flags): 0 effect id from `NatureEffect.ini` (-1: stop); ArchCore does not use
it, the weather is enough. ArchCore's choices: one weather for everyone, set by `/weather clear|rain|snow`
([[Administration|EN-Administration]]) or rolled every `World.WeatherMinutes` minutes when `World.WeatherAuto` is 1;
sent to every player in game on each change and on entering the game; not saved (clear on start). The ArchLord's sky
skills (478 "Day and Night", 522, 523) are described with the ArchLord title.

## Summons (type 0x4F, AgpmSummons)

Creatures of skills (`summons` column of SKILL_SPEC) and of items with "Use skill id". **AGPMSUMMONS** (2-byte
flags): 0 op (I8), 1 master's CID (I32), 2 creature's CID (I32), 3 creature's TID (I32), 4 summon type (op 0) or new
limit (ops 3 and 4) (I8), 5 propensity (I8: 0 attack, 1 defense, 2 shadow), 6 and 7 timestamps (U64, non-zero), 8
"forced" (U8: 1 outside the limit). Every client constructor sets field 8: ArchCore always sends it.

| Op | Direction | Fields | Role |
|---|---|---|---|
| 0 | server | 1 to 8 | Summon added; the client ignores it unless the creature is already known with its type's state |
| 1 | server | 1, 2 | Summon removed ("summoned" state cleared, forced slot given back) |
| 2 | client → server → master | 1, 2, 5 | Propensity (`UI_TargetSummonsMenu2` buttons); accepted for the player's own type 1 summon, sent back to the master (the label only changes on receipt) |
| 3 | server | 1, 4 | Summon limit (1 at first on the client) |
| 4 | server | 1, 4 | Tamed creature limit (unused) |

Appearance, in this order: creature view (0x42 op 1, CID in the 0x58000000 range), special state (AGPMCHARACTER op 1,
field 17: 0x800 for types 1 and 8, 0xC00 for type 4), then op 0, to the master and to those who see it; a player who
sees it later gets all three. End: op 1 to the master and to those who see it, then AGPMCHARACTER op 2 (op 3 after a
death, to leave the animation).

| `summons` | Skills | ArchCore |
|---|---|---|
| 1 | spirits (Griolli…), Sacred One, Bomba, Legion of the Dead, Call Royal Knight | type 1: `summons_tid`, `summons_count` copies, `duration`; master's limit, raised if needed up to the number summoned (op 3); the oldest leaves |
| 2 | Lightning Trap, Holy Ground | type 4, fixed, at most `limit_quantity` of the same skill; hits monsters within its range (none for Holy Ground); stays when the master dies |
| 4 | Increase Gate | limit raised to 1 + `limit_quantity` for `duration` (op 3), then given back (the oldest extra ones leave) |
| 5 | Elemental Heal (passive) | its `percent.max_hp` is added to the summons' HP |
| 6 | summoning scrolls (items) | forced type 1, one per scroll, the item is used up |
| 8 | pets (items, `CanStopUsingItem`) | forced type 8, one at a time; follows without fighting or being attackable; using the item again sends it back |
| 7, 11 | Call on Nature, Evil Light, rift traps | not handled (no creature TID) |

ArchCore's choices: the model's stats (CharacterDataTable) × master's level / model's "LVL" × `Summons.StatScale` %,
level = the master's (a pet keeps its model's); timestamps = appearance and planned end in Unix ms (a pet: ten years
ahead). A combat summon stays ~200 units from its master and jumps to him beyond 3000; in attack it hits the master's
target, otherwise monsters targeting him or it and players attacking them; in defense, only the latter; in shadow,
nothing. Monsters it hits target it back (normal hits). A monster it kills is killed by the master (experience with
party sharing, loot, quests; its damage counts as his). Player versus player: it only attacks a player its master may
attack; attacking it follows the rules towards its master; never one's own; players' area skills do not hit it. End:
duration, death, master dead (except fixed creatures), gone or teleported (portal, scroll, GM; a pet follows). Not
done: taming, a pet's stop packet (unknown), pets' buffs.

## Stylists (type 0x43, AgpmEventCharCustomize)

1-byte flags: 0 op, 1 event (`EVENT_BASE`, function 29), 2 player CID, 3 face (I8), 4 hair style (I8); -1:
unchanged. 14 "<Stylist>" NPCs. The protocol has no result code.

| Op | Direction | Effect |
|---|---|---|
| 0 | client | Open the window: function 29 stylist in range → op 1 |
| 1 | server | Grant (event and CID sent back); the client opens `UI_Customizing`, or shows its own refusal when mounted or transformed |
| 2 | client | Purchase (face, hair style); the client closes the window without waiting for an answer |

`character_customize` table (charactercustomizelist.txt): kind (hair style, face), number (the one of AGPMCHARACTER
fields 20 and 21), name, `Case` (0 creation and stylist, 1 stylist, 2 creation), model, required level, price (gold,
or items when `Price(Skull)` is not zero: never in the data). Purchase, as the client's shared logic: price = sum of
the prices, each with the player's region tax, required level = the face's when one is chosen (none for a hair style alone), `Case` not
checked. ArchCore also refuses, with a system message: mounted player, value missing for the player's model or equal
to the current one, item price, too low a level or not enough gold. Accepted: gold taken, look saved, AGPMCHARACTER
op 1 (changed fields 20 and 21) to the player and to those who see them. On character creation, face and hair style
must be offered at creation for the model (Case 0 or 2), otherwise 0.

## Wanted posters (type 0x46, AgpmWantedCriminal)

2-byte flags: 0 op, 1 CID, 2 page, 3 rank, 4 result / flag / kind, 5 mini-packet (`EVENT_BASE` for ops 0 and 1,
otherwise a 2-byte mini: name (33), amount (I64), level, model TID, online). Ops 4 to 8 are server-only.

| Op | Direction | Effect |
|---|---|---|
| 0 → 1 | client → server | Board opened near a function 32 NPC (18 town chiefs), 1600 units at most |
| 2 | client → server | Bounty {name, amount}; answer: 0 done, 1 under the base bounty, 2 not enough gold, 3 no right (or board full) |
| 3 → 4 | client → server | Page 0 to 9: one op 4 per line (rank, mini), then an op 4 with page -100 and no mini |
| 5 | client ↔ server | Rank and total bounty (the client asks every 30 s while wanted); no longer wanted: op 7 flag 0 |
| 6 | server | Killer's name to the victim ("Wanted" button of the death window); empty on resurrection |
| 7 | server | "Wanted" flag (`WantedAlarm` icon), on entering the game and on each bounty |
| 8 | server | Kind 4: bounty paid to the hunter; kind 5: experience lost by the criminal |

ArchCore's rules (the client only displays): op 6 is sent when the killer struck an innocent victim first, or when the
killer is a murderer (level 1 at least), never in a Battleground nor in a guild battle; the right lasts until
resurrection or a successful bounty. Base bounty by the poster's level (50,000 up to level 20, 100,000 up to 40,
200,000 up to 60, 400,000 up to 80, 1,000,000 beyond, client's table), paid from carried gold then from the bank. One
poster per criminal: bounties add up (`wanted_criminal` and `wanted_bounty` tables), each bounty pushes the expiry back
(`Wanted.ExpireDays`); 100 posters at most, ranked by decreasing bounty. The player who kills a wanted criminal,
outside the criminal's party and guild, gets the total bounty; the criminal loses `Wanted.ExpLossPercent` of the
level's experience (never a level) and the poster disappears. A wanted criminal is not innocent: killing one gives
neither murderer points nor first attacker status.

## Shrine battles (type 0x62, AgpmShrineBattle)

Episode 8 module, a **raw structure** both ways (no AuPacket fields): standard header, byte 0x13 = 1, U16 operation at
0x14 (ArchCore reads it as a 1-byte-flag packet whose only field is the operation), padding at 0x16, data from 0x18,
0x6B tail. Shrine structure (0xD0): 0x00 Id, 0x04 name (30), 0x22 carver's name (49), 0x80 state (0 closed, 1 open,
2 carving, 4 occupied, 6 over), 0x84 occupying race (1 to 4), 0xB8 carver's CID; ArchCore leaves the time fields
(0x58-0x78, 0x90-0xB0), whose meaning is unknown, at zero.

| Op | Direction | Size | Content |
|---|---|---|---|
| 0 | client → server | 0x20 | List requested (`UI_Shrine_info` window); answer ops 2, 11, 12 |
| 1 | server | 0xF0 | 0x18 Id, 0x20 Shrine: state change (gauge, effects on the carver), to everyone |
| 2 | server | 0x980 | 10 entries of 0xF0 from 0x20, Shrine at +0x20 of each; also on entering the game (the client has no `ShrineTable.txt`: its table comes from there) |
| 3 | server | 0x20 | 0x18 Id, 0x1C carving gauge in percent (0 hides it) |
| 6 | server | 0x2C | 0x18 Id, 0x1C CID, 0x20 flag, 0x24 state, 0x28 value: flag 0 state 2 = carving cancelled |
| 7 | server | 0x58 | 0x18 Id, 0x1C `shrinemessage.xml` message, 0x20 race (or number for "%d" messages), 0x24 name |
| 8 | client → server | | Unknown meaning (missing from the client): logged |
| 9 | client → server | 0x20 | 0x18 Id, 0x1C CID: enter |
| 10 | server | 0x20 | 0x18 Id, 0x1C minutes: opening soon, offer to enter |
| 11 / 12 | server | 0x24 | 0x1C wins, 0x20 losses: the player's / their race's |

ArchCore's rules (Archonia's client holds nothing of the server; `shrine` and `shrine_reward` tables filled by the
content layer): eight Shrines (central regions 236 to 243, race zones 244 to 275, outskirts 276 to 283) open every day
at 20:00, 21:00 or 22:00 for 30 minutes (`Shrine.Enabled`, GM command `/shrine open|close <n>`). Announcements at 10, 5
and 1 minute (messages 1, 2, 4; message 3 and op 10 to players of the right level), entry possible 5 minutes before.
Entry (op 9): level 50 at least (otherwise AgpmSystemMessage code 47), 50 players per race at most (code 48), teleport
to the resurrection point of one's race in the Shrine's zone. A player standing still in the central region carves the
seal (60 s, op 3 gauge; cancelled if they die, move or leave: message 10); their race then occupies the Shrine, which
they must hold 5 minutes alive and without leaving (otherwise message 16, back to state 1). Victory: messages 17 and 22
to everyone; rewards to those present (53000 coins and charisma: carving to the carver, winning race, common;
messages 18, 19, 23), scores (`shrine_score`, `shrine_race_score` tables). No winner when time runs out: messages 20
and 21. Fifteen seconds after the end, message 27 and those present go back to town, op 1 state 0. During the battle
(states 1 to 4), in the Shrine's regions: different races are enemies without Ctrl, the same race never, no murderer
points, no first-attacker status, no bounty right; the dead come back at their race's point.

## Battle Square and Battleground (types 0x4A and 0x58)

**Battle Square** (regions 189 to 234, six floors; roots 198, 208, 209, 215, 223, 229, LevelMin 20 to 70 from
RegionTemplate.ini imported into `region_template.level_min`): combat area with no race restriction; a kill there
gives no murderer points and no bounty right. The victim may drop a skull on the ground (`pvpskulldroptable.txt`,
`pvp_skull_chance` and `pvp_skull_level` tables: chance by killer − victim level gap, multiplied by `Rate.Drop`, skull
level by the victim's level, `BattleSquare.SkullTids` items). Each floor is an island of the server terrain and no
portal leads there: talking to a supervisor (NPCs 551 to 554) sends the player to the gate of the highest floor their
level allows (`BattleSquare.SupervisorTeleport`, ArchCore's choice); below level 20, AgpmPvP op 18 code 18. Arriving
from outside: codes 2 then 13.

**Battleground** (regions 110 to 114, 123 to 125, 175, 176): always open; the dead come back at their race's point in
their race's fort (111 human, 112 orc, 113 moon elf, 176 Dragon Scion). With `Battleground.Schedule` ("HH:MM-HH:MM",
empty by default), start and end are announced by the raw **0x58** packet: 0x13 op 1, 0x14 text (1024), 0x414 ARGB
colour, 0x418 kind (2 start, 4 end; the client then shows its texts 780 and 781), 0x41C variant (0 race battle).

## Boss window (type 0x64, AgpmBoss)

Archonia addition with no effect on gameplay. 1-byte flags: 0 op, 1 byte (0), 2 PACKET[32] spawns (48-byte raw mini:
0x00 TID, 0x18 U64 next spawn in Unix seconds, 0x20 hour, 0x21 minute, 0x28 alive), 3 loot (196-byte raw mini: TID,
16 item TIDs, 16 rates, 16 counts), 4 TID, 5 ranking (page, pages, player's rank, name, wins, PACKET[16] rows {rank,
name, wins}), 6 damage meter (TID, count, PACKET[20] rows {name, model TID, level, guild, mark, 0, I64 damage}).

| Op | Direction | Effect |
|---|---|---|
| 1 | server | `UIBoss.xml` bosses (`boss_window` table) present in the world, on entering the game and on every death, delay roll or respawn |
| 4 → 3, 5 | client → server | Boss picked (field 4): possible items (`monster_drop` then loot groups, 16 at most) and ranking page 0 |
| 6 → 5 | client → server | Ranking page (field 5, mini whose field 0 is the page) |
| 7 | server | Damage meter to the players hurting a tracked boss, every 2 s at most (the client clears it 10 s after the last) |

ArchCore's choices: the ranking is the last fight against that boss, sorted by damage; each row's value counts the
player's wins over that boss (`character_boss_kill` table, one per fighter of the fight in which it dies). The server
time comes from AgpmTimer fields 2 and 3.

## Siege wars (types 0x49, 0x50 and 0x61)

**AgpmSiegeWar (0x49)**, 2-byte flags: 0 op, 1 result (op 2) or flag, 2 castle (mini), 3 CID (refused unless it is the
session's), 4 guild page, 5 siege object CID, 6 object sheet, 7 inventory item IID, 8 siege message, 9 U64 never read,
10 countdown. Castle mini (2-byte flags): 0 name (memory block, key of every packet), 1 master of the owning guild, 2
owning guild, 3 U8 members, 4 and 5 U64 last and next siege (FILETIME in the local time of `World.TimeZone`), 6 U8
status, 7 and 8 U8 attacker and defender resurrection, 9 U8 (0), 10 guild of the last carving. Texts are sent with
their trailing zero (an empty text clears the previous value). Guild page: I16 page, I16 page count (n / 15 + 1),
PACKET[15] {name, master, U32 points}. Siege message: U8 code, castle, guild, second name, U8 object type, U32 (0), U8
side (0 west, 1 east). Countdown: U8 kind (1 before the start, 2 before the end), U16 seconds.

| Op | Direction | Effect |
|---|---|---|
| 0 | server | Full castle: when entering the game (all 5), on every change, at a commander |
| 1 → 2 | client → server | Declaration (castle field 5: date): 0x0B set, 1 status ≠ 0, 2 outside the slot, 3 not the owner |
| 3, 5 → 2 | client → server | Defense (8), attack (9) application; 2 outside applications, 4 not the master, 5 guild too small or too young, 6 already listed, 0x15 list full, 1 not on site |
| 4, 6 → 2 | client → server | Withdrawal (0) |
| 7 → 9, 8 → 10 | client → server | Page of attack applicants (chosen ones from status 2), of defense applicants |
| 0xB → 2 | client → server | The owner picks a defender (castle field 2): 10, ops 0xE and 0xF to everyone |
| 0xC → 0xE, 0xD → 0xF | client → server | Chosen attackers; defenders (owner first); also to everyone when applications close and at the start |
| 0x10 → 0x13 | client → server | Open a camp or catapult (field 5): result 0x0E and sheet (field 6) |
| 0x16 → 0x18, 0x1A; 0x17 → 0x1A | client → server | Load, unload an inventory item (field 7) |
| 0x12, 0x14 → 2 | client → server | Activate (0x13 already active, 1 items missing), repair (0x0D, 0x0F, 0x14) |
| 0x1B → 2 | client → server | Throne carving (field 5): 0x10 start, 0x11 cancelled (moving, dying), 0x12 done, 1 refusal |
| 0x1C | server | Message (to everyone): 1 start, 4 object destroyed, 5 seal released, 6 and 7 Eye opened, closed, 8 to 10 carving, 11 end, 12 to 17 camps |
| 0x1D | server | Countdown, every minute of the battle |

**AgpmEventSiegeWarNPC (0x50)**, 1-byte flags: 0 op, 1 EVENT_BASE, 2 CID. Client op 1 (siege commander, NPC function 33
without data) → op 0 of the castle of the NPC's region, then op 2 (UI_SiegeWarMain window).

**AgpmWarInfo (0x61)**, 1-byte flags: four INT8 read raw at 0x14 (op), 0x15 (type: 0 siege, 1 Battleground, 2 Shrine,
3 Archlord), 0x16 (step), 0x17 (parameter), followed by the op's data: 0 step (warinfodata.txt), 2 counter, 4 and 5 two
INT32 at 0x18 and 0x1C, 6 clear, 7 three 0x21-byte texts at 0x16, 0x37, 0x58 ("%s" arguments). Sieges: steps 1 start,
2 and 3 outer and inner gates, 4 seal, 5 and 6 Eyes, 7 carving, 8 carving done (op 7: guild, master, castle), 9 end
(op 7: guild, master, castle); ArchCore's choices to check on screen: op 2 = minutes left (every minute), op 4 = (0,
length in seconds) at the start, op 5 = (destroyed objects, total); op 6 30 s after the end. Sent to the players in the
castle's regions and to the members of the siege's guilds.

**AgpmCharacter (0x02) ops 0x13 and 0x14** (field 1: CID): siege resurrection at the nearest "binding" point of one's
side (attackers: type 3, while an activated camp stands; defenders: 4 inner or 7 outer, while the revival tower
stands; Rensphere: 3 and 6, 4 and 5).

Client rules applied: statuses 0 to 6 in order (0 no siege, 1 applications, 2 closed, 3 battle, 4 throne seal released,
5 Eyes open, 6 end); declaration by the master of the owning guild (a GM for a castle without an owner) between the
base day at 8 pm and 26 hours later (last siege + Duration4nextSW days, one day earlier on a Sunday; any future date if
there never was one); applications by the master on site (castle and outskirts regions) at status 1; a guild appears
only once (Rensphere apart); object attack order (guard towers and outer gate, then inner gate and revival tower, then
Life Tower, Eyes and throne; a defender only strikes camps and catapults); carving by the master of an attacking guild
(then by any siege guild other than the last carver), at status 5, throne destroyed, without moving for StampingTime
(`Siege.CarveSeconds`).

ArchCore's choices: see `world/siege/` and [[Configuration|EN-Configuration]] (`Siege.*`) — castles start without an
owner; date set automatically at the first slot (base day or next day, 8 pm) if the owner has not declared within the
NextSWDeclaring hours or if there is none; applications open for SelectGuildForSW hours; every attacking applicant is
kept (15 at most, by points); without attackers, the siege is cancelled and the next date set; the guild of the last
carving takes the castle at the end, otherwise the owner keeps it; defeated guilds present are sent back to town.
Objects (`monster_spawn.siege_war` 1) and guards (2) are hidden outside the battle, tied to their castle by the region
of their spawn point; a destroyed object spends 20 s as its "being destroyed" variant then stays in ruins until the
end, without experience or loot; an Archon's Eye brought to 0 HP opens (awakened Eye), and closes by itself after
ArchonEyeOpenTime if the other one is not open. During the battle, in the castle's regions: guilds of opposite sides
are enemies without Ctrl, same side or allies never, no murderer points or bounty. Catapult: a shot removes
`Siege.CatapultDamagePercent` of the HP of the nearest outer gate or guard tower; camp: once activated, it opens the
attackers' resurrection; required items from `SiegeWarObject.txt`, paid repairs with a duration. `SpecialType` portals
2 (castle trips): owning guild outside sieges (free without an owner), siege guilds during the battle; 3 (lord's
hunting ground): owning guild; 4 (Rensphere entrance): guilds owning a castle during the Rensphere war. The Rensphere
war is run by the ArchLord title (below). Guard towers and guards only attack attackers on their own and never target a
defender.

## ArchLord title (type 0x55)

**AgpmArchlord (0x55)**, 2-byte flags: 0 op, 1 CHAR[32] ArchLord's name, 2 CHAR[32] guard's name, 3 INT8 result, 4 INT32
step, 5 INT32 active guards, 6 INT32 maximum guards, 7 INT32 message. The client only handles ops 0 to 6 and only sends
op 7, reduced to the operation.

| Op | Direction | Role |
|---|---|---|
| 0 | server | ArchLord set (field 1): to everyone on each change, on entering the game (sysstr 630) |
| 1 | server | End of reign (field 1), to everyone, before the next one's op 0 (631) |
| 2 / 3 | server | Guard called / dismissed: guard's name (field 2), empty when refused (638 to 641) |
| 4 | server | Active and maximum guards (fields 5 and 6; 650) |
| 5 | server | Rensphere war step (field 4): 1 portal, 2 fight, 3 ArchLord battle, 4 end (642 to 645); to everyone, and on entering the game during the war |
| 6 | server | Message (field 7): 1 guard activation unavailable, 2 deactivation unavailable (647, 648) |
| 7 | client → server | Function 36 event: logged (no client object carries one) |

Chat commands read by the server: `/guardactivate`, `/guarddeactivate`, `/guardinformation` (ops 2, 3 and 4; op 6 to
anyone who is not the ArchLord), `/archlord <message>` (the ArchLord's message to everyone, "[ArchLord] name: message")
and, for GMs, `/archlord set <name>|clear`, `/archlordbegin`, `/archlordend`, `/nextstep`, `/archlordactivate <name>`,
`/archlorddeactivate`, `/castleowner <castle> <guild>`, `/deletecastleowner <castle>`.

Rensphere war (castle 0 of SiegeWar.txt): every Duration4nextSW days (30) at SWBeginTime (8 pm) when `Siege.Enabled` is
1, or with `/siege start Rensphere`. ArchCore's choices: attackers = guilds owning another castle (`SpecialType` 4 portal
open), defenders = the guild holding Rensphere; step 1 at the start, step 2 `Archlord.PortalMinutes` later; carving
allowed from the start (Archon Fountain, type 6, by the master of an attacking guild); seal carved: status 7 for half of
SWPlayingTime at most (no resurrection at Rensphere), SiegeWar.txt's seal skill (930) given to the carver, step 3; at the
end (step 4), the master of the seal's guild becomes ArchLord until the next war (`archlord` table), otherwise the
current ArchLord stays. Without attackers at the date, the war is put off by one period.

ArchLord skills (`skill_type` 9 of SKILL_CONST): given in memory to the ArchLord (AGPMSKILL op 0, also on entering the
game), refused to any other player (refusal op 7 and a system message). Call Dragon (474, `ride`): AgpmRide op 6 (field
2 = 1295 Alcadia) to him and around, op 4 on the next cast, on death or at the end of the reign; Call Royal Knight (475)
and `/guardactivate`: forced combat summons (AgpmSummons type 1, field 8 = 1) of the Royal Knight (1277, 600 s),
`Archlord.MaxGuards` at most; Rain Control (522) and Snow Control (523): AgpmEventNature 3 or 5 to everyone for the
skill's duration (5 min), then the previous weather; Day and Night (478): AGPMTIMER to everyone with the game time
shifted by 12 hours for `Archlord.SkyMinutes`, then restored; the others (472, 473, 476, 711 to 719) go through the
ordinary skill engine.

## Region taxes (type 0x53)

**AgpmTax (0x53)**, 2-byte flags: 0 op, 1 CID (the client ignores an unknown CID), 2 result, 3 EVENT_BASE (ops 0 and 1)
or castle, 4 to 7 regions. Castle mini (2-byte flags): CHAR[128] name (SiegeWar.txt's), INT64 total income paid, UINT64
last payout, UINT64 last rate change (Unix seconds). Region mini (2-byte flags): CHAR[33] name, INT64 income since the
last payout, INT32 rate.

| Op | Direction | Role |
|---|---|---|
| 0 → 1 | client → server | Steward (NPC function 34, in range): op 1 with the EVENT_BASE; the client only opens UI_TaxSteward for the castle's master |
| 3 → 3 | client → server | Castle taxes (field 3: name): result 0, castle and regions to the owner; 1 otherwise |
| 2 → 2 | client → server | New rates (regions: name, rate): 0 done, 1 not the owner, 3 outside 0 to 50, 2 already changed this week (Monday to Sunday, `World.TimeZone`); castle and regions sent back |
| 4 | server | One region's rate (field 4): all of them on entering the game, to everyone on each change |

A region's name is the one the client builds: the RegionTemplate.ini name translated by the player's language
`RegionGlossary.txt` (`region_glossary` table; otherwise English, then Korean): it is the key of its price table. Taxed
regions: the castle's and its outskirts' (`siege_war`).

Tax = price × rate / 100 (integer division), added to NPC purchases, repair (item by item), the stylist (line by line),
portals (departure region), the skill master, production (skill, recipe) and guild creation; neither sales to NPCs nor
trades and sales between players. ArchCore's choices: the tax goes to the income of the region the player stands in; a
castle without an owner does not tax; a new owner starts from 0 rates; every `Tax.PayoutDay` at `Tax.PayoutHour`
(Monday midnight), the income goes into the bank of the owning guild master's account, minus a 20 % tribute to the
ArchLord's bank (except at Rensphere; without an ArchLord, the owner keeps everything), written to `log_bank` (`taxe`,
`tribut`, `count` = castle); `/tax pay` pays at once.

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
| 0x4A | Player versus player | 0x2A | Remission |
| 0x32 | NPC shops | 0x37 | NPC dialog |
| 0x3B | Skill master | 0x36 | Bank |
| 0x31 | Teleport portals | 0x15 | Guilds |
| 0x39 | Guild manager | 0x16 | Item conversion |
| 0x38 | Blacksmith (conversion) | 0x4B | Refining |
| 0x3F | Alchemy sphere (refining) | | |
| 0x07 | Trading between players | 0x3E | NPC quests |
| 0x17 | Production | 0x3A | Production NPCs |
| 0x2B | Friends and ignore list | 0x2D | Mail |
| 0x34 | Item repair | 0x4D | Mounts |
| 0x0B | Auction house | 0x6A | Equipment switch |
| 0x30 | Weather (AgpmEventNature) | 0x47 | Nature effects |
| 0x4F | Summons (AgpmSummons) | 0x4C | Area shouts (AgpmAreaChatting) |
| 0x2C | Chat rooms (AgpmChannel) | 0x43 | Stylists (AgpmEventCharCustomize) |
| 0x46 | Wanted posters (AgpmWantedCriminal) | 0x58 | Battleground announcements (AgcmBattleGround, raw) |
| 0x62 | Shrine battles (AgpmShrineBattle, raw) | 0x64 | Boss window (AgpmBoss) |
| 0x49 | Siege wars (AgpmSiegeWar) | 0x50 | Siege commanders (AgpmEventSiegeWarNPC) |
| 0x61 | War information (AgpmWarInfo) | 0x55 | ArchLord title (AgpmArchlord) |
| 0x53 | Region taxes (AgpmTax) | | |

Full schemas for the 86 modules are in `data/re/packet-schemas.json`.
