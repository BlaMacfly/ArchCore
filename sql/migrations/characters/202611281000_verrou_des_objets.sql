-- Verrou des objets (fenêtre UI_ItemLock d'Archonia, AGPMITEM op 36, drapeau 0x2000 du champ 19).
ALTER TABLE character_items
    ADD COLUMN IF NOT EXISTS locked TINYINT NOT NULL DEFAULT 0 COMMENT 'objet verrouillé : ni vendu, ni détruit, ni échangé, ni mis en vente, ni envoyé' AFTER expire_at;
