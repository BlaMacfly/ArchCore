# Database

ArchCore uses three main MariaDB databases, plus a log database, created and updated by `archcore db-init` from
`sql/` (baseline and migrations, see below). All use `utf8mb4`.

> **Editor.** [ArchForge](https://github.com/BlaMacfly/ArchForge) edits the content of these databases without
> writing SQL and can add its changes to the content layer (`content/*.sql`), which survives imports.

## `archcore_auth` — accounts and worlds (`sql/001_auth.sql`, `sql/migrations/auth/`)

| Table | Content |
|---|---|
| `account` | Accounts: name, SHA-256 password hash, GM level, former ban end date (`banned_until`, still read for one version), last login, world entry key (unique, cleared once used), return-to-character-selection key (`return_key`, `return_key_until`: AgpmReturnToLogin, single use), language (last choice of an ArchRust client, see [[Translation|EN-Translation]]) |
| `account_banned` | Bans, with their history: account, date, end (`NULL`: no end), author, reason, `active` (0: lifted early). An account is refused while an active ban has not expired. The anti-cheat writes its own there (author `anti-triche`, `AntiCheat.Action = ban`) |
| `server_group` | Server groups shown in the list |
| `world` | Worlds: name, group, `ip:port` address (23 characters max), status |

## `archcore_characters` — characters (`sql/002_characters.sql`)

| Table | Content |
|---|---|
| `characters` | Account, slot, unique name, template (TID), level, experience, HP/MP/SP, position, money, bank, hair, face, charisma, soft delete; `evolution_tid`: evolved form of a Dragon Scion (1722, 1723, 1732; 0 none), `template_id` staying the Scion's; `skill_init`: "skill init" string (empty: the skill reset at the master is still allowed, "1": done); `pet_auto_pick`: pet bag auto pickup (AGPMITEM op 30, 1 by default); `signature`: signature taken with `/nick` and a Signature of Glory (AGPMCHARACTER field 27, at most 16 characters, empty: none) |
| `character_items` | Items: status 0 inventory, 1 equipped, 2 bank (attached to the character who deposited it), 3 guild warehouse (`guild_id`; the character who deposited it stays recorded), 4 item attached to a mail (attached to the recipient), 5 on sale in the auction house (attached to the seller), 6 Cash inventory (status 51; rank in the list in `row_idx`), 7 pet bag (status 13; bag, row, column); a Cash item still at status 0 moves to status 6 when entering the world; `durability`: -1 full (template DUR column), otherwise current durability (wear, `repair.rs`); `remain_ms` (play time left, ms, of a Cash "RemainTime" item) and `expire_at` (expiry date, Unix seconds, of an "ExpireTime" item), set at first use, 0 otherwise (timed items, `lifetime.rs`) |
| `character_quests` | Quests in progress (monster A and B counters, `journal`: tracked on screen, AgpmQuest op 14) and completed |
| `account_bank` | Account bank: money, pages beyond the first (created on first entry, with the money the characters had in the bank) |
| `character_skills` | Learned skills and their level ("Normal Attack" is not stored: every character knows it); `exp`: cumulative experience of a production skill |
| `character_recipes` | Cooking and alchemy recipes bought (`recipe_tid` = `product_compose.result_tid`) |
| `guild` | Guilds: unique name, disbanding password, notice, size, creation date, guild mark (`mark_tid`, `mark_color`), battle record (`win`, `draw`, `lose`, `points`), alliance (`joint_leader`: the founder, `joint_at`) (`sql/migrations/characters/`) |
| `guild_hostile` | Hostilities between guilds, one row per direction (`guild_id`, `other_id`, `since`) |
| `guild_warehouse` | Guild warehouse: money, pages beyond the first |
| `guild_member` | Guild member (a character is in one at most): rank (1 member, 2 join request, 3 leave request, 10 master, 11 sub-master), join date, leave request date |
| `character_buddy` | Friends and ignored players: the other character (`other_id`), packet flags (0x02000000 friend, otherwise ignored with its blocks 0x1 chat, 0x2 trade, 0x4 invitations), "Buddy" relation (unused) |
| `mail` | Mail: recipient, sender (name kept if the character is deleted), subject, text, date (Unix seconds), flags (0x01 read, 0x10 item collected), attached item (`item_id` → `character_items` with status 4) |
| `auction_sale` | Auction house: seller, item on sale (`item_id` → `character_items` with status 5, NULL once sold), TID, count, gold and CC prices, fees taken, status (0 on sale, 1 sold, to collect), listing and expiry dates, buyer; the id is also the packet's docID |
| `character_item_set` | Equipment switch: slot (0 to 99) and its items ("E,part,id" separated by ":"); the worn slot is `characters.item_set_active` (-1 none) |
| `wanted_criminal` | Wanted posters: criminal, total bounty, first and last bounty, expiry (`Wanted.ExpireDays` after the last one); deleted on capture |
| `wanted_bounty` | Bounty history: criminal, poster, amount, date; hunter who got the bounty and capture date |
| `shrine_score`, `shrine_race_score` | Shrine battles: wins and losses of every character and race present at the end of a won battle (AgpmShrineBattle ops 11 and 12) |
| `character_boss_kill` | Boss window: wins of each character over each `UIBoss.xml` boss (one per fighter of the fight in which it dies), date of the last one |
| `castle`, `castle_application` | Siege wars: owner of each castle, status (0 to 2; a siege running at a restart is closed without change), dates of the last and next siege, ends of the applications and of the declaration delay, guild of the last carving; applications (side 1 attack, 2 defense, chosen or not) |
| `archlord` | ArchLord title: one row (id 1), character holding the title (`NULL`: none), name, since when |
| `castle_tax`, `castle_tax_region` | Region taxes: per castle, total income paid, last payout, last rate change, start of the period; per region (castle and outskirts), castle, rate (0 to 50 %) and income since the last payout |
| `character_ui_status` | Interface status: the bar's life and mana potions, their automatic use thresholds (%), helmet display |
| `character_quickbelt` | Quick belt kept by the server (AgpmUIStatus): one row per filled slot (0 to 49), base type (5 item, 7 skill), TID, IID of an item; deleted with the character |
| `character_titles` | Titles owned (AgpmTitle): TID, active (one at most), chosen skill (0), date obtained (Unix seconds); the time of the last change of active title is `characters.title_changed_at` (`Title.ChangeDelaySeconds` delay) |
| `character_title_goals` | Running title goals: TID, acceptance date, values `c1` to `c5` of the template's non-empty conditions |
| `ep8_quarantine` | Switch to the episode 8 values (`archcore migrate-ep8 --apply`): what was removed or changed, nothing destroyed — pass (`run_id`), character, kind (`item`: `character_items` row as JSON with the mail and sales citing it; `options`, `sockets`: original `option_tids` / `socket_tids`; `skill_level`, `skill`; `skill_init`; `quest`, `title`, `title_goal`: `character_quests`, `character_titles` or `character_title_goals` row as JSON, quest or title missing from the imported tables), `ref_id` (item, skill, quest or title TID), original values (JSON), reason, date; `restored_at` set by `--restore` |

## `archcore_world` — game data (`sql/003_world.sql`)

Filled by `archcore import`. Value tables come from the original server tables (episode 8): EP8 rows and
values, client names and visuals joined by TID, Archonia's rows dropped (rules and figures in
[[Tools|EN-Tools]], "Episode 8 values"). The "Source" column gives the client file; the EP8 table of the
same name (`characterdatatable.txt`, `skillconst.txt`, `levelupexp.ini`…) provides the values.

| Table | Client source | Content |
|---|---|---|
| `character_template` | `charactertemplate.ini` | Character and NPC templates: race, class, stats |
| `character_data_table` | `CharacterDataTable.txt` | Server stats per template (HP, speeds, damage, experience…); names of the playable base templates |
| `level_exp` | `LevelUpExp.txt` (Webzen's `levelupexp.txt` with `--webzen`) | Experience to pass each level, `BIGINT UNSIGNED`: the Webzen curve exceeds 2³² from level 113 (6,640,723,530 at level 121); `characters.exp` is 64-bit too |
| `growup_factor` | `GrowUpFactor.txt` | Stat growth |
| `login_char_pos` | `logincharpos.ini` | Character selection screen positions |
| `item_template` | item templates | Items |
| `item_data` | `itemdatatable.txt` | Item stats: level, part, kind, type, NPC prices (gold `npc_price`, resale `pc_price`, charisma `npc_charisma_price` and Shrine Coins `npc_shrine_coin` of the Shrine shop), stack, every column as JSON (`raw`) |
| `skill_template` | `SKILLTEMPLATE.ini` | Skills (TID, name) |
| `skill_spec` | `SKILL_SPEC.txt` | Each skill's nature (non-empty columns, as JSON) |
| `quest_template` | `questtemplate.ini` (Webzen's `fr/questtemplate.ini` with `--webzen`: 3,235 quests) | Quests (non-empty columns, as JSON; English texts when Webzen has them) |
| `quest_group` | `questgroup.ini` (Webzen's `fr/questgroup.ini`) | NPC and world object groups: name, quests offered and handed in, picking up on the world object (`gather`, [[TID, rank], …], for the export) |
| `skill_level` | `SKILL_CONST.txt`, `Skill_Const2.txt` | Values of each level of each skill (non-empty columns, as JSON; `point.`, `percent.`, `magnify.` groups; those of `Skill_Const2.txt`, suffixed with "2", under `point2.`, `percent2.`…) |
| `skill_mastery` | `SkillMastery_High.txt`, `SkillMastery_Heroic.txt` | Mastery trees: key (template TID, 10000 + race for "10004;4"), skill, tree (1 high, 2 heroic), required skills `[[TID, level]…]` (ConditionA, B), levels in a template's class `class_points` `[[template TID, levels]…]` (ConditionC), total (ConditionD: levels in total, or heroic points) |
| `product_category`, `product_compose`, `product_exp`, `product_gather`, `product_result_item` | `productcategory.txt`, `productcompose.txt`, `productexp.txt`, `productgather.txt`, `productresultitem.txt` | Production: skill of each category, 95 recipes (materials `[[TID, count]…]`, price, level, chance), levels 1 to 15, 760 monsters that can be gathered, items gathered per set and level (skill names resolved through `SKILLTEMPLATE.ini`) |
| `npc_spawn` | `npc.ini` (merged with the EP8 table) | 557 NPCs: template, name, position, orientation, raw functions |
| `client_table` | other tables | Whole texts (row 0): `itemoptiontable.txt`, `itemruneattributetable.txt`, `ItemConvertTable.txt`, `itemlotterybox.txt`, `avatarset.ini`, `refineryrecipetable.txt`, `gachatypetemplate.ini`, `gachadroptable*.txt` (EP8 tables), `RegionTemplate.ini`, `TeleportPoint.ini` (EP8, kept for `export-client-tables`), `optionnumdroprate.txt`, `socketnumdroprate.txt`; for the export, `SkillMastery_High.txt`, `SkillMastery_Heroic.txt` (files read by the import), and with `--webzen` `titledatatable.txt`, `titlestringtable.txt` (kept titles, texts in the client format) and `questtemplate.ini@<language>` (quest texts of each Webzen language, JSON TID → column → text) |
| `monster_spawn` | original server tables (`spawndatatable.txt` + `spawninstancetable.txt`) | 12,802 spawn points, 36,960 creatures: template, level (the EP8 template's), count, respawn (`respawn_sec`–`respawn_max_sec` range for the `UIBoss.xml` bosses), position, radius, public name (`CharacterTemplatePublic.ini`) |
| `map_segment` | original server `world/node_data` | 45,312 sectors: blocked ground and region per 400-unit segment |
| `map_height` | client `world/b*x.ma2` (ground geometry) | 46,592 sectors: ground height at the 17 × 17 cell corners (i16) |
| `monster_drop` | original server `itemdroptable.txt` | 4,653 items of 550 monsters: count, rate out of 100,000 |
| `monster_ai` | original server `aidatatable.txt` | 7,674 AIs (3,289 with skills), named by `monster_spawn.ai_tid`: non-empty columns as JSON, repeated columns numbered (`Skill1`, `SkillCondition1`, `SkillCount1`…) |
| `drop_group` | original server `groupdroprate.txt` | 28 drop groups; each group's items come from `item_data` (GroupID, DropRank, DropRate) |
| `drop_rank_rate` | original server `droprankrate.txt` | Item ranks (percent) by the monster's drop rank |
| `region_template` | `RegionTemplate.ini` | 331 regions: name, parent region, Type field, safety (33 safe), entry levels `level_min` and `level_limit` (LevelMin, LevelLimit: Battle Square floors), bans `disabled` (bits 1 DisableSummon, 2 DisableGo, 4 DisableParty, 8 DisableLogin), `item_section` (ItemSection: Heaven's Hell), own resurrection point `resurrection_x`, `resurrection_z` (ResurrectionX/Z) |
| `region_glossary` | `RegionGlossary.txt` of every client language folder (`ini`, `ini_fr`…) | Displayed name of each region per language (key: `RegionTemplate.ini` Korean name): name of the taxed regions sent to the client (AgpmTax) |
| `boss_window` | `UIBoss.xml` | 25 bosses of Archonia's boss window: TID, order, category (1 to 7), displayed name |
| `pvp_skull_chance`, `pvp_skull_level` | `pvpskulldroptable.txt` | Battle Square skulls: chance (%) by killer − victim level gap, skull level by the victim's level |
| `resurrection_point` | `obj*.ini` (event 19, type 1) | 62 resurrection points: region, position, allowed race and class |
| `siege_binding` | `obj*.ini` (event 19, types 3 to 7) | 27 siege resurrection points: region, type (3 attackers, 4 inner defense, 7 outer defense, 5 and 6 Archlord), position |
| `siege_war` | `SiegeWar.txt` | 5 castles (0 Rensphere to 4 Drayger): castle and outskirts regions, lengths of the battle, the Archon's Eyes and the carving, declaration and application delays, gap between two sieges, start hour, carving skill |
| `siege_war_object` | `SiegeWarObject.txt` | 2 assault objects (9 camp, 10 catapult): repairs, cost, duration, required items (JSON) |
| `teleport_point` | `TeleportPoint.ini` | 301 teleport points: number (rank in the file, from 1), landing radius, allowed races, groups (arrival, destinations) |
| `teleport_object` | `obj*.ini` (event 8) | 291 portals: object, point carried, position |
| `refinery_object` | `obj*.ini` (event 26) | 20 alchemy spheres: object, position (refining within 1600 units) |
| `gacha_object` | `obj*.ini` (event 38) | 60 gacha pedestals: object, draw type (`Type=` of the `GachaStart` data), position (window within 600 units) |
| `quest_object` | `obj*.ini` (event 27) | 780 world objects with quests: object, quest group (`QuestGroupID`, usually the object's own number), position (quests taken and handed in within 1600 units) |
| `teleport_fee` | `TeleportFee.txt` | 227 trip fees (departure, arrival) |
| `bank_slot_price` | `bankslotprice.txt` | Price of extra bank pages (-1: cannot be bought with gold) |
| `item_transform` | `itemtransformtable.txt` | 89 metamorphosis potions: item TID, form, level and Korean name (for reference), bonuses added to the transformed player's factors (run speed, attack speed, max HP and MP, defense, min and max damage; may be negative) |
| `character_customize` | `charactercustomizelist.txt` | Hair styles (1) and faces (2) per model: number (AGPMCHARACTER fields 20 and 21), name, `Case` (0 creation and stylist, 1 stylist, 2 creation), required level, gold or item price, file order |
| `guild_max_member` | `guildmaxmember.txt` | Guild sizes and their price; the first row is a new guild (50 places, 10,000 coins), the next ones the enlargements |
| `guild_mark` | `guildmark.txt` | Guild mark items: layer (1 background, 2 pattern, 3 symbol, 4 colour), number, TID (number << 8 × (layer − 1)), name, gold price, skulls, cash, ARGB colour |
| `title_template` | `titledatatable.txt`, `titlestringtable.txt` | Episode 8 titles: with `--webzen`, Webzen's 2,768 (627 Expert Collector, 2,058 Natural Enemy, 30 Advanced, 29 Collector, 14 Addict, 4 Killer, 3 Storyteller, 3 Researcher), otherwise the client's 684 (10 Defender of Chantra and Schola 1-3 marked `optional`, loaded with `Title.Schola`): category (`FirstCategory`), requirements to set the goal (level, class, guild master, ArchLord, stats), gold, cost items, prerequisite titles, conditions (`checks`: [condition, Set, Value], condition in Webzen's sense — TID of the family's first title, 10001, 30001… —, Archonia's types being mapped on loading), skills and effects (JSON); Archonia's Christmas Event and Schola 4/5 not imported |
| `charisma_title` | `charismatitletable.txt` | 14 charisma ranks (min CHA, max, title): computed by the client, read by `/title` |

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
| `localized_text` | Localized texts: `kind` (`npc_name`, `monster_name`, `item_name`, `item_desc`, `quest_name`, `quest_desc`, `skill_name`, `region_name`, `npc_dialog`), `ref_id` (identifier in the source table), `lang`, `text`, `source`; filled by `archcore i18n db <language>` (source `import`, the only rows it replaces), by `archcore import --webzen` (source `webzen`: quest names and starting dialogs in fr, de, es, pt-BR, replaced on each import) and by hand (source `manual` by default, kept by `i18n db`) |

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

Traces of the economy and of administration, write-only: the server only reads them for the `/anomalies` command, and each write runs on its
own, outside the world lock (a slow or unavailable log never delays the game). No foreign key, indexes by
character or account and by date.

| Table | Content |
|---|---|
| `log_trade` | Completed trade: both characters, the gold and items (`[{iid, tid, count}]`) each one gave |
| `log_shop` | Purchase, sale or repair (`repair`) at an NPC: character, NPC, item (TID, IID), count, price |
| `log_bank` | Deposit or withdrawal at the account bank: item (TID, IID, count) or gold |
| `log_auction` | Auction house: listing, purchase, cancel or collect, sale, author (and seller for a purchase), item (TID, IID), count, gold and CC prices, fees |
| `log_gm` | GM command: author, level, command, arguments, success, reply |
| `log_anomaly` | Anti-cheat anomaly (`world/anticheat.rs`): account, character, type (`vitesse` speed, `téléportation` teleport, `cadence` hit rate, `inondation` flood), detail, sanction applied (`kick`, `ban` or empty); at most 20 rows per minute and per character. Read back by `/anomalies` |

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
| unknown evolved form (non-zero `characters.evolution_tid` → `character_template`) | warning |
| item without a definition (`character_items.template_id` → `item_data`), for example after a reimport | serious |
| unknown skill (`character_skills.skill_tid` → `skill_template`) | serious |
| quest missing from `quest_template` | warning |
| recipe missing from `product_compose` (`character_recipes.recipe_tid`) | warning |
| title or title goal missing from `title_template` (`character_titles`, `character_title_goals`) | warning |
| mail attachment not found (`mail.item_id` without an item with status 4 owned by the recipient) | warning |
| item on sale not found (`auction_sale` on sale without an item with status 5 owned by the seller) | warning |
| hair style or face missing from `character_customize` for the character's model (table imported) | warning |
| wins over a boss missing from `boss_window` (`character_boss_kill.boss_tid`) | warning |
| castles missing from `siege_war` (`castle.id`) | warning |
| taxes of a castle missing from `siege_war` (`castle_tax.castle_id`), taxed region missing from `region_template` (`castle_tax_region.region_id`) | warning |
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


`character_items`: normal inventory now accepts five bags, five rows and five columns (coordinates 0 through 4). Existing columns suffice; no migration or redistribution of stored items is needed.
