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
    (attributs de cellule) : les 16 bits hauts portent l'indice de région (`RegionTemplate.ini`),
    les 16 bits bas les attributs (collision…) ; voir « Sons » pour la musique de région ;
  * `R{sx},{sy}.dff` (`a.ma2`) : dictionnaire de textures du secteur (une texture 128 × 128 DXT1) ;
  * `D{sx},{sy}.dff` (`b.ma2`) : `u32` nombre d'atomics, puis des blocs `Atomic` autonomes
    (Struct de 24 octets, Geometry à 1536 sommets / 512 triangles / 6 jeux d'UV, matériaux
    multi-couches, extension Bin Mesh) ; voir « Textures du sol ».
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

## Sélection et création de personnage

Établi sur Archonia.exe (sha256 e9c7609b…) et les données du client ; implémenté dans
`login_settings.rs`, `net.rs` (`LoginCommand`), `ui.rs` (`character_select_panel`,
`character_create_panel`) et `app.rs` (`update_login_scene`).

* **Mise en scène** — `ini/LoginSettings.txt` (texte CP949 en clair, lu par AgcmUILoginSetting,
  …SettingCamera et …SettingCharacter) : `[ LoginDivision ]` `DivisionNumber=1817` (tuile chargée
  pour la scène), `[ CharacterSelect_Position ]` trois lignes `Pos : X=…, Y=…, Z=…, Rotate : …`
  (centre, gauche, droite), `[ CharacterSelect_MoveOffSet ]` `Race=… Class=… OffSet=196.f` (pas
  en avant du personnage choisi), `[ CharacterCreate_Position ]` une ligne par race,
  `[ CameraTM_CharacterSelect ]`, `[ CameraTM_CharacterCreate ]` par race et
  `[ CameraTM_CharacterCreateZoom ]` par race et classe (`Pos`, `LookAt`, `Perspective`). Races :
  Human, Orc, MoonElf, DragonScion ; classes de mise en scène : Knight, Ranger, Mage, Scion.
  `ini/logincharpos.ini` (chiffré, AgsmLoginClient) reprend les positions côté serveur.
* **Angles** — `Rotate` suit la convention des personnages : -90 = face à −X, vers la caméra de
  sélection placée 490 unités plus à l'ouest. `Perspective` est lue comme la demi-largeur de la
  fenêtre de vue RenderWare (`tan(champ horizontal / 2)` : 0,735 → 73° ; 0,31 en zoom).
* **Paquets** (`AGPMLOGIN`, 0x0D) — liste : op 6 → paquets `CHARACTER` → op 7 ; ArchRust lit aussi
  le visage (champ 20) et la coiffure (champ 21). Création : op 10, le serveur répond par un paquet
  `CHARACTER` par **modèle de base** puis op 10 ; op 11 avec le mini-paquet `LOGIN_CHAR_INFO`
  (0 TID, 1 nom, 6 coiffure, 7 visage) → `CHARACTER` du nouveau personnage puis op 12 (CID).
  Suppression : op 13 avec le nom (champ 1), réponse op 13. Entrée dans le monde : op 8.
* **Modèles de base** — ArchCore envoie les TID 1, 3, 4, 6, 8, 9, 96 (Archer, Sorcerer,
  Berserker, Mage, Hunter, Elementalist, Knight). Race et classe : colonnes `Name` et `Race` de
  `CharacterTemplatePublic.ini` (1 Human, 2 Orc, 3 MoonElf, 4 DragonScion) ; le TID 9 n'a pas de
  facteurs de type et sa race se déduit du nom de classe, comme les boutons de classe du client
  (`createcharacter_btnclass…_normal.png`). Nombres de visages et de coiffures :
  `charactertemplatecustomize.ini` (voir « Têtes par défaut »).
* **Interface d'origine** — `UI_LG_CHARCREATE` (et `_1280`, `_1600`), fonctions
  `CreateCharacter_OnSelectRace…`, `…OnSelectClass…`, `…OnBtnSelectHeadPrev/Next`,
  `…OnBtnSelectFacePrev/Next`, `LG_RotateLeftStart`, `LG_ToggleZoom`, `LG_CharacterCreateOK`,
  `LG_LoginMainSelectButtonClick`, `…CreateButtonClick`, `…DeleteButtonClick`,
  `LG_QUESTION_REMOVE_SELECTED_CHARACTER`, emplacements `LG_Slot01…03`. ArchRust reproduit ces
  commandes avec ses propres fenêtres ; la mise en page et les textures d'interface d'origine ne
  sont pas encore reprises.

## Objets du monde

Établi sur Archonia.exe (sha256 e9c7609b…) et les données du client ; implémenté dans
`crates/archrust-engine/src/objects.rs`.

* **Placement** — `ini/obj%05d.ini`, une table par tuile (`(sx / 16) × 100 + sy / 16`, ex.
  `obj02024.ini` = Anchorville, 1874 objets). Une section par objet, clés nommées écrites par
  ApmObject (`FUN_140233e40`) : `TID`, `ObjectType`, `Scale=x,y,z` (`+0x58`),
  `Position=x,y,z` en coordonnées monde (`+0x64`), `DegreeX` (`+0xb0`), `DegreeY` (`+0xb4`),
  `OcTreeIDNum` / `OcTreeIDData…`.
* **Modèle** — `ini/objecttemplate.ini` (5509 modèles), table positionnelle : 1 `Name`,
  4 `Category`, 10 `DFF` = `lod:fichier` dans le pack `object` (ex. `0:H003b100.ECL`),
  5 `COLLISION_DFF`, 9 `OCTREE_MAXBBOX`, 11 `BSPHERE`, 12-14 LOD, 26 `ANIMATION` (arbres,
  moulins). La colonne 2 apparaît deux fois : `ObjectType` d'ApmObject puis celui d'AgcmObject
  (drapeaux).
* **Repères** — dans la tuile 2024, seule la catégorie « spawn » (lumières `Point_Light` sans
  modèle, effets, repères `monster`) porte le bit `0x10000` des drapeaux AgcmObject : ces objets ne
  sont pas dessinés. La catégorie `NPC` place des modèles de PNJ que le serveur envoie déjà comme
  personnages (`<Garde> Yuan`) : ArchRust l'ignore aussi.
* **Frame racine** — la frame racine des clumps d'objets garde la position d'export du modèle
  (`(1280, 5, 1790)` pour la barrière `h004ac00.ecl`, `(-3100, 15, -3654)` pour l'arbre
  `h003b100.ecl`) ; la matrice de l'objet la remplace. Sans ce retrait, les objets sont décalés
  de plusieurs milliers d'unités.
* **Matrice** — `T(Position) × RotY(DegreeY) × RotX(DegreeX) × S(Scale)`, angles en degrés avec la
  convention des personnages (+Z envoyé sur (sin, 0, cos)). Contrôle (`examples/object_rotation_check.rs`) :
  avec `DegreeY` tel quel, 54 à 59 % des extrémités des barrières (catégorie `필드_펜스`) touchent
  celle d'un autre segment sur les tuiles 2024, 2025 et 1924, contre 9 à 15 % avec l'angle opposé.

ArchRust lit les tuiles des secteurs à moins de 3 secteurs du joueur, prépare un secteur d'objets
par image et dessine les objets à moins de 25 000 unités de la caméra. Vérifié en jeu à
Anchorville : maisons, tours de garde, enclos, champs, étals et arbres en place, 60 ips. Non
couverts : niveaux de détail, animations (`ANIMATION`), effets attachés, collisions et octree.

## Textures du sol

Établi sur Archonia.exe (sha256 e9c7609b…) et les secteurs réels ; implémenté dans
`rw::read_material_extension`, `world::build_terrain_mesh` et `terrain.wgsl`.

* **Extension de matériau `0xfffff001`** — enregistrée par `FUN_1405ec940` (0x60 octets par
  matériau ; lecture `FUN_14087fee0`, taille `FUN_14087fe60`) : `u16` nombre d'entrées, puis pour
  chacune un `u32` type et, s'il est non nul, un bloc `Texture` complet. La taille écrite vaut
  2 + Σ (4 + taille des données de la texture) : elle **omet les 12 octets d'en-tête** de chaque
  bloc Texture. L'erreur se répercute sur l'extension, le matériau, la liste de matériaux, la
  géométrie et l'atomic ; un lecteur qui suit les tailles déclarées perd tous les matériaux après le
  premier (d'où, auparavant, des matériaux « sans texture » et l'absence du Bin Mesh). ArchRust lit
  ce bloc par sa structure : `D325,389` donne ainsi 72 matériaux texturés et 71 sous-maillages.
* **Couches** — sur le terrain, les entrées vont par paires : type 3 = masque
  (`a0001c0t`), type 1 = texture de la couche. Un matériau a une texture de base et 0, 1 ou 2
  couches (`D325,389` : `a0a1a00t` + [masque, `a0a1200t`]…).
* **Jeux d'UV** — l'étage n du matériau (0 = base, puis chaque entrée) lit le jeu d'UV n. Chaque
  cellule de 400 unités a ses six sommets propres : UV0 et les UV des couches vont de 0 à 1 par
  cellule (une répétition de texture par cellule ; quelques bases en atlas 2 × 2, pas de 0,5) ;
  les UV des masques sont en pas de 0,25 (case de l'atlas 4 × 4) ; les étages inutilisés gardent un
  UV par défaut en pas de 0,5 ; UV5 n'est pas lu.
* **Masque** — `texture\world\a0001c0t.tif` : TIFF 256 × 256 non compressé, quatre échantillons
  (RGB blanc + un échantillon `ExtraSamples = 0`, « non précisé ») ; le quatrième porte 16 masques
  de transition 64 × 64 (bords, coins, coins inversés, plein noir, plein blanc). Les décodeurs
  génériques le perdent (alpha 255 : chaque couche recouvre toute sa cellule, en carrés nets) ;
  ArchRust le lit avec `archrust_data::tiff`. Contrôle sur quatre secteurs
  (`examples/terrain_mask_check.rs`) : sur les bords entre une cellule à une couche et une
  voisine sans couche, l'alpha lu tel quel vaut 1 vers une voisine de la texture de la couche et 0
  vers une voisine de même base (écart moyen 0,02 à 0,03, contre 0,3 à 0,8 si la case est retournée).
* **Rendu** — couleur = base(UV0), puis pour chaque couche k :
  `mix(couleur, couche(UV 2k+2), masque(UV 2k+1).a)`, multipliée par la couleur prélit des
  sommets et l'éclairage. Les textures reçoivent des mipmaps (moyennes 2 × 2) au téléversement.
* **Texture `R`** — 128 × 128 pour tout le secteur, elle n'est référencée par aucun matériau.
  L'ancien rendu la plaquait sur l'UV0 de chaque cellule (le secteur entier réduit 16 fois par
  cellule, d'où un sol « miniaturisé ») ; elle n'est plus qu'un repli pour un matériau sans base.

Vérifié en jeu sur ArchCore (Anchorville) : herbe à l'échelle, chemins et route pavée aux bords
fondus, plus de damier.

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

## Animations des personnages

Établi sur Archonia.exe (sha256 e9c7609b…) et sur les données réelles du client ; implémenté dans
`crates/archrust-engine/src/animation.rs` et `Clump::skin_matrices`.

**Choix du clip** — `ini/charactertemplateanimation.ini` (lu par `FUN_14050b1b0`) : section
`[TID]` positionnelle, l'en-tête `[]` nommant les colonnes (`2=ANIMATION_NAME000`…). Chaque valeur
`ANIMATION_NAME…` vaut `type:équipement:variante:fichier` (`"%d:%d:%d:%s"`), rangée dans une table
`[type][équipement]` ; `ANIMATION_ACTIVE_RATE…` (`"%d:%d:%d:%d"`) donne le taux de tirage d'une
variante, `ANIMATION_FLAGS…` des drapeaux, `ANIMATION_POINT…` l'instant d'impact des attaques.
L'équipement est converti par `FUN_1404addf0` selon `ANIMATION_TYPE2` (0 = sans arme). Les types 1
et 2 reçoivent d'office le drapeau 1. Le sens des types est **déduit des données** (amplitude des
clips du TID 96) : 0 = attente, 1 = marche, 2 = course, 3 = attaque, 4 = coup reçu, 5 = mort.

| Clip (TID 96) | Type, variante | Durée | Mouvement |
|---|---|---|---|
| `a0060000.ean` | attente, taux 80 | 1,53 s | respiration, 4° au plus |
| `a0060002.ean` | attente, taux 20 | 7,33 s | attente animée, jusqu'à 122° |
| `a0060200.ean` | course | 0,80 s | cycle, membres jusqu'à 101° |
| `a0060300.ean` | attaque | 1,00 s | jusqu'à 103° |

**Clip `.ean`** — bloc RenderWare `0x1b` (animation HAnim standard) : préfixe de 20 octets
(version `0x100`, type d'interpolateur 1, nombre de clés, drapeaux, durée en `f32`), puis des clés
de 36 octets : temps, quaternion x/y/z/w, translation, décalage en octets de la clé précédente de
la même piste. Les `n` premières clés (lien sans valeur, reste mémoire) ouvrent les pistes des `n`
nœuds HAnim, dans l'ordre du tableau de nœuds ; la clé `n + i` désigne la clé `i` (vérifié 86/86
sur `a0060000.ean`). Le quaternion s'applique tel quel en convention glam : appliqué conjugué, il
écarte le torse de `b0000060.ecl` de 140 unités, tel quel le corps garde ses proportions.

**Skin** — les 86 matrices inverses de bind de `b0000060.ecl` ont une quatrième ligne nulle (mots
de drapeaux/remplissage de `RwMatrix`) : elle est imposée à (0, 0, 0, 1) à la lecture. La matrice
de chaque os vaut `inverse(monde de la frame de l'atomic) × monde animé de l'os × inverse de bind`,
comme RpSkin ; les sommets restent dans l'espace de la frame de l'atomic. En pose de bind, l'écart
est de 4e-6 sur les huit géométries (sans le retrait de la frame de l'atomic, il restait la
translation de 0,45 de ses ancêtres). L'ancienne formule transposée ne revenait au bind que parce
qu'elle perdait les translations, d'où les 180 unités observées une fois animée.

**Rendu** — les géométries skinnées gardent leur liaison au squelette ; le moteur rejoue
l'attente (variantes tirées selon les taux à chaque fin de cycle) ou la course quand l'entité se
déplace, et réécrit positions et normales à 30 Hz au plus, à moins de 8 000 unités de la caméra.
Vérifié en jeu sur ArchCore : TID 96 debout bras le long du corps au lieu de la pose de bind, et
foulée de course pendant un `/goto`. Sondes : `examples/ean_probe.rs` (conventions) et
`examples/ean_motion.rs` (amplitude des clips).

## Orientation des personnages

Établi sur Archonia.exe (sha256 e9c7609b…) ; implémenté dans `scene::turn_towards` et
`Entity::model_matrix`.

* **Angles** — un personnage porte deux angles en degrés, X (`+0x180`) et Y (`+0x184`).
  `FUN_14016a070` (TurnCharacter, aussi exposé au Lua sous ce nom) les range puis construit sa
  matrice avec `D3DXMatrixRotationY(turnY × 0,017453289)` ; X vaut 0 dans les appels de
  déplacement observés.
* **Avant du modèle = +Z** — `FUN_14016bf60` calcule l'angle vers un point :
  `atan(dx / dz) × 57,29579`, corrigé de ±180° selon le quadrant (±90° si `dz = 0`), soit
  `atan2(dx, dz)` en degrés. `D3DXMatrixRotationY` envoie +Z sur (sin, 0, cos) : le modèle regarde
  donc vers sa destination, comme avec `Mat4::from_rotation_y` de glam.
* **Déplacement** — MoveCharacter (`FUN_140168070`) appelle `TurnCharacter(0, angle vers la
  destination)` au départ de chaque déplacement de direction 0 (vers un point) ; les directions
  clavier 3 à 7 ajoutent 45° × (d − 1) ou 180° × (d − 1) à l'angle du vecteur. À l'arrêt, l'angle
  est conservé.
* **Paquet 0x40** — aucun champ d'angle : le récepteur `FUN_140261840` rappelle MoveCharacter, qui
  recalcule l'angle depuis position → destination. Le champ 6 est la direction de déplacement
  (0 = vers la destination), pas un angle. Avec le drapeau `0x08` (synchronisation), l'angle
  courant est réappliqué tel quel ; avec `0x40` (suivi), le personnage est tourné vers sa cible.
* **Vue optimisée (`OPT_VIEW_CHAR`)** — `FUN_140260d40` convertit les champs 12 et 13 (`INT16`)
  en flottants et les passe comme angles X et Y du mini-paquet `CHAR_MOVE` (champs 4 et 5) qu'il
  reconstruit avec position, destination, cible suivie et distance de suivi : le champ 13 est
  l'orientation initiale (ArchCore y met `degree_y` des apparitions de PNJ).

ArchRust tourne l'entité vers sa destination tant qu'elle marche, garde l'angle à l'arrêt, et part
du champ 13 de la vue (sinon de la direction position → destination). Vérifié en jeu sur ArchCore :
en marche vers +X puis vers −Z, le TID 96 court face à sa destination ; arrivé, il garde l'angle.

## Têtes par défaut

Établi sur Archonia.exe et les données du client ; implémenté dans `scene::default_head_files`.

* **Table** — `ini/charactertemplatecustomize.ini`, section `[TID]` positionnelle :
  `DEFAULT_FACE_NUM`, `DEFAULT_FACE0000…` (noms `*.RWS` d'origine, ex. `H_KNI_M_FACE_A.RWS`),
  `DEFAULT_RENDER_TYPE_FACE…` (`6:0`), les mêmes colonnes pour `HAIR`, puis `CUSTOMIZE_REVIEW`.
  TID 96 : 24 visages, 58 coiffures.
* **Fichiers** — `FUN_14074fc70` (visages) et `FUN_14074ff40` (cheveux) parcourent la liste du
  modèle ; quand l'octet `+0x2ad` du modèle est non nul, chaque fichier est
  nommé `"%s%.*x.%s"` avec `V` et 7 chiffres, soit `V%07x.ecl` = `TID × 0x200 + i` pour le visage i
  (dossier `DefaultHead\Face\`) et `TID × 0x100 + i` pour la coiffure i (`DefaultHead\Hair\`).
  Les packs `character\defaulthead\face` (210 entrées) et `…\hair` le confirment : TID 96 →
  `v000c000…v000c017` et `v0006000…v0006039`, autant que les compteurs de la table ; TID 1 et 3 :
  14 et 15 visages en `v00002xx` / `v00006xx`. Les packs ne contenant que ces noms `v…`, c'est ce
  chemin qui sert (sinon le nom `*.RWS` de la table serait chargé tel quel).
* **Choix** — le paquet `CHARACTER` porte le visage (champ 20) et la coiffure (champ 21) ; ArchRust
  les lit pour le joueur, prend 0 pour les autres personnages et retombe sur 0 hors liste.
* **Squelette** — chaque clump de tête porte sa propre copie de la hiérarchie (88 frames, 86 nœuds
  HAnim) et une peau à 86 os, dans le même ordre que `b0000060.ecl` : visage et cheveux se
  déforment avec le clip du corps, sans attache à un os. Vérifié en jeu : TID 96 avec visage et
  cheveux blancs, à l'arrêt comme en course. Sonde : `crates/archrust-data/examples/head_probe.rs`.

## Sons

Dossier `sound/` du client (lecteur : `crates/archrust-data/src/soundpak.rs`, moteur :
`crates/archrust-engine/src/audio.rs`, zones : `crates/archrust-engine/src/zones.rs`) :

| Sous-dossier | Contenu | Format |
|---|---|---|
| `bgm/*.mp3` | 64 musiques (`hft_` humains, `oft_` orcs, `eft_` elfes, `nft_`, villes `htt_`/`ott_`/`ett_`/`ntt_`, batailles `bt_`, `cem_`, `ice_`…) | MP3 |
| `stream/*.mp3` | 65 ambiances par zone et moment (`a_cid_d` cité jour, `_e` soir, `_m` matin, `_n` nuit ; `water_*`, `lightning_*`) | MP3 |
| `ui/` | pack `Reference.Dat` + `Data.Dat` : 137 sons d'interface (`block.wav`, `alarm2.wav`…) | WAV PCM |
| `effect/*.pk`, `effect/mono/*.pk` | WAV mis bout à bout, sans en-tête de pack | WAV PCM 16 bits, 22,05 kHz |
| `effect/*.wav` | 59 WAV en clair (`mo*`) | WAV PCM |
| `soundpak.info` | index des packs `effect/*.pk` (les packs `mono/` n'y figurent pas) | voir ci-dessous |

### `soundpak.info`

Petit-boutiste, chaînes terminées par 0 dans des champs de taille fixe, un ou plusieurs groupes à la
suite jusqu'à la fin du fichier :

```text
groupe (40 octets) : u32 version (= 1), char[8] nom ("EFFECT"), 6 × u32 réservés
                     (observés : 1001, 0, 1002, 0, 1003, 0), u32 nb_packs
  pack (20 octets) : u8 drapeau (0x02), char[7] fichier ("SD.pk"), char[4] préfixe ("SD"),
                     u32 nb_entrées, u32 dernier_index
    entrée (28 octets) : u32 index, char[12] nom ("SD0000.WAV"), u32 décalage, u32 taille, u32 id
```

Le décalage et la taille situent le WAV complet (en-tête `RIFF` compris) dans le `.pk` du même
dossier ; `dernier_index` peut dépasser `nb_entrées - 1` (trous de numérotation : `NP.pk` compte
245 sons, dernier index 8012). Le client Archonia indexe 58 packs et 4 519 sons. Les packs absents
de l'index (`effect/mono/*.pk`, 33 packs) se découpent en balayant les en-têtes `RIFF….WAVE`
(taille = champ RIFF + 8, bornée au fichier) ; ArchRust les nomme `<pack><numéro sur 4 chiffres>.wav`
(`maa0003.wav` = quatrième WAV de `maa.pk`). `archrust inspect sound index|<pack>|<son>` et
`archrust extract sound <son> <sortie.wav>` lisent tout cela ; le `.pk` d'un son s'obtient par
l'index (nom d'entrée) ou, pour le repli, par le préfixe du nom.

### Musique de région

`RegionTemplate.ini` (331 régions : `Name`, `ParentIndex`, `Type`, `WorldMap`, `SkySet`,
`VisibleDistance`…) ne nomme pas de musique ; c'est `skyset.ini` (159 jeux de ciel, section
`[SkySet]`) qui porte, par tranche horaire, les couleurs du ciel, le brouillard, les effets et
`HH_BackGroundMusic` (tranches propres à chaque jeu, par exemple `00`, `05`, `07`, `13`, `19`,
`21` pour le `SkySet` 13 : nom d'un MP3 de `sound/bgm/`, vide = la tranche précédente continue). Chaîne : position → secteur → cellule de la grille `.amf` → région
(16 bits hauts) → `SkySet` → `HH_BackGroundMusic` de l'heure courante. Vérifié : Anchorville
(`C325,389`, région 9) → `SkySet` 13 → `HFT_001`/`HFT_006`/`HFT_008.mp3` selon l'heure ; la salle du
trône de la scène de connexion est la région 22 (`SkySet` 103). L'heure utilisée est l'heure locale
de la machine ; l'heure du jeu et la table des ambiances (`stream/`) restent à établir (les `SkySet`
« 엠비언스 » n'ont pas de musique et pointent sans doute vers d'autres champs).

## Inconnues

* association des équipements `d%07x` aux objets portés ;
* sons : table des ambiances `sound/stream/` et des sons 3D (quel champ de `skyset.ini` ou de la
  grille `.amf`) ; sens des 16 bits bas des cellules `.amf` ; rôle des champs `id` et `drapeau` de
  `soundpak.info` ; `socialsound.ini`, `charactertemplateskillsound.ini`, `ee_move_sound.txt` ;
* `.ean` : rôle des blocs `0x29`, `0x2a`, `0x10` et `0x0c` ; animations selon l'arme (`FUN_1404addf0`) ; 74 clés atypiques dans `a058f302.ean` et `a0800500.ean` ;
* têtes : visage et coiffure des autres joueurs (aucun champ identifié dans la vue optimisée), rôle de `DEFAULT_RENDER_TYPE_*` et de `CUSTOMIZE_REVIEW` ;
* orientation : angles X/Y du mini-paquet `CHAR_MOVE` de `CHARACTER` (op 1) à l'arrêt, non appliqués ; rotation progressive éventuelle entre deux angles ;
* texture `R` du secteur (128 × 128, non référencée par les matériaux) et jeu d'UV 5 ; mode de mélange exact des couches dans le pipeline D3D d'Archonia (le mélange par l'alpha du masque est déduit des données) ; secteurs à plus de deux couches ;
* `world/octree`, `grass`, `water` ; `images/*.bm1` (BMP) ; fichiers `.syj`, `.grp`, `.eff`.
* objets du monde : sens de chaque bit des drapeaux AgcmObject (seul `0x10000` = repère est établi, et seulement par les données), niveaux de détail `DFF` au-delà de 0, animations `ANIMATION`/`ANIM_SPEED` ;
