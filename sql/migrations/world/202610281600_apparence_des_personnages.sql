-- Coiffures et visages (charactercustomizelist.txt du client), remplis par `archcore import` : une ligne par numéro
-- proposé pour un modèle de personnage. `kind` : 1 coiffure (« Hair »), 2 visage (« Face »). `case_kind` (colonne
-- « Case ») : 0 création et coiffeur, 1 coiffeur seulement, 2 création seulement. `use_level` : niveau exigé ;
-- `price` : prix en or, ou nombre d'objets `price_item_tid` si celui-ci n'est pas nul (toujours 0 dans les données).
-- `row_order` garde l'ordre du fichier, celui du calcul de prix du client.
CREATE TABLE IF NOT EXISTS character_customize (
  row_order       SMALLINT UNSIGNED NOT NULL,
  kind            TINYINT UNSIGNED  NOT NULL COMMENT '1 coiffure, 2 visage',
  number          TINYINT UNSIGNED  NOT NULL COMMENT 'numéro envoyé au client (AGPMCHARACTER 20 / 21)',
  name            VARCHAR(64)       NOT NULL DEFAULT '',
  case_kind       TINYINT UNSIGNED  NOT NULL DEFAULT 0 COMMENT '0 création et coiffeur, 1 coiffeur, 2 création',
  character_tid   INT UNSIGNED      NOT NULL,
  use_level       INT               NOT NULL DEFAULT 1,
  price           BIGINT            NOT NULL DEFAULT 0,
  price_item_tid  INT UNSIGNED      NOT NULL DEFAULT 0,
  PRIMARY KEY (row_order),
  UNIQUE KEY ux_customize (character_tid, kind, number)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
