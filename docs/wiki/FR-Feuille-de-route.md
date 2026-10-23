# État et feuille de route

> **Épisode.** Monde de l'épisode 8 « The Dark Revelation » (2012) et extensions d'Archonia ; systèmes de la
> sortie d'origine. Détail par épisode : [[Épisode du jeu|FR-Episode]].

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

- Créatures : 36 960 monstres placés d'après les tables du serveur d'origine, hors du sol infranchissable,
  avec leurs caractéristiques (niveau, vitesses, PV). Vus et ciblés en jeu lors d'un premier essai ;
  l'envoi des PV, ajouté depuis, reste à vérifier.
- Combat au corps à corps contre les monstres : approche, coups (chance de toucher, dégâts selon la
  classe), mort, expérience et montée de niveau (caractéristiques, PV et PM selon GrowUpFactor), cadavre
  puis réapparition. Les monstres frappés
  ripostent et poursuivent leur agresseur ; mort du joueur et résurrection au village. Butin : or et
  objets au sol, ramassage. Équipement : armes et armures portées (restrictions, deux mains), comptées
  dans les dégâts et la défense, visibles des autres joueurs ; déplacement dans l'inventaire. Potions
  de vie et de mana, régénération des PV et PM hors combat. Parchemins de téléportation (village,
  aller-retour, destination) ; patrouille des monstres. Résurrection au point de la dernière région sûre
  traversée ; piles d'objets séparées, réunies, détruites. Compétences : apprentissage et amélioration
  chez les instructeurs, points de compétence, lancement ; dégâts, zones, renforcements et
  affaiblissements, dégâts sur la durée, soins, passives ; états spéciaux (étourdissement, sommeil,
  ralentissement…), coups critiques, blocage, dégâts changés en PV ou PM. Groupes (invitation, membres, chef, exclusion, partage de l'expérience, discussion). Guildes (création chez
  les gestionnaires, invitation, adhésion, départ, exclusion, dissolution, annonce, discussion). Banque du compte (objets, argent, pages en plus) et
  échange entre joueurs. Portails de téléportation (trajets payants entre points reliés). Quêtes : prise, progression (monstres, objets de quête), fin et récompenses, abandon. Voir
  [[Protocole réseau|FR-Protocole-reseau]].
- Extensions ArchRust : négociation de capacités, messages d'AddOn (murmure, `SAY`), et langue du client
  (version 1.1) : le serveur sert les noms des PNJ et des monstres dans la langue choisie, avec repli, et
  les autres textes à la demande ; le choix suit le compte. Voir
  [[Extensions ArchRust|FR-Extensions-ArchRust]] et [[Traduction|FR-Traduction]] ; côté ArchRust, sélecteur
  de langue dans les Options.
- Revente d'un objet à un PNJ.
- Client ArchRust (branche `client`) : combat (attaque, coups, mort, résurrection, butin), groupes, autres
  joueurs, inventaire et équipement, PNJ (dialogue, boutique, banque et achat de pages, quêtes), compétences
  (instructeurs, lancement, barre de raccourcis), portails de téléportation (fenêtre et chargement d'origine),
  guildes (greffier, fenêtres de création, de dissolution et de guilde, membres) et discussion de guilde et de
  groupe, packs de ressources graphismes et sons (activés, ordonnés et vérifiés depuis les Options ; voir le
  [guide](https://github.com/BlaMacfly/ArchCore/blob/client/ArchRust/docs/PACKS-DE-RESSOURCES.md)) ; validés contre ArchCore par des essais sans affichage et des captures du
  client graphique, à confirmer en jeu.

- Visibilité entre joueurs : déplacement relayé aux joueurs proches, apparition et disparition
  selon la distance.
- Discussion à portée de vue et chuchotements.
- Niveau MJ envoyé au panneau « Admin Commands ».
- Premières commandes MJ : taux, modération, objets, annonce de redémarrage, téléportation. Voir
  [[Administration|FR-Administration]].

## Prochaines étapes

1. Valider la création complète d'un personnage en jeu.
2. Vérifier la visibilité entre joueurs avec deux vrais clients.
3. Fonctions des PNJ : quêtes à objets d'interaction. Portails : zones de combat entre joueurs (confirmation,
   retour, op 7), refus en combat ou métamorphosé.
4. Compétences : renvoi de dégâts à l'attaquant, compétences des monstres, compétences de groupe ; maîtrises.
5. Combat : dégâts élémentaires des objets. Objets : raffinage, conversion.
6. Guildes : combats de guildes, alliances et hostilités, emblèmes, second et passation, agrandissement, liste
   des guildes, entrepôt. Dans ArchRust : nom de guilde au-dessus des têtes, chuchotement.
7. Reconnexion automatique (type 0x63).
8. Commandes MJ restantes de la page [[Administration|FR-Administration]] (arène, événements, failles…).
9. Fin de la traduction : environ 54 000 textes restants.

## Plus tard

- **PlayerBots** : module serveur de personnages joueurs contrôlés par le serveur, sur le modèle des
  PlayerBots des serveurs World of Warcraft, adapté à ArchLord (déplacement, combat, groupes, commerce).
  À commencer une fois les monstres, le combat et les groupes en place.
- **AddOns intégrés au client** : livrer avec ArchRust des AddOns prêts à l'emploi, intégrés au client et
  activables depuis le jeu, en s'appuyant sur le système d'AddOns Lua déjà en place. Premier AddOn :
  [ArchCoreAdmin](https://github.com/BlaMacfly/ArchCoreAdmin), le panneau MJ.
- **Éditeur de la base de données** : en cours dans [ArchForge](https://github.com/BlaMacfly/ArchForge) — un outil graphique libre, pour Windows, Linux et macOS, qui crée et
  modifie le contenu du monde sans écrire de SQL : créatures et leurs apparitions, marchands, instructeurs,
  quêtes, objets, tables de butin, dialogues, conditions et comportements des monstres. Chaque champ est
  expliqué, des sélecteurs aident à choisir les valeurs et les drapeaux, et la requête SQL produite est
  affichée (les seuls changements, ou l'entrée complète pour une sauvegarde), à copier ou à exécuter
  directement sur la base.

Les contributions sont bienvenues : ouvrir une *issue* pour proposer ou signaler quelque chose.
