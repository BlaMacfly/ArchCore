# Configuration

Settings live in two files of the `config/` folder:

| File | Read by | Contents |
|---|---|---|
| `auth.conf` | `archcore login` | Login server, databases |
| `world.conf` | `archcore world` | World server, databases, client data, world settings, rates, GM commands |

`archcore all` and the tools (`db-init`, `import`, `sim`…) read both; the `Database.*` keys, present in
both files, are then taken from `world.conf` (a warning reports a difference).

These files are not versioned. Their templates, `auth.conf.dist` and `world.conf.dist`, are, and document
every option; on first start, a missing `.conf` is created by copying its template.

## Syntax

```ini
[worldserver]
# comment
WorldServer.Port = 11008
WorldServer.Name = "ArchCore"
```

One setting per line, `Key = value`; `#` starts a comment, text may be quoted, the `[authserver]` or
`[worldserver]` header is ignored. A missing key takes its default value, with a warning in the log; an
unknown key (typo) is reported.

## `auth.conf`

| Key | Default | Purpose |
|---|---|---|
| `LoginServer.Port` | `11002` | Login server port, the one the client contacts |
| `LoginServer.Bind` | `0.0.0.0` | Listen address |

## `world.conf`

| Key | Default | Purpose |
|---|---|---|
| `WorldServer.Port` | `11008` | World server port |
| `WorldServer.Bind` | `0.0.0.0` | Listen address |
| `WorldServer.Name` / `WorldServer.Group` | `ArchCore` | World and group name in the server list |
| `WorldServer.PublicAddress` | `127.0.0.1` | World address sent to the client. Change it for a server reachable from the Internet. On startup, the world writes `PublicAddress:Port` to the `world` table, under `WorldServer.Name` and `WorldServer.Group` (23 characters max) |
| `Client.DataDir` | `../client/ArchLord/Game` | The client's `Game` folder, used by the import and translation tools |
| `World.Language` | `fr` | NPC title language (`data/i18n/npc-titles-<lang>.json`); `en` keeps them in English |
| `World.ViewRadius` | `8000` | Range of normal chat and `SAY` AddOn messages. NPCs, monsters, players and ground items are seen by sector (see [[Network protocol|EN-Network-protocol]]) |
| `World.MonsterDensity` | `1` | Multiplies the number of monsters at each spawn point (rounded, at least 1). The server tables are sparse in places: 2 or 3 come closer to the original game's density |
| `World.SlowPacketMs` | `100` | A packet handled for longer (ms) under the world lock is logged as a "slow packet" (`paquet lent`), with the lock wait time; `0`: never |
| `World.TimeZone` | `local` | Time zone of the clock sent to the client (AGPMTIMER packet): `local` = the machine's time (day/night cycle, music and ambience follow the server's local time), `utc` = raw Unix epoch. See [[Network protocol|EN-Network-protocol]] |
| `World.ExitOnRestart` | `0` | `1`: stops the server at the end of the `/announce_restart` countdown |
| `StartPosition.<race>.Village` / `.X` / `.Y` / `.Z` | see the template | Spawn position per race: 1 Human, 2 Orc, 3 Moon Elf, 4 Scion |
| `Rate.Exp`, `Rate.Gold`, `Rate.Drop`, `Rate.UniqueDrop`, `Rate.Charisma`, `Rate.Prestige`, `Rate.ChatBuff` | `1` | Rates at startup (see [[Administration|EN-Administration]]) |
| `GM.CommandLevel.<command>` | — | Required GM level for a command, for example `GM.CommandLevel.kill = 3` |

Current starting villages:

| Race | Village |
|---|---|
| 1 Human | Anchorville |
| 2 Orc | Golundo |
| 3 Moon Elf | Norine |
| 4 Scion | Cien, unconfirmed |

## Databases (both files)

| Key | Default | Purpose |
|---|---|---|
| `Database.Host` / `Database.Port` | `127.0.0.1` / `3306` | MariaDB server |
| `Database.User` / `Database.Password` | `root` / `archcore` | Credentials. **Change them** outside a development machine |
| `Database.ConnectionLimit` | `8` | Pool size per database |
| `Database.Auth` / `Database.Characters` / `Database.World` | `archcore_*` | Names of the three main databases |
| `Database.Logs` | `archcore_logs` | Log database (trades, shops, bank, GM commands), write-only. A test instance takes its own |
| `Database.SlowQueryMs` | `200` | A longer query (ms) is logged as a "slow query" (`requête lente`) in the server log, once the server is ready (loading the world is not counted); `0`: never |

## Environment variables

Any key can be overridden by an `ARCHCORE_<KEY>` variable: dots and inner capitals become `_`, all in
upper case.

| Key | Variable |
|---|---|
| `LoginServer.Port` | `ARCHCORE_LOGIN_SERVER_PORT` |
| `World.ViewRadius` | `ARCHCORE_WORLD_VIEW_RADIUS` |
| `World.MonsterDensity` | `ARCHCORE_WORLD_MONSTER_DENSITY` |
| `Database.Password` | `ARCHCORE_DATABASE_PASSWORD` |
| `Rate.Exp` | `ARCHCORE_RATE_EXP` |

Other variables:

| Variable | Effect |
|---|---|
| `ARCHCORE_CONF_DIR=<folder>` | Reads `auth.conf` and `world.conf` from another folder, for example a test server on other ports and databases |
| `ARCHCORE_ROOT=<folder>` | Repository root (current folder by default) |
| `ARCHCORE_TRACE=1` | Logs every packet received and sent |

## Former `archcore.json` configuration

Until October 2026, settings lived in `config/archcore.json`. To convert it:

```bash
archcore config migrate config/archcore.json            # writes config/auth.conf and config/world.conf
archcore config migrate test.json /path/conf-test       # into another folder (ARCHCORE_CONF_DIR)
```

The files are written from the `.dist` templates, comments included; an existing file is only replaced
with `--force`.
