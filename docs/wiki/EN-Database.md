# Database

ArchCore uses three MariaDB databases, created by `archcore db-init` from `sql/`. All use `utf8mb4`.

## `archcore_auth` — accounts and worlds (`sql/001_auth.sql`)

| Table | Content |
|---|---|
| `account` | Accounts: name, SHA-256 password hash, GM level, ban, last login, world entry key, language (last choice of an ArchRust client, see [[Translation|EN-Translation]]) |
| `server_group` | Server groups shown in the list |
| `world` | Worlds: name, group, `ip:port` address (23 characters max), status |

## `archcore_characters` — characters (`sql/002_characters.sql`)

| Table | Content |
|---|---|
| `characters` | Account, slot, unique name, template (TID), level, experience, HP/MP/SP, position, money, bank, hair, face, soft delete |
| `character_items` | Items: status 0 inventory, 1 equipped, 2 bank (attached to the character who deposited it) |
| `character_quests` | Quests in progress (monster A and B counters) and completed |
| `account_bank` | Account bank: money, pages beyond the first (created on first entry, with the money the characters had in the bank) |
| `character_skills` | Learned skills and their level ("Normal Attack" is not stored: every character knows it) |

## `archcore_world` — game data (`sql/003_world.sql`)

Filled by `archcore import` from the client tables.

| Table | Client source | Content |
|---|---|---|
| `character_template` | `charactertemplate.ini` | Character and NPC templates: race, class, stats |
| `character_data_table` | `CharacterDataTable.txt` | Server stats per template (HP, speeds, damage, experience…); names of the playable base templates |
| `level_exp` | `LevelUpExp.txt` | Experience per level |
| `growup_factor` | `GrowUpFactor.txt` | Stat growth |
| `login_char_pos` | `logincharpos.ini` | Character selection screen positions |
| `item_template` | item templates | Items |
| `skill_template` | `SKILLTEMPLATE.ini` | Skills (TID, name) |
| `skill_spec` | `SKILL_SPEC.txt` | Each skill's nature (non-empty columns, as JSON) |
| `quest_template` | `questtemplate.ini` | Quests (non-empty columns, as JSON) |
| `quest_group` | `questgroup.ini` | NPC groups: quests offered and handed in |
| `skill_level` | `SKILL_CONST.txt` | Values of each level of each skill (non-empty columns, as JSON; `point.`, `percent.`, `magnify.` groups) |
| `npc_spawn` | `npc.ini` | 567 NPCs: template, name, position, orientation, raw functions |
| `client_table` | other `.txt` tables | Rows as JSON, for tables not modelled yet |
| `monster_spawn` | original server tables (`spawndatatable.txt` + `spawninstancetable.txt`) | 12,802 spawn points, 36,960 creatures: template, level, count, respawn, position, radius, public name (`CharacterTemplatePublic.ini`) |
| `map_segment` | original server `world/node_data` | 45,312 sectors: blocked ground and region per 400-unit segment |
| `monster_drop` | original server `itemdroptable.txt` | 4,653 items of 550 monsters: count, rate out of 100,000 |
| `drop_group` | original server `groupdroprate.txt` | 28 drop groups; each group's items come from `item_data` (GroupID, DropRank, DropRate) |
| `drop_rank_rate` | original server `droprankrate.txt` | Item ranks (percent) by the monster's drop rank |
| `region_template` | `RegionTemplate.ini` | 331 regions: name, parent region, Type field, safety (33 safe) |
| `resurrection_point` | `obj*.ini` (event 19, type 1) | 62 resurrection points: region, position, allowed race and class |

### Languages (`sql/004_locale.sql`, same database)

| Table | Content |
|---|---|
| `language` | Languages offered to clients: BCP 47 code, names, tier (1 official version, 2 Webzen 2010 client, 3 community), client text folder, `enabled`, order; 11 rows inserted by `db-init` |
| `localized_text` | Localized texts: `kind` (`npc_name`, `monster_name`, `item_name`, `item_desc`, `quest_name`, `quest_desc`, `skill_name`, `region_name`, `npc_dialog`), `ref_id` (identifier in the source table), `lang`, `text`; filled by `archcore i18n db <language>` and by hand |

See [[Translation|EN-Translation]] for the fallback and [[ArchRust extensions|EN-ArchRust-extensions]] for the protocol.

## Add an account

```bash
archcore db-init --account <name> <password> [gm_level]
```
