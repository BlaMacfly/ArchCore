# Ressources libres pour ArchRust

Résultat de la recherche du 2026-10-18 et guide pour remplir `free-assets/`.

## Ce qui existe côté communauté ArchLord

Il n'existe **aucun pack de ressources ArchLord réellement libre**. Les projets communautaires
sont des émulateurs ou des outils, pas des créations graphiques :

| Projet | Contenu | Licence | Utilisable dans `free-assets/` ? |
|---|---|---|---|
| ALEmu / ArchLord: Reborn ([ALReborn](https://github.com/CoolManBob/ALReborn)) | code serveur/client, outils | aucune licence ; mention « usage éducatif, pas d'usage commercial » | non (code, et sans licence libre) |
| Packs « HD » ou « textures » ArchLord sur forums (RaGEZONE, elitepvpers…) | extractions ou retouches des fichiers du jeu | aucune ; œuvres dérivées de l'éditeur | **non**, jamais |
| osiy1996/archlord | émulateur en C | voir le dépôt | non (code, à lire sans copier) |

Conclusion : tout ce qui touche à l'apparence du jeu doit être **créé de zéro** ou pris dans des
packs génériques sous licence libre.

## Packs génériques libres recommandés

Vérifie la licence sur la page de chaque fichier avant de l'ajouter ; les packs ci-dessous sont
annoncés CC0 (domaine public, aucune attribution exigée) au moment de la recherche.

### Personnages (joueurs, PNJ, monstres)

| Pack | Contenu | Formats | Licence |
|---|---|---|---|
| [Quaternius — LowPoly RPG Characters](https://opengameart.org/content/lowpoly-rpg-characters) | 6 personnages rigués et animés (guerrier, mage, voleur, moine, rôdeur…) | FBX, OBJ, Blend (convertir en glTF) | CC0 |
| [KayKit — Adventurers](https://kaylousberg.itch.io/kaykit-adventurers) | 4 à 5 aventuriers low poly, animations de base, accessoires | FBX, **glTF** | CC0 |
| [KayKit — Animations](https://kaylousberg.itch.io/kaykit-animations) | pack d'animations partagées par les personnages KayKit (même squelette) | glTF | CC0 |
| KayKit — Skeletons | squelettes (monstres) sur le même squelette d'animation | glTF | CC0 (à vérifier sur la page) |

Il manque dans ces packs : orcs, elfes de lune et scions dans le style ArchLord, montures, boss.
C'est la partie à créer.

### Villages et décors

| Pack | Contenu | Formats | Licence |
|---|---|---|---|
| [Quaternius — Medieval Village MegaKit](https://quaternius.com/packs/medievalvillagemegakit.html) | 300+ pièces modulaires (murs, toits, portes, fenêtres, escaliers), version gratuite ≈ 176 pièces | FBX, OBJ, **glTF** | CC0 |
| [Quaternius — LowPoly Medieval Village](https://opengameart.org/content/lowpoly-medieval-village-pack) | bâtiments de village complets | FBX, OBJ, Blend | CC0 |

### Textures de terrain et de décor

| Source | Contenu | Licence |
|---|---|---|
| [Poly Haven](https://polyhaven.com/textures) (ex-Texture Haven) | herbe, terre, sable, roche, pavés en PBR 1K–8K | CC0 |
| [ambientCG](https://ambientcg.com) | milliers de matériaux PBR tuilables | CC0 |
| [cgbookcase](https://cgbookcase.com) | textures PBR tuilables, jusqu'à 4K | CC0 |
| [Watercolor Terrain Textures](https://voxelcorelab.itch.io/watercolor-terrain-textures) | 16 textures stylisées (herbe, pierre, eau, terre) | CC0 |
| [OpenGameArt — collections CC0](https://opengameart.org/content/cc0-art) | textures, icônes, sons | CC0 **par fichier** : vérifier chaque envoi |

À éviter : les listings « générés par IA » sans provenance claire (Summer Engine, certains kits
RenderHub) et tout fichier dont la page ne montre pas de licence.

## Recherche élargie : les catégories manquantes

Seconde passe sur tout le web (2026-10-18). « Confirmé » = licence lue sur la page de l'auteur ;
« annoncé » = licence indiquée par le listing ou les moteurs de recherche, page non consultable
par le robot (itch.io) : à relire avant téléchargement.

### Races et monstres

| Besoin | Piste | Licence | Formats | Verdict |
|---|---|---|---|---|
| Orcs, gobelins, trolls | [Quaternius — Ultimate Monsters](https://quaternius.com/packs/ultimatemonsters.html) : 50 monstres animés (attaque, mort, course…) | CC0 **confirmé** | FBX, OBJ, Blend, **glTF** | base pour les monstres ; les orcs « race jouable » au style ArchLord sont **à créer** |
| Monstres low poly | [Quaternius — LowPoly Animated Monsters](https://quaternius.itch.io/lowpoly-animated-monsters) : squelettes, dragons, chauves-souris, slimes | CC0 annoncé | FBX, OBJ, Blend | complément ; le gobelin exclusif est réservé au Patreon |
| Squelettes (morts-vivants) | [KayKit — Skeletons](https://kaylousberg.itch.io/kaykit-skeletons) : 4 à 6 squelettes, 90+ animations partagées avec Adventurers | CC0 annoncé | FBX, glTF | bon pour les monstres de bas niveau |
| Dragons / boss | [Cethiel's Dragon 3D](https://opengameart.org/content/cethiels-dragon-3d) : dragon rigué et animé, 2 textures | CC0 **confirmé** | zip (Blend) | un boss ; convertir en glTF |
| Elfes de lune | [Sloyd — personnage elfe](https://www.sloyd.ai/free-3d-models/model/asset-type-game-ready-3d-character-model-kbwxjsg5) (GLB, CC BY 4.0, attribution) ; [Elf game-ready](https://opengameart.org/content/elf-game-ready-and-animated) (CC-BY-SA 3.0, contaminant) | CC BY / CC-BY-SA | GLB / Blend | **aucun elfe CC0** : à créer, en partant d'un [humanoïde de base CC0](https://opengameart.org/content/base-rigged-stylized-humanoid-character-yw) (oreilles à modéliser) |
| Scions (reptiliens) | [Lizardman OGA](https://lpc.opengameart.org/node/4590) (attribution obligatoire), lizardmen RenderHub/TurboSquid (payants) | non libres | — | **aucun reptilien humanoïde CC0** : à créer |
| Golems | [Fire/Ice/Stone Golem](https://opengameart.org/content/fireicestone-golem) : rigué, 5 animations | CC-BY 3.0 / CC-BY-SA 3.0 (textures CC0) | Blend | utilisable avec attribution, pas CC0 ; sinon à créer |

### Montures et animaux

| Besoin | Piste | Licence | Formats | Verdict |
|---|---|---|---|---|
| Chevaux, loups, animaux | [Quaternius — Ultimate Animated Animals](https://quaternius.com/packs/ultimateanimatedanimals.html) : 12 animaux, 12+ animations chacun (galop, attaque, saut…) | CC0 **confirmé** | FBX, OBJ, Blend, **glTF** | base des montures ; vérifier la présence du cheval dans le zip |
| Loup | [Low Poly Wolf (ManNeko)](https://manneko.itch.io/low-poly-wolf) : rigué, GLB, idle/course | « gratuit usage commercial », pas CC0 explicite | GLB | lire la licence sur la page |
| Monture + cavalier | aucun pack libre « monté » trouvé | — | — | assembler monture + cavalier dans le moteur (le client a une colonne `RIDER_HEIGHT`) |

### Armes, armures, objets

| Besoin | Piste | Licence | Formats | Verdict |
|---|---|---|---|---|
| Armes et boucliers | [KayKit — Fantasy Weapons Bits](https://kaylousberg.itch.io/kaykit-fantasy-weapons-bits) : 25+ épées, haches, marteaux, arcs, bâtons, baguettes, boucliers, lances | CC0 annoncé | FBX, OBJ, glTF | première source |
| Armes « PSX » | [PSX weapon pack (Amos)](https://amos-makes.itch.io/psx-weapon-pack) : dague, hallebarde, katana, épée, masse, bouclier, lance, marteau | CC0 annoncé | glTF, FBX | complément |
| Armures portées (`d%07x`) | rien de libre et rigué sur le squelette ArchLord | — | — | **à créer** sur le squelette choisi |

### Interface, icônes, sons

| Besoin | Piste | Licence | Formats | Verdict |
|---|---|---|---|---|
| Cadres, boutons, fenêtres | [Kenney — Fantasy UI Borders](https://kenney-assets.itch.io/fantasy-ui-borders) (130+ sprites 9-slice) ; [Kenney — UI pack RPG extension](https://opengameart.org/content/ui-pack-rpg-extension) (87 PNG) | CC0 (Kenney) | PNG, SVG | remplace `texture/ui/*.png` |
| Icônes de compétences et d'objets | [Skill, item and spell icons](https://opengameart.org/content/skill-item-and-spell-icons) : 100 icônes 567×567 | CC0 **confirmé** | PNG | début d'inventaire ; il en faudra des centaines (à créer) |
| Sons | [CC0 Sounds Library (OGA)](https://opengameart.org/content/cc0-sounds-library), [RPG Sound Pack](https://opengameart.org/content/rpg-sound-pack), [Dark Ambiences](https://opengameart.org/content/dark-ambiences), [Fantasy Ambient SFX Pack](https://kmontesdev.itch.io/fantasy-ambient-sound-effects-pack-cc0) | CC0 | WAV/OGG | remplace `sound/effect/*.pk` (WAV renommés) |
| Musique | pistes isolées sur OpenGameArt, licences mêlées (souvent CC-BY-SA) | variable | OGG | pas de pack CC0 complet : à composer ou à sélectionner piste par piste |

### Bilan : ce qui reste à créer de zéro

1. Les quatre races jouables au style ArchLord (humain, orc, elfe de lune, scion), hommes et
   femmes, sur un squelette commun compatible avec les animations KayKit ou Quaternius.
2. Les armures et vêtements portés (fichiers `d%07x`) et leurs textures.
3. Les PNJ nommés des villages (marchands, instructeurs) : réutiliser les Adventurers avec des
   tenues différentes.
4. Les boss emblématiques, les montures spécifiques, et les reptiliens.
5. Les icônes d'objets et de compétences en grand nombre, et la musique.

## Que remplacer, et comment le déclarer

Les noms de ressources du client sont connus ; `free-assets/patch.toml` associe un nom à un fichier
libre. Priorités utiles pour voir vite un résultat :

| Famille | Nom côté client | Déclaration dans `patch.toml` |
|---|---|---|
| Carte de couleurs d'un secteur (128×128) | `R{sx},{sy}` (ex. `R325,389`, Anchorville) | `[[textures]] match = "R325,389"` ou `match = "R*"` |
| Textures de sol détaillées | `a*t` du pack `texture/world` (ex. `a1b0b00t`) | `[[textures]] match = "a1b0b00t"` |
| Textures de personnages | `A{dff hex}AA`, `…AB` (ex. `A00060AA`) | `[[textures]] match = "a00060*"` |
| Modèles de personnages | `character/a{dff hex}.ecl` + `b{…}.ecl` (ex. TID 96 → `a0000060`, `b0000060`) | `[[models]] match = "character/a0000060.ecl"` |
| PNJ d'Anchorville | TID 584 → `a0000248`, 179 → `a00000b3`… (voir `docs/FORMATS.md`) | idem |
| Interface | PNG du pack `texture/ui` (ex. `ac_base.png`) | `[[files]] path = "texture/ui/ac_base.png"` |
| Tables (noms, dialogues) | `ini/*.ini` | `[[files]] path = "ini/npc.ini"` |

Étapes :

1. Convertir en glTF/GLB si besoin (Blender : *File → Export → glTF 2.0*).
2. Déposer le fichier dans `free-assets/textures|models|files/`, puis l'ajouter au manifeste.
3. Vérifier : `cargo run -- patch check free-assets`.
4. Tester sans client : `cargo run -- play --no-client --screenshot docs/captures/sans-client.png`.
5. Noter la source et la licence de chaque fichier dans `free-assets/CREDITS.md`.

Les trois familles sont actives : textures, fichiers bruts et modèles glTF/GLB (`[[models]]`). Un
glTF remplace le clump du client et ses textures (scène par défaut, nœuds, UV0, couleurs de sommet,
texture de couleur de base ; peau et animations pas encore). Pense à `scale = 100.0` pour un modèle
en mètres. Sans client ni connexion, `play --no-client` affiche un personnage local (TID 96, donc
`character/a0000060.ecl`) à l'origine : c'est le moyen le plus rapide de voir son modèle. Pour
essayer la chaîne sans modèle à soi, `cargo run -p archrust-engine --example cube_glb -- patches/exemple-gltf`
génère un patch avec un cube texturé, et `cargo run -- inspect gltf <fichier>` décrit n'importe
quel glTF.
