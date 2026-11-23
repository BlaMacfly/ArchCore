-- PV, PM et PE (colonnes hp, mp, sp déjà là, remplies à la création seulement) : désormais enregistrés avec la position
-- (world/presence.rs). État « mort » et région de rattachement enregistrés aussi : un personnage déconnecté mort revient
-- au point de résurrection de sa région de rattachement, comme une résurrection au village.
ALTER TABLE characters
    ADD COLUMN IF NOT EXISTS dead TINYINT(1) NOT NULL DEFAULT 0 COMMENT 'mort en attente de résurrection à la déconnexion' AFTER sp,
    ADD COLUMN IF NOT EXISTS bound_region SMALLINT UNSIGNED NOT NULL DEFAULT 0 COMMENT 'région sûre de rattachement ; 0 : inconnue (recalculée)' AFTER dead;
