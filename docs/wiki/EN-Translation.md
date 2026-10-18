# French translation

ArchCore comes with a full French translation of the game, on both the client and the server side.

## Enable French in the client

The client reads an integer from `Launcher/LanguageInfo.txt`. The value **17** selects the
`Game/ini_fr` folder, which holds 19 text files. The value 5 is English.

## Tool chain

1. `node tools/i18n/strings.mjs` lists what is still in English in `data/i18n/pending-en.json`.
2. Translations are added to `data/i18n/fr.json`, the translation memory.
3. `node tools/i18n/apply.mjs` rebuilds `Game/ini_fr` from the original copy `ini_fr.orig`.
   It re-encrypts the files with RC4 and encodes them in Windows-1252, which keeps accents.

`apply.mjs` options: `--dry` for a trial without writing, `--ascii` to strip accents.
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
according to `world.language`.

## Rules

- Keep accents: the game displays them correctly in Windows-1252.
- Never translate chat commands (`/join`, `/party`…) or place names.
- Watch the length: a text longer than the original can overflow its window.
