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
  remains to be checked.
- Melee combat against monsters: approach, hits (hit chance, class-based damage), death, experience and
  level up (stats, HP and MP from GrowUpFactor), corpse then respawn. Monsters that are hit fight back and chase their attacker; player death
  and resurrection in town. Drops: gold and items on the ground, pick-up.
  Equipment: weapons and armour worn (restrictions, two hands), counted in damage and defense, visible
  to other players; moving items in the inventory. Life and mana potions, out-of-combat HP and MP
  regeneration. Teleport scrolls (town, round trip, destination); monster patrols. Resurrection at the point of
  the last safe region crossed; item stacks split, merged, destroyed. Skills: learning and upgrading at
  trainers, skill points, casting; damage, areas, buffs and debuffs, damage over time, heals, passives;
  special states (stun, sleep, slow…), critical hits, block, damage turned into HP or MP.
  Parties (invitation, members, leader, kick, experience sharing, chat). Guilds (creation at guild managers, invitation, joining, leaving, kicking, disbanding, notice, chat). Account bank (items, money, extra pages) and trading between players. Teleport portals (paid trips between linked points). Quests: taking, progress (monsters, quest
  items), completion and rewards, abandoning. See
  [[Network protocol|EN-Network-protocol]].
- ArchRust extensions: capability negotiation, AddOn messages (whisper, `SAY`), and the client language
  (version 1.1): the server serves NPC and monster names in the chosen language, with fallback, and the
  other texts on demand; the choice follows the account. See
  [[ArchRust extensions|EN-ArchRust-extensions]] and [[Translation|EN-Translation]]; on the ArchRust side,
  a language selector in the Options.
- Selling an item to an NPC.
- ArchRust client (`client` branch): combat (attacking, hits, death, resurrection, loot), parties, other
  players, inventory and equipment, NPCs (dialog, shop, bank and buying pages, quests), skills (trainers,
  casting, shortcut bar), teleport portals (original window and loading screen), guilds (registrar, creation,
  disbanding and guild windows, members) and guild and party chat; verified against ArchCore by headless runs and screenshots of the graphical client, to confirm in game.
- Player visibility: movement relayed to nearby players, appearing and disappearing by distance.
- Chat within view range, and whispers.
- GM level sent to the "Admin Commands" panel.
- First GM commands: rates, moderation, items, restart announcement, teleport. See
  [[Administration|EN-Administration]].

## Next steps

1. Verify full character creation in game.
2. Verify player visibility with two real clients.
3. NPC functions: quests with interaction objects. Portals: player-versus-player zones (confirmation, return,
   op 7), refusal in combat or while transformed.
4. Skills: reflecting damage to the attacker, monster skills, party skills; masteries.
5. Combat: elemental item damage. Items: refining, conversion.
6. Guilds: guild battles, alliances and hostilities, guild marks, sub-master and handover, enlarging, guild
   list, warehouse. In ArchRust: guild name above heads, whispers.
7. Automatic reconnection (type 0x63).
8. Remaining GM commands on the [[Administration|EN-Administration]] page (arena, events, rifts…).
9. Finish the translation: about 54,000 texts left.

## Later

- **PlayerBots**: a server module for player characters driven by the server, modelled on the
  PlayerBots of World of Warcraft servers and adapted to ArchLord (movement, combat, parties, trade).
  To start once monsters, combat and parties work.
- **Add-ons built into the client**: ship ready-to-use add-ons with ArchRust, built into the client and
  enabled from the game, on top of the Lua add-on system already in place.
- **Database editor**: a free graphical tool for Windows, Linux and macOS that creates and edits the world
  content without writing SQL: creatures and their spawns, vendors, trainers, quests, items, loot tables,
  dialogs, conditions and monster behaviour. Every field is explained, selectors help pick values and flags,
  and the generated SQL query is shown (only the changes, or the whole entry for a backup), to copy or to run
  directly on the database.

Contributions are welcome: open an issue to suggest or report something.
