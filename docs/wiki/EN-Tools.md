# Tools

Every tool is a subcommand of the `archcore` executable (see [[Installation|EN-Installation]]). Run
them from the repository root; they read `config/archcore.json`, or the file named by `ARCHCORE_CONFIG`.

| Command | Role |
|---|---|
| `archcore db-init [--account name pass [gm]]` | Creates the databases and applies `sql/`; with `--account`, creates or updates an account |
| `archcore decrypt [source] [destination]` | Decrypts `Game/ini` (RC4, MD5("1111")) into `data/client-ini` |
| `archcore import [folder]` | Imports `data/client-ini` into `archcore_world` (CP949 → UTF-8) |
| `archcore sim client\|move\|shop …` | Simulated clients (see below) |
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

## Unit tests

```bash
cargo test
```

## Reverse engineering

The findings were made with [REA](https://github.com/morluto/rea) and Ghidra 12. The approach is
described on the [[Reverse engineering|EN-Reverse-engineering]] page.
