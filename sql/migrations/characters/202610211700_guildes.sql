-- Guildes : la guilde et ses membres.
--   guild         nom (lettres et chiffres, 32 au plus), mot de passe de dissolution, annonce, taille
--   guild_member  un personnage appartient à une guilde au plus ; rang 1 membre, 2 demande d'adhésion,
--                 3 demande de départ, 10 chef, 11 second ; leave_requested_at date la demande de départ

CREATE TABLE IF NOT EXISTS guild (
    id            INT UNSIGNED NOT NULL AUTO_INCREMENT,
    name          VARCHAR(32)  NOT NULL,
    password      VARCHAR(12)  NOT NULL DEFAULT '',
    notice        VARCHAR(255) NOT NULL DEFAULT '',
    max_members   INT          NOT NULL DEFAULT 50,
    created_at    DATETIME     NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (id),
    UNIQUE KEY uq_guild_name (name)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS guild_member (
    character_id        INT UNSIGNED NOT NULL,
    guild_id            INT UNSIGNED NOT NULL,
    `rank`              TINYINT      NOT NULL DEFAULT 1,
    joined_at           DATETIME     NOT NULL DEFAULT CURRENT_TIMESTAMP,
    leave_requested_at  DATETIME     NULL,
    PRIMARY KEY (character_id),
    KEY idx_guild_member_guild (guild_id),
    CONSTRAINT fk_guild_member_character FOREIGN KEY (character_id) REFERENCES characters (id) ON DELETE CASCADE,
    CONSTRAINT fk_guild_member_guild FOREIGN KEY (guild_id) REFERENCES guild (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
