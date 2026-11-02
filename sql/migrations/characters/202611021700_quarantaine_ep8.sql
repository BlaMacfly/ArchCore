-- Passage des personnages aux valeurs de l'épisode 8 (`archcore migrate-ep8 --apply`, `tools/migrate_ep8.rs`) :
-- ce que la migration retire ou change est gardé ici, rien n'est détruit ; `--restore <passe>` le remet.
--   kind     item (ligne de character_items en JSON, avec les courriers et ventes qui la citaient), options,
--            sockets (option_tids / socket_tids d'origine), skill_level (niveau d'origine), skill (compétence
--            retirée), skill_init (valeur d'origine du « skill init »)
--   ref_id   character_items.id, ou TID de la compétence
CREATE TABLE IF NOT EXISTS ep8_quarantine (
  id            BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
  run_id        INT UNSIGNED    NOT NULL COMMENT 'passe de migrate-ep8 --apply',
  character_id  INT UNSIGNED    NOT NULL,
  kind          VARCHAR(16)     NOT NULL,
  ref_id        BIGINT UNSIGNED NOT NULL DEFAULT 0,
  data          LONGTEXT        NOT NULL COMMENT 'valeurs d''origine (JSON)',
  reason        VARCHAR(128)    NOT NULL DEFAULT '',
  created_at    DATETIME        NOT NULL DEFAULT CURRENT_TIMESTAMP,
  restored_at   DATETIME        NULL COMMENT 'remise par --restore',
  PRIMARY KEY (id),
  KEY ix_ep8_run (run_id),
  KEY ix_ep8_character (character_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
