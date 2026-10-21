-- Taille des guildes (guildmaxmember.txt), remplie par `archcore import` : la première ligne donne la taille
-- d'une guilde neuve et le prix de sa création ; les suivantes, le prix de chaque agrandissement.

CREATE TABLE IF NOT EXISTS guild_max_member (
    max_members   INT     NOT NULL,
    gold          BIGINT  NOT NULL DEFAULT 0,
    skull_tid     INT     NOT NULL DEFAULT 0,
    skull_count   INT     NOT NULL DEFAULT 0,
    PRIMARY KEY (max_members)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
