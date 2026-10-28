# Status and roadmap

> **Episode.** World of Episode 8 "The Dark Revelation" (2012) and Archonia extensions; systems of the
> original release. Detail by episode: [[Game episode|EN-Episode]].

## Working with the real client

- Server and tools entirely in Rust (a single `archcore` executable).
- Encrypted handshake, world list, account login.
- Character list and creation screen.
- World entry, with a starting village per race.
- Spawning of the 567 NPCs around the player, with translated titles.
- French client interface, accents included (translation in progress).
- Player movement, with position saving.
- NPC dialog: the npc.ini functions (dialog, shop, quests) are sent with each NPC, and clicking opens
  the dialog window.
- NPC shops: Darryl's shop (Anchorville) shows its 43 items, and buying takes the gold and puts the
  item in the inventory.
- Character inventory shown on world entry.

## Verified with the simulator, to confirm in game

- Creatures: 36,960 monsters placed from the original server tables, off blocked ground, with their stats
  (level, speeds, HP). Seen and targeted in game during a first test; sending their HP, added since,
  remains to be checked. Levels taken from the Archonia client and boss respawn from its boss window;
  fidelity report in [[Game episode|EN-Episode]].
- Melee combat against monsters: approach, hits (hit chance, class-based damage), death, experience and
  level up (stats, HP and MP from GrowUpFactor), corpse then respawn. Monsters that are hit fight back and chase their attacker; player death
  and resurrection in town. Drops: gold and items on the ground, pick-up.
  Equipment: weapons and armour worn (restrictions, two hands), counted in damage and defense, visible
  to other players; moving items in the inventory. Life and mana potions, out-of-combat HP and MP
  regeneration. Teleport scrolls (town, round trip, destination); monster patrols. Resurrection at the point of
  the last safe region crossed; item stacks split, merged, destroyed. Skills: learning and upgrading at
  trainers, skill points, casting; damage, areas, buffs and debuffs, damage over time, heals, passives;
  special states (stun, sleep, slow…), critical hits, block, damage turned into HP or MP, counter attack (damage sent back to the monster); party skills;
  masteries (required skills of the high and heroic trees); monster skills (AI from the original server tables:
  casting, damage, effects, on-hit procs).
  Parties (invitation, members, leader, kick, experience sharing, chat). Guilds (creation at guild managers, invitation, joining, leaving, kicking, disbanding, notice, chat, sub-master and handover, enlarging, guild list, guild marks, guild battles, alliances and hostilities, guild warehouse). Account bank (items, money, extra pages) and trading between players. Teleport portals (paid trips between linked points, player-versus-player zones
  and "Return", refusal in combat). Automatic reconnection (type 0x63). `/last_refine` GM command. Quests: taking, progress (monsters, quest
  items), completion and rewards, abandoning. See
  [[Network protocol|EN-Network-protocol]].
- ArchRust extensions: capability negotiation, AddOn messages (whisper, `SAY`), and the client language
  (version 1.1): the server serves NPC and monster names in the chosen language, with fallback, and the
  other texts on demand; the choice follows the account. See
  [[ArchRust extensions|EN-ArchRust-extensions]] and [[Translation|EN-Translation]]; on the ArchRust side,
  a language selector in the Options.
- Selling an item to an NPC.
- Player versus player: Archonia's attack rules (areas, level 20, Ctrl, friends, party, Battlegrounds), declared
  enemies, first attacker, wins and losses, murderer points and their penalties (party, trade, NPC prices,
  experience), charisma gained, remission at the pardoners, offensive skills on players (Ctrl, areas); on the ArchRust
  side, Ctrl attack and skills, messages, name colours, area announcements and the `UI_Remission` window. Guards stay
  passive like their Archonia AI (`UseNormalAttack` 0); wanted posters: see below.
- Preemptive monsters: those whose AI has `PreemptiveType` 1 attack a player within their range (`PreemptiveRange`) on
  their own, except targets their level conditions exclude. Validated with the simulator (`aggro` scenario). Validated with the simulator on 27 October 2026 (`pk` scenario). Murderer point and
  charisma amounts are configurable, the original ones being unknown.
- Production (types 0x17 and 0x3A): production skills learnt from the Adventurer, Cook and Alchemist NPCs (function
  24), recipes bought from them, gathering from monster corpses (Collection Skill, Gathering Knife, items from
  `productresultitem.txt` by skill level), cooking and alchemy (materials, recipe item, tools), experience and levels
  1 to 15 (AGPMSKILL "dirt" factors 4 and 6), known recipes sent on entering the game. Success, quantity and
  experience rules are ArchCore's choices, documented in [[Network protocol|EN-Network-protocol]]. Validated with the
  simulator on 27 October 2026 (`product` scenario); the ArchRust windows (`UI_PDSkill`, `UI_PDSkillBuy`,
  `UI_Compose`) are on the way, to be confirmed in game.
- Friends and ignore list (type 0x2B): friend requests relayed and accepted on both sides, refusal, removal, online
  state (op 6) and a friend's region, ignore list kept in the database and merged with the client's local file,
  blocks applied by the server (whispers, trade, party and guild invitations). Mail (type 0x2D): a letter for one
  letter paper sheet, one attached item, offline recipient, mailbox sent on entering the game, reading, collecting,
  deleting. ArchCore's rules are described in [[Network protocol|EN-Network-protocol]]. Validated with the simulator
  on 28 October 2026 (`buddy` and `mail` scenarios), to be confirmed in game; the "Buddy" relation (mentor and
  disciple) is postponed.
- Wear and repair (types 0x04 and 0x34): durability (`DUR` column) stored and sent to the client ("item" factor),
  weapon wear on every hit dealt and armour wear on every hit received (`Item.WeaponWearChance`,
  `Item.ArmorWearChance`), an item at 0 has no effect until repaired, repair at blacksmiths and animal breeders
  (function 17) at the client's price. Mounts (types 0x04 and 0x4D): mounting by equipping the part 14 item,
  AgpmRide op 2 and op 4 broadcast, `Run_Buff` speed, seen by others (field 18), refused in combat, for a murderer and
  outside allowed regions, forced dismount on death. ArchCore's rules are described in
  [[Network protocol|EN-Network-protocol]]. Validated with the simulator on 28 October 2026 (`repair` and `ride`
  scenarios), to be confirmed in game; display by ArchRust in progress.
- Auction house (type 0x0B): board opened at the "Item Trader" NPCs (function 28), listing a whole stack at a fixed
  price in gold and CC (3 % fee), search by TID, instant purchase at the database price, completed sale announced to
  the seller, collecting, cancelling, sales sent on entering the game. Equipment switch (type 0x6A): 100 saved slots,
  re-equipped through the manual equipment path, read back on entering the game (Archonia has no set bonus). World
  clock shared by everyone (AgpmTimer, `World.TimeSpeed` speed, resent every 10 min) and weather (type 0x30:
  `/weather`, automatic weather `World.WeatherAuto`). ArchCore's rules are described in
  [[Network protocol|EN-Network-protocol]]. Validated with the simulator on 28 October 2026 (`auction`, `itemset` and
  `nature` scenarios), to be confirmed in game; ArchRust windows in progress. The ArchLord's sky skills (478, 522, 523)
  act on this clock and weather (see the ArchLord title).
- Summons (type 0x4F): Elementalist spirits, Sacred One, Legion of the Dead and the other skill summons (creature at
  the master's level, `Summons.StatScale`), fixed creatures (traps), Increase Gate (raised limit), summoning scrolls
  and item pets; view, special state then op 0 in the order the client requires, following the master, attack,
  defense and shadow propensities (op 2), monsters fighting back against the summon, experience and loot to the
  master, end on expiry, on death, when the master teleports or leaves, and beyond the limit. ArchCore's rules are
  described in [[Network protocol|EN-Network-protocol]]. Validated with the simulator on 28 October 2026 (`summons`
  scenario), to be confirmed in game; ArchRust target windows still to do. Taming, Call on Nature and Evil Light (no
  creature in the data) and rift traps remain.
- Chat: Archonia's prefixes read by the server ("/p", "/g", "/s" sent as type 0, before GM commands; a player's
  unknown command is ignored), local "[Shout]" (`Chat.ShoutRange`), area shouts (type 0x4C: `/race`, `/scream`,
  `/warn`, one item used up per message, one shout per second), chat rooms (type 0x2C: public, private, guild,
  invitations, player search; 5 per character, kept in memory). Stylists (type 0x43, function 29): hair style and face
  from `charactercustomizelist.txt` bought at the client's price, seen by other players; creation look checked.
  Wanted posters (type 0x46, function 32): the victim of a first attacker or of a murderer may put a price on the
  killer, base bounty by level, bounties added up and stored (`Wanted.ExpireDays`), board of 100 posters, bounty paid
  to the hunter, experience lost by the criminal (`Wanted.ExpLossPercent`). ArchCore's rules are described in
  [[Network protocol|EN-Network-protocol]]. Validated with the simulator on 28 October 2026 (`chat`, `channel`,
  `customize` and `wanted` scenarios), to be confirmed in game; ArchRust windows in progress. The `<Chat>No</Chat>`
  region rule is not applied (there is none in the data).
- Episode 8 battles and the original combat areas: Shrines (type 0x62, raw structure; `shrine` and `shrine_reward`
  tables of the content layer, as the original tables are not shipped): eight Shrines opening every evening
  (`Shrine.Enabled`, `/shrine open|close`), entry by level and head count (codes 47 and 48), carving at the centre,
  occupation, race victory, 53000 coins and charisma, scores, `shrinemessage.xml` messages; combat rules inside the
  Shrine (races are enemies without Ctrl, no murderer points or bounty, resurrection in one's race zone). Battle Square:
  floor levels imported, travel through the supervisors (`BattleSquare.SupervisorTeleport`, no portal leads there),
  codes 2, 13 and 18, kills without penalty, skulls from `pvpskulldroptable.txt`. Battleground: resurrection in one's
  race fort, 0x58 announcements on a schedule (`Battleground.Schedule`). Archonia's boss window (type 0x64): state of
  the `UIBoss.xml` bosses, possible loot, ranking of the last fight, damage meter; server time in AgpmTimer (fields 2
  and 3). ArchCore's rules are described in [[Network protocol|EN-Network-protocol]]. Validated with the simulator on
  28 October 2026 (`shrine`, `battlesquare` and `boss` scenarios), to be confirmed in game; Shrine op 8 (unknown
  meaning) is only logged, Archonia's race war (type 0x33) is not done, ArchRust windows in progress.
- Castle siege wars (types 0x49, 0x50 and 0x61): castles from `SiegeWar.txt`, assault objects from
  `SiegeWarObject.txt` and siege resurrection points imported (`siege_war`, `siege_war_object`, `siege_binding`
  tables); owner, calendar and applications stored (`castle`, `castle_application`). Client rules: the owner declares
  within its slot (base day, 8 to 10 pm), applications by the guild master on site (15 attackers, 3 defenders of which
  2 are chosen, one guild per castle), siege commanders (function 33), object attack order, throne, Archon's Eyes,
  carving by the master (the last one wins). ArchCore's choices: castles start without an owner, date set
  automatically without a declaration, every attacking applicant kept, siege cancelled without attackers, the former
  owner keeps the castle without a successful carving, `Siege.MinGuildMembers` and `Siege.MinGuildAgeDays`
  thresholds, `Siege.DurationMinutes` length, `Siege.Enabled` calendar. Siege objects and guards only shown during
  the battle, "being destroyed" then ruin variants; fighting without Ctrl between sides, no murderer points or bounty;
  siege resurrection (AgpmCharacter 0x13 and 0x14); castle trips (`SpecialType` 2 to 4); catapults and resurrection
  camps; WarInfo board; `/siege start|end|owner|hp` commands. Validated with the simulator on 28 October 2026 (`siege`
  scenario), to be confirmed in game; guard towers and guards now only target attackers; the lord's hunting ground
  contents and the exact catapult effect remain to be established.
- ArchLord title (type 0x55): Rensphere war every 30 days at 8 pm (`Siege.Enabled`) or with `/siege start Rensphere`,
  guilds owning a castle as attackers, steps 1 to 4 (portal, fight, ArchLord battle at status 7, end), seal carved by the
  master (skill 930); that master becomes ArchLord until the next war (`archlord` table), announced to everyone (ops 0
  and 1) and on entering the game. `skill_type` 9 skills given to the ArchLord and refused to anyone else: Call Dragon
  (Alcadia, AgpmRide op 6), Call Royal Knight and `/guardactivate`, `/guarddeactivate`, `/guardinformation` (royal
  guards, `Archlord.MaxGuards`), Rain and Snow Control (rain or snow for 5 min for everyone), Day and Night (clock
  shifted by 12 hours, `Archlord.SkyMinutes`), `/archlord <message>` to the whole server; test commands
  `/archlord set|clear`, `/archlordbegin`, `/archlordend`, `/nextstep`…
- Region taxes (type 0x53): "Grand Advisor" stewards (function 34), 0 to 50 % rates for the castle and outskirts regions
  set by the owner once a week (codes 0 to 3), op 4 to everyone with the region name translated by the player's language
  `RegionGlossary.txt` (`region_glossary` table, imported); price + price × rate / 100 on NPC purchases, repair, stylist,
  portals, skill master, production and guild creation (neither sales to NPCs nor sales between players); income per
  region (`castle_tax`, `castle_tax_region`), paid every Monday at midnight (`Tax.PayoutDay`, `Tax.PayoutHour`) into the
  bank of the owning guild's master, minus the 20 % tribute to the ArchLord; `/tax` and `/tax pay`. Validated with the
  simulator on 28 October 2026 (`archlord` and `tax` scenarios), to be confirmed in game; skill 472 Evil Light (summon
  kind 7) and the guards' Archon item remain to be established, removed skills only leave the client's bar on
  reconnection.
- ArchRust client (`client` branch): combat (attacking, hits, death, resurrection, loot), parties, other
  players, inventory and equipment, NPCs (dialog, shop, bank and buying pages, quests), skills (trainers,
  casting, shortcut bar), teleport portals (original window and loading screen, confirmation of player-versus-player zones, "Return",
  refusal in combat), automatic reconnection (`UI_ReconnectHold` window, 60 s), guilds (registrar, creation,
  disbanding and guild windows, members, guild name above heads) and guild and party chat, whispers (private bar, `/w`, `/r`, guild
  window button), graphics and sound resource packs (enabled,
  ordered and checked from the Options; see the [guide](https://github.com/BlaMacfly/ArchCore/blob/client/ArchRust/docs/RESOURCE-PACKS.en.md)), original Archonia windows instead of the temporary ones (confirmations, stack split,
  buying, bank, party, main menus, character select and creation, options), movement speed like Archonia (the
  GM's `/speed` visible), potions drunk automatically below the gauge sliders' threshold, GFX, Display and
  Function options wired, AddOns that can be toggled, client texts in the chosen language; verified against ArchCore by headless runs and screenshots of the graphical client, to confirm in game.
- Player visibility: movement relayed to nearby players, appearing and disappearing by distance.
- Chat within view range, and whispers.
- GM level sent to the "Admin Commands" panel.
- First GM commands: rates, moderation, items, restart announcement, teleport. See
  [[Administration|EN-Administration]].

## Next steps

1. Full character creation and visibility between two players: checked on 25 October 2026 with two ArchRust
   clients on the test server (creation, arrival in Golundo, refusal of a taken name, players visible to each
   other). Still to confirm with the Archonia client.
3. NPC functions: quests with interaction objects. Portals: disabled points (op 5 field 5); refusal while
   transformed to check once transformation exists.
4. Skills: damage reflection (counter attack), monster skills, party skills and masteries validated with the
   simulator on 25 October 2026; damage sharing (Divine Protection, Martyr's Mark), dispels (Antimagic, Cure,
   Disentangle…), ignored defense (Lethal Attack, Stunner), heroic points earned with levels and charisma (saved, spent
   by heroic skills; earning it waits for player-versus-player fights, `/charisma` meanwhile) validated with the simulator on
   26 October 2026; all to be confirmed in game. Skill criticals remain.
5. Item conversion (sockets, spirit stones, reinforcement) and elemental damage on normal hits: checked with
   the simulator on 25 October 2026, still to confirm in game; item options (rolled on creation, effect on the
   wearer); runes; skill runes (ice, lightning and poison prayer stones) and triggered options (critical, stun,
   wound, HP returned, regeneration, evasion, ignored defense) validated with the simulator on 26 October 2026, to be
   confirmed in game; Archonia's ice, lightning, venom and stat stones refused as the client does; unique items'
   linked options (`Link_id`) and refining (alchemy sphere, quote, grid and
   recipes) done.
6. Guilds: alliance chat; effects of hostility in combat (to be established).
7. Automatic reconnection: to be confirmed with the Archonia client.
8. Remaining GM commands on the [[Administration|EN-Administration]] page: they wait for their systems (arena,
   events, rifts, region settings…), listed at the end of its section 4.
9. Finish the translation: about 54,000 texts left.
10. "Buddy" relation (mentor and disciple, AgpmBuddy ops 10 to 14): the `MentorConstrict` limits and the "inventory
    emptied" effect are unknown; waiting for a reliable source.

## Later

- **PlayerBots**: a server module for player characters driven by the server, modelled on the
  PlayerBots of World of Warcraft servers and adapted to ArchLord (movement, combat, parties, trade).
  To start once monsters, combat and parties work.
- **Add-ons built into the client**: ship ready-to-use add-ons with ArchRust, built into the client and
  enabled from the game, on top of the Lua add-on system already in place. First add-on:
  [ArchCoreAdmin](https://github.com/BlaMacfly/ArchCoreAdmin), the GM panel.
- **Database editor**: under way in [ArchForge](https://github.com/BlaMacfly/ArchForge) — a free graphical tool for Windows, Linux and macOS that creates and edits the world
  content without writing SQL: creatures and their spawns, vendors, trainers, quests, items, loot tables,
  dialogs, conditions and monster behaviour. Every field is explained, selectors help pick values and flags,
  and the generated SQL query is shown (only the changes, or the whole entry for a backup), to copy or to run
  directly on the database.

Contributions are welcome: open an issue to suggest or report something.
