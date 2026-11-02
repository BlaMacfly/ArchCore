-- Oubli des compétences chez le maître (`world/consumables.rs`, AgpmEventSkillMaster op 8) : chaîne « skill init » du
-- personnage, envoyée au champ 19 d'AGPMCHARACTER (32 caractères). Vide : l'oubli gratuit est encore permis ; « 1 » :
-- déjà fait (une seule fois par personnage). La Gem of origin ne la touche pas.
ALTER TABLE characters
    ADD COLUMN IF NOT EXISTS skill_init VARCHAR(32) NOT NULL DEFAULT '' COMMENT 'oubli des compétences chez le maître : vide permis, « 1 » fait' AFTER evolution_tid;
