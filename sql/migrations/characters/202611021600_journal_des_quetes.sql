-- Suivi des quêtes à l'écran (AgpmQuest op 14, `world/quests.rs`) : drapeau « inscrit au journal » d'une quête en
-- cours, envoyé par le client à la prise d'une quête et par la case « suivre » de la fenêtre des quêtes (10 au plus),
-- renvoyé au 5ᵉ entier du QUEST_INFO de l'op 4 à l'entrée en jeu.
ALTER TABLE character_quests
    ADD COLUMN IF NOT EXISTS journal TINYINT UNSIGNED NOT NULL DEFAULT 0 COMMENT 'inscrite au journal (suivi à l''écran) : 1 oui' AFTER count_b;
