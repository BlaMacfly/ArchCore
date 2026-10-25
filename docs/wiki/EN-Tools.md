# Tools

Every tool is a subcommand of the `archcore` executable (see [[Installation|EN-Installation]]). Run
them from the repository root; they read `config/auth.conf` and `config/world.conf`, or those of the folder
named by `ARCHCORE_CONF_DIR` (see [[Configuration|EN-Configuration]]).

| Command | Role |
|---|---|
| `archcore config migrate <archcore.json> [folder] [--force]` | Converts a former JSON configuration into `auth.conf` and `world.conf` |
| `archcore db-init [--account name pass [gm]]` | Creates the databases, replays the baseline and applies the missing migrations; with `--account`, creates or updates an account |
| `archcore db-check` | Read-only consistency check: schema versions, references between databases, identifier ranges; exits with an error on a serious anomaly (see [[Database\|EN-Database]]) |
| `archcore decrypt [source] [destination]` | Decrypts `Game/ini` (RC4, MD5("1111")) into `data/client-ini` |
| `archcore import [folder] [--server folder]` | Imports `data/client-ini` into `archcore_world` (CP949 → UTF-8) and the ground height from the Game folder's `world/b*x.ma2` (`map_height`, a few minutes); with the original server tables (`--server`, or `data/server-ini` if it exists), also imports monster spawns and the server map; finally applies the `content/*.sql` content layer and records each source in `import_run` |
| `archcore content` | Applies the `content/*.sql` content layer to the world database again, without reimporting (see [[Database\|EN-Database]]) |
| `archcore sim client\|move\|shop\|addon\|lang\|combat\|equip\|potion\|scroll\|patrol\|bag\|skill\|effects\|bank\|portal\|guild\|gm\|trade\|quest\|mobskill\|counter\|partyskill\|mastery …` | Simulated clients (see below) |
| `archcore i18n strings [Game]` | Lists the texts still in English in `data/i18n/pending-en.json` |
| `archcore i18n coverage [Game]` | Measures how much of `ini_fr` is translated |
| `archcore i18n apply [--ascii] [--dry]` | Applies the translation memory to the client (see [[Translation|EN-Translation]]) |
| `archcore i18n db [language]` | Fills `localized_text` (world database) for a language: translated NPC titles and the `data/i18n/<language>.json` memory (see [[Translation|EN-Translation]]) |
| `archcore re extract-schemas <files…>` | Extracts packet schemas from Ghidra pseudocode into `data/re/packet-schemas.json` |

## Client simulator

```bash
archcore sim client <user> <password> [character] [commands…]
```

It prints every packet exchanged, the number of NPCs received, and ends with "flux complet OK".
Arguments after the character are sent as chat once in game (`/GetExp` by default), which tests GM
commands. It is the quickest way to check a server change without starting the game.

In Git Bash, prefix with `MSYS_NO_PATHCONV=1`, otherwise `/GetExp` is turned into a Windows path.

## Two-player test

```bash
archcore sim move [userA] [passA] [charA] [userB] [passB] [charB]
```

Two simulated clients enter the world (accounts `sima` and `simb` by default, characters created as
needed). The test checks that they see each other, that one's movement is relayed to the other, that
they lose sight of each other out of range, and that disconnecting removes the character. It moves
the characters and saves their position: run it against a test server, for example:

```bash
ARCHCORE_CONF_DIR=config/test archcore all      # other ports, other databases
ARCHCORE_CONF_DIR=config/test archcore sim move
```

The test server has its own three databases, the world included: otherwise `archcore db-init` run for it
would also apply the world migrations to the main server. To create its world database, copy the main
server's (`CREATE DATABASE archcore_test_world`, then `mariadb-dump archcore_world | mariadb
archcore_test_world`), set it in `Database.World`, also give it its own log database (`Database.Logs`, for example `archcore_test_logs`,
created by `db-init`), and run `ARCHCORE_CONF_DIR=config/test archcore db-init`.

## Shop test

```bash
ARCHCORE_CONF_DIR=config/test archcore sim shop [user] [pass] [character]
```

The character (`sima` / `SimA` by default) gets 1000 gold and appears next to Darryl (Anchorville).
The test opens his shop, checks the list, buys 3 small life potions and sells them back. It changes
the character's gold, position and inventory: run it against a test server too.

## Combat test

```bash
ARCHCORE_CONF_DIR=config/test archcore sim combat [user] [pass] [character] [monster]
```

The character, a GM account (`gmtest` / `GmTest` by default), goes hunting the requested monster
(`Green Worm` by default) north of Anchorville (Limelight North). The test has two parts:

1. **Defeat**: it attacks; the monster fights back; it moves 1500 units away and the monster chases it;
   it dies, asks to be resurrected and comes back to the village with full HP.
2. **Victory**: with `/invincible 1`, it attacks another monster, which no longer wounds it; the monster
   dies, experience is gained, its loot falls on the ground (at least gold) and the character picks it
   all up; the corpse disappears and the monster respawns.

It then sets `/invincible 0` and brings the character back to the village. Allow about 2 min: a
level 1 warrior without a weapon hits a Green Worm (136 HP) for 2 to 3 HP per hit, and the worm kills
it (120 HP) in about twenty seconds.

## Equipment test

```bash
ARCHCORE_CONF_DIR=config/test archcore sim equip [user] [pass] [character]
```

A GM account (`gmtest` / `GmTest` by default) creates a short sword, a human warrior armour (level 2) and
a mage wand with `/create`, wears the sword (damage 11 to 13) then the armour (defense 4), sees the wand
refused (class), then puts both away. SimA, standing nearby, must see the sword appear on them and then
disappear. The created items stay in the inventory.

## Potion and regeneration test

```bash
ARCHCORE_CONF_DIR=config/test archcore sim potion [user] [pass] [character]
```

A GM account (`gmtest` / `GmTest` by default) creates 3 Small Life Potions, gets wounded by a Green Worm,
becomes invincible and goes back to the village; the test waits for out-of-combat regeneration (HP rising
about ten seconds after the last hit), drinks a potion (HP up, stack down, op 4), then checks that a second
potion right away is refused.

## Scroll and patrol tests

```bash
ARCHCORE_CONF_DIR=config/test archcore sim scroll [user] [pass] [character]
ARCHCORE_CONF_DIR=config/test archcore sim patrol [user] [pass] [character]
```

`sim scroll` (GM account `gmtest` by default) creates three scrolls and checks the Tullan passport (fixed
destination), the round trip (village, then back to Tullan with op 6) and the plain passport (village).
`sim patrol` (`sima` by default) stands still among the Green Worms and waits for monsters to wander
(walking), then goes back to the village.

## Inventory and resurrection test

```bash
ARCHCORE_CONF_DIR=config/test archcore sim bag [user] [pass] [character]
```

A GM account (`gmtest` by default) creates a new stack of Small Mana Potions, splits it (op 17), merges it
back (op 2 onto the original stack's slot) then destroys it (op 21); finally `/kill` and resurrection: it
must come back at Anchorville's resurrection point (-476000, -61000).

## Bank and trading test

```bash
ARCHCORE_CONF_DIR=config/test archcore sim bank [user] [pass] [character]
ARCHCORE_CONF_DIR=config/test archcore sim portal [user] [pass] [character]
ARCHCORE_CONF_DIR=config/test archcore sim trade [userA passA charA userB passB charB]
ARCHCORE_CONF_DIR=config/test archcore sim party [userA passA charA userB passB charB]
ARCHCORE_CONF_DIR=config/test archcore sim guild [userA passA charA userB passB charB]
ARCHCORE_CONF_DIR=config/test archcore sim gm [user] [pass] [character]
```

`sim bank` (GM account `gmtest`) checks that depositing without a keeper is refused, opens the bank at
Holly (south of Anchorville), buys the second page (1,000,000 coins) and is refused the next one, deposits a
potion stack and 1000 coins, reconnects and finds them, then withdraws them and destroys the stack.
`sim portal` (GM account `gmtest`) stands 2500 units from the Anchorville portal and addresses it: the
server makes it run then grants; paid trip (100 coins) to the Scout's Grounds, unlinked trip refused, paid
return (500 coins) to Anchorville. `sim trade` puts GmTest (GM) and SimA at the same spot: GmTest offers
a potion stack and 500 coins, SimA 100 coins; the offered item cannot be destroyed; locks, exchange (SimA
receives the stack, money is adjusted on both sides), then a second request refused. `sim party` (SimA and
SimB) checks that inviting an unknown name is refused, the invitation and acceptance (party and member list
on both sides), party chat, the leader setting experience sharing (refused to the other), the leader handover,
then the kick, which disbands the party. `sim guild` (GmTest master, SimA) refuses a name with a space,
creates SimGuild (10,000 coins), invites SimA who accepts, checks guild chat and the notice, a leave request
accepted by the master, coming back then being kicked, the guild found again on reconnecting, and finally
disbanding (wrong password refused). `sim gm` (GM account `gmtest`) checks `/speed` (run speed ×5 in the
factors received, refused above 10, back to ×1) and `/additem` (potions added, unknown TID refused), then
destroys the stack.

## Quests test

```bash
ARCHCORE_CONF_DIR=config/test archcore sim quest [user] [pass] [character]
```

The GM account (`gmtest`), raised to level 120 (`/questreset` first resets the test quests), takes Shattered
Roots from the journal, is refused completing it without the 50 Corrupted Root Cores, receives them
(`/create`), completes it (items taken, Tempering Gem received), is refused taking it again and taking Winged
Menace (level 25 to 40), takes then abandons Gladiator's Demise; after reconnecting, Shattered Roots is among
the completed quests and the abandoned quest is no longer in the journal.

## Skills test

```bash
ARCHCORE_CONF_DIR=config/test archcore sim skill [user] [pass] [character]
```

A GM account (`gmtest` by default, human warrior) checks that it knows "Normal Attack", goes to level 10
(`/level`), walks to Black, Anchorville's warrior trainer, learns Dual Slash (or is told "already learned"),
is refused Flame Core (a mage skill), upgrades Dual Slash, casts it on a Green Worm (cast, damage), checks
the refusal during the recast delay, then goes back to its original level.

```bash
ARCHCORE_CONF_DIR=config/test archcore sim effects [user] [pass] [character]
```

The GM account, with a separate character (`GmEffets`, created if needed, so as not to strengthen the one
used by the other tests), is raised to level 25 (at least 456 HP and 108 MP, from GrowUpFactor) and learns Speed Attack,
Defense Hold, Linear Blow and Blade Sting from Black (if needed). It checks the passive (defense +20), the buff (effect announced on itself, attack speed
+18 %), Linear Blow's area (target and neighbours hit) and Blade Sting's debuff (set on the Green Worm,
removed after 6 s). `/level` refills MP between casts.

```bash
ARCHCORE_CONF_DIR=config/test archcore sim status [user] [pass] [character]
```

The same character (`GmEffets`, level 25) learns Shield Bash from Black if needed, then casts it on Green
Worms, with `/recast` and `/level` between tries, until a stun lands (30 % chance, 25 tries at most): the
worm switches to state 0x1 (AGPMCHARACTER op 1, field 17), then back to 0 after 3 s.

```bash
ARCHCORE_CONF_DIR=config/test archcore sim mastery [user] [pass] [character]
ARCHCORE_CONF_DIR=config/test archcore sim partyskill [userA passA charA userB passB charB]
ARCHCORE_CONF_DIR=config/test archcore sim counter [user] [pass] [character]
ARCHCORE_CONF_DIR=config/test archcore sim mobskill [user] [pass] [character]
ARCHCORE_CONF_DIR=config/test archcore sim convert [user] [pass] [character]
```

`sim mastery` (GmTest, level 60) forgets Capacitate and Marching Song in the database, is refused Capacitate
at Black's ("Marching Song at level 4 required"), learns Marching Song, upgrades it three times, then learns
Capacitate. `sim partyskill` (GmTest, given Marching Song in the database, and SimA) forms a party: Marching
Song strengthens SimA next to GmTest (AGPMSKILL op 4 on SimA), not when SimA is 3000 units away. `sim counter`
gives GmTest Counter Attack level 10 in the database (a berserker skill, removed at the end), sets it to level
1, provokes a Green Worm and waits for a hit sent back (attacker = player, result 12), `/level 1` restoring HP
when needed. `sim mobskill` (GmTest, level 40) joins Midas, hits it once and waits for it to cast Insanity
(the monster's op 7, op 8 on the player with physical and water damage), then to cast it again 6 s later.
`sim convert` (GmTest, GM account) opens the window of Halo the blacksmith in Anchorville, gets a Bastard
Sword (with 1 to 3 options and its sockets rolled), a water stone and two reinforcement potions (`/additem`),
adds a socket (repeated until it succeeds),
sets the stone (op 7, result 0) then an attack prayer stone (rune, op 8, result 1), reinforces the sword to +1 then +2, logs in again and reads the state back
(+2, sockets, stone and rune) from the items received on entry, then destroys the sword.

## ArchRust extensions test

```bash
ARCHCORE_CONF_DIR=config/test archcore sim addon [userA passA charA userB passB charB userC passC charC]
```

Three simulated clients enter the world at the same spot: A and B (`sima` / `SimA`, `simb` / `SimB`)
negotiate the extensions on every connection as ArchRust will, C (`gmtest` / `GmTest`) behaves like the
original client and must be a GM (`archcore db-init --account gmtest gmtest 3`). The test checks:

- an AddOn whisper from A to B (sender filled in by the server, payload intact, no echo for A);
- a `SAY` message, received by A and B;
- every refusal: recipient without extensions, offline recipient, reserved `AC.` prefix, invalid prefix,
  256-byte payload, unsupported party channel, player under `/chatban`;
- that an AddOn message sent without negotiation is ignored;
- the rate limit: out of a burst of 25 messages, 20 go through and 5 are refused;
- that C never receives a `0xAC` packet.

The protocol is described on the [[ArchRust extensions|EN-ArchRust-extensions]] page.

## Language test

```bash
ARCHCORE_CONF_DIR=config/test archcore sim lang [user] [pass] [character]
```

The same character (`sima` / `SimA` by default) enters the world several times with the `language/1`
capability:

- in French (language announced in `HELLO`): `HELLO_ACK` grants `language/1`, applies `fr` and lists the
  offered languages; the NPC names received are recorded;
- in English: NPC names differ ("<Marchand> Darryl" → "<Merchant> Darryl");
- `TEXT_REQUEST` returns the same names as the view packets, then `SET_LANGUAGE fr` brings them back to
  French; an unknown language (`xx-YY`) falls back to `World.Language`, a malformed code and an unknown kind
  get `INVALID_REQUEST`;
- in "automatic" mode (HELLO without a language): `HELLO_ACK` announces the default language, then a
  `LANGUAGE_ACK` on entry gives the account's language (`en`), and names are in English;
- with the original client (no `HELLO`): no `0xAC` packet, names in the account's language;
- finally the account is set back to French.

`archcore db-init` must have been run (`language` and `localized_text` tables, `account.language` column);
`archcore i18n db fr` is optional, the translated NPC titles serving as fallback.

## Original server tables

Monster spawns and the server map are not in the client: they come from the original ArchLord server
tables, to be placed in `data/server-ini` (never committed) or pointed to with `--server`:

| File | Content | Table |
|---|---|---|
| `spawndatatable.txt` | What: group, monster template and level, count, respawn delay, siege war. The level kept is the client's `CharacterDataTable.txt` one when listed there; boss delays come from the client's boss window (`UIBoss.xml`) | `monster_spawn` |
| `spawninstancetable.txt` | Where: position, orientation, radius of each point (a name may have several positions) | `monster_spawn` |
| `world/node_data` (next to it or in the sibling folder) | Map: 16 × 16 segments of 400 units per sector, blocked ground and region | `map_segment` |
| `itemdroptable.txt` | Each monster's own items: count, rate out of 100,000 | `monster_drop` |
| `groupdroprate.txt` | Drop groups: rate, `DropMeditation` sensitivity, level bonus | `drop_group` |
| `droprankrate.txt` | Item ranks given by the monster's drop rank | `drop_rank_rate` |
| `aidatatable.txt` | Monster AI: skills cast (TID;level), HP condition, number of casts, normal hits | `monster_ai` |

```bash
archcore db-init
archcore import data/client-ini --server path/to/ini
```

Rows of `spawndatatable.txt` without a position in `spawninstancetable.txt` are skipped and counted.

## Unit tests

```bash
cargo test
```

## Reverse engineering

The findings were made with [REA](https://github.com/morluto/rea) and Ghidra 12. The approach is
described on the [[Reverse engineering|EN-Reverse-engineering]] page.
