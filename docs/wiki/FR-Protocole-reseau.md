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
| 0x11 | Horloge | 0x60 | Titres |
| 0x13 | Quêtes | 0x63 | Reconnexion |
| 0x32 | Boutiques des PNJ | 0x37 | Dialogue des PNJ |

Les schémas complets des 86 modules sont dans `data/re/packet-schemas.json`.
