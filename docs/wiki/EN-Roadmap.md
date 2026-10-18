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
4. Monsters, combat, experience.
5. Items: moving them in the inventory, equipment, use; skills.
6. Chat, parties, guilds.
7. Automatic reconnection (type 0x63).
8. Remaining GM commands on the [[Administration|EN-Administration]] page (arena, events, rifts…).
9. Finish the translation: about 54,000 texts left.

## Later

- **PlayerBots**: a server module for player characters driven by the server, modelled on the
  PlayerBots of World of Warcraft servers and adapted to ArchLord (movement, combat, parties, trade).
  To start once monsters, combat and parties work.

Contributions are welcome: open an issue to suggest or report something.
