-- Bannissements des comptes, avec leur historique : une ligne par bannissement (durée, auteur, raison).
-- Additive : account.banned_until reste lue pendant une version, puis sera retirée par une autre migration.
-- Les bannissements encore en cours y sont recopiés.

CREATE TABLE IF NOT EXISTS account_banned (
  id           INT UNSIGNED NOT NULL AUTO_INCREMENT,
  account_id   INT UNSIGNED NOT NULL,
  banned_at    DATETIME     NOT NULL DEFAULT CURRENT_TIMESTAMP,
  banned_until DATETIME     NULL COMMENT 'NULL : sans fin',
  banned_by    VARCHAR(48)  NULL COMMENT 'auteur (MJ ou outil)',
  reason       VARCHAR(255) NULL,
  active       TINYINT(1)   NOT NULL DEFAULT 1 COMMENT '0 : levé avant son terme',
  PRIMARY KEY (id),
  KEY ix_account_banned_account (account_id, active),
  CONSTRAINT fk_account_banned_account FOREIGN KEY (account_id) REFERENCES account (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

INSERT INTO account_banned (account_id, banned_until, reason)
SELECT a.id, a.banned_until, 'repris de account.banned_until'
FROM account a
WHERE a.banned_until IS NOT NULL AND a.banned_until > NOW()
  AND NOT EXISTS (SELECT 1 FROM account_banned b WHERE b.account_id = a.id);
