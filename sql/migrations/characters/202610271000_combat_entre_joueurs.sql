-- Combat entre joueurs (AgpmPvP) : victoires et défaites, points de meurtrier (facteur CHAR_STATUS, sous-type 11),
-- fin du statut de « premier attaquant » (champ 12 du paquet personnage, 20 min) et fin du délai de meurtrier
-- (2 h avant la rémission), en secondes Unix (0 : aucun).

ALTER TABLE characters
    ADD COLUMN IF NOT EXISTS pvp_win INT UNSIGNED NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS pvp_lose INT UNSIGNED NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS murderer_points INT UNSIGNED NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS criminal_until BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS murderer_until BIGINT NOT NULL DEFAULT 0;
