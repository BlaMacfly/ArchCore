# Administration (GM / Admin)

This page describes the game master (GM) commands the client can send, and how to use them.

> **ArchCore status**: the server handles chat, sends the GM level to the client and runs the
> commands marked ✅ in the list (see section 3). The others answer "Unknown command": they wait for
> the matching game systems, missing from ArchCore (see the end of section 4). Progress is tracked on the
> [[Roadmap|EN-Roadmap]] page.

> **GM panel.** With the ArchRust client, the [ArchCoreAdmin](https://github.com/BlaMacfly/ArchCoreAdmin) add-on
> offers a button for each of these commands and reads the server replies back.

## 1. Give GM rights to an account

The GM level is stored in the `gm_level` column of the `archcore_auth.account` table. A value of 0
is a regular player. The higher the number, the more rights the account has.

Create a GM account, or change the level of an existing one:

```bash
archcore db-init --account <username> <password> <level>
# example: archcore db-init --account admin StrongPassword 3
```

Or directly in SQL:

```sql
UPDATE archcore_auth.account SET gm_level = 3 WHERE username = 'admin';
```

The account must log in again for the change to apply. The server decides whether a command is
allowed for that level: the client only sends it. A level 0 player who types a GM command gets no
answer (the chat prefixes "/p", "/g", "/s", "/race", "/scream" and "/warn" are read before commands).

## 2. Two ways to send a command

### Through the chat window

Every command is typed in the chat, starting with a slash `/`, then confirmed with Enter:

```text
/kill "PlayerName"
/chatban "PlayerName" 30
/SetExp 2.0
```

- **Names** go between double quotes, which allows spaces.
- **Numbers** are written without quotes; decimals use a dot (`1.5`).
- Command names are **case-sensitive**: `SetExp`, but `chatban`.

### Through the "Admin Commands" panel

The client contains an "Admin Commands" graphical panel. It shows the character's GM level
("Your admin level") and groups the commands in tabs: rates, moderation, events, arena, items and
characters, world, region, system. Each button types the matching command into the chat for you,
so the result is the same as typing it by hand.

The panel reads the GM level sent by the server. ArchCore sends it on world entry (AgpmAdmin
packet, type 0x12) for accounts whose `gm_level` is above 0.
The client also reads an `admin` launch argument; its exact effect is not confirmed yet.

Server replies (a rate's value, a syntax error, missing rights…) show up in the chat as system
messages. ArchCore's replies are in French.

## 3. Commands handled by ArchCore

| Command | GM level | What ArchCore does |
|---|---|---|
| `/GetExp`, `/GetGold`, `/GetDrop`, `/GetUniqueDrop`, `/GetCharisma`, `/GetPrestigeRate`, `/GetChatBuff` | 1 | Shows the rate's value |
| `/SetExp`, `/SetGold`, `/SetDrop`, `/SetUniqueDrop`, `/SetCharisma`, `/SetPrestigeRate` `<x>` | 3 | Changes the rate (0 to 100) |
| `/SetChatBuff <0\|1>` | 3 | Turns the chat bonus on or off |
| `/chatban "name" <minutes>` | 1 | The player can no longer talk; `0` lifts the mute. The player is told |
| `/kill ["name"]` | 2 | Sets the character's HP to 0; without a name, on yourself. Online characters only |
| `/invincible [0\|1]` | 2 | Monsters no longer wound you (1, the default) or do again (0) |
| `/speed <1-10>` | 2 | Multiplies the GM's movement speed (walk and run); `1` restores it. Kept when equipment or effects change, forgotten on logout. ArchCore-specific command |
| `/rename_char "old" "new"` | 2 | Renames in the database (2 to 24 characters, no space). An online player sees the new name on next login |
| `/additem <TID> [count]` | 3 | Shortcut for `/create` on yourself: adds the item (1 by default, up to 9999) to the GM's inventory, tradable. ArchCore-specific command |
| `/weather [clear\|rain\|snow]` | 2 | Weather for every player in game (rain, snow, clear); without an argument, the weather and the game hour. ArchCore-specific command |
| `/shrine [open\| `/siege [start|end <castle> | owner <castle> <guild|-> | hp <castle> <HP>]` | 3 | Without an argument, the castles' state; `start` closes the applications and opens the battle, `end` ends it (the guild of the last carving takes the castle), `owner` gives the castle to a guild (`-`: none) and resets its calendar to status 0, `hp` lowers the HP of a warring castle's intact objects (tests). ArchCore-specific command |
|close <n>]` | 3 | Without an argument, the Shrines' state; `open` opens Shrine `n`'s battle without waiting for its schedule, `close` ends it with no winner. ArchCore-specific command |
| ✅ `/siege [start|end|owner|hp …]` | Castles' state, forced start and end of a siege, owner, objects' HP (ArchCore addition, not in the panel) |
| `/tax [pay [castle]]` | 3 | Without an argument, rates and income of each castle's regions; `pay` pays a castle's (or every castle's) income at once to the owning guild's master, minus the ArchLord's tribute. ArchCore-specific command |
| `/archlord [set <name> \| clear]` | 3 | Sets the ArchLord (ops 0 and 1 to everyone) or removes him; without an argument, the ArchLord's name. The ArchLord's `/archlord <message>`: message to the whole server. `/archlordbegin`, `/archlordend` (Rensphere war), `/nextstep`, `/archlordactivate <name>`, `/archlorddeactivate`, `/castleowner <castle> <guild>`, `/deletecastleowner <castle>`: Archonia commands (sysstr 622 to 629, 636, 657, 658) |
| `/durability <value\|-1>` | 3 | Sets the durability of the worn equipment that wears out (`-1`: full, 0: broken); to try wear and repair. ArchCore-specific command |
| `/create "name" <TID> <count> <0\|1>` | 3 | Adds the item to `character_items`, in the first free inventory slot. An online player gets it at once; the character can also be offline |
| `/questreset <TID>` | 3 | Clears quest TID (in progress or completed) from the GM's journal; visible on next login. ArchCore-specific command |
| `/recast` | 3 | Resets the GM's skill recast delays on the server side (the client may still show its own). ArchCore-specific command |
| `/last_refine` | 3 | Runs again the last recipe refined successfully since login (the panel's "Use last refine recipe"), like the client's op 6: ingredients taken from the inventory, gold paid, result in the refining window. Refused without a successful refine |
| `/level <level> ["name"]` | 3 | Sets the online character's level (1 to 120): experience reset, HP and MP restored, skill points recomputed; without a name, on yourself. ArchCore-specific command |
| `/money <amount> ["name"]` | 3 | Sets the online character's gold (0 to 1,000,000,000); without a name, on yourself. ArchCore-specific command |
| `/charisma <points> ["name"]` | 3 | Gives charisma points to the online character (1 to 1,000,000, multiplied by the `Rate.Charisma` rate and the charisma bonuses of its effects, except the boss one); without a name, to yourself. ArchCore-specific command (Archonia has none), until player-versus-player fights exist |
| `/announce_restart <minutes> <seconds>` | 3 | Announces to every player, reminds every minute, counts down the last seconds. `0 0` cancels |
| `/move x,z` | 1 | Teleports the GM to (x, z) and sends the NPCs of the new spot |

The required level can be changed per command in `config/world.conf` (see
[[Configuration|EN-Configuration]]):

```ini
Rate.Exp = 2
Rate.Drop = 1.5
GM.CommandLevel.kill = 3
GM.CommandLevel.move = 2
World.ExitOnRestart = 0
```

- `Rate.*`: rates at startup (`Exp`, `Gold`, `Drop`, `UniqueDrop`, `Charisma`, `Prestige`,
  `ChatBuff`). The `Set…` commands change them **in memory**: on restart they go back to these
  values.
- `World.ExitOnRestart`: with `1`, the server stops at the end of the `/announce_restart` countdown so
  that a supervisor can restart it. By default it only announces.

Current limits:

- Experience, gold and loot rates apply; unique loot, charisma, prestige and the chat bonus are kept
  but have no effect, the matching systems being absent.
- Chat mutes are kept in memory and are lost on restart.
- With `/create`, the last number ("untradable") is not stored yet.
- To check with the real client: the `/kill` death animation. After `/move`, the GM is put at ground
  height (`map_height`); outside the height map, they keep their altitude.
- The client itself sends `/move` when a waypoint number is typed in the chat.

## 4. Command list

Notation: `"name"` = character name in quotes; `<n>` = whole number; `<x>` = decimal number.

✅ = handled by ArchCore (details in section 3).

### Server rates

A rate is a multiplier: `1.0` = normal, `2.0` = double.

| Command | Effect |
|---|---|
| ✅ `/SetExp <x>` | Experience multiplier |
| ✅ `/SetGold <x>` | Gold gain multiplier |
| ✅ `/SetDrop <x>` | Loot multiplier |
| ✅ `/SetUniqueDrop <x>` | Unique loot multiplier |
| ✅ `/SetCharisma <x>` | Charisma point multiplier |
| ✅ `/SetPrestigeRate <x>` | Prestige multiplier |
| ✅ `/SetChatBuff <0\|1>` | Turns the chat bonus off (0) or on (1) |
| ✅ `/GetExp`, `/GetGold`, `/GetDrop`, `/GetUniqueDrop`, `/GetCharisma` | Shows the current rate |

Example: `/SetExp 2.0`, then `/GetExp` to check.

### Moderation

| Command | Effect |
|---|---|
| ✅ `/chatban "name" <minutes>` | Mutes the player for N minutes; `0` lifts the mute |
| ✅ `/kill "name"` | Kills the character |
| `/kill_explosion <effect> "name"` | Sets an explosion effect on a character's death; without a name, on yourself |
| `/kill_explosion -1 "name"` | Removes that effect from a character |
| `/kill_explosion` | Removes the effect from yourself |
| ✅ `/rename_char "old" "new"` | Renames a character |

### Arena

| Command | Effect |
|---|---|
| `/arena_ban_day "name" <days>` | Bans from the arena for N days; `0` lifts the ban |
| `/arena_ban_hour "name" <hours>` | Bans from the arena for N hours; `0` lifts the ban |
| `/arena_ranked_ban_day "name" <days>` | Bans from the ranked arena for N days |
| `/spectate_arena ["name"]` | Watches an arena; without a name, picks one |
| `/spectate_ranked_arena ["name"]` | Watches a ranked arena |
| `/spectate_normal_arena ["name"]` | Watches a normal arena |
| `/spectate_shuffle_arena ["name"]` | Watches a shuffle arena |
| `/leave_spectate` | Leaves spectator mode |
| `/arena_win_reward <type> <coins>` | Coins earned per win for a game type |
| `/arena_lose_reward <type> <coins>` | Coins earned per loss for a game type |
| `/set_chat_season_winners` | Imports the season winners |

### Events

| Command | Effect |
|---|---|
| `/tournament round` | Moves the tournament to the next round |
| `/tournament disable` | Disables the tournament |
| `/tournament recovery <percent> <0\|1>` | Healing between rounds; the second number enables base summoning |
| `/tournament ban "name" <days>` / `/tournament unban "name"` | Bans or reinstates a player |
| `/death_match freeze` / `break` / `reload` / `disable` | Freezes, interrupts, reloads or disables the death match |
| `/death_match ban "name" <days>` / `unban "name"` | Bans or reinstates a player |
| `/racewar reload` / `disable` | Reloads or disables the race war |
| `/racewar ban "name"` / `unban "name"` | Bans or reinstates a player |
| `/game_event set_only_solo` | Tournament and death match are solo only |
| `/game_event !set_only_solo` | Removes that restriction |
| `/game_event ban "name" <days>` / `unban "name"` | Bans or reinstates a player |
| `/counter open "name" <seconds>` | Opens an event counter |
| `/counter close` / `freeze` / `break` | Closes, freezes or interrupts the counter |
| `/battlepass reload` | Reloads the battle pass seasons |
| `/battlepass_premium "name"` | Unlocks a character's premium battle pass |
| `/reload_events`, `/reload_class_events`, `/reload_event_effect` | Reloads the bonus events |
| `/enable_events` / `/disable_events` | Turns bonus events on or off |

### Items and characters

| Command | Effect |
|---|---|
| ✅ `/create "name" <TID> <count> <0\|1>` | Gives an item to the character. `TID` = item ID; the last number makes it untradable (1) |
| ✅ `/additem <TID> [count]` | Adds an item to your own inventory (ArchCore addition, not in the panel) |
| ✅ `/speed <1-10>` | Multiplies your movement speed (ArchCore addition, not in the panel) |
| ✅ `/weather [clear\|rain\|snow]` | Everyone's weather, or weather and game hour (ArchCore addition, not in the panel) |
| ✅ `/shrine [open\|close <n>]` | Shrines' state, forced opening or closing of a battle (ArchCore addition, not in the panel) |
| ✅ `/durability <value\|-1>` | Sets the durability of your worn equipment (ArchCore addition, not in the panel) |
| ✅ `/money <amount> ["name"]` | Sets an online character's gold (ArchCore addition, not in the panel) |
| ✅ `/charisma <points> ["name"]` | Gives charisma points to an online character (ArchCore addition, not in the panel) |
| ✅ `/level <level> ["name"]` | Sets an online character's level (ArchCore addition, not in the panel) |
| ✅ `/questreset <TID>` | Clears a quest from the GM's journal (ArchCore addition, not in the panel) |
| ✅ `/recast` | Resets the GM's skill recast delays (ArchCore addition, not in the panel) |
| ✅ `/last_refine` | Runs the last successful refine recipe again |
| `/refresh_prestige_db` | Reloads the prestige data |
| `/refinetest` / `/refinetest stop` | Starts or stops the refine stress test |

Example: `/create "Testeur" 1234 5 0` gives 5 tradable copies of item 1234.
The panel offers an item search by name or TID.

### World and rifts

| Command | Effect |
|---|---|
| `/go_rift ["name"]` | Goes to a rift; the name is optional |
| `/close rift` or `/close_rift` | Closes the rift |
| `/next_rift_objectives` | Moves to the rift's next objectives |
| `/bg_off` | Removes battleground buffs |
| `/spawn_instance "name" <count> <radius>` | Spawns an event group around you |
| ✅ `/move x,z` | Teleports to (x, z) |
| `/region …` | Edits a region's settings (damage reduction, potion reduction, cast cap). Use the panel, which builds the command |

### System

| Command | Effect |
|---|---|
| ✅ `/announce_restart <minutes> <seconds>` | Announces a restart, with a final countdown |
| `/refresh_lua_scripts` | Reloads the server's LUA scripts |
| `/reload_ip_players` | Reloads the IP address list |
| `/log_activity` | Writes the activity log |
| `/cast_cap <0-100>` | Cast speed cap |
| `/cooldown_cap <0-100>` | Cooldown reduction cap |
| `/test <minutes>` | Test command: grants points and sets the boss spawn delay |

### Commands without a system in ArchCore

Archonia's "Admin Commands" panel only builds these commands; the server decides what they do. They stay
without effect ("Unknown command") as long as the system they drive does not exist:

| Commands | Missing system |
|---|---|
| `/arena_*`, `/spectate_*`, `/leave_spectate`, `/set_chat_season_winners` | Arena, seasons and shrine rewards |
| `/tournament …`, `/death_match …`, `/racewar …`, `/game_event …`, `/counter …` | Game events (tournament, death match, race war, counter) |
| `/battlepass …`, `/battlepass_premium` | Battle pass |
| `/reload_events`, `/reload_class_events`, `/reload_event_effect`, `/enable_events`, `/disable_events` | Bonus events |
| `/kill_explosion …` | Explosion effect on death |
| `/refresh_prestige_db` | Prestige |
| `/refinetest` | Refine stress test (original server tool) |
| `/go_rift`, `/close rift`, `/close_rift`, `/next_rift_objectives` | Rifts |
| `/bg_off` | Battlegrounds |
| `/spawn_instance` | Event spawn groups ("event spawn name"); ArchCore only has the original tables' spawn points |
| `/region …` | Region settings (damage reduction, potion reduction, cast cap, per class) |
| `/cast_cap`, `/cooldown_cap` | Cast speed and cooldown reduction bonuses, which these caps limit |
| `/refresh_lua_scripts`, `/reload_ip_players`, `/log_activity`, `/test` | LUA scripts, IP address list, activity log and test command of the original server |

## 5. Good practice

- Keep GM accounts separate from play accounts, with a strong password.
- Try a command on a test character before using it on a player.
- Character names must be exact, accents included.
