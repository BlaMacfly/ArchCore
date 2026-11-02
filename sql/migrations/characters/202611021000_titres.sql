-- Titres des personnages (AgpmTitle 0x60, `world/titles.rs`) : titres possédés (au plus un actif), objectifs en cours
-- avec les compteurs de leurs cinq conditions, heure du dernier changement de titre actif (délai
-- `Title.ChangeDelaySeconds`). Heures en secondes Unix, comme les time_t du client. Les lignes partent avec le
-- personnage supprimé.
CREATE TABLE IF NOT EXISTS character_titles (
    character_id INT UNSIGNED     NOT NULL,
    title_tid    INT UNSIGNED     NOT NULL,
    active       TINYINT UNSIGNED NOT NULL DEFAULT 0,
    skill_tid    INT UNSIGNED     NOT NULL DEFAULT 0 COMMENT 'compétence choisie (rangs 4 et 5 d''Archonia : toujours 0)',
    acquired_at  BIGINT           NOT NULL DEFAULT 0,
    PRIMARY KEY (character_id, title_tid),
    CONSTRAINT fk_character_titles_character FOREIGN KEY (character_id) REFERENCES characters (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS character_title_goals (
    character_id INT UNSIGNED NOT NULL,
    title_tid    INT UNSIGNED NOT NULL,
    accepted_at  BIGINT       NOT NULL DEFAULT 0,
    c1           INT          NOT NULL DEFAULT 0,
    c2           INT          NOT NULL DEFAULT 0,
    c3           INT          NOT NULL DEFAULT 0,
    c4           INT          NOT NULL DEFAULT 0,
    c5           INT          NOT NULL DEFAULT 0,
    PRIMARY KEY (character_id, title_tid),
    CONSTRAINT fk_character_title_goals_character FOREIGN KEY (character_id) REFERENCES characters (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

ALTER TABLE characters
    ADD COLUMN IF NOT EXISTS title_changed_at BIGINT NOT NULL DEFAULT 0 COMMENT 'dernier changement du titre actif (secondes Unix)';
