# Tables serveur de l'épisode 8 / Episode 8 server tables

## Français

Ce dossier contient les tables serveur de l'épisode 8 utilisées par ArchCore : valeurs des objets,
compétences, personnages, PNJ, boutiques, apparitions, butin, objets du monde et carte serveur.

- Les tables `.ini` et `.txt` sont à la racine ; `objects/` contient les objets du monde.
- `world/node_data` contient les segments de la carte serveur.
- Les fichiers de données conservent leurs octets et leur encodage d'origine.

Après avoir suivi le [guide d'installation](https://github.com/BlaMacfly/ArchCore/wiki/FR-Installation),
lancer depuis la racine du dépôt :

```powershell
.\target\release\archcore.exe import
```

L'import trouve ce dossier automatiquement. Il faut aussi les données de son propre client ArchLord
64 bits, déchiffrées par `archcore decrypt` dans `data/client-ini`. Aucun dump SQL ni téléchargement
séparé des tables serveur n'est nécessaire. `--server <dossier>` permet de choisir un autre lot de tables.

## English

This folder contains the Episode 8 server tables used by ArchCore: item, skill, character, NPC, shop,
spawn, drop and world-object data, along with the server map.

- The `.ini` and `.txt` tables are at the root; `objects/` holds world objects.
- `world/node_data` holds the server map segments.
- Data files retain their original bytes and encoding.

After following the [installation guide](https://github.com/BlaMacfly/ArchCore/wiki/EN-Installation),
run this from the repository root:

```powershell
.\target\release\archcore.exe import
```

The import finds this folder automatically. You also need your own 64-bit ArchLord client's data,
decrypted by `archcore decrypt` into `data/client-ini`. No SQL dump or separate server-table download
is needed. Use `--server <folder>` to select another set of tables.
