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
  ralentissement…), coups critiques, blocage, dégâts changés en PV ou PM, contre-attaque (dégâts renvoyés au monstre) ; compétences de groupe ; maîtrises (compétences exigées des arbres supérieur et héroïque, niveaux dans la classe d'un modèle « 1722;30 », passives du Dragon Scion « 10004;4 ») ; compétences des monstres (IA des tables du serveur d'origine : lancement, dégâts, effets, coups chargés). Groupes (invitation, membres, chef, exclusion, partage de l'expérience, discussion). Guildes (création chez les gestionnaires, invitation, adhésion, départ, exclusion, dissolution, annonce, discussion, second et passation de chef, agrandissement, liste des guildes, emblèmes, combats de guildes, alliances et hostilités, entrepôt). Banque du compte (objets, argent, pages en plus) et
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
  échange, prix des PNJ, expérience), charisme gagné, rémission auprès des pardonneurs, compétences offensives sur
  les joueurs (Ctrl, zones) ; côté ArchRust, attaque et compétences avec Ctrl, messages, couleur des noms, annonces
  de zone et fenêtre `UI_Remission`. Les gardes restent passifs comme leur IA d'Archonia (`UseNormalAttack` 0) ;
  avis de recherche : voir plus bas.
- Monstres préventifs : ceux dont l'IA a `PreemptiveType` 1 attaquent d'eux-mêmes un joueur dans leur portée
  (`PreemptiveRange`), sauf les cibles que leurs conditions de niveau excluent. Validé au simulateur (scénario `aggro`). Validé au simulateur le 27 octobre 2026 (scénario
  `pk`). Montants des points de meurtrier et du charisme réglables, ceux d'origine étant inconnus.
- Production (types 0x17 et 0x3A) : compétences de production apprises chez les PNJ Adventurer, Cook et Alchemist
  (fonction 24), recettes achetées chez eux, récolte sur les cadavres de monstres (Collection Skill, couteau de
  récolte, objets de `productresultitem.txt` selon le niveau), cuisine et alchimie (matériaux, objet recette,
  outils), expérience et niveaux 1 à 15 (facteurs « dirt » 4 et 6 d'AGPMSKILL), recettes connues envoyées à l'entrée
  en jeu. Les règles de réussite, de quantité et d'expérience sont des choix d'ArchCore, décrits dans
  [[Protocole réseau|FR-Protocole-reseau]]. Validé au simulateur le 27 octobre 2026 (scénario `product`) ; fenêtres
  d'ArchRust (`UI_PDSkill`, `UI_PDSkillBuy`, `UI_Compose`) en cours, à confirmer en jeu.
- Amis et ignorés (type 0x2B) : demande d'ami relayée et acceptée des deux côtés, refus, retrait, état en ligne
  (op 6) et région d'un ami, liste d'ignorés en base fusionnée avec le fichier local du client, blocages appliqués au
  serveur (chuchotement, échange, invitations de groupe et de guilde). Courrier (type 0x2D) : lettre contre une feuille
  de papier à lettre, une pièce jointe, destinataire hors ligne, boîte envoyée à l'entrée en jeu, lecture,
  récupération, suppression. Règles d'ArchCore décrites dans [[Protocole réseau|FR-Protocole-reseau]]. Validé au
  simulateur le 28 octobre 2026 (scénarios `buddy` et `mail`), à confirmer en jeu ; la relation « Buddy » (mentor
  et disciple) est reportée.
- Usure et réparation (types 0x04 et 0x34) : durabilité (colonne `DUR`) enregistrée et envoyée au client (facteur
  « objet »), usure de l'arme à chaque coup porté et d'une armure à chaque coup reçu (`Item.WeaponWearChance`,
  `Item.ArmorWearChance`), objet à 0 sans effet jusqu'à sa réparation, réparation chez les forgerons et éleveurs
  (fonction 17) au prix du client. Montures (types 0x04 et 0x4D) : monter en équipant l'objet de partie 14, AgpmRide
  op 2 et op 4 diffusées, vitesse `Run_Buff`, vue par les autres (champ 18), refus en combat, pour un meurtrier et
  hors des régions permises, descente forcée à la mort. Règles d'ArchCore décrites dans
  [[Protocole réseau|FR-Protocole-reseau]]. Validé au simulateur le 28 octobre 2026 (scénarios `repair` et `ride`), à
  confirmer en jeu ; affichage par ArchRust en cours.
- Maison des ventes (type 0x0B) : planche ouverte chez les « Item Trader » (fonction 28), mise en vente de la pile
  entière à prix fixe en or et en CC (frais de 3 %), recherche par TID, achat immédiat au prix de la base, vente
  conclue annoncée au vendeur, encaissement, annulation, ventes envoyées à l'entrée en jeu. Commutateur d'équipement
  (type 0x6A) : 100 emplacements enregistrés, rééquipés par le chemin de l'équipement manuel, relus à l'entrée en jeu
  (Archonia n'a pas de bonus de panoplie). Horloge du monde commune à tous (AgpmTimer, vitesse `World.TimeSpeed`,
  renvoyée toutes les 10 min) et météo (type 0x30 : `/weather`, météo automatique `World.WeatherAuto`). Règles
  d'ArchCore décrites dans [[Protocole réseau|FR-Protocole-reseau]]. Validé au simulateur le 28 octobre 2026
  (scénarios `auction`, `itemset` et `nature`), à confirmer en jeu ; fenêtres d'ArchRust en cours. Les compétences de
  l'ArchLord sur le ciel (478, 522, 523) agissent sur cette horloge et cette météo (voir le titre d'ArchLord).
- Invocations (type 0x4F) : esprits de l'Élémentaliste, Sacred One, Legion of the Dead et autres invocations des
  compétences (créature au niveau du maître, `Summons.StatScale`), créatures fixes (pièges), Increase Gate (limite
  relevée), parchemins d'invocation et familiers des objets ; vue, état spécial puis op 0 dans l'ordre exigé par le
  client, suivi du maître, propensions attaque, défense et ombre (op 2), monstres qui ripostent contre l'invocation,
  expérience et butin au maître, fin à l'expiration, à la mort, à la téléportation ou au départ du maître et au-delà
  de la limite. Règles d'ArchCore décrites dans [[Protocole réseau|FR-Protocole-reseau]]. Validé au simulateur le
  28 octobre 2026 (scénario `summons`), à confirmer en jeu ; fenêtres de cible d'ArchRust à faire. Restent
  l'apprivoisement, Call on Nature et Evil Light (pas de créature dans les données) et les pièges de faille.
- Inventaire Cash et sac du familier (type 0x04, statuts 51 et 13) : objets marqués « Cash » rangés dans l'inventaire
  Cash (mini Cash au champ 23, objets Cash déjà en base déplacés à l'entrée en jeu) ; familier utilisé = objet « en
  cours d'utilisation » (op 26), rappel par réutilisation ou op 23 ; sac du familier de 4 × 2 × 4 cases disponible
  avec un familier de « PetType » 1 : dépôt depuis l'inventaire, l'équipement et la banque, retrait, réunion et
  séparation de piles, objets gardés au rappel, ramassage vers le sac quand l'inventaire est plein, bouton « Auto Pick
  Up » (op 30) enregistré. Écartés (ajouts d'Archonia) : marquage des sacs (ops 37/38), « Sell All », familiers 14821
  à 15674. Règles d'ArchCore décrites dans [[Protocole réseau|FR-Protocole-reseau]]. Validé au simulateur le
  1er novembre 2026 (scénario `petbag`), à confirmer en jeu ; fenêtres `UI_SubInventory` et `UI_CashInven` d'ArchRust
  à faire. Restent l'endurance des familiers (Pet Feed) et la boutique Chantra.
- Discussion : préfixes d'Archonia lus par le serveur (« /p », « /g », « /s » envoyés en type 0, avant les commandes
  MJ ; commande inconnue d'un joueur ignorée), cri local « [Shout] » (`Chat.ShoutRange`), cris de zone (type 0x4C :
  `/race`, `/scream`, `/warn`, un objet consommé par message, un cri par seconde), salons de discussion (type 0x2C :
  publics, privés, de guilde, invitations, recherche d'un joueur ; 5 par personnage, en mémoire). Coiffeurs (type
  0x43, fonction 29) : coiffure et visage de `charactercustomizelist.txt` achetés au prix du client, vus des autres
  joueurs ; apparence de création contrôlée. Avis de recherche (type 0x46, fonction 32) : droit de mise à prix donné
  à la victime d'un premier attaquant ou d'un meurtrier, prime de base selon le niveau, primes cumulées en base
  (`Wanted.ExpireDays`), tableau des 100 avis, prime touchée par le chasseur, expérience perdue par le criminel
  (`Wanted.ExpLossPercent`). Règles d'ArchCore décrites dans [[Protocole réseau|FR-Protocole-reseau]]. Validé au
  simulateur le 28 octobre 2026 (scénarios `chat`, `channel`, `customize` et `wanted`), à confirmer en jeu ;
  fenêtres d'ArchRust en cours. La règle de région `<Chat>No</Chat>` n'est pas appliquée (aucune dans les données).
- Combats de l'épisode 8 et zones de combat d'origine : Shrines (type 0x62, structure brute ; tables `shrine` et
  `shrine_reward` de la couche de contenu, les tables d'origine n'étant pas livrées) : huit Shrines ouverts chaque soir
  (`Shrine.Enabled`, `/shrine open|close`), entrée par niveau et effectif (codes 47 et 48), gravure au centre,
  occupation, victoire de la race, pièces 53000 et charisme, scores, messages de `shrinemessage.xml` ; règles de combat
  dans le Shrine (races ennemies sans Ctrl, ni points de meurtrier ni mise à prix, résurrection dans la zone de sa
  race). Battle Square : niveaux des étages importés, voyage par les superviseurs (`BattleSquare.SupervisorTeleport`,
  aucun portail n'y mène), codes 2, 13 et 18, meurtre sans pénalité, crânes de `pvpskulldroptable.txt`. Battleground :
  résurrection dans le fort de sa race, annonces 0x58 sur horaire (`Battleground.Schedule`). Fenêtre des boss
  d'Archonia (type 0x64) : état des boss de `UIBoss.xml`, butin possible, classement du dernier combat, compteur de
  dégâts ; heure du serveur dans AgpmTimer (champs 2 et 3). Règles d'ArchCore décrites dans [[Protocole
  réseau|FR-Protocole-reseau]]. Validé au simulateur le 28 octobre 2026 (scénarios `shrine`, `battlesquare` et
  `boss`), à confirmer en jeu ; l'op 8 des Shrines (sens inconnu) n'est que journalisée, la guerre des races
  d'Archonia (type 0x33) n'est pas faite, fenêtres d'ArchRust en cours.
- Guerres de siège des châteaux (types 0x49, 0x50 et 0x61) : châteaux de `SiegeWar.txt`, objets d'assaut de
  `SiegeWarObject.txt` et points de résurrection des sièges importés (tables `siege_war`, `siege_war_object`,
  `siege_binding`) ; propriétaire, calendrier et candidatures en base (`castle`, `castle_application`). Règles du
  client : déclaration par le propriétaire dans son créneau (jour de base, 20 à 22 h), candidatures du chef de guilde
  sur place (15 attaquants, 3 défenseurs dont 2 retenus, une guilde par château), commandants de siège (fonction 33),
  ordre d'attaque des objets, trône, Yeux d'Archon, carving du chef (le dernier gagne). Choix d'ArchCore : château sans
  propriétaire au départ, date fixée d'office faute de déclaration, tous les candidats attaquants retenus, siège annulé
  sans assaillant, l'ancien propriétaire garde le château sans carving réussi, seuils `Siege.MinGuildMembers` et
  `Siege.MinGuildAgeDays`, durée `Siege.DurationMinutes`, calendrier `Siege.Enabled`. Objets et gardes des sièges
  montrés seulement pendant le combat, variantes « en destruction » puis ruines ; combat sans Ctrl entre camps, ni
  points de meurtrier ni mise à prix ; résurrection de siège (AgpmCharacter 0x13 et 0x14) ; trajets des châteaux
  (`SpecialType` 2 à 4) ; catapultes et camps de résurrection ; tableau WarInfo ; commandes `/siege start|end|owner|hp`.
  Validé au simulateur le 28 octobre 2026 (scénario `siege`), à confirmer en jeu ; tours de garde et gardes ne visent
  plus que les assaillants ; terrain de chasse du seigneur à remplir et effet exact de la catapulte à établir.
- Titre d'ArchLord (type 0x55) : guerre de Rensphere tous les 30 jours à 20 h (`Siege.Enabled`) ou par `/siege start
  Rensphere`, guildes propriétaires d'un château en assaillantes, étapes 1 à 4 (portail, combat, bataille de l'ArchLord
  au statut 7, fin), sceau gravé par le chef (compétence 930) ; son chef devient ArchLord jusqu'à la guerre suivante
  (table `archlord`), annoncé à tous (ops 0 et 1) et à l'entrée en jeu. Compétences de `skill_type` 9 données à
  l'ArchLord et refusées aux autres : Call Dragon (Alcadia, AgpmRide op 6), Call Royal Knight et `/guardactivate`,
  `/guarddeactivate`, `/guardinformation` (gardes royaux, `Archlord.MaxGuards`), Rain et Snow Control (pluie ou neige
  5 min pour tous), Day and Night (horloge décalée de 12 h, `Archlord.SkyMinutes`), `/archlord <message>` à tout le
  serveur ; commandes d'essai `/archlord set|clear`, `/archlordbegin`, `/archlordend`, `/nextstep`…
- Taxes des régions (type 0x53) : intendants « Grand Advisor » (fonction 34), taux de 0 à 50 % des régions du château et
  de ses abords fixés par le propriétaire une fois par semaine (codes 0 à 3), op 4 à tous avec le nom de région traduit
  par le `RegionGlossary.txt` de la langue du joueur (table `region_glossary`, importée) ; prix + prix × taux / 100 aux
  achats chez les PNJ, réparation, coiffeur, portails, maître des compétences, production et création de guilde (ni
  ventes aux PNJ ni ventes entre joueurs) ; recettes par région (`castle_tax`, `castle_tax_region`), versées chaque
  lundi à 0 h (`Tax.PayoutDay`, `Tax.PayoutHour`) à la banque du chef de la guilde propriétaire, moins le tribut de
  20 % à l'ArchLord ; `/tax` et `/tax pay`. Validé au simulateur le 28 octobre 2026 (scénarios `archlord` et `tax`),
  à confirmer en jeu ; la compétence 472 Evil Light (invocation de nature 7) et l'objet d'Archon des gardes restent à
  établir, les compétences retirées ne disparaissent de la barre du client qu'à la reconnexion.
- Anti-triche côté serveur (`world/anticheat.rs`, aucun contrôle côté client) : positions envoyées par le client
  bornées par un crédit de distance (vitesse de course courante × `AntiCheat.SpeedTolerance` + `AntiCheat.MoveSlack`,
  jusqu'à 5 s de lag), position valide renvoyée au-delà ; téléportations du serveur, entrée et reconnexion comme points
  de départ, MJ exemptés ; cadence des coups normaux bornée après un changement de cible ou une reprise ; débit de
  paquets par connexion (`AntiCheat.MaxPacketsPerSecond`), inondation déconnectée ; table `log_anomaly`, sanctions
  `log`, `kick` ou `ban` au-delà d'un seuil, `/anomalies`, `/move x,z "nom"` pour poser un joueur. Audit des
  duplications : raffinage avec un même objet cité deux fois (ingrédients comptés double) fermé ; échange annulé si un
  objet proposé a changé depuis sa proposition, échange qui attend de nouveau les deux verrous après un
  déverrouillage (le partenaire garde le sien, comme le client de référence), demande non acceptable par son auteur ;
  objets proposés ni convertis, ni vendus, ni raffinés, ni déposés à l'entrepôt ; or au
  sol laissé au-delà du plafond ; seconde entrée en jeu sur une même connexion. Validé au simulateur le
  29 octobre 2026 (scénario `cheat`, et les scénarios existants), à confirmer en jeu.
- Retour de test n° 1 (29 octobre 2026), côté serveur : portées d'usage des PNJ et des objets alignées sur le code
  partagé d'Archonia (600 dialogue et boutique, 800 banque, conversion, guilde, maître des compétences et coiffeur,
  1600 réparation, quêtes, production, ventes, portails et sphères d'alchimie, au lieu de 1500 partout), mesurées
  sur la position suivie à l'instant ; raffinage limité aux 20 sphères d'alchimie des `obj*.ini` ; points de portail
  désactivés (destinations sans objet d'arrivée ou interdites à la race) ; arbre héroïque du Berserker lu (deux
  arbres, colonnes décalées). Validé au simulateur, à confirmer en jeu.
- Métamorphose (`world/morph.rs`) : potions de type d'usage 7 (101 objets, forme « Polymorph Id », durée « Polymorph
  Dur ») et bonus d'ItemTransformTable.txt (table `item_transform`) ajoutés aux facteurs ; AGPMCHARACTER op 11 au joueur
  et à ceux qui le voient (vue : op 0 au TID d'origine, bit 0 du champ 18, puis op 11), op 12 à la fin de la durée, à
  l'annulation (op 13 du client) ou à la mort ; AGPMITEM op 4, op 19 (recharge commune de 10 min), op 27 code 4 à cheval,
  transformé ou évolué, code 2 en zone de siège ; forme perdue à la déconnexion, recharge propre à la session. Refus
  du client répétés (compétences, parchemins, Potions of Quickness et Beam, portails, Shrines, PNJ, coiffeur, échange
  privé si l'un des deux est métamorphosé), sauf pour l'ArchLord ; équipement permis avec le contrôle ordinaire sur les
  facteurs d'origine (`FUN_1401bca10`), retrait contrôlé sans niveau sous métamorphose ou évolution (refusé sous une
  forme évoluée qui n'admet pas l'objet), monture toujours refusée sous métamorphose (choix d'ArchCore) ; race
  d'origine pour le combat entre joueurs. Forme de créature (Type 8 de CharacterDataTable : animaux de la Mutation,
  potions 408 à 416) : le joueur n'attaque personne et aucun joueur ne l'attaque ; forme sans 0x20000 : pas la cible
  d'une compétence sur ennemi. Mutation (`transform_target` 4) : forme tirée dans `creature_tid` pour 7 s, sans
  annulation, sur un joueur ou un monstre (ni invocation, ni objet de siège, ni boss de la fenêtre des boss) ; le
  monstre s'arrête, son IA attend la fin, il n'est ni attaquable ni ciblable sous une forme d'animal (op 11 / op 12 à
  ceux qui le voient, op 11 après sa vue) ; immunité ensuite de 30 s (`downtime_duration` de SKILL_SPEC, sens déduit,
  `Skill.DowntimeSeconds`). `Item.TransformMaxDurationMs` abrège la durée (simulateur). Validé au simulateur le
  1er novembre 2026 (scénario `morph` : équipement et échange sous métamorphose, Mutation d'un Ver vert), à confirmer
  en jeu ; côté ArchRust, à reprendre : équipement non redessiné pendant la forme, menu d'échange, cible d'animal.
- Évolution du Dragon Scion (`world/evolution.rs`) : Slayer, Orbiter et Summoner of Evolution (1189, 1192, 1195),
  apprises chez les maîtres du Dragon Scion dès le niveau 20 ; lancement → AGPMSKILL op 4 (effet sans durée) puis
  AGPMCHARACTER op 26 (1722, 1723, 1732) ; d'une forme à l'autre, op 27 puis op 26 ; forme active relancée refusée ;
  annulation par AGPMSKILL op 7 action 4 → op 5 et op 27 (1724). Facteurs et croissance de la forme, PV et PM bornés,
  compétences et passives selon les colonnes `require_class_*` de la forme, armes non admises ne comptant plus, objet
  d'oubli des compétences refusé ; forme enregistrée (`characters.evolution_tid`), gardée à la mort et rendue à
  l'entrée (TID de la forme, bit 2 du champ 18). Validé au simulateur le 1er novembre 2026 (scénario `evolution`), à
  confirmer en jeu ; le client ArchRust doit encore traiter ces paquets.
- Caractéristiques secondaires de la fenêtre du personnage (`world/effects.rs`) : AGPMSKILL op 3, champ 9 (bloc de
  220 octets : taux et dégâts critiques, coup reçu changé en PV, défense et résistance ignorées, résistance aux
  critiques et à l'étourdissement) envoyé au joueur à l'entrée en jeu et à chaque changement de ses effets, part des
  compétences seule (passives comprises) : le client ajoute lui-même celle des objets. Résistance aux critiques
  (`critical_defence_percent2`, option `Critical_Defense_Rate`) ôtée de la chance de coup critique de l'assaillant
  (règle déduite) ; résistance à l'étourdissement (`stun_resistance`, `StunResistance`) lue, affichée seulement ;
  option à `Critical` sans `skill_type` (13822) comptée en dégâts critiques, comme le client. Validé au simulateur le
  1er novembre 2026 (scénario `ignore`), à confirmer en jeu avec la fenêtre d'ArchRust.
- Consommables, mort et oubli des compétences (`world/consumables.rs`) : objets à compétence (ragoûts, potions de
  vitesse, pierres, ailes, charmes, costumes, talismans de Janus : « Use skill id » au « Use skill level », listes
  « ; ») qui posent leur effet sans incantation, durée du niveau sinon de l'objet (« Apply Effect Count » × « Apply
  Effect Time », « RemainTime » / « ExpireTime » en minutes), recharge « Use Interval » + nombre × durée par objet et
  `recast_time` par compétence (refus : AGPMITEM op 5), familles `skill_classify_id` / `_level` (rang égal ou
  supérieur remplace, inférieur refusé), objets « CanStopUsingItem » gardés et éteints à la réutilisation ; défense
  élémentaire des effets (« def_point_* ») ; bonus d'expérience et de butin (« bonus_exp », « bonus_drop_rate »,
  unité réglable `Skill.GameBonusDivisor`). Orbe de résurrection (usage 14) : mort seulement (op 5 sinon), refus
  op 27 code 3 (ArchLord, siège en guerre, région sans le bit 0x100, région de combat sans orbe Cash, niveaux,
  recharge) ; sur place, PV et PM pleins, invincible 10 s, op 4 puis op 18. Oubli des compétences par la Gem of
  origin (usage 27) ou chez le maître (AgpmEventSkillMaster op 8 → op 9, une fois, or des niveaux rendu, chaîne
  « skill init » en base, `characters.skill_init`, champ 19 et op 23) ; compétences d'ArchLord, de production et
  attaque normale gardées. Perte d'expérience au village réglable (`Death.ExpPenaltyPercent`,
  `Death.ExpPenaltyRoguePercent`), désactivée par défaut comme dans Archonia, jamais de niveau perdu. Validé au
  simulateur le 1er novembre 2026 (scénario `consumables`), à confirmer en jeu ; reste le temps de vie des objets à
  durée (non décompté) et les effets d'objet perdus à la mort et à la déconnexion.
- Petits systèmes de l'épisode 8 : rappel de groupe (Summoning Orb, Bead of Summoning, usage 15 : AGPMITEM op 4 au
  lanceur, gestion op 15 aux membres, op 16 accepte et téléporte près du lanceur, op 17 refuse ; refus au mort, en
  siège, dans un Shrine en combat, dans la Battle Square, dans une région sans le bit 0x100 et en combat, réglages
  `Recall.*`) ; actions sociales (AGPMCHARACTER op 16, types 0 à 16, renvoyées au lanceur et à ses voisins, refus au
  mort, à cheval, sous métamorphose, `Social.MinIntervalMs`) ; boutique des Shrines (prix en charisme et en Shrine
  Coins importés d'itemdatatable, quantité, codes 10 et 11 ; aucun PNJ des données n'en vend, mécanisme prêt) ; barre
  de raccourcis gardée par le serveur (AgpmUIStatus : 50 cases à l'entrée, op 1 enregistrée, table
  `character_quickbelt`, compétences retrouvées par TID, objet consommé remplacé) ; retour à la sélection des
  personnages (AgpmReturnToLogin 0x2F : op 1 → op 5 avec une clé, op 6 au serveur de connexion, op 7 après la
  connexion). Validés au simulateur le 2 novembre 2026 (scénarios `recall`, `social`, `quickbelt`, `returnlogin`), à
  confirmer en jeu ; reste à choisir quels PNJ tiennent la boutique des Shrines.
- Titres de l'épisode 8 (AgpmTitle, type 0x60, structure brute) : 684 titres de `titledatatable.txt` importés (table
  `title_template` ; Expert Collector, Addict, Advanced, Researcher ; Defender of Chantra et Schola 1-3 derrière
  `Title.Schola`, Christmas Event et Schola 4/5 d'Archonia écartés), rangs de charisme (`charisma_title`). Listes et
  titre actif poussés à l'entrée (op 5, 6.7, op 9) ; « <Title Manager> » (op 10 au lieu du dialogue) ; objectif fixé
  avec les contrôles du client et ses bornes (999, 500), or et objets de coût pris (6.0 → 6.1, 6.3) ; conditions de
  récolte sur une créature, d'utilisation d'un objet, de niveau et de niveaux de production suivies (6.3) ; titre
  obtenu (6.5 puis op 1), équipé ou retiré (op 3, op 9 au joueur, à ses voisins et à l'observateur qui le voit
  arriver ; un changement toutes les 2 h, `Title.ChangeDelaySeconds`, refus code 3), supprimé ou abandonné (op 8) ;
  effets du titre équipé seulement : compétences `Skill_Tid_*` en passives, réussite de production (+10 %), récolte
  doublée sur sa créature, potions renforcées ; tables `character_titles`, `character_title_goals`,
  `characters.title_changed_at` ; `/title`. Validé au simulateur le 2 novembre 2026 (scénario `title`), à confirmer en
  jeu avec les fenêtres d'ArchRust ; moment d'envoi des listes, consommation des coûts et sens de la constante 2 de
  l'op 2 choisis par ArchCore (le serveur d'origine est absent).
- Client ArchRust ([dépôt à part](https://github.com/BlaMacfly/ArchRust)) : combat (attaque, coups, mort, résurrection, butin), groupes, autres
  joueurs, inventaire et équipement, PNJ (dialogue, boutique, banque et achat de pages, quêtes), compétences
  (instructeurs, lancement, barre de raccourcis), portails de téléportation (fenêtre et chargement d'origine, confirmation des zones de combat entre joueurs,
  « Retour », refus en combat), reconnexion automatique (fenêtre `UI_ReconnectHold`, 60 s),
  guildes (greffier, fenêtres de création, de dissolution et de guilde, membres, nom de guilde au-dessus des têtes) et discussion de
  guilde et de groupe, chuchotements (barre privée, `/w`, `/r`, bouton de la fenêtre de guilde), packs de ressources graphismes et sons (activés, ordonnés et vérifiés depuis les Options ; voir le
  [guide](https://github.com/BlaMacfly/ArchRust/blob/main/docs/PACKS-DE-RESSOURCES.md)), fenêtres d'origine d'Archonia à la place des fenêtres provisoires (confirmations,
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
3. Fonctions des PNJ : quêtes à objets d'interaction. Menus composés par le serveur (AgpmEventNPCDialog ops 2 et 3,
   boîtes de message et boutons) des PNJ « <Ville> Event », « Portal to Chaotic Frontier », « Portal to Town » et de
   Jaig : le client sait les afficher, mais leur contenu n'est ni dans le client ni dans ses données ; en attente d'une
   source. Portails : refus des métamorphosés fait (morph.rs), à confirmer en jeu ; les entrées des forts de la
   Battle Ground (`TeleportPoint.ini`) n'ont pas d'objet dans les `obj*.ini` d'Archonia.
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
10. Relation « Buddy » (mentor et disciple, AgpmBuddy ops 10 à 14) : contraintes `MentorConstrict` et effet « inventaire
    vidé » inconnus ; en attente d'une source fiable.

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
