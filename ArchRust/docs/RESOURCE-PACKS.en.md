<p align="right"><a href="PACKS-DE-RESSOURCES.md">🇫🇷 Lire en français</a></p>

# Resource packs

A resource pack changes how ArchRust looks or sounds **without touching the game files or rebuilding
the client**: it provides its own files, which the client uses instead of the original ones. Packs can
be created, shared, enabled and combined freely.

Two categories, managed separately in the client:

- **Graphics**: textures (ground, characters, NPCs, items, equipment, scenery), UI images, 3D models
  (glTF/GLB), data files.
- **Sounds**: effects, UI, ambience, music.

A pack can also mix both ("mixed" category: it then shows in both lists).

## 1. Where to install a pack

| Folder | Content |
|---|---|
| `patches/` | Community packs: one folder per pack, or a `.ahp` archive |
| `free-assets/` | **Official** ArchRust pack |

`patches/` and `free-assets/` sit next to `ArchRust.exe` (in the client's `Game` folder, or in the
repository for developers). To install a pack, drop its folder or `.ahp` archive into `patches/`, then
restart the client.

## 2. Pack layout

```text
patches/
└── my-pack/
    ├── patch.toml          ← the manifest (required)
    ├── textures/           ← free layout: paths are given in the manifest
    │   └── a0057aab.png
    └── sounds/
        └── click.wav
```

The same folder zipped and renamed to `.ahp` is a pack ready to share (see section 7).

## 3. The `patch.toml` manifest

```toml
[patch]
id = "my-pack"                  # unique, stable identifier (ideally the folder name)
name = "My pack"                # display name
version = "1.0.0"
author = "Me"
description = "What the pack does"
category = "graphics"           # "graphics", "sounds" or "mixed"; missing: inferred from the content
priority = 50                   # higher = applied over the other community packs
min_archrust = "0.1"            # minimum client version; an older client reports it
homepage = "https://…"          # optional

[[textures]]                    # replaces a texture by its client name
match = "a0057aab"              # exact name, or a pattern with * (e.g. "a0057*"); case-insensitive
file = "textures/a0057aab.png"
scale = 1.0                     # texture coordinate scale (default 1)

[[models]]                      # replaces a model with a glTF/GLB
match = "character/a0000060.ecl"
file = "models/human.glb"
scale = 100.0                   # glTF in metres → ArchLord units (≈ cm)

[[files]]                       # replaces a client file by its path (UI, tables…)
path = "texture/ui/ac_base.png"
file = "ui/ac_base.png"

[[sounds]]                      # replaces a sound (path under sound/)
path = "ui/block.wav"
file = "sounds/beep.wav"
```

Only the `[patch]` section (with `id` and `name`) is required; each `[[…]]` section can be repeated as
often as needed.

## 4. Finding a resource name

| Resource | Client name | Declaration |
|---|---|---|
| Sector colour map (128×128) | `R{x},{y}` (e.g. `R325,389`) | `[[textures]] match = "R325,389"` |
| Ground textures | `a…t` in the `texture/world` pack | `[[textures]]` |
| Character textures | `A{number}AA`, `…AB` | `[[textures]] match = "a00060*"` |
| Character models | `character/a{number}.ecl` | `[[models]]` |
| UI images | `texture/ui/<name>.png` | `[[files]]` |
| Tables (names, dialogs) | `ini/<name>.ini` | `[[files]]` |
| UI sounds | `sound/ui/<name>.wav` | `[[sounds]] path = "ui/<name>.wav"` |
| Sound effects | `sound/effect/<name>.wav` (stored in `.pk` packs) | `[[sounds]] path = "effect/<name>.wav"` |
| Music | `sound/bgm/<name>.mp3` | `[[sounds]] path = "bgm/<name>.mp3"` |

Useful commands (from the ArchRust folder):

```bash
archrust inspect pack sound/ui                  # lists the files of a client pack
archrust extract texture a0057aab a0057aab.png  # extracts a texture (as the client sees it, packs included)
archrust extract sound sound/ui/block.wav x.wav # extracts a sound (packs included)
archrust patch check patches/my-pack            # checks a pack: files present, category, version
archrust patch list                             # lists the packs in patches/
```

`/sound <path>` plays a sound in game, handy to listen to a replacement.

## 5. Order and fallback

For each resource, the client looks, in this order, at:

1. the enabled **community** packs (`patches/`), from the highest to the lowest priority;
2. the **official** pack (`free-assets/`);
3. the game's **original data**.

The first one that provides it wins. With equal priorities, the alphabetical order of the identifiers
decides. If a pack's file is missing or unreadable (corrupted image…), the client logs it and moves on to
the next pack, then to the original data: a broken pack does not break the game. Graphics and sounds are
resolved separately.

## 6. Enabling, disabling, ordering

In game, **Options** (Escape key or the `/reglages` command) → **Packs de ressources**:

- two lists, **Graphismes** (graphics) and **Sons** (sounds);
- a box to enable or disable each pack;
- **Haut** / **Bas** (up / down) to change its order among the community packs;
- hovering the name shows the description, author, category, priority and content;
- the pack's problems in red (missing file, client too old);
- **Rétablir la configuration par défaut** (restore the default configuration).

Choices are saved in `saved-variables/packs.toml` and apply **on the next start**. Deleting that file
brings back the default configuration.

## 7. Sharing a pack

```bash
archrust patch build patches/my-pack my-pack.ahp
```

The `.ahp` archive (a ZIP holding `patch.toml` and the files) is dropped as is into `patches/`. **Only
share creations you have the rights to**: a pack must not contain files extracted from the ArchLord
client.

## 8. What is supported, and the limits

| Resource | Formats | Limits |
|---|---|---|
| Textures | PNG, DDS, BMP, JPEG, TGA, TIFF; any size | — |
| UI images | same | replaced file by file; the layout stays the client's |
| Models | glTF / GLB (meshes, UVs, colours, base texture) | **static**: no skeleton or animation; a replaced character no longer animates. The original `.ecl` / `.dff` models cannot be replaced through `[[files]]` |
| `.ean` animations | original format, through `[[files]]` | must match the model's skeleton |
| Sounds | WAV, MP3 | OGG and FLAC not supported |
| `ini` tables | original format, through `[[files]]` | handle with care: an inconsistent table disturbs the game |
| `[[shaders]]` shaders | WGSL | **declared but not applied yet** by the engine |

No hot reloading: a pack change applies when the client restarts.

## 9. Bundled examples

| Pack | Category | What it does |
|---|---|---|
| [`patches/exemple-texture/`](../patches/exemple-texture/patch.toml) | graphics | replaces one texture: the `R325,389` sector colour map with a green and gold checkerboard |
| [`patches/exemple-son/`](../patches/exemple-son/patch.toml) | sounds | replaces one sound: `sound/ui/block.wav` with a beep (`/sound sound/ui/block.wav` to hear it) |
| [`patches/exemple-gltf/`](../patches/exemple-gltf/patch.toml) | graphics | replaces the default human character model with a glTF cube |

The example files are original creations (public domain).
