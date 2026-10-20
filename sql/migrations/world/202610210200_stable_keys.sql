-- Identifiants stables des tables importées dont l'identifiant n'est qu'un numéro d'ordre d'import :
-- une clé tirée des sources (colonne stable_key, unique), la même d'un réimport à l'autre tant que les
-- sources ne changent pas. Elle permet à un outil (éditeur, couche de contenu) de viser une ligne précise.
--   monster_spawn      <point>#<rang>             rang de la ligne parmi celles du même point (ordre des tables)
--   monster_drop       <monstre>:<objet>#<rang>   rang parmi les lignes du même couple
--   resurrection_point <fichier>#<objet>#<rang>   fichier obj*.ini, section de l'objet, rang de l'événement
-- Les deux premières se déduisent des lignes déjà importées ; resurrection_point a besoin du fichier et de
-- l'objet d'origine : sa clé est remplie au prochain `archcore import`.
-- Les clés tiennent compte de la casse (utf8mb4_bin) : des points ne diffèrent que par elle (Thr_b_01, thr_b_01).

ALTER TABLE monster_spawn ADD COLUMN IF NOT EXISTS stable_key VARCHAR(48) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NULL COMMENT 'point#rang, stable d''un import à l''autre' AFTER id;
UPDATE monster_spawn m
JOIN (SELECT id, CONCAT(name, '#', ROW_NUMBER() OVER (PARTITION BY name COLLATE utf8mb4_bin ORDER BY id)) AS k FROM monster_spawn) x ON x.id = m.id
SET m.stable_key = x.k
WHERE m.stable_key IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS uq_monster_spawn_key ON monster_spawn (stable_key);

ALTER TABLE monster_drop ADD COLUMN IF NOT EXISTS stable_key VARCHAR(40) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NULL COMMENT 'monstre:objet#rang, stable d''un import à l''autre' AFTER id;
UPDATE monster_drop m
JOIN (SELECT id, CONCAT(monster_tid, ':', item_tid, '#', ROW_NUMBER() OVER (PARTITION BY monster_tid, item_tid ORDER BY id)) AS k FROM monster_drop) x ON x.id = m.id
SET m.stable_key = x.k
WHERE m.stable_key IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS uq_monster_drop_key ON monster_drop (stable_key);

ALTER TABLE resurrection_point ADD COLUMN IF NOT EXISTS stable_key VARCHAR(64) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NULL COMMENT 'fichier#objet#rang, rempli par archcore import' AFTER id;
CREATE UNIQUE INDEX IF NOT EXISTS uq_resurrection_point_key ON resurrection_point (stable_key);
