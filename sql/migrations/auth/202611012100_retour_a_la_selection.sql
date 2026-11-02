-- Retour à la sélection des personnages (AgpmReturnToLogin 0x2F, `world/return_login.rs`) : clé à usage unique remise
-- au client par le serveur monde (op 5) et présentée au serveur de connexion (op 6), avec sa fin de validité. Les deux
-- serveurs peuvent tourner dans des processus séparés : la clé passe par la base.
ALTER TABLE account
    ADD COLUMN IF NOT EXISTS return_key INT UNSIGNED NULL COMMENT 'clé du retour à la sélection des personnages',
    ADD COLUMN IF NOT EXISTS return_key_until DATETIME NULL COMMENT 'fin de validité de return_key';
