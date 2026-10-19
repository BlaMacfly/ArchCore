# Installation

## Prérequis

- **Rust 1.85 ou plus**, installé avec [rustup](https://rustup.rs) (testé avec 1.99).
- **MariaDB** 10.6 ou plus (testé avec 13.0). Sous Windows : `winget install MariaDB.Server`.
- Un **client ArchLord 64 bits** extrait, par défaut dans `../client/ArchLord/Game` par rapport au dossier ArchCore.
  ArchCore ne fournit aucune donnée du jeu.

## 1. Récupérer et compiler le code

```bash
git clone https://github.com/BlaMacfly/ArchCore.git
cd ArchCore
cargo build --release
```

La compilation produit un seul exécutable, `target/release/archcore` (`archcore.exe` sous Windows).
Il contient le serveur et tous les outils. Dans la suite, `archcore` désigne cet exécutable ; on peut
aussi écrire `cargo run --release --` à sa place. Les commandes se lancent depuis la racine du dépôt,
là où se trouve `config/`.

## 2. Démarrer MariaDB

Le script `db/start-mariadb.cmd` lance une instance locale dont les données sont dans `db/data`
(port 3306, utilisateur `root`, mot de passe `archcore`). Un serveur MariaDB déjà installé convient
aussi : il suffit de renseigner ses accès (`Database.*`) dans `config/auth.conf` et `config/world.conf`, créés
au premier lancement à partir de leurs modèles `.dist` (voir [[Configuration|FR-Configuration]]).

## 3. Créer les bases et un compte

```bash
archcore db-init --account test test
```

Cette commande crée `archcore_auth`, `archcore_characters` et `archcore_world`. Elle applique les
scripts de `sql/`, puis crée le compte `test` avec le mot de passe `test`. Un niveau de maître du jeu
peut être donné en quatrième argument.

## 4. Importer les données du client

```bash
archcore decrypt     # Game/ini → data/client-ini (en clair)
archcore import      # data/client-ini → archcore_world
```

Les tables du client sont chiffrées en RC4, avec une clé dérivée de MD5(« 1111 »), et écrites en CP949.
L'import les convertit en UTF-8 : modèles de personnages, expérience, PNJ, objets, boutiques, compétences.

## 5. Lancer le serveur

```bash
archcore                     # login (11002) + monde (11008)
archcore login               # login seul
archcore world               # monde seul
ARCHCORE_TRACE=1 archcore    # journal détaillé de chaque paquet
```

## 6. Vérifier sans le jeu

```bash
archcore sim client test test Testeur
```

Le simulateur rejoue la poignée de main, la connexion, la liste des personnages et l'entrée dans le monde.
Il doit finir par « entrée en jeu confirmée », puis « flux complet OK ».

## 7. Connecter le vrai client

Le client lit la liste des serveurs dans un fichier `.ini` chiffré placé à côté de son exécutable.
Il lit d'abord, s'il existe, un fichier en texte clair du même nom suivi de `Test` (par exemple
`<NomDuClient>Test.ini`). Créer ce fichier dans `Game/` avec le contenu suivant :

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
| « Cannot access the login server » | ArchCore n'écoute pas sur 11002, ou le fichier `<NomDuClient>Test.ini` est absent |
| Chargement infini à l'entrée en jeu | Lancer avec `ARCHCORE_TRACE=1` et lire le journal |
| Inventaire ou boutique vides | Le dossier de langue du client (`ini_fr`…) doit contenir `ItemTemplateAll.ini` (voir [[Traduction|FR-Traduction]]) |
| Pas de son | Vérifier `[Sound_Option]` dans `Game/ini/COption.ini` ; le jeu réécrit ce fichier en quittant |
