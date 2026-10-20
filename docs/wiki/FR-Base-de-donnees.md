# Base de données

ArchCore utilise trois bases MariaDB, créées et mises à jour par `archcore db-init` à partir de `sql/` (base de
référence et migrations, voir plus bas).
Toutes sont en `utf8mb4`.

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
| `characters` | Compte, emplacement, nom unique, modèle (TID), niveau, expérience, PV/PM/PE, position, argent, banque, coiffure, visage, suppression logique |
| `character_items` | Objets : statut 0 inventaire, 1 équipé, 2 banque (rattaché au personnage qui l'a déposé) |
| `character_quests` | Quêtes en cours (compteurs des monstres A et B) et terminées |
| `account_bank` | Banque du compte : argent, pages en plus de la première (créée à la première entrée, avec l'argent que les personnages avaient en banque) |
| `character_skills` | Compétences apprises et leur niveau (« Normal Attack » n'y figure pas : tout personnage la connaît) |

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
| `skill_level` | `SKILL_CONST.txt` | Valeurs de chaque niveau de chaque compétence (colonnes non vides, en JSON ; groupes `point.`, `percent.`, `magnify.`) |
| `npc_spawn` | `npc.ini` | 567 PNJ : modèle, nom, position, orientation, fonctions brutes |
| `client_table` | autres tables `.txt` | Lignes en JSON, pour les tables pas encore modélisées |
| `monster_spawn` | tables du serveur d'origine (`spawndatatable.txt` + `spawninstancetable.txt`) | 12 802 points d'apparition, 36 960 créatures : modèle, niveau, nombre, réapparition, position, rayon, nom public (`CharacterTemplatePublic.ini`) |
| `map_segment` | `world/node_data` du serveur d'origine | 45 312 secteurs : sol infranchissable et région par segment de 400 unités |
| `map_height` | `world/b*x.ma2` du client (géométrie du sol) | 46 592 secteurs : hauteur du sol aux 17 × 17 coins de cellule (i16) |
| `monster_drop` | `itemdroptable.txt` du serveur d'origine | 4 653 objets propres à 550 monstres : quantité, taux sur 100 000 |
| `drop_group` | `groupdroprate.txt` du serveur d'origine | 28 groupes de butin ; les objets de chaque groupe viennent d'`item_data` (GroupID, DropRank, DropRate) |
| `drop_rank_rate` | `droprankrate.txt` du serveur d'origine | Rangs d'objets (pour cent) selon le rang de butin du monstre |
| `region_template` | `RegionTemplate.ini` | 331 régions : nom, région parente, champ Type, sûreté (33 sûres) |
| `resurrection_point` | `obj*.ini` (événement 19, type 1) | 62 points de résurrection : région, position, race et classe admises |

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
| `localized_text` | Textes localisés : `kind` (`npc_name`, `monster_name`, `item_name`, `item_desc`, `quest_name`, `quest_desc`, `skill_name`, `region_name`, `npc_dialog`), `ref_id` (identifiant de la table d'origine), `lang`, `text` ; remplie par `archcore i18n db <langue>` et à la main |

Voir [[Traduction|FR-Traduction]] pour le repli et [[Extensions ArchRust|FR-Extensions-ArchRust]] pour le protocole.

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
