-- Charisme d'un personnage : points (0 à 1 000 000) envoyés tels quels dans le facteur CHAR_STATUS, sous-type 4
-- (CHA), dépensés par les compétences héroïques (« require_charisma_point » de SKILL_CONST.txt).

ALTER TABLE characters
    ADD COLUMN IF NOT EXISTS charisma INT UNSIGNED NOT NULL DEFAULT 0;
