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
`sql/`, then creates the account `test` with the password `test`. A game master level can be given
as a fourth argument.

## 4. Import the client data

```bash
archcore decrypt     # Game/ini → data/client-ini (plain text)
archcore import      # data/client-ini → archcore_world
```

The client tables are RC4-encrypted, with a key derived from MD5("1111"), and written in CP949. The
import converts them to UTF-8: character templates, experience, NPCs, items, shops, skills.

## 5. Start the server

```bash
archcore                     # login (11002) + world (11008)
archcore login               # login only
archcore world               # world only
ARCHCORE_TRACE=1 archcore    # detailed log of every packet
```

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

## Troubleshooting

| Symptom | Lead |
|---|---|
| "Cannot access the login server" | ArchCore is not listening on 11002, or `<ClientName>Test.ini` is missing |
| Endless loading when entering the world | Run with `ARCHCORE_TRACE=1` and read the log |
| Empty inventory or shop | The client's language folder (`ini_fr`…) must hold `ItemTemplateAll.ini` (see [[Translation|EN-Translation]]) |
| No sound | Check `[Sound_Option]` in `Game/ini/COption.ini`; the game rewrites this file on exit |
