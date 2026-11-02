-- Signature of Glory (`world/consumables.rs`, commande `/nick`) : signature du personnage, envoyée au champ 27
-- d'AGPMCHARACTER (CHAR[16]). Vide : aucune.
ALTER TABLE characters
    ADD COLUMN IF NOT EXISTS signature VARCHAR(16) NOT NULL DEFAULT '' COMMENT 'signature prise par /nick (Signature of Glory)' AFTER skill_init;
