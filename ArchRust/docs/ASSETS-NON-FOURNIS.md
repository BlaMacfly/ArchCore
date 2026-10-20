# Intégrer les ressources non fournies

Le dépôt publie le code d'ArchRust, ses AddOns, ses ressources libres (`free-assets/`) et sa
documentation. Tout ce qui appartient à l'éditeur du jeu en est absent : le client ArchLord 64 bits
lui-même, ses packs de données, et tout ce qui en dérive (exports de référence, images retravaillées,
sons découpés). Ce guide explique, pour chaque famille absente, où se la procurer, où la poser et
comment vérifier qu'ArchRust la voit.

Règle d'or : **rien de ce qui est décrit ici ne doit être poussé sur GitHub**. Le `.gitignore` à la
racine protège les emplacements attendus ; si tu ranges ces fichiers ailleurs, ajoute l'exclusion
toi-même avant tout commit.

## 1. Le client ArchLord 64 bits (indispensable)

ArchRust lit les données du client **Archonia** (ArchLord 64 bits), qui fait référence pour le
projet. Il te faut une installation de ce client ; son dossier `Game` contient notamment :

| Dossier ou fichier | Contenu | Utilisé par ArchRust |
|---|---|---|
| `character/`, `object/`, `effect/` | packs `Reference.Dat` + `Data.Dat` : modèles RenderWare (`.ecl`, `.dff`), animations `.ean` | personnages, PNJ, décors, animations |
| `texture/*/` | packs de textures `.tx1` (RC4), PNG/TIF de l'interface dans `texture/ui` | tout l'affichage |
| `world/` | tuiles `a%06dx.ma1/.ma2`, `b%06dx.ma2` (MagPack) : terrain, textures de secteur, attributs | terrain, hauteur du sol, régions |
| `ini/`, `ini_fr/`… | tables chiffrées (`charactertemplateclient.ini`, `RegionTemplate.ini`, `skyset.ini`…) | TID → modèles, régions, musiques, textes |
| `sound/` | `bgm/*.mp3`, `stream/*.mp3`, pack `ui/`, `effect/*.pk` + `soundpak.info` | musique, ambiances, effets |
| `etc/`, `images/` | splines de caméra, lanceur | caméra de connexion ; le lanceur n'est pas utilisé |
| `Archonia.exe`, `Archonia.ini` | exécutable et configuration d'origine | référence pour la rétro-ingénierie (REA), jamais lancés par ArchRust |

Trois façons de le faire trouver, dans l'ordre où ArchRust cherche :

1. **`assets-archonia/Game`** (ou `assets-archonia/` directement) : copie ou lien symbolique vers le
   dossier `Game`. Tout le dossier est ignoré par git sauf son README.

   ```bash
   # Windows (invite de commandes, droits administrateur ou mode développeur)
   mklink /J assets-archonia\Game "D:\Jeux\Archonia\Game"
   # Linux / macOS
   ln -s /chemin/vers/Archonia/Game assets-archonia/Game
   ```

2. **`../client/ArchLord/Game`** : l'emplacement historique partagé avec le serveur ArchCore.
3. **`--game <dossier>`** sur n'importe quelle commande, ou bien poser `ArchRust.exe` directement
   dans le dossier `Game` du client, avec `addons/`, `free-assets/` et `patches/` à côté : il s'y
   reconnaît sans option (voir « Installation à côté du client » dans le [README](../README.md)).

Vérification :

```bash
cargo run --release -- inspect pack character --limit 5      # liste 5 modèles du pack
cargo run --release -- inspect table charactertemplateclient.ini --limit 3
cargo run --release -- inspect sound index                    # soundpak.info lu
cargo run --release -- play --screenshot verif.png --screenshot-after 4
```

Si le client manque, chaque commande le dit (« demande un client ArchLord ») et `play --no-client`
reste possible : tout ce qui manque est remplacé par un substitut généré (damier, boîte, table vide),
seuls les patchs sont utilisés.

## 2. Les exports de référence (atelier `free-assets/NN-*/`)

Les sous-dossiers numérotés de `free-assets/` (`01-humain/`, `02-orc/`, `05-pnj-anchorville/`…) sont
l'**atelier** des ressources libres : ils contiennent les modèles du client exportés en OBJ + MTL +
PNG, qui servent de gabarit UV pour dessiner les versions libres. Ce sont des données de l'éditeur :
le `.gitignore` les exclut (`/free-assets/[0-9][0-9]-*/`) et ils ne sont pas déclarés dans
`free-assets/patch.toml`.

Pour les régénérer, un modèle à la fois (la correspondance TID → fichiers est dans
[ASSETS-LIBRES.md](ASSETS-LIBRES.md) ; `a%07x` = corps, `b%07x` = armure par défaut) :

```bash
cargo run --release -- export character a0000060.ecl free-assets/01-humain/a0000060.ecl.obj
cargo run --release -- export character b0000060.ecl free-assets/01-humain/b0000060.ecl.obj
cargo run --release -- export character a0000248.ecl free-assets/05-pnj-anchorville/a0000248.ecl.obj
cargo run --release -- extract texture a0057aab free-assets/13-sol-et-terrain/a0057aab.png
```

Une texture ou un modèle du client retouché reste une œuvre dérivée : il ne va pas dans
`free-assets/` publié. Les créations originales, elles, se déclarent dans `patch.toml` et se
créditent dans `CREDITS.md`.

## 3. L'interface HD (`patches/interface-hd/`)

Un patch privé remplace les 2 500 images d'interface du pack `texture/ui` par des versions
agrandies (×2, ×4) et ré-encodées comme les originaux. Il est ignoré par git
(`/patches/interface-hd/`), car ce sont des œuvres dérivées. Pour le reconstruire :

1. **Exporter le pack d'interface** du client (une entrée = un fichier, 2 521 PNG/TIF/BMP) :

   ```bash
   cargo run -p archrust-data --example dump_pack -- assets-archonia/Game/texture/ui /tmp/ui
   ```

2. **Agrandir** les images avec l'outil de ton choix, en écrivant chaque résultat **à côté de son
   original** avec le suffixe `_upgrade` (`inven_lock.png` → `inven_lock_upgrade.png`). Les
   dimensions sont libres ; l'encodage sera réaligné à l'étape suivante.

3. **Réaligner l'encodage** sur les originaux (type de couleur et profondeur PNG, chunks gamma et
   colorimétrie recopiés, TIFF non compressé RGBA avec l'alpha repris de l'original, BMP 24 bits) :

   ```bash
   cargo run -p archrust-data --example reformat_like -- /tmp/ui              # rapport seul
   cargo run -p archrust-data --example reformat_like -- /tmp/ui --apply --backup /tmp/ui-avant
   ```

   Le rapport doit finir sur « conforme » pour toutes les paires. Les 26 cadres `et_*.tif` et
   `tb_*.tif` sont des TIFF RGBA **non compressés** : un TIFF LZW ou sans alpha n'est pas accepté.

4. **Assembler le patch** : copier chaque `_upgrade` sous son nom d'origine dans
   `patches/interface-hd/files/` et déclarer un `[[files]]` par image dans `patch.toml` :

   ```toml
   [patch]
   id = "interface-hd"
   name = "Interface HD"
   version = "0.1.0"
   priority = 100

   [[files]]
   path = "texture/ui/inven_lock.png"
   file = "files/inven_lock.png"
   ```

   Un shell suffit pour générer la liste :

   ```bash
   mkdir -p patches/interface-hd/files
   for f in /tmp/ui/*_upgrade.*; do
     n=$(basename "$f" | sed 's/_upgrade\././'); cp "$f" "patches/interface-hd/files/$n"
     printf '\n[[files]]\npath = "texture/ui/%s"\nfile = "files/%s"\n' "$n" "$n" >> patches/interface-hd/patch.toml
   done
   ```

5. **Vérifier** : `cargo run --release -- patch check patches/interface-hd`, puis `patch list` doit
   afficher `interface-hd` ; au lancement, le journal indique `patchs : free-assets (1000),
   interface-hd (100)`. Les AddOns qui demandent `texture/ui/<nom>` reçoivent la version HD.

## 4. Les sons et musiques

ArchRust joue directement les fichiers de `Game/sound` : rien à extraire pour jouer. Pour les
étudier ou préparer des remplacements, l'export complet (musiques, ambiances, sons d'interface,
effets découpés depuis les `.pk` d'après `soundpak.info`) se fait en une commande :

```bash
cargo run -p archrust-data --example dump_soundpak -- assets-archonia/Game/sound /tmp/sons
cargo run --release -- inspect sound sd.pk --limit 5           # ou effect/mono/maa.pk
cargo run --release -- extract sound sd0000.wav sd0000.wav
```

Un remplacement se déclare en `[[files]]` sur le chemin du client : `sound/bgm/htt_001.mp3`
(une musique), `sound/ui/click.wav` (un son du pack d'interface) ou `sound/effect/sd.pk` (tout un
pack, à reconstituer avec le même format : WAV bout à bout). Le format de `soundpak.info` est
décrit dans [FORMATS.md](FORMATS.md).

## 5. Les patchs HD tiers (`patches/*.ahp`)

Les archives `.ahp` ne sont pas versionnées (`/patches/*.ahp`). Un patch se distribue comme un
dossier ou une archive construite avec :

```bash
cargo run --release -- patch build patches/mon-patch mon-patch.ahp
cargo run --release -- patch check mon-patch.ahp
```

Dépose les `.ahp` reçus dans `patches/` : ils sont empilés par priorité décroissante, le premier qui
fournit une ressource l'emporte. `patch list` montre l'ordre effectif.

## 6. Fichiers locaux générés

| Fichier | Rôle | Comment l'obtenir |
|---|---|---|
| `realmlist.wtf` | serveur de connexion (`set realmlist 127.0.0.1:11002`) | à créer à côté de l'exécutable, voir le README |
| `saved-variables/` | options (volumes…) et variables des AddOns | créé au premier lancement, ignoré par git |
| `addons/*/SavedVariables/` | données des AddOns | idem |
| `dist/` | builds de test avec copies d'assets | `cargo build --release` puis copie manuelle ; jamais publié |

## 7. Avant de pousser sur GitHub

```bash
git status --short                      # aucun fichier sous assets-archonia/, patches/interface-hd/, free-assets/NN-*/
git ls-files | grep -Ei '\.(dat|tx1|ecl|dff|ean|ma[12]|pk|mp3)$'   # doit être vide
du -sh .git                             # une hausse brutale signale un asset ajouté par erreur
```

Si un fichier de l'éditeur a été commité par erreur, retire-le de l'historique avant de pousser
(`git rm --cached`, puis réécriture de l'historique si le commit est déjà poussé).
