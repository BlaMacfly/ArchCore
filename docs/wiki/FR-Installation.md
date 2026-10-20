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
(1 à 3) peut être donné en quatrième argument ; 3 ouvre toutes les commandes (voir
[[Administration|FR-Administration]]) :

```bash
archcore db-init --account admin motdepasse 3
```

La même commande met à jour un compte existant (mot de passe, niveau MJ). Elle se relance sans risque
après une mise à jour du code : elle n'ajoute que ce qui manque.

## 4. Importer les données du client

```bash
archcore decrypt     # Game/ini → data/client-ini (en clair)
archcore import      # data/client-ini → archcore_world
```

Les tables du client sont chiffrées en RC4, avec une clé dérivée de MD5(« 1111 »), et écrites en CP949.
L'import les convertit en UTF-8 : modèles de personnages, expérience, PNJ, objets, boutiques, compétences,
quêtes. Il lit aussi la hauteur du sol dans la géométrie du monde (`world/` du client), ce qui prend
quelques minutes.

**Monstres, butin et carte.** Ils ne sont pas dans le client : ils viennent des tables du serveur
d'origine d'ArchLord (`spawndatatable.txt`, `spawninstancetable.txt`, `itemdroptable.txt`,
`groupdroprate.txt`, `droprankrate.txt` et `world/node_data`), qu'ArchCore ne fournit pas. Placez-les
dans `data/server-ini` (jamais versionné), ou désignez leur dossier :

```bash
archcore import data/client-ini --server chemin/vers/ini
```

Sans elles, le serveur démarre mais le monde est vide de monstres. Le détail des fichiers est dans
[[Outils|FR-Outils]].

## 5. Lancer le serveur

```bash
archcore                     # login (11002) + monde (11008)
archcore login               # login seul
archcore world               # monde seul
ARCHCORE_TRACE=1 archcore    # journal détaillé de chaque paquet
ARCHCORE_PROBE=1 archcore    # sonde : data/probe-*.log, ce que voit chaque joueur (ajouts, retraits, déplacements, coups, position suivie)
```

## Réglages utiles

Les réglages sont dans `config/auth.conf` et `config/world.conf` (tout est décrit dans
[[Configuration|FR-Configuration]]). Les plus utiles au départ, dans `world.conf` :

| Clé | Rôle |
|---|---|
| `WorldServer.Name` | Nom du monde dans la liste des serveurs |
| `WorldServer.PublicAddress` | Adresse donnée aux joueurs pour rejoindre le monde (voir « Jouer à plusieurs ») |
| `World.MonsterDensity` | Nombre de monstres par point d'apparition (1 : tables telles quelles ; 2 ou 3 : plus peuplé) |
| `Rate.Exp`, `Rate.Gold`, `Rate.Drop` | Taux d'expérience, d'or et de butin (1 : normal) |
| `World.Language` | Langue des titres de PNJ envoyés aux joueurs (`fr` par défaut) |

Chaque clé peut aussi venir d'une variable d'environnement : `World.MonsterDensity` →
`ARCHCORE_WORLD_MONSTER_DENSITY`. Un changement de réglage demande de relancer le serveur.

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

## Jouer à plusieurs

Par défaut, tout est réglé pour jouer sur la même machine (`127.0.0.1`). Pour accueillir d'autres
joueurs :

1. **Adresse du monde.** Dans `config/world.conf`, mettre dans `WorldServer.PublicAddress` l'adresse
   que les joueurs utilisent pour joindre la machine : son adresse sur le réseau local (par exemple
   `192.168.1.20`, donnée par `ipconfig`) ou son adresse publique / un nom de domaine pour Internet.
   Le serveur de connexion la transmet aux clients pour rejoindre le monde.
2. **Pare-feu.** Autoriser les ports TCP 11002 (connexion) et 11008 (monde). Sous Windows, dans un
   PowerShell lancé en administrateur :

   ```powershell
   New-NetFirewallRule -DisplayName "ArchCore" -Direction Inbound -Protocol TCP -LocalPort 11002,11008 -Action Allow
   ```
3. **Box Internet.** Pour des joueurs hors du réseau local, rediriger ces deux ports vers la machine du
   serveur dans l'interface de la box.
4. **Côté joueur.** Dans le fichier `<NomDuClient>Test.ini`, remplacer `127.0.0.1` par l'adresse du
   serveur (`<IP>192.168.1.20:11002</IP>`).
5. **Comptes.** Créer un compte par joueur avec `archcore db-init --account nom motdepasse`.

## Après un redémarrage de la machine

Rien n'est perdu à l'arrêt : personnages, objets et positions sont en base. Au redémarrage, relancer
dans l'ordre, chacun dans sa fenêtre :

1. MariaDB : `db/start-mariadb.cmd` (inutile si MariaDB est installé comme service Windows) ;
2. le serveur : `archcore` depuis la racine du dépôt.

Si la machine a été coupée brutalement, MariaDB répare ses tables au démarrage (« Crash table recovery
finished ») : c'est normal.

## Dépannage

| Symptôme | Piste |
|---|---|
| « Cannot access the login server » | ArchCore n'écoute pas sur 11002, ou le fichier `<NomDuClient>Test.ini` est absent |
| Chargement infini à l'entrée en jeu | Lancer avec `ARCHCORE_TRACE=1` et lire le journal |
| Le serveur s'arrête au démarrage sur une erreur de base | MariaDB n'est pas lancé, ou ses accès (`Database.*`) sont faux |
| Aucun monstre dans le monde | Les tables du serveur n'ont pas été importées (`archcore import --server …`) |
| Des créatures disparaissent quand on se déplace ou qu'on approche | Réglage d'affichage du client : dans `Game/ini/COption.ini` (client fermé), `CharView` multiplie la distance d'affichage des personnages (0 : 0,25 ; 1 : 0,45 ; 2 : 0,875 ; 3 : sans limite) et `MapView` celle de la carte (0,5 à 1,8). Mettre les deux à `3`, ou au maximum dans les options graphiques du jeu |
| Un autre joueur reste bloqué après le choix du personnage | `WorldServer.PublicAddress` vaut encore `127.0.0.1`, ou le port 11008 est fermé |
| Message « colonne inconnue » dans le journal | Code mis à jour sans relancer `archcore db-init` |
| Le serveur refuse de démarrer : « schéma des bases à mettre à jour » | Base pas encore suivie ou en retard : lancer `archcore db-init` ; base en avance : mettre à jour l'exécutable (voir [[Base de données\|FR-Base-de-donnees]]) |
| Inventaire ou boutique vides | Le dossier de langue du client (`ini_fr`…) doit contenir `ItemTemplateAll.ini` (voir [[Traduction|FR-Traduction]]) |
| Pas de son | Vérifier `[Sound_Option]` dans `Game/ini/COption.ini` ; le jeu réécrit ce fichier en quittant |
