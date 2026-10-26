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
midnight of the epoch = 0), so that the game hour is the server's hour; `World.TimeZone = utc` in
the [[configuration|EN-Configuration]] sends the raw UTC Unix epoch instead. Fields 2 and 3 (u32) are
unused. `archcore sim` prints the received value and the matching game hour.

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
  as `ATK_Speed_bonus`, `Movement_Run_bonus` and `*_Percent` always do. Triggered columns (Critical, skill_*,
  bonus_*, Ignore_*, on_hit_*…) are not applied yet.
- **Linked options**: a unique item lists option groups (`Link_id` in itemdatatable.txt, separated by ";"); for
  each group the wearer gets the option whose `Level_min` ≤ their level ≤ `Level_max`. Nothing is sent: the
  client finds them itself for the tooltip.

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

Not handled yet: runes that grant a skill (`Skill_Name`), Archonia's ice, lightning and venom stones and stat
stones (`Spirit Type` 6 to 8 and 100 to 106, refused by the client).

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

Still to do: ignored defense (bits 4 and 8 of `reflect_melee_attack`), skill criticals, dispels, damage
sharing (`divide`: Divine Protection, Martyr's Mark); earning heroic and charisma points, which heroic skills
also require.

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

## Guilds (types 0x15 and 0x39, AgpmGuild and AgpmEventGuild)

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

Creating a guild takes level 20 and the price of the first `guildmaxmember.txt` row (50 places for
10,000 coins). Codes used: 0 name taken, 1 level, 2 money (number: price), 4 characters, 5 already a
member, 6 guild full, 7 cannot join, 8 cannot leave (master), 9 master only, 10 wrong password, 11 created,
12 disbanded, 13 joined, 14 refused, 15 left, 16 kicked, 17 player already in a guild, 48 join request,
50 leave request.

On entering the world, the member gets its guild (op 0), each member (op 3) and its link (op 8). Guild
managers (12 NPCs) carry function 23, with no data; AGPMEVENTGUILD (0x39): op 0 (event) ← op 1 (grant, the
client opens guild creation). Chat: type 3 goes to the guild members, type 4 to the party members.

Not done yet: guild battles, alliances and hostilities, guild marks, sub-master and handover, enlarging, guild
list, guild warehouse.

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
invitations (0x04 guild battle, 0x08 battle, 0x40 alliance or hostility, 0x80 buddy wait for their system). The
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
| 0x3B | Skill master | 0x36 | Bank |
| 0x31 | Teleport portals | 0x15 | Guilds |
| 0x39 | Guild manager | 0x16 | Item conversion |
| 0x38 | Blacksmith (conversion) | 0x4B | Refining |
| 0x3F | Alchemy sphere (refining) | | |
| 0x07 | Trading between players | 0x3E | NPC quests |

Full schemas for the 86 modules are in `data/re/packet-schemas.json`.
