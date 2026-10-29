-- Anti-triche (world/anticheat.rs) : anomalies relevées par le serveur, une ligne par anomalie notée (20 au plus
-- par personnage et par minute, les suivantes ne sont que comptées). Lue par la commande MJ /anomalies.
CREATE TABLE IF NOT EXISTS log_anomaly (
  id           BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
  at           DATETIME(3)     NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
  account_id   INT UNSIGNED    NOT NULL,
  character_id INT UNSIGNED    NOT NULL,
  name         VARCHAR(48)     NOT NULL,
  kind         VARCHAR(16)     NOT NULL COMMENT 'vitesse, téléportation, cadence ou inondation',
  details      VARCHAR(512)    NOT NULL DEFAULT '',
  action       VARCHAR(8)      NOT NULL DEFAULT '' COMMENT 'sanction appliquée à cette anomalie : kick ou ban ; vide : aucune',
  PRIMARY KEY (id),
  KEY ix_log_anomaly_character (character_id, at),
  KEY ix_log_anomaly_account (account_id, at),
  KEY ix_log_anomaly_kind (kind, at),
  KEY ix_log_anomaly_at (at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='anomalies relevées par l''anti-triche';
