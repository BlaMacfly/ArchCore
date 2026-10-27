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
  l'envoi des PV, ajouté depuis, reste à vérifier. Niveaux repris du client Archonia et réapparition des
  boss d'après sa fenêtre des boss ; bilan de fidélité dans [[Épisode du jeu|FR-Episode]].
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
  ralentissement…), coups critiques, blocage, dégâts changés en PV ou PM, contre-attaque (dégâts renvoyés au monstre) ; compétences de groupe ; maîtrises (compétences exigées des arbres supérieur et héroïque) ; compétences des monstres (IA des tables du serveur d'origine : lancement, dégâts, effets, coups chargés). Groupes (invitation, membres, chef, exclusion, partage de l'expérience, discussion). Guildes (création chez les gestionnaires, invitation, adhésion, départ, exclusion, dissolution, annonce, discussion, second et passation de chef, agrandissement, liste des guildes, emblèmes, combats de guildes, alliances et hostilités, entrepôt). Banque du compte (objets, argent, pages en plus) et
  échange entre joueurs. Portails de téléportation (trajets payants entre points reliés, zones de combat
  entre joueurs et « Retour », refus en combat). Reconnexion automatique (type 0x63). Commande MJ
  `/last_refine`. Quêtes : prise, progression (monstres, objets de quête), fin et récompenses, abandon. Voir
  [[Protocole réseau|FR-Protocole-reseau]].
- Extensions ArchRust : négociation de capacités, messages d'AddOn (murmure, `SAY`), et langue du client
  (version 1.1) : le serveur sert les noms des PNJ et des monstres dans la langue choisie, avec repli, et
  les autres textes à la demande ; le choix suit le compte. Voir
  [[Extensions ArchRust|FR-Extensions-ArchRust]] et [[Traduction|FR-Traduction]] ; côté ArchRust, sélecteur
  de langue dans les Options.
- Revente d'un objet à un PNJ.
- Combat entre joueurs : règles d'attaque d'Archonia (zones, niveau 20, Ctrl, amis, groupe, Battlegrounds),
  ennemis déclarés, premier attaquant, victoires et défaites, points de meurtrier et leurs pénalités (groupe,
  échange, prix des PNJ, expérience), charisme gagné, rémission auprès des pardonneurs ; côté ArchRust, attaque
  avec Ctrl, messages, couleur des noms et annonces de zone. Validé au simulateur le 27 octobre 2026 (scénario
  `pk`). Montants des points de meurtrier et du charisme réglables, ceux d'origine étant inconnus.
- Client ArchRust (branche `client`) : combat (attaque, coups, mort, résurrection, butin), groupes, autres
  joueurs, inventaire et équipement, PNJ (dialogue, boutique, banque et achat de pages, quêtes), compétences
  (instructeurs, lancement, barre de raccourcis), portails de téléportation (fenêtre et chargement d'origine, confirmation des zones de combat entre joueurs,
  « Retour », refus en combat), reconnexion automatique (fenêtre `UI_ReconnectHold`, 60 s),
  guildes (greffier, fenêtres de création, de dissolution et de guilde, membres, nom de guilde au-dessus des têtes) et discussion de
  guilde et de groupe, chuchotements (barre privée, `/w`, `/r`, bouton de la fenêtre de guilde), packs de ressources graphismes et sons (activés, ordonnés et vérifiés depuis les Options ; voir le
  [guide](https://github.com/BlaMacfly/ArchCore/blob/client/ArchRust/docs/PACKS-DE-RESSOURCES.md)), fenêtres d'origine d'Archonia à la place des fenêtres provisoires (confirmations,
  séparation de pile, achat, banque, groupe, menus principaux, sélection et création du personnage, options),
  vitesse de déplacement comme Archonia (`/speed` du MJ visible), potions bues automatiquement sous le seuil
  des curseurs des jauges, options GFX, Affichage et Fonction branchées, AddOns activables, textes du client dans
  la langue choisie ; validés contre ArchCore par des essais sans affichage et des captures du
  client graphique, à confirmer en jeu.

- Visibilité entre joueurs : déplacement relayé aux joueurs proches, apparition et disparition
  selon la distance.
- Discussion à portée de vue et chuchotements.
- Niveau MJ envoyé au panneau « Admin Commands ».
- Premières commandes MJ : taux, modération, objets, annonce de redémarrage, téléportation. Voir
  [[Administration|FR-Administration]].

## Prochaines étapes

1. Création complète d'un personnage et visibilité entre deux joueurs : vérifiées le 25 octobre 2026 avec
   deux clients ArchRust sur le serveur de test (création, entrée à Golundo, refus d'un nom déjà pris,
   joueurs visibles l'un de l'autre). Reste à les confirmer avec le client Archonia.
3. Fonctions des PNJ : quêtes à objets d'interaction. Portails : points désactivés (champ 5 de l'op 5) et
   trajets des guerres de siège ; refus des métamorphosés à vérifier quand la métamorphose existera.
4. Compétences : renvoi de dégâts (contre-attaque), compétences des monstres, compétences de groupe et maîtrises
   validés au simulateur le 25 octobre 2026 ; partage des dégâts (Divine Protection, Martyr's Mark), purges
   (Antimagic, Cure, Disentangle…), défense ignorée (Lethal Attack, Stunner), points héroïques gagnés au niveau et
   charisme (enregistré, dépensé par les compétences héroïques ; son gain attend le combat entre joueurs, `/charisma` en attendant)
   validés au simulateur le 26 octobre 2026 ; tout reste à confirmer en jeu. Restent les critiques des compétences.
5. Conversion des objets (sockets, pierres d'esprit, renforcement) et dégâts élémentaires des coups normaux :
   validés au simulateur le 25 octobre 2026, à confirmer en jeu ; options des objets (tirage à la création,
   effet sur le porteur) ; runes ; runes à compétence (pierres de prière de glace, de foudre, de poison) et options à
   déclenchement (critique, étourdissement, blessure, PV rendus, régénération, esquive, défense ignorée) validées au
   simulateur le 26 octobre 2026, à confirmer en jeu ; pierres de glace, de foudre, de venin et de caractéristiques
   d'Archonia refusées comme par le client ; options liées des objets uniques (`Link_id`) et raffinage (sphère d'alchimie, devis, grille et recettes) faits.
6. Guildes : discussion d'alliance ; effets de l'hostilité en combat (à établir).
7. Reconnexion automatique : à confirmer avec le client Archonia.
8. Commandes MJ restantes de la page [[Administration|FR-Administration]] : elles attendent leurs systèmes
   (arène, événements, failles, réglages de région…), listés à la fin de sa section 4.
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
