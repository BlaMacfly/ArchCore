-- Barre de raccourcis gardée par le serveur (`world/ui_status.rs`, AgpmUIStatus champ 2) : 5 couches de 10 cases.
-- Une ligne par case remplie. Une compétence est gardée par son TID (son identifiant d'instance change à chaque entrée
-- en jeu) ; un objet par son IID et son TID (objet consommé : remplacé à l'entrée en jeu par un autre de même TID).
-- Les lignes partent avec le personnage supprimé.
CREATE TABLE IF NOT EXISTS character_quickbelt (
    character_id INT UNSIGNED     NOT NULL,
    slot         TINYINT UNSIGNED NOT NULL COMMENT '0-49 : couche * 10 + colonne',
    base_type    TINYINT UNSIGNED NOT NULL COMMENT '5 objet, 7 compétence',
    tid          INT UNSIGNED     NOT NULL COMMENT 'modèle de l''objet ou de la compétence',
    item_id      INT UNSIGNED     NULL COMMENT 'IID (character_items.id) d''un objet',
    PRIMARY KEY (character_id, slot)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
