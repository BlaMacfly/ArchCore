-- ArchCore — base monde (archcore_world) : langues et textes localisés servis aux clients.
--
-- La langue d'un joueur vient du client ArchRust (capacité « language/1 » des extensions, page
-- « Extensions ArchRust » du wiki), sinon de son compte (dernier choix), sinon de World.Language.
-- Repli d'un texte : langue demandée → langue de base (fr-CA → fr) → anglais → texte d'origine du client.

-- Langues connues du serveur. Codes BCP 47 (ISO 639-1, région facultative). Sources : page
-- « Traduction » du wiki (versions officielles d'ArchLord et communautés).
CREATE TABLE IF NOT EXISTS language (
  code          VARCHAR(16) NOT NULL COMMENT 'BCP 47 : fr, de, zh-CN…',
  name_en       VARCHAR(48) NOT NULL COMMENT 'nom en anglais',
  native_name   VARCHAR(48) NOT NULL COMMENT 'nom dans la langue',
  tier          TINYINT UNSIGNED NOT NULL DEFAULT 3 COMMENT '1 version officielle d''origine, 2 client multilingue Webzen 2010, 3 communauté',
  client_folder VARCHAR(16) NULL COMMENT 'dossier de textes du client (Game/ini, Game/ini_fr…), NULL si aucun',
  enabled       TINYINT(1) NOT NULL DEFAULT 1 COMMENT '0 : la langue n''est pas proposée aux clients',
  sort_order    TINYINT UNSIGNED NOT NULL DEFAULT 100,
  PRIMARY KEY (code)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='langues proposées aux clients';

-- Textes localisés : une ligne par (sorte, identifiant, langue). `kind` reprend les sortes de
-- ext_text_kind (archcore-net) ; `ref_id` est l'identifiant de la table d'origine :
--   npc_name      npc_spawn.id        quest_name / quest_desc   quest_template.tid
--   monster_name  monster_spawn.tid   skill_name                skill_template.tid
--   item_name / item_desc  item_data.tid   region_name   region_template.id
--   npc_dialog    identifiant de dialogue du client (réservé)
-- Remplie par `archcore i18n db <langue>` (titres de PNJ, mémoire de traduction) et à la main.
CREATE TABLE IF NOT EXISTS localized_text (
  kind    VARCHAR(24)  NOT NULL,
  ref_id  INT UNSIGNED NOT NULL,
  lang    VARCHAR(16)  NOT NULL COMMENT 'code de la table language',
  text    VARCHAR(2048) NOT NULL,
  PRIMARY KEY (kind, ref_id, lang),
  KEY ix_lt_lang (lang, kind)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='textes des quêtes, PNJ, objets… par langue';

-- Niveau 1 : versions officielles d'ArchLord (NHN Corée 2005 ; Codemasters Amérique du Nord et Europe
-- 2006 ; NHN Japan / Hangame 2007 ; Shanda Chine 2006). Niveau 2 : client multilingue annoncé par Webzen
-- en janvier 2010 (allemand, français, espagnol, portugais) ; Archlord 2 (2014) : anglais, allemand,
-- français. Niveau 3 : communautés de serveurs privés (turc, russe, polonais), désactivées par défaut.
INSERT IGNORE INTO language (code, name_en, native_name, tier, client_folder, enabled, sort_order) VALUES
  ('en',    'English',               'English',          1, 'ini',    1, 10),
  ('fr',    'French',                'Français',         2, 'ini_fr', 1, 20),
  ('de',    'German',                'Deutsch',          2, NULL,     1, 30),
  ('es',    'Spanish',               'Español',          2, NULL,     1, 40),
  ('pt-BR', 'Portuguese (Brazil)',   'Português',        2, NULL,     1, 50),
  ('ko',    'Korean',                '한국어',            1, NULL,     1, 60),
  ('ja',    'Japanese',              '日本語',            1, NULL,     1, 70),
  ('zh-CN', 'Chinese (Simplified)',  '简体中文',          1, NULL,     1, 80),
  ('tr',    'Turkish',               'Türkçe',           3, NULL,     0, 90),
  ('ru',    'Russian',               'Русский',          3, NULL,     0, 91),
  ('pl',    'Polish',                'Polski',           3, NULL,     0, 92);
