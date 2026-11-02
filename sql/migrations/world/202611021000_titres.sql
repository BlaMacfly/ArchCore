-- Titres de l'épisode 8 (AgpmTitle 0x60, `world/titles.rs`) : titledatatable.txt du client (90 colonnes), rempli par
-- `archcore import`, avec la catégorie de la liste de gauche de titlestringtable.txt (FirstCategory : 200 Expert
-- Collector, 500 Addict, 1500 Advanced, 2100 Researcher). Les ajouts d'Archonia (Christmas Event, Schola de rangs 4 et 5)
-- ne sont pas importés ; Defender of Chantra et Schola de rangs 1 à 3 le sont avec `optional` = 1 (`Title.Schola`).
-- Listes en JSON : coûts [[TID, quantité], …], titres préalables [TID, …], conditions [[type, Set, Value], …],
-- compétences [TID, …], effets [[type, Set, Value_1, Value_2], …] ; cases vides ou nulles omises.
CREATE TABLE IF NOT EXISTS title_template (
    tid                  INT UNSIGNED NOT NULL,
    category             VARCHAR(64)  NOT NULL DEFAULT '' COMMENT 'Title_Class',
    category_id          INT          NOT NULL DEFAULT 0 COMMENT 'FirstCategory de titlestringtable.txt',
    name                 VARCHAR(128) NOT NULL DEFAULT '',
    optional             TINYINT UNSIGNED NOT NULL DEFAULT 0 COMMENT '1 : Defender of Chantra et Schola 1-3 (Title.Schola)',
    require_level        INT          NOT NULL DEFAULT 0,
    require_class        INT          NOT NULL DEFAULT 0 COMMENT '0 tous ; 101 chevalier … 1001 Dragon Scion, 1101-1301 formes évoluées',
    require_guild_master TINYINT UNSIGNED NOT NULL DEFAULT 0,
    require_archlord     TINYINT UNSIGNED NOT NULL DEFAULT 0,
    require_con          INT          NOT NULL DEFAULT 0,
    require_str          INT          NOT NULL DEFAULT 0,
    require_int          INT          NOT NULL DEFAULT 0,
    require_dex          INT          NOT NULL DEFAULT 0,
    require_wis          INT          NOT NULL DEFAULT 0,
    require_cha          INT          NOT NULL DEFAULT 0,
    cost_geld            BIGINT       NOT NULL DEFAULT 0,
    cost_cash            BIGINT       NOT NULL DEFAULT 0 COMMENT 'vide dans les données, ignoré',
    cost_items           LONGTEXT     NOT NULL DEFAULT '[]',
    first_titles         LONGTEXT     NOT NULL DEFAULT '[]',
    checks               LONGTEXT     NOT NULL DEFAULT '[]',
    skills               LONGTEXT     NOT NULL DEFAULT '[]',
    effects              LONGTEXT     NOT NULL DEFAULT '[]',
    PRIMARY KEY (tid)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- Rangs de charisme (charismatitletable.txt, 13 rangs) : le client les calcule lui-même d'après le CHA envoyé ; le
-- serveur ne s'en sert que pour la commande MJ /title.
CREATE TABLE IF NOT EXISTS charisma_title (
    `rank`       TINYINT UNSIGNED NOT NULL,
    charisma_min INT UNSIGNED     NOT NULL,
    charisma_max INT UNSIGNED     NOT NULL,
    title        VARCHAR(32)      NOT NULL DEFAULT '',
    PRIMARY KEY (`rank`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
