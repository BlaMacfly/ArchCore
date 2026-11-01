-- Portées des événements (retour de test n° 1), remplies par `archcore import` :
--   refinery_object          sphères d'alchimie : objets des obj*.ini qui portent l'événement de raffinage (fonction
--                            26). Le serveur n'ouvre la fenêtre de raffinage qu'à portée de l'une d'elles (1600 unités,
--                            comme le code partagé d'Archonia).
--   teleport_point.point_id  numéro du point : son rang dans TeleportPoint.ini, à partir de 1, comme au chargement du
--                            client ; il désigne les points désactivés envoyés avec l'accord d'un portail (op 5,
--                            champ 5).
CREATE TABLE IF NOT EXISTS refinery_object (
    object_id     INT UNSIGNED NOT NULL COMMENT 'section de l''objet dans son fichier obj*.ini',
    pos_x         DOUBLE   NOT NULL,
    pos_y         DOUBLE   NOT NULL,
    pos_z         DOUBLE   NOT NULL,
    PRIMARY KEY (object_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

ALTER TABLE teleport_point
    ADD COLUMN IF NOT EXISTS point_id INT NOT NULL DEFAULT 0 COMMENT 'rang dans TeleportPoint.ini, à partir de 1' AFTER name;
