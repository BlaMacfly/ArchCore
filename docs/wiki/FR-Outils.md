# Outils

Tous les outils sont des sous-commandes de l'exécutable `archcore` (voir [[Installation|FR-Installation]]).
Ils se lancent depuis la racine du dépôt et lisent `config/auth.conf` et `config/world.conf`, ou ceux du
dossier désigné par `ARCHCORE_CONF_DIR` (voir [[Configuration|FR-Configuration]]).

| Commande | Rôle |
|---|---|
| `archcore config migrate <archcore.json> [dossier] [--force]` | Convertit une ancienne configuration JSON en `auth.conf` et `world.conf` |
| `archcore db-init [--account nom mdp [mj]]` | Crée les bases et applique `sql/` ; avec `--account`, crée ou met à jour un compte |
| `archcore decrypt [source] [destination]` | Déchiffre `Game/ini` (RC4, MD5(« 1111 »)) vers `data/client-ini` |
| `archcore import [dossier] [--server dossier]` | Importe `data/client-ini` dans `archcore_world` (CP949 → UTF-8) ; avec les tables du serveur d'origine (`--server`, ou `data/server-ini` s'il existe), importe aussi les apparitions de monstres et la carte serveur |
| `archcore sim client\|move\|shop\|addon\|combat\|equip\|potion\|scroll\|patrol\|bag\|skill\|effects\|bank\|trade …` | Clients simulés (voir plus bas) |
| `archcore i18n strings [Game]` | Liste les textes encore en anglais dans `data/i18n/pending-en.json` |
| `archcore i18n coverage [Game]` | Mesure la part déjà traduite de `ini_fr` |
| `archcore i18n apply [--ascii] [--dry]` | Applique la mémoire de traduction au client (voir [[Traduction|FR-Traduction]]) |
| `archcore re extract-schemas <fichiers…>` | Extrait les schémas de paquets des pseudo-codes Ghidra vers `data/re/packet-schemas.json` |

## Simulateur de client

```bash
archcore sim client <utilisateur> <mot_de_passe> [personnage] [commandes…]
```

Il affiche chaque paquet échangé, le nombre de PNJ reçus, et termine par « flux complet OK ».
Les arguments suivant le personnage sont envoyés dans la discussion une fois en jeu (par défaut
`/GetExp`), ce qui permet de tester les commandes MJ. C'est la manière la plus rapide de vérifier une
modification du serveur sans lancer le jeu.

Sous Git Bash, préfixer par `MSYS_NO_PATHCONV=1` : sinon `/GetExp` est transformé en chemin Windows.

## Test à deux joueurs

```bash
archcore sim move [compteA] [mdpA] [persoA] [compteB] [mdpB] [persoB]
```

Deux clients simulés entrent en jeu (comptes `sima` et `simb` par défaut, personnages créés au
besoin). Le test vérifie qu'ils se voient, que le déplacement de l'un est relayé à l'autre, qu'ils
se perdent de vue hors de portée, et que la déconnexion retire le personnage. Il déplace les
personnages et sauvegarde leur position : à lancer contre un serveur de test, par exemple :

```bash
ARCHCORE_CONF_DIR=config/test archcore all      # autres ports, autres bases
ARCHCORE_CONF_DIR=config/test archcore sim move
```

## Test de boutique

```bash
ARCHCORE_CONF_DIR=config/test archcore sim shop [compte] [mdp] [personnage]
```

Le personnage (`sima` / `SimA` par défaut) reçoit 1000 pièces d'or et apparaît près de Darryl
(Anchorville). Le test ouvre sa boutique, vérifie la liste reçue, achète 3 petites potions de vie
puis les revend. Il modifie l'or, la position et l'inventaire du personnage : à lancer lui aussi
contre un serveur de test.

## Test de combat

```bash
ARCHCORE_CONF_DIR=config/test archcore sim combat [compte] [mdp] [personnage] [monstre]
```

Le personnage, un compte MJ (`gmtest` / `GmTest` par défaut), part chasser au nord d'Anchorville
(Limelight Nord) le monstre demandé (`Green Worm` par défaut). Le test se déroule en deux temps :

1. **Défaite** : il attaque ; le monstre riposte ; il s'éloigne de 1500 unités et le monstre le
   poursuit ; il meurt, demande la résurrection et revient au village avec tous ses PV.
2. **Victoire** : avec `/invincible 1`, il attaque un autre monstre, qui ne le blesse plus ; le monstre
   meurt, l'expérience est gagnée, son butin tombe au sol (de l'or au moins) et le personnage ramasse
   tout ; le cadavre disparaît puis le monstre réapparaît.

Il remet ensuite `/invincible 0` et ramène le personnage au village. Compter environ 2 min : un
guerrier de niveau 1 sans arme frappe un Ver vert (136 PV) 2 à 3 PV par coup, et le ver le tue en
une quinzaine de secondes.

## Test d'équipement

```bash
ARCHCORE_CONF_DIR=config/test archcore sim equip [compte] [mdp] [personnage]
```

Un compte MJ (`gmtest` / `GmTest` par défaut) se crée une épée courte, une armure de guerrier humain
(niveau 2) et une baguette de mage avec `/create`, porte l'épée (dégâts 11 à 13) puis l'armure (défense
4), voit la baguette refusée (classe), puis range les deux. SimA, à côté, doit voir l'épée apparaître sur
lui puis disparaître. Les objets créés restent dans l'inventaire.

## Test des potions et de la régénération

```bash
ARCHCORE_CONF_DIR=config/test archcore sim potion [compte] [mdp] [personnage]
```

Un compte MJ (`gmtest` / `GmTest` par défaut) se crée 3 Petites potions de vie, se fait blesser par un Ver
vert, devient invincible et rentre au village ; le test attend la régénération hors combat (PV en hausse
une dizaine de secondes après le dernier coup), boit une potion (PV en hausse, pile diminuée, op 4), puis
vérifie qu'une seconde potion aussitôt est refusée.

## Test des parchemins et de la patrouille

```bash
ARCHCORE_CONF_DIR=config/test archcore sim scroll [compte] [mdp] [personnage]
ARCHCORE_CONF_DIR=config/test archcore sim patrol [compte] [mdp] [personnage]
```

`sim scroll` (compte MJ `gmtest` par défaut) se crée trois parchemins et vérifie le passeport de Tullan
(destination fixe), l'aller-retour (village puis retour à Tullan par op 6) et le passeport simple (village).
`sim patrol` (`sima` par défaut) reste immobile chez les Vers verts et attend que des monstres se promènent
(en marchant), puis rentre au village.

## Test de l'inventaire et de la résurrection

```bash
ARCHCORE_CONF_DIR=config/test archcore sim bag [compte] [mdp] [personnage]
```

Un compte MJ (`gmtest` par défaut) se crée une pile neuve de Petites potions de mana, la sépare (op 17),
la réunit (op 2 sur la case de la pile d'origine) puis la détruit (op 21) ; enfin `/kill` et résurrection :
il doit revenir au point de résurrection d'Anchorville (-476000, -61000).

## Test de la banque et de l'échange

```bash
ARCHCORE_CONF_DIR=config/test archcore sim bank [compte] [mdp] [personnage]
ARCHCORE_CONF_DIR=config/test archcore sim trade [compteA mdpA persoA compteB mdpB persoB]
```

`sim bank` (compte MJ `gmtest`) vérifie le refus d'un dépôt sans gardien, ouvre la banque chez Holly (au
sud d'Anchorville), y dépose une pile de potions et 1000 pièces, se reconnecte et les retrouve, puis les
retire et détruit la pile. `sim trade` place GmTest (MJ) et SimA au même endroit : GmTest propose une pile
de potions et 500 pièces, SimA 100 pièces ; l'objet proposé ne peut pas être détruit ; verrous, échange
(SimA reçoit la pile, l'argent est ajusté des deux côtés), puis une seconde demande refusée.

## Test des compétences

```bash
ARCHCORE_CONF_DIR=config/test archcore sim skill [compte] [mdp] [personnage]
```

Un compte MJ (`gmtest` par défaut, guerrier humain) vérifie qu'il connaît « Normal Attack », passe au
niveau 10 (`/level`), se rend chez Black, l'instructeur des guerriers d'Anchorville, apprend Dual Slash
(ou se voit répondre « déjà apprise »), se voit refuser Flame Core (compétence de mage), améliore Dual
Slash, la lance sur un Ver vert (incantation, dégâts), vérifie le refus pendant le délai de relance, puis
reprend son niveau d'origine.

```bash
ARCHCORE_CONF_DIR=config/test archcore sim effects [compte] [mdp] [personnage]
```

Le compte MJ, avec un personnage à part (`GmEffets`, créé au besoin, pour ne pas renforcer celui des
autres tests), passe au niveau 25 et apprend chez Black Speed Attack, Defense Hold, Linear Blow et Blade
Sting (si besoin). Il vérifie la passive (défense +20), le renforcement (effet annoncé sur lui, vitesse
d'attaque +18 %), la zone de Linear Blow (cible et voisins touchés) et l'affaiblissement de Blade Sting
(posé sur le Ver vert, retiré au bout de 6 s). Les PM d'un guerrier ne croissant pas encore avec le
niveau, `/level` les rend entre deux lancements.

## Test des extensions ArchRust

```bash
ARCHCORE_CONF_DIR=config/test archcore sim addon [compteA mdpA persoA compteB mdpB persoB compteC mdpC persoC]
```

Trois clients simulés entrent en jeu au même endroit : A et B (`sima` / `SimA`, `simb` / `SimB`)
négocient les extensions sur chaque connexion comme le fera ArchRust, C (`gmtest` / `GmTest`) se comporte
comme le client d'origine et doit être MJ (`archcore db-init --account gmtest gmtest 3`). Le test vérifie :

- le chuchotement d'AddOn de A à B (émetteur rempli par le serveur, charge intacte, aucun écho chez A) ;
- le message `SAY`, reçu par A et B ;
- chaque refus : destinataire sans extensions, destinataire absent, préfixe réservé `AC.`, préfixe
  invalide, charge de 256 octets, canal de groupe non géré, joueur sous `/chatban` ;
- qu'un message d'AddOn envoyé sans négociation est ignoré ;
- la limite de débit : sur une rafale de 25 messages, 20 passent et 5 sont refusés ;
- que C ne reçoit jamais de paquet `0xAC`.

Le protocole est décrit sur la page [[Extensions ArchRust|FR-Extensions-ArchRust]].

## Tables du serveur d'origine

Les apparitions de monstres et la carte serveur ne sont pas dans le client : elles viennent des tables du
serveur d'origine d'ArchLord, à placer dans `data/server-ini` (jamais versionné) ou à désigner avec
`--server` :

| Fichier | Contenu | Table |
|---|---|---|
| `spawndatatable.txt` | Quoi : groupe, modèle et niveau du monstre, nombre, délai de réapparition, guerre de siège | `monster_spawn` |
| `spawninstancetable.txt` | Où : position, orientation, rayon de chaque point (un nom peut avoir plusieurs positions) | `monster_spawn` |
| `world/node_data` (à côté ou dans le dossier voisin) | Carte : 16 × 16 segments de 400 unités par secteur, sol infranchissable et région | `map_segment` |
| `itemdroptable.txt` | Objets propres à chaque monstre : quantité, taux sur 100 000 | `monster_drop` |
| `groupdroprate.txt` | Groupes de butin : taux, sensibilité à `DropMeditation`, bonus par niveau | `drop_group` |
| `droprankrate.txt` | Rangs d'objets donnés selon le rang de butin du monstre | `drop_rank_rate` |

```bash
archcore db-init
archcore import data/client-ini --server chemin/vers/ini
```

Les lignes de `spawndatatable.txt` sans position dans `spawninstancetable.txt` sont ignorées et comptées.

## Tests unitaires

```bash
cargo test
```

## Rétro-ingénierie

Les relevés ont été faits avec [REA](https://github.com/morluto/rea) et Ghidra 12. La démarche est
décrite sur la page [[Rétro-ingénierie|FR-Retro-ingenierie]].
