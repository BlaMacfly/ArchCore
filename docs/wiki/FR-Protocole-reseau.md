# Protocole réseau

Le client ArchLord 64 bits est une reconstruction du client d'origine. Son protocole reprend celui
du client 32 bits d'origine, avec des longueurs élargies. Tout ce qui suit a été relevé dans
l'exécutable du client et vérifié avec le vrai client.

## Paquet AuPacket

| Octets | Contenu |
|---|---|
| 0 | `D6`, marqueur de début |
| 1–4 | taille totale (u32) |
| 5 | type de paquet (u8) |
| 6 | drapeaux d'en-tête (u8) |
| 7–10 | propriétaire (u32) |
| 11–18 | horodatage (8 octets) |
| 19… | drapeaux de champs (1, 2 ou 4 octets selon le module), puis les champs présents |
| fin | `6B`, marqueur de fin |

- Un bit à 1 dans les drapeaux de champs signale un champ présent.
- Les mini-paquets, blocs mémoire et listes sont précédés de leur longueur en **u32**
  (u16 dans le client 32 bits).
- Le type 3 (`AgcmCharacter`) utilise des drapeaux de champs sur **4 octets**.

## Trames chiffrées

Après la poignée de main, chaque paquet est encapsulé :

```text
A1 (serveur) / B1 (client) | taille u32 | compteur u32 | données Blowfish ECB | AF (serveur) / BF (client)
```

Blowfish travaille en mots de 32 bits petit-boutistes, et les données sont complétées par des zéros
jusqu'à un multiple de 8 octets.

## Poignée de main (type 0x48)

1. Le client envoie `REQ`.
2. Le serveur répond `SET_ALGO`, puis `PUBLIC_KEY` (32 octets).
3. Le client envoie `PRIVATE_KEY` : sa clé, chiffrée avec celle du serveur.
4. Le serveur confirme avec `COMPLETE`. Tout le reste est chiffré avec la clé du client.

## Connexion (type 0x0D, AgpmLogin)

| Op | Sens | Rôle |
|---|---|---|
| 0 | ↔ | Demande et envoi d'une clé de 8 caractères pour chiffrer le mot de passe (RC4 CryptoAPI) |
| 1 | ↔ | Identifiant et mot de passe (65 octets), puis réponse |
| 3 | ↔ | Choix du monde ; réponse avec la race courante (champ 4 du mini `CHAR_INFO`) |
| 4, 5 | → | Noms des personnages, puis fin de liste |
| 6, 7 | ↔ | Demande de la liste ; un paquet personnage (type 2) par personnage, puis fin |
| 10 | ↔ | Écran de création : modèles de base, puis confirmation |
| 11, 12 | ↔ | Création d'un personnage |
| 13 | ↔ | Suppression d'un personnage |
| 8 | ↔ | Entrée dans le monde : CID au champ 6, `CHAR_INFO` au champ 8, adresse du monde au champ 9 |
| 15 | → | Résultat d'erreur (mauvais mot de passe, nom déjà pris…) |

La race envoyée par l'op 3 compte : le bouton « Créer » du client n'envoie l'op 10 que pour la race
1 ou 2. Avec 0, l'écran de création reste en chargement. Les modèles de l'écran de création
reçoivent des CID à part, à partir de `0x7F000000`, pour ne jamais croiser un vrai personnage.

La liste des mondes passe par le type 0x18 (AgpmWorld), au format `Nom=Groupe=0=Statut=Drapeau;…`.

## Entrée dans le monde (type 0x03, AgcmCharacter)

1. Le client envoie l'**op 2** : nom au champ 3, clé d'authentification au champ 6.
2. Le serveur envoie la configuration, le personnage, puis l'**op 8** avec la position.
3. Le client répond l'**op 9** avec son CID.
4. Le serveur envoie les PNJ proches, puis l'**op 7** : le joueur est en jeu.
5. Le client envoie l'**op 13** toutes les 15 secondes pour rester connecté.

## Horloge (type 0x11, AgpmTimer)

Entre la position de chargement (op 8) et l'entrée en jeu, le serveur envoie un paquet AGPMTIMER :
op 6 au champ 0, et au champ 1 l'heure du serveur en **millisecondes** (u64). Le client la fait
avancer localement et en tire l'heure du jeu par `(ms / 3 600 000) % 24`, **sans correction de
fuseau** (`AgpmEventNature`) : cette heure pilote le cycle jour/nuit, la musique et l'ambiance de zone
(`skyset.ini`). Le serveur coréen d'origine envoyait vraisemblablement son heure locale.

ArchCore envoie donc par défaut l'époque Unix **décalée du fuseau local** de la machine (minuit local
de l'époque = 0), pour que l'heure du jeu soit l'heure du serveur ; `World.TimeZone = utc` dans la
[[configuration|FR-Configuration]] renvoie à l'époque Unix UTC brute. Les champs 2 et 3 (u32) ne sont
pas utilisés. `archcore sim` affiche la valeur reçue et l'heure en jeu correspondante.

## Apparition des PNJ (type 0x42, AgpmOptimizedView)

L'op 1 porte au champ 1 un mini-paquet « vue de personnage » de 26 champs : CID, TID, nom,
position, destination, orientation, etc. ArchCore donne aux PNJ des CID à partir de `0x40000000`.

Les monstres passent par le même paquet, avec des CID à partir de `0x50000000`. Il leur faut en plus le
champ 5 (facteurs : niveau et vitesses, type, PV/PM/PE actuels et maximum, attaque), sans quoi le client
les croit à 0 PV ; le champ 6 vaut `0x04` (arrêté) et le champ 9 (destination) est absent.

**Hauteur.** La plupart des points d'apparition des tables du serveur n'ont pas de hauteur (0). Envoyée
telle quelle, une créature se trouvait plusieurs milliers d'unités sous le sol : le client la remontait
tant que le terrain lointain était grossier, puis elle disparaissait sous le relief détaillé quand le
joueur s'approchait. ArchCore pose donc ces créatures sur le sol du client (`map_height`, lue dans
`world/b*x.ma2`) et leur fait suivre le terrain quand elles marchent.

**Visibilité par secteurs.** Le monde est découpé en 800 × 800 secteurs de 6400 unités, à partir de
(-2 560 000, -2 560 000) en X et Z. Comme sur le serveur d'origine, PNJ, monstres, autres joueurs et objets
au sol apparaissent à un joueur dès qu'ils entrent dans les 3 × 3 secteurs centrés sur le sien. ArchCore ne
les retire qu'au-delà des 5 × 5 secteurs (au moins 12 800 unités) : le client dessine les personnages
jusqu'à 10 000 unités environ, et un retrait à la sortie des 3 × 3 secteurs pouvait tomber à 6400 unités,
sous ses yeux. Ils y apparaissent (vue optimisée op 1, paquet
personnage op 0, objet op 0) ou en disparaissent (AGPMCHARACTER op 3 « retrait de la vue », objet op 1) quand lui ou eux
changent de secteur. Le serveur suit la marche du joueur entre deux paquets de déplacement, d'après sa
vitesse (dixièmes d'unité par seconde : 2000 en marche, 5000 en course pour un humain). C'est la règle du client
Archonia : un trajet dure `10000 / vitesse × distance` ms, avec le facteur de statut 8 en marche et 9 en course
(drapeau 0x10 du déplacement, plafond 60 000) ; une vitesse inconnue vaut 4000 en marche et 12 000 en course. Les retraits passent par l'op 3 (« retrait de la vue »), comme le serveur d'origine : l'op 2 supprime le personnage aussitôt, tandis que l'op 3 passe d'abord par les fonctions de vue du client, qui peuvent différer la suppression.

## Fonctions des PNJ (types 0x3C et 0x37)

Le client ne sait qu'un PNJ a des fonctions que si elles arrivent avec sa vue : le **champ 4** du paquet
de vue (0x42) contient une suite de paquets **ApmEventManager (0x3C)** complets, un par fonction.

| Champ du 0x3C | Contenu |
|---|---|
| 0 | désignation de l'événement (mini EVENT_BASE) : type d'objet **3** (personnage), CID du PNJ, fonction |
| 1 | donnée de la fonction (mini à un entier) : modèle de dialogue, modèle de boutique, groupe de quêtes |

Fonctions lues dans `npc.ini` : **21** dialogue, **9** boutique, **27** quêtes.

Au clic, le client envoie **AgpmEventNPCDialog (0x37) op 0** : événement au champ 1 (type 3, CID du
PNJ, fonction 21), CID du joueur au champ 2. Le serveur répond **op 1** avec les mêmes champs, et le
client ouvre la fenêtre ; le texte vient du client (`npcdialog.txt`). ArchCore accorde le dialogue si
le joueur est à moins de 1500 unités du PNJ.

## Objets (type 0x04, AgpmItem)

Drapeaux de champs sur **4 octets**, 27 champs. Les principaux :

| Champ | Contenu |
|---|---|
| 0 | op (INT8) : 0 ajout, 1 retrait, 2 mise à jour |
| 1 | statut (INT8) : 2 inventaire, 10 boutique d'un PNJ |
| 2 | IID, identifiant de l'objet |
| 3 | TID, modèle de l'objet |
| 4 | CID du propriétaire |
| 5 | quantité |
| 7 | case : mini à 3 INT16 (sac, ligne, colonne) |

Le client crée l'objet à partir de son propre modèle, lu dans `ItemTemplateAll.ini`. Si le TID lui
est inconnu, il ignore l'objet sans rien afficher.

À l'entrée en jeu, l'inventaire n'est pas envoyé en paquets séparés. Les paquets AGPMITEM complets
sont mis bout à bout dans le **champ 6** du paquet de vue 0x42 **op 2**. La grille d'inventaire du
client fait 5 sacs de 5 × 5 cases (grille de type 1).

## Boutiques des PNJ (type 0x32, AgcmEventNPCTrade)

Drapeaux de champs sur **2 octets** :

| Champ | Contenu |
|---|---|
| 0 | événement (mini EVENT_BASE) |
| 1 | op |
| 2 | CID du joueur |
| 3 | IID de l'objet |
| 4 | détail (mini, drapeaux sur 2 octets) : TID, prix (entier), quantité, statut, sac, ligne, colonne… |
| 5 | horodatage (INT64) |
| 6 | résultat |
| 7–9 | case visée dans l'inventaire (achat) |

1. Au clic sur « Commerce », le client ouvre tout de suite la fenêtre, avant toute réponse, et
   envoie l'**op 0** pour demander la liste.
2. Pour chaque objet, le serveur envoie un AGPMITEM de **statut 10**, avec le CID du PNJ comme
   propriétaire, puis un 0x32 **op 0** qui porte l'IID et le détail. L'**op 1** termine la liste.
   La grille d'une boutique fait 1 × 20 × 4 cases.
3. Achat : le client envoie l'**op 2**, avec la quantité au champ 2 du détail. Le serveur répond
   l'op 2 avec le résultat : **2** réussi, **3** pas assez d'or, **4** inventaire plein.
4. Vente : le client envoie l'**op 3** avec l'IID d'un objet de son inventaire. Le serveur répond
   l'op 3 avec le résultat : **0** réussi, **1** trop d'or.

L'argent du personnage est mis à jour par un paquet personnage (type 2) op 1, champ 8.

## Déplacement (type 0x40, AgpmOptimizedCharMove)

Le client envoie un paquet 0x40 à chaque clic de déplacement (`FUN_140264010`), avec des drapeaux
d'en-tête sur 1 octet :

| Champ | Contenu |
|---|---|
| 0 | CID du personnage |
| 1 | position actuelle (souvent absente : ArchCore prend alors la destination précédente) |
| 2 | destination |
| 3, 4 | cible suivie et distance de suivi (u16) |
| 5 | drapeaux de déplacement : `0x04` arrêt, `0x08` synchronisation, `0x01` direction… |
| 6 | direction |

ArchCore met à jour la position du personnage, relaie le même paquet aux joueurs qui le voient, puis
recalcule la visibilité par secteurs (voir plus haut) : un paquet personnage **op 0** ajoute un joueur
entré dans les secteurs voisins, l'**op 3** (retrait de la vue, CID au champ 1) retire un joueur qui en est sorti ou s'est
déconnecté. La position est
sauvegardée à la déconnexion et toutes les minutes.

## Discussion (type 0x0F, AgpmChatting)

Drapeaux de champs sur 1 octet : op, CID émetteur, type de canal, CID cible, nom émetteur (49),
nom cible (49), message (bloc mémoire).

- Le client envoie l'op 0 avec son CID, le type (0 normal, 5 chuchotement avec le nom cible) et le
  message **sans zéro final**, de moins de 256 octets. Une commande MJ est un message normal qui
  commence par `/`.
- Si le joueur a choisi une couleur de discussion, le client ajoute `\x01RRGGBB` à la fin du message.
  ArchCore le retire avant d'interpréter une commande et le garde pour l'affichage.
- À la réception, le client refuse un message de plus de 512 octets. Le type 7 (message système)
  s'affiche sans émetteur ; ArchCore l'utilise pour répondre au MJ, et le type 10 pour les annonces.

## Combat (type 0x41, AgpmOptimizedCharAction)

Drapeaux sur 2 octets : 0 attaquant (I32), 1 cible (I32), 2 résultat (I8), 3 facteurs (paquet), 4 PV de
la cible (I32), 5 combo (I8), 6 attaque forcée (I8), 7 effet additionnel (U32), 8 index de coup (U8).

- **Client → serveur** : 0, 1 et 6. Le client désigne sa cible ; il n'envoie pas un paquet par coup.
- **Serveur → clients** : un paquet par coup, à tous les joueurs qui voient la cible. Résultat 1 =
  manqué, 5 = touché (6 critique, 7 esquive, 9 blocage…). Le champ 3 n'est présent que si le coup
  touche ; le client le lit avec le **schéma complet AGPMCHAR_FACTORS** (vérifié dans le client x64) :
  le sous-paquet 0 « résultat » contient un bloc de points (PV, PM, dégâts normaux en négatif, champ 9)
  qui sert à l'affichage du coup, le sous-paquet 3 les PV et PM restants de la cible.

Hors de portée (portée d'attaque du joueur + rayon `HitRange` de la cible), le serveur fait marcher le
joueur jusqu'à la cible avec un AGPMOPTIMIZEDCHARMOVE ; un déplacement demandé par le client interrompt
l'attaque. Les coups suivent la vitesse d'attaque (60 000 / vitesse ms).

**Dégâts élémentaires.** Un coup qui touche ajoute, pour chaque élément (magie, eau, feu, terre, air,
poison, foudre, glace) dont l'attaquant a des dégâts, une part qui porte avec une chance de 70 % pour un
joueur, de 80 % plus l'écart de niveau (60 % au moins) pour un monstre : un jet entre le minimum et le
maximum de l'élément, doublé, moins la résistance de la cible à cet élément en pour cent (80 % au plus pour
un joueur). Chaque part s'affiche dans le bloc de points du résultat, aux champs 10 (magie) à 17 (glace) ;
les PV perdus en font la somme. Les dégâts élémentaires d'un joueur viennent de son équipement (colonnes
`Magic_Min_Damage`… d'itemdatatable.txt) et des pierres d'esprit serties ; ceux d'un monstre de
CharacterDataTable.txt. Les monstres n'ont pas de résistance élémentaire dans les données.

À la mort d'un monstre, AGPMCHARACTER op 1 avec le champ 11 (statut d'action) à 2 ; le cadavre est
retiré 20 s plus tard (op 3), puis le monstre réapparaît à sa place avec tous ses PV. Le tueur reçoit
l'expérience du modèle (`Exp` de CharacterDataTable.txt × taux `exp`) dans ses facteurs (op 1, champ 7)
et un message système ; l'expérience se compte par niveau (`LevelUpExp.txt`), et une montée de niveau
est signalée par AGPMCHARACTER **op 5**. Seuls les modèles dont le type porte les bits « attaquable »
(0x10000) et « ciblable » (0x20000) peuvent être visés : les créatures d'ambiance (type 8 : coqs,
poules…) ne le sont pas.

**Croissance par niveau.** Les facteurs d'un joueur suivent son niveau d'après `GrowUpFactor.txt` (table
`growup_factor`) : un bloc par modèle (ligne d'en-tête = TID, puis une ligne par niveau, de 1 à 121), en
valeurs absolues. La ligne du niveau remplace les caractéristiques (FOR, DEX, CON, SAG, INT), les PV, PM
et PE maximum et la défense physique (`AC`) du modèle ; charisme, précision, esquive et dégâts ne le
remplacent que s'ils ne sont pas nuls (ils le sont partout dans le fichier), et les résistances ne sont
pas lues. Au-delà du dernier niveau, la dernière ligne vaut ; un modèle sans bloc prend celui d'un modèle
jouable de même race et de même classe. À la création, le personnage reçoit les PV, PM et PE de son
niveau. À chaque montée de niveau et à `/level`, les facteurs sont recalculés (croissance, équipement,
effets des compétences), PV et PM remis au maximum, puis envoyés (op 1, champ 7). Un guerrier humain
passe ainsi de 120 PV et 32 PM au niveau 1 à 456 PV et 108 PM au niveau 25 (le modèle seul donnait 90 et
46 à tout niveau).

**Riposte des monstres.** Un monstre frappé retient son agresseur (dégâts cumulés, oublié 30 s après son
dernier coup) et vise, toutes les 500 ms, celui qui lui a fait le plus de dégâts parmi les joueurs
vivants à moins de 5000 unités. Il le rejoint par AGPMOPTIMIZEDCHARMOVE (champ 0 = CID du monstre),
puis le frappe par le même paquet 0x41 (attaquant = monstre, cible = joueur), au rythme de sa vitesse
d'attaque. Sans agresseur, il rentre à son point d'apparition ; à plus de 10 000 unités de ce point, il
y est ramené avec tous ses PV. Pas d'attaque spontanée à vue.

**Patrouille.** Un monstre au repos (type « monstre », sans agresseur), vu par au moins un joueur et dont le
point d'apparition a un rayon, marche toutes les 15 à 45 s vers un endroit tiré au hasard dans ce rayon,
hors sol infranchissable : AGPMOPTIMIZEDCHARMOVE avec le champ 5 à 0 (marche), alors qu'une poursuite
court (0x10). Au plus 16 départs par tour.

**Mort du joueur.** À 0 PV : AGPMCHARACTER op 1 (facteurs, champ 11 = 2) au joueur et à ceux qui le
voient ; il ne bouge ni n'attaque plus, et les monstres le lâchent. Le client demande la résurrection au
village par AGPMCHARACTER **op 9** : le serveur rend tous les PV et PM, renvoie le statut 0, téléporte le
personnage au point de résurrection de sa région de rattachement et le rend invincible 10 s.

**Régions et points de résurrection.** Chaque segment de la carte serveur porte un numéro de région
(RegionTemplate.ini), dont le champ `Type` dit si elle est sûre (bits 2-3 à 0), libre ou dangereuse. Un
joueur est rattaché à la dernière région sûre traversée ; à l'entrée en jeu, à celle où il se trouve si elle
est sûre, sinon à celle de sa race (9 Anchorville, 5 Golundo, 26 Norine). Les points de résurrection sont
les objets des obj*.ini du client qui portent un événement 19 (« binding ») de type 1 : région par son nom
(`TownName`), race et classe admises (-1 ou 0 : toutes), position de l'objet. Le joueur revient au point de
sa région le plus proche qu'il peut utiliser ; à défaut, au village de départ de sa race.

**Butin.** À la mort d'un monstre, son butin tombe au sol autour du cadavre (± 150 unités) : AGPMITEM
op 0 avec le statut 1 (« au sol »), l'IID (plage `0x30000000`), le TID, le propriétaire (CID du tueur),
la quantité et, au champ 6, un mini-paquet d'un seul VEC3F, la position (relevé dans le client x64). L'or
est l'objet de TID 268, sa quantité étant la somme. Le client demande le ramassage par AGPMITEM **op 11**
(champ 2 = IID) ; le serveur range l'objet (AGPMITEM op 0 statut 2, ou op 2 pour une pile existante) ou
crédite l'or (AGPMCHARACTER op 1, champ 8), puis répond **op 12** (champ 1 : 0 réussi, 1 échec ; 2 IID,
3 TID, 5 quantité) et retire l'objet du sol (op 1) chez ceux qui le voyaient. Un objet est réservé au
tueur 30 s, ramassable à 2000 unités au plus, et disparaît au bout de 45 s.

Tirage : or entre `Geld_Min` et `Geld_Max` du monstre × taux `gold` ; pour chaque groupe de
`groupdroprate.txt`, une chance sur 100 000 (taux du groupe × taux `drop`, × `DropMeditation` du
monstre pour les groupes qui y sont sensibles, + bonus selon le niveau du tueur), puis un rang d'objet
selon le rang de butin du monstre (`droprankrate.txt`) et un objet de ce groupe et de ce rang
(`GroupID`, `DropRank`, `DropRate` d'itemdatatable.txt) adapté au niveau du monstre ; enfin les objets
propres au monstre (`itemdroptable.txt`), toujours tirés à 100 000.

## Équipement (type 0x04, AgpmItem)

Le client demande un changement de place par AGPMITEM **op 2** : statut 3 pour porter un objet de
l'inventaire, statut 2 avec la case voulue au champ 7 (mini-paquet de trois INT16 ; -1 = au choix du
serveur) pour ranger un objet porté ou déplacer un objet dans l'inventaire. Le serveur vérifie le niveau,
la race, la classe et le sexe exigés (`Level`, `Race`, `Class`, `Gender` d'itemdatatable.txt), range
dans l'inventaire l'objet déjà porté sur la même partie du corps (`Part` : 2 corps, 3 tête, 5 mains,
6 jambes, 7 pieds, 9 main gauche, 10 main droite, 11 et 12 anneaux, 13 collier) et libère l'autre main
pour une arme à deux mains (`Hand` = 2). Il répond au porteur par des op 2 (statut 3 sans case pour
l'objet porté, statut 2 avec sa case pour un objet rangé), envoie aux joueurs qui le voient un op 0
(statut 3, propriétaire = porteur) ou un op 1, puis renvoie les facteurs du porteur (AGPMCHARACTER op 1,
champ 7). L'équipement s'ajoute aux facteurs du modèle : dégâts physiques min et max, défense physique,
précision (`Physical_ATKRate`), PV, dégâts et défenses élémentaires, et pour une arme sa vitesse et sa
portée d'attaque, le tout avec la conversion de l'objet (section suivante). Un refus donne
un message système et remet l'objet à sa place. En base, un objet porté a le statut 1 et sa partie dans
`layer`. L'équipement d'un joueur est envoyé (op 0, statut 3) à ceux qui commencent à le voir.

Dans l'inventaire, poser une pile sur une pile du même objet (op 2, statut 2, case de la cible) les réunit
si le total tient dans une pile : la source disparaît (op 1), la cible est mise à jour (op 2). AGPMITEM
**op 17** sépare une pile (quantité au champ 5, case libre au champ 7) : la nouvelle pile arrive par op 0,
l'ancienne est mise à jour par op 2. **Op 21** détruit un objet de l'inventaire (op 1 en réponse).

## Options des objets (type 0x04, champ 20)

Un équipement peut porter jusqu'à 5 options d'`itemoptiontable.txt` (importée dans `client_table`, avec
`optionnumdroprate.txt` et `socketnumdroprate.txt` des tables du serveur d'origine). Elles sont tirées quand
l'objet est créé : butin d'un monstre (niveau de la source = niveau du monstre), `/create` et `/additem`
(niveau de l'objet). En base : `character_items.option_tids`.

- **Nombre** : entre `MinOption` et `MaxOption` de l'objet, pondéré par `optionnumdroprate.txt` (colonne =
  `MaxOption`). Les sockets de l'objet neuf se tirent de même entre `MinSocket` et `MaxSocket`
  (`socketnumdroprate.txt`).
- **Choix** : un « Type » d'option au hasard, sans remise, parmi ceux qui ont une option admise ; puis une
  option de ce type au poids (`Probability`, 0 au-delà de 100 000). Admise : la partie de l'objet est cochée
  (une arme tenue à gauche prend les options d'arme), le rang de l'objet est entre `Rank_Min` (0 vaut 1) et
  `Rank_Max`, le niveau de la source vaut au moins `Level_limit` (et 1 à 120). Une cellule `Refined` ou
  `Gacha` non vide (même « 0 »), ou un `CategoryName`, range l'option dans une autre famille, hors butin.
- **Réseau** : AGPMITEM champ 20, mini-paquet de 5 UINT16 (TID), dans chaque op 0 d'un objet connu
  (inventaire, équipement vu des autres, banque, échange, objet au sol). Le client applique lui-même les options
  aux facteurs de l'objet pour l'infobulle (« Additional Ability », `Description` de la table).
- **Effet** : le serveur ajoute aux facteurs du personnage les options de l'équipement porté. Chaque colonne
  vise un facteur (dégâts min et max d'un attribut, défense en points ou en taux, PV et PM max, précision,
  caractéristiques, portée et vitesse d'attaque, vitesse de déplacement) ; `PointType` 0 ajoute la valeur,
  1 l'applique en pour cent au total, comme toujours `ATK_Speed_bonus`, `Movement_Run_bonus` et
  `*_Percent`. Les colonnes à déclenchement (Critical, skill_*, bonus_*, Ignore_*, on_hit_*…) ne sont pas
  encore appliquées.
- **Options liées** : un objet unique cite des groupes d'options (`Link_id` d'itemdatatable.txt, séparés par
  « ; ») ; pour chaque groupe, le porteur reçoit l'option dont `Level_min` ≤ son niveau ≤ `Level_max`. Rien
  n'est envoyé : le client les retrouve lui-même pour l'infobulle.

## Conversion des objets (types 0x16 et 0x38, AgpmItemConvert et AgpmEventItemConvert)

Règles et codes relevés dans `Archonia.exe` ; chances et valeurs d'`ItemConvertTable.txt` (importée dans
`client_table`). L'état d'un objet (niveau de renforcement, sockets, TID sertis) est en base dans
`character_items` (`convert_level`, `socket_count`, `socket_tids`).

AGPMITEMCONVERT (drapeaux 2) : 0 op, 1 CID, 2 IID de l'objet, 3 IID de ce qui le convertit (potion, pierre),
4 résultat, 5 niveau de renforcement, 6 sockets, 7 sockets remplis, 8 TID ajouté, 9 TID sertis (mini-paquet
de 8 INT32).

- **op 0** : état d'un objet, envoyé après son AGPMITEM op 0 (entrée en jeu, banque, échange, ramassage,
  création) pour un objet renforcé ou à sockets. Un objet neuf reçoit ses sockets par tirage (section
  précédente) ; un objet qui reçoit des sockets et n'en a pas d'enregistré en a max(1, `MinSocket`), comme le
  calcule le client.
- **Forgeron** (fonction 22 de npc.ini) : AGPMEVENTITEMCONVERT op 0 (événement, CID) demande la fenêtre,
  op 1 l'accorde. L'ajout de socket et le renforcement exigent un forgeron à portée.
- **op 2 → op 6, ajout de socket** : arme (sauf lance, `Type` 17), armure, ou « autre » (genres 4, 8, 16,
  64), 8, 6 et 5 sockets au plus. Chance du n-ième socket : section « Socket Slot Success » (en-tête coréen
  du fichier du serveur d'origine : succès d'ajout de socket) ; en cas d'échec, second tirage dans
  « Socket Slot Fail » : 5 sans perte, 6 sertissages perdus, 7 objet remis à son état d'origine, 8 objet
  détruit. Réussite 1, plein 2, objet inadapté 9. Pas de prix : la table des coûts du client est vide et il
  affiche 0. Après 6 ou 7, un op 0 redonne les sertissages, que le client ne relit pas dans la réponse.
- **op 4 → op 7, pierre d'esprit** : pierre (`Type` 2) d'élément 1 à 5 (`Spirit Type` : magie, eau, feu,
  terre, air) et de rang 1 à 5 (`Rank`), dans une arme (sauf lance) ou une armure de l'inventaire qui a un
  socket libre ; la ligne `PhysicalRank` de l'objet dans la première section doit avoir la case
  `Spirit<rang>`. Chance « Weapon Probability » ou « Armor Probability » du rang ; la pierre est consommée
  dans tous les cas. Ici **0 = réussite**, 1 échec, 2 pierre refusée, 3 plein, 5 objet inadapté. Effet :
  « Weapon Value Increase » du rang aux dégâts min et max de l'élément d'une arme, « Reinforcement Level% »
  à la résistance d'une armure ; chaque pierre de plus du même élément ajoute « Elemental Stone Bonus » (3).
- **op 1 → op 5, renforcement** : potion de renforcement (`Type` 8, consommée dans tous les cas) sur une
  arme (sauf lance) ou une armure de corps ou de jambes ; niveau au plus `PhysicalRank`. Ligne
  `PhysicalRank`, colonne niveau visé : réussite selon « Success », sinon second tirage : « Unchanged »
  (5), « Reset Reinforcement » (6, retour à +0), « Destroy » (8). Plein 2, objet inadapté 3, mauvaise potion
  9. Chaque niveau ajoute « Weapon Increase » aux dégâts physiques d'une arme, « Armor Increase » à la
  défense physique du reste, en cumulant les niveaux.

- **op 3 → op 8, rune** : une rune (objet d'`itemruneattributetable.txt`, importée dans `client_table`)
  dans un objet de l'inventaire dont la partie est cochée pour elle (mêmes colonnes que les options) et qui a
  un socket libre ; refus : 2 rune inconnue, 3 plein, 6 objet sous `Mount_Restriction_Level`, 7 partie
  refusée, 8 rune du même `ANTI Number` déjà sertie. Réussite (1) selon `Success_Rate`, toujours avec
  `AlwaysSuccess` ; sinon second tirage dans « Socket Slot Fail » à l'indice sertissages + 1 : 9 sans perte,
  10 sertissages perdus, 11 retour à +0 et sertissages perdus, 12 objet détruit. La rune est consommée dans tous
  les cas. Une rune sertie apporte au porteur les options de son `OptionTID` (itemdatatable.txt). Une rune
  envoyée par l'op 4 est reconnue à sa table et traitée comme une rune.

Pas encore gérés : les runes qui donnent une compétence (`Skill_Name`), les pierres de glace, de foudre et de
venin et les pierres de caractéristiques d'Archonia (`Spirit Type` 6 à 8 et 100 à 106, refusées par le client).

## Raffinage (types 0x4B et 0x3F, AgpmRefinery et AgpmEventRefinery)

Recettes de `RefineryItem2.txt`, la table que lit `Archonia.exe` (5 731 recettes, importée dans
`client_table` ; à défaut, `refineryrecipetable.txt` des tables du serveur d'origine). Une recette se reconnaît
à ses ingrédients (`Resource TID`, `Resource Count`), dans n'importe quel ordre ; ses `MakeItem Total` lignes
sont autant d'issues équiprobables (objet et quantité produits, objet bonus, nombre d'options `Item Option
Min/Max` et de sockets `Socket Inchant Min/Max` de l'objet produit) ; `Combine Use Price` est son prix en or.

- **Ouverture** : AGPMEVENTREFINERY op 0 (événement, CID) sur un objet du monde de fonction 26 (la sphère
  d'alchimie des `obj*.ini`, pas un PNJ) ; op 1 l'accorde, le client ouvre `UI_Refinery`.
- **AGPMREFINERY** (drapeaux 2) : 0 op, 1 CID, 2 identifiant de recette, 3 IID des 20 cases de la grille
  (INT32[20], 0 = case vide), 4 résultat, 5 TID produit, 6 prix (INT64).
  - **op 5 → op 5, devis** : la grille désigne une recette, le serveur rend son prix au champ 6.
  - **op 2, raffinage de la grille** : les objets de la grille, en entier, sont les ingrédients.
  - **op 6, raffinage d'une recette** : identifiant au champ 2, ingrédients pris dans les piles de l'inventaire.
  - **op 1, résultat** : 0 réussite (TID produit au champ 5), 1 ingrédients manquants, 2 objets sans recette,
    4 or insuffisant, 5 inventaire plein. Le client affiche `REFINE_*_ITEM` d'uimessage.
- **Objet produit** : un équipement reçoit des options (entre `Item Option Min` et `Max`, tirées comme celles du
  butin) et des sockets (`Socket Inchant Min`/`Max`), ou ceux d'un objet neuf ; il garde de l'équipement fourni
  (le même modèle, sinon le premier) son renforcement, ses sockets, ses sertissages et ses options selon les
  colonnes `Keep Reinforcement Level`, `Keep Item Socket Count`, `Keep Socketed Items`, `Keep Item Options`.
  L'issue « échec » (`MakeItem IsFail`) produit son objet comme les autres : le client ne lit pas la colonne.

## Potions et régénération (types 0x04 et 0x02)

Le client utilise un objet par AGPMITEM **op 3** (champ 2 = IID). Une potion est un objet utilisable
(`ItemType` 1 d'itemdatatable.txt) qui rend `HP` PV ou `MP` PM, sans dépasser le maximum ; elle est
refusée au mort, à jauge pleine, ou tant que le délai de sa sorte (vie ou mana, `Use Interval` en ms)
court encore. Une utilisation consomme un exemplaire (AGPMITEM op 2 avec la nouvelle quantité, ou op 1 pour
le dernier), se confirme par AGPMITEM **op 4** (TID au champ 3, utilisateur aux champs 4 et 12, délai de
réutilisation en ms au champ 18) et renvoie les facteurs (AGPMCHARACTER op 1, champ 7) au joueur et à ceux
qui le voient.

Hors combat (aucun coup donné ni reçu depuis 10 s), un joueur vivant regagne toutes les 10 s 1,5 % de ses
PV et PM maximum, plus son taux de récupération, plus 5 ; ses nouveaux facteurs sont envoyés de la même
façon.

### Prise automatique (type 0x14, AgpmUIStatus)

Deux curseurs `PointScrollBar` posés sur les jauges de PV et de PM de `UI_CharInfo` (`CONTROL_9745`,
`CONTROL_24256`, « Use Life Potion Automatically ») règlent un seuil en pourcentage (position × 100) : le
client boit la potion de la case de vie ou de mana de la barre quand la jauge passe dessous. Le serveur ne
fait que garder les réglages (table `character_ui_status`) : potions et seuils lui sont envoyés par
AGPMUISTATUS **op 1** (champs 6 et 7 : TID des potions de vie et de mana ; 9 et 10 : seuils), le casque par
l'op 3 (champ 8) ; seuls les champs présents changent. À l'entrée en jeu, il renvoie le tout par l'**op 0**.
Format d'Archonia, drapeaux sur 2 octets : `INT8 INT32 PACKET[50] INT8 PACKET MEMORY_BLOCK INT32 INT32 INT8
INT8 INT8 MEMORY_BLOCK` (opération, CID, barre de raccourcis, case changée, son contenu, barre encodée,
potions, casque, seuils, temps de recharge encodés).

## Parchemins de téléportation (type 0x04)

Un parchemin (« Passport ») est un objet utilisable de `Type` 4 ; son `SubType` donne sa sorte : 0 retour au
village, 1 aller-retour, 2 destination fixe (colonne `Destination` : x;y;z), 3 retour au village immédiat. Il
s'utilise par AGPMITEM op 3 comme une potion (un exemplaire consommé, confirmation op 4), puis le serveur
téléporte le joueur (AGPMOPTIMIZEDCHARMOVE et AGPMCHARACTER). Le « village » est le point de résurrection de
la région de rattachement, comme pour la résurrection. Le retour au village (0 et 1) est refusé en combat. L'aller-retour mémorise
l'endroit quitté et l'annonce par AGPMITEM **op 9** (champ 4 = CID) ; le client demande à y revenir par
**op 6**, refusé en combat ; le serveur répond **op 10** (retour désactivé) et y ramène le joueur.

## Compétences (types 0x05 et 0x3B)

**Données.** `SKILL_SPEC.txt` décrit la nature de chaque compétence (champ `attribute` : renforcement,
affaiblissement, dégâts physiques ou magiques, passive ; `target` ; `melee_attack` : dégâts immédiats) ;
`SKILL_CONST.txt` ses valeurs par niveau : classes admises (`require_class_*`), niveau et points exigés,
prix d'apprentissage (`skill_cost`) et d'amélioration (`skill_upgrade_cost`), coûts en PV et PM, durées
d'incantation, d'effet et de relance (ms), portée (mètres, × 100 en unités du jeu), dégâts physiques et
élémentaires (fixes et pour cent). Les deux tables désignent la compétence par son nom, relié au TID par
`SKILLTEMPLATE.ini`.

**Compétences d'un personnage.** AGPMSKILL (drapeaux 2) : op, identifiant, propriétaire (mini « base » :
type 3 = personnage, CID), facteurs (AGPMCHAR_FACTORS dont le sous-paquet « dirt » porte le niveau à
l'indice 4), TID, statut, action, points, effets actifs. À l'entrée en jeu, chaque compétence part en
**op 0** dans le champ 8 de la vue optimisée op 2. Tout personnage connaît « Normal Attack » (TID 222).
Les points de compétence valent le niveau du personnage (plus un par niveau de 50 à 70) moins les points
exigés par chaque niveau appris ; ils sont envoyés dans les facteurs « dirt » du personnage (indice 5).

**Maître des compétences.** Les PNJ « Instructeur » portent la fonction 25, avec la race et la classe
enseignées (`SkillRaceType`, `SkillClassType`), envoyées dans leur vue (mini INT8, INT8).
AGPMEVENTSKILLMASTER (0x3B, drapeaux 1) : op, événement, CID, identifiant de compétence, TID, résultat,
points. **Op 4** ouvre la fenêtre (réponse op 5, résultat 0) ; **op 1** apprend le TID du champ 4 :
AGPMSKILL op 0 puis op 6 avec le résultat (0 réussi, 1 échec dont l'argent, 2 déjà apprise, 3 pas assez
de points, 4 niveau trop bas, 5 classe) ; **op 2** améliore la compétence du champ 3 : AGPMSKILL **op 2**
(nouveau niveau) puis op 3.

**Lancement.** Le client envoie AGPMSKILL **op 7** : identifiant et action (mini : type, cible en mini
« base »). Le serveur vérifie la compétence, son délai de relance, les PM et PV, fait marcher le joueur
jusqu'à portée, retire le coût et diffuse **op 7** (action 1 : cible, position, durées d'incantation,
d'effet et de relance, niveau). Au bout de l'incantation, **op 8** (action 2 : cible, facteurs du
résultat) part aux joueurs qui voient la cible : comme pour le combat, le sous-paquet 0 des facteurs porte
les PV restants et les dégâts en négatif, à l'indice 9 pour la part physique et 10 à 17 pour les éléments
(magie, eau, feu, terre, air, poison, foudre, glace). L'attaque automatique reprend ensuite sur la cible.

Dégâts : la part physique reprend le jet d'attaque du personnage, multiplié par (100 + pour cent) / 100 et
augmenté des dégâts fixes ; chaque élément reprend les dégâts élémentaires du personnage de la même façon,
doublés. Une cible de niveau supérieur retire 5 % par niveau d'écart (80 % au plus) ; l'armure ne réduit
que la part physique ; chaque part fait au moins 1.

**Cibles et zones.** Une compétence vise des ennemis si le chiffre des unités de son champ `target` vaut
1 ; sinon elle s'applique à son lanceur. Une compétence de groupe (champ `range` 303 : sphère autour du lanceur,
réservée aux membres de son groupe ; Marching Song, Combat Echo, Battle Cry…) touche aussi les membres de son
groupe vivants et en jeu à moins de `area_r` du lanceur (700 unités pour Marching Song) : même soin, même
effet, annoncé par AGPMSKILL op 4 sur chacun. Le champ `range` donne la zone : au-delà
de 1000, elle est centrée sur la cible, sinon sur le lanceur ; centaines 3 : sphère de rayon `area_r`
(mètres), 4 : boîte du lanceur vers la cible (`area_f1` de long, `area_f2` de large ; 600 × 300 par
défaut). Les monstres de la zone sont touchés comme la cible, les plus proches d'abord
(`max_target_num`, sinon 12 au plus). Une zone autour du lanceur se lance sans cible.

**Effets durables.** L'import range les colonnes de `SKILL_CONST.txt` qui suivent les marqueurs
`FACTOR_POINT`, `FACTOR_PERCENT` et `FACTOR_MAGNIFY` sous les préfixes `point.`, `percent.` et
`magnify.` (elles répètent les mêmes noms). Une compétence à durée (`duration`) laisse sur ses cibles
un effet : caractéristiques, PV et PM max, dégâts et défense physiques, précision, esquive, vitesses de
déplacement et d'attaque (`point.*` s'ajoute, `percent.*` multiplie), et dégâts sur la durée
(`dot_dmg_*` : dégâts fixes par élément toutes les `dot_damage_time` ms, moins 5 % par niveau d'écart).
`point.hp` et `percent.hp` (et mp) soignent à l'application. Un nouvel effet de la même compétence
remplace l'ancien. AGPMSKILL **op 4** annonce l'effet (identifiant de la compétence, porteur au champ 2,
champ 8 = liste de mini « effet » : TID, durée en ms, TID du lanceur) aux joueurs qui voient le porteur ;
**op 5** le retire (champ 8 : TID). Sur un joueur, les effets s'ajoutent aux facteurs (modèle et
équipement) renvoyés par AGPMCHARACTER op 1 ; ils tombent à la mort. Sur un monstre, ils modifient sa
défense, ses dégâts et ses vitesses d'attaque et de déplacement ; chaque coup de dégâts sur la durée part
en AGPMCHARACTER op 1 (facteurs du résultat). Un effet qui tuerait un monstre dont le lanceur est parti le
laisse à 1 PV.

**Passives.** Une compétence passive (unités du champ `attribute` à 9) pose un effet sans fin tant
qu'elle est connue ; il est recalculé à l'entrée en jeu, à l'apprentissage et à l'amélioration.

**États spéciaux.** La colonne `special_status` de `SKILL_SPEC.txt` donne des codes séparés par « ; »
(ceux à partir de 1000 règlent autre chose) : 1 étourdissement, 2 gel, 3 ralentissement, 4 invincibilité,
8 protection contre l'étourdissement, 9 halte (ni compétence ni attaque), 10 invisibilité, 12 et 13
invincibilité aux attaques normales ou aux compétences, 14 et 15 compétences ou attaques interdites,
16 sommeil, 20 immobilisation, 22 peur… Ils sont portés par l'effet de la compétence. Sur une cible, ils
prennent avec la chance `skill_rate` (toujours sans elle ; la protection contre l'étourdissement retire
encore sa propre chance) et durent `stun_time` ms pour un étourdissement, sinon `duration` ; l'effet dure
le plus long des deux. Sur soi, ils prennent toujours. L'état d'un personnage est l'union de ceux de ses
effets ; à chaque changement, AGPMCHARACTER **op 1** (identifiant au champ 1, bits au champ 17, u64 :
0x1 étourdi, 0x2 gelé, 0x4 ralenti, 0x8 invincible, 0x200 invisible, 0x80000000 compétences interdites,
0x100000000 attaques interdites, 0x200000000 endormi, 0x10000000000 immobilisé…) part aux joueurs qui le
voient. Étourdi, gelé ou endormi, un monstre ne bouge ni ne frappe ; immobilisé, il ne poursuit pas ; sous
« attaques interdites » ou la peur, il ne frappe pas ; un coup le réveille. Un joueur sous ces états ne
peut pas attaquer ou lancer de compétence ; les monstres délaissent un joueur invincible ou invisible.

**Effets de combat.** Les compétences dont `melee_attack` vaut 106, 109 ou 110 ajoutent `skill_rate` à
la chance de coup critique et `critical` aux dégâts critiques (pour cent des dégâts normaux, 100 au
moins) : un coup normal critique part avec le résultat 6. `reflect_melee_attack` 2 (bouclier, comme
Defense Hold) ajoute `skill_rate` à la chance de bloquer un coup de monstre, de même que
`point.physical_block` (80 % au plus) : résultat 9, sans dégâts. Invincible aux attaques normales, le
joueur esquive (résultat 7). `convert_type` 1 ou 2 change un coup reçu en PV ou en PM rendus (chance
`skill_rate`, sinon `special_status_rate` ; part `damage_convert_hp` ou `damage_convert_mp` des
dégâts) : résultat 1, puis les facteurs du joueur (op 1, champ 7).

**Renvoi des dégâts (contre-attaque).** `defense_melee_attack` 2 (Counter Attack, passive des berserkers :
2 % au niveau 1, 11 % au niveau 10) renvoie à son auteur, avec la chance `skill_rate`, les dégâts d'un coup
de monstre qui touche ou qui est bloqué : AGPMOPTIMIZEDCHARACTION avec le joueur pour attaquant, le monstre
pour cible, le résultat **12** (contre-attaque : `Archonia.exe` affiche alors la mention « C.Attack! » d'`IM_attack.png`) et les facteurs du monstre (PV restants, dégâts en négatif) ;
le monstre peut en mourir, l'expérience revient alors au joueur. Le bit 1 de `reflect_melee_attack` (renvoi
des dégâts reçus) n'est porté par aucune compétence des données d'Archonia.

**Maîtrises.** `SkillMastery_High.txt` (compétences supérieures) et `SkillMastery_Heroic.txt` (compétences
héroïques) rangent les compétences en arbres, par modèle de classe (96 guerrier humain… 1722 slayer, d'après
`SkillMasteryEvolution.txt`). Chaque compétence d'un arbre exige jusqu'à trois autres compétences à un niveau
donné (colonnes ConditionA à C, « TID;points », le client affiche « %s Skill %d Points Requirement ») et un
total de points dépensés (ConditionD : points de compétence dans l'arbre supérieur, « All Skills %d Points
Requirement », points héroïques dans l'arbre héroïque, « Total Heroic Skill Points Required ») ; les points
d'une compétence sont son niveau. Faute de quoi l'apprentissage est refusé : message système puis
AGPMEVENTSKILLMASTER op 6, résultat 1. Capacitate (525) exige ainsi Marching Song (526) au niveau 4.

**Compétences des monstres.** Chaque point d'apparition désigne une IA (`MobAITID` de
`spawndatatable.txt`, table `monster_ai` tirée d'`aidatatable.txt` des tables du serveur d'origine) : jusqu'à
cinq compétences « SkillN » (TID;niveau), chacune avec une condition sur les PV (« HP;100%;이하 » : au plus
100 % du maximum) et un nombre de lancements (« 무한 » sans limite, « 횟수;1 » une fois par vie) ;
« UseNormalAttack » 0 interdit les coups normaux. Une compétence offensive prête (relance écoulée, condition
remplie, cible à sa portée : `range`, rayon d'une zone autour du monstre, sinon portée du monstre) part à la
place d'un coup normal, ou de la poursuite quand la cible est déjà à sa portée : AGPMSKILL **op 7**
(lancement, TID au champ 4) aux joueurs qui voient le monstre, sans op 0 : la liste des compétences (op 0) ne
va qu'à leur propriétaire, et le client joue le lancement d'un personnage vu d'après ce seul paquet ; au bout de
l'incantation, **op 8** (cible = joueur, PV restants, dégâts en négatif) aux joueurs qui voient la cible. Les
dégâts suivent la formule des compétences des joueurs, à partir des dégâts physiques et élémentaires du
monstre (`CharacterDataTable.txt`) ; une zone touche les joueurs voisins ; effet durable, dégâts sur la durée
(AGPMCHARACTER op 1 à chaque coup) et états spéciaux prennent sur chaque cible. Un monstre étourdi, gelé ou
endormi perd son incantation ; une cible partie à plus de 5000 unités (limite de la poursuite) y échappe. Une compétence sans SKILL_SPEC, sans incantation ni relance, qui affaiblit
(`buff_type` 1 : « <MOB>Water Elemental Skill »…) accompagne les coups normaux qui touchent, avec la chance
`skill_rate`. Les tables ne disent pas à quel rythme le serveur d'origine lançait ces compétences : ArchCore
lance chacune dès qu'elle est prête. Midas (TID 40, IA 403) lance ainsi Insanity (181) toutes les 6 s.

Restent à faire : défense ignorée (bits 4 et 8 de `reflect_melee_attack`), critiques des compétences,
purges, partage des dégâts (`divide` : Divine Protection, Martyr's Mark) ; gain des points héroïques et de
charisme, que les compétences héroïques exigent aussi.

## Portails de téléportation (type 0x31, AgpmEventTeleport)

Les portails sont des objets du décor (`obj*.ini`) qui portent l'événement 8 et le nom d'un point de
`TeleportPoint.ini` (`PointName`) ; aucun PNJ ne téléporte. AGPMEVENTTELEPORT (0x31, drapeaux 1) : op,
événement (EVENT_BASE de type 1, objet), CID, nom du point (CP949, 33 octets), position, points désactivés.

- **op 4** : le client s'adresse au portail. À moins de 1600 unités, le serveur répond **op 5** (le client
  ouvre la liste des destinations) ; plus loin, il fait courir le joueur vers l'objet et répond dès qu'il est
  à portée. Une marche demandée par le joueur annule l'attente.
- **op 0** : le client choisit sa destination (nom du point). Le trajet est permis si l'un des groupes visés
  par le point de départ (`TargetGroup`) compte le point d'arrivée (`Group`) et si la race du joueur est
  admise aux deux bouts (`UseType` : 1 humain, 2 orc, 4 elfe de lune, 8 dragonien ; 0 toutes). Le prix
  (`TeleportFee.txt`, que le client affiche aussi) est pris sur l'argent porté ; le serveur envoie **op 1**
  (CID, position d'arrivée), déplace le joueur, puis **op 8** (chargement terminé).

On arrive sur un cercle de rayon `RadiusMin`..`RadiusMax` autour de l'objet qui porte le point d'arrivée,
au premier des huit points espacés de 45° (à partir d'un angle tiré au hasard) qui n'est pas sur un sol
infranchissable.

**Zones de combat entre joueurs.** Les 19 points `RegionType=1` (tous `SpecialType=1`) : arène de guilde,
Battle Square, salles et entrées des donjons. Dans Archonia (AgcmUIEventTeleport) :

- choisir une telle arrivée ouvre la boîte `TELEPORT_ConfirmPvPZone` (OK / Annuler) ; OK envoie l'op 0 ;
- depuis un tel point, la liste ne propose que « Retour » (`Teleport_ReturnOnly`), qui envoie l'**op 7**
  (événement, CID). Le module AgpmEventTeleport note sur le personnage le point de départ de chaque trajet
  réussi ; ArchCore y ramène le joueur (sans frais), ou au village de rattachement s'il n'en a pas (après une
  reconnexion, par exemple). L'op 7 est ignorée depuis un point qui n'est pas `SpecialType=1`.

**Refus.** Le client n'envoie pas l'op 4 en mode combat (10 s après le dernier coup donné ou reçu, ou une
compétence offensive) : message `Teleport_CombatMode` dans la discussion ; ni métamorphosé (champ 18 du
personnage) : `Teleport_TransformChar`. ArchCore ignore de même les ops 0, 4 et 7 d'un joueur mort ou en
combat. Un départ annulé est signalé par l'**op 2** (fermeture de l'écran de chargement).

## Banque (types 0x36, 0x04 et 0x02)

La banque est commune aux personnages d'un même compte. Les gardiens (« Safe Keeper », 18 PNJ) portent la
fonction 20, sans données, envoyée dans leur vue. AGPMEVENTBANK (0x36, drapeaux 1 : op, événement, CID) :
le client demande l'ouverture par l'**op 0** ; le serveur, si le gardien est à portée, répond **op 1**.

La banque compte 4 pages de 7 lignes × 4 colonnes ; la première est toujours ouverte, les suivantes selon
le champ 23 du personnage (pages en plus). Les objets en banque ont le **statut 100**, leur case est au
**champ 8** d'AGPMITEM (au lieu du champ 7 de l'inventaire) ; ils partent avec l'inventaire à l'entrée en
jeu. Pour déposer ou ranger, le client envoie AGPMITEM op 2 avec le statut 100 et la case voulue ; pour
retirer, op 2 avec le statut 2 et une case d'inventaire (ou aucune : première case libre). Poser sur une
pile du même objet les réunit. Le serveur répond par op 2 (nouveau statut et case) ; il refuse tant que la
fenêtre n'est pas ouverte auprès d'un gardien proche.

L'argent passe par AGPMCHARACTER **op 7** : le champ 9 porte le montant déposé (négatif : retiré). Le
serveur renvoie op 1 avec l'argent sur soi (champ 8) et en banque (champ 9). En base, `account_bank`
garde l'argent et les pages du compte ; les objets restent dans `character_items` (statut 2), rattachés
au personnage qui les a déposés et passés à celui qui les retire.

**Page en plus.** Depuis la fenêtre de la banque, le client envoie AGPMITEM **op 22** (CID au champ 4). La
page suivante se paie sur l'argent porté, au prix de `bankslotprice.txt` (1 000 000 pièces pour la
deuxième ; -1, les suivantes, ne s'achètent pas contre de l'or). Le serveur répond AGPMCHARACTER op 1
(argent au champ 8, pages en plus au champ 23) et le message système 30 (page ajoutée), ou 29 si la banque
a déjà ses 4 pages.

## Guildes (types 0x15 et 0x39, AgpmGuild et AgpmEventGuild)

AGPMGUILD (0x15, drapeaux 4) a 25 champs : op, CID (ou code d'un message système), nom de la guilde (32),
chef ou joueur (48), 8 × INT32, rang, date de création (INT64), taille, union, mot de passe (12), annonce,
statut, victoires, nuls, défaites, points, 6 sous-paquets (dont le **membre**, champ 16 : nom, rang, date
d'adhésion, niveau, TID, en ligne), emblème (TID, couleur), vainqueur. Rangs : 1 membre, 2 demande
d'adhésion, 3 demande de départ, 10 chef, 11 second.

| Op | Sens | Rôle |
|---|---|---|
| 0 | ↔ | Création (nom en lettres et chiffres, mot de passe) ; le serveur renvoie la guilde (nom, chef, date, taille, annonce) |
| 1 | ↔ | Invitation par le chef ou le second (invité au champ 16) ; l'invité la reçoit avec le nom de la guilde et de l'inviteur |
| 2 | → | Refus de l'invité ; le chef reçoit le message 14 |
| 3 | ↔ | Adhésion : membre si invité, sinon demande d'adhésion (rang 2) que le chef accepte par le même op ; le serveur diffuse le membre |
| 4 | ↔ | Départ : un membre le demande (rang 3, message 50 avec son nom, à toute la guilde) ; diffusé quand il se fait |
| 29 | → | Départ accepté par le chef ou le second ; sans réponse, il se fait au bout d'un jour |
| 5 | ↔ | Exclusion par le chef ou le second (le chef ne peut pas être exclu) |
| 6 | ↔ | Dissolution par le chef, avec le mot de passe de la création |
| 8 | ← | Lien d'un personnage (CID) à sa guilde, avec son membre : à l'entrée en jeu, aux autres membres (en ligne, hors ligne) et aux joueurs qui le voient |
| 9 | ↔ | Annonce (champ 10), par le chef ou le second |
| 10 | ← | Message système : code (champ 1), textes (2, 3), nombre (4, premier des 8) |
| 7 | ↔ | Agrandissement par le chef d'une guilde de 20 membres au moins : taille suivante (champ 7, dizaine supérieure) au prix de `guildmaxmember.txt` ; la nouvelle taille va à toute la guilde |
| 28 | ↔ | Liste des guildes : page demandée (champ 21, mini GUILD_LIST, page au champ 1) ou guilde cherchée (champ 2) ; réponse : nom cherché (2) et GUILD_LIST (total, page, nombre en INT16, 19 entrées GUILD_LIST_ITEM) |
| 32, 33 | ↔ | Emblème (champs 22 TID et 23 couleur) choisi par le chef d'une guilde de 10 membres ; 33 l'achète malgré une autre guilde au même emblème ; diffusé à la guilde, et les liens (op 8, champs 22-23) aux joueurs qui voient ses membres |
| 48 | ↔ | Second et passation de chef ; seule l'op est un champ, la suite est brute (voir plus bas) |

Créer une guilde demande le niveau 20 et le prix de la première ligne de `guildmaxmember.txt` (50 places
pour 10 000 pièces). Codes utilisés : 0 nom pris, 1 niveau, 2 argent (nombre : prix), 4 caractères, 5 déjà
membre, 6 guilde pleine, 7 adhésion impossible, 8 départ impossible (le chef), 9 réservé au chef,
10 mauvais mot de passe, 11 création, 12 dissolution, 13 adhésion, 14 refus, 15 départ, 16 exclusion,
17 joueur déjà dans une guilde, 44 or insuffisant pour agrandir, 45 crânes insuffisants, 47 guilde cherchée
introuvable, 48 demande d'adhésion, 50 demande de départ, 56 à 59 emblème (crânes, or ou argent liquide, fond
manquant, même emblème qu'une autre guilde : seul 59 a un message dans Archonia, qui propose alors l'op 33).

**Liste des guildes** (FUN_140203630 d'Archonia). Les guildes sont prises dans l'ordre de leur création,
19 par page ; une page vide n'a pas de réponse. Entrée : nom, chef, second, niveau, classement, membres,
taille, (inconnu), emblème (TID, couleur), points de combat ; Archonia écrit 0 au niveau et au champ inconnu,
et le classement vient d'une donnée de la guilde non identifiée : ArchCore envoie 0. Une recherche rend la page
qui contient la guilde.

**Emblème** (guildmark.txt, `FUN_1402050f0`). TID = fond | motif << 8 | symbole << 16 (numéros des éléments),
couleur = indice de couleur de chaque couche aux mêmes décalages. Le prix est la somme des éléments choisis, en
or et en argent liquide ; le fond est obligatoire.

**Second et passation (op 48).** Paquet d'Archonia : en-tête, drapeaux 1, op 48 à +0x17, sous-op à +0x18,
puis une structure ; taille = structure + 1, pied 0x6B.

| Sous-op | Sens | Taille | Contenu |
|---|---|---|---|
| 2 | → | 0x80 | +0x19 émetteur, +0x4a membre, +0x7c rang (11 nommer second, 1 retirer ce rang) |
| 3 | ← | 0xa0 | +0x19 guilde, +0x4a chef, +0x6b membre, +0x9c rang : le membre confirme |
| 4 | → | 0x84 | +0x19 guilde, +0x4a soi, +0x7c rang, +0x80 réponse (1 accepte) |
| 5 | → | 0x7b | +0x19 émetteur, +0x4a second : passation |
| 6 | ← | 0x9c | +0x19 guilde, +0x4a chef, +0x6b second : le second confirme |
| 7 | → | 0x80 | +0x19 guilde, +0x4a soi, +0x7c réponse |
| 8 | ← | 0x70 | +0x19 guilde, +0x3a membre, +0x6c nouveau rang (1, 10 chef, 11 second), à toute la guilde |

Comme les boutons d'Archonia : seul le chef nomme ; le membre a le niveau 20, est en ligne et la guilde n'a pas
de second (ou c'est le second, pour lui retirer ce rang) ; la passation ne va qu'au second, en ligne, hors
combat de guilde. À la passation, l'ancien chef reçoit le rang 1 puis le second le rang 10.

À l'entrée en jeu, le membre reçoit sa guilde (op 0), chaque membre (op 3) et son lien (op 8).
Les gestionnaires de guilde (12 PNJ) portent la fonction 23, sans données ; AGPMEVENTGUILD (0x39) :
op 0 (événement) ← op 1 (accord, le client ouvre la création). Discussion : le type 3 va aux membres de la
guilde, le type 4 aux membres du groupe.

Pas encore faits : combats de guildes, alliances et hostilités, entrepôt.

## Groupes (types 0x08 et 0x09, AgpmParty)

Deux types de paquets : **0x09** (gestion, drapeaux 1 : opération, CID de l'auteur, CID de la cible, nom
sur 49 octets) et **0x08** (le groupe, drapeaux 2 : opération, identifiant, membres au plus, nombre de
membres, CID d'un membre, liste des CID en u32, facteurs, zone d'effet, répartition de l'expérience,
bonus, répartition des objets).

| Gestion (0x09) | Sens |
|---|---|
| 0 | invitation : le client envoie la cible (CID ou nom) ; l'invité reçoit l'op 0 (auteur, nom de l'auteur) |
| 1 | refus de l'invité ; l'auteur reçoit l'op 1 (nom de l'invité) |
| 2 | acceptation ; l'auteur reçoit l'op 2 (nom de l'invité) |
| 3 | départ |
| 4 | exclusion par le chef ; l'exclu reçoit l'op 4 |
| 11 | passage de chef ; le groupe reçoit l'op 11 (auteur = nouveau chef) |
| 5, 6, 7, 8, 10 | échecs : déjà dans un autre groupe, déjà membre, groupe plein, pas chef, joueur absent |

Un groupe compte cinq membres au plus, chef en tête ; un joueur garde seize invitations en attente au plus.
À l'acceptation, le groupe est formé (deux membres) ou agrandi. Le nouveau membre reçoit la fiche de chaque
autre membre, l'**op 0** du type 0x08 (le groupe et sa liste) puis un **op 3** par membre ; les autres
reçoivent sa fiche et un op 3 (lui). Au départ, le groupe et lui reçoivent l'**op 4** (lui), lui l'**op 1**
(groupe retiré) ; un groupe réduit à un membre est dissous. Le chef règle la répartition de l'expérience
(**op 5**, champ 8 : 0 selon les dégâts, 1 selon le niveau) et celle des objets (**op 6**, champ 10).

Les membres d'un groupe se voient à toute distance ; un membre déconnecté quitte le groupe. L'expérience
d'un monstre tué se partage entre les membres vivants assez proches (5 × 5 secteurs autour du monstre),
selon les dégâts infligés ou selon le niveau : cette répartition est un ajout d'ArchCore, le serveur
d'origine ne la décrit pas. L'or et les objets tombent encore au sol pour le tueur.

## Échange entre joueurs (type 0x07, AgpmPrivateTrade)

Drapeaux 2 : op, CID, CID de l'autre joueur, IID, TID, argent (INT64), cases d'origine et de destination,
argument.

| Op | Sens | Rôle |
|---|---|---|
| 1 | client | A demande un échange à B (champ 2) ; tous deux passent au statut d'action 7 |
| 3 | serveur | B reçoit la demande (champ 1 = A) |
| 4 / 25 | client | B accepte / refuse |
| 6 | serveur | Début, à chacun (champ 1 = soi, champ 2 = l'autre) |
| 9 / 10 | client | Mettre / reprendre l'objet d'IID donné (champ 3) |
| 23 / 24 | client, serveur | Argent proposé (champ 5) ; l'autre reçoit op 24 |
| 16, 17, 18, 19 | | Verrouiller (soi ← 16, l'autre ← 17), déverrouiller (18 / 19) |
| 20 | serveur | Les deux côtés sont verrouillés |
| 21, 22 | | Prêt (soi ← 21, l'autre ← 22) ; tous deux prêts : échange |
| 0 | serveur | Résultat (champ 8 = 8 : échange fait) |
| 5 | | Annulation, des deux côtés |

Un objet mis dans l'échange passe, chez son propriétaire, au statut 6 (grille d'échange : 1 × 3 × 4) par
AGPMITEM op 2 ; l'autre le reçoit par AGPMITEM op 0, statut 6, propriétaire = le premier. Il reste en base
et dans l'inventaire jusqu'à l'échange, et ne peut être ni déplacé, ni utilisé, ni détruit. À l'échange, le
serveur vérifie l'argent et la place dans les inventaires, transfère les objets (AGPMITEM op 1 chez l'ancien
propriétaire, op 0 statut 2 chez le nouveau) et l'argent (AGPMCHARACTER champ 8). Une déconnexion annule
l'échange.

## Quêtes (types 0x13 et 0x3E)

**Données.** `questtemplate.ini` (tabulé) décrit chaque quête : niveaux, race et classe admises, quête
précédente, quête « personnelle » (`Self_Quest`, prise depuis le journal), objectifs (monstres A et B à tuer,
objet à rapporter), objets laissés par les monstres de la quête (`DropItem*` : TID, pour cent, nombre),
récompenses (objets, objets au choix, argent, expérience). `questgroup.ini` donne, pour chaque groupe de PNJ
(`QuestGroupID` de npc.ini), les quêtes qu'il propose et celles qu'on lui rend. Le client français n'en
connaît qu'une soixantaine (quêtes d'événement et de haut niveau).

**Protocole**, relevé dans le client x64. AGPMQUEST (drapeaux 2) : op, CID, événement du PNJ (EVENT_BASE),
quête (mini : TID, monstres A et B tués, objectifs remplis, valeur, texte de 49 octets), TID, deux entiers,
résultat, récompense choisie, trois blocs de drapeaux.

| Op | Sens | Rôle |
|---|---|---|
| 0 | client | Prendre la quête du champ 4 (événement du PNJ au champ 2, absent pour une quête personnelle) |
| 1 | serveur | Réponse : champ 4 = TID, champ 6 = 1 (réussi) ou 0 ; le client l'inscrit au journal |
| 2, 15 | client | Terminer (champ 7 = récompense choisie) |
| 3 | serveur | Réponse (champs 4 et 6) |
| 4 | serveur | Quête du journal (mini), à l'entrée en jeu |
| 5 | serveur | Drapeaux des quêtes terminées, à l'entrée en jeu : champs 8, 9, 10 de 0x800 octets ; la quête TID est le bit TID % 16 384 du bloc TID / 16 384 (octet bit / 8, bit de poids faible d'abord) |
| 6 | client | Abandonner (champ 4) |
| 7 | serveur | Réponse (champs 4 et 6) |
| 10 | serveur | Progression (mini) : le client affiche « monstre n/total » |

AGPMEVENTQUEST (0x3E, drapeaux 1 : op, événement, CID, entier) : le client demande la fenêtre des quêtes d'un
PNJ (fonction 27) par l'op 0 ; le serveur la lui ouvre en renvoyant l'op 0.

**Règles.** Une quête se prend auprès d'un PNJ dont le groupe la propose, ou depuis le journal si elle est
personnelle ; elle se refuse si elle est déjà en cours ou terminée, si le journal est plein (60), hors des
niveaux, de la race ou de la classe, ou sans la quête précédente. Chaque monstre tué fait avancer les quêtes
qui le comptent et peut laisser l'objet de la quête (selon son pour cent, jusqu'au nombre demandé). La quête
se termine (auprès d'un PNJ qui la reçoit, ou depuis le journal) quand les monstres sont tués et les objets
réunis : les objets demandés sont repris, les récompenses données (objets, argent, expérience). En base,
`character_quests` garde les quêtes en cours (compteurs) et terminées.

## Refus des options (type 0x02, op 0x11)

L'onglet « Fonction » des options règle des refus que le client envoie par AGPMCHARACTER **op 0x11** (champ 1 :
CID, champ 22 : drapeaux `INT32`), à l'entrée en jeu et à chaque case : 0x01 échanges, 0x02 invitations de
groupe, 0x10 invitations de guilde (0x04 combat de guilde, 0x08 combat, 0x40 alliance ou hostilité, 0x80 ami
attendent leur système). Le serveur garde la valeur (`characters.option_flag`), la renvoie par la même op et
arrête à la source ce qui vise un joueur qui refuse ; l'auteur reçoit le message d'Archonia :

| Demande | Réponse à l'auteur | Phrase (`uimessage.txt`) |
|---|---|---|
| Invitation de groupe | AGPMPARTYMANAGEMENT op 14 (champ 2 : invité, champ 3 : son nom) | `Option_Party_Off_User` |
| Invitation de guilde | AGPMGUILD op 10, message système 42 | `Option_Guild_Off_User` |
| Demande d'échange | AGPMPRIVATETRADE op 25 (champ 1 : CID visé) | `Option_Trade_Off_User` |

## Niveau MJ (type 0x12, AgpmAdmin)

Drapeaux de champs sur 2 octets : op, CID, puis 13 mini-paquets. L'op 0 avec, au champ 2, un
mini-paquet « données MJ » d'op 1 (CID, nom, niveau MJ en INT16, nom du compte) enregistre le niveau
du personnage. Le CID doit être celui d'un personnage déjà connu du client : ArchCore l'envoie juste
après le paquet personnage de l'entrée en jeu.

## Extensions ArchRust (type 0xAC)

Type propre à ArchCore. Le client d'origine n'aiguille que les types 0x01 à 0x77 et ignore les autres ;
un client ArchRust l'utilise après la poignée de main pour négocier ses extensions (`HELLO` /
`HELLO_ACK`) puis échanger des messages d'AddOn. Le serveur n'en envoie jamais à une connexion qui n'a
pas envoyé `HELLO`. Spécification complète : [[Extensions ArchRust|FR-Extensions-ArchRust]].

## Reconnexion automatique (type 0x63, AgpmReconnect)

AGPMRECONNECT (drapeaux 4) : op, CID, nom (CHAR[48]), compte (CHAR[1024]), mot de passe (CHAR[1024]), clé,
accord. Le client d'origine (AgcmReconnect) :

- garde la clé de chaque **op 2** reçue (champ 5 non nul). ArchCore en envoie une à la fin de l'entrée en
  jeu (accord 0), tirée au hasard ;
- quand la connexion au monde se perd, personnage en jeu : ouvre `UI_ReconnectHold`
  (`ReconnectingServerShow`), retente toutes les 5 s pendant 60 s la connexion à l'adresse du monde et
  affiche « Attempting to reconnect to the game server... » ;
- une fois connecté, envoie l'**op 1** (CID, nom, compte, mot de passe, clé) à la place de l'entrée normale
  (type 0x03 op 2) ;
- sur une **op 2** avec accord 1 (champ 6), reprend la partie sur la nouvelle connexion, garde la nouvelle
  clé et ferme la fenêtre ; au bout de 60 s, abandonne (« Failed reconnecting to the server. »).

ArchCore garde dans le monde, immobile, un joueur dont la connexion se coupe (60 s, le délai du client). Une
reprise valable (clé, CID, nom et compte ; le mot de passe n'est pas contrôlé) rattache le personnage à la
nouvelle connexion, envoie l'op 2 avec une nouvelle clé et l'accord 1, puis renvoie ce que le joueur voit
(retrait puis nouvel envoi des PNJ, monstres, joueurs et objets au sol, position et facteurs). Une reprise
refusée ferme la connexion. Passé le délai, le personnage quitte le monde comme à une déconnexion ; une
nouvelle entrée par le serveur de connexion le remplace aussitôt.

## Types connus

| Type | Module | Type | Module |
|---|---|---|---|
| 0x00 | Système | 0x15 | Guilde |
| 0x01 | Configuration | 0x18 | Mondes |
| 0x02 | Personnage | 0x2F | Retour à l'écran de connexion |
| 0x03 | Gestion des personnages | 0x3C | Événements (fonctions des PNJ) |
| 0x04 | Objets | 0x40 | Déplacement |
| 0x05 | Compétences | 0x41 | Actions |
| 0x08 | Groupe | 0x42 | Vue optimisée |
| 0x0D | Connexion | 0x44 | Message système |
| 0x0F | Discussion | 0x48 | Chiffrement de démarrage |
| 0x11 | Horloge (AgpmTimer) | 0x60 | Titres |
| 0x13 | Quêtes | 0x63 | Reconnexion |
| 0x32 | Boutiques des PNJ | 0x37 | Dialogue des PNJ |
| 0x3B | Maître des compétences | 0x36 | Banque |
| 0x31 | Portails de téléportation | 0x15 | Guildes |
| 0x39 | Gestionnaire de guilde | 0x16 | Conversion des objets |
| 0x38 | Forgeron (conversion) | 0x4B | Raffinage |
| 0x3F | Sphère d'alchimie (raffinage) | | |
| 0x07 | Échange entre joueurs | 0x3E | Quêtes des PNJ |

Les schémas complets des 86 modules sont dans `data/re/packet-schemas.json`.
