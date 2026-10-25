# Configuration

La configuration tient en deux fichiers du dossier `config/` :

| Fichier | Lu par | Contenu |
|---|---|---|
| `auth.conf` | `archcore login` | Serveur de connexion, bases de données |
| `world.conf` | `archcore world` | Serveur monde, bases de données, données du client, réglages du monde, taux, commandes MJ |

`archcore all` et les outils (`db-init`, `import`, `sim`…) lisent les deux ; les clés `Database.*`, présentes
dans les deux fichiers, sont alors prises dans `world.conf` (un avertissement signale une différence).

Ces fichiers ne sont pas versionnés. Leurs modèles, `auth.conf.dist` et `world.conf.dist`, le sont et
documentent chaque option ; au premier lancement, un `.conf` absent est créé en copiant son modèle.

## Syntaxe

```ini
[worldserver]
# commentaire
WorldServer.Port = 11008
WorldServer.Name = "ArchCore"
```

Une ligne par réglage, `Clé = valeur` ; `#` commence un commentaire, les textes peuvent être entre
guillemets, l'en-tête `[authserver]` ou `[worldserver]` est ignoré. Une clé absente prend sa valeur par
défaut, avec un avertissement dans le journal ; une clé inconnue (faute de frappe) est signalée.

## `auth.conf`

| Clé | Défaut | Rôle |
|---|---|---|
| `LoginServer.Port` | `11002` | Port du serveur de connexion, celui que le client contacte |
| `LoginServer.Bind` | `0.0.0.0` | Adresse d'écoute |

## `world.conf`

| Clé | Défaut | Rôle |
|---|---|---|
| `WorldServer.Port` | `11008` | Port du serveur monde |
| `WorldServer.Bind` | `0.0.0.0` | Adresse d'écoute |
| `WorldServer.Name` / `WorldServer.Group` | `ArchCore` | Nom du monde et de son groupe dans la liste des serveurs |
| `WorldServer.PublicAddress` | `127.0.0.1` | Adresse du monde envoyée au client. À changer pour un serveur accessible depuis Internet. Au démarrage, le monde inscrit `PublicAddress:Port` dans la table `world`, sous `WorldServer.Name` et `WorldServer.Group` (23 caractères au plus) |
| `Client.DataDir` | `../client/ArchLord/Game` | Dossier `Game` du client, utilisé par les outils d'import et de traduction |
| `World.Language` | `fr` | Langue des titres de PNJ (`data/i18n/npc-titles-<langue>.json`) ; `en` les laisse en anglais |
| `World.ViewRadius` | `8000` | Portée de la discussion normale et des messages d'AddOn `SAY`. PNJ, monstres, joueurs et objets au sol se voient par secteurs (voir [[Protocole réseau|FR-Protocole-reseau]]) |
| `World.MonsterDensity` | `1` | Multiplie le nombre de monstres de chaque point d'apparition (arrondi, au moins 1). Les tables du serveur sont peu peuplées par endroits : 2 ou 3 rapprochent de la densité du jeu d'origine |
| `World.SlowPacketMs` | `100` | Un paquet traité plus longtemps (ms) sous le verrou du monde est noté « paquet lent », avec le temps d'attente du verrou ; `0` : jamais |
| `World.TimeZone` | `local` | Fuseau de l'horloge envoyée au client (paquet AGPMTIMER) : `local` = heure de la machine (jour/nuit, musique et ambiance suivent l'heure locale du serveur), `utc` = époque Unix brute. Voir [[Protocole réseau|FR-Protocole-reseau]] |
| `World.ExitOnRestart` | `0` | `1` : arrête le serveur à la fin du compte à rebours de `/announce_restart` |
| `StartPosition.<race>.Village` / `.X` / `.Y` / `.Z` | voir le modèle | Position d'apparition par race : 1 Humain, 2 Orc, 3 Elfe de lune, 4 Scion |
| `Rate.Exp`, `Rate.Gold`, `Rate.Drop`, `Rate.UniqueDrop`, `Rate.Charisma`, `Rate.Prestige`, `Rate.ChatBuff` | `1` | Taux au démarrage (voir [[Administration|FR-Administration]]) |
| `GM.CommandLevel.<commande>` | — | Niveau MJ requis pour une commande, par exemple `GM.CommandLevel.kill = 3` |

Villages de départ actuels :

| Race | Village |
|---|---|
| 1 Humain | Anchorville |
| 2 Orc | Golundo |
| 3 Elfe de lune | Norine |
| 4 Scion | Cien, à confirmer |

## Bases de données (les deux fichiers)

| Clé | Défaut | Rôle |
|---|---|---|
| `Database.Host` / `Database.Port` | `127.0.0.1` / `3306` | Serveur MariaDB |
| `Database.User` / `Database.Password` | `root` / `archcore` | Accès. **À changer** hors d'un poste de développement |
| `Database.ConnectionLimit` | `8` | Taille du pool par base |
| `Database.Auth` / `Database.Characters` / `Database.World` | `archcore_*` | Noms des trois bases principales |
| `Database.Logs` | `archcore_logs` | Base des journaux (échanges, boutiques, banque, commandes MJ), en écriture seule. Une instance de test prend la sienne |
| `Database.SlowQueryMs` | `200` | Une requête plus longue (ms) est notée « requête lente » dans le journal du serveur, une fois le serveur prêt (le chargement du monde n'est pas compté) ; `0` : jamais |

## Variables d'environnement

Toute clé peut être remplacée par une variable `ARCHCORE_<CLÉ>` : les points et les majuscules
intérieures deviennent des `_`, le tout en majuscules.

| Clé | Variable |
|---|---|
| `LoginServer.Port` | `ARCHCORE_LOGIN_SERVER_PORT` |
| `World.ViewRadius` | `ARCHCORE_WORLD_VIEW_RADIUS` |
| `World.MonsterDensity` | `ARCHCORE_WORLD_MONSTER_DENSITY` |
| `Database.Password` | `ARCHCORE_DATABASE_PASSWORD` |
| `Rate.Exp` | `ARCHCORE_RATE_EXP` |

Autres variables :

| Variable | Effet |
|---|---|
| `ARCHCORE_CONF_DIR=<dossier>` | Lit `auth.conf` et `world.conf` dans un autre dossier, par exemple pour un serveur de test sur d'autres ports et d'autres bases |
| `ARCHCORE_ROOT=<dossier>` | Racine du dépôt (dossier courant par défaut) |
| `ARCHCORE_TRACE=1` | Journalise chaque paquet reçu et envoyé |

## Ancienne configuration `archcore.json`

Jusqu'en octobre 2026, la configuration tenait dans `config/archcore.json`. Pour la convertir :

```bash
archcore config migrate config/archcore.json            # écrit config/auth.conf et config/world.conf
archcore config migrate test.json /chemin/conf-test     # vers un autre dossier (ARCHCORE_CONF_DIR)
```

Les fichiers sont écrits à partir des modèles `.dist`, commentaires compris ; un fichier existant n'est
remplacé qu'avec `--force`.
