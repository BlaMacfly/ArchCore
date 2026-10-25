-- Réapparition des boss telle qu'Archonia l'annonce dans sa fenêtre des boss (UIBoss.xml du client) :
-- « Spawn Period: 30-45 minutes » donne respawn_sec = 1800 et respawn_max_sec = 2700. Le délai est tiré au
-- hasard dans cette plage à chaque mort. 0 : délai fixe (respawn_sec), comme pour les autres monstres.
-- Rempli par `archcore import`.

ALTER TABLE monster_spawn ADD COLUMN IF NOT EXISTS respawn_max_sec INT NOT NULL DEFAULT 0 COMMENT 'délai maximal de réapparition (UIBoss.xml), 0 : délai fixe' AFTER respawn_sec;
