# Outils

Tous les outils sont dans `tools/` et se lancent avec Node.js depuis le dossier ArchCore.

| Outil | Rôle |
|---|---|
| `db-init.mjs` | Crée les bases et applique `sql/`. Avec `--account nom mdp [mj]`, crée un compte |
| `decrypt-client-data.mjs` | Déchiffre `Game/ini` (RC4, MD5(« 1111 »)) vers `data/client-ini` |
| `import-client-data.mjs` | Importe `data/client-ini` dans `archcore_world` (CP949 → UTF-8) |
| `client-sim.mjs` | Client de test : rejoue la connexion complète jusqu'à l'entrée en jeu |
| `i18n/strings.mjs` | Liste les textes encore en anglais dans `data/i18n/pending-en.json` |
| `i18n/coverage.mjs` | Mesure la part déjà traduite de `ini_fr` |
| `i18n/apply.mjs` | Applique la mémoire de traduction au client (voir [[Traduction|FR-Traduction]]) |
| `re/extract-schemas.mjs` | Extrait les schémas de paquets des pseudo-codes Ghidra vers `data/re/packet-schemas.json` |

## Simulateur de client

```bash
npm run sim -- <utilisateur> <mot_de_passe> [personnage]
```

Il affiche chaque paquet échangé, le nombre de PNJ reçus, et termine par « flux complet OK ».
C'est la manière la plus rapide de vérifier une modification du serveur sans lancer le jeu.

## Rétro-ingénierie

Les relevés ont été faits avec [REA](https://github.com/morluto/rea) et Ghidra 12. La démarche est
décrite sur la page [[Rétro-ingénierie|FR-Retro-ingenierie]].
