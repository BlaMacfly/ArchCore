-- Guerres de siège des châteaux (AgpmSiegeWar 0x49), remplies par `archcore import` depuis les fichiers du client.
--
-- `siege_war` : une ligne par château de SiegeWar.txt (0 Rensphere, 1 Inflame, 2 Arthian, 3 Traqua, 4 Drayger) ; régions
-- du château et de ses abords (colonne Region, deux noms de RegionTemplate.ini séparés par « / »), durées en
-- millisecondes, délais de déclaration et de candidature en heures, écart entre deux sièges en jours, heure de début
-- (SWBeginTime, ramenée à 21 au-delà de 24), compétence posée au carving (Rensphere).
CREATE TABLE IF NOT EXISTS siege_war (
  id                 TINYINT UNSIGNED  NOT NULL COMMENT 'index de la ligne (0 : Rensphere)',
  name               VARCHAR(32)       NOT NULL COMMENT 'nom du château, clé des paquets',
  castle_region      SMALLINT UNSIGNED NOT NULL DEFAULT 0,
  outskirts_region   SMALLINT UNSIGNED NOT NULL DEFAULT 0,
  playing_ms         INT UNSIGNED      NOT NULL DEFAULT 0 COMMENT 'SWPlayingTime',
  eye_open_ms        INT UNSIGNED      NOT NULL DEFAULT 0 COMMENT 'ArchonEyeOpenTime, 0 : pas d''Yeux d''Archon',
  stamping_ms        INT UNSIGNED      NOT NULL DEFAULT 0 COMMENT 'StampingTime (durée du carving)',
  declare_hours      INT UNSIGNED      NOT NULL DEFAULT 0 COMMENT 'NextSWDeclaring(Hours)',
  select_hours       INT UNSIGNED      NOT NULL DEFAULT 0 COMMENT 'SelectGuildForSW(Hours)',
  interval_days      INT UNSIGNED      NOT NULL DEFAULT 7 COMMENT 'Duration4nextSW(Days)',
  begin_hour         TINYINT UNSIGNED  NOT NULL DEFAULT 20 COMMENT 'SWBeginTime',
  stamp_skill_tid    INT UNSIGNED      NOT NULL DEFAULT 0,
  stamp_skill_level  INT UNSIGNED      NOT NULL DEFAULT 0,
  disabled           TINYINT UNSIGNED  NOT NULL DEFAULT 0 COMMENT 'IsDisabled (colonne absente du fichier d''Archonia)',
  PRIMARY KEY (id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- `siege_war_object` : objets d'assaut réparables de SiegeWarObject.txt (type 10 catapulte, 9 camp de résurrection des
-- assiégeants) ; `items` : objets à charger, liste JSON de paires [TID, nombre].
CREATE TABLE IF NOT EXISTS siege_war_object (
  type            TINYINT UNSIGNED NOT NULL COMMENT 'SiegeWarCharType',
  description     VARCHAR(64)      NOT NULL DEFAULT '',
  repair_count    INT UNSIGNED     NOT NULL DEFAULT 0,
  repair_cost     BIGINT           NOT NULL DEFAULT 0,
  repair_minutes  INT UNSIGNED     NOT NULL DEFAULT 0,
  items           TEXT             NOT NULL,
  PRIMARY KEY (type)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- `siege_binding` : points de résurrection des sièges (événements « binding » des objets obj*.ini du client) :
-- type 3 assaillants (abords), 4 défense intérieure, 7 défense extérieure, 5 Archlord, 6 assaillants de l'Archlord.
CREATE TABLE IF NOT EXISTS siege_binding (
  id            INT UNSIGNED      NOT NULL,
  stable_key    VARCHAR(64)       NULL,
  region_id     SMALLINT UNSIGNED NOT NULL,
  name          VARCHAR(96)       NOT NULL DEFAULT '',
  binding_type  TINYINT UNSIGNED  NOT NULL,
  pos_x         DOUBLE            NOT NULL,
  pos_y         DOUBLE            NOT NULL,
  pos_z         DOUBLE            NOT NULL,
  PRIMARY KEY (id),
  UNIQUE KEY ux_siege_binding_key (stable_key),
  KEY ix_siege_binding_region (region_id, binding_type)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
