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

## Test à deux joueurs

```bash
node tools/sim-move.mjs [compteA] [mdpA] [persoA] [compteB] [mdpB] [persoB]
```

Deux clients simulés entrent en jeu (comptes `sima` et `simb` par défaut, personnages créés au
besoin). Le test vérifie qu'ils se voient, que le déplacement de l'un est relayé à l'autre, qu'ils
se perdent de vue hors de portée, et que la déconnexion retire le personnage. Il déplace les
personnages et sauvegarde leur position : à lancer contre un serveur de test, par exemple :

```bash
ARCHCORE_CONFIG=config/test.json node src/index.js all      # autres ports, autres bases
ARCHCORE_CONFIG=config/test.json node tools/sim-move.mjs
```

## Test de boutique

```bash
ARCHCORE_CONFIG=config/test.json npm run sim:shop -- [compte] [mdp] [personnage]
```

Le personnage (`sima` / `SimA` par défaut) reçoit 1000 pièces d'or et apparaît près de Darryl
(Anchorville). Le test ouvre sa boutique, vérifie la liste reçue, achète 3 petites potions de vie
puis les revend. Il modifie l'or, la position et l'inventaire du personnage : à lancer lui aussi
contre un serveur de test.

## Rétro-ingénierie

Les relevés ont été faits avec [REA](https://github.com/morluto/rea) et Ghidra 12. La démarche est
décrite sur la page [[Rétro-ingénierie|FR-Retro-ingenierie]].
