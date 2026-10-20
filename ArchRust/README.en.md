<p align="right">
  <a href="README.md"><img src="https://img.shields.io/badge/🇫🇷_Lire_en_français-005BBB?style=for-the-badge" alt="Lire en français"></a>
</p>

<p align="center">
  <img src="docs/assets/archrust-logo.png" alt="Official ArchRust logo" width="320">
</p>

<h1 align="center">ArchRust</h1>

<p align="center">
  <b>ArchLord client rewritten in Rust</b><br>
  x86-64 · wgpu · Lua AddOns · HD patches
</p>

**A Rust (x86-64) rewrite of the 64-bit ArchLord client**, with a World of Warcraft-style Lua
AddOn system and an HD graphics patch format for developers.

File formats and the network protocol were rebuilt by reverse engineering the client executable
(Ghidra driven by [REA](https://github.com/morluto/rea)) and verified against the real game files
and the [ArchCore](https://github.com/BlaMacfly/ArchCore) server emulator. This repository contains
**no game data**: you need your own client. License: [GPL-3.0-or-later](LICENSE.md).

## Status

| Area | Status |
|---|---|
| Cryptography (CryptoAPI RC4, little-endian Blowfish, AES-128-GCM pack indexes) | ✅ verified |
| AuPacket protocol (19-byte header, field schemas, frames, handshake) | ✅ verified against ArchCore |
| Login → world list → account → characters → world entry → NPCs in view | ✅ `archrust login` |
| `Reference.Dat`/`Data.Dat` packs (character, object, textures, octree…) | ✅ every index read |
| RenderWare models (`.ecl`/`.dff`: frames, geometries, skins, materials) | ✅ 200/200 tested |
| `.tx1` textures (RenderWare TXD, DXT1/3/5, BGRA, palette) | ✅ 66/66 tested |
| World tiles (`MagPack` + aPLib): sectors, textures, attributes | ✅ 1280/1280 sectors read |
| wgpu rendering (terrain around the player, models, orbit camera), egui interface | ✅ terrain, player and NPCs drawn ([screenshot](docs/captures/anchorville.png)) |
| Ground textures (base + mask-blended layers, `0xfffff001` extension) | ✅ correct scale, soft transitions, verified in game |
| World objects (`ini/obj%05d.ini` + `objecttemplate.ini`, `object` pack) | ✅ Anchorville buildings, trees, fences, props; LOD and animations remain |
| Character models (`charactertemplateclient.ini` rule → hexadecimal `a/b/c` files) | ✅ player and the 18 Anchorville NPCs |
| Lua AddOns (`.toc`, frames, events, slash commands, timers, SavedVariables, LibStub + Ace3, extended API) | ✅ see [docs/ADDONS.en.md](docs/ADDONS.en.md) |
| HD patches (`patch.toml`, folder or `.ahp`, priorities, patterns) | ✅ textures/models/files |
| Movement (click to walk, `/goto`), terrain height, packet 0x40 and stop on arrival | ✅ verified with two clients on ArchCore ([screenshot](docs/captures/deplacement.png)) |
| Optimized action 0x41 | 🟡 factors/HP decoded; known HP changes appear in combat log; bars/animations remain |
| Inventory and equipment | 🟡 0x04 and equipped views decoded, names resolved, `d…` models loaded; rig attachment remains unverified |
| `.ean` animations (idle, run) | ✅ verified in game on ArchCore (TID 96); combat animations remain |
| Character selection and creation (original `LoginSettings.txt` scene: throne room, race temples, cameras, zoom) | ✅ list, race/class/face/hair/name creation, deletion, entering the world, verified on ArchCore |
| Facing and default heads | ✅ characters face their walking direction, skinned face and hair (`character\defaulthead`), verified in game (TID 96) |
| Sound (region music `RegionTemplate.ini` → `skyset.ini`, ambience, effects from `.pk` packs and `sound/ui`, volumes, AddOn `PlaySound`) | ✅ rodio; login and Anchorville music, effects verified on ArchCore; ambience table still to identify |
| Combat, skills | ⏳ to do |

## Quick start

Requirements: stable Rust (MSVC), a 64-bit ArchLord client in `../client/ArchLord/Game`, and, to
play, a running ArchCore (MariaDB + `npm start`).

```bash
cargo build --release
cargo run --release -- inspect pack character              # list a pack
cargo run --release -- inspect model character a0000005.ecl
cargo run --release -- inspect animation a0060000.ean --time 0.5 --model b0000060.ecl
cargo run --release -- extract texture a0057aab a0057aab.png   # decoded texture
cargo run --release -- inspect sound index                 # soundpak.info: sound packs
cargo run --release -- inspect sound sd.pk --limit 5       # one .pk pack (or effect/mono/maa.pk)
cargo run --release -- extract sound sd0000.wav sd0000.wav # a sound from a pack (or sound/ui/block.wav)
cargo run --release -- login test test Testeur             # full flow, headless
cargo run --release -- play --connect test:test@127.0.0.1:11002/Testeur
cargo run --release -- addons                              # check the AddOns
cargo run --release -- patch build patches/my-patch my-patch.ahp
```

### Installing beside the client and `realmlist.wtf`

`ArchRust.exe` (icon: the official ArchRust logo, `apps/archrust/res/archrust.ico`) can live in the client's `Game` folder, in place of `Archonia.exe`, together with
`addons/`, `free-assets/` and `patches/`: it detects the client there without options, and a
double-click starts the game (`play` is the default command).

The login server is set as in World of Warcraft, with a `realmlist.wtf` file:

```text
# comment
set realmlist 127.0.0.1:11002
```

Form `set realmlist <host>[:<port>]` (port 11002 by default, optional quotes, the last line wins).
The file is looked up next to the executable, then in the current folder, then in the `Game`
folder; it pre-fills the login screen. `--connect user:pass@/character` (no host) connects to that
server; a host given to `--connect` still takes precedence.

The login screen shows `crates/archrust-engine/assets/login-screen.png` ("ArchRust — Awakening of the
Eternal Storm"), embedded in the executable; as in WoW, AddOn UI only appears in game.

After login, the selection screen uses Archonia's scene (account characters in the throne room,
click to pick); "Créer un personnage" opens the chosen race's creation scene. For tests:
`--connect test:test@` (no `/character`) goes through these screens and
`--login-exec "créer; race Orc; classe Berserker; visage 3; nom Grokk; valider; choisir 1; entrer"`
plays commands there.

In game: left-click the ground to walk, right-drag to orbit, mouse wheel to zoom, WASD to move the
camera, `` ` `` opens the Lua console, and the chat window accepts `/` commands (`/goto x y z`, `/cam distance [yaw°] [pitch°]`,
`/reload`, `/quit`, plus AddOn commands). For testing: `--exec "/goto …"` runs a command on world
entry and `--screenshot file.png` saves a capture then exits (under Git Bash, prefix with
`MSYS_NO_PATHCONV=1`).

## Architecture

```text
crates/
  archrust-crypto     RC4 (CryptoAPI MD5), little-endian Blowfish, AES-128-GCM
  archrust-protocol   AuPacket, field schemas, A1/B1 frames, handshake
  archrust-data       ini tables, packs, MagPack + aPLib, RenderWare (DFF/TXD/sectors), .pk sound packs
  archrust-netclient  tokio client: login, world, events, commands
  archrust-addons     Lua 5.4 (mlua), .toc, frames, events, slash commands, SavedVariables, LibStub/Ace3
  archrust-patch      TOML manifest, patch stack, .ahp archives
  archrust-engine     winit + wgpu + egui: rendering, terrain, entities, UI, network bridge, audio (rodio)
apps/archrust         executable: play / login / inspect / extract / addons / patch
addons/HelloArch      sample AddOn (core API)
addons/ArchAce        LibStub/Ace3 sample AddOn (unit frames, nameplates, theme, options)
docs/FORMATS.md       reconstructed file formats (in French)
```

## AddOns

An AddOn lives in `addons/<Name>/` with a `<Name>.toc` file:

```text
## Title: Hello ArchLord
## Interface: 10000
## SavedVariables: HelloArchDB
core.lua
```

The API follows WoW so that code and habits carry over: frames as Lua tables (`CreateFrame`,
anchors, strata, textures, `.ttf` fonts, `GameTooltip`, `StaticPopup`, options panel), events
(`PLAYER_LOGIN`, `UNIT_HEALTH`, `PLAYER_TARGET_CHANGED`, `CHAT_MSG_*`, `COMBAT_LOG_EVENT_UNFILTERED`…),
`Unit*`, `C_Timer`, `SlashCmdList`, `hooksecurefunc`, CVars. The safe standard Lua libraries are open
(`string`, `table`, `math`, `utf8`, `coroutine`, a reduced `os`, `bit`/`bit32`, `require` limited to
AddOn folders) with WoW aliases (`strsplit`, `tinsert`, `wipe`, `format`…), and LibStub,
CallbackHandler, Ace3 (AceAddon, AceEvent, AceTimer, AceConsole, AceDB, AceHook, AceLocale,
AceSerializer), LibDataBroker and LibSharedMedia are built in. The `ArchRust` namespace changes the
client: camera, sky colour and lighting, model tints, native window colours, key bindings,
nameplates (`ArchRust.WorldToScreen`). AddOns load when entering the world, as in WoW. No disk access
beyond the AddOns' own files and SavedVariables.

Full reference: **[docs/ADDONS.en.md](docs/ADDONS.en.md)**. Sample: `addons/ArchAce` (`/archace`).

## HD patches

A patch is a folder (or an `.ahp` archive) with a `patch.toml`:

```toml
[patch]
id = "anchorville-hd"
name = "Anchorville HD"
version = "1.0.0"
priority = 100

[[textures]]
match = "a0057aab"            # RenderWare name, * wildcard allowed
file = "textures/a0057aab.png" # PNG/DDS of any size

[[models]]
match = "character/a0000001.ecl"
file = "models/a0000001.glb"
scale = 100.0                 # glTF in metres → ArchLord units (≈ cm)

[[files]]
path = "ini/npc.ini"
file = "npc.ini"
```

Patches in the `patches/` folder are stacked by decreasing priority; the first one providing a
resource wins, otherwise the original game data is used. glTF/GLB `[[models]]` are drawn in place of
the client's model (characters, NPCs, `world/D272,272.dff` sectors) with their own textures;
`archrust inspect gltf <file>` describes a glTF file.

## Sound

The audio engine (`rodio`, MP3/WAV decoding by Symphonia) plays three tracks:

* **music**: looped MP3 with crossfade. On the login screens, `sound/bgm/htt_001.mp3` (CVar
  `loginMusic`); in game, the music of the player's region from the client tables (sector `.amf`
  grid → `RegionTemplate.ini` → `skyset.ini`, per time slot, see [docs/FORMATS.md](docs/FORMATS.md),
  in French), else `worldMusic`;
* **ambience**: looped `sound/stream/*.mp3` (CVar `worldAmbience`, city by day as default: the
  ambience table has not been identified yet);
* **effects**: one-shot WAVs, `sound/ui/<name>.wav` (pack), `sound/effect/sd0000.wav` (entry of a
  `.pk` through `soundpak.info`, or RIFF scan for `effect/mono/`), or an AddOn file.

Every sound goes through the patches: a `[[files]]` rule on `sound/bgm/htt_001.mp3` or
`sound/effect/sd.pk` replaces the music or a whole pack. Without a client (`--no-client`), only
sounds provided by patches play; everything else is silent.

Volumes (music, ambience, effects, mute): **Options** window (button or Escape key), saved in
`saved-variables/options.toml`. Commands: `/music <path>|off|auto`, `/sound <path>`; AddOns:
`PlaySound`/`PlaySoundFile(path)`, `PlayMusic(path)`, `StopMusic()`.

## Client data and free assets

The repository keeps the publisher's files strictly apart from what can be published:

| Folder | Content | Published? |
|---|---|---|
| `assets-archonia/` | the client's `Game` folder (or a link to it); searched first by ArchRust | **No**: git-ignored except its README |
| `free-assets/` | your original creations (textures, glTF models, files) declared in `patch.toml`; top-priority patch | Yes |
| `patches/` | third-party HD patches (`.ahp` or folders) | your call |

A retouched game texture or model is still a derivative work: it does not belong in `free-assets/`.
As free assets replace the client's, the game becomes playable without any original file:

```bash
cargo run --release -- play --no-client            # patches and placeholders only
```

Without a client, anything missing is replaced by a generated placeholder (colored checkerboard,
box, empty table) and the login window works as usual. Free (CC0) asset sources and the mapping
from client resource names to free files are listed in [docs/ASSETS-LIBRES.md](docs/ASSETS-LIBRES.md)
(in French).
Everything the repository does not ship (client, reference exports, HD interface, sounds, `.ahp` patches):
where to get it, where to put it and how to check it, in [docs/ASSETS-NON-FOURNIS.en.md](docs/ASSETS-NON-FOURNIS.en.md).

## Disclaimer

ArchLord is a trademark of its publisher. ArchRust is an independent, unaffiliated project for
study, interoperability and preservation. See [LICENSE.md](LICENSE.md).

<p align="right">
  <a href="README.md"><img src="https://img.shields.io/badge/🇫🇷_Lire_en_français-005BBB?style=for-the-badge" alt="Lire en français"></a>
</p>
