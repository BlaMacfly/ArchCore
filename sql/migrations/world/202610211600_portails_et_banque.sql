-- Portails de téléportation et prix des cases de banque, remplis par `archcore import` :
--   teleport_point   points de TeleportPoint.ini (rayon d'arrivée, races admises, groupes)
--   teleport_object  objets des obj*.ini qui portent un point (événement 8) : départ et arrivée
--   teleport_fee     prix d'un trajet (TeleportFee.txt)
--   bank_slot_price  prix d'une page de banque en plus (bankslotprice.txt ; -1 : pas en vente)

CREATE TABLE IF NOT EXISTS teleport_point (
    name          VARCHAR(64) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL,
    target_type   TINYINT  NOT NULL DEFAULT 2 COMMENT '1 position fixe, 2 objet',
    pos_x         DOUBLE   NOT NULL DEFAULT 0,
    pos_y         DOUBLE   NOT NULL DEFAULT 0,
    pos_z         DOUBLE   NOT NULL DEFAULT 0,
    radius_min    FLOAT    NOT NULL DEFAULT 0,
    radius_max    FLOAT    NOT NULL DEFAULT 0,
    region_type   INT      NOT NULL DEFAULT 0 COMMENT '0 normale, 1 combat entre joueurs',
    special_type  INT      NOT NULL DEFAULT 0,
    use_type      INT      NOT NULL DEFAULT 0 COMMENT 'races admises : 1 humain, 2 orc, 4 elfe de lune, 8 dragonien ; 0 toutes',
    point_groups  TEXT     NOT NULL COMMENT 'groupes où le point est une arrivée (liste JSON)',
    target_groups TEXT     NOT NULL COMMENT 'groupes vers lesquels il mène (liste JSON)',
    PRIMARY KEY (name)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS teleport_object (
    object_id     INT UNSIGNED NOT NULL COMMENT 'section de l''objet dans son fichier obj*.ini',
    point         VARCHAR(64) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL,
    pos_x         DOUBLE   NOT NULL,
    pos_y         DOUBLE   NOT NULL,
    pos_z         DOUBLE   NOT NULL,
    PRIMARY KEY (object_id),
    KEY idx_teleport_object_point (point)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS teleport_fee (
    source        VARCHAR(64) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL,
    destination   VARCHAR(64) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL,
    fee           BIGINT   NOT NULL,
    PRIMARY KEY (source, destination)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS bank_slot_price (
    slot          TINYINT UNSIGNED NOT NULL COMMENT 'page achetée : 1 pour la deuxième page, etc.',
    price         BIGINT   NOT NULL COMMENT '-1 : pas en vente contre de l''or',
    PRIMARY KEY (slot)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
