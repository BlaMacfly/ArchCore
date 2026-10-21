-- Base des journaux (archcore_logs) : traces de l'économie et de l'administration, en écriture seule.
-- Le serveur n'y lit jamais ; sans clé étrangère (les personnages et comptes peuvent disparaître, la trace
-- reste), avec des index par personnage ou compte et par date pour les enquêtes.

CREATE TABLE IF NOT EXISTS log_trade (
  id      BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
  at      DATETIME(3)     NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
  char_a  INT UNSIGNED    NOT NULL,
  name_a  VARCHAR(48)     NOT NULL,
  char_b  INT UNSIGNED    NOT NULL,
  name_b  VARCHAR(48)     NOT NULL,
  gold_a  BIGINT          NOT NULL DEFAULT 0 COMMENT 'or donné par A',
  gold_b  BIGINT          NOT NULL DEFAULT 0 COMMENT 'or donné par B',
  items_a JSON            NOT NULL COMMENT 'objets donnés par A : [{iid, tid, count}]',
  items_b JSON            NOT NULL COMMENT 'objets donnés par B',
  PRIMARY KEY (id),
  KEY ix_log_trade_a (char_a, at),
  KEY ix_log_trade_b (char_b, at),
  KEY ix_log_trade_at (at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='échanges conclus entre joueurs';

CREATE TABLE IF NOT EXISTS log_shop (
  id           BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
  at           DATETIME(3)     NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
  character_id INT UNSIGNED    NOT NULL,
  name         VARCHAR(48)     NOT NULL,
  npc          VARCHAR(96)     NOT NULL DEFAULT '',
  action       VARCHAR(8)      NOT NULL COMMENT 'achat ou vente',
  item_tid     INT UNSIGNED    NOT NULL,
  item_iid     BIGINT UNSIGNED NOT NULL,
  count        INT             NOT NULL,
  price        BIGINT          NOT NULL COMMENT 'or payé (achat) ou reçu (vente)',
  PRIMARY KEY (id),
  KEY ix_log_shop_character (character_id, at),
  KEY ix_log_shop_at (at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='achats et ventes aux PNJ';

CREATE TABLE IF NOT EXISTS log_bank (
  id           BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
  at           DATETIME(3)     NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
  account_id   INT UNSIGNED    NOT NULL,
  character_id INT UNSIGNED    NOT NULL,
  name         VARCHAR(48)     NOT NULL,
  action       VARCHAR(8)      NOT NULL COMMENT 'dépôt ou retrait',
  item_tid     INT UNSIGNED    NULL,
  item_iid     BIGINT UNSIGNED NULL,
  count        INT             NULL,
  money        BIGINT          NULL COMMENT 'or déplacé',
  PRIMARY KEY (id),
  KEY ix_log_bank_account (account_id, at),
  KEY ix_log_bank_character (character_id, at),
  KEY ix_log_bank_at (at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='mouvements de la banque des comptes (objets et or)';

CREATE TABLE IF NOT EXISTS log_gm (
  id           BIGINT UNSIGNED  NOT NULL AUTO_INCREMENT,
  at           DATETIME(3)      NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
  account_id   INT UNSIGNED     NOT NULL,
  character_id INT UNSIGNED     NOT NULL,
  name         VARCHAR(48)      NOT NULL,
  gm_level     TINYINT UNSIGNED NOT NULL,
  command      VARCHAR(48)      NOT NULL,
  args         VARCHAR(512)     NOT NULL DEFAULT '',
  ok           TINYINT(1)       NOT NULL COMMENT '1 : commande exécutée, 0 : refusée (arguments)',
  reply        VARCHAR(512)     NOT NULL DEFAULT '',
  PRIMARY KEY (id),
  KEY ix_log_gm_character (character_id, at),
  KEY ix_log_gm_command (command, at),
  KEY ix_log_gm_at (at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='commandes MJ exécutées';
