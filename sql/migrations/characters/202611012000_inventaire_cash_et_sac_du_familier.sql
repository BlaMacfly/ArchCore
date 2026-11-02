-- Inventaire Cash et sac du familier (`world/items.rs`, `world/pet_bag.rs`). Deux statuts enregistrés de plus dans
-- `character_items.status` : 6 inventaire Cash (statut 51 du protocole, rang dans la liste en `row_idx`), 7 sac du
-- familier (statut 13 : sac 0 à 3, ligne 0 à 1, colonne 0 à 3). Les objets Cash déjà en base (statut 0) sont rangés
-- dans l'inventaire Cash à l'entrée en jeu du personnage (`Inventory::sort_cash`) : la colonne « Cash » de leur modèle
-- est dans la base du monde, que cette migration ne voit pas ; leur case de l'inventaire se libère alors.
-- Ramassage automatique du sac du familier (AGPMITEM op 30) : 1 par défaut, comme le client.
ALTER TABLE character_items
    MODIFY COLUMN status TINYINT UNSIGNED NOT NULL DEFAULT 0 COMMENT '0 inventaire, 1 équipé, 2 banque, 3 entrepôt de guilde, 4 courrier, 5 vente, 6 inventaire Cash, 7 sac du familier';

ALTER TABLE characters
    ADD COLUMN IF NOT EXISTS pet_auto_pick TINYINT UNSIGNED NOT NULL DEFAULT 1 COMMENT 'ramassage automatique du sac du familier (AGPMITEM op 30)' AFTER skill_init;
