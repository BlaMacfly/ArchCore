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

CREATE TABLE IF NOT EXISTS skill_spec (
  tid   INT UNSIGNED NOT NULL,
  name  VARCHAR(128) NOT NULL,
  spec  JSON NOT NULL COMMENT 'colonnes non vides de SKILL_SPEC.txt',
  PRIMARY KEY (tid)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS skill_level (
  tid     INT UNSIGNED NOT NULL,
  level   TINYINT UNSIGNED NOT NULL,
  consts  JSON NOT NULL COMMENT 'colonnes non vides de SKILL_CONST.txt pour ce niveau',
  PRIMARY KEY (tid, level)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS quest_template (
  tid   INT UNSIGNED NOT NULL,
  name  VARCHAR(128) NOT NULL,
  data  JSON NOT NULL COMMENT 'colonnes non vides de questtemplate.ini',
  PRIMARY KEY (tid)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS quest_group (
  id       INT UNSIGNED NOT NULL,
  name     VARCHAR(128) NOT NULL,
  give     JSON NOT NULL COMMENT 'quêtes proposées par les PNJ de ce groupe',
  confirm  JSON NOT NULL COMMENT 'quêtes terminées auprès de ces PNJ',
  PRIMARY KEY (id)
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

CREATE TABLE IF NOT EXISTS item_data (
  tid        INT UNSIGNED NOT NULL COMMENT 'TID (itemdatatable.txt)',
  name       VARCHAR(128) NOT NULL,
  level      INT NOT NULL DEFAULT 0,
  part       INT NOT NULL DEFAULT 0,
  kind       INT NOT NULL DEFAULT 0,
  item_type  INT NOT NULL DEFAULT 0,
  type       INT NOT NULL DEFAULT 0,
  npc_price  BIGINT NOT NULL DEFAULT 0 COMMENT 'prix d''achat chez un PNJ',
  pc_price   BIGINT NOT NULL DEFAULT 0 COMMENT 'prix de revente à un PNJ',
  stack      INT NOT NULL DEFAULT 1 COMMENT 'quantité maximale par pile',
  raw        JSON NULL,
  PRIMARY KEY (tid)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='caractéristiques des objets (itemdatatable.txt)';

CREATE TABLE IF NOT EXISTS npc_trade_list (
  template_id INT UNSIGNED NOT NULL COMMENT 'modèle de boutique (« Template » de la fonction 9 dans npc.ini)',
  idx         INT UNSIGNED NOT NULL COMMENT 'ordre dans la liste',
  tid         INT UNSIGNED NOT NULL,
  count       INT UNSIGNED NOT NULL DEFAULT 1 COMMENT 'quantité vendue par achat',
  PRIMARY KEY (template_id, idx)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='listes des boutiques des PNJ (npctradeitemlist.txt)';

CREATE TABLE IF NOT EXISTS monster_spawn (
  id          INT UNSIGNED NOT NULL COMMENT 'ordre d''import (une ligne par position du point)',
  group_name  VARCHAR(32)  NOT NULL COMMENT 'groupe d''apparition (GroupName)',
  name        VARCHAR(32)  NOT NULL COMMENT 'point d''apparition (spawninstancetable.txt)',
  tid         INT UNSIGNED NOT NULL COMMENT 'modèle de personnage du monstre',
  display_name VARCHAR(96) NOT NULL DEFAULT '' COMMENT 'nom public (CharacterTemplatePublic.ini)',
  level       INT NOT NULL DEFAULT 1,
  count       INT NOT NULL DEFAULT 0 COMMENT 'monstres sur ce point (TotalMobCount)',
  max_count   INT NOT NULL DEFAULT 0,
  respawn_sec INT NOT NULL DEFAULT 0 COMMENT 'délai de réapparition (Spawntime)',
  rate        INT NOT NULL DEFAULT 10000 COMMENT 'probabilité sur 10 000',
  ai_tid      INT NOT NULL DEFAULT 0,
  siege_war   INT NOT NULL DEFAULT 0 COMMENT '0 : hors guerre de siège',
  is_boss     TINYINT NOT NULL DEFAULT 0,
  pos_x       DOUBLE NOT NULL,
  pos_y       DOUBLE NOT NULL,
  pos_z       DOUBLE NOT NULL,
  degree_x    DOUBLE NOT NULL DEFAULT 0,
  degree_y    DOUBLE NOT NULL DEFAULT 0,
  radius      DOUBLE NOT NULL DEFAULT 0 COMMENT 'rayon de dispersion autour du point',
  PRIMARY KEY (id),
  KEY ix_ms_xz (pos_x, pos_z)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='apparitions des monstres (tables du serveur d''origine)';

CREATE TABLE IF NOT EXISTS map_segment (
  sector_x  SMALLINT UNSIGNED NOT NULL COMMENT 'secteur de 6400 unités, 0..799 depuis X = -2 560 000',
  sector_z  SMALLINT UNSIGNED NOT NULL COMMENT 'secteur de 6400 unités, 0..799 depuis Z = -2 560 000',
  segments  BLOB NOT NULL COMMENT '16 × 16 segments de 400 unités, u32 LE, ligne Z puis colonne X : octet 0 type de case, octet 1 blocage (bit 0 sol, bit 1 ciel), octets 2-3 région',
  PRIMARY KEY (sector_x, sector_z)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='carte serveur du monde (node_data des tables du serveur d''origine)';

CREATE TABLE IF NOT EXISTS monster_drop (
  id          INT UNSIGNED NOT NULL COMMENT 'ordre dans itemdroptable.txt',
  monster_tid INT UNSIGNED NOT NULL,
  item_tid    INT UNSIGNED NOT NULL,
  min_count   INT NOT NULL DEFAULT 1,
  max_count   INT NOT NULL DEFAULT 1,
  rate        INT NOT NULL COMMENT 'sur 100 000 ; 100 000 = toujours',
  PRIMARY KEY (id),
  KEY ix_md_monster (monster_tid)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='butin propre à un monstre (itemdroptable.txt du serveur d''origine)';

CREATE TABLE IF NOT EXISTS drop_group (
  group_id    INT UNSIGNED NOT NULL,
  name        VARCHAR(64) NOT NULL DEFAULT '',
  meditation  TINYINT NOT NULL DEFAULT 0 COMMENT 'taux multiplié par DropMeditation du monstre',
  rate        INT NOT NULL COMMENT 'sur 100 000',
  level_bonus JSON NOT NULL COMMENT 'bonus de taux par tranche de 10 niveaux du tueur (LevelBonus10…100)',
  PRIMARY KEY (group_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='groupes de butin (groupdroprate.txt du serveur d''origine)';

CREATE TABLE IF NOT EXISTS drop_rank_rate (
  monster_rank INT UNSIGNED NOT NULL COMMENT 'DropRank du monstre (CharacterDataTable.txt)',
  rates        JSON NOT NULL COMMENT 'pour cent par rang d''objet 1 à 10',
  PRIMARY KEY (monster_rank)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='rang des objets selon le rang du monstre (droprankrate.txt du serveur d''origine)';

CREATE TABLE IF NOT EXISTS region_template (
  id        INT UNSIGNED NOT NULL COMMENT 'section [N] de RegionTemplate.ini (numéro de région de la carte)',
  name      VARCHAR(96) NOT NULL DEFAULT '',
  parent_id INT NOT NULL DEFAULT -1,
  type_bits INT UNSIGNED NOT NULL DEFAULT 0 COMMENT 'champ Type : bits 0-1 terrain, 2-3 sûreté…',
  safety    TINYINT NOT NULL DEFAULT 0 COMMENT '0 sûre, 1 libre, 2 dangereuse',
  PRIMARY KEY (id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='régions du monde (RegionTemplate.ini)';

CREATE TABLE IF NOT EXISTS resurrection_point (
  id        INT UNSIGNED NOT NULL COMMENT 'ordre d''import',
  region_id INT UNSIGNED NOT NULL,
  name      VARCHAR(96) NOT NULL DEFAULT '',
  pos_x     DOUBLE NOT NULL,
  pos_y     DOUBLE NOT NULL,
  pos_z     DOUBLE NOT NULL,
  race      INT NOT NULL DEFAULT 0 COMMENT '0 : toutes',
  class     INT NOT NULL DEFAULT 0 COMMENT '0 : toutes',
  PRIMARY KEY (id),
  KEY ix_rp_region (region_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='points de résurrection (objets à événement 19, BindingType 1, des obj*.ini du client)';
