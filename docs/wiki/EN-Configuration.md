# Configuration

All settings live in `config/archcore.json`. Missing keys fall back to the defaults in
`src/common/config.js`.

## `login`

| Key | Default | Purpose |
|---|---|---|
| `port` | `11002` | Login server port, the one the client contacts |
| `bind` | `0.0.0.0` | Listen address |

## `world`

| Key | Default | Purpose |
|---|---|---|
| `port` | `11008` | World server port |
| `bind` | `0.0.0.0` | Listen address |
| `name` / `group` | `ArchCore` | World and group name in the server list |
| `publicAddress` | `127.0.0.1` | World address sent to the client. Change it for a server reachable from the Internet |
| `startPositions` | see the file | Spawn position per race: 1 Human, 2 Orc, 3 Moon Elf, 4 Scion |
| `viewRadius` | `8000` | Radius within which NPCs are sent to the player |
| `language` | `fr` | NPC title language (`data/i18n/npc-titles-<lang>.json`); `en` keeps them in English |

Current starting villages:

| Race | Village |
|---|---|
| 1 Human | Anchorville |
| 2 Orc | Golundo |
| 3 Moon Elf | Norine |
| 4 Scion | Cien, unconfirmed |

## `database`

| Key | Default | Purpose |
|---|---|---|
| `host` / `port` | `127.0.0.1` / `3306` | MariaDB server |
| `user` / `password` | `root` / `archcore` | Credentials. **Change them** outside a development machine |
| `connectionLimit` | `8` | Pool size per database |
| `auth` / `characters` / `world` | `archcore_*` | Names of the three databases |

## `client`

| Key | Default | Purpose |
|---|---|---|
| `dataDir` | `../client/ArchLord/Game` | The client's `Game` folder, used by the import and translation tools |

## Environment variables

| Variable | Effect |
|---|---|
| `ARCHCORE_TRACE=1` | Logs every packet received and sent |
