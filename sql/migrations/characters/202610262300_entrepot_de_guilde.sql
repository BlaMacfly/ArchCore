-- Entrepôt de guilde : argent et pages en plus ; ses objets sont des lignes de character_items au statut 3,
-- avec guild_id (le personnage qui a déposé l'objet reste noté dans character_id).
CREATE TABLE IF NOT EXISTS guild_warehouse (
    guild_id  INT UNSIGNED     NOT NULL,
    money     BIGINT UNSIGNED  NOT NULL DEFAULT 0,
    slots     TINYINT UNSIGNED NOT NULL DEFAULT 0 COMMENT 'pages en plus de la première',
    PRIMARY KEY (guild_id),
    CONSTRAINT fk_guild_warehouse_guild FOREIGN KEY (guild_id) REFERENCES guild (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

ALTER TABLE character_items
    ADD COLUMN IF NOT EXISTS guild_id INT UNSIGNED NULL,
    ADD KEY IF NOT EXISTS ix_item_guild (guild_id);
