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
| `Guild.BattleReadySeconds` | `180` | Délai (s) entre l'acceptation d'un combat de guildes et son début ; le client l'annonce en minutes. Les données d'Archonia ne le donnent pas |
| `PvP.MurdererPointsPerKill` | `20` | Points de meurtrier pour le meurtre d'un innocent de plus de 4 niveaux en dessous, attaqué en premier (paliers 40, 60, 100). Valeur d'origine inconnue |
| `PvP.RemissionPoints` | `2` | Points de meurtrier retirés par une rémission de 10 lettres de pardon, comme le client l'annonce |
| `Item.WeaponWearChance` | `10` | Probabilité (%) qu'un coup qui blesse use d'un point l'arme qui le porte (choix d'ArchCore ; 0 : pas d'usure) |
| `Item.ArmorWearChance` | `10` | Probabilité (%) qu'un coup reçu qui blesse use d'un point une pièce d'armure portée tirée au hasard (choix d'ArchCore) |
| `World.TimeSpeed` | `6` | Vitesse du temps du jeu envoyée au client (AGPMTIMER champ 0) : 6 comme Archonia, une journée en 4 h réelles ; 1 = heure réelle |
| `World.TimeStart` | `-1` | Heure du jeu au démarrage du serveur (0 à 23), la même pour tous ; -1 = horloge de la machine × vitesse, identique d'un démarrage à l'autre. Voir [[Protocole réseau|FR-Protocole-reseau]] |
| `World.WeatherAuto` | `0` | 1 = météo automatique (pluie, neige ou temps clair tirés pour tout le monde) ; 0 = seulement `/weather` (choix d'ArchCore) |
| `World.WeatherMinutes` | `30` | Minutes entre deux tirages de la météo automatique |
| `World.WeatherRainChance`, `World.WeatherSnowChance` | `20`, `10` | Probabilités (%) de la pluie et de la neige à chaque tirage ; le reste : temps clair |
| `Auction.MaxSales` | `100` | Ventes au plus par personnage dans la maison des ventes (1 à 100, la boîte du client) |
| `Auction.ExpireDays` | `30` | Durée d'une vente en jours ; échue, elle sort de la recherche mais reste annulable |
| `Summons.StatScale` | `100` | Caractéristiques des créatures invoquées, en pour cent de celles de leur modèle mises au niveau du maître (choix d'ArchCore) |
| `Chat.ShoutRange` | `0` | Portée du cri local « /s » ; 0 : deux fois `World.ViewRadius` (choix d'ArchCore) |
| `Wanted.ExpireDays` | `7` | Durée d'un avis de recherche, en jours depuis la dernière mise à prix (choix d'ArchCore) |
| `Wanted.ExpLossPercent` | `5` | Part de l'expérience du niveau perdue par un criminel recherché tué par un chasseur, sans perte de niveau (choix d'ArchCore) |
| `Shrine.Enabled` | `1` | Combats de Shrine selon l'horaire de la table `shrine` ; `0` : seulement par `/shrine open` |
| `BattleSquare.SupervisorTeleport` | `1` | Parler à un superviseur de la Battle Square (PNJ 551 à 554) envoie à l'étage de son niveau (choix d'ArchCore) |
| `BattleSquare.SkullTids` | `500,981,982` | TID des crânes de niveau 1, 2 et 3 lâchés dans la Battle Square (choix d'ArchCore) |
| `Battleground.Schedule` | vide | Plages « HH:MM-HH:MM » (séparées par « ; ») dont début et fin sont annoncés à tous (paquet 0x58) ; vide : aucune annonce |
| `Siege.Enabled` | `1` | Calendrier automatique des guerres de siège (date d'office, clôture des candidatures, début, fin) ; `0` : seulement les commandes `/siege` |
| `Siege.MinGuildMembers`, `Siege.MinGuildAgeDays` | `30`, `14` | Membres et âge (jours) d'une guilde candidate à un siège (valeurs du client ; à abaisser pour un jeu local) |
| `Siege.DurationMinutes` | `0` | Durée du combat de siège ; `0` : SWPlayingTime de `SiegeWar.txt` (60 min), à défaut 120 |
| `Siege.CarveSeconds` | `0` | Durée du carving du trône ; `0` : StampingTime de `SiegeWar.txt` |
| `Siege.CatapultDamagePercent` | `5` | Dégâts d'un tir de catapulte, en pour cent des PV de la porte extérieure ou de la tour visée (choix d'ArchCore) |
| `Archlord.MaxGuards` | `10` | Gardes royaux que l'ArchLord peut appeler à la fois (`/guardactivate`, Call Royal Knight) |
| `Archlord.PortalMinutes` | `10` | Guerre de Rensphere : durée de l'étape « portail » avant l'étape « combat » |
| `Archlord.SkyMinutes` | `5` | Durée du décalage de 12 h de l'horloge par Day and Night |
| `Tax.PayoutDay`, `Tax.PayoutHour` | `1`, `0` | Versement hebdomadaire des taxes des régions : jour (1 lundi à 7 dimanche) et heure (fuseau `World.TimeZone`) |
| `AntiCheat.Enabled` | `1` | Contrôles anti-triche côté serveur : positions envoyées par le client et cadence des coups normaux (voir [[Administration|FR-Administration]]) ; `0` : désactivés (la limite de paquets reste) |
| `AntiCheat.Action` | `log` | Sanction d'un joueur qui atteint `AntiCheat.Threshold` anomalies : `log` (journal seul), `kick` (déconnexion, sans attente de reconnexion) ou `ban` (bannissement du compte, puis déconnexion) |
| `AntiCheat.Threshold`, `AntiCheat.WindowMinutes` | `10`, `10` | Anomalies d'un personnage dans la fenêtre glissante (minutes) qui déclenchent la sanction |
| `AntiCheat.BanHours` | `24` | Durée du bannissement de `ban` (heures) ; `0` : sans fin (levé à la main dans `account_banned`) |
| `AntiCheat.SpeedTolerance` | `150` | Vitesse permise aux positions envoyées par le client, en pour cent de la vitesse de course courante (monture, renforcements et `/speed` compris) |
| `AntiCheat.MoveSlack` | `300` | Marge fixe de lag (unités) des positions ; le crédit de distance accumule en plus jusqu'à 5 s de course, pour les paquets retardés |
| `AntiCheat.AttackTolerance` | `150` | Cadence permise des coups normaux, en pour cent de la vitesse d'attaque : le premier coup d'une nouvelle cible ou d'une reprise attend l'intervalle divisé par 1,5 |
| `AntiCheat.MaxPacketsPerSecond`, `AntiCheat.PacketBurst` | `150`, `500` | Paquets par seconde et par connexion, et rafale tolérée (login et monde) ; au-delà ignorés, et une seconde de débit ignorée sans pause ferme la connexion (toujours, même avec `AntiCheat.Enabled = 0`). `0` : sans limite |
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
| `Database.Logs` | `archcore_logs` | Base des journaux (échanges, boutiques, banque, commandes MJ, anomalies de l'anti-triche), en écriture seule (sauf `/anomalies`, qui relit les anomalies). Une instance de test prend la sienne |
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
