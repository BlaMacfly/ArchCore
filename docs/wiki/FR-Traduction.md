# Traduction française

ArchCore accompagne une traduction complète du jeu en français, côté client et côté serveur.

## Activer le français dans le client

Le client lit un entier dans `Launcher/LanguageInfo.txt`. La valeur **17** sélectionne le dossier
`Game/ini_fr`, qui contient 19 fichiers de textes. La valeur 5 correspond à l'anglais.

Les modèles d'objets viennent eux aussi du dossier de la langue : le client lit
`ini_fr/ItemTemplateAll.ini`, puis `ini_fr/ItemTemplateAll2.ini`, mais seulement si le premier a été
chargé. Sans `ItemTemplateAll.ini`, aucun objet n'existe côté client : l'inventaire et les boutiques
restent vides. Ce fichier manquait dans `ini_fr`, il a été copié depuis `Game/ini` (noms des objets
en anglais, à traduire). `archcore i18n apply` ne le supprime pas.

## Chaîne d'outils

1. `archcore i18n strings` liste ce qui reste en anglais dans `data/i18n/pending-en.json`.
2. Les traductions sont ajoutées à `data/i18n/fr.json`, la mémoire de traduction.
3. `archcore i18n apply` reconstruit `Game/ini_fr` à partir de la copie d'origine `ini_fr.orig`.
   Il rechiffre les fichiers en RC4 et les encode en Windows-1252, ce qui garde les accents.

Options de `archcore i18n apply` : `--dry` pour un essai sans écriture, `--ascii` pour retirer les accents.
L'outil repart toujours de `ini_fr.orig`, on peut donc le relancer autant de fois que nécessaire.

## Format de `fr.json`

```json
{
  "files": {
    "UI_1024X768.ini": { "1:Fabriquer": "1:Créer" },
    "uimessage.txt": { "ko:<texte coréen>": "<traduction>" }
  },
  "global": { "<texte exact>": "<traduction>" }
}
```

- La clé est le texte exact de la cellule dans `ini_fr.orig` : une colonne séparée par des tabulations,
  ou la valeur après `=`.
- Le préfixe `ko:` cible les textes restés en coréen (CP949 décodé).

## Côté serveur

Les titres des PNJ (« <Marchand> Darryl ») sont traduits au chargement avec
`data/i18n/npc-titles-fr.json`, selon `World.Language` : c'est la langue des joueurs qui n'en ont pas
choisi d'autre.

## Langues du serveur

Depuis le 2026-10-19, le serveur sert ses textes dans la **langue du joueur** : un client ArchRust l'annonce
par la capacité `language/1` des [[Extensions ArchRust|FR-Extensions-ArchRust]] (sélecteur dans la fenêtre
Options), le choix suit le compte (`account.language`), et un client d'origine reçoit la dernière langue du
compte, sinon `World.Language`. Les noms des PNJ et des monstres partent localisés dans les paquets de vue ;
les autres textes (quêtes, objets, compétences, régions) se demandent par `TEXT_REQUEST`.

### Langues retenues

ArchLord n'a jamais eu de client multilingue unique ; les langues proposées (table `language`) viennent de
l'histoire du jeu :

| Code | Langue | Pourquoi |
|---|---|---|
| `ko` | coréen | version d'origine, NHN Corée (bêta ouverte 2005, service 2006) |
| `en` | anglais | Codemasters, Amérique du Nord et Europe (octobre 2006), puis Webzen monde (2009-2014) ; langue du client de référence |
| `ja` | japonais | NHN Japan, portail Hangame, « ARCHLORD ~THE GRAND CROSS~ » (bêta ouverte 26 juillet 2007) |
| `zh-CN` | chinois simplifié | Shanda, « 霸王 », version chinoise annoncée en mars 2006 |
| `de`, `fr`, `es`, `pt-BR` | allemand, français, espagnol, portugais | client multilingue annoncé par Webzen en janvier 2010 ; Archlord 2 (2014) : anglais, allemand, français (équipe de localisation Webzen Dublin) |
| `tr`, `ru`, `pl` | turc, russe, polonais | communautés de serveurs privés (fils turcs sur DonanimHaber, client russe non officiel d'Archlord 2) ; désactivées par défaut (`enabled = 0`) |

Sources : [Wikipédia, ArchLord](https://en.wikipedia.org/wiki/ArchLord) (dates et éditeurs),
[Engadget, 14 janvier 2010](https://www.engadget.com/2010-01-14-archlord-announces-server-and-content-updates-for-2010.html)
(client multilingue allemand, français, espagnol, portugais),
[WorthPlaying, octobre 2009](https://worthplaying.com/article/2009/10/8/news/69161-webzen-expands-its-archlord-to-north-america-europe-adds-new-character-class/)
(Webzen hors Corée, Japon et Chine), [Webzen, Archlord 2](https://company.webzen.com/en/pr/pressview?page=5&seq=363)
(anglais, allemand, français), [Impress Game Watch, juillet 2007](https://game.watch.impress.co.jp/docs/20070720/on0720.htm)
et [Inside Games](https://www.inside-games.jp/article/2007/07/20/22175.html) (Japon, Hangame),
[17173, mars 2006](https://news.17173.com/content/2006-03-24/20060324104258614.shtml) (Chine, Shanda),
[DonanimHaber](https://forum.donanimhaber.com/archlord-private-server--46358974) (communauté turque).
Les versions japonaise et chinoise avaient leurs propres clients : seuls `ini` (anglais) et `ini_fr`
(français) existent dans le client de référence (`client_folder`).

### Tables

- `language` : code BCP 47, noms, niveau (1 version officielle, 2 client Webzen 2010, 3 communauté),
  dossier de textes du client, `enabled`, ordre d'affichage. Les langues désactivées ne sont pas proposées.
- `localized_text` : une ligne par (`kind`, `ref_id`, `lang`). `kind` : `npc_name` (`npc_spawn.id`),
  `monster_name` (TID), `item_name`, `item_desc` (TID), `quest_name`, `quest_desc` (TID), `skill_name`
  (TID), `region_name`, `npc_dialog` (réservé).
- `account.language` (base d'authentification) : dernier souhait du compte.

Repli d'un texte : langue demandée → langue de base (`fr-CA` → `fr`) → titres de PNJ traduits
(`npc-titles-<langue>.json`) → anglais → texte d'origine du client. Une ligne `en` sert aussi à corriger un
texte d'origine pour tout le monde.

### Remplir les textes

```bash
archcore db-init                 # crée language et localized_text, ajoute account.language
archcore i18n db fr              # 555 noms de PNJ depuis npc-titles-fr.json, et la mémoire data/i18n/fr.json
```

`i18n db <langue>` remplace les lignes de cette langue pour les sortes qu'il produit, à partir des titres de
PNJ (`npc_name`, `monster_name`) et des entrées `global` de `data/i18n/<langue>.json` dont la clé est
exactement un nom de quête, d'objet, de compétence ou de région. Les traductions saisies à la main pour
d'autres identifiants se font directement dans `localized_text` :

```sql
INSERT INTO localized_text (kind, ref_id, lang, text) VALUES ('quest_name', 5007, 'de', 'Erste Schritte')
  ON DUPLICATE KEY UPDATE text = VALUES(text);
```

Le serveur relit les tables au démarrage. Vérification sans le jeu : `archcore sim lang` (page
[[Outils|FR-Outils]]).

## Règles

- Garder les accents : le jeu les affiche correctement en Windows-1252.
- Ne jamais traduire les commandes de discussion (`/join`, `/party`…) ni les noms de lieux.
- Vérifier la longueur : un texte plus long que l'original peut déborder de sa fenêtre.
