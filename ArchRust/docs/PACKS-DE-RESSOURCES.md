<p align="right"><a href="RESOURCE-PACKS.en.md">🇬🇧 Read in English</a></p>

# Packs de ressources

Un pack de ressources change l'apparence ou les sons d'ArchRust **sans toucher aux fichiers du jeu ni
recompiler le client** : il fournit ses propres fichiers, que le client utilise à la place de ceux
d'origine. Les packs se créent, se partagent, s'activent et se combinent librement.

Deux catégories, gérées séparément dans le client :

- **Graphismes** : textures (sol, personnages, PNJ, objets, équipement, décor), images de l'interface,
  modèles 3D (glTF/GLB), fichiers de données.
- **Sons** : effets, interface, ambiance, musique.

Un pack peut aussi mêler les deux (catégorie « mixte » : il figure alors dans les deux listes).

## 1. Où installer un pack

| Dossier | Contenu |
|---|---|
| `patches/` | Packs de la communauté : un dossier par pack, ou une archive `.ahp` |
| `free-assets/` | Pack **officiel** d'ArchRust |

`patches/` et `free-assets/` sont à côté de l'exécutable `ArchRust.exe` (dans le dossier `Game` du
client, ou dans le dépôt pour les développeurs). Pour installer un pack, on dépose son dossier ou son
archive `.ahp` dans `patches/`, puis on relance le client.

## 2. Structure d'un pack

```text
patches/
└── mon-pack/
    ├── patch.toml          ← le manifeste (obligatoire)
    ├── textures/           ← organisation libre : les chemins sont donnés dans le manifeste
    │   └── a0057aab.png
    └── sons/
        └── click.wav
```

Le même dossier compressé en ZIP et renommé en `.ahp` est un pack prêt à partager (voir section 7).

## 3. Le manifeste `patch.toml`

```toml
[patch]
id = "mon-pack"                 # identifiant unique et stable (de préférence le nom du dossier)
name = "Mon pack"               # nom affiché
version = "1.0.0"
author = "Moi"
description = "Ce que fait le pack"
category = "graphics"           # "graphics", "sounds" ou "mixed" ; absent : déduit du contenu
priority = 50                   # plus grand = appliqué par-dessus les autres packs de la communauté
min_archrust = "0.1"            # version minimale du client ; un client plus ancien le signale
homepage = "https://…"          # facultatif

[[textures]]                    # remplace une texture par son nom dans le client
match = "a0057aab"              # nom exact, ou motif avec * (ex. "a0057*") ; casse indifférente
file = "textures/a0057aab.png"
scale = 1.0                     # facteur d'échelle des coordonnées de texture (défaut 1)

[[models]]                      # remplace un modèle par un glTF/GLB
match = "character/a0000060.ecl"
file = "modeles/humain.glb"
scale = 100.0                   # glTF en mètres → unités ArchLord (≈ cm)

[[files]]                       # remplace un fichier du client par son chemin (interface, tables…)
path = "texture/ui/ac_base.png"
file = "interface/ac_base.png"

[[sounds]]                      # remplace un son (chemin sous sound/)
path = "ui/block.wav"
file = "sons/bip.wav"
```

Seule la section `[patch]` (avec `id` et `name`) est obligatoire ; chaque section `[[…]]` peut être
répétée autant que voulu.

## 4. Trouver le nom d'une ressource

| Ressource | Nom côté client | Déclaration |
|---|---|---|
| Carte de couleurs d'un secteur (128×128) | `R{x},{y}` (ex. `R325,389`) | `[[textures]] match = "R325,389"` |
| Textures du sol | `a…t` du pack `texture/world` | `[[textures]]` |
| Textures des personnages | `A{numéro}AA`, `…AB` | `[[textures]] match = "a00060*"` |
| Modèles des personnages | `character/a{numéro}.ecl` | `[[models]]` |
| Images de l'interface | `texture/ui/<nom>.png` | `[[files]]` |
| Tables (noms, dialogues) | `ini/<nom>.ini` | `[[files]]` |
| Sons d'interface | `sound/ui/<nom>.wav` | `[[sounds]] path = "ui/<nom>.wav"` |
| Effets sonores | `sound/effect/<nom>.wav` (rangés dans des packs `.pk`) | `[[sounds]] path = "effect/<nom>.wav"` |
| Musique | `sound/bgm/<nom>.mp3` | `[[sounds]] path = "bgm/<nom>.mp3"` |

Commandes utiles (depuis le dossier d'ArchRust) :

```bash
archrust inspect pack sound/ui                 # liste les fichiers d'un pack du client
archrust extract texture a0057aab a0057aab.png # extrait une texture (telle que le client la voit, packs compris)
archrust extract sound sound/ui/block.wav x.wav # extrait un son (packs compris)
archrust patch check patches/mon-pack          # vérifie un pack : fichiers présents, catégorie, version
archrust patch list                            # liste les packs de patches/
```

`/sound <chemin>` joue un son en jeu, pratique pour écouter un remplacement.

## 5. Ordre et repli

Pour chaque ressource, le client cherche, dans cet ordre :

1. les packs **de la communauté** actifs (`patches/`), du plus prioritaire au moins prioritaire ;
2. le pack **officiel** (`free-assets/`) ;
3. les **données d'origine** du jeu.

Le premier qui la fournit l'emporte. À priorité égale, l'ordre alphabétique des identifiants tranche.
Si le fichier d'un pack est absent ou illisible (image corrompue…), le client l'écrit dans son journal et
passe au pack suivant, puis aux données d'origine : un pack défectueux ne casse pas le jeu. Les graphismes
et les sons se résolvent séparément.

## 6. Activer, désactiver, ordonner

Dans le jeu, **Options** (touche Échap ou commande `/reglages`) → **Packs de ressources** :

- deux listes, **Graphismes** et **Sons** ;
- une case pour activer ou désactiver chaque pack ;
- **Haut** / **Bas** pour changer son ordre parmi les packs de la communauté ;
- au survol du nom : description, auteur, catégorie, priorité, contenu ;
- les problèmes du pack en rouge (fichier manquant, client trop ancien) ;
- **Rétablir la configuration par défaut**.

Les choix sont enregistrés dans `saved-variables/packs.toml` et s'appliquent **au prochain démarrage**.
Supprimer ce fichier revient à la configuration par défaut.

## 7. Partager un pack

```bash
archrust patch build patches/mon-pack mon-pack.ahp
```

L'archive `.ahp` (un ZIP contenant `patch.toml` et les fichiers) se dépose telle quelle dans `patches/`.
**Ne partagez que des créations dont vous avez les droits** : un pack ne doit pas contenir de fichiers
extraits du client ArchLord.

## 8. Ce qui est pris en charge, et les limites

| Ressource | Formats | Limites |
|---|---|---|
| Textures | PNG, DDS, BMP, JPEG, TGA, TIFF ; toute taille | — |
| Images de l'interface | idem | remplacées fichier par fichier ; la mise en page reste celle du client |
| Modèles | glTF / GLB (maillages, UV, couleurs, texture de base) | **statiques** : pas de squelette ni d'animation ; un personnage remplacé ne s'anime plus. Les modèles d'origine `.ecl` / `.dff` ne se remplacent pas par `[[files]]` |
| Animations `.ean` | format d'origine, par `[[files]]` | doivent correspondre au squelette du modèle |
| Sons | WAV, MP3 | OGG et FLAC non pris en charge |
| Tables `ini` | format d'origine, par `[[files]]` | à manier avec prudence : une table incohérente gêne le jeu |
| Shaders `[[shaders]]` | WGSL | **déclarés mais pas encore appliqués** par le moteur |

Pas de rechargement à chaud : un changement de pack s'applique au redémarrage du client.

## 9. Exemples fournis

| Pack | Catégorie | Ce qu'il fait |
|---|---|---|
| [`patches/exemple-texture/`](../patches/exemple-texture/patch.toml) | graphismes | remplace une texture : la carte de couleurs du secteur `R325,389` par un damier vert et or |
| [`patches/exemple-son/`](../patches/exemple-son/patch.toml) | sons | remplace un son : `sound/ui/block.wav` par un bip (`/sound sound/ui/block.wav` pour l'entendre) |
| [`patches/exemple-gltf/`](../patches/exemple-gltf/patch.toml) | graphismes | remplace le modèle du personnage humain par défaut par un cube glTF |

Les fichiers des exemples sont des créations originales (domaine public).
