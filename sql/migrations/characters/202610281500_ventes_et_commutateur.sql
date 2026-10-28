-- Maison des ventes (AgpmAuction, type 0x0B) : achat immédiat de la pile entière à prix fixe. L'identifiant sert à la
-- fois de numéro de vente (salesID) et de clé de recherche (docID, croissante à l'insertion). L'objet reste une ligne
-- de character_items (options, conversion, durabilité conservées) au statut 5 « en vente », rattachée au vendeur ;
-- à l'achat il passe à l'acheteur (`item_id` repasse à NULL, la vente au statut 1) et le vendeur encaisse ensuite
-- (CONFIRM), ce qui efface la ligne. Une annulation rend l'objet et efface aussi la ligne. Les frais (3 %) sont
-- prélevés à la mise en vente et notés pour mémoire. Après `expires_at`, la vente n'est plus proposée à la recherche
-- mais reste annulable.
CREATE TABLE IF NOT EXISTS auction_sale (
  id             INT UNSIGNED     NOT NULL AUTO_INCREMENT COMMENT 'salesID et docID du paquet',
  seller_id      INT UNSIGNED     NOT NULL,
  item_id        BIGINT UNSIGNED  NULL COMMENT 'objet en vente (character_items, statut 5) ; NULL une fois vendu',
  item_tid       INT UNSIGNED     NOT NULL,
  quantity       INT UNSIGNED     NOT NULL,
  price_gold     BIGINT UNSIGNED  NOT NULL DEFAULT 0,
  price_cc       BIGINT UNSIGNED  NOT NULL DEFAULT 0,
  fee_gold       BIGINT UNSIGNED  NOT NULL DEFAULT 0 COMMENT 'frais prélevés à la mise en vente, non remboursés',
  fee_cc         BIGINT UNSIGNED  NOT NULL DEFAULT 0,
  status         TINYINT UNSIGNED NOT NULL DEFAULT 0 COMMENT '0 en vente, 1 vendu (à encaisser)',
  registered_at  DATETIME         NOT NULL DEFAULT CURRENT_TIMESTAMP,
  expires_at     DATETIME         NOT NULL,
  buyer_id       INT UNSIGNED     NULL,
  sold_at        DATETIME         NULL,
  PRIMARY KEY (id),
  KEY ix_auction_search (item_tid, status, id),
  KEY ix_auction_seller (seller_id),
  CONSTRAINT fk_auction_seller FOREIGN KEY (seller_id) REFERENCES characters (id) ON DELETE CASCADE,
  CONSTRAINT fk_auction_item FOREIGN KEY (item_id) REFERENCES character_items (id) ON DELETE SET NULL,
  CONSTRAINT fk_auction_buyer FOREIGN KEY (buyer_id) REFERENCES characters (id) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- Commutateur d'équipement (AgpmItemSet, type 0x6A) : 100 emplacements par personnage (5 profils × 10 configurations
-- × jeux A et B, numérotés par le client). Une ligne par emplacement rempli ; `items` est la chaîne du paquet,
-- « E,<partie>,<identifiant de character_items> » séparés par « : », 10 objets au plus. L'emplacement porté en dernier
-- est dans characters.item_set_active (-1 : aucun).
CREATE TABLE IF NOT EXISTS character_item_set (
  character_id  INT UNSIGNED     NOT NULL,
  slot          TINYINT UNSIGNED NOT NULL COMMENT '0 à 99',
  items         VARCHAR(3999)    NOT NULL,
  PRIMARY KEY (character_id, slot),
  CONSTRAINT fk_item_set_character FOREIGN KEY (character_id) REFERENCES characters (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

ALTER TABLE characters
    ADD COLUMN IF NOT EXISTS item_set_active SMALLINT NOT NULL DEFAULT -1 COMMENT 'commutateur d''équipement : emplacement porté, -1 aucun';
