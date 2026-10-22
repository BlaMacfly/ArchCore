# Database

ArchCore uses three main MariaDB databases, plus a log database, created and updated by `archcore db-init` from
`sql/` (baseline and migrations, see below). All use `utf8mb4`.

> **Editor.** [ArchForge](https://github.com/BlaMacfly/ArchForge) edits the content of these databases without
> writing SQL and can add its changes to the content layer (`content/*.sql`), which survives imports.

## `archcore_auth` — accounts and worlds (`sql/001_auth.sql`, `sql/migrations/auth/`)

| Table | Content |
|---|---|
| `account` | Accounts: name, SHA-256 password hash, GM level, former ban end date (`banned_until`, still read for one version), last login, world entry key (unique, cleared once used), language (last choice of an ArchRust client, see [[Translation|EN-Translation]]) |
| `account_banned` | Bans, with their history: account, date, end (`NULL`: no end), author, reason, `active` (0: lifted early). An account is refused while an active ban has not expired |
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
| `guild` | Guilds: unique name, disbanding password, notice, size, creation date (`sql/migrations/characters/`) |
| `guild_member` | Guild member (a character is in one at most): rank, join date, leave request date |

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
| `map_height` | client `world/b*x.ma2` (ground geometry) | 46,592 sectors: ground height at the 17 × 17 cell corners (i16) |
| `monster_drop` | original server `itemdroptable.txt` | 4,653 items of 550 monsters: count, rate out of 100,000 |
| `drop_group` | original server `groupdroprate.txt` | 28 drop groups; each group's items come from `item_data` (GroupID, DropRank, DropRate) |
| `drop_rank_rate` | original server `droprankrate.txt` | Item ranks (percent) by the monster's drop rank |
| `region_template` | `RegionTemplate.ini` | 331 regions: name, parent region, Type field, safety (33 safe) |
| `resurrection_point` | `obj*.ini` (event 19, type 1) | 62 resurrection points: region, position, allowed race and class |
| `teleport_point` | `TeleportPoint.ini` | 301 teleport points: landing radius, allowed races, groups (arrival, destinations) |
| `teleport_object` | `obj*.ini` (event 8) | 291 portals: object, point carried, position |
| `teleport_fee` | `TeleportFee.txt` | 227 trip fees (departure, arrival) |
| `bank_slot_price` | `bankslotprice.txt` | Price of extra bank pages (-1: cannot be bought with gold) |
| `guild_max_member` | `guildmaxmember.txt` | Guild sizes and their price; the first row is a new guild (50 places, 10,000 coins) |

**Stable identifiers.** In `monster_spawn`, `monster_drop` and `resurrection_point`, the `id` column is only an
import order number. The `stable_key` column (unique, case-sensitive) names the same row from one import to the
next as long as the sources do not change, for a tool that must target a given row (editor, content layer):
`point#rank` for a spawn (rank among the rows of the same point), `monster:item#rank` for a drop,
`file#object#rank` for a resurrection point (`obj*.ini` file, object section). The resurrection points' key is
filled by `archcore import`.

### Languages (`sql/004_locale.sql`, same database)

| Table | Content |
|---|---|
| `language` | Languages offered to clients: BCP 47 code, names, tier (1 official version, 2 Webzen 2010 client, 3 community), client text folder, `enabled`, order; 11 rows inserted by `db-init` |
| `localized_text` | Localized texts: `kind` (`npc_name`, `monster_name`, `item_name`, `item_desc`, `quest_name`, `quest_desc`, `skill_name`, `region_name`, `npc_dialog`), `ref_id` (identifier in the source table), `lang`, `text`, `source`; filled by `archcore i18n db <language>` (source `import`, the only rows it replaces) and by hand (source `manual` by default, kept by `i18n db`) |

See [[Translation|EN-Translation]] for the fallback and [[ArchRust extensions|EN-ArchRust-extensions]] for the protocol.

## ArchCore's own content and import log

`archcore import` rebuilds the world tables from the client files (and the original server tables): anything
written there by hand would be erased by the next import, and that data is not publishable. ArchCore's own
content (fixes, added spawns, NPCs made with the editor…) therefore goes in the repository's **`content/`**
folder, as `<YYYYMMDDHHMM>_<topic>.sql` files: `archcore import` applies them at the end, and `archcore content`
applies them again on their own. Each file runs in a transaction (all or nothing), must be replayable and holds
data only, targeted by TID or `stable_key` (see `content/README.md`).

The **`import_run`** table records each source read (`client`, `relief` (terrain), `serveur` (server),
`contenu` (content)): folder, file count, SHA-256 fingerprint (names and sizes; texts for the content), ArchCore
version, start and end. An empty end means an interrupted import, which `archcore db-check` reports.

## Schema versions (migrations)

The `sql/001…004` files are the **baseline**: `archcore db-init` replays them every time (they are idempotent)
and they are no longer edited. Every later change goes into a **migration**:

```
sql/migrations/<base>/<YYYYMMDDHHMM>_<topic>.sql      <base> = auth, characters or world
```

- `archcore db-init` applies each migration once, in id order, and records it in the `schema_migrations` table of
  its database (id, date, SHA-256 of the text). The baseline is recorded as `0000_reference`.
- Scripts are embedded in the executable at build time: `archcore` no longer needs the `sql/` folder to create
  the databases.
- **At start-up the server refuses to run** when a database was never tracked, is behind (migrations to apply:
  run `archcore db-init`) or ahead (migrations unknown to the executable: update it). A migration edited after
  being applied, or a changed baseline, gives a warning.
- Migrations are additive by default; a rename or a removal takes two versions (add and read both, then
  remove). Never a destructive migration without a backup first (`mariadb-dump`).

## `archcore_logs` — logs (`sql/migrations/logs/`)

Traces of the economy and of administration, write-only: the server never reads them, and each write runs on its
own, outside the world lock (a slow or unavailable log never delays the game). No foreign key, indexes by
character or account and by date.

| Table | Content |
|---|---|
| `log_trade` | Completed trade: both characters, the gold and items (`[{iid, tid, count}]`) each one gave |
| `log_shop` | Purchase or sale to an NPC: character, NPC, item (TID, IID), count, price |
| `log_bank` | Deposit or withdrawal at the account bank: item (TID, IID, count) or gold |
| `log_gm` | GM command: author, level, command, arguments, success, reply |

**Measurement.** A query longer than `Database.SlowQueryMs` (200 ms by default) is logged as a slow query in the
server log, once the server is ready (start-up loads are not counted); a packet handled for longer than `World.SlowPacketMs` (100 ms) under the world lock is logged as a slow
packet, with the lock wait. First measurements (test instance, 1 ms thresholds): the longest queries are the
start-up loads (up to 0.3 s), and the lock is held at most 8 ms per packet (entering the world), with no wait.

## Database consistency (`archcore db-check`)

References from one database to another cannot be foreign keys (they would tie the databases together and
block the world reimport). `archcore db-check` checks them, read-only, and the server does the same at
start-up:

| Check | Severity |
|---|---|
| character without an account (`characters.account_id` → `account`) | serious |
| character with an unknown template (`characters.template_id` → `character_template`) | serious |
| item without a definition (`character_items.template_id` → `item_data`), for example after a reimport | serious |
| unknown skill (`character_skills.skill_tid` → `skill_template`) | serious |
| quest missing from `quest_template` | warning |
| bank without an account, unknown account language | warning |
| character id in the NPC range (≥ 0x40000000), item id in the shop range (≥ 0x20000000) | serious |

`db-check` also shows the schema versions and how full the identifier ranges are, and exits with an error
when it finds a serious anomaly or a database that is not up to date. At start-up, each anomaly goes to the
log with a few examples, and the server starts anyway.

After updating ArchCore, run `archcore db-init` before the server (without `--account`, it touches no account).

## Add an account

```bash
archcore db-init --account <name> <password> [gm_level]
```
