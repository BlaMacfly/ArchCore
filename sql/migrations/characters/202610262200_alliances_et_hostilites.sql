-- Alliances et hostilités entre guildes.
--   guild.joint_leader  guilde fondatrice de l'alliance (elle-même pour la fondatrice), NULL hors alliance
--   guild.joint_at      date d'entrée dans l'alliance
--   guild_hostile       une ligne par sens : guild_id est hostile à other_id depuis since
ALTER TABLE guild
    ADD COLUMN IF NOT EXISTS joint_leader  INT UNSIGNED NULL,
    ADD COLUMN IF NOT EXISTS joint_at      DATETIME     NULL;

CREATE TABLE IF NOT EXISTS guild_hostile (
    guild_id  INT UNSIGNED NOT NULL,
    other_id  INT UNSIGNED NOT NULL,
    since     DATETIME     NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (guild_id, other_id),
    CONSTRAINT fk_guild_hostile_guild FOREIGN KEY (guild_id) REFERENCES guild (id) ON DELETE CASCADE,
    CONSTRAINT fk_guild_hostile_other FOREIGN KEY (other_id) REFERENCES guild (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
