-- ArchCore — base d'authentification (archcore_auth)
-- Comptes, groupes de serveurs et liste des mondes.

CREATE TABLE IF NOT EXISTS account (
  id            INT UNSIGNED NOT NULL AUTO_INCREMENT,
  username      VARCHAR(48)  NOT NULL,
  password_hash CHAR(64)     NOT NULL COMMENT 'SHA-256 hexadécimal du mot de passe',
  auth_token    INT          NULL COMMENT 'clé remise au client pour entrer sur le monde',
  gm_level      TINYINT UNSIGNED NOT NULL DEFAULT 0,
  banned_until  DATETIME     NULL,
  last_login    DATETIME     NULL,
  last_ip       VARCHAR(45)  NULL,
  created_at    DATETIME     NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (id),
  UNIQUE KEY uq_account_username (username)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS server_group (
  id   INT UNSIGNED NOT NULL,
  name VARCHAR(32)  NOT NULL,
  PRIMARY KEY (id),
  UNIQUE KEY uq_group_name (name)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS world (
  id        INT UNSIGNED NOT NULL,
  group_id  INT UNSIGNED NOT NULL,
  name      VARCHAR(32)  NOT NULL,
  address   VARCHAR(23)  NOT NULL COMMENT 'ip:port du serveur monde, 23 octets max dans le paquet',
  status    INT UNSIGNED NOT NULL DEFAULT 4,
  flag      INT UNSIGNED NOT NULL DEFAULT 4,
  PRIMARY KEY (id),
  UNIQUE KEY uq_world_name (name),
  KEY ix_world_group (group_id),
  CONSTRAINT fk_world_group FOREIGN KEY (group_id) REFERENCES server_group (id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

INSERT IGNORE INTO server_group (id, name) VALUES (1, 'ArchCore');
INSERT IGNORE INTO world (id, group_id, name, address, status, flag) VALUES (1, 1, 'ArchCore', '127.0.0.1:11008', 4, 4);
