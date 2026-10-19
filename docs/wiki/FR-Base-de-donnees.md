# Base de données

ArchCore utilise trois bases MariaDB, créées par `archcore db-init` à partir de `sql/`.
Toutes sont en `utf8mb4`.

## `archcore_auth` — comptes et mondes (`sql/001_auth.sql`)

| Table | Contenu |
|---|---|
| `account` | Comptes : nom, empreinte SHA-256 du mot de passe, niveau MJ, bannissement, dernière connexion, clé d'entrée dans le monde |
| `server_group` | Groupes de serveurs affichés dans la liste |
| `world` | Mondes : nom, groupe, adresse `ip:port` (23 caractères max), statut |

## `archcore_characters` — personnages (`sql/002_characters.sql`)

| Table | Contenu |
|---|---|
| `characters` | Compte, emplacement, nom unique, modèle (TID), niveau, expérience, PV/PM/PE, position, argent, banque, coiffure, visage, suppression logique |
| `character_items` | Objets possédés (prévu) |
| `character_skills` | Compétences apprises (prévu) |

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
| `skill_template` | `SKILLTEMPLATE.ini` | Compétences |
| `npc_spawn` | `npc.ini` | 567 PNJ : modèle, nom, position, orientation, fonctions brutes |
| `client_table` | autres tables `.txt` | Lignes en JSON, pour les tables pas encore modélisées |
| `monster_spawn` | tables du serveur d'origine (`spawndatatable.txt` + `spawninstancetable.txt`) | 12 802 points d'apparition, 36 960 créatures : modèle, niveau, nombre, réapparition, position, rayon, nom public (`CharacterTemplatePublic.ini`) |
| `map_segment` | `world/node_data` du serveur d'origine | 45 312 secteurs : sol infranchissable et région par segment de 400 unités |
| `monster_drop` | `itemdroptable.txt` du serveur d'origine | 4 653 objets propres à 550 monstres : quantité, taux sur 100 000 |
| `drop_group` | `groupdroprate.txt` du serveur d'origine | 28 groupes de butin ; les objets de chaque groupe viennent d'`item_data` (GroupID, DropRank, DropRate) |
| `drop_rank_rate` | `droprankrate.txt` du serveur d'origine | Rangs d'objets (pour cent) selon le rang de butin du monstre |
| `region_template` | `RegionTemplate.ini` | 331 régions : nom, région parente, champ Type, sûreté (33 sûres) |
| `resurrection_point` | `obj*.ini` (événement 19, type 1) | 62 points de résurrection : région, position, race et classe admises |

## Ajouter un compte

```bash
archcore db-init --account <nom> <mot_de_passe> [niveau_MJ]
```
