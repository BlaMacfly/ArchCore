# Tools

Every tool is a subcommand of the `archcore` executable (see [[Installation|EN-Installation]]). Run
them from the repository root; they read `config/archcore.json`, or the file named by `ARCHCORE_CONFIG`.

| Command | Role |
|---|---|
| `archcore db-init [--account name pass [gm]]` | Creates the databases and applies `sql/`; with `--account`, creates or updates an account |
| `archcore decrypt [source] [destination]` | Decrypts `Game/ini` (RC4, MD5("1111")) into `data/client-ini` |
| `archcore import [folder] [--server folder]` | Imports `data/client-ini` into `archcore_world` (CP949 → UTF-8); with the original server tables (`--server`, or `data/server-ini` if it exists), also imports monster spawns and the server map |
| `archcore sim client\|move\|shop\|addon\|combat\|equip\|potion …` | Simulated clients (see below) |
| `archcore i18n strings [Game]` | Lists the texts still in English in `data/i18n/pending-en.json` |
| `archcore i18n coverage [Game]` | Measures how much of `ini_fr` is translated |
| `archcore i18n apply [--ascii] [--dry]` | Applies the translation memory to the client (see [[Translation|EN-Translation]]) |
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
ARCHCORE_CONFIG=config/test.json archcore all      # other ports, other databases
ARCHCORE_CONFIG=config/test.json archcore sim move
```

## Shop test

```bash
ARCHCORE_CONFIG=config/test.json archcore sim shop [user] [pass] [character]
```

The character (`sima` / `SimA` by default) gets 1000 gold and appears next to Darryl (Anchorville).
The test opens his shop, checks the list, buys 3 small life potions and sells them back. It changes
the character's gold, position and inventory: run it against a test server too.

## Combat test

```bash
ARCHCORE_CONFIG=config/test.json archcore sim combat [user] [pass] [character] [monster]
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
it in about fifteen seconds.

## Equipment test

```bash
ARCHCORE_CONFIG=config/test.json archcore sim equip [user] [pass] [character]
```

A GM account (`gmtest` / `GmTest` by default) creates a short sword, a human warrior armour (level 2) and
a mage wand with `/create`, wears the sword (damage 11 to 13) then the armour (defense 4), sees the wand
refused (class), then puts both away. SimA, standing nearby, must see the sword appear on them and then
disappear. The created items stay in the inventory.

## Potion and regeneration test

```bash
ARCHCORE_CONFIG=config/test.json archcore sim potion [user] [pass] [character]
```

A GM account (`gmtest` / `GmTest` by default) creates 3 Small Life Potions, gets wounded by a Green Worm,
becomes invincible and goes back to the village; the test waits for out-of-combat regeneration (HP rising
about ten seconds after the last hit), drinks a potion (HP up, stack down, op 4), then checks that a second
potion right away is refused.

## ArchRust extensions test

```bash
ARCHCORE_CONFIG=config/test.json archcore sim addon [userA passA charA userB passB charB userC passC charC]
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

## Original server tables

Monster spawns and the server map are not in the client: they come from the original ArchLord server
tables, to be placed in `data/server-ini` (never committed) or pointed to with `--server`:

| File | Content | Table |
|---|---|---|
| `spawndatatable.txt` | What: group, monster template and level, count, respawn delay, siege war | `monster_spawn` |
| `spawninstancetable.txt` | Where: position, orientation, radius of each point (a name may have several positions) | `monster_spawn` |
| `world/node_data` (next to it or in the sibling folder) | Map: 16 × 16 segments of 400 units per sector, blocked ground and region | `map_segment` |
| `itemdroptable.txt` | Each monster's own items: count, rate out of 100,000 | `monster_drop` |
| `groupdroprate.txt` | Drop groups: rate, `DropMeditation` sensitivity, level bonus | `drop_group` |
| `droprankrate.txt` | Item ranks given by the monster's drop rank | `drop_rank_rate` |

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
