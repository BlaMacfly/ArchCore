-- Durabilité des objets (usure et réparation, AgpmEventItemRepair 0x34). La colonne existait sans être lue ni
-- écrite : toutes les lignes valent 0. Elle prend la valeur -1 par défaut, lue comme « pleine » (colonne DUR du
-- modèle dans itemdatatable.txt) : les lignes existantes ne paraissent pas cassées et tout objet inséré sans la
-- nommer (butin, achat, courrier, commande MJ…) naît plein. Une valeur de 0 ou plus est la durabilité actuelle,
-- écrite par l'usure ; une réparation remet -1.

ALTER TABLE character_items
    MODIFY COLUMN durability INT NOT NULL DEFAULT -1 COMMENT '-1 : pleine (DUR du modèle), sinon durabilité actuelle';

UPDATE character_items SET durability = -1 WHERE durability = 0;
