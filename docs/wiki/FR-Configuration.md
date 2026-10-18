# Configuration

Toute la configuration est dans `config/archcore.json`. Les clés absentes prennent la valeur par défaut
de `crates/archcore/src/config.rs`.

## `login`

| Clé | Défaut | Rôle |
|---|---|---|
| `port` | `11002` | Port du serveur de login, celui que le client contacte |
| `bind` | `0.0.0.0` | Adresse d'écoute |

## `world`

| Clé | Défaut | Rôle |
|---|---|---|
| `port` | `11008` | Port du serveur monde |
| `bind` | `0.0.0.0` | Adresse d'écoute |
| `name` / `group` | `ArchCore` | Nom du monde et de son groupe dans la liste des serveurs |
| `publicAddress` | `127.0.0.1` | Adresse du monde envoyée au client. À changer pour un serveur accessible depuis Internet |
| `startPositions` | voir le fichier | Position d'apparition par race : 1 Humain, 2 Orc, 3 Elfe de lune, 4 Scion |
| `viewRadius` | `8000` | Rayon dans lequel les PNJ sont envoyés au joueur |
| `language` | `fr` | Langue des titres de PNJ (`data/i18n/npc-titles-<langue>.json`) ; `en` les laisse en anglais |
| `rates` | tous à `1` | Taux au démarrage : `exp`, `gold`, `drop`, `uniqueDrop`, `charisma`, `prestige`, `chatBuff` (voir [[Administration|FR-Administration]]) |
| `gmCommandLevels` | `{}` | Niveau MJ requis par commande, par exemple `{ "kill": 3 }` |
| `exitOnRestart` | `false` | Arrête le serveur à la fin du compte à rebours de `/announce_restart` |

Villages de départ actuels :

| Race | Village |
|---|---|
| 1 Humain | Anchorville |
| 2 Orc | Golundo |
| 3 Elfe de lune | Norine |
| 4 Scion | Cien, à confirmer |

## `database`

| Clé | Défaut | Rôle |
|---|---|---|
| `host` / `port` | `127.0.0.1` / `3306` | Serveur MariaDB |
| `user` / `password` | `root` / `archcore` | Accès. **À changer** hors d'un poste de développement |
| `connectionLimit` | `8` | Taille du pool par base |
| `auth` / `characters` / `world` | `archcore_*` | Noms des trois bases |

## `client`

| Clé | Défaut | Rôle |
|---|---|---|
| `dataDir` | `../client/ArchLord/Game` | Dossier `Game` du client, utilisé par les outils d'import et de traduction |

## Variables d'environnement

| Variable | Effet |
|---|---|
| `ARCHCORE_TRACE=1` | Journalise chaque paquet reçu et envoyé |
| `ARCHCORE_CONFIG=<fichier>` | Utilise un autre fichier de configuration, par exemple pour un serveur de test sur d'autres ports et d'autres bases |
