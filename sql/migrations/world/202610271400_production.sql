-- Production (récolte sur cadavre, cuisine, alchimie) : tables Product*.txt du client, remplies par
-- `archcore import`. Les noms de compétence de productcategory.txt et productexp.txt sont résolus en TID par
-- SKILLTEMPLATE.ini à l'import.
--
-- product_category : compétence de chaque catégorie (0 Hunt, 1 Cook, 2 Alchemy) et PNJ qui l'enseigne. Le prix et
-- le niveau de la ligne ne servent pas : le client affiche et vérifie ceux de SKILL_CONST.
--
-- product_compose : recettes de cuisine et d'alchimie, désignées par le TID de l'objet produit. `price` : prix
-- d'achat de la recette au PNJ (MakingPrice) ; `item_level` : niveau de compétence exigé (ItemLv) ; `title_tid` :
-- titre actif exigé (UsingMonster, 0 : aucun) ; `materials` : [[TID, quantité]…] des colonnes ItemA à ItemD.
--
-- product_exp : par compétence et niveau (1 à 15), expérience gagnée par action (min, max), facteur ajouté à la
-- chance de réussite, expérience du niveau et expérience cumulée à sa fin.
--
-- product_gather : monstres récoltables (Target TID), classes admises (bits 1 à 4), compétence, chance de réussite
-- et ensemble d'objets de product_result_item.
--
-- product_result_item : objets d'un ensemble par niveau de compétence, avec leur poids (100 par bloc).

CREATE TABLE IF NOT EXISTS product_category (
  category    TINYINT      NOT NULL COMMENT '0 Hunt, 1 Cook, 2 Alchemy',
  skill_tid   INT UNSIGNED NOT NULL,
  skill_name  VARCHAR(64)  NOT NULL,
  price       INT          NOT NULL DEFAULT 0,
  level       INT          NOT NULL DEFAULT 0,
  npc         VARCHAR(32)  NOT NULL DEFAULT '',
  PRIMARY KEY (category)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS product_compose (
  result_tid    INT UNSIGNED NOT NULL COMMENT 'objet produit = identifiant de la recette',
  category      TINYINT      NOT NULL,
  name          VARCHAR(64)  NOT NULL,
  title_tid     INT UNSIGNED NOT NULL DEFAULT 0,
  item_level    INT          NOT NULL DEFAULT 1,
  price         INT          NOT NULL DEFAULT 0,
  success_rate  INT          NOT NULL DEFAULT 0,
  recipe_tid    INT UNSIGNED NOT NULL DEFAULT 0 COMMENT 'objet « recette » facultatif',
  recipe_bonus  INT          NOT NULL DEFAULT 0,
  materials     JSON         NOT NULL COMMENT '[[TID, quantité]…]',
  PRIMARY KEY (result_tid),
  KEY category (category)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS product_exp (
  skill_tid   INT UNSIGNED NOT NULL,
  level       TINYINT      NOT NULL,
  exp_min     INT          NOT NULL DEFAULT 0,
  exp_max     INT          NOT NULL DEFAULT 0,
  rate_bonus  INT          NOT NULL DEFAULT 0 COMMENT 'ajouté à la chance de réussite',
  level_exp   INT          NOT NULL DEFAULT 0,
  total_exp   INT          NOT NULL DEFAULT 0 COMMENT 'expérience cumulée à la fin du niveau',
  PRIMARY KEY (skill_tid, level)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS product_gather (
  monster_tid  INT UNSIGNED NOT NULL,
  classes      TINYINT      NOT NULL COMMENT 'bit n : classe n (1 à 4)',
  skill_tid    INT UNSIGNED NOT NULL,
  rate         INT          NOT NULL DEFAULT 0,
  item_set     INT          NOT NULL,
  PRIMARY KEY (monster_tid)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS product_result_item (
  item_set     INT          NOT NULL,
  skill_level  TINYINT      NOT NULL,
  item_tid     INT UNSIGNED NOT NULL,
  weight       INT          NOT NULL,
  PRIMARY KEY (item_set, skill_level, item_tid)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
