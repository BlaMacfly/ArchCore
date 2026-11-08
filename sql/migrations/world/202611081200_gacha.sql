-- Gacha de l'épisode 8 (« Fortune Gate », AgpmEventGacha), rempli par `archcore import` :
--   gacha_object  socles de tirage : objets des obj*.ini qui portent l'événement de gacha (fonction 38), avec le
--                 type de tirage de leurs données (`GachaStart` / `Type=` / `GachaEnd` : 1 armes, 2 armures,
--                 3 accessoires, 4 événement, 5 charisme). Le serveur n'ouvre la fenêtre qu'à portée de l'un d'eux
--                 (600 unités, comme le code partagé d'Archonia).
-- Les tables de tirage (gachatypetemplate.ini, gachadroptable*.txt) vont dans `client_table`.
CREATE TABLE IF NOT EXISTS gacha_object (
    object_id     INT UNSIGNED NOT NULL COMMENT 'section de l''objet dans son fichier obj*.ini',
    gacha_type    INT UNSIGNED NOT NULL COMMENT 'section de gachatypetemplate.ini',
    pos_x         DOUBLE   NOT NULL,
    pos_y         DOUBLE   NOT NULL,
    pos_z         DOUBLE   NOT NULL,
    PRIMARY KEY (object_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
