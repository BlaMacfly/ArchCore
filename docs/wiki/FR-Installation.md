# Installation

## Prérequis

- **Node.js 20 ou plus** (testé avec la version 24).
- **MariaDB** 10.6 ou plus (testé avec 13.0). Sous Windows : `winget install MariaDB.Server`.
- Le **client Archonia** extrait, par défaut dans `../client/ArchLord/Game` par rapport au dossier ArchCore.
  ArchCore ne fournit aucune donnée du jeu.

## 1. Récupérer le code

```bash
git clone https://github.com/BlaMacfly/ArchCore.git
cd ArchCore
npm install
```

## 2. Démarrer MariaDB

Le script `db/start-mariadb.cmd` lance une instance locale dont les données sont dans `db/data`
(port 3306, utilisateur `root`, mot de passe `archcore`). Un serveur MariaDB déjà installé convient
aussi : il suffit de renseigner ses accès dans `config/archcore.json` (voir [[Configuration|FR-Configuration]]).

## 3. Créer les bases et un compte

```bash
node tools/db-init.mjs --account test test
```

Cette commande crée `archcore_auth`, `archcore_characters` et `archcore_world`. Elle applique les
scripts de `sql/`, puis crée le compte `test` avec le mot de passe `test`. Un niveau de maître du jeu
peut être donné en quatrième argument.

## 4. Importer les données du client

```bash
node tools/decrypt-client-data.mjs     # Game/ini → data/client-ini (en clair)
node tools/import-client-data.mjs      # data/client-ini → archcore_world
```

Les tables du client sont chiffrées en RC4, avec une clé dérivée de MD5(« 1111 »), et écrites en CP949.
L'import les convertit en UTF-8 : modèles de personnages, expérience, PNJ, objets, compétences.

## 5. Lancer le serveur

```bash
npm start                    # login (11002) + monde (11008)
npm run login                # login seul
npm run world                # monde seul
ARCHCORE_TRACE=1 npm start   # journal détaillé de chaque paquet
```

## 6. Vérifier sans le jeu

```bash
npm run sim -- test test Testeur
```

Le simulateur rejoue la poignée de main, la connexion, la liste des personnages et l'entrée dans le monde.
Il doit finir par « entrée en jeu confirmée — flux complet OK ».

## 7. Connecter le vrai client

Créer `Game/ArchoniaTest.ini` en texte clair. Le client le lit avant `Archonia.ini`, qui est chiffré :

```xml
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<IPInfo>
  <ServerGroup><GroupName>ArchCore</GroupName><IP>127.0.0.1:11002</IP></ServerGroup>
</IPInfo>
```

Le client se lance avec les droits administrateur. Il faut donc saisir l'identifiant dans la
fenêtre du jeu : un autre programme ne peut pas lui envoyer de touches.

Pour jouer en français, écrire `17` dans `Launcher/LanguageInfo.txt` (voir [[Traduction|FR-Traduction]]).

## Dépannage

| Symptôme | Piste |
|---|---|
| « Cannot access the login server » | ArchCore n'écoute pas sur 11002, ou `ArchoniaTest.ini` est absent |
| Chargement infini à l'entrée en jeu | Lancer avec `ARCHCORE_TRACE=1` et lire `data/archcore.log` |
| Pas de son | Vérifier `[Sound_Option]` dans `Game/ini/COption.ini` ; le jeu réécrit ce fichier en quittant |
