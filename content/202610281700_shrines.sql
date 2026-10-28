-- Combats de Shrine (épisode 8) : valeurs propres à ArchCore, les tables du serveur d'origine n'étant pas livrées.
-- Les huit Shrines (régions centrales 236 à 243) ouvrent tous les jours, répartis sur 20 h, 21 h et 22 h, pour
-- 30 minutes ; gravure en 60 s, occupation de 5 minutes (300 s) ; entrée dès le niveau 50, 50 joueurs par race au plus.
INSERT INTO shrine (id, name, region_id, days, open_hour, open_minute, duration_min, carve_sec, occupy_sec, level_min, level_max, max_per_race) VALUES
  (1, 'Shrine of Lightning', 236, 127, 20, 0, 30, 60, 300, 50, 0, 50),
  (2, 'Shrine of Syripus',   237, 127, 20, 0, 30, 60, 300, 50, 0, 50),
  (3, 'Shrine of Water',     238, 127, 20, 0, 30, 60, 300, 50, 0, 50),
  (4, 'Shrine of Lava',      239, 127, 21, 0, 30, 60, 300, 50, 0, 50),
  (5, 'Shrine of Wind',      240, 127, 21, 0, 30, 60, 300, 50, 0, 50),
  (6, 'Shrine of Poison',    241, 127, 21, 0, 30, 60, 300, 50, 0, 50),
  (7, 'Shrine of Fire',      242, 127, 22, 0, 30, 60, 300, 50, 0, 50),
  (8, 'Shrine of the Spirit', 243, 127, 22, 0, 30, 60, 300, 50, 0, 50)
ON DUPLICATE KEY UPDATE name = VALUES(name), region_id = VALUES(region_id), days = VALUES(days), open_hour = VALUES(open_hour),
  open_minute = VALUES(open_minute), duration_min = VALUES(duration_min), carve_sec = VALUES(carve_sec), occupy_sec = VALUES(occupy_sec),
  level_min = VALUES(level_min), level_max = VALUES(level_max), max_per_race = VALUES(max_per_race);

-- Récompenses d'un combat gagné : Shrine Coins (objet 53000, « Arena Coin » dans Archonia) et charisme.
INSERT INTO shrine_reward (category, item_tid, item_count, charisma) VALUES
  ('Carving', 53000, 10, 20),
  ('WinRace', 53000, 5, 10),
  ('Common',  53000, 1, 2)
ON DUPLICATE KEY UPDATE item_count = VALUES(item_count), charisma = VALUES(charisma);
