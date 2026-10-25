-- IA des monstres (aidatatable.txt des tables du serveur d'origine) et maîtrises des compétences
-- (SkillMastery_High.txt et SkillMastery_Heroic.txt du client), remplies par `archcore import`.
--
-- monster_ai : une ligne par IA, désignée par `monster_spawn.ai_tid`. Les colonnes non vides de la table
-- sont gardées en JSON ; celles que la table répète (SkillCondition, SkillCount) sont numérotées comme la
-- compétence qui les précède (Skill1 → SkillCondition1, SkillCount1…).
--
-- skill_mastery : arbre de compétences d'une classe (1 compétences supérieures, 2 compétences héroïques).
-- `requires` : compétences exigées, [[TID, points]…] (« %s Skill %d Points Requirement ») ; `total_points` :
-- points de compétence (arbre supérieur) ou points héroïques (arbre héroïque) déjà dépensés en tout.

CREATE TABLE IF NOT EXISTS monster_ai (
  tid   INT UNSIGNED NOT NULL,
  name  VARCHAR(128) NOT NULL,
  data  JSON NOT NULL COMMENT 'colonnes non vides d''aidatatable.txt',
  PRIMARY KEY (tid)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS skill_mastery (
  class_tid     INT UNSIGNED NOT NULL COMMENT 'modèle de la classe (SkillMasteryEvolution.txt)',
  skill_tid     INT UNSIGNED NOT NULL,
  tree          TINYINT      NOT NULL COMMENT '1 compétences supérieures, 2 héroïques',
  requires      JSON         NOT NULL COMMENT '[[TID, points]…] des colonnes ConditionA à ConditionC',
  total_points  INT          NOT NULL DEFAULT 0 COMMENT 'ConditionD : points dépensés en tout',
  PRIMARY KEY (class_tid, skill_tid),
  KEY skill_tid (skill_tid)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
