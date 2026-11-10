-- Origine des textes localisés : place pour « archcore-traduction », les traductions d'ArchCore des textes de Webzen
-- qu'une langue n'a pas (fichiers <langue>.tsv lus par `archcore import --webzen … --traductions <dossier>`, gardés
-- hors du dépôt). Elles ne remplacent jamais une ligne existante et sont réécrites à chaque import --webzen.
ALTER TABLE localized_text MODIFY COLUMN source VARCHAR(24) NOT NULL DEFAULT 'manual' COMMENT 'import (archcore i18n db), webzen, archcore-traduction (import --traductions) ou manual (saisi autrement)';
