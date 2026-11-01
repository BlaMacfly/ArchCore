-- Évolution du Dragon Scion (`world/evolution.rs`) : forme prise par Slayer, Orbiter ou Summoner of Evolution (TID 1722,
-- 1723, 1732 ; 0 : aucune). `template_id` reste celui du Scion (1724) ; la forme est rendue à l'entrée en jeu et
-- survit à la mort, jusqu'à l'annulation.
ALTER TABLE characters
    ADD COLUMN IF NOT EXISTS evolution_tid INT UNSIGNED NOT NULL DEFAULT 0 COMMENT 'forme évoluée du Dragon Scion, 0 : aucune' AFTER template_id;
