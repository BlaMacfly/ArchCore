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

## Déplacement (type 0x40, AgpmOptimizedCharMove)

Le client envoie un paquet 0x40 à chaque clic de déplacement (`FUN_140264010`), avec des drapeaux
d'en-tête sur 1 octet :

| Champ | Contenu |
|---|---|
| 0 | CID du personnage |
| 1 | position actuelle |
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

## Niveau MJ (type 0x12, AgpmAdmin)

Drapeaux de champs sur 2 octets : op, CID, puis 13 mini-paquets. L'op 0 avec, au champ 2, un
mini-paquet « données MJ » d'op 1 (CID, nom, niveau MJ en INT16, nom du compte) enregistre le niveau
du personnage. Le CID doit être celui d'un personnage déjà connu du client : ArchCore l'envoie juste
après le paquet personnage de l'entrée en jeu.

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

Les schémas complets des 86 modules sont dans `data/re/packet-schemas.json`.
