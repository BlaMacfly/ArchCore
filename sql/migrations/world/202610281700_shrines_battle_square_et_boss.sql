-- Niveaux d'entrée des régions (RegionTemplate.ini du client : LevelMin, LevelLimit), remplis par `archcore import`.
-- Seuls les étages de la Battle Square (régions racines 198, 208, 209, 215, 223, 229) en ont : 20-30 au 1er étage,
-- jusqu'à 70-80 au 6e. 0 : pas de limite.
ALTER TABLE region_template ADD COLUMN IF NOT EXISTS level_min INT NOT NULL DEFAULT 0 COMMENT 'niveau minimal d''entrée (LevelMin), 0 : aucun' AFTER safety;
ALTER TABLE region_template ADD COLUMN IF NOT EXISTS level_limit INT NOT NULL DEFAULT 0 COMMENT 'niveau maximal conseillé (LevelLimit), 0 : aucun' AFTER level_min;

-- Crânes lâchés par un joueur tué dans la Battle Square (pvpskulldroptable.txt du client), remplis par `archcore import`.
-- `pvp_skull_chance` : chance (pour cent) selon l'écart de niveau tueur − victime ; `pvp_skull_level` : niveau du crâne
-- selon le niveau de la victime (1 à 3, objet choisi par `BattleSquare.SkullTids`).
CREATE TABLE IF NOT EXISTS pvp_skull_chance (
  level_gap   INT     NOT NULL COMMENT 'niveau du tueur − niveau de la victime',
  chance      DOUBLE  NOT NULL DEFAULT 0 COMMENT 'pour cent',
  PRIMARY KEY (level_gap)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS pvp_skull_level (
  victim_level_min  INT     NOT NULL,
  victim_level_max  INT     NOT NULL,
  skull_level       TINYINT NOT NULL,
  PRIMARY KEY (victim_level_min)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- Boss de la fenêtre des boss d'Archonia (UIBoss.xml du client), remplis par `archcore import` : le serveur envoie leur
-- état (vivant, prochaine apparition) par AgpmBoss op 1. `category` : 1 ChaoticFrontier, 2 Asylum, 3 Battlesquare,
-- 4 Shinewood, 5 HeavensHell, 6 Firmament, 7 Purgatory (0 inconnue).
CREATE TABLE IF NOT EXISTS boss_window (
  tid        INT UNSIGNED      NOT NULL,
  row_order  SMALLINT UNSIGNED NOT NULL COMMENT 'ordre du fichier (32 boss au plus pour le client)',
  category   TINYINT UNSIGNED  NOT NULL DEFAULT 0,
  name       VARCHAR(96)       NOT NULL DEFAULT '',
  PRIMARY KEY (tid)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- Combats de Shrine de l'épisode 8 (AgpmShrineBattle 0x62). Les tables du serveur d'origine (ShrineTable.txt,
-- ShrineRegion.txt, ShrineReward.xml) ne sont pas livrées : ces tables propres à ArchCore sont remplies par la couche
-- de contenu (`content/202610281700_shrines.sql`), à régler à volonté.
-- Un Shrine ouvre chaque jour de `days` (bit 0 lundi … bit 6 dimanche) à open_hour:open_minute (heure de
-- `World.TimeZone`) pour `duration_min` minutes ; un joueur de la région centrale (`region_id`, 236 à 243) grave le
-- sceau en `carve_sec` secondes, puis sa race doit le garder `occupy_sec` secondes. Entrée : niveau `level_min` à
-- `level_max` (0 : sans limite), `max_per_race` joueurs par race au plus.
CREATE TABLE IF NOT EXISTS shrine (
  id            TINYINT UNSIGNED  NOT NULL COMMENT 'Id du Shrine (colonne 0 de ShrineTable), 1 à 10',
  name          VARCHAR(29)       NOT NULL,
  region_id     SMALLINT UNSIGNED NOT NULL COMMENT 'région centrale (236 à 243)',
  days          TINYINT UNSIGNED  NOT NULL DEFAULT 127,
  open_hour     TINYINT UNSIGNED  NOT NULL DEFAULT 20,
  open_minute   TINYINT UNSIGNED  NOT NULL DEFAULT 0,
  duration_min  SMALLINT UNSIGNED NOT NULL DEFAULT 30,
  carve_sec     SMALLINT UNSIGNED NOT NULL DEFAULT 60,
  occupy_sec    SMALLINT UNSIGNED NOT NULL DEFAULT 300,
  level_min     INT               NOT NULL DEFAULT 50,
  level_max     INT               NOT NULL DEFAULT 0,
  max_per_race  SMALLINT UNSIGNED NOT NULL DEFAULT 50,
  PRIMARY KEY (id),
  UNIQUE KEY ux_shrine_region (region_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- Récompenses d'un combat gagné (catégories de ShrineReward.xml) : `Carving` au graveur, `WinRace` à chaque joueur de la
-- race victorieuse présent dans le Shrine, `Common` à tout joueur présent. Objet `item_tid` × `item_count` (0 : aucun)
-- et `charisma` points de charisme.
CREATE TABLE IF NOT EXISTS shrine_reward (
  category    VARCHAR(16)   NOT NULL COMMENT 'Carving, WinRace ou Common',
  item_tid    INT UNSIGNED  NOT NULL DEFAULT 0,
  item_count  INT           NOT NULL DEFAULT 0,
  charisma    INT           NOT NULL DEFAULT 0,
  PRIMARY KEY (category, item_tid)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
