-- Ramassage d'un objet de quête sur un objet du monde (AGPMQUEST op 11), rempli par `archcore import --webzen` :
--   quest_group.gather  5ᵉ colonne de questgroup.ini (« TID-rang: ») : quêtes dont un objet (`ObjectItem<rang>_TID`)
--                       se ramasse sur l'objet du monde du groupe, en JSON [[TID, rang], …] (rang 0 = ObjectA …
--                       4 = ObjectE). Le serveur suit les colonnes de la quête ; la liste sert à l'export du client.
ALTER TABLE quest_group ADD COLUMN IF NOT EXISTS gather JSON NOT NULL DEFAULT '[]' COMMENT 'ramassage sur l''objet du monde du groupe : [[TID, rang], …]' AFTER confirm;
