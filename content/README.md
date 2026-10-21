# Couche de contenu / Content layer

**Français.** Ce dossier contient le contenu propre à ArchCore : corrections, apparitions ajoutées, PNJ
créés avec l'éditeur… Il est versionné et publiable, contrairement au monde importé, régénéré chez chacun
depuis les fichiers du client par `archcore import`.

- `archcore import` régénère les tables du monde puis applique ces fichiers ; `archcore content` les
  réapplique seuls, sans réimporter.
- Un fichier par sujet, nommé `<AAAAMMJJHHMM>_<sujet>.sql` : ils passent dans l'ordre de leur nom, chacun
  dans une transaction (tout ou rien), sur la base monde.
- Chaque fichier doit pouvoir être rejoué : `INSERT … ON DUPLICATE KEY UPDATE`, ou `UPDATE` et `DELETE`
  ciblés par TID ou par clé stable (`stable_key` de `monster_spawn`, `monster_drop`, `resurrection_point`).
- Données seulement : pas de `CREATE`, `ALTER` ni `DROP` (ils valideraient la transaction en cours ; le
  schéma passe par `sql/migrations/`).
- Aucune donnée copiée des fichiers du client ni des tables du serveur d'origine : seulement des valeurs
  créées pour ArchCore.
- Chaque passage est noté dans la table `import_run` (source « contenu », empreinte des fichiers).

**English.** This folder holds ArchCore's own content: fixes, added spawns, NPCs made with the editor…
It is versioned and publishable, unlike the imported world, which everyone rebuilds from the client files
with `archcore import`. `archcore import` rebuilds the world tables then applies these files; `archcore
content` applies them again on their own. One file per topic, named `<YYYYMMDDHHMM>_<topic>.sql`, applied in
name order, each in a transaction, on the world database. Every file must be replayable (`INSERT … ON
DUPLICATE KEY UPDATE`, or `UPDATE` and `DELETE` targeted by TID or stable key); data only, no schema
statement; no data copied from the client files or the original server tables. Each run is recorded in the
`import_run` table.
