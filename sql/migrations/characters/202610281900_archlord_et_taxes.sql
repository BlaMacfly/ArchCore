-- Titre d'ArchLord (AgpmArchlord 0x55) et taxes des régions des châteaux (AgpmTax 0x53).
--
-- `archlord` : une seule ligne (id 1), l'ArchLord en titre : chef de la guilde qui a gravé le sceau de Rensphere à la
-- dernière guerre de Rensphere, ou désigné par un MJ (`/archlord set`) ; `character_id` NULL : aucun ArchLord.
CREATE TABLE IF NOT EXISTS archlord (
  id            TINYINT UNSIGNED NOT NULL DEFAULT 1,
  character_id  INT UNSIGNED     NULL,
  name          VARCHAR(48)      NOT NULL DEFAULT '',
  since         DATETIME         NULL,
  PRIMARY KEY (id),
  CONSTRAINT fk_archlord_character FOREIGN KEY (character_id) REFERENCES characters (id) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- `castle_tax` : comptes des taxes d'un château (`siege_war.id` de la base du monde) : recette totale déjà versée au
-- propriétaire, dernier versement, dernière modification des taux (une par semaine, du lundi au dimanche), début de la
-- période en cours (dernier versement, ou création de la ligne).
CREATE TABLE IF NOT EXISTS castle_tax (
  castle_id     TINYINT UNSIGNED NOT NULL,
  total_income  BIGINT           NOT NULL DEFAULT 0,
  last_paid_at  DATETIME         NULL,
  modified_at   DATETIME         NULL,
  period_start  DATETIME         NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (castle_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- `castle_tax_region` : régions taxées d'un château (région du château et de ses abords, numéros de RegionTemplate.ini) :
-- taux (0 à 50 %) et recette depuis le dernier versement.
CREATE TABLE IF NOT EXISTS castle_tax_region (
  region_id  SMALLINT UNSIGNED NOT NULL,
  castle_id  TINYINT UNSIGNED  NOT NULL,
  ratio      TINYINT UNSIGNED  NOT NULL DEFAULT 0,
  income     BIGINT            NOT NULL DEFAULT 0,
  PRIMARY KEY (region_id),
  KEY ix_castle_tax_region_castle (castle_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
