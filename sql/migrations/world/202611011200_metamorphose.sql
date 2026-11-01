-- Métamorphose (`world/morph.rs`) : bonus des potions de métamorphose, d'après ItemTransformTable.txt, remplis par
-- `archcore import`. Une ligne par potion (`item_tid`, TID de l'objet d'itemdatatable.txt) ; `tid` (forme), `level` et
-- `name` (nom coréen) sont gardés pour mémoire, la forme et la durée venant de l'objet (« Polymorph Id »,
-- « Polymorph Dur »). Les bonus s'ajoutent aux facteurs du joueur transformé ; ils peuvent être négatifs.
CREATE TABLE IF NOT EXISTS item_transform (
    item_tid      INT UNSIGNED NOT NULL,
    tid           INT UNSIGNED NOT NULL DEFAULT 0 COMMENT 'modèle de la forme (colonne Tid)',
    level         INT          NOT NULL DEFAULT 0,
    name          VARCHAR(64)  NOT NULL DEFAULT '',
    movement_run  INT          NOT NULL DEFAULT 0 COMMENT 'course (« Movement Run »)',
    atk_speed     INT          NOT NULL DEFAULT 0 COMMENT 'vitesse d''attaque (« ATK Speed »)',
    max_hp        INT          NOT NULL DEFAULT 0,
    max_mp        INT          NOT NULL DEFAULT 0,
    defense       INT          NOT NULL DEFAULT 0 COMMENT 'défense physique (« Defence »)',
    min_damage    INT          NOT NULL DEFAULT 0,
    max_damage    INT          NOT NULL DEFAULT 0,
    PRIMARY KEY (item_tid)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
