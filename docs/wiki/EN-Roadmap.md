# Status and roadmap

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
  level up, corpse then respawn. Monsters that are hit fight back and chase their attacker; player death
  and resurrection in town. Drops: gold and items on the ground, pick-up.
  Equipment: weapons and armour worn (restrictions, two hands), counted in damage and defense, visible
  to other players; moving items in the inventory. Life and mana potions, out-of-combat HP and MP
  regeneration. Teleport scrolls (town, round trip, destination); monster patrols. See
  [[Network protocol|EN-Network-protocol]].
- ArchRust extensions, server side: capability negotiation and AddOn messages (whisper, `SAY`). See
  [[ArchRust extensions|EN-ArchRust-extensions]]; the client side remains to be written in ArchRust.
- Selling an item to an NPC.
- Player visibility: movement relayed to nearby players, appearing and disappearing by distance.
- Chat within view range, and whispers.
- GM level sent to the "Admin Commands" panel.
- First GM commands: rates, moderation, items, restart announcement, teleport. See
  [[Administration|EN-Administration]].

## Next steps

1. Verify full character creation in game.
2. Verify player visibility with two real clients.
3. NPC functions: quests, teleport, bank.
4. Combat: per-region resurrection points, skills, elemental item damage.
5. Items: merging stacks, destroying; skills.
6. Chat, parties, guilds.
7. Automatic reconnection (type 0x63).
8. Remaining GM commands on the [[Administration|EN-Administration]] page (arena, events, rifts…).
9. Finish the translation: about 54,000 texts left.

## Later

- **PlayerBots**: a server module for player characters driven by the server, modelled on the
  PlayerBots of World of Warcraft servers and adapted to ArchLord (movement, combat, parties, trade).
  To start once monsters, combat and parties work.

Contributions are welcome: open an issue to suggest or report something.
