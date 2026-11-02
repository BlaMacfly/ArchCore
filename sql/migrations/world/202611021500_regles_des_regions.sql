-- Règles des régions (RegionTemplate.ini du client, mêmes clés que la table d'origine), remplies par `archcore import` :
--   disabled        bits : 1 DisableSummon (rappel de groupe), 2 DisableGo (`/go`), 4 DisableParty (groupes),
--                   8 DisableLogin (pas d'entrée en jeu dans la région) ;
--   item_section    ItemSection : les objets du même « Item_section_num » (itemdatatable.txt) n'y comptent pas
--                   (1 : Heaven's Hell, régions 180 à 183) ; 0 : aucune ;
--   resurrection_x, resurrection_z
--                   ResurrectionX, ResurrectionZ : point de résurrection propre à la région (0 : aucun).
ALTER TABLE region_template
    ADD COLUMN IF NOT EXISTS disabled INT UNSIGNED NOT NULL DEFAULT 0 COMMENT 'bits : 1 DisableSummon, 2 DisableGo, 4 DisableParty, 8 DisableLogin' AFTER level_limit,
    ADD COLUMN IF NOT EXISTS item_section INT NOT NULL DEFAULT 0 COMMENT 'ItemSection : Item_section_num des objets qui n''y comptent pas, 0 : aucun' AFTER disabled,
    ADD COLUMN IF NOT EXISTS resurrection_x DOUBLE NOT NULL DEFAULT 0 COMMENT 'ResurrectionX, 0 : aucun point' AFTER item_section,
    ADD COLUMN IF NOT EXISTS resurrection_z DOUBLE NOT NULL DEFAULT 0 COMMENT 'ResurrectionZ' AFTER resurrection_x;
