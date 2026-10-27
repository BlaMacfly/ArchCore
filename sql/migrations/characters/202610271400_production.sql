-- Production : expérience cumulée d'une compétence de production (facteur « dirt » 6 d'AGPMSKILL ; 0 pour les
-- autres compétences) et recettes de cuisine et d'alchimie achetées aux PNJ (TID de l'objet produit,
-- `product_compose.result_tid`), envoyées à l'entrée en jeu (AgpmProduct op 1).

ALTER TABLE character_skills
    ADD COLUMN IF NOT EXISTS exp INT UNSIGNED NOT NULL DEFAULT 0;

CREATE TABLE IF NOT EXISTS character_recipes (
  character_id  INT UNSIGNED NOT NULL,
  recipe_tid    INT UNSIGNED NOT NULL,
  learned_at    TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (character_id, recipe_tid),
  CONSTRAINT fk_recipe_character FOREIGN KEY (character_id) REFERENCES characters (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
