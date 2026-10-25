# Game episode

Each major ArchLord update was released as an **episode**. This page tells where ArchCore stands,
separating two things: the **world** loaded (game data) and the **systems** already programmed.

> **Marker.** ArchCore loads the world of **Episode 8, "The Dark Revelation"** (Webzen,
> 26 September 2012), plus the extensions of the Archonia server, whose 64-bit client is the base. The
> gameplay systems programmed so far cover the core of the original release; those of later episodes are
> still to catch up (see below).

## ArchLord episodes

| Episode | Name | Date | Main content |
|---|---|---|---|
| — | Original release | 2005 (Korea), 2006 (Europe) | Humans, orcs, moon elves; combat, quests, the ArchLord title |
| 2 | Season of Siege | 2007 | Castle sieges; move to free play |
| 3 | Spirits Awakening | August 2008 | 3 zones, over 80 skills, ArchLord powers, armour sets |
| 4 | Chaotic Frontier | March 2010 | Dungeons (6 worlds), levels 31 to 70 |
| 5 | name not found | 2009-2010 | Swashbuckler class (October 2009, link to episode 5 not confirmed) |
| 6 | DragonScion | 16 December 2010 | 4th race: Scion, then Slayer, Orbiter or Summoner at level 20 |
| 7 | Ascent From Hell | July 2011 | Level cap from 99 to 120, Heaven's Hell zone, renewed sieges |
| 8 | The Dark Revelation | 26 September 2012 | Shrines (team battles, Shrine coins), rebuilt Human and Orc cities, extra skill slot |

The official service closed on 1 January 2014. The names of episodes 1 and 5 were not found.

## What the data contains

Client data (`data/client-ini`, Archonia client) and server tables (`--server`):

| Marker | Episode | Present |
|---|---|---|
| Swashbuckler class | 5 | ✅ items and class known to ArchCore |
| Dragon Scion race and its classes | 6 | ✅ templates, "드래곤시온" zones, skills |
| Experience table up to level 120, monsters up to level 120 | 7 | ✅ `level_exp` comes from the client (the server tables' table stops at 100) |
| Heaven's Hell zone ("헤븐즈 헬의 옥좌") | 7 | ✅ client and server |
| Five elemental Shrines (fire, water, wind, lightning, poison), each split by race; "Shrine" potions | 8 | ✅ client and server |
| Battle Ground (two fields) | 2012 Webzen update | ✅ regions |

**Archonia additions**, absent from the original game and from the server tables: 31 regions of the client
only (English or placeholder names: The Shattered Firmament, Archonia expansion of 24 April 2026; Omega Reactor, The Elemental Forge,
Spookywood Forest, Throne of Fire, Lava Temple Ruins, Guild vs Guild Area…), rifts (`Rifts.xml`), the battle
pass (`battlepass.xml`), the Battle Royale arena, Oreads and Elixirs, the Nightfall armour. ArchCore does
not program them: they stay out of scope until a decision brings them in.

## Fidelity to Archonia's content: NPCs, quests, monsters

Check of 25 October 2026, Archonia client data and `Archonia.exe`:

| Content | Source in ArchCore | Status |
|---|---|---|
| NPCs | Archonia client's `npc.ini` (567 NPCs, positions, functions, shops, quest groups) | ✅ copy of the client |
| Quests | `questtemplate.ini` of the client's `Ini` folder (53 quests) | ✅ like the client: at start-up, `Archonia.exe` only reads `IniQuestTemplate.ini`, whatever the language. The `ini_ko` table (3,321 quests, those of the original game) is not loaded: group quests missing from it are offered neither by the client nor by ArchCore |
| Monster stats | Archonia client's `CharacterDataTable.txt` | ✅ copy of the client |
| Monster level | client's `CharacterDataTable.txt`, instead of the server tables' level | ✅ Archonia raised 70 monsters (MoonElf guards to level 120, Amantorg to level 92…): 1,148 spawn points corrected |
| Boss respawn | Archonia's boss window (`UIBoss.xml`): delay drawn in the announced range | ✅ 16 points (Battlesquare 30 to 45 min, Asylum and Balor King 30 to 60 min, Chaotic Frontier 20 to 40 min, Shinewood invaders 75 to 90 min) |
| Monster placement | original server tables (`spawndatatable.txt`, `spawninstancetable.txt`) | 🟡 spawns are handled by Archonia's server and absent from the client; the original game's are used |
| Archonia's own monsters | — | ⛔ 13 bosses of the boss window (Heaven's Hell, The Shattered Firmament, Purgatory, new Chaotic Frontier and Asylum bosses), 23 quest targets and the 46 rift monster groups (`Rifts.xml`) have no known placement. The client does not give it: they are left out rather than invented |

## Systems programmed, by episode

| Episode | System | Status in ArchCore |
|---|---|---|
| Original | Login, characters, world entry, NPCs, movement, view | ✅ verified in game |
| Original | Combat, monsters, loot, equipment, potions, scrolls, skills | 🟡 verified with the simulator |
| Original | Quests, parties, bank, trading, shops, teleport portals | 🟡 verified with the simulator |
| Original | Guilds (creation, members, leaving, kicking, disbanding, notice, chat), party chat | 🟡 verified with the simulator |
| Original | Guild battles, alliances, guild marks, guild warehouse | ⏳ to do |
| Original | Battle Square | ⏳ to do |
| 2 | Castle sieges | ⏳ to do |
| 3 | ArchLord powers | ⏳ to do |
| 4 | Chaotic Frontier dungeons | 🟡 zones and monsters loaded; dungeon rules to check |
| 6 | Playable Dragon Scion | 🟡 data and skills present; creation to confirm in game |
| 7 | Level 120, Heaven's Hell | 🟡 table and monsters loaded; renewed sieges to do |
| 8 | Shrines and Shrine coins | ⏳ to do |
| — | Masteries, heroic skills | ⏳ to do |

The catch-up order follows the [[roadmap|EN-Roadmap]]: guilds, then sieges, then ArchLord powers and
Shrines.

## Sources

- [Wikipedia — ArchLord](https://en.wikipedia.org/wiki/ArchLord)
- [Engadget — free play, Season of Siege](https://www.engadget.com/2006-12-06-archlord-moves-to-free-play-codemasters-desperate.html)
- [RPGWatch — ArchLord news (Spirits Awakening)](https://rpgwatch.com/games/archlord-127/news.html)
- [elitepvpers — Chaotic Frontier, episode 4](https://www.elitepvpers.com/forum/archlord/502635-webzen-archlord-chaotic-frontier-episode-4-info.html)
- [Webzen — Swashbuckler (2009)](https://company.webzen.com/en/pr/pressview?page=93&seq=274)
- [Archlord: Episode VI – DragonScion](https://mmorpguk.wordpress.com/2010/12/09/archlord-episode-vi-dragonscion/)
- [MMOBomb — Episode 7 Ascent From Hell](https://www.mmobomb.com/news/webzen-all-in-one-updates-and-events)
- [Webzen — The Dark Revelation (2012)](https://company.webzen.com/en/pr/pressview?page=8&seq=311)
- [Archonia — The Shattered Firmament](https://archonia.net/news/150/archonia%20expansion%20release%20-%20the%20shattered%20firmament?culture=en)
