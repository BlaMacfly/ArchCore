-- ArchCore — base des personnages (archcore_characters)

CREATE TABLE IF NOT EXISTS characters (
  id            INT UNSIGNED NOT NULL AUTO_INCREMENT,
  account_id    INT UNSIGNED NOT NULL,
  slot          TINYINT UNSIGNED NOT NULL DEFAULT 0,
  name          VARCHAR(48)  NOT NULL,
  template_id   INT UNSIGNED NOT NULL COMMENT 'TID dans charactertemplate.ini',
  level         INT UNSIGNED NOT NULL DEFAULT 1,
  exp           BIGINT UNSIGNED NOT NULL DEFAULT 0,
  hp            INT NOT NULL DEFAULT 0,
  mp            INT NOT NULL DEFAULT 0,
  sp            INT NOT NULL DEFAULT 0,
  skill_points  INT NOT NULL DEFAULT 0,
  pos_x         DOUBLE NOT NULL DEFAULT 0,
  pos_y         DOUBLE NOT NULL DEFAULT 0,
  pos_z         DOUBLE NOT NULL DEFAULT 0,
  money         BIGINT UNSIGNED NOT NULL DEFAULT 0,
  bank_money    BIGINT UNSIGNED NOT NULL DEFAULT 0,
  cash          BIGINT UNSIGNED NOT NULL DEFAULT 0,
  hair          TINYINT UNSIGNED NOT NULL DEFAULT 0,
  face          TINYINT UNSIGNED NOT NULL DEFAULT 0,
  bank_size     TINYINT UNSIGNED NOT NULL DEFAULT 0,
  option_flag   INT UNSIGNED NOT NULL DEFAULT 0,
  created_at    DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  last_played   DATETIME NULL,
  deleted_at    DATETIME NULL,
  PRIMARY KEY (id),
  UNIQUE KEY uq_character_name (name),
  KEY ix_character_account (account_id, slot)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS character_items (
  id            BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
  character_id  INT UNSIGNED NOT NULL,
  template_id   INT UNSIGNED NOT NULL,
  status        TINYINT UNSIGNED NOT NULL DEFAULT 0 COMMENT '0 inventaire, 1 équipé, 2 banque',
  layer         TINYINT UNSIGNED NOT NULL DEFAULT 0,
  row_idx       TINYINT UNSIGNED NOT NULL DEFAULT 0,
  col_idx       TINYINT UNSIGNED NOT NULL DEFAULT 0,
  count         INT UNSIGNED NOT NULL DEFAULT 1,
  durability    INT NOT NULL DEFAULT 0,
  PRIMARY KEY (id),
  KEY ix_item_character (character_id),
  CONSTRAINT fk_item_character FOREIGN KEY (character_id) REFERENCES characters (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS character_skills (
  character_id  INT UNSIGNED NOT NULL,
  skill_tid     INT UNSIGNED NOT NULL,
  level         TINYINT UNSIGNED NOT NULL DEFAULT 1,
  PRIMARY KEY (character_id, skill_tid),
  CONSTRAINT fk_skill_character FOREIGN KEY (character_id) REFERENCES characters (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
