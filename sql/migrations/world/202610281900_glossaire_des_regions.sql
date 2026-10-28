-- Glossaire des régions (RegionGlossary.txt de chaque dossier de langue du client), rempli par `archcore import`.
--
-- Le client cherche le taux de taxe de sa région (AgpmTax op 4) par le nom de RegionTemplate.ini traduit par le glossaire
-- de sa langue : le serveur envoie donc ce même nom. `korean_name` : nom de la région dans RegionTemplate.ini
-- (`region_template.name`), `lang` : code de la table `language`.
CREATE TABLE IF NOT EXISTS region_glossary (
  lang         VARCHAR(16)  NOT NULL,
  korean_name  VARCHAR(128) NOT NULL,
  name         VARCHAR(128) NOT NULL,
  PRIMARY KEY (lang, korean_name)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
