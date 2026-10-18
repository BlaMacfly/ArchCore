# Traduction française

ArchCore accompagne une traduction complète du jeu en français, côté client et côté serveur.

## Activer le français dans le client

Le client lit un entier dans `Launcher/LanguageInfo.txt`. La valeur **17** sélectionne le dossier
`Game/ini_fr`, qui contient 19 fichiers de textes. La valeur 5 correspond à l'anglais.

Les modèles d'objets viennent eux aussi du dossier de la langue : le client lit
`ini_fr/ItemTemplateAll.ini`, puis `ini_fr/ItemTemplateAll2.ini`, mais seulement si le premier a été
chargé. Sans `ItemTemplateAll.ini`, aucun objet n'existe côté client : l'inventaire et les boutiques
restent vides. Ce fichier manquait dans `ini_fr`, il a été copié depuis `Game/ini` (noms des objets
en anglais, à traduire). `apply.mjs` ne le supprime pas.

## Chaîne d'outils

1. `node tools/i18n/strings.mjs` liste ce qui reste en anglais dans `data/i18n/pending-en.json`.
2. Les traductions sont ajoutées à `data/i18n/fr.json`, la mémoire de traduction.
3. `node tools/i18n/apply.mjs` reconstruit `Game/ini_fr` à partir de la copie d'origine `ini_fr.orig`.
   Il rechiffre les fichiers en RC4 et les encode en Windows-1252, ce qui garde les accents.

Options de `apply.mjs` : `--dry` pour un essai sans écriture, `--ascii` pour retirer les accents.
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
`data/i18n/npc-titles-fr.json`, selon `world.language`.

## Règles

- Garder les accents : le jeu les affiche correctement en Windows-1252.
- Ne jamais traduire les commandes de discussion (`/join`, `/party`…) ni les noms de lieux.
- Vérifier la longueur : un texte plus long que l'original peut déborder de sa fenêtre.
