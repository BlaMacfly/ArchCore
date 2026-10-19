# French translation

ArchCore comes with a full French translation of the game, on both the client and the server side.

## Enable French in the client

The client reads an integer from `Launcher/LanguageInfo.txt`. The value **17** selects the
`Game/ini_fr` folder, which holds 19 text files. The value 5 is English.

Item templates also come from the language folder: the client reads `ini_fr/ItemTemplateAll.ini`,
then `ini_fr/ItemTemplateAll2.ini`, but only if the first one loaded. Without `ItemTemplateAll.ini`,
no item exists on the client side: the inventory and the shops stay empty. This file was missing from
`ini_fr` and was copied from `Game/ini` (item names in English, to be translated). `archcore i18n apply`
does not delete it.

## Tool chain

1. `archcore i18n strings` lists what is still in English in `data/i18n/pending-en.json`.
2. Translations are added to `data/i18n/fr.json`, the translation memory.
3. `archcore i18n apply` rebuilds `Game/ini_fr` from the original copy `ini_fr.orig`.
   It re-encrypts the files with RC4 and encodes them in Windows-1252, which keeps accents.

`archcore i18n apply` options: `--dry` for a trial without writing, `--ascii` to strip accents.
The tool always starts from `ini_fr.orig`, so it can be run again as often as needed.

## `fr.json` format

```json
{
  "files": {
    "UI_1024X768.ini": { "1:Fabriquer": "1:Créer" },
    "uimessage.txt": { "ko:<Korean text>": "<translation>" }
  },
  "global": { "<exact text>": "<translation>" }
}
```

- The key is the exact text of the cell in `ini_fr.orig`: a tab-separated column, or the value
  after `=`.
- The `ko:` prefix targets texts left in Korean (decoded CP949).

## Server side

NPC titles ("<Merchant> Darryl") are translated on load with `data/i18n/npc-titles-fr.json`,
according to `World.Language`: the language of players who chose no other.

## Server languages

Since 2026-10-19 the server serves its texts in the **player's language**: an ArchRust client announces it
through the `language/1` capability of the [[ArchRust extensions|EN-ArchRust-extensions]] (selector in the
Options window), the choice follows the account (`account.language`), and an original client gets the
account's last language, otherwise `World.Language`. NPC and monster names leave localized in view packets;
other texts (quests, items, skills, regions) are fetched with `TEXT_REQUEST`.

### Languages kept

ArchLord never had a single multi-language client; the offered languages (`language` table) come from the
game's history:

| Code | Language | Why |
|---|---|---|
| `ko` | Korean | original version, NHN Korea (open beta 2005, service 2006) |
| `en` | English | Codemasters, North America and Europe (October 2006), then Webzen worldwide (2009-2014); language of the reference client |
| `ja` | Japanese | NHN Japan, Hangame portal, "ARCHLORD ~THE GRAND CROSS~" (open beta 26 July 2007) |
| `zh-CN` | Simplified Chinese | Shanda, "霸王", Chinese version announced in March 2006 |
| `de`, `fr`, `es`, `pt-BR` | German, French, Spanish, Portuguese | multi-language client announced by Webzen in January 2010; Archlord 2 (2014): English, German, French (Webzen Dublin localisation team) |
| `tr`, `ru`, `pl` | Turkish, Russian, Polish | private-server communities (Turkish threads on DonanimHaber, unofficial Russian Archlord 2 client); disabled by default (`enabled = 0`) |

Sources: [Wikipedia, ArchLord](https://en.wikipedia.org/wiki/ArchLord) (dates and publishers),
[Engadget, 14 January 2010](https://www.engadget.com/2010-01-14-archlord-announces-server-and-content-updates-for-2010.html)
(German, French, Spanish, Portuguese client),
[WorthPlaying, October 2009](https://worthplaying.com/article/2009/10/8/news/69161-webzen-expands-its-archlord-to-north-america-europe-adds-new-character-class/)
(Webzen outside Korea, Japan and China), [Webzen, Archlord 2](https://company.webzen.com/en/pr/pressview?page=5&seq=363)
(English, German, French), [Impress Game Watch, July 2007](https://game.watch.impress.co.jp/docs/20070720/on0720.htm)
and [Inside Games](https://www.inside-games.jp/article/2007/07/20/22175.html) (Japan, Hangame),
[17173, March 2006](https://news.17173.com/content/2006-03-24/20060324104258614.shtml) (China, Shanda),
[DonanimHaber](https://forum.donanimhaber.com/archlord-private-server--46358974) (Turkish community).
The Japanese and Chinese versions had their own clients: only `ini` (English) and `ini_fr` (French) exist
in the reference client (`client_folder`).

### Tables

- `language`: BCP 47 code, names, tier (1 official version, 2 Webzen 2010 client, 3 community), client
  text folder, `enabled`, display order. Disabled languages are not offered.
- `localized_text`: one row per (`kind`, `ref_id`, `lang`). `kind`: `npc_name` (`npc_spawn.id`),
  `monster_name` (TID), `item_name`, `item_desc` (TID), `quest_name`, `quest_desc` (TID), `skill_name`
  (TID), `region_name`, `npc_dialog` (reserved).
- `account.language` (auth database): the account's last wish.

Text fallback: requested language → base language (`fr-CA` → `fr`) → translated NPC titles
(`npc-titles-<language>.json`) → English → the client's original text. An `en` row also corrects an
original text for everyone.

### Filling the texts

```bash
archcore db-init                 # creates language and localized_text, adds account.language
archcore i18n db fr              # 555 NPC names from npc-titles-fr.json, plus the data/i18n/fr.json memory
```

`i18n db <language>` replaces that language's rows for the kinds it produces, from the NPC titles
(`npc_name`, `monster_name`) and from the `global` entries of `data/i18n/<language>.json` whose key is
exactly a quest, item, skill or region name. Hand-written translations for other identifiers go straight
into `localized_text`:

```sql
INSERT INTO localized_text (kind, ref_id, lang, text) VALUES ('quest_name', 5007, 'de', 'Erste Schritte')
  ON DUPLICATE KEY UPDATE text = VALUES(text);
```

The server reads the tables at startup. Check without the game: `archcore sim lang` ([[Tools|EN-Tools]]
page).

## Rules

- Keep accents: the game displays them correctly in Windows-1252.
- Never translate chat commands (`/join`, `/party`…) or place names.
- Watch the length: a text longer than the original can overflow its window.
