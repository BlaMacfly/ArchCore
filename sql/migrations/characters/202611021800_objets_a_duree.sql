-- Objets à durée (`world/lifetime.rs`) : temps de jeu restant d'un objet Cash « RemainTime » (ms, décompté seulement en
-- usage et en jeu) et date d'expiration d'un objet Cash « ExpireTime » (secondes Unix, absolue : court même hors ligne),
-- fixés à la première utilisation ; 0 : pas encore utilisé. Envoyés aux champs 1 et 2 du mini Cash (AGPMITEM champ 23).
ALTER TABLE character_items
    ADD COLUMN IF NOT EXISTS remain_ms BIGINT NOT NULL DEFAULT 0 COMMENT 'temps de jeu restant (ms) d''un objet RemainTime ; 0 : pas commencé' AFTER durability,
    ADD COLUMN IF NOT EXISTS expire_at BIGINT NOT NULL DEFAULT 0 COMMENT 'date d''expiration (s Unix) d''un objet ExpireTime ; 0 : pas commencé' AFTER remain_ms;
