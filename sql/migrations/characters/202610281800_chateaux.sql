-- Châteaux des guerres de siège (AgpmSiegeWar 0x49) : propriétaire, calendrier et candidatures.
--
-- `castle` : un château par ligne de `siege_war` (base du monde). Sans ligne, le château est sans propriétaire (PNJ) et
-- sans siège prévu. `status` : 0 aucun siège prévu, 1 candidatures ouvertes, 2 candidatures closes, 3 à 8 combat (un siège
-- en cours au redémarrage du serveur est clos sans changement de propriétaire). `carve_guild_id` : guilde du dernier carving
-- réussi. `declare_until` : fin du délai laissé au propriétaire pour déclarer le prochain siège ; `apply_until` : fin des
-- candidatures.
CREATE TABLE IF NOT EXISTS castle (
  id              TINYINT UNSIGNED NOT NULL COMMENT 'siege_war.id',
  owner_guild_id  INT UNSIGNED     NULL,
  carve_guild_id  INT UNSIGNED     NULL,
  status          TINYINT UNSIGNED NOT NULL DEFAULT 0,
  last_siege_at   DATETIME         NULL,
  next_siege_at   DATETIME         NULL,
  apply_until     DATETIME         NULL,
  declare_until   DATETIME         NULL,
  updated_at      DATETIME         NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  PRIMARY KEY (id),
  CONSTRAINT fk_castle_owner FOREIGN KEY (owner_guild_id) REFERENCES guild (id) ON DELETE SET NULL,
  CONSTRAINT fk_castle_carve FOREIGN KEY (carve_guild_id) REFERENCES guild (id) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- `castle_application` : candidatures d'une guilde au prochain siège d'un château, `side` 1 attaque, 2 défense ;
-- `selected` : retenue (assaillants à la clôture des candidatures, défenseurs choisis par le propriétaire).
CREATE TABLE IF NOT EXISTS castle_application (
  castle_id   TINYINT UNSIGNED NOT NULL,
  guild_id    INT UNSIGNED     NOT NULL,
  side        TINYINT UNSIGNED NOT NULL,
  selected    TINYINT UNSIGNED NOT NULL DEFAULT 0,
  applied_at  DATETIME         NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (castle_id, guild_id),
  KEY ix_castle_application_guild (guild_id),
  CONSTRAINT fk_castle_application_guild FOREIGN KEY (guild_id) REFERENCES guild (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
