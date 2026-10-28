-- Scores des combats de Shrine (AgpmShrineBattle op 11 et 12) : à la fin d'un combat gagné, chaque joueur présent
-- compte une victoire si sa race a pris le Shrine, une défaite sinon ; chaque race présente fait de même.
CREATE TABLE IF NOT EXISTS shrine_score (
  character_id  INT UNSIGNED NOT NULL,
  wins          INT UNSIGNED NOT NULL DEFAULT 0,
  losses        INT UNSIGNED NOT NULL DEFAULT 0,
  PRIMARY KEY (character_id),
  CONSTRAINT fk_shrine_score_character FOREIGN KEY (character_id) REFERENCES characters (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS shrine_race_score (
  race    TINYINT UNSIGNED NOT NULL COMMENT '1 humain, 2 orc, 3 elfe de lune, 4 Dragon Scion',
  wins    INT UNSIGNED     NOT NULL DEFAULT 0,
  losses  INT UNSIGNED     NOT NULL DEFAULT 0,
  PRIMARY KEY (race)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- Fenêtre des boss d'Archonia (AgpmBoss op 5) : victoires de chaque personnage sur chaque boss de UIBoss.xml. Une
-- victoire compte pour chaque joueur qui a blessé le boss au cours du combat où il meurt.
CREATE TABLE IF NOT EXISTS character_boss_kill (
  character_id  INT UNSIGNED NOT NULL,
  boss_tid      INT UNSIGNED NOT NULL,
  kills         INT UNSIGNED NOT NULL DEFAULT 0,
  last_kill_at  DATETIME     NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (character_id, boss_tid),
  KEY ix_boss_kill_boss (boss_tid, kills),
  CONSTRAINT fk_boss_kill_character FOREIGN KEY (character_id) REFERENCES characters (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
