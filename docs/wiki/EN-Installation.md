# Installation

## Requirements

- **Rust 1.85 or later**, installed with [rustup](https://rustup.rs) (tested with 1.99).
- **MariaDB** 10.6 or later (tested with 13.0). On Windows: `winget install MariaDB.Server`.
- An extracted **64-bit ArchLord client**, by default in `../client/ArchLord/Game` relative to the ArchCore folder.
  ArchCore ships no game data.

## 1. Get and build the code

```bash
git clone https://github.com/BlaMacfly/ArchCore.git
cd ArchCore
cargo build --release
```

The build produces a single executable, `target/release/archcore` (`archcore.exe` on Windows). It
holds the server and every tool. Below, `archcore` means that executable; `cargo run --release --`
works too. Run the commands from the repository root, where `config/` lives.

## 2. Start MariaDB

The `db/start-mariadb.cmd` script starts a local instance whose data lives in `db/data`
(port 3306, user `root`, password `archcore`). An existing MariaDB server works too: just set its
credentials (`Database.*`) in `config/auth.conf` and `config/world.conf`, created on first start from
their `.dist` templates (see [[Configuration|EN-Configuration]]).

## 3. Create the databases and an account

```bash
archcore db-init --account test test
```

This creates `archcore_auth`, `archcore_characters` and `archcore_world`, applies the scripts in
`sql/`, then creates the account `test` with the password `test`. A game master level (1 to 3) can be
given as a fourth argument; 3 opens every command (see [[Administration|EN-Administration]]):

```bash
archcore db-init --account admin password 3
```

The same command updates an existing account (password, GM level). It is safe to run again after a code
update: it only adds what is missing.

## 4. Import the client data

```bash
archcore decrypt     # Game/ini → data/client-ini (plain text)
archcore import      # data/client-ini → archcore_world
```

The client tables are RC4-encrypted, with a key derived from MD5("1111"), and written in CP949. The
import converts them to UTF-8: character templates, experience, NPCs, items, shops, skills, quests. It
also reads the ground height from the world geometry (the client's `world/`), which takes a few minutes.

**Monsters, drops and map.** They are not in the client: they come from the original ArchLord server
tables (`spawndatatable.txt`, `spawninstancetable.txt`, `itemdroptable.txt`, `groupdroprate.txt`,
`droprankrate.txt` and `world/node_data`), which ArchCore does not ship. Put them in `data/server-ini`
(never versioned), or point to their folder:

```bash
archcore import data/client-ini --server path/to/ini
```

Without them the server starts, but the world has no monsters. The files are detailed in
[[Tools|EN-Tools]].

## 5. Start the server

```bash
archcore                     # login (11002) + world (11008)
archcore login               # login only
archcore world               # world only
ARCHCORE_TRACE=1 archcore    # detailed log of every packet
```

## Useful settings

Settings live in `config/auth.conf` and `config/world.conf` (all described in
[[Configuration|EN-Configuration]]). The most useful ones to start with, in `world.conf`:

| Key | Purpose |
|---|---|
| `WorldServer.Name` | World name in the server list |
| `WorldServer.PublicAddress` | Address given to players to join the world (see "Playing with others") |
| `World.MonsterDensity` | Monsters per spawn point (1: tables as they are; 2 or 3: more crowded) |
| `Rate.Exp`, `Rate.Gold`, `Rate.Drop` | Experience, gold and drop rates (1: normal) |
| `World.Language` | Language of the NPC titles sent to players (`fr` by default) |

Every key can also come from an environment variable: `World.MonsterDensity` →
`ARCHCORE_WORLD_MONSTER_DENSITY`. A setting change needs a server restart.

## 6. Check without the game

```bash
archcore sim client test test Testeur
```

The simulator replays the handshake, the login, the character list and the world entry. It should
end with "entrée en jeu confirmée", then "flux complet OK".

## 7. Connect the real client

The client reads the server list from an encrypted `.ini` file next to its executable. It first
reads, if present, a plain-text file with the same name followed by `Test` (for example
`<ClientName>Test.ini`). Create that file in `Game/` with:

```xml
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<IPInfo>
  <ServerGroup><GroupName>ArchCore</GroupName><IP>127.0.0.1:11002</IP></ServerGroup>
</IPInfo>
```

The client runs with administrator rights, so type the login in the game window: another program
cannot send it keystrokes.

To play in French, write `17` in `Launcher/LanguageInfo.txt` (see [[Translation|EN-Translation]]).

## Playing with others

By default everything is set up to play on the same machine (`127.0.0.1`). To welcome other players:

1. **World address.** In `config/world.conf`, set `WorldServer.PublicAddress` to the address players use
   to reach the machine: its local network address (for example `192.168.1.20`, given by `ipconfig`) or
   its public address / a domain name for the Internet. The login server hands it to clients to join the
   world.
2. **Firewall.** Allow TCP ports 11002 (login) and 11008 (world). On Windows, in a PowerShell run as
   administrator:

   ```powershell
   New-NetFirewallRule -DisplayName "ArchCore" -Direction Inbound -Protocol TCP -LocalPort 11002,11008 -Action Allow
   ```
3. **Router.** For players outside the local network, forward these two ports to the server machine in
   the router's settings.
4. **Player side.** In `<ClientName>Test.ini`, replace `127.0.0.1` with the server address
   (`<IP>192.168.1.20:11002</IP>`).
5. **Accounts.** Create one account per player with `archcore db-init --account name password`.

## After restarting the machine

Nothing is lost on shutdown: characters, items and positions are in the database. After a restart,
start again in order, each in its own window:

1. MariaDB: `db/start-mariadb.cmd` (not needed if MariaDB is installed as a Windows service);
2. the server: `archcore` from the repository root.

If the machine was cut off abruptly, MariaDB repairs its tables on start ("Crash table recovery
finished"): this is normal.

## Troubleshooting

| Symptom | Lead |
|---|---|
| "Cannot access the login server" | ArchCore is not listening on 11002, or `<ClientName>Test.ini` is missing |
| Endless loading when entering the world | Run with `ARCHCORE_TRACE=1` and read the log |
| The server stops at start-up on a database error | MariaDB is not running, or its credentials (`Database.*`) are wrong |
| No monsters in the world | The server tables were not imported (`archcore import --server …`) |
| Another player is stuck after choosing a character | `WorldServer.PublicAddress` is still `127.0.0.1`, or port 11008 is closed |
| "Unknown column" message in the log | Code updated without running `archcore db-init` again |
| Empty inventory or shop | The client's language folder (`ini_fr`…) must hold `ItemTemplateAll.ini` (see [[Translation|EN-Translation]]) |
| No sound | Check `[Sound_Option]` in `Game/ini/COption.ini`; the game rewrites this file on exit |
