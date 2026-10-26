-- Emblèmes de guilde (guildmark.txt), remplis par `archcore import` : un élément par fond (couche 1),
-- motif (2), symbole (3) ou couleur (4). Le TID est le numéro décalé de 8 bits par couche ; l'emblème d'une
-- guilde est la somme d'un fond, d'un motif et d'un symbole, et sa couleur un indice de couleur par couche.
CREATE TABLE IF NOT EXISTS guild_mark (
    tid           INT UNSIGNED NOT NULL,
    layer         TINYINT      NOT NULL,
    number        INT          NOT NULL,
    name          VARCHAR(64)  NOT NULL DEFAULT '',
    gold          BIGINT       NOT NULL DEFAULT 0,
    skull_tid     INT          NOT NULL DEFAULT 0,
    skull_count   INT          NOT NULL DEFAULT 0,
    cash          BIGINT       NOT NULL DEFAULT 0,
    argb          BIGINT       NOT NULL DEFAULT 0,
    PRIMARY KEY (tid)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
