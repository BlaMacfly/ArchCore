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

Entre la position de chargement (op 8) et l'entrée en jeu, le serveur envoie un paquet AGPMTIMER (drapeaux 1) : au
champ 0 la **vitesse du temps** (u8, ce n'est pas une opération ; ignorée si 0), au champ 1 le temps du jeu en
**millisecondes** (u64 ; ignoré si 0). Le client avance ce temps de `vitesse` ms par ms réelle et en tire l'heure par
`(ms / 3 600 000) % 24`, **sans correction de fuseau** : elle pilote le cycle jour/nuit, la musique et l'ambiance de
zone (`skyset.ini`). Archonia part de 12 h et de la vitesse 6 tant qu'il n'a rien reçu. Les champs 2 (heure du serveur,
secondes Unix, UTC) et 3 (décalage du fuseau, secondes) sont lus par la fenêtre des boss d'Archonia (« Server Time »,
comptes à rebours) : ArchCore les remplit d'après `World.TimeZone` (décalage 0 en `utc`).

ArchCore tient une horloge **globale** (choix d'ArchCore) : vitesse `World.TimeSpeed` (6 par défaut, une journée en
4 h réelles) ; avec `World.TimeStart` = h (0 à 23), le temps vaut h heures au démarrage du serveur ; avec -1 (défaut),
il vaut l'horloge de la machine (`World.TimeZone` : époque Unix décalée du fuseau local, ou UTC) multipliée par la
vitesse, la même d'un démarrage à l'autre. Deux joueurs entrés à des moments différents voient la même heure. Le
paquet est renvoyé à tous les joueurs en jeu toutes les 10 minutes (le client recale son horloge). `archcore sim`
affiche la valeur reçue et l'heure en jeu correspondante. Voir la [[configuration|FR-Configuration]].

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
le joueur est à moins de 600 unités du PNJ.

**Portées d'usage.** Le code partagé d'Archonia vérifie, à la réception de chaque module, la distance au sol entre
le joueur et le PNJ ou l'objet (`FUN_140304580`, portée fixe par module). ArchCore reprend ces portées, mesurées sur
la position suivie à l'instant (marche en cours comprise), plus 50 unités de marge :

| Portée | Fonctions |
|---|---|
| 600 | dialogue (21), boutique (9, ouverte depuis le dialogue), commandant de siège (33), superviseurs de la Battle Square |
| 800 | banque (20), conversion (22), guilde et entrepôt de guilde (23, 35), maître des compétences (25), coiffeur (29) |
| 1600 | réparation (17), production (24), quêtes (27), maison des ventes (28), avis de recherche (32), taxes (34), rémission, portails (8), sphères d'alchimie (26) |

## Objets (type 0x04, AgpmItem)

Drapeaux de champs sur **4 octets**, 27 champs. Les principaux :

| Champ | Contenu |
|---|---|
| 0 | op (INT8) : 0 ajout, 1 retrait, 2 mise à jour |
| 1 | statut (INT8) : 2 inventaire, 10 boutique d'un PNJ, 13 sac du familier, 51 inventaire Cash |
| 2 | IID, identifiant de l'objet |
| 3 | TID, modèle de l'objet |
| 4 | CID du propriétaire |
| 5 | quantité |
| 7 | case : mini à 3 INT16 (sac, ligne, colonne) |
| 23 | objet Cash : mini (« en cours d'utilisation », temps restant…), voir l'inventaire Cash |

Le client crée l'objet à partir de son propre modèle, lu dans `ItemTemplateAll.ini`. Si le TID lui
est inconnu, il ignore l'objet sans rien afficher.

À l'entrée en jeu, l'inventaire n'est pas envoyé en paquets séparés. Les paquets AGPMITEM complets
sont mis bout à bout dans le **champ 6** du paquet de vue 0x42 **op 2**. La grille d'inventaire du
client fait 5 sacs de 5 × 5 cases (grille de type 1).

**Or jeté** : **op 15** (client, boîte « Inventory_DropMoneyYes » de l'inventaire : champ 4 = CID, champ 5 = montant ;
pas de réponse). La somme (1 au moins, pas plus que l'or porté, pas pendant un échange) est retirée de l'or du joueur
(op 1 du personnage, champ 8) et tombe à ses pieds en une pile d'or (TID 268) **sans propriétaire** (champ 4 absent),
ramassable par tous (op 11, op 12) ; elle disparaît au bout de 45 s. **Op 28** (UPDATE_COOLDOWN, serveur : 3 TID, 4 CID,
5 entier laissé à 0, 18 temps restant en ms) : recharge des objets d'un modèle ; envoyée à 0 par la commande MJ
`/recast`. Op 8 (annuler le parchemin de retour) n'a aucun appelant dans le client ; op 29 (endurance du familier) :
format non établi ; ni l'une ni l'autre ne sont gérées.

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

**Boutique des Shrines (épisode 8).** Un objet peut coûter, en plus de l'or, des points de charisme (colonne 29
d'itemdatatable, « NPC Charisma Price », table `item_data.npc_charisma_price`) et des Shrine Coins (colonne 35, « NPC
Shrine Coin », objets TID 53000 de l'inventaire, toutes piles comptées), multipliés par la quantité. Archonia ouvre
pour tout achat la fenêtre `UI_SHRINEITEM_BUY_CONFIRM` qui affiche les trois prix, mais n'envoie aucun prix : le
serveur relit le modèle. Contrôles dans l'ordre : charisme (résultat **10**, « NPCTMSG_NotEnoughCharisma »), pièces
(**11**, « NPCTMSG_NotEnoughShrineCoin »), or (3), place (4) ; puis charisme, pièces, or et objet sont enregistrés
ensemble (facteurs du personnage op 1, AGPMITEM op 2 ou op 1 des piles de pièces). La taxe et le doublement du
meurtrier ne portent que sur l'or (choix d'ArchCore). Les monnaies d'Archonia (Archonium Coin, Siege Token,
Bloodmark, résultats 12 à 14) sont ignorées. Aucune liste de vente des données ne propose ces objets : le mécanisme
sert dès qu'une boutique en contient.

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

Anti-triche (`world/anticheat.rs`) : une position fournie au champ 1 n'est acceptée que dans la limite d'un crédit de
distance (vitesse de course courante × `AntiCheat.SpeedTolerance` + `AntiCheat.MoveSlack`, jusqu'à 5 s de course pour
les paquets retardés) ; au-delà, le paquet est ignoré et le serveur renvoie au client la position qu'il suit (0x40
champs 1 et 2 égaux, drapeaux `0x0C`, puis AGPMCHARACTER op 1 champ 5), comme une téléportation. Les MJ ne sont pas
contrôlés. Toutes connexions confondues, au-delà de `AntiCheat.MaxPacketsPerSecond` paquets par seconde (rafale
`AntiCheat.PacketBurst`) les paquets sont ignorés, et une inondation qui dure ferme la connexion.

## Discussion (type 0x0F, AgpmChatting)

Drapeaux de champs sur 1 octet : op, CID émetteur, type de canal, CID cible, nom émetteur (49),
nom cible (49), message (bloc mémoire).

- Le client envoie l'op 0 avec son CID, le type (0 normal, 5 chuchotement avec le nom cible) et le
  message **sans zéro final**, de moins de 256 octets. Une commande MJ est un message normal qui
  commence par `/`.
- Archonia envoie **tout** en type 0, le mode de la barre ajoutant son préfixe au texte : `/p` groupe, `/g`
  guilde, `/s` cri local ; le joueur tape aussi `/race` (ou `/racetrumpet`), `/scream`, `/warn`. ArchCore lit ces
  préfixes (casse ignorée) **avant** les commandes MJ ; une commande inconnue d'un joueur sans droits MJ est ignorée
  sans réponse. Seul le chuchotement arrive dans son type (5). Les types 3 et 4 envoyés directement restent acceptés.
- `/s` : type 1 « [Shout] » aux joueurs à moins de `Chat.ShoutRange` (0 : deux fois `World.ViewRadius`).
- Cadence (valeurs du code partagé d'Archonia) : un message toutes les 100 ms au plus, le surplus est ignoré ; les
  commandes MJ n'y sont pas soumises.
- Si le joueur a choisi une couleur de discussion, le client ajoute `\x01RRGGBB` à la fin du message.
  ArchCore le retire avant d'interpréter une commande et le garde pour l'affichage.
- À la réception, le client refuse un message de plus de 512 octets. Types et affichage d'Archonia : 0 normal, 1
  « [Shout] », 3 guilde, 4 groupe, 5 et 6 chuchotement, 7 (monde entier), 11 et 12 (annonces) en « [Notice] », 8 à
  10 (système) en « [System] » (une ligne répétée en moins de 500 ms y est ignorée), 13 annonce de guilde, 16 siège,
  17 jaune, 19 trompette de race, 20 cri des éléments ; 17, 19 et 20 sans en-tête ni nom. ArchCore répond au MJ et
  envoie ses messages système en type 7, ses annonces en type 10 (noms corrigés le 28 octobre 2026 :
  `WHOLE_WORLD`, `SYSTEM_LEVEL3`, `NOTICE_LEVEL1` ; valeurs envoyées inchangées).
- Chuchotements : destinataire absent → **op 4** (champ 1 = CID de l'émetteur, champ 5 = nom du destinataire ; le
  client écrit « `<nom>` is not connected. » dans l'onglet système). Le client envoie l'**op 1** (champ 1 = CID) pour
  bloquer les chuchotements reçus et l'**op 2** pour les débloquer ; ArchCore les renvoie (le client note le blocage)
  et garde le blocage jusqu'à la déconnexion. Un chuchotement vers un joueur qui bloque donne à l'émetteur l'**op 3**
  (champ 5 = nom : « `<nom>` is blocking whispering. ») et n'est pas transmis.

## Cris de zone (type 0x4C, AgpmAreaChatting)

Du serveur au client seulement (Archonia n'en envoie jamais). Drapeaux sur 1 octet : 0 op (seule l'op 0 compte),
1 CID de l'émetteur, 2 genre de cri (I8 : 1 race, 2 monde, 3 tous les mondes ; ignoré par le client), 3 I8 (1,
ignoré), 4 type AGPMCHATTING du message (U32, choisit l'onglet), 5 nom (49), 6 texte (bloc mémoire). Règles
d'ArchCore :

| Commande | Objets (« usable » de genre 20, SubType) | Destinataires | Type |
|---|---|---|---|
| `/race` | Race Trumpet (3905), genre 1 | joueurs en jeu de la même race | 19 |
| `/scream` | Shout of Elemental (3906, 6182, 11518), genre 2 | tous les joueurs en jeu | 20 |
| `/warn` | Warning of Ashtal (3911), genre 3 | tous les joueurs en jeu (un seul monde) | 7 |

Un objet est consommé par message (refus avec message système s'il manque) ; un cri par seconde (refus avec message).
Le texte est composé par le serveur : `[Objet] [Nom] message` (ces onglets n'ont ni en-tête ni nom), suffixe de
couleur gardé ; l'émetteur le reçoit aussi. L'interdiction de discussion (`/chatban`) s'applique.

## Salons de discussion (type 0x2C, AgpmChannel)

Drapeaux sur 4 octets : 0 op, 1 CID du demandeur, 2 nom du salon (41), 3 mot de passe (9), 4 nom de personnage (33 :
membre, invitant, cible ou auteur), 5 genre (1 public, 2 privé, 3 guilde), 6 code de résultat, 7 message (121),
8 liste des membres (bloc mémoire). Un CID autre que celui du joueur est refusé.

| Op | Sens | Champs | Effet |
|---|---|---|---|
| 1 | client | 2, 3, 5 | Créer : nom pris → 1 ; 5 salons → 4 ; salon de guilde : chef seulement → 5, un par guilde → 7 ; sinon op 3 |
| 3 | client | 2 | Entrer : inconnu → 2 ; déjà membre → 11 ; autre guilde → 6 ; plein ou 5 salons → 4 ; privé : op 8 |
| 3 | serveur | 2, 3, 5, 8 | Entrée faite ; liste `nom1;nom2;…;` et un zéro, l'entrant compris, **toujours présente** (le client la lit sans vérifier) |
| 4 | client → serveur | 2 | Sortie (op 4 au sortant, op 6 aux autres) ; un salon vide disparaît |
| 5, 6 | serveur | 2, 4 | Membre arrivé, parti |
| 7 | client → cible | 2, 4, 5 | Invitation (joueur absent → 3) ; elle dispense du mot de passe (choix d'ArchCore) |
| 8 | serveur / client | 2 (3) | Mot de passe demandé, puis saisi (faux → 8) |
| 9 | client → membres | 2, 4, 7 | Message relayé à tous les membres, auteur compris (le client n'affiche rien avant le retour) |
| 10 | client → serveur | 4 | Recherche d'un joueur : op 10 s'il est en ligne, sinon 3 |
| 11 | serveur | 6 | Résultat : 1 nom pris, 2 inconnu, 3 joueur introuvable, 4 plein, 5 pas chef, 6 pas membre de la guilde, 7 la guilde a déjà son salon, 8 mauvais mot de passe, 11 déjà membre |

Limites du client : 5 salons par personnage, 15 membres (100 pour un salon de guilde), nom de 40 caractères, mot de
passe de 8, message de 120. Salons en mémoire, quittés à la sortie du monde ; noms comparés sans la casse. La règle de
région `<Chat>No</Chat>` (regionperculiarity.xml, message 43) n'est pas appliquée : le fichier d'Archonia n'en a
aucune.

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

## Inventaire Cash et sac du familier (type 0x04, statuts 51 et 13)

**Inventaire Cash (statut 51).** Les objets dont la colonne `Cash` d'itemdatatable.txt n'est pas nulle (6 : familier)
y sont rangés : AGPMITEM op 0 au statut 51, **sans case** (le client les place lui-même dans sa liste) et avec, au
**champ 23**, le mini Cash relevé dans le client x64 (drapeaux 1) : 0 INT8 « en cours d'utilisation » (0 non, 1 oui,
2 en pause), 1 INT64 temps restant, 2 UINT64 date d'expiration, 3 INT32 (sens non relevé), 4 INT64 (non lu par le
client). ArchCore n'envoie que le champ 0. **Op 26** (champ 2 IID, 4 CID, 23 mini Cash) met l'objet à jour et fait
rafraîchir l'inventaire Cash au client. Une utilisation (op 3) est admise depuis l'inventaire, l'inventaire Cash et le
sac du familier ; la destruction (op 21), sauf pour un objet en cours d'utilisation. En base, statut 6, rang dans la
liste en `row_idx` (200 places) ; un objet Cash encore enregistré dans l'inventaire ordinaire (objet d'avant
l'inventaire Cash, reçu hors ligne, par courrier ou par échange) y passe à l'entrée en jeu et libère sa case. Choix
d'ArchCore : les 9 têtes Cash qui se portent (partie 17) restent dans l'inventaire ordinaire.

**Familier.** Utiliser l'objet (op 3) fait sortir la créature (AgpmSummons, type 8) et passe l'objet à 1 (op 26) ;
le réutiliser, ou demander son arrêt par **op 23** (champ 2 IID ; objet à `CanStopUsingItem`), la renvoie, et l'objet
repasse à 0, comme pour tout départ du familier (maître mort, parti, familier remplacé). L'état n'est pas enregistré :
le familier ne revient pas de lui-même à l'entrée en jeu.

**Sac du familier (statut 13).** 4 sacs × 2 lignes × 4 colonnes (fenêtre `UI_SubInventory`, touche V), case au
champ 7. Il est disponible, comme le veut le client, quand un objet de l'inventaire Cash est en cours d'utilisation,
de type Cash 6, utilisable, et que sa créature (`summons_tid` de sa compétence) a `PetType` = 1 dans
CharacterDataTable ; sinon le client ignore les objets du statut 13. Les familiers 14821 à 15674 (ajouts d'Archonia,
absents des tables EP8) sortent sans sac.

| Geste | Paquet | Réponse |
|---|---|---|
| Déposer depuis l'inventaire (2), l'équipement (3) ou la banque (100, gardien à portée) | op 2, statut 13, IID, CID, champ 7 = case ; ligne et colonne à -1 (onglet d'un sac) : première case libre de ce sac ; sans champ 7 : première case libre | op 2 statut 13 avec la case ; sur une pile du même objet, réunion (op 1 de la source, op 2 de la cible) |
| Déplacer dans le sac | op 2, statut 13, case | op 2 statut 13 (ou réunion) |
| Sortir vers l'inventaire / la banque | op 2, statut 2 (champ 7) / 100 (champ 8) | op 2 au nouveau statut (ou réunion) |
| Séparer une pile du sac | op 17, quantité au champ 5, case libre du sac au champ 7 | op 0 statut 13 de la nouvelle pile, op 2 de l'ancienne |
| « Auto Pick Up ON/OFF » | **op 30**, champ 2 IID du familier, 4 CID, champ 20 = mini {INT16 0/1} | op 30 en écho |

Tout refus (sac indisponible, case prise, sac ou inventaire plein, banque fermée) renvoie l'objet à sa place (op 2).
Le sac part (op 0 statut 13) la première fois qu'il devient disponible dans la session, après les objets Cash ; le
familier rappelé, le client ferme le sac et garde ses objets, invisibles, jusqu'au prochain appel. En base, statut 7
(sac, ligne, colonne) ; la valeur de l'op 30 dans `characters.pet_auto_pick` (1 par défaut, comme le client). Choix
d'ArchCore : au ramassage (op 11), un objet Cash va dans l'inventaire Cash, un autre dans l'inventaire, puis, s'il est
plein, dans le sac du familier disponible ; pas de ramassage à la chute (règle de l'op 30 inconnue). Écartés (ajouts
d'Archonia) : marquage des sacs par Ctrl+clic (ops 37/38) et « Sell All ». Non faits : achat en boutique vers le sac,
retrait de la grille d'échange vers le sac, équipement depuis le sac, endurance des familiers.

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
  `Gacha` non vide (même « 0 ») range l'option dans une autre famille, hors butin. `CategoryName` est propre à
  Archonia (colonne absente de la table d'origine, aucune de ses 589 options n'y figure) : ces options ne sont jamais
  tirées (butin, options liées), seulement appliquées si un objet déjà en base les porte. `Item_Level_Min` et `Item_Level_Max` ne règlent pas le tirage (Archonia ne les lit pas, la
  table d'origine les lit sans s'en servir).
- **Réseau** : AGPMITEM champ 20, mini-paquet de 5 UINT16 (TID), dans chaque op 0 d'un objet connu
  (inventaire, équipement vu des autres, banque, échange, objet au sol). Le client applique lui-même les options
  aux facteurs de l'objet pour l'infobulle (« Additional Ability », `Description` de la table).
- **Effet** : le serveur ajoute aux facteurs du personnage les options de l'équipement porté. Chaque colonne
  vise un facteur (dégâts min et max d'un attribut, défense en points ou en taux, PV et PM max, précision,
  caractéristiques, portée et vitesse d'attaque, vitesse de déplacement) ; `PointType` 0 ajoute la valeur,
  1 l'applique en pour cent au total, comme toujours `ATK_Speed_bonus`, `Movement_Run_bonus` et
  `*_Percent`.
- **Options liées** : un objet unique cite des groupes d'options (`Link_id` d'itemdatatable.txt, séparés par
  « ; ») ; pour chaque groupe, le porteur reçoit l'option dont `Level_min` ≤ son niveau ≤ `Level_max`. Rien
  n'est envoyé : le client les retrouve lui-même pour l'infobulle.
- **Options à déclenchement** (colonne `skill_type`, lue par `Archonia.exe` sans calcul côté client) : 1 coup
  critique (chance `skill_rate`, dégâts `Critical` en pour cent d'un coup normal ; sans chance, `Critical`
  s'ajoute aux dégâts critiques ; sans `skill_type`, `Critical` seul s'y ajoute aussi, comme le compte la fenêtre du
  personnage du client, malgré la description « Critical Hit Rate » de l'option 13822), 2 étourdissement de la
  cible d'un coup qui touche (chance, `stun_time` ou à défaut `duration` en ms), 3 coup reçu changé en PV
  (chance, part `damage_convert_hp`), 4 régénération (`regen_hp`, `regen_mp` toutes les 10 s, en combat aussi),
  5 blessure de la cible (chance ; dégâts de la colonne de leur élément chaque seconde pendant
  `dot_damage_time`, qui remplace `duration` ; ces dégâts ne s'ajoutent pas aux facteurs), 6 esquive
  (`evade_rate` : chance d'éviter un coup de monstre, résultat 7). `Ignore_Physical_Defense_Rate` et
  `Ignore_Attribute_Defense_Rate` s'ajoutent à la défense ignorée, `Critical_Defense_Rate` à la résistance aux
  critiques (section Compétences) ; `StunResistance`, propre à Archonia, n'est pas lue. Pénétrations (`Ignore_*`,
  points de pour cent ôtés à la défense de la cible) : `PhysicalAttackResist`, `BlockRate`, `SkillBlockRate`,
  `EvadeRate` et `DodgeRate` (une fois pour les deux), `CriticalDefenseRate`, `StunDefenseRate`. `skill_cast` et
  `skill_delay` vont aux facteurs d'attaque 3 et 4 ; `bonus_exp`, `bonus_money`, `bonus_drop_rate` majorent en pour
  cent l'expérience, l'or et le taux de butin du porteur ; `Add_Debuff_Duration` allonge (ms) les affaiblissements
  qu'il pose. L'étourdissement
  part dans l'état du monstre (AGPMCHARACTER op 1, champ 17), la blessure dans ses PV (op 1) ; aucun paquet
  d'effet ne les annonce.

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
- **Runes à compétence** : les pierres de prière de glace, de foudre et de poison (`Skill_Name`, `Skill_Level`,
  `Skill_Rate` d'itemruneattributetable.txt ; le client relie la compétence par son nom) lancent leur compétence
  sur la cible d'un coup normal qui touche, avec la chance `Skill_Rate` (4 à 6 %) : AGPMSKILL **op 8** (lanceur
  = joueur, TID de la compétence, dégâts élémentaires fixes du niveau moins la résistance de la cible), puis
  **op 4** de l'effet du niveau (Cold Prayer Stone : ralentissement de 15 % pendant 5 s ; Lightning : étourdi
  3 s ; Deadly Poison : poison toutes les 3 s pendant 30 s).

Pierres d'Archonia : le contrôle du client (`Spirit Type` 1 à 5, rang 1 à 5) refuse les pierres de poison (6),
de foudre (7), de glace (8) et de caractéristiques (100 à 106), qui n'ont pas de `Rank` ; ArchCore les refuse de
même (op 7, résultat 2).

## Raffinage (types 0x4B et 0x3F, AgpmRefinery et AgpmEventRefinery)

Recettes de `RefineryItem2.txt`, la table que lit `Archonia.exe` (5 731 recettes, importée dans
`client_table` ; à défaut, `refineryrecipetable.txt` des tables du serveur d'origine). Une recette se reconnaît
à ses ingrédients (`Resource TID`, `Resource Count`), dans n'importe quel ordre ; ses `MakeItem Total` lignes
sont autant d'issues équiprobables (objet et quantité produits, objet bonus, nombre d'options `Item Option
Min/Max` et de sockets `Socket Inchant Min/Max` de l'objet produit) ; `Combine Use Price` est son prix en or.

- **Ouverture** : AGPMEVENTREFINERY op 0 (événement, CID) sur un objet du monde de fonction 26 (la sphère
  d'alchimie des `obj*.ini`, pas un PNJ, table `refinery_object`) ; op 1 l'accorde à 1600 unités au plus de la
  sphère, le client ouvre `UI_Refinery`.
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

## Production (types 0x17 et 0x3A, AgpmProduct et AgpmEventProduct)

Récolte sur cadavre (Collection Skill, TID 663, outil 654 Gathering Knife), cuisine (Cooking 203, outil 655 Cooking
Tools) et alchimie (Alchemy 285, outil 1016 Alchemy Cauldron). Données : `productcategory.txt` (compétence de chaque
catégorie : 0 Hunt, 1 Cook, 2 Alchemy), `productcompose.txt` (95 recettes, désignées par le TID de l'objet produit),
`productexp.txt` (niveaux 1 à 15 : expérience par action, facteur de réussite, cumul), `productgather.txt` (monstres
récoltables par classe, chance, ensemble d'objets) et `productresultitem.txt` (objets d'un ensemble par niveau de
compétence, poids de 100 par bloc ; table du serveur fournie avec le client), importées dans les tables `product_*`.
L'outil d'une compétence est l'`arg1` de SKILL_SPEC quand `requirement` vaut 4 ; il suffit qu'il soit dans
l'inventaire.

- **AGPMEVENTPRODUCT** (drapeaux 1) : 0 op, 1 événement (EVENT_BASE), 2 CID, 3 a, 4 b. Fonction de PNJ 24, envoyée
  avec le PNJ (donnée : catégorie, 0 Adventurer, 1 Cook, 2 Alchemist).
  - **op 0 → op 1** : ouverture de `UI_PDSkillBuy` (PNJ à 1 600 unités au plus).
  - **op 2 → op 3** : apprendre la compétence `a` ; AGPMSKILL op 0 (niveau 1, expérience 0) puis b = 0 réussite,
    2 déjà apprise, 3 or insuffisant, 4 niveau trop bas, 1 autre refus. Niveau et prix : niveau 1 de SKILL_CONST
    (7 / 10 / 15, 300 / 500 / 1 000 or), sans point de compétence ; prix taxé dans la région du joueur (voir les taxes des
    régions).
  - **op 4 → op 5** : acheter la recette `a` ; b = 0 achetée (le client l'ajoute à sa liste), 2 déjà connue,
    3 or insuffisant (`MakingPrice`), 4 compétence sous `ItemLv`, 5 compétence de la catégorie absente, 1 autre.
- **AGPMPRODUCT** (drapeaux 2) : 0 op, 1 CID, 2 identifiant de la compétence, 3 recette / cadavre / TID obtenu,
  4 IID de l'objet recette / quantité, 5 code, 6 expérience gagnée, 7 liste d'INT32.
  - **op 1** (serveur) : recettes connues, envoyées à l'entrée en jeu.
  - **Récolte** : le client lance Collection Skill sur un cadavre (AGPMSKILL op 7) ; ArchCore accepte une
    compétence de production sur un monstre mort et ciblable dont le modèle a une récolte pour la classe du
    joueur, l'approche à 250 unités et renvoie l'action 1, ou refuse par l'action 16 (cible invalide) ou 17 (déjà
    récoltée, ou réservée par un autre). Le client montre une barre de 5,4 s, puis envoie l'**op 2** (champ 3 :
    cadavre) ; le lancement garde le cadavre 15 s de plus.
  - **Fabrication** : Cooking et Alchemy ne se lancent pas ; `UI_Compose` envoie l'**op 0** (champ 3 : recette,
    champ 4 : objet recette ou 0).
  - **op 3** (serveur) : résultat ; champ 3 TID obtenu, 4 quantité, 5 code (0 réussite, 1 échec, 2 outil absent,
    3 cible invalide, 4 trop loin, 5 inventaire plein, 6 matériaux insuffisants, 7 plus rien à récolter),
    6 expérience. L'objet arrive par AGPMITEM, l'expérience par AGPMSKILL op 2.
- **Niveau** d'une compétence de production : facteur « dirt » 4 d'AGPMSKILL ; **expérience cumulée** : facteur 6,
  enregistrée en base (`character_skills.exp`).

Règles d'ArchCore, le serveur d'origine ne les ayant pas laissées : réussite si un tirage dans 0..100 est sous
min(100, chance + facteur de `productexp` au niveau courant), la chance étant `Probability` (récolte) ou
`SucessProb` plus `Recipe_addProb` si l'objet recette est fourni (fabrication) ; récolte : un objet tiré selon les
poids du bloc (ensemble, niveau), quantité 1, une seule récolte par cadavre tous joueurs confondus, inventaire
plein sans rien perdre ; fabrication : matériaux consommés en cas de réussite comme d'échec (un matériau listé
deux fois compte deux fois), objet recette consommé s'il est fourni, 1 objet produit ; recette inconnue, d'une
autre catégorie, au-dessus du niveau ou exigeant un titre (`UsingMonster`, vide partout) : code 1 sans rien
consommer et un message système ; expérience tirée dans [min, max] du niveau sur une réussite seulement, niveau
suivant quand le cumul du niveau est atteint (15 au plus).

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

### Barre de raccourcis (type 0x14, AgpmUIStatus)

Cinq couches de dix cases, index = couche × 10 + colonne (0 à 49) ; chaque case est un mini `AGPMSKILL_BASE`
(drapeaux 1 : `INT8` type de base, `INT32` identifiant) : **5** objet (IID), **7** compétence (identifiant
d'instance de la compétence possédée). Le client n'affiche ni n'envoie d'autre type (actions sociales comprises).

- **op 0** (serveur, entrée en jeu, après les objets et les compétences : le client remplace ses 50 cases et vide
  celles dont il ne connaît pas la base) : champ 2 = les 50 minis (mini sans champ : case vide).
- **op 1** (client, sans réponse) : champ 3 = case, champ 4 = mini (absent : case vidée) ; potions et seuils
  facultatifs. La même base est retirée des autres cases de sa rangée de dix, comme le fait le client.

Le serveur garde le **TID** de chaque case (table `character_quickbelt`) : à l'entrée suivante, une compétence est
retrouvée par son TID (son identifiant d'instance change), un objet par son IID, sinon remplacé par un autre objet du
même TID (pile consommée), sinon envoyé vide. Une base que le joueur n'a pas vide la case. L'op 2 (barre encodée en
texte) n'est pas employée : aucun abonné dans le client.

## Parchemins de téléportation (type 0x04)

Un parchemin (« Passport ») est un objet utilisable de `Type` 4 ; son `SubType` donne sa sorte : 0 retour au
village, 1 aller-retour, 2 destination fixe (colonne `Destination` : x;y;z), 3 retour au village immédiat. Il
s'utilise par AGPMITEM op 3 comme une potion (un exemplaire consommé, confirmation op 4), puis le serveur
téléporte le joueur (AGPMOPTIMIZEDCHARMOVE et AGPMCHARACTER). Le « village » est le point de résurrection de
la région de rattachement, comme pour la résurrection. Le retour au village (0 et 1) est refusé en combat. L'aller-retour mémorise
l'endroit quitté et l'annonce par AGPMITEM **op 9** (champ 4 = CID) ; le client demande à y revenir par
**op 6**, refusé en combat ; le serveur répond **op 10** (retour désactivé) et y ramène le joueur.

## Consommables à compétence, résurrection par objet, oubli des compétences (types 0x04, 0x02, 0x05, 0x3B)

**Consommables à compétence** : objets d'`ItemType` 1 qui ont un `Use skill id` (listes « ; », niveau `Use skill
level`) : ragoûts, potions de vitesse, pierres, ailes, charmes (usage 12), costumes (31), talismans de Janus (37).
AGPMITEM **op 3** → chaque compétence pose son effet sur le joueur, sans incantation (AGPMSKILL **op 4** autour de
lui, facteurs par AGPMCHARACTER op 1), puis pile diminuée (op 2) ou objet retiré (op 1) et **op 4** (3 TID, 4 CID,
12 CID, 18 recharge en ms). Durée : `duration` du niveau, sinon `Apply Effect Count` × `Apply Effect Time`, sinon la
durée de l'objet (`RemainTime`, sinon `ExpireTime`, en minutes), sinon 24 h. Recharge : `Use Interval` + `Apply Effect
Count` × `Apply Effect Time` par TID d'objet (celle qu'affiche la barre du client) et `recast_time` du niveau par TID de
compétence (420 000 ms aux ragoûts) ; pendant la recharge, **AGPMITEM op 5** (3 TID, 4 CID) et un message système.
Familles (`skill_classify_id`, `skill_classify_level` de SKILL_CONST, choix déduit) : un effet de la même famille est
remplacé (AGPMSKILL op 5) si le nouveau est de rang égal ou supérieur, sinon l'objet est refusé (op 5). Un objet
`CanStopUsingItem` n'est pas consommé : le réutiliser retire ses effets. Les effets apportent aussi la défense
élémentaire (`point.def_point_*`, ajoutée aux résistances) et des bonus d'expérience (`bonus_exp`, sur l'expérience
des monstres) et de butin (`bonus_drop_rate`, sur le taux de butin du tueur), en pour cent = valeur /
`Skill.GameBonusDivisor` (10 ; unité inconnue). Les familiers restent à AgpmSummons.

**Orbe de résurrection** (usage 14 ; 842 Resurrection Orb 90 s, 1054… Reverse Orb 30 min, « for Novices » niveaux
1-30) : AGPMITEM op 3 du mort. Vivant : **op 5** (« Regeneration has failed »). Refus par **op 27 code 3**
(« ReverseOrbError » : « You cannot use the resurrection item. ») pour l'ArchLord, en zone de siège en guerre, dans une
région dont le `Type` n'a pas le bit 0x100 (Battle Square, prison, tribune du Battle Royale), en région de combat
(sûreté 2) avec un orbe qui n'est pas Cash, hors des niveaux de l'orbe (`Level` « 1;30 »), ou pendant la recharge
(précédé de l'op 18 avec le reste). Sinon : un exemplaire consommé, statut normal sur place, PV et PM pleins,
invincible 10 s, AGPMCHARACTER op 1 (facteurs, champ 11 = 0) autour, **op 4** (3 TID, 4 CID, 18 recharge) puis
**op 18** (4 CID, 18 recharge restante en ms) ; recharge commune à tous les orbes : `Use Interval`, sinon 900 000 ms.
Pas de perte d'expérience. La résurrection sur place sans objet (AGPMCHARACTER op 10) n'est jamais envoyée par Archonia.

**Oubli des compétences.** Gem of origin (usage 27) : AGPMITEM op 3 (refusée au mort et pendant l'évolution) →
AGPMSKILL **op 1** (1 identifiant, 2 propriétaire) par compétence retirée, facteurs (points), objet consommé, op 4.
Maître : AGPMEVENTSKILLMASTER **op 8** (champ 2 = CID, sans événement : envoyé depuis la fenêtre des compétences, si la
chaîne « skill init » est vide) → op 1 par compétence, argent rendu (AGPMCHARACTER op 1 champ 8), **AGPMCHARACTER op 23**
(1 CID, 19 « 1 ») puis **op 9** (2 CID, 5 résultat : 0 réussi, 1 échec — mort, évolution, déjà fait, rien à oublier —,
2 pas de place pour le remboursement : l'argent dépasserait 1 000 000 000). Une seule fois par personnage : la chaîne
(AGPMCHARACTER champ 19, CHAR[32], dans l'op 0 du personnage) est enregistrée dans `characters.skill_init` ; l'op 5 du
maître porte au champ 7 « déjà fait » (1). Choix d'ArchCore : l'attaque normale, les compétences de l'ArchLord et
celles de production restent ; le maître rend `skill_cost` du niveau 1 et `skill_upgrade_cost` des suivants (sans les
taxes payées), la gemme ne rend que les points ; le charisme dépensé n'est pas rendu.

**Return Orb** (usage 13). Le bouton « Reset Selected » de la fenêtre des compétences envoie AGPMSKILL **op 13** (1
identifiant de la compétence, 2 mini « base » du propriétaire ; pas d'IID d'orbe) ; un double-clic sur l'orbe envoie
l'op 3, sans effet. Refus (message système) : mort, évolution du Scion, compétence qui ne s'oublie pas (attaque normale,
ArchLord, production), prérequis d'une autre compétence apprise (condition de son arbre de maîtrise remplie avant et
plus après), aucune orbe. Sinon : AGPMSKILL op 1 (tous ses niveaux), facteurs (points et points héroïques rendus), une
orbe consommée (inventaire, puis inventaire Cash), op 4 ; pas d'or rendu.

**Secret Safe** (usage 21) : AGPMITEM op 3 → une page de banque en plus sans gardien ni or (AGPMCHARACTER op 1 champ 23,
AGPMSYSTEMMESSAGE 30, op 4) ; refus si la deuxième page n'est pas achetée (texte 723 du client en message) ou au plafond
`Bank.SecretSafeMaxExtraPages` (message 29). Les pages 3 et 4 (prix -1) ne s'ouvrent qu'ainsi.

**Release of Agreement** (usage 28) : glissé sur un équipement, AGPMITEMCONVERT **op 12** (1 CID, 2 IID de
l'équipement, 3 IID de l'objet). Objet de l'inventaire qui reçoit des sockets (hors partie 17), avec un sertissage :
pierres et runes retirées, sockets et renforcement gardés (choix d'ArchCore), objet consommé, AGPMITEMCONVERT op 0
(état), op 4 ; un refus ne renvoie rien (réponse d'origine inconnue).

**Commandes des objets** (discussion, seulement avec l'objet dans l'inventaire ou l'inventaire Cash, hors recharge
« Use Interval », 30 min par TID) : `/go Nom` (Gate of Phantom, Jumper's Illusion) téléporte près d'un joueur en jeu ;
`/safekeeper` (Safe Keeper 4923) ouvre la banque par le gardien le plus proche (AGPMEVENTBANK op 1 avec son
événement) ; `/merchant` (Merchant 4924) la boutique du marchand le plus proche (liste 0x32 ordinaire). Fenêtres
ouvertes à distance valables tant que le joueur reste près de l'endroit de l'ouverture. Refus : mort, métamorphose,
région ou état où le rappel de groupe est refusé (aussi pour la cible de `/go`). `/nick texte` (Signature of Glory) :
signature de 16 octets au plus (sans `"`, `#`, `\`), AGPMCHARACTER op 1 **champ 27** au joueur et à ses voisins, en base
(`characters.signature`), puis dans l'op 0 du personnage ; `/nick` seul l'efface. Chaque fois : objet consommé, op 4
(recharge), message « Objet … utilisé » (texte `JumpItemUsed`).

**Feux d'artifice** (usage 22 : Small firecracker, Middle Fireworks Shine 1-3, Unique Fireworks Heart, Bead of Love) :
un exemplaire consommé, AGPMITEM op 4 (3 TID, 4 CID, 18 recharge) au joueur et à ceux qui le voient ; recharge
« Use Interval » (op 5 sinon). Les autres objets de l'usage 22 (paquets de la boutique Chantra) restent sans effet.

**Objets allumés** de l'inventaire Cash sans compétence : Fortune Manual (usage 25) et Talisman of Protection (usage 19).
Op 3 : allumé (un seul par usage) ou éteint ; op 23 : éteint ; AGPMITEM op 26 (mini Cash, octet « en cours
d'utilisation »), non enregistré. Allumé, le Fortune Manual ajoute `Convert.LuckyScrollBonus` points de pour cent à la
réussite d'un sertissage de pierre ou de rune et d'un renforcement, un exemplaire consommé par tentative (0 par
défaut : sans effet). Le talisman reste sans effet : aucune perte d'objets à la mort, comme les données d'origine
(pvpitemdroptable.txt à 0, AGPMCONFIG « bPCDropItemOnDeath » à 0, texte « Holding items will not be dropped even in case
of PK death ») ; AgpmPvP op 18 codes 8 et 23 jamais envoyés.

**Perte d'expérience** : à la résurrection au village (AGPMCHARACTER op 9), `Death.ExpPenaltyPercent` de
l'expérience du niveau (`Death.ExpPenaltyRoguePercent` pour un meurtrier de palier 3), sans descendre sous le début du
niveau ; rien sous le niveau 6 ni après une mort en combat entre joueurs. Désactivée par défaut : Archonia masque
les textes de l'épisode 8 (3 % au village, 5 % sur place, 10 % au meurtrier de palier 3). Active, AGPMCONFIG champ 1
vaut 1.

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

**Effets et niveaux.** **Op 6** (ALL_BUFFED_SKILL : 2 base, 8 minis SKILL_BUFF {TID, durée restante, TID du lanceur})
: tous les effets en cours d'un personnage, au joueur à l'entrée en jeu et aux autres à son apparition. **Op 15**
(INIT_COOLTIME : 1 identifiant, 2 base, 4 TID) : quand tous les effets d'un joueur tombent (mort, purge), la recharge
de chaque compétence dont il s'était lui-même donné l'effet est remise à zéro ; aussi par `/recast`. **Op 14**
(MODIFIED_SKILL_LEVEL : 1 bonus, moins de 31, 2 base) : niveaux en plus de toutes les compétences apprises, somme des
colonnes `skill_levelup` des options portées (« Learned skills upgraded by n ranks »), envoyé à l'entrée en jeu et à
chaque changement ; le serveur lance les compétences à ce niveau (dernier niveau de la compétence au plus, pas les
compétences de production). L'op 16 (niveau d'une seule compétence) n'est pas envoyée : aucune option des données n'en
donne (`Skill_id`, `Skill_level` vides) ; l'op 10 (effet secondaire) non plus, faute de savoir quand l'envoyer ; l'op 17
est un ajout d'Archonia.

**Maître des compétences.** Les PNJ « Instructeur » portent la fonction 25, avec la race et la classe
enseignées (`SkillRaceType`, `SkillClassType`), envoyées dans leur vue (mini INT8, INT8).
AGPMEVENTSKILLMASTER (0x3B, drapeaux 1) : op, événement, CID, identifiant de compétence, TID, résultat,
points. **Op 4** ouvre la fenêtre (réponse op 5, résultat 0) ; **op 1** apprend le TID du champ 4 :
AGPMSKILL op 0 puis op 6 avec le résultat (0 réussi, 1 échec dont l'argent, 2 déjà apprise, 3 pas assez
de points, 4 niveau trop bas, 5 classe) ; **op 2** améliore la compétence du champ 3 : AGPMSKILL **op 2**
(nouveau niveau) puis op 3 ; **op 8** oublie toutes les compétences (réponse op 9, voir plus haut).

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

**Résistances.** `critical_defence_percent2` (Skill_Const2.txt ; compétences de `defense_melee_attack` 4 comme
Sword Stopper, et Natural Warrior ; pas Counter Attack, dont l'infobulle ne l'annonce pas) et l'option
`Critical_Defense_Rate` forment la résistance aux critiques de la cible : elle est ôtée de la chance de coup
critique de l'assaillant (règle déduite ; un affaiblissement comme Defence Breaker la rend négative). La
résistance à l'étourdissement (`stun_resistance` de SKILL_CONST, colonne rangée après `FACTOR_MAGNIFY`) est propre
à Archonia, absente des tables de l'épisode 8 : elle n'est plus lue et la case 0xbc du bloc ci-dessous reste à 0 ;
l'étourdissement suit la protection (état 8), diminuée de l'`Ignore_StunDefenseRate` du lanceur.

**Règles de l'épisode 8 portées par les tables.** Formules choisies par ArchCore : les tables en donnent les valeurs,
aucun binaire ne montre le calcul (le client reçoit les résultats).

- *Toucher* : chance = niveau / (niveau + niveau de la cible) × AR / (AR + DR) × 290 (10 à 95 %), DR d'un monstre
  compris (`DR` de CharacterDataTable, plus les `evade_rate` de ses effets).
- *Résistance physique* : dégâts physiques après la défense × (100 − r) / 100, au moins 1 ; r = facteur de défense 0
  d'un joueur (`PhysicalAttackResist` des options, `point.physical_resistance` des effets ; 80 au plus) ou
  `Physical_Resistance` d'un monstre (100 au plus), moins l'`Ignore_PhysicalAttackResist` de l'assaillant. Coups
  normaux et compétences.
- *Blocage des compétences* : une compétence offensive est bloquée (ni dégâts ni effet) avec la chance de la cible
  (facteur de défense 10 : `SkillBlockRate`, `point.skill_block` ; `Skill_Block` d'un monstre), moins
  l'`Ignore_SkillBlockRate` du lanceur, 80 % au plus.
- *Esquive et blocage d'un joueur* : contre un monstre et entre joueurs, esquive des options et des effets, puis
  blocage (compétences et `BlockRate` des options, facteur de défense 9), chacun 80 % au plus, moins l'`Ignore_EvadeRate`
  / `Ignore_DodgeRate` et l'`Ignore_BlockRate` de l'assaillant (résultats 7 et 9).
- *Renforcements* : `point.dmg_min_*` / `dmg_max_*` s'ajoutent aux dégâts élémentaires, `point.atk_range` (mètres,
  ×100) à la portée d'attaque (50 au moins), `point.skill_cast` / `skill_delay` aux facteurs d'attaque 3 et 4.
- *Incantation et relance* : × (100 − s) / 100, s = facteur d'attaque 3 ou 4 (−100 à 90) ; la réduction de relance ne
  dépasse pas `applied_cooldown_cap` de SKILL_SPEC (pour cent). PM : × (100 + c) / 100, c = somme des
  `cost_mp_decrease` des effets (négatif : moins de PM). Durées réduites envoyées dans l'op 7.
- *Second coup* (Skill_Const2 : `damage_a2`, `damage_a_PERCENT2`, `damage_<élément>2`, `…_PERCENT2`) : une seconde
  part calculée comme la première s'ajoute au même résultat.
- *Préférence de cible* (aidatatable : `HateRace` 인간/오크/문엘프, `HateClass` 전사/궁수/법사, avec leurs pour cent) :
  les dégâts d'un agresseur de la race ou de la classe haïe sont multipliés par ces pour cent (les deux se cumulent) ;
  un monstre préventif divise la distance de chaque joueur par le même poids.
- *Bonus d'or* : `bonus_money` des effets (÷ `Skill.GameBonusDivisor`, comme l'expérience) et des options (pour cent)
  majorent l'or lâché au tueur.

**Caractéristiques secondaires (AGPMSKILL op 3).** À l'entrée en jeu et à chaque changement de ses effets
(effet posé, retiré ou échu, purge, mort, passive apprise ou améliorée), le joueur reçoit seul AGPMSKILL
**op 3** : champ 2 = mini « base » (type 3, son identifiant), champ 9 = bloc mémoire de **220 octets** (longueur
en u32), nul sauf huit i32 petit-boutistes : 0x08 taux critique, 0x0c dégâts critiques, 0x2c et 0x3c coup reçu
changé en PV (chance, part), 0xa4 défense ignorée, 0xa8 résistance élémentaire ignorée, 0xb0 résistance aux
critiques, 0xbc résistance à l'étourdissement. Ce sont les parts des **compétences** seules (passives comprises) :
`Archonia.exe` copie le bloc sans en vérifier la longueur et y ajoute lui-même les options de l'équipement porté
pour les lignes de la fenêtre du personnage (sauf la résistance à l'étourdissement, compétences seules). Le client
n'a pas d'autre traitement pour l'op 3 et n'y répond pas.

**Renvoi des dégâts (contre-attaque).** `defense_melee_attack` 2 (Counter Attack, passive des berserkers :
2 % au niveau 1, 11 % au niveau 10) renvoie à son auteur, avec la chance `skill_rate`, les dégâts d'un coup
de monstre qui touche ou qui est bloqué : AGPMOPTIMIZEDCHARACTION avec le joueur pour attaquant, le monstre
pour cible, le résultat **12** (contre-attaque : `Archonia.exe` affiche alors la mention « C.Attack! » d'`IM_attack.png`) et les facteurs du monstre (PV restants, dégâts en négatif) ;
le monstre peut en mourir, l'expérience revient alors au joueur. Le bit 1 de `reflect_melee_attack` (renvoi
des dégâts reçus) n'est porté par aucune compétence des données d'Archonia.

**Maîtrises.** `SkillMastery_High.txt` (compétences supérieures) et `SkillMastery_Heroic.txt` (compétences
héroïques) rangent les compétences en arbres, par modèle de classe (96 guerrier humain… 1722 slayer, d'après
`SkillMasteryEvolution.txt`). Conditions d'une compétence, comme les lit Archonia (chargeur FUN_140229e10,
contrôle FUN_1402b8f40 ; arbre héroïque FUN_14022a510 et FUN_1402b9200) :
- ConditionA et B, « TID;niveau » : une compétence à ce niveau (le client affiche « %s Skill %d Points
  Requirement »).
- ConditionC, « modèle;n » (« 1722;30 » des compétences 1155, 1165 et 1178 des arbres du Slayer, de l'Orbiter et du
  Summoner) : n niveaux cumulés des compétences de la classe de ce modèle (colonne `require_class_*` de sa race et
  de sa classe), passives et attaque normale exceptées.
- ConditionD, « total; » : niveaux cumulés de toutes les compétences dans l'arbre supérieur (« All Skills %d
  Points Requirement »), points héroïques dépensés dans l'arbre héroïque (« Total Heroic Skill Points
  Required »). Le niveau d'une compétence compte au plus 30 (100 en production).
- L'arbre héroïque n'a que ConditionA et D (B et C ne sont pas lues).

La clé d'un arbre est le nombre en tête de sa ligne : « 10004;4 » ouvre l'arbre de la race 4 (10000 + race), les
15 passives du Dragon Scion, valable pour toutes ses formes ; Archonia cherche d'abord l'arbre du modèle courant,
puis celui de la race. La classe d'une compétence est celle de la forme courante (FUN_1402fd890 : race et classe
des facteurs) : un Dragon Scion apprend les arbres du Slayer, de l'Orbiter et du Summoner sous la forme
correspondante. Faute de quoi l'apprentissage est refusé : message système puis AGPMEVENTSKILLMASTER op 6,
résultat 1 (5 pour la classe). Capacitate (525) exige ainsi Marching Song (526) au niveau 4.

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

**Défense ignorée.** Les bits 4 et 8 de `reflect_melee_attack` (au-delà des valeurs exactes 1 et 2) font
ignorer, tant que dure l'effet, une part de la défense physique (Lethal Attack, Paint With Blood) ou des
résistances élémentaires (Stunner, Testament) de la cible des coups du porteur ; les montants viennent de
`Skill_Const2.txt` (`ignore_physical_defence_percent2`, `ignore_attribute_defence_percent2`, importés avec les
niveaux de `SKILL_CONST.txt` sous le préfixe `percent2.`), auxquels s'ajoutent les options
`Ignore_*_Defense_Rate` de l'équipement. Les résistances élémentaires des monstres (`Magic Res.`, `Water Res.`…
de `CharacterDataTable.txt`, 100 % au plus) réduisent désormais la part élémentaire des coups et des
compétences des joueurs. Stunner niveau 20 (48 %) fait ainsi passer la part d'eau d'un coup contre un Ver vert
(50 %) de 3 à 4.

**Partage des dégâts.** La colonne `divide` de `SKILL_SPEC.txt` (bits relevés dans le lecteur d'`Archonia.exe`,
sens d'après les infobulles) : 1 dégâts élémentaires, 2 dégâts physiques, 4 absorbés par les invocations, 8
dégâts héroïques, 16 pris par le lanceur. Divine Protection (17) et Martyr's Mark (18), compétences de groupe,
posent sur leur lanceur et les membres de son groupe proches un effet : tant qu'il dure, chaque coup de monstre
reçu par un membre fait prendre au lanceur, s'il est en jeu et vivant, `divide_rate` pour cent (10 à 50) de sa
part élémentaire ou physique (le client affiche « Party Damage Absorption Rate »). Le lanceur perd ces PV
(AGPMCHARACTER op 1, facteurs du résultat) et peut en mourir. Dangerous Position (8, dégâts héroïques) et Crown of
Madness (6, invocations) n'ont rien à partager dans ArchCore.

**Purges.** La colonne `dispel_magic` (modulo 10000 ; 10000 et plus : purge répétée) : 2 (valeur exacte)
affaiblissements, 4 étourdissements, 8 ralentissements, 16 tous les effets, 32 invisibilité, 64 effets à état
spécial. Une compétence amie purge ses cibles (Assistance et Disentangle, `target` 10, peuvent viser un autre
joueur à portée), une offensive purge le monstre touché (Antimagic) et aussi son lanceur si les centaines de
`target` valent 1 (Bow of Meditation). Cure (10002) pose un effet qui, toutes les `interval` ms et avec la chance
`skill_rate`, retire les affaiblissements de son porteur. Chaque effet retiré part en AGPMSKILL **op 5** ; les
passives restent.

**Points héroïques et charisme.** Les points héroïques (facteurs « dirt », indice 7) se gagnent au niveau : 1 au
niveau 90, puis 2 par niveau jusqu'au 110 et 3 jusqu'au 120 (71 en tout), moins les `require_heroic_point` des
niveaux appris. Le charisme est le facteur de statut 4, en points bruts (0 à 1 000 000, `characters.charisma`) ;
`Archonia.exe` ne contient aucune règle de gain : le serveur d'origine le donnait en combat entre joueurs et, selon
les règles de région `MobCharismaDrop` et `PVPCharismaDrop` d'`iniRegionPerculiarity.xml` que lit le client, sur les
monstres ; le fichier d'Archonia n'en active aucune et ne donne aucun montant. Le gain attend donc le combat entre
joueurs ; en attendant, la commande d'ArchCore `/charisma` le donne (taux `Rate.Charisma` et bonus
`bonus_charisma_add_rate` des effets qui portent sur ce gain : Charisma Double oui, Boss Additional Charisma
seulement sur un boss), et l'apprentissage ou l'amélioration d'un niveau héroïque le dépense (`require_charisma_point`), comme le
retour arrière du client le laisse voir. Faute de points héroïques ou de charisme, l'apprentissage répond 6 ou 7
(codes du contrôle du client, que sa fenêtre n'affiche pas : un message système les accompagne) et
l'amélioration (op 3, champ 5) 3 ou 4 (1 argent, 2 points de compétence), que le client affiche.

Restent à faire : critiques des compétences.

## Portails de téléportation (type 0x31, AgpmEventTeleport)

Les portails sont des objets du décor (`obj*.ini`) qui portent l'événement 8 et le nom d'un point de
`TeleportPoint.ini` (`PointName`) ; aucun PNJ ne téléporte. AGPMEVENTTELEPORT (0x31, drapeaux 1) : op,
événement (EVENT_BASE de type 1, objet), CID, nom du point (CP949, 33 octets), position, points désactivés.

- **op 4** : le client s'adresse au portail. À moins de 1600 unités, le serveur répond **op 5** (le client
  ouvre la liste des destinations) ; plus loin, il fait courir le joueur vers l'objet et répond dès qu'il est
  à portée. Une marche demandée par le joueur annule l'attente. Champ 5 de l'op 5 : au plus 20 numéros de point
  désactivés (entiers sur 4 octets ; numéro = rang dans `TeleportPoint.ini` à partir de 1), que le client grise
  (`FUN_1406ea050`) ; ArchCore y met les destinations sans objet d'arrivée ou qui n'admettent pas la race du joueur.
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
personnage) : `Teleport_TransformChar`. ArchCore refuse de même les ops 0, 4 et 7 d'un joueur mort, en combat ou qui
a des ennemis (AgpmPvP op 18 code 11), et, sous métamorphose (sauf l'ArchLord), avec un message système. Une demande
(op 4) refusée reçoit l'**op 6** (REQUEST_IGNORED : 1 EVENT_BASE, 2 CID ; la fenêtre ne s'ouvre pas) ; un trajet ou un
retour refusé, et une demande en attente annulée par une marche, l'**op 2** (CANCELED : 2 CID ; liste et chargement
refermés). L'**op 3** (CUSTOM_POS : 2 CID, 4 position) fait partir vers une position libre (rappel de groupe), suivie
du déplacement et de l'op 8.

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

## Guildes (types 0x15, 0x39 et 0x54, AgpmGuild, AgpmEventGuild et AgpmGuildWarehouse)

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
| 11, 12 | ← | Statut de la guilde (champ 11 : 0 aucun, 1 combat déclaré, 2 préparation, 3 combat) ; bilan (12 victoires, 13 nuls, 14 défaites, 15 points), à l'entrée en jeu et après chaque combat |
| 13 | ← | Combat en cours (champ 17, mini GUILD_BATTLE), à l'entrée en jeu |
| 15 à 20, 24 | ↔ | Combat de guildes : défi, acceptation, refus, demande d'annulation, réponse, abandon (voir plus bas) |
| 21, 22, 23, 25 | ← | Début du combat, horloge (chaque seconde), score, résultat |
| 34 à 43 | ↔ | Alliances et hostilités : demandes, réponses, départ, trêve (voir plus bas) |
| 44, 45 | ← | Détails des alliées, des hostiles (champ 21, GUILD_LIST : 3 entrées au plus par paquet) |

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

**Combats de guildes** (Archonia : FUN_14084de70, FUN_14084ebd0, FUN_14084f610, FUN_14084f400). Mini
GUILD_BATTLE (champ 17, drapeaux 2) : guilde adverse, chef adverse, acceptation, délai, début (secondes),
type, durée (s), combattants, horloge, score, score adverse, gain, gain adverse, résultat, manche. Seuls les
chefs agissent. Défi (op 15 : guilde adverse ; type 0 points, 1 élimination, 2 mort du chef, 3 survie de
groupe, 4 survie individuelle ; durée de 5 à 120 minutes ; combattants des types 3 et 4) : les deux guildes
ont 10 membres et 100 niveaux cumulés (codes 20 à 23 ; le nombre exigé est le premier des huit nombres, le
deuxième pour 22 et 23), sont hors combat (24, 25), le chef adverse est en ligne (26) et ne refuse pas les
combats de guilde (option 0x04 : code 43). Le chef adverse reçoit l'op 15, le demandeur le code 28 et le
statut 1 ; sans réponse en 60 secondes, la demande tombe. Refus (op 17) : code 30. Acceptation (op 16) :
statut 2 et GUILD_BATTLE aux deux guildes ; le combat commence `Guild.BattleReadySeconds` plus tard (op 21,
statut 3), puis l'horloge (op 22) part chaque seconde ; au début aussi, **op 46** (manche : 1, au premier des huit
nombres du champ 4 et au champ 14 du combat ; ArchCore n'a pas de combat en plusieurs manches) et **op 27** (listes
des combattants des deux camps : mini du champ 20 {U32 n1, bloc de n1 enregistrements de 0x48 octets de la guilde du
champ 2, U32 n2, bloc de la guilde adverse} ; enregistrement (`FUN_1404dee40`) : nom zéro-terminé dans 0x38 octets,
points, adversaires tués, morts (U32), puis un entier de sens inconnu, -1 = inchangé). À chaque adversaire tué,
**op 26** aux deux guildes pour le tueur et la victime (champ 2 = guilde du destinataire, mini du champ 19 {guilde,
nom, points, adversaires tués, morts}) ; op 27 de nouveau avec le résultat. Aux types 3 et 4, le chef choisit ses
combattants dans la fenêtre `UI_GuildBattleMember` : **op 14** (1 CID, 2 guilde, mini du champ 18 {I16 nombre, bloc de
(U16 longueur, nom sans zéro)}) juste avant la demande (op 15) ou l'acceptation (op 16) ; ce sont les combattants
(ceux qui sont en ligne au début), sinon les membres en ligne au début. Annulation (op 18, puis 19 ou 20 ; code 33 au refus) :
impossible à moins d'une minute du début (code 31). Pendant le combat, les membres des deux guildes peuvent se
frapper ; un adversaire tué rapporte un point (op 23). Fin : à la durée (scores comparés), abandon « GG »
(op 24, refusé avant le début : code 35 ; résultats 4 et 5), mort du chef adverse (type 2) ou plus aucun
combattant adverse debout (types 3 et 4, combattants : les membres en ligne au début, dans la limite choisie).
Op 25 (1 victoire, 2 nul, 3 défaite) et bilan (op 12), enregistré dans `guild`. Une guilde en combat ne peut
pas être dissoute (code 41).

**Alliances et hostilités** (Archonia : FUN_140202390 à FUN_140203130, FUN_140605a40 à FUN_140606630, rappels
FUN_1408cc440, FUN_1408cbbf0, FUN_1408cc1c0, FUN_1408cbb00, FUN_1408cb2f0, FUN_1408cba20, FUN_1408cb890,
FUN_1408cbd20, FUN_1408cb400). Champs : guilde (2), nom (3), date (6), rôle (11), autre guilde (17 : mini
GUILD_BATTLE réduit à son champ 0) ; 37 et 43 portent aussi le CID (1). Seuls les chefs agissent.

| Op | Sens | Contenu |
|---|---|---|
| 34 | → | Demande d'alliance : 2 sa guilde, 3 son nom, 17 la guilde visée ; ← au chef visé : 2 sa guilde, 3 chef demandeur, 17 la demandeuse |
| 35, 36 | → | Refus (code 0x44 au demandeur), accord ; ← 36 à chaque guilde de l'alliance : 17 l'alliée, 6 date d'entrée, 11 rôle (2 fondatrice, 1 autre) |
| 37 | ↔ | Départ de l'alliance (17 = sa propre guilde) ; ← 37 : 17 la partante, 3 la nouvelle fondatrice si la fondatrice part |
| 38, 39, 40 | ↔ | Déclaration d'hostilité, refus (0x52), accord ; ← 40 aux deux guildes : 17 l'hostile, 6 date |
| 41 | ↔ | Demande de trêve, transmise au chef visé comme l'op 38 ; Archonia la refuse par l'op 39 : le demandeur reçoit l'**op 42** (2 sa guilde, 17 {guilde qui refuse}) : `Guild_Hostile_Leave_Reject` |
| 43 | ↔ | Trêve acceptée ; ← 43 aux deux guildes : 17 l'autre |

Une alliance a une fondatrice et 3 autres guildes au plus (le client n'en garde pas plus, FUN_1401ff510) ;
20 membres exigés de part et d'autre (0x41, 20 écrit par Archonia ; 0x42), 10 pour une hostilité (0x4f ; 0x50) ;
5 hostilités au plus (0x4d). Une guilde déjà dans une autre alliance, ou hostile, ne peut pas être invitée
(0x3c) ; seule la fondatrice invite (0x3d) ; 0x3e déjà alliées, 0x40 chef visé hors ligne, 0x43 en attente,
0x46 alliance faite (0x4a et le nom de la nouvelle aux autres alliées), 0x47 départ, 0x48 et le nom de la
partante aux autres, 0x49 alliance dissoute (une seule guilde restante) ; 0x4b hostilité impossible (alliée,
guilde visée à 5 hostilités), 0x4c déjà hostiles, 0x4e chef visé hors ligne, 0x51 en attente, 0x54 hostilité
déclarée, 0x55 trêve impossible, 0x56 en attente, 0x59 trêve faite. Une demande sans réponse tombe au bout
d'une minute. Le chef qui refuse les relations de guilde (option 0x40 : le client n'affiche alors pas la
question) n'est pas sollicité : 0x3c ou 0x4b au demandeur. À l'entrée en jeu, le membre reçoit l'op 36 de
chaque alliée, l'op 40 de chaque hostile et leurs détails (44, 45). La dissolution d'une guilde la retire de
son alliance et met fin à ses hostilités. Ce que l'hostilité permet en combat n'est pas établi par le client :
ArchCore n'en tire rien pour l'instant. Discussion d'alliance (« [Union] ») : pas encore faite.

**Entrepôt de guilde** (Archonia : AgpmGuildWarehouse FUN_140309ad0, réception FUN_140309e50 ; AgcmUIGuildWarehouse
FUN_1405fca70 et suivantes ; AgcmEventGuild FUN_1406a6740). Les gardiens de banque portent aussi la fonction 35,
sans données (bouton `sysstr` 611 du dialogue) : AGPMEVENTGUILD op 2 (événement, CID) ← op 3 (accord), puis le
client envoie l'op 1 du paquet **0x54** (drapeaux 2 : 0 op, 1 CID, 2 argent en INT64, 3 pages en plus, 4 IID, 5 page,
6 ligne, 7 colonne, 8 résultat).

| Op | Sens | Contenu |
|---|---|---|
| 1 | → | Ouverture ; ← les objets (AGPMITEM op 0, statut 53, case au champ 7), l'op 0 (argent, pages en plus) et l'op 1 (résultat 2 : la fenêtre s'ouvre) |
| 2 | → | Fermeture (le joueur s'est éloigné de plus de 149 du point d'ouverture) |
| 3, 4 | → | Argent déposé, retiré (champ 2) ; ← argent du personnage (AGPMCHARACTER op 1) et op 0 à ceux qui ont la fenêtre ouverte |
| 5 | → | Objet du sac déposé (4 IID, 5 à 7 case) ; ← AGPMITEM op 2 au statut 53, op 0 aux autres |
| 6 | → | Objet repris (3 statut visé : 2, 4 IID, 5 à 7 case, -1 : au choix du serveur) ; ← AGPMITEM op 2 au statut 2, op 1 aux autres |
| 7 | ↔ | Agrandissement : Archonia demande 999 999 999 999 pièces, ← résultat 4 (`GWARE_NOT_ENOUGH_MONEY`) |

Grille : 4 pages de 7 lignes × 4 colonnes, la première ouverte. Droits (masques d'Archonia, FUN_14030b830) : le chef
a tout (0x1f), les autres membres déposent objets et argent (5). Il faut 10 membres (code AgpmGuild 0x5f,
`Guild_Store_Member_Fail`) ; une guilde dont l'entrepôt contient des objets ou de l'argent ne peut pas être
dissoute (0x60, `Guild_Destory_Keep_Fail`). L'entrepôt est chargé à sa première ouverture ; ses objets sont des
lignes de `character_items` au statut enregistré 3 avec `guild_id`.

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
| 9 | échec sans précision (« You cannot form a party. ») : région sans groupes, invitations en attente trop nombreuses |
| 12 | passage de chef refusé (pas chef, cible hors du groupe) |
| 13 | invitation refusée : l'invité a au moins `Party.InviteLevelGap` (10) niveaux de moins que l'invitant |

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

**Zone d'effet et bonus (op 2 du type 0x08).** Champ 7 : mini de 5 octets, un par membre dans l'ordre de la liste
(1 : à moins de `Party.EffectRange` unités du chef) ; champ 9 : mini {dégâts I8 et défense I8 en pour cent, PV max
I16, PM max I16} ; drapeaux des deux minis sur 1 octet. Le client montre l'icône de zone selon la classe du chef
(chevalier, archer, mage) et les textes `Party_Add_*` (`Party_Add_None` quand tout est nul). Règles d'origine
inconnues, celles d'ArchCore sont réglables : avec n membres dans la zone, chacun d'eux reçoit (n − 1) ×
`Party.BonusPercentPerMember` pour cent de défense (chef chevalier) ou de dégâts physiques (chef archer ou scion), ou
(n − 1) × `Party.BonusPointsPerMember` PV et PM max (chef mage) ; rien quand l'écart de niveau des membres dépasse
`Party.BonusMaxLevelGap` (20). Revue chaque seconde, op 2 aux membres à chaque changement ; le bonus s'ajoute aux
facteurs. Le rappel de groupe fait partir le membre par l'**op 3** d'AGPMEVENTTELEPORT (position libre), puis l'op 8.

### Rappel de groupe (types 0x04 et 0x09)

Les orbes d'usage 15 (Summoning Orb 1174 et 6181, Bead of Summoning 2756 et 3327, Jumper's Summons 6494) rappellent
les membres du groupe auprès du lanceur, qui l'utilise par AGPMITEM op 3 :

| Paquet | Sens | Effet |
|---|---|---|
| AGPMITEM op 4 (3 TID, 4 CID, 18 recharge) | serveur → lanceur | un exemplaire consommé, « PartyUseRecallSuccess » |
| gestion op 15 (1 lanceur, 3 son nom) | serveur → chaque autre membre en jeu | fenêtre `UI_ConfirmRecall` |
| gestion op 16 (1 membre, 2 lanceur) | membre → serveur | accepte (le client annule d'abord son échange, 0x07 op 5) : téléporté près du lanceur |
| gestion op 17 (1 membre, 2 lanceur) | membre → serveur | refuse |
| gestion op 16 / 17 (1 membre, 3 son nom) | serveur → lanceur | « PartyTargetAcceptRecall » / « PartyTargetRejectRecall » |

Le client ne contrôle ni la région, ni le combat, ni la distance. Règles d'ArchCore (celles d'origine ne sont pas
connues), pour le lanceur à l'usage et pour les deux à l'acceptation : refus au mort, en zone de siège en guerre,
dans un Shrine en combat, dans la Battle Square et les Battlegrounds, dans une région sans le bit 0x100 du Type
(prison, tribune…), en combat sauf `Recall.AllowInCombat`. À l'usage, refus aussi sans autre membre en jeu, pendant
la recharge (« Use Interval ») et hors des niveaux de l'objet (« SuitableLevelMin/Max ») : AGPMITEM op 5. La
demande vaut `Recall.RequestSeconds` ; le membre est rappelé à toute distance. Bead of Nothernland (4817, usage 15,
description sans rapport) ne rappelle pas (`Recall.ExcludedItems`).

## Échange entre joueurs (type 0x07, AgpmPrivateTrade)

Drapeaux 2 : op, CID, CID de l'autre joueur, IID, TID, argent (INT64), cases d'origine et de destination,
argument. Le client envoie toujours son propre CID au champ 1 ; les champs lus à la réception sont ceux du
client de référence.

| Op | Sens | Rôle |
|---|---|---|
| 1 | client | A demande un échange à B (champ 2) ; tous deux passent au statut d'action 7 |
| 3 | serveur | B reçoit la demande : champ 2 = A (seul lu par le client), champ 1 = B |
| 4 / 5 | client | B accepte / refuse (« No » : op 5) |
| 6 | serveur | Début, à chacun (champ 1 = soi, champ 2 = l'autre) |
| 9 | client | Mettre l'objet d'IID donné (champ 3) : case d'origine (champ 6), case visée de la grille (champ 7, prise si libre, sinon la première libre), genre (champ 8 : 0 grille, 1 objet particulier, 2 grille Ex : Marvel Scroll of Release de l'inventaire Cash, voir plus bas) |
| 10 | client | Reprendre l'objet d'IID donné (champ 3) : statut d'arrivée (champ 5, 2 = sac), case d'échange (champ 6), case d'arrivée du sac (champ 7, prise si libre, sinon la case d'origine) |
| 23 / 24 | client, serveur | Argent proposé (champ 5) ; l'autre reçoit op 24 |
| 16, 17, 18, 19 | | Verrouiller (soi ← 16, champ 1 = soi ; l'autre ← 17), déverrouiller (soi ← 18, champ 1 = soi ; l'autre ← 19) |
| 20 | serveur | Les deux côtés sont verrouillés |
| 21, 22 | | Prêt (soi ← 21, l'autre ← 22) ; tous deux prêts : échange |
| 0 | serveur | Résultat (champ 1 = soi, champ 8 = 8 : échange fait) |
| 5 | | Annulation, des deux côtés (champ 1 = soi) |
| 25 | serveur | La cible refuse les échanges (`Option_Trade_Off_User`, voir plus bas) |
| 26 / 27 | serveur | Pièces Chantra : ignorées (boutique Chantra laissée de côté), jamais envoyées |

Un objet mis dans l'échange passe, chez son propriétaire, au statut 6 (grille d'échange : 1 × 3 × 4) par
AGPMITEM op 2 ; l'autre le reçoit par AGPMITEM op 0, statut 6, propriétaire = le premier. Il reste en base
et dans l'inventaire jusqu'à l'échange, et ne peut être ni déplacé, ni utilisé, ni détruit. À l'échange, le
serveur vérifie l'argent et la place dans les inventaires, transfère les objets (AGPMITEM op 1 chez l'ancien
propriétaire, op 0 statut 2 chez le nouveau) et l'argent (AGPMCHARACTER champ 8). Une déconnexion annule
l'échange.

Déverrouiller ne lève que son propre verrou : le client ne relâche son bouton « Lock » qu'au clic, et un
verrou levé par le serveur lui laisserait un bouton faux. Le partenaire voit l'offre changer, et l'échange
attend de nouveau les deux verrous. Le client n'envoie jamais l'op 25 ; reçue, elle vaut une annulation
(comme l'op 5). Meurtriers : demande refusée (op 5) au palier 1 du demandeur ou au palier 2 de l'un des deux
(le client cache alors le menu d'échange), acceptation refusée au palier 1 de celui qui accepte.

**Objets liés et grille Ex** (épisode 8 : `PrivateTradeGridEx` de `[UI_PrivateTrade]`). Un objet lié dès l'obtention
(« BoundType » 1, comme pour le courrier et la maison des ventes) ne se propose qu'avec un Marvel Scroll of Release
(usage 35) posé sur la grille Ex, un seul objet lié par échange, et jamais un objet lié d'événement (refus : message
système). L'op 9 de genre 2 pose le parchemin, pris dans l'inventaire Cash, un par échange : soi ← AGPMITEM op 1 puis
op 0 au **statut 14** (case 1,1,1, celle de la référence : à confirmer en jeu) ; l'autre ne le voit pas. Il ne se
reprend pas (l'op 10 ne vise que la grille ordinaire) : annulé, il revient (op 1 puis op 0 au statut 51) ; l'échange
fait en consomme un (op 1, ou op 0 de la pile restante). « BoundType » 2 (lié en s'équipant) n'est pas suivi par
objet : un tel objet s'échange librement.

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
| 2, 15 | client | Terminer (champ 7 = rang de la récompense choisie à partir de 0, -1 sans choix, comme le clic d'Archonia ; rang hors des choix d'une quête qui en a : op 3 refusé) |
| 3 | serveur | Réponse (champs 4 et 6) |
| 4 | serveur | Quête du journal (mini), à l'entrée en jeu |
| 5 | serveur | Drapeaux des quêtes terminées, à l'entrée en jeu : champs 8, 9, 10 de 0x800 octets ; la quête TID est le bit TID % 16 384 du bloc TID / 16 384 (octet bit / 8, bit de poids faible d'abord) |
| 6 | client | Abandonner (champ 4) |
| 7 | serveur | Réponse (champs 4 et 6) |
| 8 | serveur | Récompenses qui ne tiennent pas dans l'inventaire (champs 1 et 4) : sysstr 447, la quête reste en cours |
| 9 | serveur | Objet remis au départ (`Quest_TransItemTid`, vide dans les données) qui ne tient pas : sysstr 440, quête non prise |
| 10 | serveur | Progression (mini) : le client affiche « monstre n/total » |
| 14 | client | Suivi à l'écran : champ 3 = mini {TID, 0, 0, 0, drapeau} (1 inscrire, 0 retirer), champ 4 = TID ; envoyé à la prise d'une quête et par la case « suivre » ; 10 quêtes au plus ; pas de réponse. Enregistré (`character_quests.journal`) et renvoyé au 5ᵉ entier du mini de l'op 4 |

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
groupe, 0x10 invitations de guilde, 0x80 demandes d'ami (0x04 combat de guilde, 0x08 combat, 0x40 alliance ou hostilité
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
pas envoyé `HELLO`. Depuis la version 1.2, avec la capacité `client-tables/1`, le `HELLO_ACK` porte au
champ 21 l'empreinte des tables de valeurs exportées pour le client (`archcore export-client-tables`).
Spécification complète : [[Extensions ArchRust|FR-Extensions-ArchRust]].

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

## Combat entre joueurs (types 0x4A et 0x2A, AgpmPvP et AgpmRemission)

**AGPMPVP** (drapeaux 2, du serveur au client seulement) : 0 op, 1 CID (code du message à l'op 18), 2 CID cible,
3 victoires (entier du message), 4 défaites, 5 mode (1 sûr, 2 libre, 3 combat), 6 statut (bit 1 invincible),
7 octet de l'op 0, 8 guilde (CHAR[33], chaîne 1 du message), 9 nom (CHAR[49], chaîne 2). Op 0 victoires et
défaites, op 2 / 4 ennemi ajouté / retiré, op 6 liste vidée, op 18 message système (textes `PvP_Msg_*`
d'uimessage.txt : 3 « en combat », 4 « fin du combat », 5 défaite, 12 victoire, 17 cible invincible, 20 ami,
24 charisme gagné, 25 niveau insuffisant…). Les codes 0 à 2 (entrée en zone sûre, libre, de combat) sont produits
par le client lui-même au changement de région ; ArchCore envoie le code 2 puis le 13 (« You moved to a Battle
Square ») à l'arrivée dans la Battle Square, et le 18 (niveau insuffisant) au refus d'un superviseur.

Règles d'ArchCore, reprises du client (`FUN_1402d8e80`) : la sûreté d'une zone vient des bits 2-3 du `Type` de
RegionTemplate.ini ; jamais d'attaque en zone sûre ; ailleurs, ennemi déclaré ou attaque forcée (Ctrl, champ 6 de
l'action 0x41), niveau 20 des deux côtés, pas son groupe en zone libre, pas un ami (même guilde, guilde alliée) en
zone de combat ; guildes hostiles et combat de guildes : ennemies d'office ; Battlegrounds (régions 110-114,
123-125, 175, 176) et Shrines en combat : races différentes seulement, sans Ctrl ; Battleground, Battle Square et Shrine
en combat : ni points de meurtrier, ni premier attaquant, ni mise à prix (voir plus bas). Une attaque forcée rend les deux joueurs ennemis pour 5 minutes
prolongées à chaque coup ; en zone libre contre un innocent, l'agresseur devient premier attaquant 20 minutes
(champ 12 du personnage = 1, champ 25 = secondes restantes, nom violet). Tuer un innocent de plus de 4 niveaux en
dessous après l'avoir attaqué en premier donne des points de meurtrier (facteur de statut 11,
`PvP.MurdererPointsPerKill`) et ouvre 2 heures de délai (champ 26) ; paliers 40 (exclu du groupe, plus d'invitation
ni d'échange, prix des PNJ doublés), 60 (objets de genre 0x20 refusés par les PNJ, code 9), 100 (plus d'attaque
forcée, expérience des monstres réduite de moitié). Tuer un joueur de son niveau ou plus rapporte autant de charisme
que son niveau. Les montants (points, charisme) ne viennent pas du serveur d'origine, qui ne les a pas laissés.

**Listes d'ennemis** (moments choisis par ArchCore, le serveur d'origine ne les a pas laissés) : op 6 (liste vidée) à
l'entrée en jeu et après la mort du joueur (ses inimitiés tombent alors des deux côtés, op 4) ; op 8 (1, 2, 5) quand un
ennemi connu frappe de nouveau, si le mode de la zone a changé ou toutes les 30 s. **Guildes ennemies** : quand un
membre de la guilde G attaque (attaque forcée, hors zone sûre) un membre de la guilde H (non alliée), les membres en
ligne de H reçoivent l'op 10 (1, 8 = G, 5 = mode), puis l'op 16 aux agressions suivantes (même rythme que l'op 8) ;
l'inimitié dure 5 minutes prolongées à chaque agression, puis op 12 (1, 8). Les membres de H frappent alors sans Ctrl
ceux de G hors zone sûre, comme le client le juge (`FUN_1402d8b90`). Op 14 (liste vidée) à l'entrée en jeu et à
chaque changement de guilde, suivie des guildes ennemies de la nouvelle guilde. Un joueur qui a des ennemis ne prend
pas de portail : op 18 code 11 (`PvP_Msg_CannotUseTeleport`) ; l'op 17 lève un événement d'interface absent de
UI_1024X768.ini, elle n'est pas envoyée.

**AGPMREMISSION** (drapeaux 1) : 0 op, 1 CID, 2 mini-paquet. Op 0 demande au pardonneur (fonction de PNJ 31, mini
EVENT_BASE), op 1 accord (fenêtre `UI_Remission`), op 2 rémission : mini {type 4 = lettres de pardon} du client,
{type, résultat} du serveur (type 4 résultat 1 réussite, 3 sans point, 7 lettres insuffisantes ; type 1 résultat 0
délai de 2 heures pas écoulé). 10 lettres (TID 3311 ou 1836) retirent `PvP.RemissionPoints` points (2, comme le
client l'annonce).

## Amis et ignorés (type 0x2B, AgpmBuddy)

**AGPMBUDDY** (drapeaux 4, champs de taille fixe) : 0 op, 1 CID du propriétaire de la liste (celui qui reçoit, du
serveur au client : le client jette le paquet s'il ne connaît pas ce CID), 2 nom de l'autre joueur (CHAR[33]),
3 drapeaux (`UINT32` : 0x1 bloquer la discussion, 0x2 l'échange, 0x4 les invitations de groupe et de guilde,
0x00100000 en ligne, 0x02000000 liste d'amis — absent : liste d'ignorés), 4 région (op 7), 5 relation « Buddy »,
6 code de résultat (op 8). L'op 15 a son propre paquet, même type : 0 op, 1 CID, 2 nombre (`INT16`), 3 30 noms
de 33 octets à la suite (CHAR[990]), 4 30 drapeaux (`UINT32`).

| Op | Sens | Rôle |
|---|---|---|
| 1 | client, serveur | Ajout : ignoré (drapeaux 0), ou acceptation d'une demande d'ami (nom du demandeur, 0x02000000) ; le serveur répond par l'entrée retenue |
| 2 | client, serveur | Demande d'ami : relayée à la cible en ligne (nom = demandeur) |
| 3 | client, serveur | Refus : relayé au demandeur (nom = la cible) |
| 4 | client, serveur | Retrait d'un ami ou d'un ignoré |
| 5 | client, serveur | Cases de blocage d'un ignoré |
| 6 | serveur | État en ligne d'un joueur de la liste (bit 0x00100000) ; le client ne fait que changer l'icône |
| 7 | client, serveur | Détail d'un ami : sa région (champ 4, RegionTemplate ; -1 hors ligne) |
| 8 | serveur | Résultat (champ 6) : 1 déjà dans une liste, 2 hors ligne ou inconnu, 3 liste pleine, 4 soi-même ou nom invalide, 12 demande « Buddy » invalide |
| 9 | serveur | Fin de la liste initiale : le client relit `Ini\IgnoreList.txt` et envoie l'op 15 |
| 10 à 14 | | Relation « Buddy » (mentor et disciple) : reportée ; l'op 10 reçoit le code 12 |
| 15 | client | Ignorés du fichier local du client (30 au plus) |

À l'entrée en jeu, ArchCore envoie une op 1 par entrée (amis avec l'état en ligne, ignorés avec leurs blocages),
puis l'op 9 ; l'op 15 qui suit est fusionnée en base (noms connus manquants, avec leurs cases), sans réponse. Les
joueurs en jeu qui ont le nouveau venu dans leurs amis reçoivent l'op 6, et de même à sa sortie du monde.

Choix d'ArchCore (le serveur d'origine n'est pas connu) : 30 amis et 30 ignorés, noms comparés sans casse, un même
personnage dans une seule des deux listes ; demande d'ami seulement vers un joueur en jeu, ignoré connecté ou non ;
l'acceptation ajoute chacun chez l'autre, le retrait ne vaut que chez le demandeur ; l'option « refuser les demandes
d'ami » (bit 0x80 de l'op 0x11) renvoie aussitôt l'op 3. Les blocages s'appliquent au serveur : chuchotement d'un
ignoré « Block Chat » non transmis (l'auteur voit son message comme d'habitude, comme avec le filtre du client) ;
demande d'échange (« Block Trade ») et invitations de groupe et de guilde (« Block Invites ») refusées avec le message
des options (voir plus haut). En base : `character_buddy`.

## Courrier (type 0x2D, AgpmMailBox)

**AGPMMAILBOX** (drapeaux 2) : 0 op, 1 CID du joueur, 2 courrier (mini-paquet), 3 IID de la pièce jointe (op 3),
4 résultat (0 succès ; le client ne distingue aucun autre code). **Courrier** (drapeaux 2) : 0 identifiant, 1
expéditeur (CHAR[33]), 2 destinataire (CHAR[33], le nom exact du personnage : comparé avec la casse), 3 objet
(CHAR[51]), 4 date (secondes Unix), 5 drapeaux (0x01 lu, 0x10 pièce récupérée), 6 TID et 7 quantité (`INT16`) de la
pièce jointe, 8 texte (bloc mémoire, zéro final compris, 1 à 1000 octets). Pour un courrier existant, le client ne
remplit que l'identifiant.

| Op | Sens | Rôle |
|---|---|---|
| 0 | serveur | Courrier arrivé (« YouHaveMail »), inséré en tête de la liste du client |
| 1 | client, serveur | Suppression ; à la réponse 0, le client retire le courrier |
| 2 | client | Courrier ouvert : marqué lu, sans réponse |
| 3 | client, serveur | Écriture (destinataire, objet, texte ; champ 3 : IID joint ou 0) ; réponse : `MAIL_WRITE_SUCCESS` ou `MAIL_WRITE_FAIL` |
| 4 | client, serveur | Récupération de la pièce jointe ; réponse avec l'identifiant (drapeau 0x10) |
| 5 | serveur | Boîte vidée (inutilisée) |

Le client ne demande jamais la liste : à l'entrée en jeu, ArchCore envoie une op 0 par courrier, du plus ancien au
plus récent. Il ne retire rien lui-même : à l'envoi, le serveur décompte une feuille d'« Ordinary Letter Paper »
(TID 3601, AGPMITEM op 2 ou op 1) et retire la pièce jointe (op 1) ; à la récupération, il l'ajoute à l'inventaire
(op 0, statut 2, première case libre). Choix d'ArchCore : une feuille par lettre, pas d'or joint, une seule pièce
(la pile entière) qui reste la même ligne de `character_items` (statut 4, options et conversion gardées) ;
destinataire cherché en base, hors ligne compris, et servi aussitôt s'il est en jeu ; 30 courriers par boîte ; pas
d'expiration. Codes de refus (accompagnés d'un message système) : 1 échec (soi-même, courrier mal formé), 2
destinataire inconnu, 3 boîte pleine, 4 pas de papier, 5 objet interdit (papiers 3601-3602, TID 4609 à 4623, comme le
client), 6 inventaire plein, 7 objet lié (`BoundType` 1), 8 hors de l'inventaire, 10 objet de la boutique
(`Cash`), 11 suppression d'un courrier dont la pièce n'est pas récupérée. En base : `mail`.

## Usure et réparation (types 0x04 et 0x34, AgpmItem et AgpmEventItemRepair)

La durabilité maximale d'un objet est la colonne `DUR` d'itemdatatable.txt (une valeur commençant par `@` marque un
objet inusable) ; le client ne lit le maximum que dans son modèle. La durabilité actuelle est le **facteur « objet »**
(sous-paquet 10 du mini CHAR_FACTORS, 5 `INT32`, drapeaux 1), champ 0, dans le champ 10 d'AGPMITEM : envoyée avec
l'objet (op 0) quand elle est entamée, et à chaque changement par une op 2 sans statut ni case (champs 0, 2 IID, 3
TID, 4 propriétaire, 10). Le client n'en affiche que le pourcentage : fenêtre `UI_EquipStatus` allumée sous 21 %,
case marquée à 0 %.

Usure (règles d'ArchCore, le client n'en contient aucune) : chaque coup qui blesse, compétences comprises, use d'un
point l'arme qui le porte avec la probabilité `Item.WeaponWearChance` (10 %) — arme montée à cheval, sinon main
droite, sinon arme de la main gauche ; chaque coup reçu qui blesse use d'un point, avec `Item.ArmorWearChance`
(10 %), une pièce d'armure portée tirée au hasard (corps, tête, mains, jambes, pieds, bouclier). Pas d'usure à la
mort. À 0, l'objet reste porté mais ne compte plus dans les facteurs (dégâts, défense, options, runes) jusqu'à sa
réparation. Commande MJ `/durability <valeur|-1>` : durabilité de l'équipement porté.

**AGPMEVENTITEMREPAIR** (0x34, drapeaux 1) : 0 op, 1 événement (EVENT_BASE, fonction 17), 2 CID du joueur, 3 IID des
20 cases de la grille de réparation (`INT32[20]`, ligne × 4 + colonne, 0 : vide), 4 coût calculé par le client
(`INT64`), 5 code de résultat. La fonction 17 des PNJ (25 forgerons, 4 éleveurs) est envoyée sans données, comme la
banque.

| Op | Sens | Rôle |
|---|---|---|
| 3 | client | Ouverture demandée au PNJ (champs 0 à 2) |
| 4 | serveur | Accord : fenêtres `UI_Repair` et inventaire |
| 0 | client | Réparation des objets de la grille (objets de l'inventaire seulement : un objet porté doit être retiré) |
| 1 | serveur | Résultat : 1 réussi (fenêtre fermée), 2 pas assez d'or, 3 déjà plein, 4 échec, 5 objet qui ne se répare pas |

Prix, comme le client : `tronc((prix au PNJ / DUR) × (DUR − actuelle) × 0,5)` par objet, plus la taxe de la région du joueur sur chaque objet ; un
écart avec le coût envoyé donne le code 4. Réussite : argent débité (AGPMCHARACTER champ 8), chaque objet remis
plein (op 2, champ 10), puis code 1 ; noté dans `log_shop` (action `repair`). Refus : PNJ hors de portée ou sans
fonction 17 (code 4), objet sans `DUR` ou inusable (code 5), objets tous pleins (code 3). Un meurtrier de palier 2 ou
plus est refusé avant l'ouverture, par un message système (le client a le texte `CharacterPenalty_CantConvert`,
pénalité 3, mais le paquet qui le déclenche n'est pas établi).

## Montures (types 0x04 et 0x4D, AgpmItem et AgpmRide)

Une monture est un objet de **partie 14** (Kind 32) ; la partie 15 porte l'arme montée (lance, étendard). Le client
n'envoie jamais AgpmRide : il monte en équipant l'objet (AGPMITEM op 2, statut 3) et descend en le retirant (statut
2). **AGPMRIDE** (0x4D, drapeaux 1) : 0 op, 1 CID du cavalier, 2 IID de l'objet monture (op 6 : TID du modèle), 3
temps restant (ignoré). Ops du serveur : 2 monté, 4 descendu, 5 fin de durée (sans effet dans le client), 6 monté sur
un modèle (inutilisée).

Montée : objet équipé (au joueur et à ceux qui le voient), facteurs recalculés (au joueur et à ses voisins), puis op 2
(CID, IID, 0) au joueur et à ses voisins ; le client crée la monture d'après le `SubType` de l'objet. Descente : objet
rangé, facteurs, op 4. À cheval, `Run_Buff` de l'objet remplace marche et course (le client lève alors son plafond de
60 000). Vue : bit 1 du champ 18 d'AGPMCHARACTER (« transformation ») ; un joueur qui arrive en vue reçoit le
personnage, son équipement puis l'op 2 ; à l'entrée en jeu, le cavalier reçoit l'op 2 après ses objets.

Refus de monter (l'objet reste dans l'inventaire) : en combat (coup donné ou reçu depuis moins de 10 s,
AgpmSystemMessage code 24 « CannotRideWhileCombat »), meurtrier de palier 2 ou plus (message système), région sans le
bit 0x10 du champ `Type` de RegionTemplate (code 42), personnage mort. Descente forcée à la mort et dans une région sans
montures (marche ou téléportation) ; un inventaire plein l'empêche (message système, nouvel essai à chaque tour). À
cheval : combat permis, l'arme montée remplace celles des mains (à pied, elle ne compte pas) ; compétences refusées
(seules celles marquées `auto_attack`, que le client ne lance pas, y échappent) ; consommables refusés sans
`EnableOnRide`. Choix d'ArchCore : pas de retrait des renforcements en montant, pas d'expiration (aucune monture des
données n'a `RemainTime` ni `ExpireTime`).

## Métamorphose et évolution du Scion (types 0x02, 0x04 et 0x05)

**Champ 18 d'AGPMCHARACTER** (« transformation ») : bit 0 métamorphosé, bit 1 monté, bit 2 évolué ; le client ne le lit
qu'à l'op 0.

| Op AGPMCHARACTER | Sens | Champs | Effet |
|---|---|---|---|
| 11 | serveur | 1 CID, 2 TID de la forme | métamorphose (le client garde le TID d'origine, rendu à l'op 12) |
| 12 | serveur | 1 CID | fin de la métamorphose |
| 13 | client | 1 CID du joueur | annulation (icône « Cancel Metamorphosis ») ; refusée pour une Mutation subie |
| 26 | serveur | 1 CID, 2 TID de la forme | évolution (ignorée par le client sur un personnage déjà évolué) |
| 27 | serveur | 1 CID, 2 TID à rétablir | fin de l'évolution |

**Potions de métamorphose** (objets d'`ItemType` 1 et de `Type` 7 d'itemdatatable.txt : forme `Polymorph Id`, durée
`Polymorph Dur`, 0 : jusqu'à l'annulation) : AGPMITEM op 3 → pile diminuée (op 2) ou objet retiré (op 1), **op 4**
(le client lance son compte à rebours d'après la durée de l'objet), **op 19** (champ 22 : recharge restante, 600 000 ms,
commune à toutes ces potions), facteurs (op 1, bonus d'ItemTransformTable.txt), puis op 11 au joueur et à ceux qui le
voient. Refus : **AGPMITEM op 27** (champ 1 code, champ 4 CID) : 2 en zone de siège en combat, 4 à cheval, transformé
ou évolué (le code 3, qui ouvre une fenêtre, n'est jamais envoyé) ; recharge ou niveau (« Level ») : message système.
Un joueur qui arrive en vue reçoit l'op 0 avec le **TID d'origine** et le bit 0, son équipement, puis l'op 11. Fin :
durée écoulée (`Item.TransformMaxDurationMs` l'abrège), op 13, mort → op 12 à tous et facteurs d'origine ; rien n'est
enregistré. Sous métamorphose (sauf l'ArchLord) : compétences, parchemins de téléportation, Potions of Quickness et
Beam, portails, Shrines, dialogues et boutiques de PNJ, coiffeur, monture, échange privé (AGPMPRIVATETRADE op 1 → op 5
si l'un des deux est métamorphosé) refusés. L'équipement change (AGPMITEM op 2, statut 3 ou 2) avec le contrôle ordinaire
sur les facteurs : le mini FACTOR_TYPE (race, sexe, classe) reste celui d'origine, et c'est la race d'origine qui vaut
pour le combat entre joueurs ; sous métamorphose ou évolution, le serveur n'envoie que les retraits que le client
accepte (`FUN_1401bca10` sans niveau, contre la forme sous évolution), sinon l'objet reste porté (op 2 statut 3).
Sous une forme de créature (Type 8 de CharacterDataTable), un joueur n'attaque personne et n'est attaqué par aucun
joueur ; une forme sans 0x20000 n'est pas la cible d'une compétence sur ennemi. **Mutation** (compétence de
`transform_target` 4) : la cible prend pour la durée du niveau (7 s) une forme tirée dans `creature_tid`. Sur un
**monstre**, mêmes paquets que pour un joueur : op 11 (CID du monstre, TID de la forme) à ceux qui le voient, op 11
après sa vue (OPT_VIEW op 1 au TID d'origine) pour qui le découvre, op 12 à la fin ; il s'arrête (OPT_CHAR_MOVE
arrêt), ne frappe ni ne lance rien, et n'est ni attaquable ni ciblable sous une forme d'animal. Ensuite, la cible est
immunisée 30 s (`downtime_duration` de SKILL_SPEC, sens déduit ; `Skill.DowntimeSeconds`).

**Évolution du Dragon Scion** : Slayer, Orbiter et Summoner of Evolution (1189, 1192, 1195, `transform_target` 5),
lancées sur soi (AGPMSKILL op 7, action 1). Au bout de l'incantation : facteurs de la forme (op 1), **AGPMSKILL op 4**
(effet sans durée de la compétence, au lanceur : le client le cherche pour l'annulation), puis **op 26** (1722, 1723,
1732) au joueur et à ceux qui le voient ; depuis une autre forme, op 5 et **op 27** (1724) d'abord. Relancer la forme
active : message système. **Annulation** : AGPMSKILL op 7, identifiant de la compétence d'évolution, mini
SKILL_ACTION de **type 4** → op 5, facteurs du Scion, op 27 (1724). Une action de type 4 sur une autre compétence arrête
seulement une approche en cours. Vue et entrée en jeu : op 0 avec le TID de la forme et le bit 2 ; à l'entrée, l'op 4
de la compétence suit les compétences. Sous une forme, une compétence ne sert que si ses colonnes `require_class_*`
admettent la classe de la forme (les évolutions exceptées) ; l'apprentissage suit la même règle.

## Maison des ventes (type 0x0B, AgpmAuction)

Achat immédiat, à prix fixe, de **la pile entière** (pas d'enchères). **AGPMAUCTION** (drapeaux 2) : 0 op, 1 CID du
joueur, 2 sous-paquet selon l'op, 3 résultat (0 succès). Sous-paquets : **SALES** (drapeaux 2 : 0 identifiant de
vente, 1 docID, 2 numéro de l'objet en base, 3 IID, 4 prix en or, 5 prix en CC, 6 quantité, 7 statut 0 en vente / 1
vendu, 8 date, 9 TID), **SELL** (0 IID, 1 quantité, 2 or, 3 CC, 4 date, 5 IID d'un parchemin Marvel), **CANCEL** (0
identifiant, 1 TID, 2 quantité), **CONFIRM** (0 identifiant, 1 TID, 2 quantité, 3 gain en or, 4 gain en CC),
**BUY** (0 docID, 1 quantité, 2 TID, 3 or, 4 CC), **SELECT2** (0 TID, 1 curseur docID, 2 sens, 3 total, 4 résultats),
**ROWSET** (drapeaux 2 : 0 index, 1 texte de requête, 2 en-têtes, 3 lignes, 4 colonnes, 5 taille d'une ligne, 6
tampon, 7 décalages U32 des colonnes ; chaque valeur est une chaîne terminée par un zéro). Prix et identifiants en
I64/U64 ; Archonia n'a pas d'op « SELECT ».

| Op | Sens | Rôle |
|---|---|---|
| 1, 2, 3 | serveur | Vente ajoutée à la boîte du vendeur (SALES), retirée (identifiant), statut changé (1 : vendu, bouton « Confirm ») |
| 4 | client, serveur | Mise en vente ; réponse 0 (sysstr 451) avant le retrait de l'objet, 6 : plus de place (279) |
| 5 | client, serveur | Annulation (identifiant) ; 0 (452), 7 : inventaire plein (280) |
| 6 | client, serveur | Encaissement (identifiant) ; 0 (453) avec les gains |
| 7 | client, serveur | Achat ; 0 (454), 5 quantité (274), 7 inventaire plein (275), 12 plus disponible (276) |
| 9, 10 | client, serveur | Ouverture chez un PNJ de fonction 28 (EVENT_BASE), accord avec le même EVENT_BASE |
| 12 | client, serveur | Recherche par TID |

Ouverture : le client envoie l'op 9 avec l'EVENT_BASE de l'« Item Trader » (fonction 28, sans données dans sa vue,
comme la banque) ; ArchCore répond l'op 10 si le PNJ est à portée (1600), sauf au palier 3 de meurtrier. Recherche :
le client part du curseur 9 999 999 999 999 en sens 0 et redemande avec le docID de la dernière ligne tant qu'il en
reçoit 15 ; ArchCore rend au plus 15 ventes du TID (sens 0 : docID inférieurs, décroissants ; sens 1 : supérieurs) et
le nombre total, sans les ventes échues. Colonnes des lignes : 0 docID, 1 vendeur, 2 identifiant de vente, 3 or, 4 CC,
5 IID, 6 TID, 7 quantité, 8 date, 9 conversion (vide), 10 durabilité, 11 durabilité maximale, 12 options « a,b,c »,
13 à 15 vides. Le client trie et pagine lui-même.

Choix d'ArchCore (le serveur d'origine n'est pas connu) : l'identifiant de vente sert aussi de docID ; or et CC
(`characters.cash`) au prix de la base, jamais celui du paquet (le client envoie un CC nul) ; frais de 3 % de l'or et
du CC prélevés à la mise en vente, non remboursés à l'annulation, pas de taxe à l'encaissement ; prix comme le client
(or 0 ou de 100 000 à 1 000 000 000, CC jusqu'à 1 000 000, pas les deux nuls) ; seulement un objet de l'inventaire
ordinaire, hors échange, classé dans la maison des ventes (`FirstCategory`, `SecondCategory`), ni d'événement ni lié
dès l'obtention ; parchemin Marvel ignoré ; `Auction.MaxSales` ventes par personnage ; après `Auction.ExpireDays`
jours une vente sort de la recherche mais reste annulable. Ouvrir, chercher et acheter exigent le PNJ ; vendre,
annuler et encaisser se font de la boîte de ventes (fenêtre `UI_SalesBox2`), partout. Le vendeur en jeu reçoit l'op
3 à l'achat ; hors ligne, il retrouve sa vente « vendue » à l'entrée (une op 1 par vente). Un refus sans code connu du
client est accompagné d'un message système. L'objet reste une ligne de `character_items` (statut 5), puis passe à
l'acheteur. En base : `auction_sale`, journal `log_auction`.

## Commutateur d'équipement (type 0x6A, AgpmItemSet)

Jeux d'équipement de la fenêtre `UI_SwitchEditor` d'Archonia ; malgré son nom, AgpmItemSet n'a **aucun bonus de
panoplie** (aucune table dans les données). **AGPMITEMSET** (drapeaux 1) : 0 op, 1 emplacement (0 à 99 ; op 4 :
emplacement actif, -1 aucun), 2 argument (op 3 : rempli 1 / vide 0 ; op 6 : `(de << 4) | vers` ; op 7 : position),
3 objets (CHAR[4000]) : jetons `E,<partie>,<identifiant>` séparés par `:`, 10 au plus, l'identifiant étant le champ
26 (U64) d'AGPMITEM. Emplacement = `(profil × 10 + configuration) × 2 + jeu` (jeu A 0, B 1), calculé par le client.

| Op | Sens | Rôle |
|---|---|---|
| 0 | client | Enregistrer l'équipement porté ; réponse op 3 puis op 5 |
| 1 | client | Porter le jeu (vide : tout retirer) ; réponse : changements d'équipement (AGPMITEM), op 4 |
| 2 | client | Vider ; op 3 (0), op 5 vide |
| 3, 4, 5 | serveur | Emplacement rempli ou vide, emplacement actif, objets |
| 6, 7 | client | Déplacer, retirer un objet du jeu ; op 5 |

À l'entrée en jeu, ArchCore envoie ops 3 et 5 par emplacement rempli puis l'op 4, après les objets (AGPMITEM porte
désormais le champ 26 = IID, l'identifiant en base, pour tous les objets ; le champ 19 « drapeaux », où le client lit
la liaison, n'est pas envoyé). Choix d'ArchCore : gratuit ; porter un jeu passe par le chemin de l'équipement manuel
(niveau, race, classe, sexe, monture, place pour ranger), objet par objet, sans message en cas de refus ; objets
absents de l'inventaire ignorés ; pas d'op 4 après un enregistrement ; un déplacement au-delà du dernier objet le met
en fin de liste. Profils, noms et touches restent dans le fichier local du client. En base : `character_item_set`,
`characters.item_set_active`.

## Météo (types 0x30 et 0x47, AgpmEventNature et AgpmNatureEffect)

Du serveur au client seulement. **AGPMEVENTNATURE** (0x30, drapeaux 1) : 0 météo (I8), 1 et 2 (I64) sans lecteur
connu. Le client ne réagit qu'à 0 (fin de l'effet en fondu, 5 s), 3 (pluie : effet de nature 1, ciel 99) et 5
(neige : effet 0, ciel 98). **AGPMNATUREEFFECT** (0x47, drapeaux 2) : 0 identifiant d'effet de `NatureEffect.ini`
(-1 : arrêt) ; ArchCore ne s'en sert pas, la météo suffit. Choix d'ArchCore : une météo commune à tous, fixée par
`/weather clear|rain|snow` ([[Administration|FR-Administration]]) ou tirée toutes les `World.WeatherMinutes` minutes
si `World.WeatherAuto` vaut 1 ; envoyée à tous les joueurs en jeu à chaque changement et à l'entrée en jeu ; non
enregistrée (temps clair au démarrage). Les compétences de l'ArchLord sur le ciel (478 « Day and Night », 522, 523)
sont décrites avec le titre d'ArchLord.

## Invocations (type 0x4F, AgpmSummons)

Créatures des compétences (colonne `summons` de SKILL_SPEC) et des objets à « Use skill id ». **AGPMSUMMONS**
(drapeaux 2) : 0 op (I8), 1 CID du maître (I32), 2 CID de la créature (I32), 3 TID de la créature (I32), 4 type
d'invocation (op 0) ou nouvelle limite (ops 3 et 4) (I8), 5 propension (I8 : 0 attaque, 1 défense, 2 ombre), 6 et 7
horodatages (U64, non nuls), 8 « forcée » (U8 : 1 hors limite). Tous les constructeurs du client posent le champ 8 :
ArchCore l'envoie toujours.

| Op | Sens | Champs | Rôle |
|---|---|---|---|
| 0 | serveur | 1 à 8 | Invocation ajoutée ; le client l'ignore si la créature ne lui est pas déjà connue avec l'état de son type |
| 1 | serveur | 1, 2 | Invocation retirée (état « invoqué » effacé, emplacement forcé rendu) |
| 2 | client → serveur → maître | 1, 2, 5 | Propension (boutons de `UI_TargetSummonsMenu2`) ; acceptée pour une invocation de type 1 du joueur, renvoyée au maître (son étiquette ne change qu'à la réception) |
| 3 | serveur | 1, 4 | Limite des invocations (1 au départ côté client) |
| 4 | serveur | 1, 4 | Limite des créatures apprivoisées (inutilisée) |

Apparition, dans cet ordre : vue de la créature (0x42 op 1, CID de la plage 0x58000000), état spécial (AGPMCHARACTER
op 1, champ 17 : 0x800 pour les types 1 et 8, 0xC00 pour le type 4), puis l'op 0, au maître et à ceux qui la voient ;
un joueur qui l'aperçoit plus tard reçoit les trois. Fin : op 1 au maître et à ceux qui la voient, puis AGPMCHARACTER
op 2 (op 3 après la mort, pour laisser l'animation).

| `summons` | Compétences | ArchCore |
|---|---|---|
| 1 | esprits (Griolli…), Sacred One, Bomba, Legion of the Dead, Call Royal Knight | type 1 : `summons_tid`, `summons_count` exemplaires, `duration` ; limite du maître, relevée au besoin jusqu'au nombre invoqué (op 3) ; la plus ancienne part |
| 2 | Lightning Trap, Holy Ground | type 4, immobile, au plus `limit_quantity` de la même compétence ; frappe les monstres à sa portée (aucune pour Holy Ground) ; reste à la mort du maître |
| 4 | Increase Gate | limite relevée à 1 + `limit_quantity` pendant `duration` (op 3), puis rendue (les plus anciennes en trop partent) |
| 5 | Elemental Heal (passive) | son `percent.max_hp` s'ajoute aux PV des invocations |
| 6 | parchemins d'invocation (objets) | type 1 forcé, un par parchemin, l'objet est consommé |
| 8 | familiers (objets, `CanStopUsingItem`) | type 8 forcé, un à la fois ; suit sans combattre ni être attaquable ; réutiliser l'objet le renvoie |
| 7, 11 | Call on Nature, Evil Light, pièges de faille | non pris en charge (pas de TID de créature) |

Choix d'ArchCore : caractéristiques du modèle (CharacterDataTable) × niveau du maître / « LVL » du modèle ×
`Summons.StatScale` %, niveau = celui du maître (un familier garde celles de son modèle) ; horodatages = apparition et
fin prévue en ms Unix (un familier : dans dix ans). Une invocation de combat reste à ~200 unités du maître et le
rejoint d'un saut au-delà de 3000 ; en attaque, elle frappe la cible du maître, sinon les monstres qui le visent ou la
visent et les joueurs qui les attaquent ; en défense, ces derniers seulement ; en ombre, rien. Les monstres frappés la
prennent pour cible (coups normaux). Un monstre qu'elle tue l'est par le maître (expérience avec partage de groupe,
butin, quêtes ; ses dégâts comptent pour lui). Combat entre joueurs : elle n'attaque un joueur que si son maître le
peut ; l'attaquer suit les règles envers son maître ; jamais la sienne ; les compétences de zone des joueurs ne la
touchent pas. Fin : durée, mort, maître mort (sauf créatures fixes), parti ou téléporté (portail, parchemin, MJ ; un
familier le suit). Non faits : apprivoisement, paquet d'arrêt d'un familier (inconnu), renforcements des familiers.

## Coiffeurs (type 0x43, AgpmEventCharCustomize)

Drapeaux sur 1 octet : 0 op, 1 événement (`EVENT_BASE`, fonction 29), 2 CID du joueur, 3 visage (I8), 4 coiffure
(I8) ; -1 : inchangé. 14 PNJ « <Stylist> ». Le protocole n'a pas de code de résultat.

| Op | Sens | Effet |
|---|---|---|
| 0 | client | Ouvrir la fenêtre : coiffeur de fonction 29 à portée → op 1 |
| 1 | serveur | Accord (événement et CID renvoyés) ; le client ouvre `UI_Customizing`, ou affiche lui-même son refus s'il est monté ou transformé |
| 2 | client | Achat (visage, coiffure) ; le client ferme la fenêtre sans attendre de réponse |

Table `character_customize` (charactercustomizelist.txt) : genre (coiffure, visage), numéro (celui des champs 20 et 21
d'AGPMCHARACTER), nom, `Case` (0 création et coiffeur, 1 coiffeur, 2 création), modèle, niveau exigé, prix (or, ou
objets si `Price(Skull)` n'est pas nul : jamais dans les données). Achat, comme la logique partagée du client : prix =
somme des prix, chacun avec la taxe de la région du joueur, niveau exigé = celui du visage s'il est choisi (aucun pour une coiffure seule),
`Case` non vérifié. ArchCore refuse en plus, par un message système : joueur monté, valeur absente pour son modèle ou
identique à l'actuelle, prix en objets, niveau ou or insuffisant. Accepté : or débité, apparence enregistrée,
AGPMCHARACTER op 1 (champs 20 et 21 changés) au joueur et à ceux qui le voient. À la création d'un personnage, visage
et coiffure doivent être proposés à la création pour le modèle (Case 0 ou 2), sinon 0.

## Avis de recherche (type 0x46, AgpmWantedCriminal)

Drapeaux sur 2 octets : 0 op, 1 CID, 2 page, 3 rang, 4 résultat / drapeau / genre, 5 mini-paquet (`EVENT_BASE` pour
les ops 0 et 1, sinon mini sur 2 octets : nom (33), montant (I64), niveau, TID du modèle, en ligne). Les ops 4 à 8
sont réservées au serveur.

| Op | Sens | Effet |
|---|---|---|
| 0 → 1 | client → serveur | Tableau ouvert près d'un PNJ de fonction 32 (18 chefs de ville), à 1600 unités au plus |
| 2 | client → serveur | Mise à prix {nom, prime} ; réponse : 0 réussie, 1 sous la prime de base, 2 or insuffisant, 3 pas de droit (ou tableau plein) |
| 3 → 4 | client → serveur | Page 0 à 9 : une op 4 par ligne (rang, mini), puis op 4 de page -100 sans mini |
| 5 | client ↔ serveur | Rang et prime cumulée (le client la demande toutes les 30 s quand il est recherché) ; plus recherché : op 7 drapeau 0 |
| 6 | serveur | Nom du tueur à la victime (bouton « Wanted » de la fenêtre de mort) ; sans nom à la résurrection |
| 7 | serveur | Drapeau « recherché » (icône `WantedAlarm`), à l'entrée en jeu et à chaque mise à prix |
| 8 | serveur | Genre 4 : prime touchée par le chasseur ; genre 5 : expérience perdue par le criminel |

Règles d'ArchCore (le client ne fait qu'afficher) : l'op 6 est envoyée quand le tueur a frappé le premier une victime
innocente, ou quand il est meurtrier (palier 1 au moins), jamais dans un Battleground ni en combat de guildes ; le
droit dure jusqu'à la résurrection ou une mise à prix réussie. Prime de base selon le niveau du poseur (50 000 jusqu'au
niveau 20, 100 000 jusqu'à 40, 200 000 jusqu'à 60, 400 000 jusqu'à 80, 1 000 000 au-delà, table du client), payée sur
l'argent porté puis sur la banque. Un avis par criminel : les primes s'ajoutent (tables `wanted_criminal` et
`wanted_bounty`), l'échéance est repoussée à chaque mise à prix (`Wanted.ExpireDays`) ; 100 avis au plus, classés par
prime décroissante. Le joueur qui tue un criminel recherché, hors de son groupe et de sa guilde, touche la prime
cumulée ; le criminel perd `Wanted.ExpLossPercent` de l'expérience de son niveau (sans perdre de niveau) et son avis
disparaît. Un criminel recherché n'est pas innocent : le tuer ne donne ni points de meurtrier ni statut de premier
attaquant.

## Combats de Shrine (type 0x62, AgpmShrineBattle)

Module de l'épisode 8 en **structure brute** dans les deux sens (pas de champs AuPacket) : en-tête standard, octet
0x13 = 1, opération U16 en 0x14 (ArchCore le lit comme un paquet à drapeaux 1 dont le seul champ est l'opération),
bourrage en 0x16, données dès 0x18, pied 0x6B. Structure d'un Shrine (0xD0) : 0x00 Id, 0x04 nom (30), 0x22 nom du
graveur (49), 0x80 état (0 fermé, 1 ouvert, 2 gravure, 4 occupation, 6 fin), 0x84 race occupante (1 à 4), 0xB8 CID
du graveur ; ArchCore laisse à zéro les champs de temps (0x58-0x78, 0x90-0xB0), de sens inconnu.

| Op | Sens | Taille | Contenu |
|---|---|---|---|
| 0 | client → serveur | 0x20 | Liste demandée (fenêtre `UI_Shrine_info`) ; réponse op 2, 11, 12 |
| 1 | serveur | 0xF0 | 0x18 Id, 0x20 Shrine : changement d'état (jauge, effets sur le graveur), à tous |
| 2 | serveur | 0x980 | 10 entrées de 0xF0 dès 0x20, Shrine en +0x20 de chacune ; aussi à l'entrée en jeu (le client n'a pas `ShrineTable.txt` : sa table vient de là) |
| 3 | serveur | 0x20 | 0x18 Id, 0x1C jauge de gravure en pour cent (0 la masque) |
| 6 | serveur | 0x2C | 0x18 Id, 0x1C CID, 0x20 drapeau, 0x24 état, 0x28 valeur : drapeau 0 état 2 = gravure annulée |
| 7 | serveur | 0x58 | 0x18 Id, 0x1C message de `shrinemessage.xml`, 0x20 race (ou nombre des messages en « %d »), 0x24 nom |
| 8 | client → serveur | | Sens inconnu (absent du client) : journalisé |
| 9 | client → serveur | 0x20 | 0x18 Id, 0x1C CID : entrer |
| 10 | serveur | 0x20 | 0x18 Id, 0x1C minutes : ouverture prochaine, proposition d'entrer |
| 11 / 12 | serveur | 0x24 | 0x1C victoires, 0x20 défaites : du joueur / de sa race |

Règles d'ArchCore (le client d'Archonia ne contient rien du serveur ; tables `shrine` et `shrine_reward` remplies par
la couche de contenu) : huit Shrines (régions centrales 236 à 243, zones de race 244 à 275, extérieurs 276 à 283)
ouvrent tous les jours à 20 h, 21 h ou 22 h pour 30 minutes (`Shrine.Enabled`, commande MJ `/shrine open|close <n>`).
Annonces à 10, 5 et 1 minute (messages 1, 2, 4 ; message 3 et op 10 aux joueurs du niveau voulu), entrée possible
5 minutes avant. Entrée (op 9) : niveau 50 au moins (sinon AgpmSystemMessage code 47), 50 joueurs par race au plus
(code 48), téléportation au point de résurrection de sa race dans la zone du Shrine. Un joueur immobile dans la région
centrale grave le sceau (60 s, jauge op 3 ; annulée s'il meurt, bouge ou sort : message 10) ; sa race occupe alors le
Shrine, qu'il doit garder 5 minutes en vie et sans en sortir (sinon message 16, retour à l'état 1). Victoire : messages
17 et 22 à tous ; récompenses aux présents (pièces 53000 et charisme : gravure au graveur, race victorieuse, commune ;
messages 18, 19, 23), scores (tables `shrine_score`, `shrine_race_score`). Sans vainqueur à la fin du temps :
messages 20 et 21. Quinze secondes après la fin, message 27 et retour en ville des présents, op 1 état 0. Pendant le
combat (états 1 à 4), dans les régions du Shrine : races différentes ennemies sans Ctrl, même race jamais, ni points
de meurtrier, ni statut de premier attaquant, ni droit de mise à prix ; un mort ressuscite au point de sa race.

## Titres (type 0x60, AgpmTitle)

Module de l'épisode 8 en **structure brute** dans les deux sens, comme AgpmShrineBattle (constructeur d'Archonia sans
champ AuPacket) : en-tête standard, octet 0x13 = 1, opération U16 en 0x14, **nom du personnage concerné** CHAR[49] en
0x16 (le client retrouve le personnage par son nom), bourrage en 0x47, **TID du titre** en 0x48, champs de l'opération
dès 0x4C (i32, « i64 » sur 8 octets), pied 0x6B ; longueur = taille de la structure + 1.

| Op | Sens | Taille | Contenu (après 0x48) |
|---|---|---|---|
| 1 | serveur | 0x54 | 0x4C résultat (1 : titre ajouté), 0x50 compétence choisie (0) |
| 2 | client → serveur | 0x54 | 0x4C = 2 (constante, ignorée), 0x50 1 équiper / 0 retirer |
| 3 | serveur | 0x64 | 0x4C bUse, 0x50 résultat (1), 0x58 heure du changement (i64), 0x60 code (3 : moins de 2 h depuis le dernier changement) |
| 4 | client → serveur | 0x4C | Liste des titres (sans appelant dans Archonia) |
| 5 | serveur | 0x68 | Un par titre possédé : 0x50 heure du dernier changement (i64), 0x5C 1 valide (0 : liste vide, un seul paquet), 0x60 0, 0x64 compétence |
| 6.0 | client → serveur | 0x50 | Fixer le titre comme objectif (sous-opération en 0x4C) |
| 6.1 | serveur | 0x5C | 0x50 date d'acceptation (i64), 0x58 résultat (1 ; 0 : refus) |
| 6.3 | serveur | 0x68 | 0x50 i32[5] valeurs des conditions non vides, 0x64 1 (progression de `UI_TitleMini`) |
| 6.4 | client → serveur | 0x54 | Terminer l'objectif ; 0x50 compétence choisie (0) |
| 6.5 | serveur | 0x5C | 0x50 date d'achèvement (i64), 0x58 résultat (1) : objectif retiré, suivi de l'op 1 qui ajoute le titre |
| 6.6 | client → serveur | 0x50 | Liste des objectifs, TID 0 (sans appelant dans Archonia) |
| 6.7 | serveur | 0x84 | Un par objectif : 0x50 1, 0x58 0 en cours, 0x60 acceptation (i64), 0x68 achèvement (i64), 0x70 compteurs i32[5] |
| 7 | client → serveur | 0x4C | Supprimer un titre possédé (« Delete Title ») ou abandonner un objectif (« Give Up ») |
| 8 | serveur | 0x50 | 0x4C résultat (1) |
| 9 | serveur | 0x50 | 0x4C 1 afficher / 0 effacer le titre au-dessus du personnage nommé en 0x16 (soi compris : titre actif) |
| 10 | serveur | 0x4C | Ouvre `UI_TitleAll` et `UI_TitleMini` ; TID 1 si `Title.Schola` (Archonia ne le lit pas) |
| 11 | les deux | 0x50 | Compétence d'un titre de rang 4 ou 5 (Archonia) : journalisée, non reprise |

Règles d'ArchCore (le serveur d'origine est absent du client) : le client ne demande jamais ses listes, le serveur
les **pousse à l'entrée** (op 5, 6.7, op 9 du titre actif) ; un « <Title Manager> » (18 PNJ de npc.ini, fonction 21)
répond à la demande de dialogue par l'op 10 (`Title.ManagerOpensWindow`). Fixer l'objectif (6.0) refait les contrôles
du client dans son ordre (ArchLord, chef de guilde, classe — `Require_class_Type` 101 à 901 race et classe, 1001
Dragon Scion, 1101 à 1301 ses formes évoluées —, niveau, Con, Str, Int, Dex, Wis, Cha, or, objets, titres préalables)
et ses bornes (999 objectifs, 500 titres et objectifs), puis **consomme** l'or et les objets de coût ; 6.1, puis 6.3 si
des conditions sont déjà remplies ; refus : 6.1 résultat 0 et un message. Conditions : 2 récolte réussie sur un
cadavre de la créature Set (`Title.CollectCountsKills` : ses morts aussi), 5 utilisation de l'objet Set (potions), 15
niveau, 25 / 26 / 27 niveau d'Alchimie / Cuisine / Collecte ; 6.3 à chaque changement, compteurs enregistrés.
Terminer (6.4) : toutes les valeurs atteintes, 6.5 puis op 1. Équiper ou retirer (op 2) : refusé moins de
`Title.ChangeDelaySeconds` (7200) après le dernier changement (op 3 résultat 0, code 3, heure du changement) ; sinon
op 3 puis op 9 au joueur et aux joueurs qui le voient ; un joueur qui entre dans la vue d'un porteur reçoit son op 9.
Supprimer le titre actif l'efface (op 9 bUse 0) sans délai. Effets **du titre équipé** seulement : compétences
`Skill_Tid_*` (« <Title> Novice Knight »… : Str et Con) comme passives, 3 / 4 / 5 réussite d'Alchimie / Cuisine /
Collecte + Value_1 %, 7 objets récoltés × Value_2 sur la créature Value_1, 9 potion Value_1 + Value_2 points (Set 0) ou
pour cent (Set 1). Rangs de charisme : calculés par le client d'après le CHA envoyé.

## Battle Square et Battleground (types 0x4A et 0x58)

**Battle Square** (régions 189 à 234, six étages ; racines 198, 208, 209, 215, 223, 229, LevelMin 20 à 70 de
RegionTemplate.ini importé dans `region_template.level_min`) : zone de combat sans restriction de race ; un meurtre
n'y donne ni points de meurtrier ni mise à prix. La victime peut lâcher un crâne au sol (`pvpskulldroptable.txt`,
tables `pvp_skull_chance` et `pvp_skull_level` : chance selon l'écart de niveau tueur − victime, multipliée par
`Rate.Drop`, niveau du crâne selon la victime, objets `BattleSquare.SkullTids`). Chaque étage est une île du relief
serveur et aucun portail n'y mène : parler à un superviseur (PNJ 551 à 554) envoie à la porte de l'étage le plus haut
que permet le niveau (`BattleSquare.SupervisorTeleport`, choix d'ArchCore) ; sous le niveau 20, AgpmPvP op 18 code 18.
À l'arrivée de l'extérieur, codes 2 puis 13.

**Battleground** (régions 110 à 114, 123 à 125, 175, 176) : toujours ouvert ; un mort ressuscite au point de sa race
dans le fort de sa race (111 humain, 112 orc, 113 elfe de lune, 176 Dragon Scion). Avec `Battleground.Schedule`
(« HH:MM-HH:MM », vide par défaut), début et fin annoncés par le paquet brut **0x58** : 0x13 op 1, 0x14 texte
(1024), 0x414 couleur ARGB, 0x418 type (2 début, 4 fin ; le client affiche alors ses textes 780 et 781), 0x41C
variante (0 combat des races).

## Fenêtre des boss (type 0x64, AgpmBoss)

Ajout d'Archonia sans effet sur le jeu. Drapeaux 1 : 0 op, 1 octet (0), 2 PACKET[32] apparitions (mini brut de
48 octets : 0x00 TID, 0x18 U64 prochaine apparition en secondes Unix, 0x20 heure, 0x21 minute, 0x28 vivant), 3 butin
(mini brut de 196 octets : TID, 16 TID d'objet, 16 taux, 16 quantités), 4 TID, 5 classement (page, pages, rang du
joueur, son nom, ses victoires, PACKET[16] lignes {rang, nom, victoires}), 6 compteur de dégâts (TID, nombre,
PACKET[20] lignes {nom, TID du modèle, niveau, guilde, marque, 0, dégâts I64}).

| Op | Sens | Effet |
|---|---|---|
| 1 | serveur | Boss de `UIBoss.xml` (table `boss_window`) présents dans le monde, à l'entrée en jeu et à chaque mort, tirage du délai ou réapparition |
| 4 → 3, 5 | client → serveur | Boss choisi (champ 4) : objets possibles (`monster_drop` puis groupes de butin, 16 au plus) et page 0 du classement |
| 6 → 5 | client → serveur | Page du classement (champ 5, mini dont le champ 0 est la page) |
| 7 | serveur | Compteur de dégâts aux joueurs qui blessent un boss suivi, toutes les 2 s au plus (le client l'efface 10 s après le dernier) |

Choix d'ArchCore : le classement est celui du dernier combat contre ce boss, trié par dégâts ; la valeur de chaque
ligne compte les victoires du joueur sur ce boss (table `character_boss_kill`, une par combattant au combat où il
meurt). L'heure du serveur vient d'AgpmTimer champs 2 et 3.

## Guerres de siège (types 0x49, 0x50 et 0x61)

**AgpmSiegeWar (0x49)**, drapeaux 2 : 0 op, 1 résultat (op 2) ou drapeau, 2 château (mini), 3 CID (refusé s'il n'est
pas celui de la session), 4 page de guildes, 5 CID d'un objet de siège, 6 fiche d'objet, 7 IID d'un objet
d'inventaire, 8 message de siège, 9 U64 jamais lu, 10 compte à rebours. Mini château (drapeaux 2) : 0 nom (bloc
mémoire, clé de tous les paquets), 1 chef de la guilde propriétaire, 2 guilde propriétaire, 3 U8 membres, 4 et 5 U64
dernier et prochain siège (FILETIME en heure locale de `World.TimeZone`), 6 U8 statut, 7 et 8 U8 résurrection des
assaillants et des défenseurs, 9 U8 (0), 10 guilde du dernier carving. Les textes partent avec leur zéro final (un texte
vide efface l'ancienne valeur). Page de guildes : I16 page, I16 nombre de pages (n / 15 + 1), PACKET[15] {nom, chef,
U32 points}. Message de siège : U8 code, château, guilde, deuxième nom, U8 type d'objet, U32 (0), U8 côté (0 ouest,
1 est). Compte à rebours : U8 genre (1 avant le début, 2 avant la fin), U16 secondes.

| Op | Sens | Effet |
|---|---|---|
| 0 | serveur | Château complet : à l'entrée en jeu (les 5), à chaque changement, chez un commandant |
| 1 → 2 | client → serveur | Déclaration (château champ 5 : date) : 0x0B fixée, 1 statut ≠ 0, 2 hors du créneau, 3 pas le propriétaire |
| 3, 5 → 2 | client → serveur | Candidature de défense (8), d'attaque (9) ; 2 hors candidatures, 4 pas le chef, 5 guilde trop petite ou trop jeune, 6 déjà inscrite, 0x15 liste pleine, 1 pas sur place |
| 4, 6 → 2 | client → serveur | Retrait (0) |
| 7 → 9, 8 → 10 | client → serveur | Page des candidats à l'attaque (retenus dès le statut 2), à la défense |
| 0xB → 2 | client → serveur | Le propriétaire retient un défenseur (château champ 2) : 10, ops 0xE et 0xF à tous |
| 0xC → 0xE, 0xD → 0xF | client → serveur | Assaillants retenus ; défenseurs (propriétaire d'abord) ; aussi à tous à la clôture et au début |
| 0x10 → 0x13 | client → serveur | Ouvrir un camp ou une catapulte (champ 5) : résultat 0x0E et fiche (champ 6) |
| 0x16 → 0x18, 0x1A ; 0x17 → 0x1A | client → serveur | Charger, retirer un objet d'inventaire (champ 7) |
| 0x12, 0x14 → 2 | client → serveur | Activer (0x13 déjà actif, 1 objets manquants), réparer (0x0D, 0x0F, 0x14) |
| 0x1B → 2 | client → serveur | Carving du trône (champ 5) : 0x10 début, 0x11 annulé (bouger, mourir), 0x12 fini, 1 refus |
| 0x1C | serveur | Message (à tous) : 1 début, 4 objet détruit, 5 sceau levé, 6 et 7 Œil ouvert, refermé, 8 à 10 carving, 11 fin, 12 à 17 camps |
| 0x1D | serveur | Compte à rebours, chaque minute du combat |

**AgpmEventSiegeWarNPC (0x50)**, drapeaux 1 : 0 op, 1 EVENT_BASE, 2 CID. Op 1 du client (commandant de siège, fonction
de PNJ 33 sans données) → op 0 du château de la région du PNJ, puis op 2 (fenêtre UI_SiegeWarMain).

**AgpmWarInfo (0x61)**, drapeaux 1 : quatre INT8 lus bruts en 0x14 (op), 0x15 (type : 0 siège, 1 Battleground,
2 Shrine, 3 Archlord), 0x16 (étape), 0x17 (paramètre), suivis des données de l'op : 0 étape (warinfodata.txt), 2
compteur, 4 et 5 deux INT32 en 0x18 et 0x1C, 6 effacer, 7 trois textes de 0x21 octets en 0x16, 0x37, 0x58 (arguments
« %s »). Sièges : étapes 1 début, 2 et 3 portes extérieure et intérieure, 4 sceau, 5 et 6 Yeux, 7 carving, 8 carving
fini (op 7 : guilde, chef, château), 9 fin (op 7 : guilde, chef, château) ; choix d'ArchCore à vérifier à l'écran : op 2
= minutes restantes (chaque minute), op 4 = (0, durée en secondes) au début, op 5 = (objets détruits, total) ; op 6
30 s après la fin. Envoyé aux joueurs des régions du château et aux membres des guildes du siège.

**AgpmCharacter (0x02) ops 0x13 et 0x14** (champ 1 : CID) : résurrection de siège au point « binding » de son camp le
plus proche (assaillants : type 3, si un camp activé tient ; défenseurs : 4 intérieur ou 7 extérieur, si la tour de
résurrection tient ; Rensphere : 3 et 6, 4 et 5).

Règles du client appliquées : statuts 0 à 6 dans l'ordre (0 aucun siège, 1 candidatures, 2 closes, 3 combat, 4 sceau du
trône levé, 5 Yeux ouverts, 6 fin) ; déclaration par le chef de la guilde propriétaire (un MJ pour un château sans
propriétaire) entre le jour de base à 20 h et 26 h plus tard (dernier siège + Duration4nextSW jours, reculé d'un jour un
dimanche ; toute date à venir s'il n'y en a jamais eu) ; candidatures du chef sur place (régions du château et de ses
abords) au statut 1 ; une guilde ne figure qu'une fois (Rensphere à part) ; ordre d'attaque des objets (tours de garde
et porte extérieure, puis porte intérieure et tour de résurrection, puis Tour de vie, Yeux et trône ; un défenseur ne
frappe que camps et catapultes) ; carving par le chef d'une guilde assaillante (puis par toute guilde du siège autre que
la dernière graveuse), au statut 5, trône détruit, sans bouger pendant StampingTime (`Siege.CarveSeconds`).

Choix d'ArchCore : voir `world/siege/` et [[Configuration|FR-Configuration]] (`Siege.*`) — château sans propriétaire au
départ ; date fixée d'office au premier créneau (jour de base ou lendemain, 20 h) si le propriétaire n'a pas déclaré
dans les NextSWDeclaring heures ou s'il n'y en a pas ; candidatures ouvertes SelectGuildForSW heures ; tous les
candidats attaquants retenus (15 au plus, par points) ; sans assaillant, siège annulé et date suivante ; la guilde du
dernier carving prend le château à la fin, sinon le propriétaire le garde ; guildes vaincues présentes renvoyées en
ville. Objets (`monster_spawn.siege_war` 1) et gardes (2) cachés hors du combat, rattachés à leur château par la région
de leur point ; un objet détruit passe 20 s en variante « en destruction » puis reste en ruine jusqu'à la fin, sans
expérience ni butin ; un Œil d'Archon réduit à 0 PV s'ouvre (Œil éveillé), et se referme seul au bout
d'ArchonEyeOpenTime si l'autre ne l'est pas. Pendant le combat, dans les régions du château : guildes de camps opposés
ennemies sans Ctrl, même camp ou alliées jamais, ni points de meurtrier ni mise à prix. Catapulte : un tir retire
`Siege.CatapultDamagePercent` des PV de la porte extérieure ou de la tour de garde la plus proche ; camp : activé, il
ouvre la résurrection des assaillants ; objets requis de `SiegeWarObject.txt`, réparations payantes à durée. Portails
`SpecialType` 2 (trajets du château) : guilde propriétaire hors siège (libres sans propriétaire), guildes du siège
pendant le combat ; 3 (terrain de chasse du seigneur) : guilde propriétaire ; 4 (entrée de Rensphere) : guildes
propriétaires d'un château pendant la guerre de Rensphere. La guerre de Rensphere est conduite par le titre
d'ArchLord (ci-dessous). Tours de garde et gardes n'attaquent d'eux-mêmes que les assaillants et ne visent jamais un
défenseur.

## Titre d'ArchLord (type 0x55)

**AgpmArchlord (0x55)**, drapeaux 2 : 0 op, 1 CHAR[32] nom de l'ArchLord, 2 CHAR[32] nom du garde, 3 INT8 résultat,
4 INT32 étape, 5 INT32 gardes actifs, 6 INT32 gardes au plus, 7 INT32 message. Le client ne traite que les ops 0 à 6 et
n'envoie que l'op 7, réduite à l'opération.

| Op | Sens | Rôle |
|---|---|---|
| 0 | serveur | ArchLord désigné (champ 1) : à tous à chaque changement, à l'entrée en jeu (sysstr 630) |
| 1 | serveur | Fin de règne (champ 1), à tous, avant l'op 0 du suivant (631) |
| 2 / 3 | serveur | Garde appelé / renvoyé : nom du garde (champ 2), vide si refusé (638 à 641) |
| 4 | serveur | Gardes actifs et au plus (champs 5 et 6 ; 650) |
| 5 | serveur | Étape de la guerre de Rensphere (champ 4) : 1 portail, 2 combat, 3 bataille de l'ArchLord, 4 fin (642 à 645) ; à tous, et à l'entrée en jeu pendant la guerre |
| 6 | serveur | Message (champ 7) : 1 activation de garde impossible, 2 retrait impossible (647, 648) |
| 7 | client → serveur | Événement de fonction 36 : journalisé (aucun objet du client n'en porte) |

Commandes de discussion lues par le serveur : `/guardactivate`, `/guarddeactivate`, `/guardinformation` (ops 2, 3 et 4 ;
op 6 à qui n'est pas l'ArchLord), `/archlord <message>` (message de l'ArchLord à tous, « [ArchLord] nom : message ») et,
pour les MJ, `/archlord set <nom>|clear`, `/archlordbegin`, `/archlordend`, `/nextstep`, `/archlordactivate <nom>`,
`/archlorddeactivate`, `/castleowner <château> <guilde>`, `/deletecastleowner <château>`.

Guerre de Rensphere (château 0 de SiegeWar.txt) : tous les Duration4nextSW jours (30) à SWBeginTime (20 h) si
`Siege.Enabled` vaut 1, ou par `/siege start Rensphere`. Choix d'ArchCore : assaillants = guildes propriétaires d'un
autre château (portail `SpecialType` 4 ouvert), défenseurs = guilde qui tient Rensphere ; étape 1 au début, étape 2
`Archlord.PortalMinutes` plus tard ; carving permis dès le début (fontaine d'Archon, type 6, par le chef d'une guilde
assaillante) ; sceau gravé : statut 7 pour la moitié de SWPlayingTime au plus (pas de résurrection à Rensphere),
compétence de sceau de SiegeWar.txt (930) au graveur, étape 3 ; à la fin (étape 4), le chef de la guilde du sceau devient
ArchLord jusqu'à la guerre suivante (table `archlord`), sinon l'ArchLord en place le reste. Sans assaillant à la date, la
guerre est remise d'une période.

Compétences de l'ArchLord (`skill_type` 9 de SKILL_CONST) : données en mémoire à l'ArchLord (AGPMSKILL op 0, à l'entrée
en jeu aussi), refusées à tout autre joueur (op 7 de refus et message système). Call Dragon (474, `ride`) : AgpmRide op 6
(champ 2 = 1295 Alcadia) à lui et autour, op 4 au lancer suivant, à la mort ou à la fin du règne ; Call Royal Knight (475)
et `/guardactivate` : invocations de combat forcées (AgpmSummons type 1, champ 8 = 1) du Royal Knight (1277, 600 s),
`Archlord.MaxGuards` au plus ; Rain Control (522) et Snow Control (523) : AgpmEventNature 3 ou 5 à tous pendant la durée
de la compétence (5 min), puis la météo d'avant ; Day and Night (478) : AGPMTIMER à tous avec le temps du jeu décalé de
12 h pendant `Archlord.SkyMinutes`, puis rétabli ; les autres (472, 473, 476, 711 à 719) passent par le moteur de
compétences ordinaire.

## Taxes des régions (type 0x53)

**AgpmTax (0x53)**, drapeaux 2 : 0 op, 1 CID (le client ignore un CID inconnu), 2 résultat, 3 EVENT_BASE (ops 0 et 1) ou
château, 4 à 7 régions. Mini château (drapeaux 2) : CHAR[128] nom (celui de SiegeWar.txt), INT64 recette totale versée,
UINT64 dernier versement, UINT64 dernière modification des taux (secondes Unix). Mini région (drapeaux 2) : CHAR[33] nom,
INT64 recette depuis le dernier versement, INT32 taux.

| Op | Sens | Rôle |
|---|---|---|
| 0 → 1 | client → serveur | Intendant (fonction de PNJ 34, à portée) : op 1 avec l'EVENT_BASE ; le client n'ouvre UI_TaxSteward qu'au maître du château |
| 3 → 3 | client → serveur | Taxes du château (champ 3 : nom) : résultat 0, château et régions au propriétaire ; 1 sinon |
| 2 → 2 | client → serveur | Nouveaux taux (régions : nom, taux) : 0 fait, 1 pas le propriétaire, 3 hors de 0 à 50, 2 déjà changés cette semaine (lundi à dimanche, `World.TimeZone`) ; château et régions renvoyés |
| 4 | serveur | Taux d'une région (champ 4) : toutes à l'entrée en jeu, à tous à chaque changement |

Le nom d'une région est celui que compose le client : nom de RegionTemplate.ini traduit par le `RegionGlossary.txt` de
la langue du joueur (table `region_glossary` ; à défaut l'anglais, puis le coréen) : c'est la clé de sa table des prix.
Régions taxées : celles du château et de ses abords (`siege_war`).

Taxe = prix × taux / 100 (division entière), ajoutée aux achats chez les PNJ, à la réparation (objet par objet), au
coiffeur (ligne par ligne), aux portails (région du départ), au maître des compétences, à la production (compétence,
recette) et à la création de guilde ; ni la vente aux PNJ ni les échanges et ventes entre joueurs. Choix d'ArchCore : la
taxe va à la recette de la région où se tient le joueur ; un château sans propriétaire ne taxe pas ; un nouveau
propriétaire repart de taux à 0 ; chaque `Tax.PayoutDay` à `Tax.PayoutHour` (lundi 0 h), les recettes vont à la banque
du compte du chef de la guilde propriétaire, moins 20 % de tribut à la banque de l'ArchLord (sauf à Rensphere ; sans
ArchLord, le propriétaire garde tout), inscrits dans `log_bank` (`taxe`, `tribut`, `count` = château) ; `/tax pay` verse
aussitôt.

## Actions sociales (type 0x02, op 16)

Fenêtre « Emotes » (`UI_SocialAction`, `SocialAction.txt`) : le client envoie AGPMCHARACTER **op 16** (champ 1 : son
CID, champ 16 : type U8, 0 salut … 16 assis) et n'anime rien lui-même. Le serveur renvoie l'op 16 (champ 1, champ 16)
au lanceur et à tous ceux qui le voient ; le client joue alors l'animation et le son de `INI\SOCIAL_PC.txt` (ligne =
type, colonne = TID du personnage). Aucun code de résultat : un refus est silencieux. Refusés (choix d'ArchCore) :
les types suivants (SELECT* de l'écran de sélection, GM_*), le mort, un joueur à cheval ou métamorphosé (le client
pose trois conditions de ce genre dont le sens n'est pas établi), une action moins de `Social.MinIntervalMs` après la
précédente.

## Retour à la sélection des personnages (type 0x2F, AgpmReturnToLogin)

Drapeaux 1 : 0 opération, 1 CID, 2 compte (49 octets), 3 groupe de serveurs (32), 4 adresse « hôte:port » du serveur
de connexion (25), 5 clé. Bouton « Select Character » de `UI_EXIT` (relevé dans Archonia) :

| Op | Sens | Effet |
|---|---|---|
| 1 | client → monde | demande (le client a vérifié lui-même 5 s sans combat et l'arène) |
| 5 | monde → client | clé à usage unique (champ 5) et adresse (champ 4 ; absente : le client reprend celle du serveur de connexion qu'il connaît) ; le personnage quitte le monde, la connexion se ferme 2 s plus tard |
| 2 | monde → client | refus (ArchCore : mort, en combat) |
| 6 | client → connexion | une fois la connexion chiffrée, à la place de la demande de clé de hachage : compte (champ 2) et clé (champ 5) |
| 7 | connexion → client | après la connexion réussie (AGPMLOGIN op 1) ; le client redemande ses personnages (AGPMLOGIN op 3) |

Au serveur de connexion, une clé valable (`account.return_key`, `ReturnToLogin.KeySeconds`) est consommée et suivie
de la clé de hachage (AGPMLOGIN op 0) : le client se reconnecte avec l'identifiant et le mot de passe qu'il a gardés.
Une clé inconnue, échue ou déjà servie est refusée par AGPMLOGIN op 15 (identifiants incorrects). La clé passe par
la base : les deux serveurs peuvent tourner dans des processus séparés.

## Règles restantes des données (types 0x02, 0x04, 0x05, 0x41, 0x44)

Règles proposées d'après les tables de l'épisode 8, réglables (`Rules.*`, [[Configuration|FR-Configuration]]) ; le
client n'en joue aucune, il n'en montre que les entrées et les sorties.

| Règle | Paquets | Contenu |
|---|---|---|
| Part héroïque (`heroic.rs`) | 0x41 champ 3, AGPMSKILL op 8 | point 19 du résultat (en négatif), ajouté aux PV ôtés ; facteurs d'attribut 11 (dégâts min/max, défense) et 12 à 14 (résistances mêlée, distance, magie) envoyés avec les autres |
| Munitions (`ammo.rs`) | 0x41 au seul joueur ; AGPMSKILL op 7 ; AGPMITEM op 2 / op 1 | coup sans munition : résultat 2 (flèches) ou 3 (carreaux), sans facteurs ; compétence : type d'action 12 au mini SKILL_ACTION ; chaque munition consommée : quantité de la pile (op 2) ou retrait (op 1) |
| Arme exigée (`requirement.rs`) | AGPMSKILL op 7 au seul lanceur | type d'action 8 (« This skill cannot be used with the equipped weapon ») |
| Cibles interdites (`target_restriction`) | AGPMSKILL op 7 au seul lanceur | type d'action 7 (cible invalide) |
| Death Strike | 0x41 | résultat 13, dégâts = PV restants du monstre |
| Objets à durée (`lifetime.rs`) | AGPMITEM op 0 et op 26, champ 23 ; 0x44 op 0 | mini Cash : 0 en cours d'utilisation (0, 1 ; 2 pause, non envoyé), 1 temps de jeu restant (ms, i64), 2 date d'expiration (secondes Unix, u64) ; op 26 à la première utilisation, chaque minute de temps de jeu et à la fin ; fin : message système code 5, nom de l'objet au champ 4, puis op 2 ou op 1 |
| Redemption | AGPMCHARACTER op 24 | serveur → mort : champ 1 CID du mort, 3 nom du lanceur, 13 CID du lanceur (« … wants to resurrect you ») ; réponse du client par l'op 24 (champ 4 à 0 : refus) ou l'op 10 : résurrection sur place (op 1, statut d'action 0) |
| Bonus de niveau | AGPMSKILL op 14 | champ 1 = options `skill_levelup` + effets de type 1 (Expert Skill…) ; types 2 et 4 comptés au seul serveur |
| États | AGPMCHARACTER op 1, champ 17 | codes de special_status 26 à 33 (Archonia) ramenés à la valeur EP8 ; 9 halte (0x80000000 + 0x100000000), 10 invisibilité (0x200) ; `disturb` 1, 2, 8 → attaque, déplacement, compétence interdits (bits du client) ; 4 (objets) : bit propre au serveur, jamais envoyé |
| Déplacements forcés | 0x40 | arrêt net du monstre à sa nouvelle place (drapeaux 0x0c), vue revue chez chacun |

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
| 0x11 | Horloge (AgpmTimer) | 0x60 | Titres (AgpmTitle, brut) |
| 0x13 | Quêtes | 0x63 | Reconnexion |
| 0x4A | Combat entre joueurs | 0x2A | Rémission |
| 0x32 | Boutiques des PNJ | 0x37 | Dialogue des PNJ |
| 0x3B | Maître des compétences | 0x36 | Banque |
| 0x31 | Portails de téléportation | 0x15 | Guildes |
| 0x39 | Gestionnaire de guilde | 0x16 | Conversion des objets |
| 0x38 | Forgeron (conversion) | 0x4B | Raffinage |
| 0x3F | Sphère d'alchimie (raffinage) | | |
| 0x07 | Échange entre joueurs | 0x3E | Quêtes des PNJ |
| 0x17 | Production | 0x3A | PNJ de production |
| 0x2B | Amis et ignorés | 0x2D | Courrier |
| 0x34 | Réparation des objets | 0x4D | Montures |
| 0x0B | Maison des ventes | 0x6A | Commutateur d'équipement |
| 0x30 | Météo (AgpmEventNature) | 0x47 | Effets de nature |
| 0x4F | Invocations (AgpmSummons) | 0x4C | Cris de zone (AgpmAreaChatting) |
| 0x2C | Salons de discussion (AgpmChannel) | 0x43 | Coiffeurs (AgpmEventCharCustomize) |
| 0x46 | Avis de recherche (AgpmWantedCriminal) | 0x58 | Annonces du Battleground (AgcmBattleGround, brut) |
| 0x62 | Combats de Shrine (AgpmShrineBattle, brut) | 0x64 | Fenêtre des boss (AgpmBoss) |
| 0x49 | Guerres de siège (AgpmSiegeWar) | 0x50 | Commandants de siège (AgpmEventSiegeWarNPC) |
| 0x61 | Information des guerres (AgpmWarInfo) | 0x55 | Titre d'ArchLord (AgpmArchlord) |
| 0x53 | Taxes des régions (AgpmTax) | | |

Les schémas complets des 86 modules sont dans `data/re/packet-schemas.json`.
