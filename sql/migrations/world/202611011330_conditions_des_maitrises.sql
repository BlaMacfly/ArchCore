-- Conditions des maîtrises (SkillMastery_High.txt), remplies par `archcore import`, comme les lit Archonia
-- (chargeur FUN_140229e10, contrôle FUN_1402b8f40) :
--   requires      ConditionA et B : [[TID de la compétence, niveau]…].
--   class_points  ConditionC : [[TID du modèle d'une classe, niveaux]…] (« 1722;30 » : 30 niveaux cumulés dans les
--                 compétences du Slayer), et non plus une compétence exigée.
--   total_points  ConditionD : niveaux cumulés de toutes les compétences (arbre supérieur) ou points héroïques
--                 dépensés (arbre héroïque, dont seules ConditionA et D sont lues).
-- La ligne « 10004;4 » ouvre l'arbre de la race 4 (passives du Dragon Scion) : class_tid 10004.
ALTER TABLE skill_mastery
    ADD COLUMN IF NOT EXISTS class_points JSON NOT NULL DEFAULT '[]' COMMENT '[[TID du modèle, niveaux]…] de ConditionC' AFTER requires;
