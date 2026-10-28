-- Avis de recherche (AgpmWantedCriminal, type 0x46) : un avis par criminel, dont la prime cumule toutes les mises à prix.
-- La victime d'un meurtre (tuée par un joueur qui l'a frappée le premier alors qu'elle était innocente, ou par un
-- meurtrier) peut mettre son tueur à prix depuis la fenêtre de mort ; la prime est débitée de son argent porté, puis
-- de sa banque. L'avis échoit `expires_at` (dernière mise à prix + `Wanted.ExpireDays`). Le joueur qui tue un criminel
-- recherché (hors de son groupe et de sa guilde) touche la prime cumulée : l'avis est alors effacé.
CREATE TABLE IF NOT EXISTS wanted_criminal (
  criminal_id  INT UNSIGNED    NOT NULL,
  bounty       BIGINT UNSIGNED NOT NULL DEFAULT 0 COMMENT 'prime cumulée',
  created_at   DATETIME        NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at   DATETIME        NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT 'dernière mise à prix',
  expires_at   DATETIME        NOT NULL,
  PRIMARY KEY (criminal_id),
  KEY ix_wanted_bounty (bounty),
  CONSTRAINT fk_wanted_criminal FOREIGN KEY (criminal_id) REFERENCES characters (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- Historique des mises à prix : qui a posé quel montant, et qui a touché la prime (NULL tant qu'elle court ou si
-- l'avis est échu).
CREATE TABLE IF NOT EXISTS wanted_bounty (
  id           BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
  criminal_id  INT UNSIGNED    NOT NULL,
  placed_by    INT UNSIGNED    NULL,
  amount       BIGINT UNSIGNED NOT NULL,
  placed_at    DATETIME        NOT NULL DEFAULT CURRENT_TIMESTAMP,
  claimed_by   INT UNSIGNED    NULL,
  claimed_at   DATETIME        NULL,
  PRIMARY KEY (id),
  KEY ix_wanted_bounty_criminal (criminal_id),
  CONSTRAINT fk_wanted_bounty_criminal FOREIGN KEY (criminal_id) REFERENCES characters (id) ON DELETE CASCADE,
  CONSTRAINT fk_wanted_bounty_placed FOREIGN KEY (placed_by) REFERENCES characters (id) ON DELETE SET NULL,
  CONSTRAINT fk_wanted_bounty_claimed FOREIGN KEY (claimed_by) REFERENCES characters (id) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
