# Status and roadmap

> **Episode.** World of Episode 8 "The Dark Revelation" (2012); game values from the original episode 8
> server tables (Archonia additions dropped); systems of the original release. Detail by episode: [[Game episode|EN-Episode]].

## Working with the real client

- Server and tools entirely in Rust (a single `archcore` executable).
- Encrypted handshake, world list, account login.
- Character list and creation screen.
- World entry, with a starting village per race.
- Spawning of the NPCs around the player (557 since the switch to episode 8 values), with translated titles.
- French client interface, accents included (translation in progress).
- Player movement, with position saving.
- NPC dialog: the npc.ini functions (dialog, shop, quests) are sent with each NPC, and clicking opens
  the dialog window.
- NPC shops: Darryl's shop (Anchorville) shows its 43 items, and buying takes the gold and puts the
  item in the inventory.
- Character inventory shown on world entry.
- Inventory: five bags of 25 slots (125 total), matching the client grid; existing item coordinates are preserved.
- Necklace wing/cloak runes: socketing locates the rune in normal or Cash inventory, rejecting active items; On/Off consumables remain distinct. The client displays socketed rune effects and names.

## Verified with the simulator, to confirm in game

- Creatures: 36,960 monsters placed from the original server tables, off blocked ground, with their stats
  (level, speeds, HP). Seen and targeted in game during a first test; sending their HP, added since,
  remains to be checked. Levels from the template in the episode 8 tables and boss respawn from its boss window;
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
  masteries (required skills of the high and heroic trees, levels in a template's class "1722;30", Dragon Scion
  passives "10004;4"); monster skills (AI from the original server tables:
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
- Cash inventory and pet bag (type 0x04, statuses 51 and 13): items marked "Cash" stored in the Cash inventory (Cash
  mini-packet in field 23, Cash items already in the database moved when entering the world); a pet in use = item "in
  use" (op 26), recalled by using the item again or by op 23; 4 × 2 × 4 pet bag available with a "PetType" 1 pet:
  deposit from the inventory, equipment and bank, withdrawal, stack merging and splitting, items kept when the pet is
  recalled, loot into the bag when the inventory is full, "Auto Pick Up" button (op 30) saved. Left out (Archonia
  additions): bag marking (ops 37/38), "Sell All", pets 14821 to 15674. ArchCore's rules are described in
  [[Network protocol|EN-Network-protocol]]. Validated with the simulator on 1 November 2026 (`petbag` scenario), to be
  confirmed in game; ArchRust `UI_SubInventory` and `UI_CashInven` windows still to do. Pet stamina (Pet Feed) and the
  Chantra shop remain.
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
- Server-side anti-cheat (`world/anticheat.rs`, no client-side check): positions sent by the client bounded by a
  distance credit (current run speed × `AntiCheat.SpeedTolerance` + `AntiCheat.MoveSlack`, up to 5 s of lag), valid
  position sent back beyond it; server teleports, entering the world and reconnection as starting points, GMs exempt;
  normal hit rate bounded after a target change or a resume; packet rate per connection
  (`AntiCheat.MaxPacketsPerSecond`), floods disconnected; `log_anomaly` table, `log`, `kick` or `ban` sanctions beyond a
  threshold, `/anomalies`, `/move x,z "name"` to place a player. Duplication audit: refining with the same item listed
  twice (ingredients counted twice) closed; trade cancelled if an offered item changed since it was offered, after an
  unlock the exchange waits for both locks again (the partner keeps its own, as in the reference client), a request
  cannot be accepted by its author; offered items can no longer be converted, sold,
  refined or put in the guild warehouse; ground gold beyond the cap left on the ground; second world entry on one
  connection. Validated with the simulator on 29 October 2026 (`cheat` scenario, and the existing ones), to be confirmed
  in game.
- Test report no. 1 (29 October 2026), server side: NPC and object use ranges aligned on Archonia's shared code (600
  dialog and shop, 800 bank, conversion, guild, skill master and stylist, 1600 repair, quests, production, auction,
  portals and alchemy spheres, instead of 1500 everywhere), measured on the position tracked at that moment;
  refining limited to the 20 alchemy spheres of the `obj*.ini` files; disabled portal points (destinations without
  an arrival object or forbidden to the race); Berserker heroic tree read (two trees, shifted columns). Validated with
  the simulator, to be confirmed in game.
- Metamorphosis (`world/morph.rs`): usable type 7 potions (101 items, form "Polymorph Id", duration "Polymorph
  Dur") and ItemTransformTable.txt bonuses (`item_transform` table) added to the factors; AGPMCHARACTER op 11 to the
  player and those who see them (view: op 0 with the original TID, bit 0 of field 18, then op 11), op 12 when the
  duration ends, on cancellation (client op 13) or on death; AGPMITEM op 4, op 19 (shared 10 min cooldown), op 27 code 4
  when mounted, transformed or evolved, code 2 in a siege zone; form lost on logout, cooldown kept per session. Client
  refusals repeated (skills, scrolls, Potions of Quickness and Beam, portals, Shrines, NPCs, stylist, private trade when
  either side is transformed), except for the ArchLord; equipment allowed with the ordinary check on the original
  factors (`FUN_1401bca10`), removal checked without level while transformed or evolved (refused under an evolved form
  that does not allow the item), mounts still refused while transformed (ArchCore's choice); original race for
  player-versus-player rules. Creature form (CharacterDataTable Type 8: Mutation animals, potions 408 to 416): the
  player attacks nobody and no player attacks them; form without 0x20000: not the target of an enemy skill. Mutation
  (`transform_target` 4): form drawn from `creature_tid` for 7 s, no cancellation, on a player or a monster (no
  summons, siege objects or boss-window bosses); the monster stops, its AI waits for the end, it can be neither
  attacked nor targeted under an animal form (op 11 / op 12 to those who see it, op 11 after its view); then 30 s of
  immunity (SKILL_SPEC `downtime_duration`, deduced meaning, `Skill.DowntimeSeconds`). `Item.TransformMaxDurationMs`
  shortens the duration (simulator). Validated with the simulator on 1 November 2026 (`morph` scenario: equipment and
  trade while transformed, Mutation of a Green Worm), to be confirmed in game; on the ArchRust side, still to do:
  equipment not redrawn during the form, trade menu, animal target.
- Dragon Scion evolution (`world/evolution.rs`): Slayer, Orbiter and Summoner of Evolution (1189, 1192, 1195), learnt
  from the Dragon Scion skill masters from level 20; casting → AGPMSKILL op 4 (effect without duration) then
  AGPMCHARACTER op 26 (1722, 1723, 1732); from one form to another, op 27 then op 26; casting the active form again is
  refused; cancellation by AGPMSKILL op 7 action 4 → op 5 and op 27 (1724). Form's factors and growth, HP and MP
  capped, skills and passives by the form's `require_class_*` columns, weapons not allowed no longer counted, skill
  reset item refused; form saved (`characters.evolution_tid`), kept on death and restored on entry (form's TID, bit 2
  of field 18). Validated with the simulator on 1 November 2026 (`evolution` scenario), to be confirmed in game; the
  ArchRust client still has to handle these packets.
- Secondary stats of the character window (`world/effects.rs`): AGPMSKILL op 3, field 9 (220-byte block: critical
  rate and damage, hit taken turned into HP, ignored defense and resistance, critical hit and stun resistance) sent
  to the player on entering the game and on every change of their effects, skills' share only (passives included):
  the client adds the items' share itself. Critical hit resistance (`critical_defence_percent2`,
  `Critical_Defense_Rate` option) subtracted from the attacker's critical hit chance (deduced rule); stun resistance
  (`stun_resistance`, `StunResistance`) Archonia-only, no longer read since 2 November 2026; an option with `Critical` and no `skill_type` (13822)
  counts as critical damage, as in the client. Validated in the simulator on 1 November 2026 (`ignore` scenario), to
  be confirmed in game with the ArchRust window.
- Consumables, death and skill reset (`world/consumables.rs`): skill items (stews, speed potions, stones, wings,
  charms, costumes, Janus talismans: "Use skill id" at "Use skill level", ";" lists) that apply their effect without
  casting, for the level's duration or else the item's ("Apply Effect Count" × "Apply Effect Time", "RemainTime" /
  "ExpireTime" in minutes), cooldown "Use Interval" + count × time per item and `recast_time` per skill (refusal:
  AGPMITEM op 5), `skill_classify_id` / `_level` families (an equal or higher rank replaces, a lower one is refused),
  "CanStopUsingItem" items kept and switched off when used again; elemental defense of effects ("def_point_*");
  experience and loot bonuses ("bonus_exp", "bonus_drop_rate", unit set by `Skill.GameBonusDivisor`). Resurrection
  orb (use type 14): dead players only (op 5 otherwise), refused with op 27 code 3 (ArchLord, siege at war, region
  without bit 0x100, combat region without a Cash orb, levels, cooldown); on the spot, full HP and MP, invincible for
  10 s, op 4 then op 18. Skill reset with the Gem of origin (use type 27) or at the skill master (AgpmEventSkillMaster
  op 8 → op 9, once, gold of the learnt levels refunded, "skill init" string saved in `characters.skill_init`, field
  19 and op 23); ArchLord and production skills and the normal attack are kept. Experience loss on town resurrection
  can be set (`Death.ExpPenaltyPercent`, `Death.ExpPenaltyRoguePercent`), off by default as in Archonia, never a lost
  level. Validated with the simulator on 1 November 2026 (`consumables` scenario), to be confirmed in game; left:
  the lifetime of timed items (not counted down) and item effects lost on death and on logout.
- Small episode 8 systems: party recall (Summoning Orb, Bead of Summoning, usage 15: AGPMITEM op 4 to the caster,
  management op 15 to the members, op 16 accepts and teleports next to the caster, op 17 declines; refused when dead,
  in a siege, in a Shrine during a battle, in the Battle Square, in a region without bit 0x100 and in combat, `Recall.*`
  settings); social actions (AGPMCHARACTER op 16, types 0 to 16, sent back to the player and to those around, refused
  when dead, mounted, transformed, `Social.MinIntervalMs`); Shrine shop (charisma and Shrine Coin prices imported from
  itemdatatable, quantity, codes 10 and 11; no NPC of the data sells such items, the mechanism is ready); quick belt
  kept by the server (AgpmUIStatus: 50 slots on entry, op 1 saved, `character_quickbelt` table, skills found again by
  TID, used-up item replaced); return to character selection (AgpmReturnToLogin 0x2F: op 1 → op 5 with a key, op 6 to
  the login server, op 7 after logging in). Validated with the simulator on 2 November 2026 (`recall`, `social`,
  `quickbelt`, `returnlogin` scenarios), to be confirmed in game; which NPCs run the Shrine shop is still to decide.
- Episode 8 titles (AgpmTitle, type 0x60, raw structure): 684 titles of `titledatatable.txt` imported (`title_template`
  table; Expert Collector, Addict, Advanced, Researcher; Defender of Chantra and Schola 1-3 behind `Title.Schola`,
  Archonia's Christmas Event and Schola 4/5 left out), charisma ranks (`charisma_title`). Lists and active title pushed
  on entry (op 5, 6.7, op 9); "<Title Manager>" (op 10 instead of the dialog); goal set with the client's checks and
  bounds (999, 500), gold and cost items taken (6.0 → 6.1, 6.3); conditions of gathering on a creature, using an item,
  level and production levels tracked (6.3); title obtained (6.5 then op 1), equipped or removed (op 3, op 9 to the
  player, to those around and to an observer who sees the holder arrive; one change every 2 hours,
  `Title.ChangeDelaySeconds`, refusal code 3), deleted or given up (op 8); effects of the equipped title only: its
  `Skill_Tid_*` skills as passives, production success (+10 %), doubled gathering on its creature, stronger potions;
  `character_titles`, `character_title_goals` tables and `characters.title_changed_at`; `/title`. Validated with the
  simulator on 2 November 2026 (`title` scenario), to be confirmed in game with ArchRust's windows; when the lists are
  sent, when costs are taken and the meaning of op 2's constant 2 are ArchCore's choices (the original server is
  missing).
- Remaining episode 8 consumables (`world/consumables.rs`, `bank.rs`, `convert.rs`, `trade.rs`): Return Orb (usage
  13, "Reset Selected" button: AGPMSKILL op 13 → skill removed with op 1, skill and heroic points given back, orb used
  from the inventory then the Cash inventory; skills that cannot be forgotten and prerequisites of another learnt
  skill are refused); Secret Safe (usage 21: bank pages 3 and 4 without a keeper or gold, once page 2 is bought, cap
  `Bank.SecretSafeMaxExtraPages`, messages 29 and 30); Release of Agreement (usage 28, AGPMITEMCONVERT op 12: stones
  and runes removed, sockets and reinforcement kept, op 0); `/go`, `/safekeeper`, `/merchant` commands (usage 24, bank
  and shop of the nearest NPC opened from afar) and `/nick` (usage 26, signature in field 27, `characters.signature`),
  only with the item, 30 min cooldown; fireworks (usage 22, op 4 to those around; Chantra shop packages left out);
  Fortune Manual (usage 25, switched on in the Cash inventory, bonus `Convert.LuckyScrollBonus` 0 by default, original
  value unknown); Talisman of Protection (usage 19, switches on without effect: no item loss on death, as in the
  original data — pvpitemdroptable.txt all 0, "bPCDropItemOnDeath" 0, client text "Holding items will not be dropped
  even in case of PK death"); the trade's Ex grid, which is episode 8 after all (Marvel Scroll of Release, usage 35,
  op 9 kind 2) and bound items ("BoundType" 1) refused in a trade without a scroll, one per trade. Validated with the
  simulator on 2 November 2026 (`usables` and `trade` scenarios), to be confirmed in game: signature display, firework
  effect for those around, replies to ops 13 and 12, Ex grid slot. Left out: skill books (11, no skill in the data), Map
  of Asylum (16, nothing on the server), Plastic Surgeon Coupon (32), quest scrolls (33); Sentence Combination (23):
  effect unknown. ArchRust side: "Reset Selected" button (op 13), dragging the Release of Agreement (op 12), Ex grid,
  local `BankSlotFull` refusal.
- Rules carried by the episode 8 tables (`combat.rs`, `effects.rs`, `skills.rs`, `options.rs`, `ai.rs`, `regions.rs`):
  monster evasion (CharacterDataTable `DR` in the hit chance); `point.dmg_min/max_*` buffs (elemental damage),
  `point.physical_resistance` and the `PhysicalAttackResist` option (physical damage taken reduced, 80% at most for a
  player; monsters' `Physical_Resistance`), `point.skill_block`, the `SkillBlockRate` option and monsters' `Skill_Block`
  (offensive skill blocked, 80% at most), `point.atk_range` (metres); monster target preference (aidatatable
  `HateRace`, `HateClass`: weighted damage, distance divided for a preemptive monster); shorter casting and cooldown
  (`point.skill_cast`, `point.skill_delay`, `skill_cast`, `skill_delay` options in attack factors 3 and 4,
  `applied_cooldown_cap` limit) and MP (`cost_mp_decrease`); `Ignore_*` options (seven penetrations, in points),
  `bonus_exp`, `bonus_money`, `bonus_drop_rate` (percent) and `Add_Debuff_Duration`; skills' `bonus_money`; option
  block (`BlockRate`) and evasion against monsters and between players; Skill_Const2 second hit (`damage_*2`, 15
  skills); regions `DisableSummon` (party recall), `DisableGo` (`/go`), `DisableParty`, `DisableLogin` (placed at the
  region's point or in the village), `ItemSection` with `Item_section_num` (items that do not count in Heaven's Hell)
  and `ResurrectionX/Z` (resurrection inside the region). Options' `Item_Level_Min/Max` do not drive the roll (neither
  Archonia nor the original table use them). Archonia-only columns: the 589 options with a `CategoryName`, all from
  Archonia, are never rolled (only known for an item already in the database); no longer read: `StunResistance` and
  `magnify.stun_resistance` (slot 0xbc of the secondary stats block at 0); CharacterDataTable's
  `Critical_Defense_Rate` was not read. Formulas chosen, no binary shows them (see
  [[Network protocol|EN-Network-protocol]]). Unit tests; simulator scenarios rerun on 2 November 2026 (full series: only `divide` and `guildstage` fail, as before), to be confirmed in game.
- Remaining data rules (2 November 2026, episode 8 table values; tunable `Rules.*` proposals, no binary shows them):
  heroic system (`heroic.rs`: heroic damage and defense of items, options, effects and monsters, melee, ranged and
  magic resistances of effects; heroic part at point 19 of normal hits, of skills with `dmg_heroic`,
  `dmg_heroic_percent` and the level-gap penalty, and of damage over time `dot_dmg_heroic`); ammunition (`ammo.rs`:
  one arrow 194 or bolt 238 per hit with a bow or crossbow, skill `cost_arrow`, results 2 and 3, refusal 12, kataria
  and chakram need none); required weapon (`requirement.rs`: `requirement` mask, refusal action 8, passives without
  effect, disarm); timed items (`lifetime.rs`: `RemainTime` play time counted while in use, absolute `ExpireTime`
  date, full Cash mini, op 26, system message 5 then the item is consumed; quest items and started timed items are
  neither traded nor sold); statuses (Archonia codes 26 to 33 mapped back to the skill's EP8 value, 9 halt, 10
  invisibility; rules for 5, 6, 7, 18, 19, 21, 23, 24, 25: `status_rules.rs`); skill effects (`skill_extra.rs`:
  `move_pos`, Body Check knockback, Death Strike result 13, `at_field` ground zones of skills and summons, chains 3
  and 4, summons' additional skills, `skill_levelup` 1, 2, 4, detection, Redemption through AGPMCHARACTER op 24,
  `disturb`, `target_restriction`, provocation); monster AI (`ai_kin.rs`: `SummonTID` summons, `FollowerTID`
  followers, `PinchMonster` reinforcement); boots `PDR` in evasion, ring stat bonuses, `QuestItem` = quest number
  (item destroyed when the quest is abandoned or completed). Not done: pet stamina (Archonia does not show it: pet
  "Immortal"), taming (no skill in the tables), `Skill_plus` (field 21: the `skill_levelup` option still applies to every skill). Unit tests; `ammo` and `lifetime`
  scenarios (new), skill scenarios now hold a weapon; to be confirmed in game.
- Last points surveyed in Archonia (4 November 2026): AGPMSKILL op 10 (visual effects: 1 end of Time Attack, 2 forced
  move and teleport, 4 HP absorbed, 6 hit turned into HP, 7 invisibility (others drop their target), 8 Lens Stone, 9
  detection, 10 friendly purge; the client does nothing with 5, no table carries 3); op 16 (level bonus of one skill: `skill_levelup` 2 and 4); HP absorption
  (`convert_type` 4 Life Extinction, 6 Vampiric Impact); chains 1 (Time Attack: damage at the end of the effect), 2 (Shadow
  Chaser, Ambush: teleport to contact), 6 (Bomba: the creature explodes when it dies or its duration ends), 7 and 8 (Scion
  combos: "…2" columns only if the target bears the `Connection_TID2` effect or the caster is invisible: the second hit is
  no longer unconditional); ground targeting (`range` 701 Narcolepsy, Imposed Fatigue, Meteoric Shower; 702 Concealment,
  ShadowIllusion: caster's guild); GUILD op 46: round in battle field 14 only. Set aside, with proof: heroic reflection
  (columns empty on every table row), timed item pause (op 25: no column or text says when), stamina (op 29: empty mini,
  decoded without storing anything), <Battle> and <Event> Ability (chain 6: nothing casts them). Lottery boxes and keys
  (`lottery.rs`, usages 17 and 18, `itemlotterybox.txt` table, `Rules.LotteryKeys`); costume sets (`avatar.rs`, usage 31,
  `avatarset.ini`: pieces of parts 16 to 21 worn with no effect on factors). Unit tests; `chain` scenario (new), `consumables`
  extended.
- Decisions of 8 November 2026: **episode 8 gacha** ("Fortune Gate", AgpmEventGacha 0x57, `gacha.rs`): town pedestals
  (function 38 objects, `gacha_object` table: weapons 9081, armour 5780, accessories 5891, charisma 13173), EP8 draw
  types and tables (`gachatypetemplate.ini`, `gachadroptable*.txt`), drawable items from their `Gacha_Type_Numer`,
  `GachaRank`, `GachaMinLv`/`GachaMaxLv`, race and class columns; cost (Fortune Gate Pass, gold, charisma), the client's
  refusals 1 to 6, item stored with its options. **Loot**: `Rules.LootLevelGap` on by default (11) and in both gap
  directions. **Casting**: the caster is frozen when casting starts (stop sent to observers only). **Effects**: every
  skill with a duration is announced (AGPMSKILL op 4), even with no column ArchCore reads. Unit tests; `gacha`
  scenario (new); to be confirmed in game. **Guild** (same day, last messages surveyed in Archonia): disbanding is
  refused during the 24 hours after creation (code 46, "Guild_DestroyFailTooEarly", the client's original text),
  set by `Guild.DestroyDelayHours`; `guilddestroy` scenario (new).
- Missing protocol ops (Archonia survey of 2 November 2026): quests (op 14 on-screen tracking, 10 at most, stored in
  `character_quests.journal` and sent back as the 5th integer of op 4; ops 8 and 9 inventory full); party (op 2: effect
  area and party bonus, configurable `Party.*` rules; management ops 9, 12 and 13, refusal of an invitee at least 10
  levels below); PvP (ops 6, 8, 10, 12, 14, 16: enemy lists and enemy guilds, which hit each other without Ctrl outside
  safe areas; portal refused to a player with enemies, op 18 code 11); guild battles (op 14 fighters picked for types 3
  and 4, ops 26 and 27 fighters and lists, op 46 round 1; op 42 truce refused); skills (op 6 effect list, op 15 cooldowns
  reset when effects end, op 14 level bonus of the `skill_levelup` options, applied to casts); chat (op 4 recipient
  absent, ops 1 to 3 whisper blocking); items (op 15 gold dropped on the ground, without an owner; op 28 cooldown per
  template); AGPMCHARACTER op 8 before closing a session taken over by another connection; portals (ops 2, 3 and 6). Set
  aside: quests 11 to 13, items 8 and 29, skills 9 and 17, PvP 17. Unit tests; `quest`, `party`, `pk`, `pvp`,
  `guildbattle`, `guildrelation`, `chat` and `gold` (new) scenarios in the simulator on 2 November 2026, to be confirmed
  in game (party bonus rules in particular).
- ArchRust client ([own repository](https://github.com/BlaMacfly/ArchRust)): combat (attacking, hits, death, resurrection, loot), parties, other
  players, inventory and equipment, NPCs (dialog, shop, bank and buying pages, quests), skills (trainers,
  casting, shortcut bar), teleport portals (original window and loading screen, confirmation of player-versus-player zones, "Return",
  refusal in combat), automatic reconnection (`UI_ReconnectHold` window, 60 s), guilds (registrar, creation,
  disbanding and guild windows, members, guild name above heads) and guild and party chat, whispers (private bar, `/w`, `/r`, guild
  window button), graphics and sound resource packs (enabled,
  ordered and checked from the Options; see the [guide](https://github.com/BlaMacfly/ArchRust/blob/main/docs/RESOURCE-PACKS.en.md)), original Archonia windows instead of the temporary ones (confirmations, stack split,
  buying, bank, party, main menus, character select and creation, options), movement speed like Archonia (the
  GM's `/speed` visible), potions drunk automatically below the gauge sliders' threshold, GFX, Display and
  Function options wired, AddOns that can be toggled, client texts in the chosen language; verified against ArchCore by headless runs and screenshots of the graphical client, to confirm in game.
- Player visibility: movement relayed to nearby players, appearing and disappearing by distance.
- Chat within view range, and whispers.
- GM level sent to the "Admin Commands" panel.
- First GM commands: rates, moderation, items, restart announcement, teleport. See
  [[Administration|EN-Administration]].

## Next steps

00. Webzen source (8 November 2026): `archcore import … --webzen <ini folder>` imports from the official Webzen
   client the quests (3,082 of 3,270: 72 given or taken back by a world object, which the server does not handle, 3
   without a kept NPC, 113 following them; Archonia's 14 quests removed), their groups, NPCs and texts in four
   languages, the experience curve for levels 1 to 121 (64 bits beyond level 112), the 2,768 titles (Natural Enemy,
   Collector, Killer and Storyteller added, conditions in Webzen's sense) and the original high and heroic masteries
   (the Berserker's second tree, Archonia's berserk mode, removed); log of dropped rows (`data/import-webzen.log`),
   export for ArchRust (quests, groups, titles, masteries), `migrate-ep8` extended to missing quests and titles;
   `quest`, `title` and `exp` scenarios adapted or added. Remaining: quests given by world objects (quest event on
   an object), in-game tests.

0. Episode 8 values (2 November 2026): import (original server table values, client names, per-table
   summary), table export for ArchRust (`archcore export-client-tables`, fingerprint announced through the
   `client-tables/1` capability), character migration (`archcore migrate-ep8`, reversible quarantine) done;
   full simulator series run again. Remaining: in-game tests (tooltips, prices, cooldowns, refining,
   teleports, remaining quests), then the main server switch (reimport, `migrate-ep8 --apply`, export) with
   the user's approval, and episode 8 style balancing.

1. Full character creation and visibility between two players: checked on 25 October 2026 with two ArchRust
   clients on the test server (creation, arrival in Golundo, refusal of a taken name, players visible to each
   other). Still to confirm with the Archonia client.
3. NPC functions: quests with interaction objects. Server-built menus (AgpmEventNPCDialog ops 2 and 3, message
   boxes and buttons) of the "<Town> Event", "Portal to Chaotic Frontier", "Portal to Town" NPCs and of Jaig: the client
   can show them, but their content is neither in the client nor in its data; waiting for a source. Portals: refusal
   while transformed done (morph.rs), to be confirmed in game; the Battle Ground fort entrances (`TeleportPoint.ini`) have
   no object in Archonia's `obj*.ini` files.
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
