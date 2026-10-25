-- Conversion des objets : renforcement physique, sockets et ce qui y est serti.
--   convert_level  niveau de renforcement (0 à 16, ItemConvertTable.txt)
--   socket_count   sockets ouverts (8 au plus)
--   socket_tids    TID serties dans l'ordre (pierres d'esprit, runes), séparés par des virgules

ALTER TABLE character_items
    ADD COLUMN IF NOT EXISTS convert_level TINYINT UNSIGNED NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS socket_count  TINYINT UNSIGNED NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS socket_tids   VARCHAR(96)      NOT NULL DEFAULT '';
