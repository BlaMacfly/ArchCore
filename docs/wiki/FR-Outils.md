# Outils

Tous les outils sont des sous-commandes de l'exécutable `archcore` (voir [[Installation|FR-Installation]]).
Ils se lancent depuis la racine du dépôt et lisent `config/auth.conf` et `config/world.conf`, ou ceux du
dossier désigné par `ARCHCORE_CONF_DIR` (voir [[Configuration|FR-Configuration]]).

| Commande | Rôle |
|---|---|
| `archcore config migrate <archcore.json> [dossier] [--force]` | Convertit une ancienne configuration JSON en `auth.conf` et `world.conf` |
| `archcore db-init [--account nom mdp [mj]]` | Crée les bases, rejoue la base de référence et applique les migrations manquantes ; avec `--account`, crée ou met à jour un compte |
| `archcore db-check` | Contrôle de cohérence, en lecture seule : versions du schéma, références entre bases, plages d'identifiants ; se termine en erreur sur une anomalie grave (voir [[Base de données\|FR-Base-de-donnees]]) |
| `archcore decrypt [source] [destination]` | Déchiffre `Game/ini` (RC4, MD5(« 1111 »)) vers `data/client-ini` |
| `archcore import [dossier] [--server dossier]` | Importe dans `archcore_world` les valeurs de l'épisode 8 (tables du serveur d'origine, `--server` ou `data/server-ini`, exigées) avec les noms, visuels et tables purement clientes de `data/client-ini` (CP949 → UTF-8, voir « Valeurs de l'épisode 8 » plus bas), la hauteur du sol depuis `world/b*x.ma2` du dossier Game (`map_height`, quelques minutes), les apparitions de monstres, le butin et la carte serveur ; applique enfin la couche de contenu `content/*.sql`, note chaque source dans `import_run` et affiche le résumé des lignes gardées et écartées par table |
| `archcore export-client-tables <dossier> [--game dossier]` | Écrit depuis la base monde les tables de valeurs au format du client, pour ArchRust (voir « Export pour le client ») |
| `archcore migrate-ep8 [--apply \| --restore n]` | Rapport, puis migration des personnages existants vers les valeurs de l'épisode 8 (voir « Migration des personnages ») |
| `archcore content` | Réapplique la couche de contenu `content/*.sql` à la base monde, sans réimporter (voir [[Base de données\|FR-Base-de-donnees]]) |
| `archcore sim client\|move\|shop\|addon\|lang\|combat\|equip\|potion\|scroll\|patrol\|bag\|skill\|effects\|bank\|portal\|guild\|guildlead\|guildmark\|guildbattle\|guildrelation\|guildwarehouse\|guildstage\|gm\|trade\|quest\|mobskill\|counter\|partyskill\|mastery …` | Clients simulés (voir plus bas) |
| `archcore i18n strings [Game]` | Liste les textes encore en anglais dans `data/i18n/pending-en.json` |
| `archcore i18n coverage [Game]` | Mesure la part déjà traduite de `ini_fr` |
| `archcore i18n apply [--ascii] [--dry]` | Applique la mémoire de traduction au client (voir [[Traduction|FR-Traduction]]) |
| `archcore i18n db [langue]` | Remplit `localized_text` (base monde) pour une langue : titres de PNJ traduits et mémoire `data/i18n/<langue>.json` (voir [[Traduction|FR-Traduction]]) |
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

Le serveur de test a ses trois bases à lui, monde compris : sinon `archcore db-init` lancé pour lui
appliquerait aussi les migrations du monde au serveur principal. Pour créer sa base monde, copier celle du
serveur principal (`CREATE DATABASE archcore_test_world`, puis
`mariadb-dump archcore_world | mariadb archcore_test_world`), la déclarer dans `Database.World`, donner aussi une base de journaux à part (`Database.Logs`, par exemple
`archcore_test_logs`, créée par `db-init`), et lancer `ARCHCORE_CONF_DIR=config/test archcore db-init`.

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
guerrier de niveau 1 sans arme frappe un Ver vert (136 PV) 2 à 3 PV par coup, et le ver le tue (120 PV)
en une vingtaine de secondes.

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
ARCHCORE_CONF_DIR=config/test archcore sim portal [compte] [mdp] [personnage]
ARCHCORE_CONF_DIR=config/test archcore sim trade [compteA mdpA persoA compteB mdpB persoB]
ARCHCORE_CONF_DIR=config/test archcore sim party [compteA mdpA persoA compteB mdpB persoB]
ARCHCORE_CONF_DIR=config/test archcore sim guild [compteA mdpA persoA compteB mdpB persoB]
ARCHCORE_CONF_DIR=config/test archcore sim gm [compte] [mdp] [personnage]
ARCHCORE_CONF_DIR=config/test archcore sim refusal [compteA mdpA persoA compteB mdpB persoB]
```

`sim bank` (compte MJ `gmtest`) vérifie le refus d'un dépôt sans gardien, ouvre la banque chez Holly (au
sud d'Anchorville), achète la deuxième page (1 000 000 pièces) et se voit refuser la suivante, y dépose une
pile de potions et 1000 pièces, se reconnecte et les retrouve, puis les retire et détruit la pile.
`sim portal` (compte MJ `gmtest`) se place à 2500 unités du portail d'Anchorville et s'y adresse : le
serveur le fait courir puis donne l'accord ; trajet payant (100 pièces) vers le Repaire de l'éclaireur, refus
d'un trajet non relié, retour payant (500 pièces) à Anchorville. `sim trade` place GmTest (MJ) et SimA au même endroit : GmTest propose une pile
de potions (refusée sur la grille Ex, réservée au Marvel Scroll of Release, puis posée sur la case visée 0,1,2) et 500 pièces, SimA 100 pièces ;
demandeur lu au champ 2 de l'op 3 ; l'objet proposé ne peut pas être détruit ; verrous (ops 16 / 18 avec le
CID), déverrouillage de GmTest seul (le verrou de SimA reste, son « prêt » attend), échange (SimA reçoit la
pile, l'argent est ajusté des deux côtés) ; lettres liées (735, 736) : refusées sans parchemin, Marvel Scroll of Release
(11256) posé sur la grille Ex (op 1 puis op 0 statut 14) et rendu à l'annulation (statut 51), une seule lettre liée
par échange, échangée avec le parchemin, consommé ; puis une seconde demande refusée par l'op 5. `sim party`
(SimA et SimB) vérifie le refus d'une invitation à un nom inconnu, l'invitation et l'acceptation (groupe et
liste des membres chez les deux), la discussion de groupe, le réglage de l'expérience par le chef (refusé à
l'autre), le passage de chef puis l'exclusion, qui dissout le groupe. `sim guild` (GmTest chef, SimA) refuse
un nom à espace, crée SimGuild (10 000 pièces), essuie le refus de SimA qui bloque les invitations de guilde (message système
42), invite SimA qui accepte, vérifie la discussion de guilde et
l'annonce, la demande de départ acceptée par le chef, le retour puis l'exclusion, la guilde retrouvée à la
reconnexion, enfin la dissolution (mauvais mot de passe refusé). `sim refusal` (GmTest, SimA) coche chez GmTest les refus
d'échange et de groupe (op 0x11) : SimA reçoit l'op 14 à son invitation et l'op 25 à sa demande d'échange, GmTest
ne reçoit rien. `sim gm` (compte MJ `gmtest`) vérifie `/speed` (course ×5
dans les facteurs reçus, refus au-delà de 10, retour à ×1) et `/additem` (potions ajoutées, TID inconnu
refusé), puis détruit la pile.

## Test des guildes

```bash
ARCHCORE_CONF_DIR=config/test archcore sim guildlead [compteA mdpA persoA compteB mdpB persoB]
ARCHCORE_CONF_DIR=config/test archcore sim guildmark [compteA mdpA persoA compteB mdpB persoB]
ARCHCORE_CONF_DIR=config/test archcore sim guildbattle [compteA mdpA persoA compteB mdpB persoB]
ARCHCORE_CONF_DIR=config/test archcore sim guildrelation [compteA mdpA persoA compteB mdpB persoB]
ARCHCORE_CONF_DIR=config/test archcore sim guildwarehouse [compteA mdpA persoA compteB mdpB persoB]
ARCHCORE_CONF_DIR=config/test archcore sim guildstage setup|present|chef|dissoudre|guerre-prep|guerre-defi|guerre-refus|relation-prep|relation-repond|relation-demande|entrepot-prep [secondes]
```

Ces scénarios (`tools/sim/guilds.rs`) créent au besoin des comptes d'appoint `simg01`… (mot de passe identique
au nom) dans la base de test, pour atteindre les effectifs qu'exige Archonia. `sim guildlead` (GmTest chef,
SimA, 18 comptes d'appoint : 20 membres) vérifie la liste des guildes (page, recherche, guilde introuvable :
code 47), l'agrandissement (or insuffisant : code 44, puis 60 places pour 1 000 000 pièces), la nomination de
SimA refusée au niveau 1 puis acceptée (op 48, sous-op 2 → 3 → 4 → 8), le retrait de ce rang, la passation
(5 → 6 → 7 → 8 : GmTest rang 1, SimA chef), les rangs relus à la reconnexion et la dissolution par le nouveau
chef. `sim guildmark` (GmTest, SimA, 9 comptes d'appoint) vérifie l'emblème : rien sous 10 membres, fond
manquant (code 58), or insuffisant (code 57), achat (60 000 pièces) annoncé à la guilde et dans les liens,
une seconde guilde qui demande le même emblème (code 59) puis l'achète malgré tout (op 33), l'emblème relu à
la reconnexion. `sim guildbattle` (GmTest et SimA, chacun chef d'une guilde de 10 membres ; le serveur d'essai
doit avoir un `Guild.BattleReadySeconds` court, 10 par exemple) vérifie les combats de guildes : guilde inconnue
(code 27), niveaux insuffisants (code 22, 100 en deuxième nombre), défi refusé (op 15, code 28, puis op 17,
code 30), défi accepté (op 16), annulation trop tardive (code 31), aucun coup avant le début, début (op 21),
horloge (op 22), un adversaire tué (op 23 : 1 point), abandon (op 24 → op 25 : résultats 4 et 5, bilan op 12),
annulation en plein combat (op 18 → 19), victoire par la mort du chef adverse (résultats 1 et 3), bilan relu à
la reconnexion et refus des combats par option (code 43). `sim guildrelation` (GmTest et SimA, 38 comptes
d'appoint) vérifie les alliances et les hostilités : guilde inconnue (0x3c), effectif insuffisant (0x42),
demande refusée (op 34 → 35, 0x44) puis acceptée (op 36 des deux côtés, rôles 2 et 1, détails op 44), déjà
alliées (0x3e), hostilité envers une alliée (0x4b), départ (op 37 : 0x47, et 0x49 pour l'autre, l'alliance
étant dissoute), hostilité refusée (op 38 → 39, 0x52) puis acceptée (op 40, détails op 45), déjà hostiles (0x4c),
trêve refusée (op 41 → 39, 0x57) puis acceptée (op 43, 0x59), hostilité relue à la reconnexion et retirée par
la dissolution. `sim guildwarehouse` (GmTest chef, SimA, 8 comptes d'appoint) vérifie l'entrepôt de guilde chez
Holly : accord du gardien (fonction 35, op 3), refus sous 10 membres (0x5f), ouverture (op 0, op 1 résultat 2),
dépôt d'une pile (op 5 → statut 53, case au champ 7) et de 1000 pièces (op 3), droits d'un membre (pas de
retrait), dépôt d'argent d'un membre vu des deux, dissolution refusée (0x60), relecture à la reconnexion, reprise
de la pile (op 6, partie chez l'autre) et de l'argent (op 4), agrandissement refusé (résultat 4). `sim guildstage` prépare les captures d'ArchRust : `setup` crée SimLead (GmTest chef, SimA
niveau 20, 18 comptes d'appoint) et laisse la guilde en place ; `present` garde SimA en jeu, qui accepte
nominations et passations ; `chef` fait entrer GmTest, qui propose à SimA d'être second puis de diriger la
guilde ; `dissoudre` dissout SimLead (et SimWarB) ; `guerre-prep` crée SimLead (GmTest) et SimWarB (SimA), 10 membres
chacune ; `guerre-defi` fait défier SimWarB par GmTest après `secondes`, attendre le début puis abandonner ;
`guerre-refus` fait refuser par GmTest les défis reçus pendant `secondes` ; `relation-prep` comme `guerre-prep`
avec 20 membres par guilde ; `relation-repond` fait accepter par GmTest alliances, hostilités et trêves
pendant `secondes` ; `relation-demande` fait déclarer SimWarB hostile par GmTest après `secondes` ; `entrepot-prep` (après `guerre-prep`)
emmène SimA chez Holly, qui dépose une pile et 1000 pièces dans l'entrepôt de SimWarB et garde une autre pile.

## Test des zones de combat, de la reconnexion et de /last_refine

```bash
ARCHCORE_CONF_DIR=config/test archcore sim pvp [compte] [mdp] [personnage]
ARCHCORE_CONF_DIR=config/test archcore sim reconnect [compteA mdpA persoA compteB mdpB persoB]
ARCHCORE_CONF_DIR=config/test archcore sim lastrefine [compte] [mdp] [personnage]
```

`sim pvp` (compte MJ `gmtest`) frappe un Ver vert au nord d'Anchorville puis se place au portail de la cité de
Tullan : sa demande est ignorée en combat, accordée dix secondes après le dernier coup ; il part pour l'arène
de guilde (zone de combat entre joueurs), d'où « Retour » (op 7) le ramène au portail de Tullan ; l'op 7 est
refusée depuis un portail ordinaire. `sim reconnect` (GmTest, SimA) : clé reçue à l'entrée, coupure de
GmTest sans départ du monde (SimA le voit toujours), reprise refusée avec une mauvaise clé, acceptée avec la
bonne (nouvelle clé, vue renvoyée, commande sur la nouvelle connexion), puis seconde coupure : GmTest quitte
le monde au bout de 60 s et la reprise est refusée (environ 70 s). `sim lastrefine` (GmTest) vérifie le refus
de `/last_refine` avant tout raffinage, raffine la recette 4013, la relance par `/last_refine`, puis obtient
le résultat 1 (ingrédients manquants).

## Test de la production

```bash
ARCHCORE_CONF_DIR=config/test archcore sim product [compte] [mdp] [personnage]
```

Compte MJ (`gmtest`, GmTest) : compétences et recettes de production effacées en base, op 1 vide à l'entrée,
niveau 20, or, couteau de récolte et ustensiles de cuisine (`/additem`). Chez Kauman (Adventurer d'Anchorville) :
Cooking refusée (code 1), Collection Skill apprise (300 or), puis déjà apprise (code 2). Au nord du village, des
Vers verts tués puis récoltés (lancement accepté, op 2 → op 3 : objet de l'ensemble 46, expérience dans le facteur
« dirt » 6) jusqu'à une réussite ; le premier cadavre n'est pas récolté deux fois (code 7, lancement refusé par
l'action 17). Chez Lucy (Cook) : recette refusée sans Cooking (code 5), Cooking apprise, Broiled Meat of Vitality
achetée puis déjà connue, potion d'alchimie refusée ; fabrication sans matériaux (code 6), puis avec matériaux et
assaisonnement (consommés) jusqu'à une réussite. Reconnexion : op 1 rend la recette. Tout est effacé à la fin.

## Test des amis et du courrier

```bash
ARCHCORE_CONF_DIR=config/test archcore sim buddy [compteA mdpA persoA compteB mdpB persoB]
ARCHCORE_CONF_DIR=config/test archcore sim mail [compteA mdpA persoA compteB mdpB persoB]
```

`sim buddy` (GmTest MJ, SimA ; listes vidées en base au début et à la fin) : listes vides à l'entrée (op 9), fichier
local d'ignorés envoyé par GmTest (op 15 : SimB), refus du serveur (soi-même code 4, nom inconnu code 2, relation
« Buddy » code 12, option « refus d'ami » de SimA : op 3 aussitôt), demande acceptée (op 1 des deux côtés, en ligne),
doublon (code 1), région de l'ami (op 7) ; SimA se reconnecte (op 6 hors ligne puis en ligne, liste relue de la base),
retire GmTest (chez lui seulement) et l'ignore avec « Block Chat » et « Block Trade » : chuchotement non transmis,
échange refusé (op 25) ; invitation de groupe reçue, puis refusée (op 14) avec « Block Invites » ; cases décochées,
le chuchotement passe ; GmTest revient et retrouve son ami et l'ignoré SimB. `sim mail` (GmTest MJ, SimA ;
courriers effacés) : GmTest reçoit 3 feuilles de papier à lettre et 5 potions, se voit refuser un destinataire
inconnu (code 2), lui-même (code 1) et le papier joint (code 5), écrit à SimA hors ligne avec les potions (retirées,
une feuille décomptée) ; SimA entre et reçoit le courrier (op 0), le lit (noté en base), ne peut pas le supprimer
avant de récupérer la pièce (code 11), récupère les potions (même objet) puis le supprime ; une seconde lettre lui
arrive aussitôt, SimA en jeu.

## Test de la réparation et des montures

```bash
ARCHCORE_CONF_DIR=config/test archcore sim repair [compte] [mdp] [perso]
ARCHCORE_CONF_DIR=config/test archcore sim ride [compteA mdpA persoA compteB mdpB persoB]
```

`sim repair` (GmTest MJ ; à lancer avec `Item.WeaponWearChance = 100` dans la configuration du serveur de test) :
GmTest porte une Short Sword (DUR 88) et une Halfsteel Armor (DUR 63), frappe un Ver vert et voit l'épée perdre un
point par coup (AGPMITEM op 2, champ 10 : 87, 86, 85) ; `/durability 0` : l'épée cassée ne compte plus dans les
dégâts ; `/durability 40`, objets rangés, reconnexion : 40 relu à l'entrée en jeu ; chez Halo, ouverture (op 3 →
op 4), coût faux (code 4), réparation (argent débité du prix du client, objets pleins, code 1), objets déjà pleins
(code 3), potion (code 5). `sim ride` (SimDepart sur le compte MJ, SimA témoin) : niveau 40, White Horse (TID 3002) ;
monter (objet porté, course 10 500, AgpmRide op 2 chez les deux), reconnexion à cheval (champ 18 = 2, op 2 à
l'entrée), descendre (op 4, course d'origine), montée refusée juste après un coup sur un Ver vert (code 24, cheval
resté dans l'inventaire) puis acceptée 11 s plus tard, mort à cheval (`/kill`) : descente forcée.

## Test de la métamorphose et de l'évolution du Scion

```bash
ARCHCORE_CONF_DIR=config/test archcore sim morph [compteA mdpA persoA compteB mdpB persoB]
ARCHCORE_SIM_CREATE_TID=1724 ARCHCORE_CONF_DIR=config/test archcore sim client test test SimScion   # une fois
ARCHCORE_CONF_DIR=config/test archcore sim evolution [compteA mdpA persoA compteB mdpB persoB]
```

`sim morph` (GmTest MJ, SimA témoin ; à lancer avec `Item.TransformMaxDurationMs = 8000`) : trois Spider Potions
(forme 272, dégâts +26 / +28) et un Passport ; potion bue (op 4, op 19 à 600 000 ms, dégâts majorés, op 11 chez les
deux), seconde potion refusée (op 27 code 4), Passport refusé ; SimA revient en jeu et reçoit GmTest par l'op 0 (TID
d'origine, bit 0) puis l'op 11 ; fin de durée au bout de 8 s (op 12 chez les deux, dégâts d'origine) ; GmTest revient,
boit une potion et l'annule (op 13 → op 12). Sous la forme d'araignée, une Short Sword se porte puis se range (AGPMITEM
op 2, statuts 3 et 2) et l'échange demandé à SimA est refusé (op 5, rien chez SimA). Enfin, Mutation niveau 1 posée en
base, GmTest la lance sur un Ver vert : op 11 (forme de la liste du niveau), aucun coup sur l'animal, op 12 au bout de
7 s, seconde Mutation sans effet pendant l'immunité. `sim evolution` (GmTest MJ, SimScion Scion du compte `test`, posé chez
Gilliwen, sans forme ni compétence d'évolution au départ) : niveau 20 et or par le MJ, Slayer et Orbiter of Evolution
apprises, Power Stunner refusée (code 5) ; Slayer of Evolution (facteurs du Slayer, FOR 83, op 4, op 26 → 1722 chez
les deux), Orbiter of Evolution (op 27 → 1724 puis op 26 → 1723), forme active relancée refusée, reconnexion (TID
1723, champ 18 = 4, op 4 à l'entrée, vue du MJ), annulation par AGPMSKILL op 7 action 4 (op 5, op 27 → 1724, forme
effacée en base).

## Test de la maison des ventes, du commutateur et de la météo

```bash
ARCHCORE_CONF_DIR=config/test archcore sim auction [compteA mdpA persoA compteB mdpB persoB]
ARCHCORE_CONF_DIR=config/test archcore sim itemset [compte] [mdp] [perso]
ARCHCORE_CONF_DIR=config/test archcore sim nature [compteA mdpA persoA compteB mdpB persoB]
```

`sim auction` (GmTest MJ vendeur, SimA acheteuse ; ventes effacées, or et CC fixés en base) : chez Fotherbee
(« Item Trader »), ouverture (op 9 → op 10) ; refus d'une épée sans catégorie, d'un prix sous 100 000 et d'une pile
partielle ; 5 potions mises en vente pour 150 000 or et 20 CC (op 4, op 1, potions retirées, 4 500 d'or de frais) ;
SimA les trouve par le TID (op 12, ligne du rowset), se voit refuser une mauvaise quantité (code 5), achète (même
IID chez elle, or et CC débités, op 7) ; GmTest reçoit l'op 3 (vendu), un second achat donne le code 12 ; GmTest
encaisse (op 6, or et CC crédités, op 2) ; une seconde vente revient à l'entrée en jeu (op 1) et s'annule (potions
rendues, op 5, op 2). `sim itemset` (GmTest) : champ 26 = IID ; épée et armure portées, enregistrées dans
l'emplacement 3 (op 3, op 5, pas d'op 4), rangées puis rééquipées par l'op 1 (op 4), déplacement (op 6) et retrait
(op 7) d'un objet du jeu, emplacement vide porté (tout est retiré), relecture à l'entrée (ops 3, 5 puis 4), emplacement
vidé (op 2). `sim nature` : GmTest puis SimA 3 s plus tard reçoivent la même horloge (écart réel × vitesse), temps
clair à l'entrée ; `/weather rain` reçu par les deux et à la reconnexion de SimA, `/weather` décrit la météo,
`/weather clear` reçu par les deux.

## Test des invocations

```bash
ARCHCORE_CONF_DIR=config/test archcore sim summons [compte] [mdp] [perso]
```

GmEffets (compte MJ) reçoit en base Griolli (TID 315) et Bomba (619), passe niveau 80, invincible, chez les Vers
verts. Griolli : vue de la créature (TID 543), état 0x800 puis AgpmSummons op 0, dans cet ordre (type 1, horodatages
non nuls). Attaque d'un ver éloigné : l'esprit (portée 1500) le tue avant l'arrivée du maître, l'expérience revient au
maître. Propension « défense » (op 2) renvoyée ; refusée pour un monstre ordinaire. `/move` : op 1 puis retrait de la
vue. Bomba (Big Tim, 10 s) : op 1 à l'expiration. Griolli puis Bomba : la limite d'une invocation renvoie Griolli
(op 1) avant Big Tim (op 0). Doogie <Rank 1> (objet 7564) : familier (type 8, forcé), renvoyé en réutilisant l'objet.
Niveau rétabli, compétences et objet retirés de la base à la fin.

## Test des consommables, de l'orbe de résurrection et de l'oubli des compétences

```bash
ARCHCORE_CONF_DIR=config/test archcore sim consumables [compte] [mdp] [perso]
```

GmTest (compte MJ) part avec Dual Slash au niveau 2 seulement et une chaîne « skill init » vide. Scorching Stew (TID 111) :
effet 459 (AGPMSKILL op 4, 300 s), op 4 de l'objet avec la recharge de la compétence (420 000 ms), second ragoût refusé
(op 5). Janus Talisman of Growth LV1 (50101) : effet 1801 de 30 min, talisman consommé. Reverse Orb (1054) : refusé
vivant (op 5) ; après `/kill`, résurrection sur place (op 1 statut 0, op 4 puis op 18 de 1 800 000 ms) ; mort de
nouveau, second orbe refusé pendant la recharge (op 18 puis op 27 code 3), retour au village (op 9). Maître (0x3B op 8
sans événement) : Dual Slash retirée (AGPMSKILL op 1), 560 pièces rendues, op 23 « 1 », op 9 résultat 0 ; seconde
demande, puis une autre après reconnexion (chaîne relue en base) : résultat 1. Gem of origin (4876) : Dual Slash, remise
en base, retirée (op 1), op 4. Compétences, argent et chaîne de GmTest rétablis à la fin.

## Test des objets sans compétence (Return Orb, Secret Safe, commandes, feux, objets allumés)

```bash
ARCHCORE_CONF_DIR=config/test archcore sim usables [compteA mdpA persoA compteB mdpB persoB]
```

GmTest (MJ) part avec Dual Slash au niveau 2, sans page de banque en plus et une Bastard Sword +1 à deux sockets sertis ;
SimA est posé à côté. Return Orb (1171) : AGPMSKILL op 13 sur l'attaque normale refusé, sur Dual Slash → op 1, orbe
consommée (op 4), puis refus sans orbe. Secret Safe (3916) refusé sans deuxième page ; `/safekeeper` (4923) ouvre la
banque à distance (AGPMEVENTBANK op 1), la page 2 s'y achète (op 22) ; deux Secret Safe ouvrent les pages 3 et 4
(champ 23 = 2, 3, message 30), le troisième est refusé (message 29). `/merchant` (4924) : liste d'une boutique. Release
of Agreement (4877) : AGPMITEMCONVERT op 12 → op 0 (+1, 2 sockets, rien de serti), objet consommé ; second essai sans
effet. Small firecracker (4606) : op 4 chez GmTest et chez SimA, second refusé (op 5). Fortune Manual (4569) et
Talisman of Protection (1842) : op 26 allumé puis éteint (op 3, op 23). `/nick` : 17 caractères refusés, « Héros » vu
des deux (champ 27) et en base, « Il faut » sans objet. `/go` : joueur absent refusé, GmTest éloigné rejoint SimA,
recharge. Compétences, argent, pages de banque et signature de GmTest rétablis à la fin.

## Test du rappel de groupe, des actions sociales, de la barre de raccourcis et du retour à la sélection

```bash
ARCHCORE_CONF_DIR=config/test archcore sim recall|social [compteA mdpA persoA compteB mdpB persoB]
ARCHCORE_CONF_DIR=config/test archcore sim quickbelt|returnlogin [compte] [mdp] [perso]
```

`sim recall`, GmTest (compte MJ) et SimA : Bead of Summoning (2756) sans groupe refusé (AGPMITEM op 5) ; groupe formé,
GmTest s'éloigne (`/move`) et rappelle : op 4 au lanceur, gestion op 15 (lanceur, nom) à SimA, qui accepte (0x07 op 5
puis op 16) et arrive à côté de GmTest, op 16 au lanceur ; second rappel refusé (op 17 au lanceur) ; acceptation sans
demande ignorée ; Summoning Orb (1174) : op 4 avec 300 000 ms, second usage refusé (op 5). `sim social` : SimA danse
(op 16 type 5 reçue par les deux), type 17 ignoré, action trop rapprochée ignorée puis permise. `sim quickbelt`, GmTest :
op 0 de 50 cases, op 1 sans réponse (compétence déplacée dans sa rangée, potion, objet inconnu vidé, potion de vie),
barre relue à la reconnexion (compétence par son nouvel identifiant), potion disparue remplacée, case vidée ; barre de
GmTest effacée au début et à la fin. `sim returnlogin`, GmTest : 0x2F op 1 → op 5 et fermeture du monde ; au serveur de
connexion op 6, clé de hachage, connexion, op 7, liste des personnages ; même clé refusée (AGPMLOGIN op 15) ; nouvelle
entrée en jeu.

## Test des titres

```bash
ARCHCORE_CONF_DIR=config/test archcore sim title [compteA mdpA persoA compteB mdpB persoB]
```

GmTest (compte MJ, chevalier humain) et SimA. Entrée sans titre : op 5 unique (0x5C = 0). Warrante (« <Title
Manager> », npc.ini 535) : op 10 au lieu du dialogue. Novice Human Knight (140001) visé au niveau 10 : 6.1 résultat 0 ;
au niveau 15 : 6.1 puis 6.3 = 15 ; au niveau 16, 6.3 = 16 ; 6.4 → 6.5 puis op 1. Équipé : op 3 (1, 1), op 9 à GmTest et
à SimA posé à côté, force + 2 (« <Title> Novice Knight ») ; SimA éloigné puis ramené reçoit de nouveau l'op 9. Retrait
aussitôt après refusé (op 3 résultat 0, code 3). Suppression du titre actif : op 9 bUse 0 chez SimA, op 8 ; objectif
10001 fixé puis abandonné (op 8) ; `/title` sans titre actif. `isole.sh` vide `character_titles`,
`character_title_goals` et `characters.title_changed_at`.

## Test de la discussion, des salons, des coiffeurs et des avis de recherche

```bash
ARCHCORE_CONF_DIR=config/test archcore sim chat|channel|customize|wanted [compteA mdpA persoA compteB mdpB persoB]
```

GmTest (compte MJ) et SimA. `sim chat` : SimA envoie comme Archonia « /p » et « /g » en type 0 (refus du groupe et
de la guilde, pas « Commande inconnue »), « /s » (type 1 reçu par GmTest), une commande inconnue (aucune réponse),
« /race » sans trompette (refus) puis avec deux Race Trumpet données par le MJ (AgpmAreaChatting type 19
« [Race Trumpet] [SimA] … » chez les deux, une trompette consommée ; second cri dans la seconde refusé ; le suivant
consomme la dernière). `sim channel` : salon public (création, nom pris, entrée, déjà membre, message relayé aux
deux), salon privé (op 8, mauvais mot de passe, entrée), sortie, invitation sans mot de passe, recherche d'un joueur,
salon inconnu, salon vide disparu. `sim customize` : chez Gladiol, ouverture (op 0 → op 1), coiffure 4 refusée faute
d'or puis achetée (or débité, champ 21 chez GmTest et chez SimA), même coiffure refusée, coiffure 1 remise.
`sim wanted` : GmTest tue SimA (op 6), mise à prix refusée sous la prime de base puis acceptée (op 7 et op 5 chez
GmTest), seconde refusée, tableau chez Rembrandt (op 4 puis fin de page), SimA niveau 60 tue GmTest : prime touchée
(op 8 genre 4), op 8 genre 5 et op 7 drapeau 0 chez GmTest ; niveaux rétablis. Les avis (`wanted_criminal`,
`wanted_bounty`) restent en base : à vider entre deux passes.

## Test des quêtes

```bash
ARCHCORE_CONF_DIR=config/test archcore sim quest [compte] [mdp] [personnage]
```

Le compte MJ (`gmtest`), passé au niveau 120 (`/questreset` remet d'abord les quêtes du test à zéro), prend
Shattered Roots depuis le journal, se voit refuser de la terminer sans les 50 Corrupted Root Cores, les
reçoit (`/create`), la termine (objets repris, Tempering Gem reçue), se voit refuser de la reprendre et de
prendre Winged Menace (niveau 25 à 40), prend puis abandonne Gladiator's Demise ; à la reconnexion,
Shattered Roots figure parmi les quêtes terminées et la quête abandonnée n'est plus au journal.

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
autres tests), passe au niveau 25 (au moins 456 PV et 108 PM, d'après GrowUpFactor) et apprend chez
Black Speed Attack, Defense Hold, Linear Blow et Blade Sting (si besoin). Il vérifie la passive (défense +20), le renforcement (effet annoncé sur lui, vitesse
d'attaque +18 %), la zone de Linear Blow (cible et voisins touchés) et l'affaiblissement de Blade Sting
(posé sur le Ver vert, retiré au bout de 6 s). `/level` rend les PM entre deux lancements.

```bash
ARCHCORE_CONF_DIR=config/test archcore sim status [compte] [mdp] [personnage]
```

Le même personnage (`GmEffets`, niveau 25) apprend Shield Bash chez Black si besoin, puis la lance sur
des Vers verts, avec `/recast` et `/level` entre deux essais, jusqu'à un étourdissement (30 % de
chances, 25 essais au plus) : le ver passe à l'état 0x1 (AGPMCHARACTER op 1, champ 17), puis revient à 0
au bout de 3 s.

```bash
ARCHCORE_CONF_DIR=config/test archcore sim mastery [compte] [mdp] [personnage]
ARCHCORE_CONF_DIR=config/test archcore sim partyskill [compteA mdpA persoA compteB mdpB persoB]
ARCHCORE_CONF_DIR=config/test archcore sim counter [compte] [mdp] [personnage]
ARCHCORE_CONF_DIR=config/test archcore sim mobskill [compte] [mdp] [personnage]
ARCHCORE_CONF_DIR=config/test archcore sim convert [compte] [mdp] [personnage]
ARCHCORE_CONF_DIR=config/test archcore sim refinery [compte] [mdp] [personnage]
```

`sim mastery` (GmTest, niveau 60) oublie en base Capacitate et Marching Song, se voit refuser Capacitate chez
Black (« il faut Marching Song au niveau 4 »), apprend Marching Song, l'améliore trois fois, puis apprend
Capacitate. `sim partyskill` (GmTest, qui reçoit Marching Song en base, et SimA) forme un groupe : Marching
Song renforce SimA à côté de GmTest (AGPMSKILL op 4 sur SimA), pas quand SimA est à 3000 unités. `sim counter`
pose en base Counter Attack niveau 10 sur GmTest (compétence de berserker, retirée à la fin), le passe au
niveau 1, provoque un Ver vert et attend un coup renvoyé (attaquant = joueur, résultat 12), `/level 1` rendant
les PV au besoin. `sim mobskill` (GmTest, niveau 40) rejoint Midas, le frappe une fois et attend qu'il lance
Insanity (op 7 du monstre, op 8 sur le joueur avec des dégâts physiques et d'eau), puis la relance
6 s plus tard. `sim convert` (GmTest, compte MJ) ouvre la fenêtre du forgeron Halo à Anchorville, reçoit une
Bastard Sword (avec 1 à 3 options et ses sockets tirés), une pierre d'eau et deux potions de renforcement
(`/additem`), ajoute un socket (répété jusqu'à la
réussite), sertit la pierre (op 7, résultat 0) puis une pierre de prière d'attaque (rune, op 8, résultat 1), renforce l'épée à +1 puis +2, se reconnecte et relit l'état
(+2, sockets, pierre et rune) dans les objets reçus à l'entrée, puis détruit l'épée. `sim refinery` (GmTest, compte MJ) ouvre une sphère d'alchimie, voit refuser une
potion seule (résultat 2), obtient le devis puis raffine 100 Breath Elemental Stone en 10 Gale Elemental Stone
(recette 4013, 5000 pièces), refait la recette par son identifiant (op 6), est refusé sans ingrédients
(résultat 1), puis raffine une White Wing Dagger et ses pierres (recette 183) en dague à une option.

```bash
ARCHCORE_CONF_DIR=config/test archcore sim heroic [compte] [mdp] [personnage]
ARCHCORE_CONF_DIR=config/test archcore sim divide [compteA mdpA persoA compteB mdpB persoB]
ARCHCORE_CONF_DIR=config/test archcore sim dispel [compte] [mdp] [personnage]
ARCHCORE_CONF_DIR=config/test archcore sim ignore [compte] [mdp] [personnage]
ARCHCORE_CONF_DIR=config/test archcore sim rune [compte] [mdp] [personnage]
ARCHCORE_CONF_DIR=config/test archcore sim trigger [compte] [mdp] [personnage]
```

Compétences et objets (`tools/sim/competences.rs`, GmTest, compte MJ ; compétences et objets posés en base puis
retirés). `sim heroic` passe aux niveaux 89, 100 et 120 et lit les points héroïques (facteurs « dirt », indice 7 :
0, 21, 71), gagne 300 points de charisme par `/charisma 300` (statut 4) et les relit après reconnexion.
`sim divide` donne Martyr's Mark niveau 7 à GmTest, forme un groupe avec SimA, lance la compétence (effet sur les
deux) ; un Ver vert frappe SimA, GmTest prend 40 % du coup (AGPMCHARACTER op 1). `sim dispel` pose Blade Sting
niveau 10 sur un Ver vert, puis Antimagic (niveau 100 pour ses 500 PM) le lui retire (AGPMSKILL op 5) bien avant
ses 33 s. `sim ignore` sertit en base une pierre d'eau dans une Short Sword : contre un ver résistant à 50 %, la
part d'eau d'un coup vaut 3, puis 4 sous Stunner niveau 20 (48 % des résistances ignorées). `sim rune` sertit en
base une pierre de prière de glace (TID 588) et frappe jusqu'au déclenchement de Cold Prayer Stone <Small>
(AGPMSKILL op 8, dégâts de glace, puis op 4 : ralentissement). `sim trigger` donne à l'épée l'option 5658
(28 % d'étourdir 3 s) et frappe jusqu'à un ver étourdi (champ 17), état levé 3 s plus tard.

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

## Test des langues

```bash
ARCHCORE_CONF_DIR=config/test archcore sim lang [compte] [mdp] [personnage]
```

Le même personnage (`sima` / `SimA` par défaut) entre en jeu plusieurs fois avec la capacité `language/1` :

- en français (langue annoncée au `HELLO`) : `HELLO_ACK` accorde `language/1`, applique `fr` et liste les
  langues proposées ; les noms des PNJ reçus sont relevés ;
- en anglais : les noms de PNJ diffèrent (« <Marchand> Darryl » → « <Merchant> Darryl ») ;
- `TEXT_REQUEST` rend les mêmes noms que les paquets de vue, puis `SET_LANGUAGE fr` les fait revenir en
  français ; une langue inconnue (`xx-YY`) retombe sur `World.Language`, un code mal formé et une sorte
  inconnue valent `INVALID_REQUEST` ;
- en « automatique » (HELLO sans langue) : `HELLO_ACK` annonce la langue par défaut, puis un `LANGUAGE_ACK`
  à l'entrée donne la langue du compte (`en`), et les noms sont en anglais ;
- avec le client d'origine (sans `HELLO`) : aucun paquet `0xAC`, noms dans la langue du compte ;
- enfin le compte est remis en français.

Il faut avoir lancé `archcore db-init` (tables `language`, `localized_text`, colonne `account.language`) ;
`archcore i18n db fr` est facultatif, les titres de PNJ traduits servant de repli.

## Valeurs de l'épisode 8

`archcore import` prend les valeurs de jeu dans les tables du serveur d'origine (« tables EP8 ») et dans le
client d'Archonia seulement les noms, les visuels et les tables purement clientes (`tools/ep8.rs`) :

| Règle | Détail |
|---|---|
| Présence | Une ligne de valeurs (compétence, niveau de compétence, objet, option, rune, modèle, recette, région, boutique, point de téléportation) n'existe que si elle existe dans la table EP8 ; une ligne propre à la table EP8 que le client ne connaît pas (38 boutiques, 21 PNJ « Trainer » / « Event », 5 points de téléportation, colonne `NPC Arena Coin`) n'est pas importée |
| Valeurs | Colonnes de la table EP8 ; les colonnes d'Archonia (`CategoryName` des options, `AlwaysSuccess` des runes, `Keep …` du raffinage, `Critical_Defense_Rate`, `Regen_HP` des monstres…) ne sont pas lues : la règle tombe sur la valeur neutre d'origine (objet raffiné neuf, rune selon `Success_Rate`) |
| Noms | Ceux du client, joints par TID (`ItemName`, `Name`, `Description` des options) |
| Compétences | `skilltemplate.ini`, `skillspec.txt`, `skillconst.txt`, `skillconst2.txt` ; la jointure nom → TID se fait avec le modèle EP8 (930 s'y appelle « <Battle>Skill Upgrade », le nom du 524 est mal encodé côté client) ; `skill_template` garde le nom et les icônes du client |
| Expérience | Niveaux 1 à 100 de la table EP8, 101 à 121 du client (seul barème connu au-delà) |
| Croissance | `growupfactor.ini` ; le modèle 1276 (ArchLord), absent de la table EP8, garde la croissance du client |
| PNJ | Fusion de `npc.ini` : nom du client ; modèle, position et événements EP8 (listes Function / EventID parallèles, élément par élément) ; quêtes (fonction 27), réparation (17) et avis de recherche (32) du client, que la table EP8 a retirés ; boutique EP8 si elle est importée, sinon celle du client ; un PNJ déplacé de plus de 1 000 unités hors du terrain du client garde sa position ; PNJ au modèle hors EP8 (2207) et PNJ 814 et 526 écartés. 35 numéros désignent un autre PNJ dans la table EP8 (sapins de Noël, PNJ « Event » à la place des maîtres des titres, des superviseurs de la Battle Square…) : ces PNJ du client gardent leurs valeurs, ceux de la table EP8 ne sont pas importés |
| Tables purement clientes | Quêtes (39 retirées : les 30 qui citent un monstre ou un objet à rapporter hors EP8, et 9 dont la récompense « TID;nombre » est un objet d'Archonia — Shard of Rebirth, Tempering Gem… ; leurs groupes vidés aussi), maîtrises, titres, production, fenêtre des boss (13 boss d'Archonia retirés), sièges : gardées, sans les lignes qui citent un TID hors EP8 |
| Partage des dégâts | Colonne `divide` de la table EP8 (Divine Protection 1, Martyr's Mark 2) : le lanceur prend la part de son groupe ; le bit 16 qu'Archonia y ajoutait n'est plus exigé |
| Régénération des monstres | Pas de colonne d'origine : règle d'ArchCore (soignés en revenant à leur point d'apparition) |

Résumé du dernier import (bases de copie) : 16 973 objets gardés (3 848 écartés), 2 028 modèles (428),
1 825 compétences (923), 10 910 niveaux de compétence (2 042), 5 025 options (1 097), 532 runes (606),
4 541 recettes (1 190), 300 régions (31), 301 points de téléportation (5), 557 PNJ (2 du client, 8 au modèle
d'Archonia ; 56 de la table EP8 non importés), 144 boutiques (39), 14 quêtes (39), 12 boss (13).

## Export pour le client

`archcore export-client-tables <dossier>` écrit, depuis la base monde (couche `content/` comprise), les
tables de valeurs au format exact du client (`tools/export.rs`), pour le dossier « données du serveur »
d'ArchRust (`server-data/` à côté de `realmlist.wtf`, lu avant les packs) :

- `ini/` et chaque dossier de langue du client (`ini_fr`, `ini_de`…) qui a sa copie de la table ; noms de
  fichiers du client (`itemdatatable.txt`, `SKILL_CONST.txt`, `RefineryItem2.txt`, `LevelUpExp.txt`…) ;
- en-têtes et ordre des colonnes du client ; une colonne propre à Archonia reste présente et vide ;
- texte en clair, fins de ligne CRLF, page de code du dossier : CP949 pour `ini/` et `ini_ko`,
  Windows-1252 pour `ini_fr`, `ini_de`, `ini_es`, Shift-JIS pour `ini_ja`, GBK pour `ini_zh` ;
- dans un dossier de langue, les textes traduits du client sont gardés, les valeurs viennent de la base ;
- `tables.toml` à la racine : `format`, `fingerprint` (« sha256:… », chemins et contenus des fichiers),
  `software`, `generated`.

Avec `Client.TablesDir` (voir [[Configuration|FR-Configuration]]), le serveur monde lit cette empreinte au
démarrage et l'annonce aux clients ArchRust (capacité `client-tables/1`, [[Extensions ArchRust|FR-Extensions-ArchRust]]).
Le dossier Game du client vient de `Client.DataDir`, ou de `--game`.

## Migration des personnages

`archcore migrate-ep8` compare les personnages à la base monde EP8 et écrit un rapport par personnage :
objets hors EP8, options et runes retirées, niveaux de compétence au-delà du maximum EP8 (140 compétences
passent de 20 à 15 niveaux), compétences hors EP8. Avec `--apply`, dans une transaction et sans rien
détruire (table `ep8_quarantine`, voir [[Base de données|FR-Base-de-donnees]]) : objets hors EP8 mis en
quarantaine (avec les courriers et ventes qui les citaient), options et runes hors EP8 retirées, niveaux
ramenés au maximum, compétences hors EP8 retirées, et oubli des compétences offert à tous (« skill init »
remis à vide), les coûts en points des niveaux ayant changé. `--restore n` annule la passe n. Sauvegarder
les bases (`mariadb-dump`) avant `--apply`.

## Tables du serveur d'origine

Les apparitions de monstres et la carte serveur ne sont pas dans le client : elles viennent des tables du
serveur d'origine d'ArchLord, fournies dans `data/server-ini` avec la carte dans `world/node_data`.
`archcore import` les trouve automatiquement. L'option `--server` permet de choisir un autre dossier :

| Fichier | Contenu | Table |
|---|---|---|
| `spawndatatable.txt` | Quoi : groupe, modèle et niveau du monstre, nombre, délai de réapparition, guerre de siège. Le niveau retenu est celui du modèle dans `characterdatatable.txt` (EP8) ; le délai des boss vient de la fenêtre des boss du client (`UIBoss.xml`) | `monster_spawn` |
| `spawninstancetable.txt` | Où : position, orientation, rayon de chaque point (un nom peut avoir plusieurs positions) | `monster_spawn` |
| `world/node_data` (à côté ou dans le dossier voisin) | Carte : 16 × 16 segments de 400 unités par secteur, sol infranchissable et région | `map_segment` |
| `itemdroptable.txt` | Objets propres à chaque monstre : quantité, taux sur 100 000 | `monster_drop` |
| `groupdroprate.txt` | Groupes de butin : taux, sensibilité à `DropMeditation`, bonus par niveau | `drop_group` |
| `droprankrate.txt` | Rangs d'objets donnés selon le rang de butin du monstre | `drop_rank_rate` |
| `aidatatable.txt` | IA des monstres : compétences lancées (TID;niveau), condition sur les PV, nombre de lancements, coups normaux | `monster_ai` |

```bash
archcore db-init
archcore import
```

Les lignes de `spawndatatable.txt` sans position dans `spawninstancetable.txt` sont ignorées et comptées.

## Tests unitaires

```bash
cargo test
```

## Rétro-ingénierie

Les relevés ont été faits avec [REA](https://github.com/morluto/rea) et Ghidra 12. La démarche est
décrite sur la page [[Rétro-ingénierie|FR-Retro-ingenierie]].
