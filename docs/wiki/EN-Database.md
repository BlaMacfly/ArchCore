# Database

ArchCore uses three MariaDB databases, created by `tools/db-init.mjs` from `sql/`. All use `utf8mb4`.

## `archcore_auth` — accounts and worlds (`sql/001_auth.sql`)

| Table | Content |
|---|---|
| `account` | Accounts: name, SHA-256 password hash, GM level, ban, last login, world entry key |
| `server_group` | Server groups shown in the list |
| `world` | Worlds: name, group, `ip:port` address (23 characters max), status |

## `archcore_characters` — characters (`sql/002_characters.sql`)

| Table | Content |
|---|---|
| `characters` | Account, slot, unique name, template (TID), level, experience, HP/MP/SP, position, money, bank, hair, face, soft delete |
| `character_items` | Owned items (planned) |
| `character_skills` | Learned skills (planned) |

## `archcore_world` — game data (`sql/003_world.sql`)

Filled by `tools/import-client-data.mjs` from the client tables.

| Table | Client source | Content |
|---|---|---|
| `character_template` | `charactertemplate.ini` | Character and NPC templates: race, class, stats |
| `character_data_table` | `CharacterDataTable.txt` | Names of the playable base templates |
| `level_exp` | `LevelUpExp.txt` | Experience per level |
| `growup_factor` | `GrowUpFactor.txt` | Stat growth |
| `login_char_pos` | `logincharpos.ini` | Character selection screen positions |
| `item_template` | item templates | Items |
| `skill_template` | `SKILLTEMPLATE.ini` | Skills |
| `npc_spawn` | `npc.ini` | 567 NPCs: template, name, position, orientation, raw functions |
| `client_table` | other `.txt` tables | Rows as JSON, for tables not modelled yet |

## Add an account

```bash
node tools/db-init.mjs --account <name> <password> [gm_level]
```
