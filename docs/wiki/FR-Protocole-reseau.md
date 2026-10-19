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
les croit à 0 PV ; le champ 6 vaut `0x04` (arrêté) et le champ 9 (destination) est absent. La hauteur
envoyée est celle des tables (souvent 0) : le client pose la créature sur le terrain. Un monstre sorti
du rayon de vue (× 1,2) est retiré par AGPMCHARACTER op 2.

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
recalcule la visibilité : un paquet personnage **op 0** ajoute un joueur entré dans le rayon de vue,
l'**op 2** (CID au champ 1) retire un joueur sorti du rayon ou déconnecté. La position est
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

À la mort d'un monstre, AGPMCHARACTER op 1 avec le champ 11 (statut d'action) à 2 ; le cadavre est
retiré 20 s plus tard (op 2), puis le monstre réapparaît à sa place avec tous ses PV. Le tueur reçoit
l'expérience du modèle (`Exp` de CharacterDataTable.txt × taux `exp`) dans ses facteurs (op 1, champ 7)
et un message système ; l'expérience se compte par niveau (`LevelUpExp.txt`), et une montée de niveau
est signalée par AGPMCHARACTER **op 5**. Seuls les modèles dont le type porte les bits « attaquable »
(0x10000) et « ciblable » (0x20000) peuvent être visés : les créatures d'ambiance (type 8 : coqs,
poules…) ne le sont pas.

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
précision (`Physical_ATKRate`), PV, et pour une arme sa vitesse et sa portée d'attaque. Un refus donne
un message système et remet l'objet à sa place. En base, un objet porté a le statut 1 et sa partie dans
`layer`. L'équipement d'un joueur est envoyé (op 0, statut 3) à ceux qui commencent à le voir.

Dans l'inventaire, poser une pile sur une pile du même objet (op 2, statut 2, case de la cible) les réunit
si le total tient dans une pile : la source disparaît (op 1), la cible est mise à jour (op 2). AGPMITEM
**op 17** sépare une pile (quantité au champ 5, case libre au champ 7) : la nouvelle pile arrive par op 0,
l'ancienne est mise à jour par op 2. **Op 21** détruit un objet de l'inventaire (op 1 en réponse).

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

## Parchemins de téléportation (type 0x04)

Un parchemin (« Passport ») est un objet utilisable de `Type` 4 ; son `SubType` donne sa sorte : 0 retour au
village, 1 aller-retour, 2 destination fixe (colonne `Destination` : x;y;z), 3 retour au village immédiat. Il
s'utilise par AGPMITEM op 3 comme une potion (un exemplaire consommé, confirmation op 4), puis le serveur
téléporte le joueur (AGPMOPTIMIZEDCHARMOVE et AGPMCHARACTER). Le « village » est le point de résurrection de
la région de rattachement, comme pour la résurrection. Le retour au village (0 et 1) est refusé en combat. L'aller-retour mémorise
l'endroit quitté et l'annonce par AGPMITEM **op 9** (champ 4 = CID) ; le client demande à y revenir par
**op 6**, refusé en combat ; le serveur répond **op 10** (retour désactivé) et y ramène le joueur.

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

Les schémas complets des 86 modules sont dans `data/re/packet-schemas.json`.
