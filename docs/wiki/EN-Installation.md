# Installation

## Requirements

- **Node.js 20 or later** (tested with version 24).
- **MariaDB** 10.6 or later (tested with 13.0). On Windows: `winget install MariaDB.Server`.
- An extracted **64-bit ArchLord client**, by default in `../client/ArchLord/Game` relative to the ArchCore folder.
  ArchCore ships no game data.

## 1. Get the code

```bash
git clone https://github.com/BlaMacfly/ArchCore.git
cd ArchCore
npm install
```

## 2. Start MariaDB

`db/start-mariadb.cmd` starts a local instance whose data lives in `db/data` (port 3306, user
`root`, password `archcore`). An existing MariaDB server works too: put its credentials in
`config/archcore.json` (see [[Configuration|EN-Configuration]]).

## 3. Create the databases and an account

```bash
node tools/db-init.mjs --account test test
```

This creates `archcore_auth`, `archcore_characters` and `archcore_world`. It applies the `sql/`
scripts, then creates the account `test` with password `test`. A game master level can be given
as a fourth argument.

## 4. Import the client data

```bash
node tools/decrypt-client-data.mjs     # Game/ini → data/client-ini (plain text)
node tools/import-client-data.mjs      # data/client-ini → archcore_world
```

The client tables are RC4-encrypted, with a key derived from MD5("1111"), and written in CP949.
The import converts them to UTF-8: character templates, experience, NPCs, items, skills.

## 5. Run the server

```bash
npm start                    # login (11002) + world (11008)
npm run login                # login only
npm run world                # world only
ARCHCORE_TRACE=1 npm start   # detailed log of every packet
```

## 6. Check without the game

```bash
npm run sim -- test test Testeur
```

The simulator replays the handshake, login, character list and world entry. It should end with
« entrée en jeu confirmée — flux complet OK », meaning the full flow worked.

## 7. Connect the real client

The client reads its server list from an encrypted `.ini` file next to its executable. It first
reads, when present, a plain-text file with the same name followed by `Test` (for example
`<ClientName>Test.ini`). Create that file in `Game/` with this content:

```xml
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<IPInfo>
  <ServerGroup><GroupName>ArchCore</GroupName><IP>127.0.0.1:11002</IP></ServerGroup>
</IPInfo>
```

The client runs with administrator rights. Type the login in the game window yourself: other
programs cannot send it keystrokes.

To play in French, write `17` in `Launcher/LanguageInfo.txt` (see [[Translation|EN-Translation]]).

## Troubleshooting

| Symptom | Lead |
|---|---|
| "Cannot access the login server" | ArchCore is not listening on 11002, or the `<ClientName>Test.ini` file is missing |
| Endless loading on world entry | Run with `ARCHCORE_TRACE=1` and read `data/archcore.log` |
| No sound | Check `[Sound_Option]` in `Game/ini/COption.ini`; the game rewrites this file on exit |
