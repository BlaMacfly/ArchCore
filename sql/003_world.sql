-- ArchCore — base monde (archcore_world) : données importées du client ArchLord 64 bits.
-- Remplie par tools/import-client-data.mjs à partir de data/client-ini (tables déchiffrées).

CREATE TABLE IF NOT EXISTS character_template (
  tid            INT UNSIGNED NOT NULL COMMENT 'section [N] de charactertemplate.ini',
  name           VARCHAR(96)  NOT NULL,
  character_type INT NOT NULL DEFAULT 0,
  race           INT NOT NULL DEFAULT 0,
  gender         INT NOT NULL DEFAULT 0,
  class          INT NOT NULL DEFAULT 0,
  level          INT NOT NULL DEFAULT 1,
  con            FLOAT NOT NULL DEFAULT 0,
  str            FLOAT NOT NULL DEFAULT 0,
  intel          FLOAT NOT NULL DEFAULT 0,
  dex            FLOAT NOT NULL DEFAULT 0,
  cha            FLOAT NOT NULL DEFAULT 0,
  luk            FLOAT NOT NULL DEFAULT 0,
  wis            FLOAT NOT NULL DEFAULT 0,
  movement       INT NOT NULL DEFAULT 0,
  movement_fast  INT NOT NULL DEFAULT 0,
  hp             INT NOT NULL DEFAULT 0,
  mp             INT NOT NULL DEFAULT 0,
  sp             INT NOT NULL DEFAULT 0,
  attack_point   INT NOT NULL DEFAULT 0,
  magic_attack   INT NOT NULL DEFAULT 0,
  ar             INT NOT NULL DEFAULT 0,
  dr             INT NOT NULL DEFAULT 0,
  base_exp       INT NOT NULL DEFAULT 0,
  min_damage     INT NOT NULL DEFAULT 0,
  max_damage     INT NOT NULL DEFAULT 0,
  attack_range   INT NOT NULL DEFAULT 0,
  attack_speed   INT NOT NULL DEFAULT 0,
  sight          INT NOT NULL DEFAULT 0,
  raw            JSON NULL COMMENT 'toutes les clés de la section, pour les champs non modélisés',
  PRIMARY KEY (tid),
  KEY ix_ct_race_class (race, gender, class)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS character_data_table (
  tid    INT UNSIGNED NOT NULL COMMENT 'CharacterDataTable.txt (stats serveur par TID)',
  name   VARCHAR(96) NOT NULL,
  data   JSON NOT NULL COMMENT 'colonne → valeur, en-têtes du fichier',
  PRIMARY KEY (tid)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS level_exp (
  level INT UNSIGNED NOT NULL,
  exp   BIGINT UNSIGNED NOT NULL,
  PRIMARY KEY (level)
) ENGINE=InnoDB;

CREATE TABLE IF NOT EXISTS growup_factor (
  template_id INT UNSIGNED NOT NULL COMMENT 'bloc de GrowUpFactor.txt (ligne d''en-tête = TID)',
  level       INT UNSIGNED NOT NULL,
  str  INT NOT NULL DEFAULT 0,
  dex  INT NOT NULL DEFAULT 0,
  con  INT NOT NULL DEFAULT 0,
  wis  INT NOT NULL DEFAULT 0,
  intel INT NOT NULL DEFAULT 0,
  cha  INT NOT NULL DEFAULT 0,
  hp   INT NOT NULL DEFAULT 0,
  mp   INT NOT NULL DEFAULT 0,
  sp   INT NOT NULL DEFAULT 0,
  ac   INT NOT NULL DEFAULT 0,
  ar   INT NOT NULL DEFAULT 0,
  dr   INT NOT NULL DEFAULT 0,
  res_fire INT NOT NULL DEFAULT 0,
  res_water INT NOT NULL DEFAULT 0,
  res_earth INT NOT NULL DEFAULT 0,
  res_air INT NOT NULL DEFAULT 0,
  res_magic INT NOT NULL DEFAULT 0,
  min_damage INT NOT NULL DEFAULT 0,
  max_damage INT NOT NULL DEFAULT 0,
  PRIMARY KEY (template_id, level)
) ENGINE=InnoDB;

CREATE TABLE IF NOT EXISTS login_char_pos (
  idx      INT UNSIGNED NOT NULL COMMENT 'section [N] de logincharpos.ini',
  race     VARCHAR(32) NOT NULL,
  pos_x    DOUBLE NOT NULL,
  pos_y    DOUBLE NOT NULL,
  pos_z    DOUBLE NOT NULL,
  degree_x DOUBLE NOT NULL DEFAULT 0,
  degree_y DOUBLE NOT NULL DEFAULT 0,
  PRIMARY KEY (idx)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS item_template (
  tid   INT UNSIGNED NOT NULL,
  name  VARCHAR(128) NOT NULL,
  file  VARCHAR(255) NULL COMMENT 'fichier de template (itemtemplateentry.ini)',
  raw   JSON NULL,
  PRIMARY KEY (tid)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS skill_template (
  tid   INT UNSIGNED NOT NULL,
  name  VARCHAR(128) NOT NULL,
  raw   JSON NULL,
  PRIMARY KEY (tid)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS client_table (
  name      VARCHAR(96) NOT NULL COMMENT 'fichier source dans data/client-ini',
  row_idx   INT UNSIGNED NOT NULL,
  data      JSON NOT NULL,
  PRIMARY KEY (name, row_idx)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='tables tabulées génériques (.txt) non modélisées';

CREATE TABLE IF NOT EXISTS npc_spawn (
  id        INT UNSIGNED NOT NULL COMMENT 'section [N] de npc.ini',
  tid       INT UNSIGNED NOT NULL COMMENT 'template de personnage (charactertemplate.ini)',
  name      VARCHAR(96)  NOT NULL,
  pos_x     DOUBLE NOT NULL,
  pos_y     DOUBLE NOT NULL,
  pos_z     DOUBLE NOT NULL,
  degree_x  DOUBLE NOT NULL DEFAULT 0,
  degree_y  DOUBLE NOT NULL DEFAULT 0,
  raw       JSON NULL COMMENT 'fonctions (dialogue, boutique, quêtes…) telles que dans npc.ini',
  PRIMARY KEY (id),
  KEY ix_npc_xz (pos_x, pos_z)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
