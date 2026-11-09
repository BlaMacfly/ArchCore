-- Quêtes données ou reprises par un objet du monde, rempli par `archcore import` :
--   quest_object  objets des obj*.ini qui portent l'événement de quête (fonction 27), avec le `QuestGroupID` de
--                 l'événement (groupe de quest_group, d'ordinaire le numéro de l'objet lui-même). Le client les désigne
--                 par un EVENT_BASE de type 1 (objet) ; le serveur ouvre la fenêtre, laisse prendre et rendre les quêtes
--                 du groupe à 1600 unités au plus de l'objet, comme le code partagé d'Archonia.
CREATE TABLE IF NOT EXISTS quest_object (
    object_id     INT UNSIGNED NOT NULL COMMENT 'section de l''objet dans son fichier obj*.ini',
    quest_group   INT UNSIGNED NOT NULL COMMENT 'QuestGroupID de l''événement (quest_group.id)',
    pos_x         DOUBLE   NOT NULL,
    pos_y         DOUBLE   NOT NULL,
    pos_z         DOUBLE   NOT NULL,
    PRIMARY KEY (object_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
