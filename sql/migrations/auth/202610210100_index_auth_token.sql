-- Clé d'entrée sur le monde (account.auth_token) : index unique, NULL permis.
-- L'entrée en jeu cherche le compte par cette clé : sans index, elle parcourait toute la table, et deux
-- comptes pouvaient recevoir la même clé. Les clés en double (restes d'entrées jamais terminées) sont
-- d'abord effacées : elles ne servent qu'entre la connexion et l'entrée sur le monde.

UPDATE account a
JOIN (SELECT auth_token FROM account WHERE auth_token IS NOT NULL GROUP BY auth_token HAVING COUNT(*) > 1) d
  ON a.auth_token = d.auth_token
SET a.auth_token = NULL;

CREATE UNIQUE INDEX IF NOT EXISTS uq_account_auth_token ON account (auth_token);
