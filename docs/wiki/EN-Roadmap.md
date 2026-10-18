# Status and roadmap

## Working with the real client

- Encrypted handshake, world list, account login.
- Character list and creation screen.
- World entry, with a starting village per race.
- Spawning of the 567 NPCs around the player, with translated titles.
- French client interface, accents included (translation in progress).

## Verified with the simulator, to confirm in game

- Chat within view range, and whispers.
- GM level sent to the "Admin Commands" panel.
- First GM commands: rates, moderation, items, restart announcement, teleport. See
  [[Administration|EN-Administration]].

## Next steps

1. Verify full character creation in game.
2. Players and NPCs disappearing when they leave view range (movement itself, type 0x40, is done:
   position saved, players revealed to each other, moves relayed).
3. NPC functions: dialogues, shops, quests (type 0x3C).
4. Monsters, combat, experience.
5. Items, inventory, skills.
6. Chat, parties, guilds.
7. Automatic reconnection (type 0x63).
8. Remaining GM commands on the [[Administration|EN-Administration]] page (arena, events, rifts…).
9. Finish the translation: about 54,000 texts left.

Contributions are welcome: open an issue to suggest or report something.
