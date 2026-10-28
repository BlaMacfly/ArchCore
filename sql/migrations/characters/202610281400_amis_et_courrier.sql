-- Liste d'amis et d'ignorés (AgpmBuddy, type 0x2B) : une ligne par entrée, l'autre personnage par son identifiant
-- (son nom actuel est relu à l'entrée en jeu). `flags` garde les drapeaux du paquet : 0x02000000 pour un ami, sinon
-- ignoré, avec ses blocages 0x1 discussion, 0x2 échange, 0x4 invitations ; l'état en ligne (0x00100000) n'est
-- jamais enregistré. Un même personnage n'est que dans une des deux listes.
CREATE TABLE IF NOT EXISTS character_buddy (
  character_id  INT UNSIGNED      NOT NULL,
  other_id      INT UNSIGNED      NOT NULL,
  flags         INT UNSIGNED      NOT NULL DEFAULT 0,
  relation      SMALLINT UNSIGNED NOT NULL DEFAULT 0 COMMENT 'relation « Buddy » (mentor), non utilisée',
  created_at    TIMESTAMP         NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (character_id, other_id),
  KEY ix_buddy_other (other_id),
  CONSTRAINT fk_buddy_character FOREIGN KEY (character_id) REFERENCES characters (id) ON DELETE CASCADE,
  CONSTRAINT fk_buddy_other FOREIGN KEY (other_id) REFERENCES characters (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- Courrier entre joueurs (AgpmMailBox, type 0x2D). L'identifiant est celui du paquet. La pièce jointe reste une
-- ligne de character_items (options, conversion conservées) au statut 4, rattachée au destinataire, jusqu'à sa
-- récupération ; `item_id` repasse alors à NULL et `flags` prend 0x10. Pas d'expiration.
CREATE TABLE IF NOT EXISTS mail (
  id            INT UNSIGNED     NOT NULL AUTO_INCREMENT,
  recipient_id  INT UNSIGNED     NOT NULL,
  sender_id     INT UNSIGNED     NULL COMMENT 'NULL : personnage supprimé',
  sender_name   VARCHAR(32)      NOT NULL,
  subject       VARCHAR(50)      NOT NULL,
  body          VARCHAR(999)     NOT NULL COMMENT 'sans le zéro final du paquet',
  sent_at       INT UNSIGNED     NOT NULL COMMENT 'secondes Unix',
  flags         INT UNSIGNED     NOT NULL DEFAULT 0 COMMENT '0x01 lu, 0x10 pièce récupérée',
  item_id       BIGINT UNSIGNED  NULL COMMENT 'pièce jointe (character_items, statut 4)',
  PRIMARY KEY (id),
  KEY ix_mail_recipient (recipient_id, id),
  CONSTRAINT fk_mail_recipient FOREIGN KEY (recipient_id) REFERENCES characters (id) ON DELETE CASCADE,
  CONSTRAINT fk_mail_sender FOREIGN KEY (sender_id) REFERENCES characters (id) ON DELETE SET NULL,
  CONSTRAINT fk_mail_item FOREIGN KEY (item_id) REFERENCES character_items (id) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
