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
| `Client.TablesDir` | *(vide)* | Dossier écrit par `archcore export-client-tables` : le serveur monde annonce aux clients ArchRust l'empreinte de son `tables.toml` (capacité `client-tables/1`) ; vide : rien d'annoncé |
| `World.Language` | `fr` | Langue des titres de PNJ (`data/i18n/npc-titles-<langue>.json`) ; `en` les laisse en anglais |
| `World.ViewRadius` | `8000` | Portée de la discussion normale et des messages d'AddOn `SAY`. PNJ, monstres, joueurs et objets au sol se voient par secteurs (voir [[Protocole réseau|FR-Protocole-reseau]]) |
| `World.MonsterDensity` | `1` | Multiplie le nombre de monstres de chaque point d'apparition (arrondi, au moins 1). Les tables du serveur sont peu peuplées par endroits : 2 ou 3 rapprochent de la densité du jeu d'origine |
| `World.SlowPacketMs` | `100` | Un paquet traité plus longtemps (ms) sous le verrou du monde est noté « paquet lent », avec le temps d'attente du verrou ; `0` : jamais |
| `Guild.BattleReadySeconds` | `180` | Délai (s) entre l'acceptation d'un combat de guildes et son début ; le client l'annonce en minutes. Les données d'Archonia ne le donnent pas |
| `PvP.MurdererPointsPerKill` | `20` | Points de meurtrier pour le meurtre d'un innocent de plus de 4 niveaux en dessous, attaqué en premier (paliers 40, 60, 100). Valeur d'origine inconnue |
| `PvP.RemissionPoints` | `2` | Points de meurtrier retirés par une rémission de 10 lettres de pardon, comme le client l'annonce |
| `Item.WeaponWearChance` | `10` | Probabilité (%) qu'un coup qui blesse use d'un point l'arme qui le porte (choix d'ArchCore ; 0 : pas d'usure) |
| `Item.ArmorWearChance` | `10` | Probabilité (%) qu'un coup reçu qui blesse use d'un point une pièce d'armure portée tirée au hasard (choix d'ArchCore) |
| `Item.TransformMaxDurationMs` | `0` | Durée au plus (ms) d'une potion de métamorphose ; 0 : celle de l'objet (« Polymorph Dur »). Sert au simulateur |
| `Skill.DowntimeSeconds` | `-1` | Immunité (s) de la cible après une Mutation ; -1 : `downtime_duration` de SKILL_SPEC (30), 0 : aucune. Sens de la colonne déduit : choix d'ArchCore |
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
| `Death.ExpPenaltyPercent` | `0` | Part de l'expérience du niveau perdue en ressuscitant au village, sans perte de niveau, rien sous le niveau 6 ni après une mort en combat entre joueurs ; 0 : aucune, comme Archonia (épisode 8 : 3) |
| `Death.ExpPenaltyRoguePercent` | `0` | La même pour un meurtrier de palier 3, à la place de la précédente quand celle-ci est active (épisode 8 : 10) |
| `Skill.GameBonusDivisor` | `10` | Unité des bonus d'expérience et de butin des effets (`bonus_exp`, `bonus_drop_rate` : 100 à 1000 aux talismans de Janus) : pour cent = valeur / ce diviseur (unité inconnue, choix d'ArchCore) |
| `Recall.RequestSeconds` | `60` | Rappel de groupe : secondes laissées à un membre pour accepter la demande (choix d'ArchCore) |
| `Recall.AllowInCombat` | `0` | 1 = rappel permis en combat, du lanceur comme du membre ; toujours refusé au mort, en siège en guerre, dans un Shrine en combat, dans la Battle Square et dans les régions sans résurrection (choix prudent d'ArchCore) |
| `Recall.ExcludedItems` | `4817` | TID des objets d'usage 15 qui ne rappellent pas (Bead of Nothernland, description sans rapport) |
| `Social.MinIntervalMs` | `1000` | Délai minimal entre deux actions sociales d'un joueur ; une action plus rapprochée est ignorée (anti-inondation, choix d'ArchCore) |
| `Party.InviteLevelGap` | `10` | Invitation de groupe refusée (gestion op 13) quand l'invité a au moins tant de niveaux de moins que l'invitant ; 0 = pas de règle |
| `Party.EffectRange` | `1600` | Zone d'effet du groupe : membres à moins de tant d'unités du chef (portée d'origine inconnue : celle des sphères) |
| `Party.BonusPercentPerMember` | `2` | Bonus du groupe par membre dans la zone au-delà du premier : pour cent de dégâts (chef archer ou autre) ou de défense (chef chevalier) ; proposition à valider en jeu |
| `Party.BonusPointsPerMember` | `50` | Bonus du groupe d'un chef mage : PV et PM maximaux par membre dans la zone au-delà du premier |
| `Party.BonusMaxLevelGap` | `20` | Aucun bonus du groupe quand l'écart de niveau entre ses membres dépasse cette valeur |
| `Bank.SecretSafeMaxExtraPages` | `3` | Secret Safe (usage 21) : pages de banque en plus (1 à 3) au-delà desquelles il est refusé ; 3 = le refus du client (4 pages), le texte de l'objet 3916 annonce 3 pages (2). Choix d'ArchCore |
| `Convert.LuckyScrollBonus` | `0` | Fortune Manual (usage 25) allumé : points de pour cent ajoutés à la réussite d'un sertissage de pierre ou de rune et d'un renforcement (0 à 100), un exemplaire consommé par tentative ; valeur d'origine inconnue, 0 : sans effet |
| `ReturnToLogin.LoginAddress` | vide | Adresse « hôte:port » du serveur de connexion donnée au client pour revenir à la sélection des personnages ; vide : le client reprend celle où il s'est connecté |
| `ReturnToLogin.KeySeconds` | `60` | Durée de validité de la clé à usage unique du retour à la sélection des personnages |
| `Shrine.Enabled` | `1` | Combats de Shrine selon l'horaire de la table `shrine` ; `0` : seulement par `/shrine open` |
| `Title.Schola` | `0` | Titres Defender of Chantra et Schola de rangs 1 à 3 (origine incertaine, probablement Archonia) ; rangs 4 et 5 et Christmas Event jamais repris |
| `Title.ChangeDelaySeconds` | `7200` | Délai entre deux changements du titre actif (équiper ou retirer), les « 2 hours » du client ; `0` : aucun |
| `Title.ManagerOpensWindow` | `1` | Un « <Title Manager> » ouvre les fenêtres des titres (AgpmTitle op 10) au lieu du dialogue (choix d'ArchCore) |
| `Title.CollectCountsKills` | `0` | `1` : une condition de collecte compte aussi les morts de la créature ; `0` : seulement les récoltes, comme les textes |
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
| `Rules.Heroic` | `1` | Système héroïque (`heroic.rs`) : part héroïque des coups et des compétences (dégâts héroïques moins défense héroïque, résistance selon la classe de l'assaillant, 80 % au plus, contre un joueur seulement). Règle proposée |
| `Rules.Ammunition` | `1` | Munitions (`ammo.rs`) : une flèche (194) ou un carreau (238) par coup à l'arc ou à l'arbalète, `cost_arrow` par compétence ; sans munition, résultat 2 / 3 ou refus 12 |
| `Rules.WeaponRequirement` | `1` | Arme exigée par une compétence (`requirement.rs`, colonne `requirement`) : lancement refusé (action 8), passive sans effet |
| `Rules.ItemLifetime` | `1` | Objets à durée (`lifetime.rs`) : temps de jeu (`RemainTime`) et date d'expiration (`ExpireTime`) des objets Cash décomptés ; à la fin, message 5 et objet consommé |
| `Rules.LotteryKeys` | `1` | Boîte de loterie à `SubType` (or, argent, bronze, platine) : la clé de même `SubType` est exigée et consommée |
| `Rules.DeathStrike` | `1` | Death Strike : mort instantanée d'un monstre ordinaire (ni boss, ni objet de siège), chance `death` de la passive |
| `Rules.ForcedMove` | `1` | Déplacements forcés des compétences (`move_pos` : cible tirée au contact, ruée d'un monstre ; recul de Body Check) |
| `Rules.KnockbackUnit` | `100` | Unités du jeu par point de `move_distance` (recul de Body Check ; unité de la table inconnue) |
| `Rules.ShrinkPercent` | `30` | Rétrécissement (état 25) : pour cent de portée d'attaque et de dégâts physiques ôtés (choix d'ArchCore) |
| `Rules.ConfusionIntervalMs` | `1500` | Confusion et berserk d'un monstre (états 21 et 24) : délai entre deux coups sur une cible tirée au hasard ou la plus proche |
| `Rules.ProvokeSeconds` | `10` | Provoke, Zone Provoke : durée pendant laquelle le monstre garde le lanceur pour cible |
| `Rules.MonsterKin` | `1` | Invocations, suiveurs et renfort des monstres (`ai_kin.rs`, colonnes `SummonTID`, `FollowerTID`, `PinchMonster` d'aidatatable) |
| `Rules.PinchHpPercent` | `30` | Seuil de PV (pour cent) sous lequel un monstre appelle son renfort (`PinchMonster`), une fois par vie |
| `Rules.LootLevelGap` | `11` | Règle de butin d’origine : au-delà de cet écart de niveaux entre le tueur et le monstre, dans un sens comme dans l’autre (monstre trop faible ou trop fort), le monstre ne donne qu’une pièce d’or et aucun objet. 0 coupe la règle. Morts réelles seulement ; `/simloot` l’ignore. |
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
| `GM.CommandLevel.<commande>` | — | Niveau MJ requis pour une commande, par exemple `GM.CommandLevel.kill = 3` ; pour une commande de console SOAP, mots séparés par des points (`GM.CommandLevel.account.create = 3`) |
| `SOAP.Enabled` | `0` | `1` : interface SOAP d'administration, compatible avec celle d'AzerothCore et de TrinityCore (voir [[Administration|FR-Administration]]) |
| `SOAP.IP` / `SOAP.Port` | `127.0.0.1` / `7878` | Adresse et port d'écoute HTTP de SOAP, les défauts d'AzerothCore. Les mots de passe y passent en clair : n'exposer qu'en local ou derrière un pare-feu |
| `SOAP.MinGmLevel` | `3` | Niveau MJ minimal d'un compte pour se servir de SOAP (comme SEC_ADMINISTRATOR) ; en dessous, HTTP 403. Chaque commande demande en plus son propre niveau |

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
| `SOAP.Enabled` | `ARCHCORE_SOAP_ENABLED` |

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
