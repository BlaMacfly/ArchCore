-- Options des objets : TID d'itemoptiontable.txt tirés à la création d'un équipement (5 au plus),
-- séparés par des virgules.

ALTER TABLE character_items
    ADD COLUMN IF NOT EXISTS option_tids VARCHAR(40) NOT NULL DEFAULT '';
