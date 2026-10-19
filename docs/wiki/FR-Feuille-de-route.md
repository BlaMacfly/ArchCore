# État et feuille de route

## Fonctionne avec le vrai client

- Serveur et outils entièrement en Rust (un seul exécutable `archcore`).
- Poignée de main chiffrée, liste des mondes, connexion au compte.
- Liste des personnages et écran de création.
- Entrée dans le monde, avec un village de départ par race.
- Apparition des 567 PNJ autour du joueur, avec titres traduits.
- Interface du client en français, accents compris (traduction en cours).
- Déplacement du joueur, avec sauvegarde de la position.
- Dialogue avec les PNJ : les fonctions de npc.ini (dialogue, boutique, quêtes) sont envoyées avec
  chaque PNJ, et le clic ouvre la fenêtre de dialogue.
- Boutiques des PNJ : la boutique de Darryl (Anchorville) affiche ses 43 objets, et l'achat débite
  l'or et range l'objet dans l'inventaire.
- Inventaire du personnage affiché à l'entrée en jeu.

## Validé avec le simulateur, à confirmer en jeu

- Extensions ArchRust, côté serveur : négociation de capacités et messages d'AddOn (murmure, `SAY`).
  Voir [[Extensions ArchRust|FR-Extensions-ArchRust]] ; la partie client reste à écrire dans ArchRust.
- Revente d'un objet à un PNJ.

- Visibilité entre joueurs : déplacement relayé aux joueurs proches, apparition et disparition
  selon la distance.
- Discussion à portée de vue et chuchotements.
- Niveau MJ envoyé au panneau « Admin Commands ».
- Premières commandes MJ : taux, modération, objets, annonce de redémarrage, téléportation. Voir
  [[Administration|FR-Administration]].

## Prochaines étapes

1. Valider la création complète d'un personnage en jeu.
2. Vérifier la visibilité entre joueurs avec deux vrais clients.
3. Fonctions des PNJ : quêtes, téléportation, banque.
4. Monstres, combat, expérience.
5. Objets : déplacement dans l'inventaire, équipement, utilisation ; compétences.
6. Discussion, groupes, guildes.
7. Reconnexion automatique (type 0x63).
8. Commandes MJ restantes de la page [[Administration|FR-Administration]] (arène, événements, failles…).
9. Fin de la traduction : environ 54 000 textes restants.

## Plus tard

- **PlayerBots** : module serveur de personnages joueurs contrôlés par le serveur, sur le modèle des
  PlayerBots des serveurs World of Warcraft, adapté à ArchLord (déplacement, combat, groupes, commerce).
  À commencer une fois les monstres, le combat et les groupes en place.

Les contributions sont bienvenues : ouvrir une *issue* pour proposer ou signaler quelque chose.
