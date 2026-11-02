-- Boutique des Shrines (`world/npc_trade.rs`) : prix d'un objet chez un PNJ en points de charisme (colonne 29 « NPC
-- Charisma Price » d'itemdatatable.txt, sous-type 5 du facteur prix d'Archonia) et en Shrine Coins (colonne 35 « NPC
-- Shrine Coin », sous-type 6 : objets TID 53000), remplis par `archcore import`. Ils s'ajoutent au prix en or. Les
-- monnaies ajoutées par Archonia (Archonium Coin, Siege Token, Bloodmark : colonnes 30, 33 et 34) ne sont pas importées.
ALTER TABLE item_data
    ADD COLUMN IF NOT EXISTS npc_charisma_price BIGINT NOT NULL DEFAULT 0 COMMENT 'prix en points de charisme chez un PNJ' AFTER pc_price,
    ADD COLUMN IF NOT EXISTS npc_shrine_coin BIGINT NOT NULL DEFAULT 0 COMMENT 'prix en Shrine Coins (TID 53000) chez un PNJ' AFTER npc_charisma_price;
