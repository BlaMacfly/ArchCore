-- Statut d'interface d'un personnage (AgpmUIStatus, 0x14) : potions de vie et de mana de la barre,
-- seuils de leur prise automatique en pourcentage (curseurs posés sur les jauges de PV et de PM de
-- UI_CharInfo, 0 : désactivé) et affichage du casque. Envoyé au client à l'entrée en jeu.

CREATE TABLE IF NOT EXISTS character_ui_status (
    character_id   INT UNSIGNED NOT NULL,
    hp_potion_tid  INT          NOT NULL DEFAULT 0,
    mp_potion_tid  INT          NOT NULL DEFAULT 0,
    view_helmet    TINYINT      NOT NULL DEFAULT 1,
    auto_hp        TINYINT      NOT NULL DEFAULT 0,
    auto_mp        TINYINT      NOT NULL DEFAULT 0,
    PRIMARY KEY (character_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
