-- Maison des ventes : mises en vente, achats, annulations et encaissements (AgpmAuction, type 0x0B).
CREATE TABLE IF NOT EXISTS log_auction (
  id           BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
  at           DATETIME(3)     NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
  sale_id      INT UNSIGNED    NOT NULL,
  action       VARCHAR(12)     NOT NULL COMMENT 'vente, achat, annulation ou encaissement',
  character_id INT UNSIGNED    NOT NULL COMMENT 'auteur : vendeur, ou acheteur pour un achat',
  name         VARCHAR(48)     NOT NULL,
  other_id     INT UNSIGNED    NULL COMMENT 'achat : le vendeur',
  item_tid     INT UNSIGNED    NOT NULL,
  item_iid     BIGINT UNSIGNED NULL,
  count        INT             NOT NULL,
  gold         BIGINT          NOT NULL DEFAULT 0 COMMENT 'prix en or de la vente (gain pour un encaissement)',
  cc           BIGINT          NOT NULL DEFAULT 0,
  fee_gold     BIGINT          NOT NULL DEFAULT 0 COMMENT 'frais payés (mise en vente)',
  fee_cc      BIGINT          NOT NULL DEFAULT 0,
  PRIMARY KEY (id),
  KEY ix_log_auction_character (character_id, at),
  KEY ix_log_auction_sale (sale_id),
  KEY ix_log_auction_at (at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='maison des ventes entre joueurs';
