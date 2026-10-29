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
| `Guild.BattleReadySeconds` | `180` | Delay (s) between accepting a guild battle and its start; the client announces it in minutes. Archonia's data do not give it |
| `PvP.MurdererPointsPerKill` | `20` | Murderer points for killing an innocent more than 4 levels below, attacked first (thresholds 40, 60, 100). Original value unknown |
| `PvP.RemissionPoints` | `2` | Murderer points removed by a remission with 10 letters of pardon, as the client announces |
| `Item.WeaponWearChance` | `10` | Chance (%) that a wounding hit wears the weapon that dealt it by one point (ArchCore's choice; 0: no wear) |
| `Item.ArmorWearChance` | `10` | Chance (%) that a wounding hit received wears one worn armour piece picked at random by one point (ArchCore's choice) |
| `World.TimeSpeed` | `6` | Game time speed sent to the client (AGPMTIMER field 0): 6 as in Archonia, a day lasts 4 real hours; 1 = real time |
| `World.TimeStart` | `-1` | Game hour when the server starts (0 to 23), the same for everyone; -1 = machine clock × speed, the same from one start to the next. See [[Network protocol|EN-Network-protocol]] |
| `World.WeatherAuto` | `0` | 1 = automatic weather (rain, snow or clear rolled for everyone); 0 = `/weather` only (ArchCore's choice) |
| `World.WeatherMinutes` | `30` | Minutes between two automatic weather rolls |
| `World.WeatherRainChance`, `World.WeatherSnowChance` | `20`, `10` | Chances (%) of rain and snow on each roll; otherwise clear |
| `Auction.MaxSales` | `100` | Maximum sales per character in the auction house (1 to 100, the client's box) |
| `Auction.ExpireDays` | `30` | Duration of a sale in days; once expired it leaves the search but can still be cancelled |
| `Summons.StatScale` | `100` | Stats of summoned creatures, in percent of their model's stats brought to the master's level (ArchCore's choice) |
| `Chat.ShoutRange` | `0` | Range of the local "/s" shout; 0: twice `World.ViewRadius` (ArchCore's choice) |
| `Wanted.ExpireDays` | `7` | Lifetime of a wanted poster, in days since the last bounty (ArchCore's choice) |
| `Wanted.ExpLossPercent` | `5` | Share of the level's experience lost by a wanted criminal killed by a hunter, never a level (ArchCore's choice) |
| `Shrine.Enabled` | `1` | Shrine battles on the `shrine` table's schedule; `0`: only through `/shrine open` |
| `BattleSquare.SupervisorTeleport` | `1` | Talking to a Battle Square supervisor (NPCs 551 to 554) sends the player to their level's floor (ArchCore's choice) |
| `BattleSquare.SkullTids` | `500,981,982` | TIDs of the level 1, 2 and 3 skulls dropped in the Battle Square (ArchCore's choice) |
| `Battleground.Schedule` | empty | "HH:MM-HH:MM" ranges (separated by ";") whose start and end are announced to everyone (0x58 packet); empty: no announcement |
| `Siege.Enabled` | `1` | Automatic siege war calendar (automatic date, closing of applications, start, end); `0`: only the `/siege` commands |
| `Siege.MinGuildMembers`, `Siege.MinGuildAgeDays` | `30`, `14` | Members and age (days) of a guild applying to a siege (client values; lower them for a local game) |
| `Siege.DurationMinutes` | `0` | Length of the siege battle; `0`: SWPlayingTime of `SiegeWar.txt` (60 min), otherwise 120 |
| `Siege.CarveSeconds` | `0` | Length of the throne carving; `0`: StampingTime of `SiegeWar.txt` |
| `Siege.CatapultDamagePercent` | `5` | Damage of a catapult shot, in percent of the targeted outer gate's or tower's HP (ArchCore's choice) |
| `Archlord.MaxGuards` | `10` | Royal guards the ArchLord can call at once (`/guardactivate`, Call Royal Knight) |
| `Archlord.PortalMinutes` | `10` | Rensphere war: length of the "portal" step before the "fight" step |
| `Archlord.SkyMinutes` | `5` | Length of the 12-hour clock shift of Day and Night |
| `Tax.PayoutDay`, `Tax.PayoutHour` | `1`, `0` | Weekly payout of the region taxes: day (1 Monday to 7 Sunday) and hour (`World.TimeZone`) |
| `AntiCheat.Enabled` | `1` | Server-side anti-cheat checks: positions sent by the client and normal hit rate (see [[Administration|EN-Administration]]); `0`: off (the packet limit stays) |
| `AntiCheat.Action` | `log` | Sanction for a player reaching `AntiCheat.Threshold` anomalies: `log` (journal only), `kick` (disconnect, no reconnection wait) or `ban` (account ban, then disconnect) |
| `AntiCheat.Threshold`, `AntiCheat.WindowMinutes` | `10`, `10` | Anomalies of one character within the sliding window (minutes) that trigger the sanction |
| `AntiCheat.BanHours` | `24` | Length of a `ban` (hours); `0`: no end (lift it by hand in `account_banned`) |
| `AntiCheat.SpeedTolerance` | `150` | Allowed speed for positions sent by the client, in percent of the current run speed (mount, buffs and `/speed` included) |
| `AntiCheat.MoveSlack` | `300` | Fixed lag margin (units) for positions; the distance credit also builds up to 5 s of running, for delayed packets |
| `AntiCheat.AttackTolerance` | `150` | Allowed normal hit rate, in percent of the attack speed: the first hit on a new target or after resuming waits the interval divided by 1.5 |
| `AntiCheat.MaxPacketsPerSecond`, `AntiCheat.PacketBurst` | `150`, `500` | Packets per second per connection, and tolerated burst (login and world); beyond, ignored, and one second worth of ignored packets without a pause closes the connection (always, even with `AntiCheat.Enabled = 0`). `0`: no limit |
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
| `Database.Logs` | `archcore_logs` | Log database (trades, shops, bank, GM commands, anti-cheat anomalies), write-only (except `/anomalies`, which reads the anomalies back). A test instance takes its own |
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
