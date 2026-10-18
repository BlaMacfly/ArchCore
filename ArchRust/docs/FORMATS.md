# Formats de fichiers du client ArchLord 64 bits

Relevés dans `Archonia.exe` (x86-64, sha256 `e9c7609b…`) avec Ghidra via REA, et vérifiés sur les
fichiers du client. Les adresses de fonctions changent à chaque mise à jour automatique du client ;
les formats, non.

## Cryptographie

| Usage | Algorithme | Détail |
|---|---|---|
| Tables `ini/*.ini`, fichier de serveurs | RC4 | clé = 16 octets du MD5 de la chaîne `1111` (CryptoAPI, `CryptDeriveKey`) |
| Textures `.tx1` | RC4 | même mécanisme, chaîne `asdfqwer` |
| Compte / mot de passe au login | RC4 | chaîne de 8 caractères envoyée par le serveur |
| Trames réseau | Blowfish ECB | mots 32 bits petit-boutistes, clés de 32 octets |
| Index des packs `Reference.Dat` | AES-128-GCM | clé `AgcmUIOPTIONSOFA`, IV `AgcmCharactersOf`, étiquette de 16 octets en fin |

La clé et l'IV AES sont stockés obfusqués (33 octets, `b[i] ^= (i % 0x38) + 0x32`) dans le
singleton `ApCrypto`.

## Packs `Reference.Dat` + `Data.Dat`

Index déchiffré :

```text
u32 nombre d'entrées
u32 longueur, puis nom racine terminé par 0 ("character", "\images", "packed_client"…)
par entrée : u32 longueur, nom terminé par 0 (mis en minuscules par le client), u64 offset, u64 taille
```

`Data.Dat` est la concaténation des fichiers. Contenus observés : `character/*.ecl` (clumps
RenderWare), `object/*.ecl`, `texture/<groupe>/*.tx1` (+ PNG pour l'interface et les cartes),
`character/animation/*.ean`, `effect/clump/*.ecl`, `world/octree|grass|water/*.dat`.

### Résolution des ressources

Le chargeur d'ArchRust reproduit les règles relevées dans `Archonia.exe` :

* Au démarrage, chaque dossier contenant `Reference.Dat` et `Data.Dat` est indexé comme un pack,
  sous son chemin relatif en minuscules (`character`, `texture\\character\\high`, etc.).
* Pour ouvrir `dossier\\fichier`, si un pack existe pour ce dossier, seule l'entrée du pack est
  consultée. Une entrée absente ne retombe pas sur le fichier du disque. Sans pack pour ce dossier,
  le client lit le fichier en clair sur le disque.
* Si cette lecture échoue, le chargeur peut chercher le chemin complet (`dossier\\fichier`) dans
  le pack de surcharge `etc` (racine `packed_client`). Les entrées de ce pack sont chiffrées par
  AES-128-GCM avec la seconde clé `AgcmCharacterOpt`, l'IV `AgcmOptionDesign` et une étiquette de
  16 octets en fin. Le contenu déchiffré conserve son format habituel (par exemple RenderWare ou,
  pour un `.tx1`, le chiffrement RC4 des textures). Le chiffrement et les clés sont documentés dans
  `crates/archrust-crypto/src/pack.rs`.
* Pour remplacer un modèle `.ecl` par un glTF/GLB natif, le client cherche d'abord le même nom de
  base avec l'extension `.glb` dans le même dossier.

Les textures RenderWare `.tx1` sont recherchées sous le nom `<nom>.tx1`, dans cet ordre de dossiers
(casse normalisée en minuscules) :

1. `texture\\effect\\<qualité>`
2. `texture\\ui`
3. `texture\\character\\<qualité>`
4. `texture\\object\\<qualité>`
5. `texture\\world\\<qualité>`
6. `texture\\skill`
7. `texture\\item`
8. `texture\\etc`
9. `texture\\minimap`
10. `texture\\notpacked`
11. `texture\\amboccl`
12. `map\\temp\\rough`

`<qualité>` est choisi indépendamment pour les effets, personnages, objets et monde par les clés
`TexEffect`, `TexCharacter`, `TexObject` et `TexWorld` de `[Patch_Option]` dans `ini/COption.ini` :
`0` ou valeur absente = dossier de base, `1` = `high`, `2` = `medium`, `3` = `low`. Le dossier
`texture\\ui` et les autres groupes n'ont pas de sous-dossier de qualité dans cet ordre de recherche.

ArchRust applique le repli du pack `etc` à chaque chemin de texture absent de la recherche normale.
Cette voie a été vérifiée sur les données du client : `texture\\character\\a0157caa.tx1`, présent
dans `etc` et absent des packs de textures parcourus, est déchiffré puis décodé en texture DXT3 de
512 × 512 (10 niveaux de mipmap). La preuve confirme le chargeur d'ArchRust sur les fichiers réels
du client ; l'appel direct du lecteur de textures à ce repli dans `Archonia.exe` reste une
inférence, car il n'a pas été observé dans le code désassemblé.

## RenderWare 3.7

Les modèles et textures sont des flux RenderWare (estampille `0x1C02002D` = 3.7.0.2) :
`Clump → FrameList, GeometryList → Geometry (Struct, MaterialList, Extension{BinMesh, Skin})`,
`Atomic`, extensions `HAnim PLG` et `Frame Node Name`. Les dictionnaires de textures sont en
format D3D9 (DXT1/3/5, A8R8G8B8, palette 8 bits).

## Tuiles du monde (`world/`)

* `a%06dx.ma1`, `a%06dx.ma2`, `b%06dx.ma2` : archives **MagPack Ver 0.1a**
  * en-tête de 50 octets (signature + zéros) ;
  * table des matières : `u8 longueur, nom, u32 taille ^ 0x6969`, jusqu'à une longueur nulle
    (toujours 256 entrées = 16 × 16 secteurs) ;
  * données : pour chaque entrée, `taille` octets puis un `u32` nul.
* Chaque entrée : `u32 taille décompressée ^ 0x6969`, puis un flux **aPLib** (littéraux, codes
  gamma, correspondances courtes et longues, fin sur décalage nul).
* Contenus :
  * `C{sx},{sy}.amf` (`.ma1`) : 1040 octets = 16 octets d'en-tête + grille 16 × 16 de `u32`
    (attributs de cellule) ;
  * `R{sx},{sy}.dff` (`a.ma2`) : dictionnaire de textures du secteur (une texture 128 × 128 DXT1) ;
  * `D{sx},{sy}.dff` (`b.ma2`) : `u32` nombre d'atomics, puis des blocs `Atomic` autonomes
    (Struct de 24 octets, Geometry à 1536 sommets / 512 triangles / 6 jeux d'UV, matériaux dont
    un texturé par une texture `texture/world`, extension Bin Mesh).
* Coordonnées : secteur de **6400** unités, indice = `coordonnée / 6400 + 400`, tuile =
  `(sx / 16) * 100 + sy / 16`. Le secteur `D272,272` couvre x, z ∈ [-819200, -812800].

## Tables `ini`

Texte INI (`[section]`, `clé=valeur`, clés répétées autorisées) en CP949 pour `ini/`, en
Windows-1252 pour `ini_fr`, `ini_de`, `ini_es`. Certaines tables sont **positionnelles** : la
section d'en-tête `[]` nomme les colonnes (`0=ModuleData1`, `1=DFF`…) et chaque `[TID]` donne
`colonne=valeur` ; `charactertemplateclient.ini` colonne `DFF` = numéro du clump `a%07d.ecl`.

## Protocole réseau

Voir la documentation d'ArchCore (`docs/wiki/FR-Protocole-reseau.md`) et les commentaires de
`crates/archrust-protocol`. Points vérifiés avec le vrai client : en-tête AuPacket de 19 octets,
longueurs en `u32`, drapeaux de champs de 1/2/4 octets selon le module, un champ `PACKET` = `u32`
longueur + charge (une seule longueur), trames `A1/B1 … AF/BF` chiffrées Blowfish.

Le paquet optimisé d'action personnage `0x41` suit les neuf champs du schéma ArchCore : CID
attaquant, CID cible, résultat (`i8`), paquet de facteurs, PV cible (`i32`), combo (`i8`), attaque
forcée (`i8`), effet additionnel (`u32`) et index du coup (`u8`). Le callback Archonia
`ap_optimized_packet2_on_receive_char_action` lit précisément les CID, résultat, paquet de facteurs,
PV, combo, attaque forcée et effet additionnel ; l'index du coup n'est pas consommé par ce callback.
Archonia construit le paquet de facteurs avec `ap_factors_make_damage_packet` au format
`AGPMCHAR_FACTOR_POINT` : 20 champs `i32` à bitmap sparse (PV, PM, SP, EXP, AP, MAP, MI, aggro,
dégâts normaux, magique, eau, feu, terre, air, poison, foudre, glace, bonus d'EXP, héroïque).
Pour ce paquet de dégâts, le code fournit précisément PV, PM, dégâts normaux et élémentaires,
ainsi que les dégâts héroïques. ArchRust conserve les octets et décode ces champs avec le mini-schéma
`FACTOR_POINT`. Les PV courants de la cible sont conservés sur l'entité et affichés dans le panneau
du monde. Quand deux valeurs de PV connues diffèrent, le journal de discussion affiche aussi leur
variation (`+/- PV`) pour l'acteur et la cible. Les barres au-dessus des personnages, les animations,
les effets et le traitement des dégâts restent à implémenter. ArchCore n'émet pas encore le paquet
`0x41`.

Le paquet `CHARACTER` (`0x02`) utilise les opérations `ADD=0`, `UPDATE=1`, `REMOVE=2` et
`REMOVE_FOR_VIEW=3`, confirmées par `ap_character.h`. Le serveur Archonia envoie explicitement
`REMOVE_FOR_VIEW` aux observateurs lorsqu'un personnage sort de leur vue (`as_map.c`). ArchRust
traite les mises à jour incrémentales de position/facteurs et retire les personnages sur les deux
opérations de suppression. Le cache de vue et la liste d'entités du moteur sont synchronisés avec
ces événements.

Le schéma principal `AgpmItem` (`0x04`, drapeaux sur 4 octets) reprend les 25 champs relevés dans
`ap_item_create_module` (`ap_item.c`). ArchRust décode les opérations `ADD=0`, `REMOVE=1` et
`UPDATE=2`, conserve les objets par ID et affiche les statuts inventaire (2) et équipé (3) dans la
fenêtre Inventaire. À l'entrée en jeu, `OPT_VIEW` op 2 regroupe les `AgpmItem` dans le bloc mémoire
6 ; chaque enregistrement est préfixé par sa longueur u32 dans le format x64, puis contient drapeaux
et champs `AgpmItem`. ArchRust lit cet instantané et remplace son état local. Les emplacements
inventaire et banque sont des mini-paquets à drapeaux sur un
octet contenant trois `INT16` (onglet, ligne, colonne), d'après le constructeur `packet_inventory`.
`OPT_VIEW` op 1 transporte les objets équipés sous forme d'une liste de mini-paquets
`OPT_VIEW_ITEM` dans le champ 2. ArchRust en extrait ID, TID et quantité. Le nom d'un objet vient
de la colonne `Name` de `ItemTemplateAll.ini` ; `BASE_DFF` donne son clump `character/d%07x.ecl`.
Vérification sur les données réelles : le TID 1 « Short Bow » a `BASE_DFF=1` et
`character/d0000001.ecl` est présent (3 frames, 1 géométrie, 1 atomic). ArchRust charge ces clumps
pour les personnages en vue. L'alignement des modèles sur les os n'est pas encore validé en jeu.
ArchCore ne transmet actuellement aucun objet ni équipement à l'entrée en jeu ; le flux reste à
valider en session, de même que les icônes, les options d'objets et les mises à jour d'équipement.

Les entrées `D` portent, après les extensions RenderWare, des données privées du client (blocs
d'identifiants non RenderWare) : le lecteur s'arrête au premier en-tête qui ne tient pas dans le
bloc. Vérifié sur 1280 secteurs (tuiles 1717, 1924, 2024, 2025, 3232) : 1536 sommets et
512 triangles partout, de 1 à 85 matériaux selon le secteur.

## Modèles de personnages

Les numéros de fichiers du pack `character` sont les valeurs de `charactertemplateclient.ini`
écrites en **hexadécimal sur 7 chiffres** (`%07x`) :

| Colonne | Fichier | Contenu |
|---|---|---|
| `DFF` | `a%07x.ecl` | squelette (frames + HAnim) et corps de base, parfois sans géométrie |
| `DEFAULT_ARMOUR_DFF` | `b%07x.ecl` | armure/vêtements par défaut (peau sur le même squelette) |
| `PICK_DFF` | `c%07x.ecl` | volume de sélection (boîte) |
| objets équipés | `d%07x.ecl` | 3557 fichiers d'équipement (à relier à `ItemTemplateAll.ini`) |

Exemples vérifiés : TID 96 (joueur humain) → `a0000060.ecl` (87 frames, 0 atomic) +
`b0000060.ecl` (8 géométries) ; PNJ TID 584 → `a0000248.ecl` ; TID 179 → `a00000b3.ecl`. Les
textures suivent la même numérotation (`A00096AA` pour le fichier `a0000096`, c'est-à-dire le
TID 0x96 = 150). Le chargeur générique du client assemble `"%s%s.%s"` (dossier, nom, extension).

## Inconnues

* association des équipements `d%07x` aux objets portés ;
* `.ean` : le pack `character/animation` contient 4 886 entrées, dont 4 580 `.ean`. Le lecteur
  ArchRust parcourt désormais le flux complet de chaque fichier. Il y a 4 579 blocs `0x1b`, chacun
  avec 20 octets de préfixe et des enregistrements de 36 octets. L'interprétation actuelle du
  flux est temps, quaternion (x/y/z/w), translation, puis un mot de liaison : elle produit des
  flottants finis et des translations cohérentes avec certains squelettes, mais reste à confirmer
  dans le sérialiseur d'Archonia. Les en-têtes RenderWare de référence décrivent `RpHAnimKeyFrame`
  en mémoire comme pointeur précédent, temps, quaternion, translation ; cet ABI ne prouve pas
  l'ordre employé dans le flux `.ean`.
  Le contrôle de toutes les clés donne des temps finis ; 74 rotations/translations atypiques sont
  concentrées dans `a058f302.ean` et `a0800500.ean`. Les blocs `0x29`, `0x2a`, `0x10` et `0x0c`
  sont préservés, mais leur rôle reste à établir ; les clés ne sont pas encore reliées aux os et
  appliquées aux maillages. Les liens internes alignés sur des offsets de 36 octets reconstruisent
  des pistes candidates. Sur les fichiers réels, `a0060000.ean` a 3 997 clés : 3 911 mots de
  liaison sont des offsets précédents alignés sur 36 octets et les 86 autres forment les racines
  candidates ; `a0001000.ean` donne de même 2 460 liens internes pour 2 543 clés et 83 pistes.
  Pour `a0001011.ean`, le graphe donne 83 pistes (82 de 14 clés, une de 2)
  et l’échantillonnage à mi-durée produit 83 transformations finies. Ce regroupement est déduit du
  flux. Le rapprochement pertinent pour le TID 96 est `a0060000.ean` avec `a0000060.ecl` :
  les deux exposent 86 éléments (pistes candidates et nœuds HAnim), et 78 des 86 pistes ont
  comme frame le plus proche leur frame d'indice correspondant lorsqu'on compare le premier
  échantillon à la transformation locale au repos ; les translations du premier échantillon
  correspondent à celles du squelette pour 76 pistes sur 86. C'est un indice d'ordre, mais
  l'association complète et la convention des rotations restent à établir. Le calcul
  expérimental de pose et de
  skinning est implémenté, mais l'application directe des clés à `b0000060.ecl` déplace certaines
  géométries jusqu'à 180 unités ; il n'est pas validé pour l'affichage. La pose sans animation
  conserve les sommets à moins de `2.5e-5`. Il faut résoudre l'écart entre transformation au repos
  et clés `.ean`, puis comparer plusieurs clips. Les probes sont `inspect_ean.rs` et
  `inspect_animation_rig.rs`. `cargo run -- inspect animation <nom>.ean --time <secondes> --model
  <rig>.ecl` résume désormais les blocs/pistes et compare les premières translations aux indices
  HAnim du rig. Pour TID 96, les échantillons du clip `a0060000.ean` ne reproduisent pas la pose
  de bind : l'écart maximal des matrices monde est de 83.85 à t=0, et les géométries skinnées se
  déplacent jusqu'à 180.46 unités. Ces mesures ne prouvent pas que le clip est mal interprété : la
  convention des rotations, l'association des pistes et les offsets d'animation restent à valider
  contre le runtime Archonia ; le skinning animé ne doit donc pas encore alimenter le rendu ;
* `.ecl` skinnés : `Skin::deform_positions` applique les quatre influences de chaque sommet avec
  une matrice de peau par entrée de palette. Sur `character/b0000060.ecl`, la palette HAnim de 86
  os correspond aux indices de peau ; en combinant les matrices monde des os avec les matrices
  inverses de bind transposées dans la convention `glam`, les huit géométries reviennent à leur
  pose de bind avec un écart maximal inférieur à `2.5e-5`. L'échantillon est reproductible avec
  `crates/archrust-data/examples/inspect_character_skin.rs`. Il reste à alimenter ces matrices
  avec une pose `.ean` et téléverser les sommets déformés pour chaque image ;
* rôle exact des 6 jeux d'UV et de la texture `R` du secteur (carte de couleurs / lightmap) ;
* `world/octree`, `grass`, `water` ; `images/*.bm1` (BMP) ; fichiers `.syj`, `.grp`, `.eff`.
