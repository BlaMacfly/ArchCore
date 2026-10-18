# Tools

All tools live in `tools/` and run with Node.js from the ArchCore folder.

| Tool | Purpose |
|---|---|
| `db-init.mjs` | Creates the databases and applies `sql/`. With `--account name pass [gm]`, creates an account |
| `decrypt-client-data.mjs` | Decrypts `Game/ini` (RC4, MD5("1111")) into `data/client-ini` |
| `import-client-data.mjs` | Imports `data/client-ini` into `archcore_world` (CP949 → UTF-8) |
| `client-sim.mjs` | Test client: replays the full login up to game entry |
| `i18n/strings.mjs` | Lists texts still in English in `data/i18n/pending-en.json` |
| `i18n/coverage.mjs` | Measures how much of `ini_fr` is translated |
| `i18n/apply.mjs` | Applies the translation memory to the client (see [[Translation|EN-Translation]]) |
| `re/extract-schemas.mjs` | Extracts packet schemas from Ghidra pseudocode into `data/re/packet-schemas.json` |

## Client simulator

```bash
npm run sim -- <user> <password> [character]
```

It prints every packet exchanged and the number of NPCs received, and ends with "flux complet OK"
(full flow OK). It is the fastest way to check a server change without starting the game.

## Two-player test

```bash
node tools/sim-move.mjs [userA] [passA] [charA] [userB] [passB] [charB]
```

Two simulated clients enter the game (accounts `sima` and `simb` by default, characters created
when needed). The test checks that they see each other, that one player's movement reaches the
other, that they lose sight of each other out of range, and that disconnecting removes the
character. It moves characters and saves their position, so run it against a test server:

```bash
ARCHCORE_CONFIG=config/test.json node src/index.js all      # other ports, other databases
ARCHCORE_CONFIG=config/test.json node tools/sim-move.mjs
```

## Shop test

```bash
ARCHCORE_CONFIG=config/test.json npm run sim:shop -- [user] [password] [character]
```

The character (`sima` / `SimA` by default) gets 1,000 gold and appears next to Darryl (Anchorville).
The test opens his shop, checks the list it receives, buys 3 small life potions and sells them back.
It changes the character's gold, position and inventory, so run it against a test server as well.

## Reverse engineering

The findings were made with [REA](https://github.com/morluto/rea) and Ghidra 12. The approach is
described on the [[Reverse engineering|EN-Reverse-engineering]] page.
