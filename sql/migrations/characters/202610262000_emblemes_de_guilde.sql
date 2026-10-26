-- Emblème d'une guilde : TID (fond | motif << 8 | symbole << 16) et couleur (indice par couche).
ALTER TABLE guild
    ADD COLUMN IF NOT EXISTS mark_tid    INT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS mark_color  INT NOT NULL DEFAULT 0;
