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

## Inconnues

* Structure des entrées `D` à plusieurs atomics (certaines tuiles) ;
* animations `.ean` (conteneur de type 0x29 autour de blocs `RpAnim` 0x1B) ;
* rôle exact des 6 jeux d'UV et de la texture `R` du secteur (carte de couleurs / lightmap) ;
* `world/octree`, `grass`, `water` ; `images/*.bm1` (BMP) ; fichiers `.syj`, `.grp`, `.eff`.
