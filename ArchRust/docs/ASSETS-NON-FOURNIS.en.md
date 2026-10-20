# Integrating the assets that are not shipped

The repository publishes ArchRust's code, its AddOns, its free assets (`free-assets/`) and its
documentation. Everything that belongs to the game's publisher is absent: the 64-bit ArchLord
client itself, its data packs, and anything derived from them (reference exports, reworked images,
split sounds). This guide explains, for each missing family, where to get it, where to put it and
how to check that ArchRust sees it.

Golden rule: **nothing described here may be pushed to GitHub**. The root `.gitignore` protects the
expected locations; if you store these files elsewhere, add the exclusion yourself before any commit.

## 1. The 64-bit ArchLord client (required)

ArchRust reads the data of the **Archonia** client (64-bit ArchLord), the project's reference. You
need an installation of that client; its `Game` folder contains, among others:

| Folder or file | Content | Used by ArchRust for |
|---|---|---|
| `character/`, `object/`, `effect/` | `Reference.Dat` + `Data.Dat` packs: RenderWare models (`.ecl`, `.dff`), `.ean` animations | characters, NPCs, scenery, animations |
| `texture/*/` | `.tx1` texture packs (RC4), interface PNG/TIF in `texture/ui` | all rendering |
| `world/` | `a%06dx.ma1/.ma2`, `b%06dx.ma2` tiles (MagPack): terrain, sector textures, attributes | terrain, ground height, regions |
| `ini/`, `ini_fr/`… | encrypted tables (`charactertemplateclient.ini`, `RegionTemplate.ini`, `skyset.ini`…) | TID → models, regions, music, texts |
| `sound/` | `bgm/*.mp3`, `stream/*.mp3`, `ui/` pack, `effect/*.pk` + `soundpak.info` | music, ambiences, effects |
| `etc/`, `images/` | camera splines, launcher | login camera; the launcher is not used |
| `Archonia.exe`, `Archonia.ini` | original executable and configuration | reverse-engineering reference (REA), never run by ArchRust |

Three ways to make it found, in the order ArchRust searches:

1. **`assets-archonia/Game`** (or `assets-archonia/` itself): a copy of, or a symbolic link to, the
   `Game` folder. The whole folder is git-ignored except its README.

   ```bash
   # Windows (command prompt, administrator rights or developer mode)
   mklink /J assets-archonia\Game "D:\Games\Archonia\Game"
   # Linux / macOS
   ln -s /path/to/Archonia/Game assets-archonia/Game
   ```

2. **`../client/ArchLord/Game`**: the historical location shared with the ArchCore server.
3. **`--game <folder>`** on any command, or drop `ArchRust.exe` straight into the client's `Game`
   folder with `addons/`, `free-assets/` and `patches/` next to it: it recognises the client without
   any option (see "Installing next to the client" in the [README](../README.en.md)).

Check:

```bash
cargo run --release -- inspect pack character --limit 5      # lists 5 models of the pack
cargo run --release -- inspect table charactertemplateclient.ini --limit 3
cargo run --release -- inspect sound index                    # soundpak.info is read
cargo run --release -- play --screenshot check.png --screenshot-after 4
```

Without the client every command says so ("requires an ArchLord client") and `play --no-client`
still works: everything missing is replaced by a generated placeholder (checkerboard, box, empty
table) and only patches are used.

## 2. Reference exports (the `free-assets/NN-*/` workshop)

The numbered sub-folders of `free-assets/` (`01-humain/`, `02-orc/`, `05-pnj-anchorville/`…) are
the free assets **workshop**: they hold client models exported as OBJ + MTL + PNG, used as UV
templates to draw the free versions. They are publisher data: `.gitignore` excludes them
(`/free-assets/[0-9][0-9]-*/`) and they are not declared in `free-assets/patch.toml`.

To regenerate them, one model at a time (the TID → files mapping is in
[ASSETS-LIBRES.md](ASSETS-LIBRES.md); `a%07x` = body, `b%07x` = default armour):

```bash
cargo run --release -- export character a0000060.ecl free-assets/01-humain/a0000060.ecl.obj
cargo run --release -- export character b0000060.ecl free-assets/01-humain/b0000060.ecl.obj
cargo run --release -- export character a0000248.ecl free-assets/05-pnj-anchorville/a0000248.ecl.obj
cargo run --release -- extract texture a0057aab free-assets/13-sol-et-terrain/a0057aab.png
```

A retouched client texture or model is still a derivative work: it does not belong in the published
`free-assets/`. Original creations are declared in `patch.toml` and credited in `CREDITS.md`.

## 3. The HD interface (`patches/interface-hd/`)

A private patch replaces the 2,500 interface images of the `texture/ui` pack with enlarged (×2, ×4)
versions re-encoded like the originals. It is git-ignored (`/patches/interface-hd/`) because these
are derivative works. To rebuild it:

1. **Export the interface pack** from the client (one entry = one file, 2,521 PNG/TIF/BMP):

   ```bash
   cargo run -p archrust-data --example dump_pack -- assets-archonia/Game/texture/ui /tmp/ui
   ```

2. **Upscale** the images with the tool of your choice, writing each result **next to its original**
   with the `_upgrade` suffix (`inven_lock.png` → `inven_lock_upgrade.png`). Dimensions are free; the
   encoding is realigned in the next step.

3. **Realign the encoding** with the originals (PNG colour type and bit depth, gamma and chromaticity
   chunks copied, uncompressed RGBA TIFF with the alpha taken from the original, 24-bit BMP):

   ```bash
   cargo run -p archrust-data --example reformat_like -- /tmp/ui              # report only
   cargo run -p archrust-data --example reformat_like -- /tmp/ui --apply --backup /tmp/ui-before
   ```

   The report must end with every pair "conforme". The 26 `et_*.tif` and `tb_*.tif` frames are
   **uncompressed** RGBA TIFFs: an LZW or alpha-less TIFF is not accepted.

4. **Assemble the patch**: copy each `_upgrade` under its original name into
   `patches/interface-hd/files/` and declare one `[[files]]` per image in `patch.toml`:

   ```toml
   [patch]
   id = "interface-hd"
   name = "HD interface"
   version = "0.1.0"
   priority = 100

   [[files]]
   path = "texture/ui/inven_lock.png"
   file = "files/inven_lock.png"
   ```

   A shell loop is enough to generate the list:

   ```bash
   mkdir -p patches/interface-hd/files
   for f in /tmp/ui/*_upgrade.*; do
     n=$(basename "$f" | sed 's/_upgrade\././'); cp "$f" "patches/interface-hd/files/$n"
     printf '\n[[files]]\npath = "texture/ui/%s"\nfile = "files/%s"\n' "$n" "$n" >> patches/interface-hd/patch.toml
   done
   ```

5. **Check**: `cargo run --release -- patch check patches/interface-hd`, then `patch list` must show
   `interface-hd`; at start-up the log reads `patchs : free-assets (1000), interface-hd (100)`.
   AddOns asking for `texture/ui/<name>` get the HD version.

## 4. Sounds and music

ArchRust plays the files of `Game/sound` directly: nothing to extract to play. To study them or
prepare replacements, the full export (music, ambiences, interface sounds, effects split from the
`.pk` files according to `soundpak.info`) is a single command:

```bash
cargo run -p archrust-data --example dump_soundpak -- assets-archonia/Game/sound /tmp/sounds
cargo run --release -- inspect sound sd.pk --limit 5           # or effect/mono/maa.pk
cargo run --release -- extract sound sd0000.wav sd0000.wav
```

A replacement is declared as a `[[files]]` on the client path: `sound/bgm/htt_001.mp3` (one music
track), `sound/ui/click.wav` (one interface sound) or `sound/effect/sd.pk` (a whole pack, to rebuild
in the same format: WAV files back to back). The `soundpak.info` format is described in
[FORMATS.md](FORMATS.md).

## 5. Third-party HD patches (`patches/*.ahp`)

`.ahp` archives are not versioned (`/patches/*.ahp`). A patch is distributed as a folder or as an
archive built with:

```bash
cargo run --release -- patch build patches/my-patch my-patch.ahp
cargo run --release -- patch check my-patch.ahp
```

Drop received `.ahp` files into `patches/`: they are stacked by decreasing priority, the first one
providing a resource wins. `patch list` shows the effective order.

## 6. Generated local files

| File | Role | How to get it |
|---|---|---|
| `realmlist.wtf` | login server (`set realmlist 127.0.0.1:11002`) | create it next to the executable, see the README |
| `saved-variables/` | options (volumes…) and AddOn variables | created on first launch, git-ignored |
| `addons/*/SavedVariables/` | AddOn data | same |
| `dist/` | test builds with asset copies | `cargo build --release` then manual copy; never published |

## 7. Before pushing to GitHub

```bash
git status --short                      # nothing under assets-archonia/, patches/interface-hd/, free-assets/NN-*/
git ls-files | grep -Ei '\.(dat|tx1|ecl|dff|ean|ma[12]|pk|mp3)$'   # must be empty
du -sh .git                             # a sudden jump means an asset slipped in
```

If a publisher file was committed by mistake, remove it from history before pushing
(`git rm --cached`, then rewrite history if the commit is already pushed).
