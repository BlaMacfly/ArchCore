# Migrations du schéma / Schema migrations

**Français.** Les fichiers `sql/001…004` forment la base de référence : ne plus les modifier. Tout
changement de schéma ou de données de référence va dans une migration :

```
sql/migrations/<base>/<AAAAMMJJHHMM>_<sujet>.sql
```

- `<base>` vaut `auth`, `characters` ou `world` : c'est la base sur laquelle la migration s'applique.
- L'identifiant (le nom du fichier sans `.sql`) fixe l'ordre : date et heure UTC, puis un sujet court en
  minuscules, par exemple `202610211430_index_auth_token.sql`.
- `archcore db-init` applique chaque migration une seule fois, dans l'ordre, et la note dans la table
  `schema_migrations` de sa base (identifiant, date, empreinte SHA-256 du texte).
- Les scripts sont embarqués à la compilation : le serveur refuse de démarrer si une base est en retard
  (lancer `archcore db-init`) ou en avance (mettre à jour l'exécutable).
- Une migration ne doit pas être modifiée une fois publiée : en écrire une nouvelle.
- Additive par défaut (`ADD COLUMN IF NOT EXISTS`, `CREATE TABLE IF NOT EXISTS`…). Un renommage ou une
  suppression se fait en deux versions : ajout et double lecture, puis retrait.
- Pas de migration destructive sans sauvegarde (`mariadb-dump`) préalable.

**English.** `sql/001…004` are the baseline: do not edit them any more. Every schema or reference data
change goes into a migration, `sql/migrations/<base>/<YYYYMMDDHHMM>_<topic>.sql`, where `<base>` is `auth`,
`characters` or `world`. `archcore db-init` applies each migration once, in order, and records it in the
`schema_migrations` table of its database (id, date, SHA-256 of the text). Scripts are embedded at build
time: the server refuses to start when a database is behind (run `archcore db-init`) or ahead (update the
executable). Never edit a published migration: write a new one. Migrations are additive by default; a
rename or a removal takes two versions (add and read both, then remove).
