# Protocole réseau

Le client Archonia est une reconstruction 64 bits du client ArchLord. Son protocole reprend celui
du client 32 bits d'origine, avec des longueurs élargies. Tout ce qui suit a été relevé dans
`Archonia.exe` et vérifié avec le vrai client.

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
