-- Journal des imports et origine des textes localisés.
--
-- import_run : une ligne par source lue par `archcore import` ou `archcore content` (fichiers du client,
-- relief, tables du serveur d'origine, couche de contenu content/), avec l'empreinte des fichiers lus et la
-- version d'ArchCore. finished_at reste NULL si l'import s'est arrêté en route.
--
-- localized_text.source : « import » pour les textes écrits par `archcore i18n db` (qui ne remplace plus
-- que ceux-là), « manual » pour les textes saisis autrement (à la main, éditeur), qui survivent. Les lignes
-- existantes viennent toutes d'`i18n db` : elles sont marquées « import », puis la valeur par défaut passe
-- à « manual ».

CREATE TABLE IF NOT EXISTS import_run (
  id               INT UNSIGNED NOT NULL AUTO_INCREMENT,
  source           VARCHAR(16)  NOT NULL COMMENT 'client, relief, serveur ou contenu',
  location         VARCHAR(255) NOT NULL DEFAULT '' COMMENT 'dossier lu',
  files            INT UNSIGNED NOT NULL DEFAULT 0 COMMENT 'fichiers lus',
  fingerprint      CHAR(64)     NOT NULL COMMENT 'SHA-256 des noms et tailles des fichiers (contenu : de leurs textes)',
  archcore_version VARCHAR(32)  NOT NULL DEFAULT '',
  started_at       DATETIME     NOT NULL DEFAULT CURRENT_TIMESTAMP,
  finished_at      DATETIME     NULL COMMENT 'NULL : import interrompu',
  PRIMARY KEY (id),
  KEY ix_import_run_source (source, id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='journal des imports du monde';

ALTER TABLE localized_text ADD COLUMN IF NOT EXISTS source VARCHAR(8) NOT NULL DEFAULT 'import' COMMENT 'import (archcore i18n db) ou manual (saisi autrement)';
ALTER TABLE localized_text ALTER COLUMN source SET DEFAULT 'manual';
