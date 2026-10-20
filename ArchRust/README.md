<p align="right">
  <a href="README.en.md"><img src="https://img.shields.io/badge/🇬🇧_Read_in_English-B22234?style=for-the-badge" alt="Read in English"></a>
</p>

<p align="center">
  <img src="docs/assets/archrust-logo.png" alt="Logo officiel d’ArchRust" width="320">
</p>

<h1 align="center">ArchRust</h1>

<p align="center">
  <b>Client ArchLord réécrit en Rust</b><br>
  x86-64 · wgpu · AddOns Lua · patchs HD
</p>

**Réécriture en Rust (x86-64) du client ArchLord 64 bits**, avec un système d'AddOns Lua façon
World of Warcraft et un format de patch graphique HD pour les développeurs.

Les formats de fichiers et le protocole réseau ont été reconstruits par rétro-ingénierie de
l'exécutable du client (Ghidra piloté par [REA](https://github.com/morluto/rea)) et validés contre
les vrais fichiers du jeu et l'émulateur serveur [ArchCore](https://github.com/BlaMacfly/ArchCore).
Ce dépôt ne contient **aucune donnée du jeu** : il faut posséder son propre client.
Licence : [GPL-3.0-or-later](LICENSE.md).

## État

| Domaine | État |
|---|---|
| Chiffrement (RC4 CryptoAPI, Blowfish petit-boutiste, AES-128-GCM des index) | ✅ validé |
| Protocole AuPacket (en-tête 19 octets, schémas, trames, poignée de main) | ✅ validé avec ArchCore |
| Connexion → liste des mondes → compte → personnages → entrée dans le monde → PNJ en vue | ✅ `archrust login` |
| Ressources client (packs/disque, surcharge AES-GCM `etc`, qualité et ordre des textures, glTF natif) | ✅ règles reproduites et testées sur les données du client |
| Packs `Reference.Dat`/`Data.Dat` (character, object, textures, octree…) | ✅ tous les index lus |
| Modèles RenderWare (`.ecl`/`.dff` : frames, géométries, peaux, matériaux) | ✅ 200/200 testés |
| Textures `.tx1` (TXD RenderWare, DXT1/3/5, BGRA, palette) | ✅ 66/66 testées |
| Tuiles du monde (`MagPack` + aPLib) : secteurs, textures, attributs | ✅ 1280/1280 secteurs lus |
| Rendu wgpu (terrain autour du joueur, modèles, caméra orbitale), interface egui | ✅ terrain, joueur et PNJ affichés ([capture](docs/captures/anchorville.png)) |
| Textures du sol (base + couches fondues par masque, extension `0xfffff001`) | ✅ à l'échelle, transitions douces, validé en jeu |
| Objets du monde (`ini/obj%05d.ini` + `objecttemplate.ini`, pack `object`) | ✅ bâtiments, arbres, barrières, décor d'Anchorville ; LOD et animations à faire |
| Modèles de personnages (règle `charactertemplateclient.ini` → fichiers `a/b/c` en hexadécimal) | ✅ joueur et 18 PNJ d'Anchorville |
| AddOns Lua (`.toc`, frames, événements, slash, minuteurs, SavedVariables, LibStub + Ace3, API étendue) | ✅ voir [docs/ADDONS.md](docs/ADDONS.md) |
| Patchs HD (`patch.toml`, dossier ou `.ahp`, priorités, motifs) | ✅ textures/modèles/fichiers |
| Déplacement (clic au sol, `/goto`), hauteur du terrain, paquet 0x40 et arrêt à l'arrivée | ✅ validé avec deux clients sur ArchCore ([capture](docs/captures/deplacement.png)) |
| Cycle d'entités `CHARACTER` (opérations update/remove/remove-for-view) | ✅ schéma et événements vérifiés sur le code Archonia/ArchCore |
| Action optimisée 0x41 | 🟡 facteurs et PV affichés ; variation connue au journal ; barres/animations à faire |
| Inventaire et équipement | 🟡 paquets 0x04 et vues équipées décodés, noms résolus, modèles `d…` chargés ; rendu sur les os à valider |
| Animations `.ean` (attente, course) | ✅ validées en jeu sur ArchCore (TID 96) ; animations de combat à faire |
| Sélection et création de personnage (scène d'origine `LoginSettings.txt` : salle du trône, temples par race, caméras, zoom) | ✅ liste, création race/classe/visage/coiffure/nom, suppression, entrée en jeu, validés sur ArchCore |
| Orientation et têtes par défaut | ✅ personnages tournés vers leur marche, visage et cheveux skinnés (`character\defaulthead`), validés en jeu (TID 96) |
| Son (musique de région `RegionTemplate.ini` → `skyset.ini`, ambiance, effets des packs `.pk` et de `sound/ui`, volumes, `PlaySound` des AddOns) | ✅ rodio ; musique de connexion et d'Anchorville, effets validés sur ArchCore ; table des ambiances à identifier |
| Combat, compétences | ⏳ à faire |

## Démarrage rapide

Prérequis : Rust stable (MSVC), un client ArchLord 64 bits dans `../client/ArchLord/Game`, et pour
jouer, ArchCore lancé (MariaDB + `npm start`).

```bash
cargo build --release
cargo run --release -- inspect pack character              # liste un pack
cargo run --release -- inspect model character a0000005.ecl
cargo run --release -- inspect animation a0060000.ean --time 0.5 --model b0000060.ecl
cargo run --release -- extract texture a0057aab a0057aab.png   # texture décodée
cargo run --release -- inspect sound index                 # soundpak.info : packs de sons
cargo run --release -- inspect sound sd.pk --limit 5       # un pack .pk (ou effect/mono/maa.pk)
cargo run --release -- extract sound sd0000.wav sd0000.wav # un son d'un pack (ou sound/ui/block.wav)
cargo run --release -- login test test Testeur             # flux complet sans affichage
cargo run --release -- play --connect test:test@127.0.0.1:11002/Testeur
cargo run --release -- addons                              # vérifie les AddOns
cargo run --release -- patch build patches/mon-patch mon-patch.ahp
```

### Installation à côté du client et `realmlist.wtf`

`ArchRust.exe` (icône : le logo officiel d'ArchRust, `apps/archrust/res/archrust.ico`) peut être posé dans le dossier `Game` du client, à la place d'`Archonia.exe`, avec
`addons/`, `free-assets/` et `patches/` : il y reconnaît le client sans option, et un double-clic
lance le jeu (`play` est la commande par défaut).

Le serveur de connexion se règle comme dans World of Warcraft, par un fichier `realmlist.wtf` :

```text
# commentaire
set realmlist 127.0.0.1:11002
```

Forme `set realmlist <hôte>[:<port>]` (port 11002 par défaut, guillemets facultatifs, la dernière
ligne l'emporte). Le fichier est cherché à côté de l'exécutable, puis dans le dossier courant, puis
dans le dossier `Game` ; il préremplit l'écran de connexion. `--connect utilisateur:mdp@/personnage`
(sans hôte) se connecte à ce serveur ; un hôte donné à `--connect` reste prioritaire.

L'écran de connexion affiche `crates/archrust-engine/assets/login-screen.png` (« ArchRust — Awakening of the
Eternal Storm »), intégré à l'exécutable ; comme dans WoW, l'interface des AddOns n'apparaît qu'en jeu.

Après la connexion, l'écran de sélection reprend la scène d'Archonia (personnages du compte dans
la salle du trône, clic pour choisir) ; « Créer un personnage » ouvre la scène de création de la
race choisie. Pour les tests : `--connect test:test@` (sans `/personnage`) passe par ces écrans et
`--login-exec "créer; race Orc; classe Berserker; visage 3; nom Grokk; valider; choisir 1; entrer"`
y joue des commandes.

Dans le jeu : clic gauche au sol pour marcher, clic droit pour orbiter, molette pour zoomer,
ZQSD/WASD pour déplacer la caméra, `` ` `` ouvre la console Lua, la fenêtre de discussion accepte
les commandes `/` (`/goto x y z`, `/cam distance [lacet°] [tangage°]`, `/reload`, `/quit`, et celles des AddOns). Pour les tests :
`--exec "/goto …"` exécute une commande à l'entrée en jeu et `--screenshot fichier.png` enregistre
une capture puis quitte (sous Git Bash, préfixer par `MSYS_NO_PATHCONV=1`).

## Architecture

```text
crates/
  archrust-crypto     RC4 (MD5 CryptoAPI), Blowfish LE, AES-128-GCM
  archrust-protocol   AuPacket, schémas de champs, trames A1/B1, poignée de main
  archrust-data       tables ini, packs, MagPack + aPLib, RenderWare (DFF/TXD/secteurs), packs de sons .pk
  archrust-netclient  client tokio : login, monde, événements, commandes
  archrust-addons     Lua 5.4 (mlua), .toc, frames, événements, slash, SavedVariables, LibStub/Ace3
  archrust-patch      manifeste TOML, pile de patchs, archives .ahp
  archrust-engine     winit + wgpu + egui : rendu, terrain, entités, interface, liaison réseau, audio (rodio)
apps/archrust         exécutable : play / login / inspect / extract / addons / patch
addons/HelloArch      AddOn d'exemple (API de base)
addons/ArchAce        AddOn d'exemple LibStub/Ace3 (cadres d'unité, plaques de nom, thème, options)
docs/FORMATS.md       formats de fichiers reconstruits
docs/ROADMAP-ARCHONIA.md jalons de reconstitution du comportement d'Archonia
```

## AddOns

Un AddOn vit dans `addons/<Nom>/` avec un fichier `<Nom>.toc` :

```text
## Title: Hello ArchLord
## Interface: 10000
## SavedVariables: HelloArchDB
core.lua
```

L'API reprend celle de WoW pour que le code et les habitudes se transposent : frames en tables Lua
(`CreateFrame`, ancres, strates, textures, polices `.ttf`, `GameTooltip`, `StaticPopup`, panneau
d'options), événements (`PLAYER_LOGIN`, `UNIT_HEALTH`, `PLAYER_TARGET_CHANGED`, `CHAT_MSG_*`,
`COMBAT_LOG_EVENT_UNFILTERED`…), `Unit*`, `C_Timer`, `SlashCmdList`, `hooksecurefunc`, CVars. Les
bibliothèques Lua standard sûres sont ouvertes (`string`, `table`, `math`, `utf8`, `coroutine`, `os`
réduit, `bit`/`bit32`, `require` borné aux dossiers d'AddOns) avec les alias de WoW (`strsplit`,
`tinsert`, `wipe`, `format`…), et LibStub, CallbackHandler, Ace3 (AceAddon, AceEvent, AceTimer,
AceConsole, AceDB, AceHook, AceLocale, AceSerializer), LibDataBroker et LibSharedMedia sont
intégrées. L'espace `ArchRust` permet de modifier le client : caméra, couleur du ciel et lumière,
teinte des modèles, couleurs des fenêtres natives, raccourcis clavier, plaques de nom
(`ArchRust.WorldToScreen`). Les AddOns se chargent à l'entrée en jeu, comme sous WoW. Pas d'accès
disque hors des fichiers des AddOns et de leurs SavedVariables.

Référence complète : **[docs/ADDONS.md](docs/ADDONS.md)**. Exemple : `addons/ArchAce` (`/archace`).

## Patchs HD

Un patch est un dossier (ou une archive `.ahp`) avec un `patch.toml` :

```toml
[patch]
id = "anchorville-hd"
name = "Anchorville HD"
version = "1.0.0"
priority = 100

[[textures]]
match = "a0057aab"            # nom RenderWare, motif * accepté
file = "textures/a0057aab.png" # PNG/DDS de n'importe quelle taille

[[models]]
match = "character/a0000001.ecl"
file = "models/a0000001.glb"
scale = 100.0                 # glTF en mètres → unités ArchLord (≈ cm)

[[files]]
path = "ini/npc.ini"
file = "npc.ini"
```

Les patchs du dossier `patches/` sont empilés par priorité décroissante ; le premier qui fournit une
ressource l'emporte, sinon les données d'origine sont utilisées. Les `[[models]]` glTF/GLB sont
affichés à la place du modèle du client (personnages, PNJ, secteurs `world/D272,272.dff`), avec leurs
propres textures ; `archrust inspect gltf <fichier>` décrit un glTF.

## Son

Le moteur audio (`rodio`, décodage MP3/WAV par Symphonia) joue trois pistes :

* **musique** : MP3 en boucle avec fondu enchaîné. Sur les écrans de connexion, `sound/bgm/htt_001.mp3`
  (CVar `loginMusic`) ; en jeu, la musique de la région du joueur d'après les tables du client
  (grille `.amf` du secteur → `RegionTemplate.ini` → `skyset.ini`, par tranche horaire, voir
  [docs/FORMATS.md](docs/FORMATS.md)), sinon `worldMusic` ;
* **ambiance** : `sound/stream/*.mp3` en boucle (CVar `worldAmbience`, cité de jour par défaut : la
  table des ambiances n'est pas encore identifiée) ;
* **effets** : WAV ponctuels, `sound/ui/<nom>.wav` (pack), `sound/effect/sd0000.wav` (entrée d'un
  `.pk` par `soundpak.info`, ou balayage RIFF pour `effect/mono/`), fichier d'un AddOn.

Tous les sons passent par les patchs : un `[[files]]` sur `sound/bgm/htt_001.mp3` ou
`sound/effect/sd.pk` remplace la musique ou tout un pack. Sans client (`--no-client`), seuls les
sons fournis par les patchs sont joués, en silence sinon.

Volumes (musique, ambiance, effets, coupure) : fenêtre **Options** (bouton ou touche Échap),
enregistrés dans `saved-variables/options.toml`. Commandes : `/music <chemin>|off|auto`,
`/sound <chemin>` ; AddOns : `PlaySound`/`PlaySoundFile(chemin)`, `PlayMusic(chemin)`, `StopMusic()`.

## Données du client et ressources libres

Le dépôt sépare strictement ce qui appartient à l'éditeur de ce qui peut être publié :

| Dossier | Contenu | Publié ? |
|---|---|---|
| `assets-archonia/` | le dossier `Game` du client (ou un lien vers lui) ; cherché en premier par ArchRust | **Non** : ignoré par git, sauf son README |
| `free-assets/` | tes créations originales (textures, modèles glTF, fichiers), déclarées dans `patch.toml` ; patch de priorité maximale | Oui |
| `patches/` | patchs HD tiers (`.ahp` ou dossiers) | au choix |

Une texture ou un modèle du jeu retouché reste une œuvre dérivée : il ne va pas dans `free-assets/`.
Au fur et à mesure que les ressources libres remplacent celles du client, le jeu devient jouable
sans aucun fichier d'origine :

```bash
cargo run --release -- play --no-client            # patchs et substituts seulement
```

Sans client, tout ce qui manque est remplacé par un substitut généré (damier coloré, boîte, table
vide) et la fenêtre de connexion fonctionne normalement. Les sources de packs libres (CC0) et la
correspondance nom du client → fichier libre sont dans [docs/ASSETS-LIBRES.md](docs/ASSETS-LIBRES.md).

## Avertissement

ArchLord est une marque de son éditeur. ArchRust est un projet indépendant, non affilié, à but
d'étude, d'interopérabilité et de préservation. Voir [LICENSE.md](LICENSE.md).

<p align="right">
  <a href="README.en.md"><img src="https://img.shields.io/badge/🇬🇧_Read_in_English-B22234?style=for-the-badge" alt="Read in English"></a>
</p>
