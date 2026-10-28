# Base de données

ArchCore utilise trois bases MariaDB principales, plus une base de journaux, créées et mises à jour par
`archcore db-init` à partir de `sql/` (base de référence et migrations, voir plus bas).
Toutes sont en `utf8mb4`.

> **Éditeur.** [ArchForge](https://github.com/BlaMacfly/ArchForge) modifie le contenu de ces bases sans écrire de
> SQL et peut ajouter ses changements à la couche de contenu (`content/*.sql`), qui survit aux imports.

## `archcore_auth` — comptes et mondes (`sql/001_auth.sql`, `sql/migrations/auth/`)

| Table | Contenu |
|---|---|
| `account` | Comptes : nom, empreinte SHA-256 du mot de passe, niveau MJ, ancienne date de fin de bannissement (`banned_until`, encore lue pendant une version), dernière connexion, clé d'entrée dans le monde (unique, effacée une fois utilisée), langue (dernier choix d'un client ArchRust, voir [[Traduction|FR-Traduction]]) |
| `account_banned` | Bannissements, avec leur historique : compte, date, fin (`NULL` : sans fin), auteur, raison, `active` (0 : levé avant son terme). Un compte est refusé tant qu'un bannissement actif n'est pas échu |
| `server_group` | Groupes de serveurs affichés dans la liste |
| `world` | Mondes : nom, groupe, adresse `ip:port` (23 caractères max), statut |

## `archcore_characters` — personnages (`sql/002_characters.sql`)

| Table | Contenu |
|---|---|
| `characters` | Compte, emplacement, nom unique, modèle (TID), niveau, expérience, PV/PM/PE, position, argent, banque, coiffure, visage, charisme, suppression logique |
| `character_items` | Objets : statut 0 inventaire, 1 équipé, 2 banque (rattaché au personnage qui l'a déposé), 3 entrepôt de guilde (`guild_id` ; le personnage qui l'a déposé reste noté), 4 pièce jointe d'un courrier (rattachée au destinataire), 5 en vente dans la maison des ventes (rattaché au vendeur) ; `durability` : -1 pleine (colonne DUR du modèle), sinon durabilité actuelle (usure, `repair.rs`) |
| `character_quests` | Quêtes en cours (compteurs des monstres A et B) et terminées |
| `account_bank` | Banque du compte : argent, pages en plus de la première (créée à la première entrée, avec l'argent que les personnages avaient en banque) |
| `character_skills` | Compétences apprises et leur niveau (« Normal Attack » n'y figure pas : tout personnage la connaît) ; `exp` : expérience cumulée d'une compétence de production |
| `character_recipes` | Recettes de cuisine et d'alchimie achetées (`recipe_tid` = `product_compose.result_tid`) |
| `guild` | Guildes : nom unique, mot de passe de dissolution, annonce, taille, date de création, emblème (`mark_tid`, `mark_color`), bilan des combats (`win`, `draw`, `lose`, `points`), alliance (`joint_leader` : la fondatrice, `joint_at`) (`sql/migrations/characters/`) |
| `guild_hostile` | Hostilités entre guildes, une ligne par sens (`guild_id`, `other_id`, `since`) |
| `guild_warehouse` | Entrepôt de guilde : argent, pages en plus de la première |
| `guild_member` | Membre d'une guilde (un personnage au plus dans une) : rang (1 membre, 2 demande d'adhésion, 3 demande de départ, 10 chef, 11 second), date d'adhésion, date de la demande de départ |
| `character_buddy` | Amis et ignorés : l'autre personnage (`other_id`), drapeaux du paquet (0x02000000 ami, sinon ignoré avec ses blocages 0x1 discussion, 0x2 échange, 0x4 invitations), relation « Buddy » (inutilisée) |
| `mail` | Courrier : destinataire, expéditeur (nom gardé si le personnage est supprimé), objet, texte, date (secondes Unix), drapeaux (0x01 lu, 0x10 pièce récupérée), pièce jointe (`item_id` → `character_items` au statut 4) |
| `auction_sale` | Maison des ventes : vendeur, objet en vente (`item_id` → `character_items` au statut 5, NULL une fois vendu), TID, quantité, prix en or et en CC, frais prélevés, statut (0 en vente, 1 vendu à encaisser), dates d'enregistrement et d'échéance, acheteur ; l'identifiant est aussi le docID du paquet |
| `character_item_set` | Commutateur d'équipement : emplacement (0 à 99) et ses objets (« E,partie,identifiant » séparés par « : ») ; l'emplacement porté est `characters.item_set_active` (-1 aucun) |
| `wanted_criminal` | Avis de recherche : criminel, prime cumulée, première et dernière mise à prix, échéance (`Wanted.ExpireDays` après la dernière) ; effacé à la capture |
| `wanted_bounty` | Historique des mises à prix : criminel, poseur, montant, date ; chasseur qui a touché la prime et date de la capture |
| `shrine_score`, `shrine_race_score` | Combats de Shrine : victoires et défaites de chaque personnage et de chaque race présents à la fin d'un combat gagné (AgpmShrineBattle op 11 et 12) |
| `character_boss_kill` | Fenêtre des boss : victoires de chaque personnage sur chaque boss de `UIBoss.xml` (une par combattant au combat où il meurt), date de la dernière |
| `castle`, `castle_application` | Guerres de siège : propriétaire de chaque château, statut (0 à 2 ; un siège en cours au redémarrage est clos sans changement), dates du dernier et du prochain siège, fins des candidatures et du délai de déclaration, guilde du dernier carving ; candidatures (camp 1 attaque, 2 défense, retenue ou non) |
| `archlord` | Titre d'ArchLord : une ligne (id 1), personnage en titre (`NULL` : aucun), nom, depuis quand |
| `castle_tax`, `castle_tax_region` | Taxes des régions : par château, recette totale versée, dernier versement, dernière modification des taux, début de la période ; par région (château et abords), château, taux (0 à 50 %) et recette depuis le dernier versement |
| `character_ui_status` | Statut d'interface : potions de vie et de mana de la barre, seuils de leur prise automatique (%), affichage du casque |

## `archcore_world` — données du jeu (`sql/003_world.sql`)

Remplie par `archcore import` à partir des tables du client.

| Table | Source dans le client | Contenu |
|---|---|---|
| `character_template` | `charactertemplate.ini` | Modèles de personnages et de PNJ : race, classe, statistiques |
| `character_data_table` | `CharacterDataTable.txt` | Caractéristiques serveur par modèle (PV, vitesses, dégâts, expérience…) ; noms des modèles de base jouables |
| `level_exp` | `LevelUpExp.txt` | Expérience par niveau |
| `growup_factor` | `GrowUpFactor.txt` | Progression des statistiques |
| `login_char_pos` | `logincharpos.ini` | Positions de l'écran de sélection |
| `item_template` | modèles d'objets | Objets |
| `skill_template` | `SKILLTEMPLATE.ini` | Compétences (TID, nom) |
| `skill_spec` | `SKILL_SPEC.txt` | Nature de chaque compétence (colonnes non vides, en JSON) |
| `quest_template` | `questtemplate.ini` | Quêtes (colonnes non vides, en JSON) |
| `quest_group` | `questgroup.ini` | Groupes de PNJ : quêtes proposées et rendues |
| `skill_level` | `SKILL_CONST.txt`, `Skill_Const2.txt` | Valeurs de chaque niveau de chaque compétence (colonnes non vides, en JSON ; groupes `point.`, `percent.`, `magnify.` ; celles de `Skill_Const2.txt`, suffixées de « 2 », sous `point2.`, `percent2.`…) |
| `skill_mastery` | `SkillMastery_High.txt`, `SkillMastery_Heroic.txt` | Arbres de maîtrise : classe (TID du modèle), compétence, arbre (1 supérieur, 2 héroïque), compétences exigées `[[TID, points]…]`, total de points dépensés |
| `product_category`, `product_compose`, `product_exp`, `product_gather`, `product_result_item` | `productcategory.txt`, `productcompose.txt`, `productexp.txt`, `productgather.txt`, `productresultitem.txt` | Production : compétence de chaque catégorie, 95 recettes (matériaux `[[TID, quantité]…]`, prix, niveau, chance), niveaux 1 à 15, 760 monstres récoltables, objets récoltés par ensemble et niveau (noms de compétence résolus par `SKILLTEMPLATE.ini`) |
| `npc_spawn` | `npc.ini` | 567 PNJ : modèle, nom, position, orientation, fonctions brutes |
| `client_table` | autres tables `.txt` | Lignes en JSON, pour les tables pas encore modélisées |
| `monster_spawn` | tables du serveur d'origine (`spawndatatable.txt` + `spawninstancetable.txt`) | 12 802 points d'apparition, 36 960 créatures : modèle, niveau (celui du client), nombre, réapparition (plage `respawn_sec`–`respawn_max_sec` pour les boss de `UIBoss.xml`), position, rayon, nom public (`CharacterTemplatePublic.ini`) |
| `map_segment` | `world/node_data` du serveur d'origine | 45 312 secteurs : sol infranchissable et région par segment de 400 unités |
| `map_height` | `world/b*x.ma2` du client (géométrie du sol) | 46 592 secteurs : hauteur du sol aux 17 × 17 coins de cellule (i16) |
| `monster_drop` | `itemdroptable.txt` du serveur d'origine | 4 653 objets propres à 550 monstres : quantité, taux sur 100 000 |
| `drop_group` | `groupdroprate.txt` du serveur d'origine | 28 groupes de butin ; les objets de chaque groupe viennent d'`item_data` (GroupID, DropRank, DropRate) |
| `drop_rank_rate` | `droprankrate.txt` du serveur d'origine | Rangs d'objets (pour cent) selon le rang de butin du monstre |
| `monster_ai` | `aidatatable.txt` du serveur d'origine | 7 674 IA (3 289 avec des compétences), désignées par `monster_spawn.ai_tid` : colonnes non vides en JSON, les colonnes répétées numérotées (`Skill1`, `SkillCondition1`, `SkillCount1`…) |
| `region_template` | `RegionTemplate.ini` | 331 régions : nom, région parente, champ Type, sûreté (33 sûres), niveaux d'entrée `level_min` et `level_limit` (LevelMin, LevelLimit : étages de la Battle Square) |
| `region_glossary` | `RegionGlossary.txt` de chaque dossier de langue du client (`ini`, `ini_fr`…) | Nom affiché de chaque région par langue (clé : nom coréen de `RegionTemplate.ini`) : nom des régions taxées envoyé au client (AgpmTax) |
| `boss_window` | `UIBoss.xml` | 25 boss de la fenêtre des boss d'Archonia : TID, ordre, catégorie (1 à 7), nom affiché |
| `pvp_skull_chance`, `pvp_skull_level` | `pvpskulldroptable.txt` | Crânes de la Battle Square : chance (%) selon l'écart de niveau tueur − victime, niveau du crâne selon le niveau de la victime |
| `resurrection_point` | `obj*.ini` (événement 19, type 1) | 62 points de résurrection : région, position, race et classe admises |
| `siege_binding` | `obj*.ini` (événement 19, types 3 à 7) | 27 points de résurrection des sièges : région, type (3 assaillants, 4 défense intérieure, 7 défense extérieure, 5 et 6 Archlord), position |
| `siege_war` | `SiegeWar.txt` | 5 châteaux (0 Rensphere à 4 Drayger) : régions du château et des abords, durées du combat, des Yeux d'Archon et du carving, délais de déclaration et de candidature, écart entre deux sièges, heure de début, compétence du carving |
| `siege_war_object` | `SiegeWarObject.txt` | 2 objets d'assaut (9 camp, 10 catapulte) : réparations, coût, durée, objets requis (JSON) |
| `teleport_point` | `TeleportPoint.ini` | 301 points de téléportation : rayon d'arrivée, races admises, groupes (arrivée, destinations) |
| `teleport_object` | `obj*.ini` (événement 8) | 291 portails : objet, point porté, position |
| `teleport_fee` | `TeleportFee.txt` | 227 prix de trajet (départ, arrivée) |
| `bank_slot_price` | `bankslotprice.txt` | Prix des pages de banque en plus (-1 : pas en vente contre de l'or) |
| `character_customize` | `charactercustomizelist.txt` | Coiffures (1) et visages (2) par modèle : numéro (champs 20 et 21 d'AGPMCHARACTER), nom, `Case` (0 création et coiffeur, 1 coiffeur, 2 création), niveau exigé, prix en or ou en objets, ordre du fichier |
| `guild_max_member` | `guildmaxmember.txt` | Tailles de guilde et leur prix ; la première ligne est la guilde neuve (50 places, 10 000 pièces), les suivantes les agrandissements |
| `guild_mark` | `guildmark.txt` | Éléments d'emblème : couche (1 fond, 2 motif, 3 symbole, 4 couleur), numéro, TID (numéro << 8 × (couche − 1)), nom, prix en or, crânes, argent liquide, couleur ARGB |

**Identifiants stables.** Dans `monster_spawn`, `monster_drop` et `resurrection_point`, la colonne `id` n'est qu'un
numéro d'ordre d'import. La colonne `stable_key` (unique, sensible à la casse) désigne la même ligne d'un import
à l'autre tant que les sources ne changent pas, pour un outil qui doit viser une ligne précise (éditeur, couche
de contenu) : `point#rang` pour une apparition (rang parmi les lignes du même point), `monstre:objet#rang` pour
un butin, `fichier#objet#rang` pour un point de résurrection (fichier `obj*.ini`, section de l'objet). Celle des
points de résurrection est remplie par `archcore import`.

### Langues (`sql/004_locale.sql`, même base)

| Table | Contenu |
|---|---|
| `language` | Langues proposées aux clients : code BCP 47, noms, niveau (1 version officielle, 2 client Webzen 2010, 3 communauté), dossier de textes du client, `enabled`, ordre ; 11 lignes insérées par `db-init` |
| `localized_text` | Textes localisés : `kind` (`npc_name`, `monster_name`, `item_name`, `item_desc`, `quest_name`, `quest_desc`, `skill_name`, `region_name`, `npc_dialog`), `ref_id` (identifiant de la table d'origine), `lang`, `text`, `source` ; remplie par `archcore i18n db <langue>` (source `import`, seules lignes qu'il remplace) et à la main (source `manual` par défaut, gardée par `i18n db`) |

Voir [[Traduction|FR-Traduction]] pour le repli et [[Extensions ArchRust|FR-Extensions-ArchRust]] pour le protocole.

## Contenu propre à ArchCore et journal des imports

`archcore import` régénère les tables du monde depuis les fichiers du client (et les tables du serveur d'origine) :
tout ce qu'on y écrirait à la main serait effacé au prochain import, et ces données ne sont pas publiables. Le
contenu propre à ArchCore (corrections, apparitions ajoutées, PNJ créés avec l'éditeur…) va donc dans le dossier
**`content/`** du dépôt, en fichiers `<AAAAMMJJHHMM>_<sujet>.sql` : `archcore import` les applique à la fin, et
`archcore content` les réapplique seuls. Chaque fichier passe dans une transaction (tout ou rien), doit pouvoir
être rejoué et ne contient que des données, ciblées par TID ou par `stable_key` (voir `content/README.md`).

Les tables **`shrine`** (Shrines : région centrale, jours, heure, durée, gravure, occupation, niveaux, effectif par
race) et **`shrine_reward`** (récompenses `Carving`, `WinRace`, `Common` : objet, quantité, charisme), créées par une
migration, ne viennent d'aucun fichier du client (les tables d'origine ne sont pas livrées) : elles sont remplies par
`content/202610281700_shrines.sql` (valeurs choisies par ArchCore, à régler à volonté).

La table **`import_run`** note chaque source lue (`client`, `relief`, `serveur`, `contenu`) : dossier, nombre de
fichiers, empreinte SHA-256 (noms et tailles ; textes pour le contenu), version d'ArchCore, début et fin. Une fin
vide signale un import interrompu, que `archcore db-check` relève.

## Versions du schéma (migrations)

Les fichiers `sql/001…004` forment la **base de référence** : `archcore db-init` les rejoue à chaque fois (ils
sont idempotents) et ne les modifie plus. Tout changement suivant va dans une **migration** :

```
sql/migrations/<base>/<AAAAMMJJHHMM>_<sujet>.sql      <base> = auth, characters ou world
```

- `archcore db-init` applique chaque migration une seule fois, dans l'ordre des identifiants, et la note dans la
  table `schema_migrations` de sa base (identifiant, date, empreinte SHA-256 du texte). La base de référence y
  figure sous `0000_reference`.
- Les scripts sont embarqués dans l'exécutable à la compilation : `archcore` n'a plus besoin du dossier `sql/`
  pour créer les bases.
- **Au démarrage, le serveur refuse de partir** si une base n'a jamais été suivie, est en retard (migrations à
  appliquer : lancer `archcore db-init`) ou en avance (migrations inconnues de l'exécutable : le mettre à
  jour). Une migration modifiée après son application, ou une base de référence changée, donne un
  avertissement.
- Une migration est additive par défaut ; un renommage ou une suppression se fait en deux versions (ajout et
  double lecture, puis retrait). Jamais de migration destructive sans sauvegarde préalable (`mariadb-dump`).

## `archcore_logs` — journaux (`sql/migrations/logs/`)

Traces de l'économie et de l'administration, en écriture seule : le serveur n'y lit jamais, et chaque écriture part
à part, hors du verrou du monde (un journal lent ou indisponible ne retarde pas le jeu). Pas de clé étrangère, des
index par personnage ou compte et par date.

| Table | Contenu |
|---|---|
| `log_trade` | Échange conclu : les deux personnages, l'or et les objets (`[{iid, tid, count}]`) donnés par chacun |
| `log_shop` | Achat, vente ou réparation (`repair`) chez un PNJ : personnage, PNJ, objet (TID, IID), quantité, prix |
| `log_bank` | Dépôt ou retrait à la banque du compte : objet (TID, IID, quantité) ou or |
| `log_auction` | Maison des ventes : mise en vente, achat, annulation ou encaissement, vente, auteur (et vendeur pour un achat), objet (TID, IID), quantité, prix en or et en CC, frais |
| `log_gm` | Commande MJ : auteur, niveau, commande, arguments, réussite, réponse |

**Mesure.** Une requête plus longue que `Database.SlowQueryMs` (200 ms par défaut) est notée « requête lente » dans le
journal du serveur, une fois le serveur prêt (les chargements du démarrage ne sont pas comptés) ; un paquet traité plus longtemps que `World.SlowPacketMs` (100 ms) sous le verrou du monde est
noté « paquet lent », avec l'attente du verrou. Premières mesures (instance de test, seuils à 1 ms) : les requêtes
les plus longues sont les chargements du démarrage (jusqu'à 0,3 s), et le verrou est tenu au plus 8 ms par paquet
(entrée en jeu), sans attente.

## Cohérence des bases (`archcore db-check`)

Les références d'une base à l'autre ne peuvent pas être des clés étrangères (elles lieraient les bases entre
elles et bloqueraient le réimport du monde). `archcore db-check` les vérifie, en lecture seule, et le serveur
fait de même au démarrage :

| Contrôle | Gravité |
|---|---|
| personnage sans compte (`characters.account_id` → `account`) | grave |
| personnage au modèle inconnu (`characters.template_id` → `character_template`) | grave |
| objet sans définition (`character_items.template_id` → `item_data`), par exemple après un réimport | grave |
| compétence inconnue (`character_skills.skill_tid` → `skill_template`) | grave |
| quête absente de `quest_template` | avertissement |
| recette absente de `product_compose` (`character_recipes.recipe_tid`) | avertissement |
| pièce jointe de courrier introuvable (`mail.item_id` sans objet au statut 4 chez le destinataire) | avertissement |
| objet en vente introuvable (`auction_sale` en vente sans objet au statut 5 chez le vendeur) | avertissement |
| coiffure ou visage absent de `character_customize` pour le modèle du personnage (table importée) | avertissement |
| victoires sur un boss absent de `boss_window` (`character_boss_kill.boss_tid`) | avertissement |
| châteaux absents de `siege_war` (`castle.id`) | avertissement |
| taxes d'un château absent de `siege_war` (`castle_tax.castle_id`), région taxée absente de `region_template` (`castle_tax_region.region_id`) | avertissement |
| banque sans compte, langue de compte inconnue | avertissement |
| identifiant de personnage dans la plage des PNJ (≥ 0x40000000), d'objet dans celle des boutiques (≥ 0x20000000) | grave |

`db-check` affiche aussi les versions du schéma et l'occupation des plages d'identifiants, et se termine en
erreur s'il trouve une anomalie grave ou une base pas à jour. Au démarrage, chaque anomalie est écrite dans le
journal avec quelques exemples, et le serveur démarre quand même.

Après une mise à jour d'ArchCore, lancer `archcore db-init` avant le serveur (sans `--account`, la commande ne
touche à aucun compte).

## Ajouter un compte

```bash
archcore db-init --account <nom> <mot_de_passe> [niveau_MJ]
```
