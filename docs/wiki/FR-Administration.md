# Administration (MJ / Admin)

Cette page décrit les commandes de maître du jeu (MJ) que le client sait envoyer, et comment les utiliser.

> **État d'ArchCore** : le serveur gère la discussion, envoie le niveau MJ au client et exécute les
> commandes marquées ✅ dans la liste (voir la section 3). Les autres répondent « Commande inconnue » :
> elles attendent les systèmes de jeu correspondants, absents d'ArchCore (voir la fin de la section 4).
> Le suivi se fait sur la page [[Feuille de route|FR-Feuille-de-route]].

> **Panneau MJ.** Avec le client ArchRust, l'AddOn [ArchCoreAdmin](https://github.com/BlaMacfly/ArchCoreAdmin)
> propose un bouton pour chacune de ces commandes et relit les réponses du serveur.

## 1. Donner les droits MJ à un compte

Le niveau MJ est stocké dans la colonne `gm_level` de la table `archcore_auth.account`. La valeur 0
correspond à un joueur normal. Plus le nombre est élevé, plus le compte a de droits.

Créer un compte MJ, ou changer le niveau d'un compte existant :

```bash
archcore db-init --account <identifiant> <mot_de_passe> <niveau>
# exemple : archcore db-init --account admin MotDePasseSolide 3
```

Ou directement en SQL :

```sql
UPDATE archcore_auth.account SET gm_level = 3 WHERE username = 'admin';
```

Le compte doit se reconnecter pour que le changement compte. C'est le serveur qui décide si une
commande est autorisée selon ce niveau : le client se contente de l'envoyer. Un joueur de niveau 0
qui tape une commande MJ n'a aucune réponse (les préfixes de discussion « /p », « /g », « /s », « /race »,
« /scream » et « /warn » sont lus avant les commandes).

## 2. Deux façons d'envoyer une commande

### Par la fenêtre de discussion

Toutes les commandes s'écrivent dans la discussion, précédées d'une barre oblique `/`, puis on
valide avec Entrée :

```text
/kill "NomDuJoueur"
/chatban "NomDuJoueur" 30
/SetExp 2.0
```

- Les **noms** se mettent entre guillemets doubles, ce qui permet les espaces.
- Les **nombres** s'écrivent sans guillemets ; les décimales utilisent un point (`1.5`).
- Les noms de commandes respectent la **casse** : `SetExp`, mais `chatban`.

### Par le panneau « Admin Commands »

Le client contient un panneau graphique « Admin Commands ». Il affiche le niveau MJ du personnage
(« Your admin level ») et range les commandes dans des onglets : taux, modération, événements,
arène, objets et personnages, monde, région, système. Chaque bouton tape pour vous la commande
correspondante dans la discussion. Le résultat est donc le même que la saisie manuelle.

Le panneau lit le niveau MJ envoyé par le serveur. ArchCore l'envoie à l'entrée en jeu (paquet
AgpmAdmin, type 0x12) pour les comptes dont `gm_level` est supérieur à 0.
Le client lit aussi un argument de lancement `admin` ; son effet exact reste à confirmer.

Les réponses du serveur (valeur d'un taux, erreur de syntaxe, droits insuffisants…) arrivent dans
la discussion comme messages système.

## 3. Commandes gérées par ArchCore

| Commande | Niveau MJ | Ce que fait ArchCore |
|---|---|---|
| `/GetExp`, `/GetGold`, `/GetDrop`, `/GetUniqueDrop`, `/GetCharisma`, `/GetPrestigeRate`, `/GetChatBuff` | 1 | Affiche la valeur du taux |
| `/SetExp`, `/SetGold`, `/SetDrop`, `/SetUniqueDrop`, `/SetCharisma`, `/SetPrestigeRate` `<x>` | 3 | Change le taux (de 0 à 100) |
| `/SetChatBuff <0\|1>` | 3 | Active ou désactive le bonus de discussion |
| `/chatban "nom" <minutes>` | 1 | Le joueur ne peut plus parler ; `0` lève l'interdiction. Le joueur est prévenu |
| `/kill ["nom"]` | 2 | Met les PV du personnage à 0 ; sans nom, sur soi. Personnage connecté uniquement |
| `/invincible [0\|1]` | 2 | Les monstres ne vous blessent plus (1, par défaut) ou de nouveau (0) |
| `/speed <1-10>` | 2 | Multiplie la vitesse de déplacement (marche et course) du MJ ; `1` la rétablit. Gardée quand l'équipement ou les effets changent, oubliée à la déconnexion. Commande propre à ArchCore |
| `/rename_char "ancien" "nouveau"` | 2 | Renomme en base (2 à 24 caractères, sans espace). Un joueur connecté voit son nouveau nom à sa prochaine connexion |
| `/create "nom" <TID> <quantité> <0\|1>` | 3 | Ajoute l'objet dans `character_items`, dans la première case libre de l'inventaire. Un joueur connecté le reçoit tout de suite ; le personnage peut aussi être hors ligne |
| `/additem <TID> [quantité]` | 3 | Raccourci de `/create` sur soi : ajoute l'objet (1 par défaut, jusqu'à 9999) dans l'inventaire du MJ, échangeable. Commande propre à ArchCore |
| `/simloot <TID du monstre> <nombre> [taux de butin] [taux d'or]` | 2 | Simule le butin de `nombre` monstres de ce modèle (1 à 10 000) : chaque mort tire le butin exactement comme à la mort d'un vrai monstre (or, groupes de butin, objets propres au monstre), sans rien créer ni poser au sol. Taux du serveur (`/GetDrop`, `/GetGold`) sauf s'ils sont donnés (0 à 100) ; niveau du monstre : celui d'un monstre de ce modèle présent dans le monde, sinon celui du modèle ; le niveau du personnage du MJ tient lieu de niveau du tueur. Réponse : un en-tête (monstres, taux, or, nombre d'objets et de sortes), puis une ligne `· quantité × nom (TID n)` par objet, du plus fréquent au plus rare (40 au plus, puis une ligne de résumé). Commande propre à ArchCore |
| `/weather [clear\|rain\|snow]` | 2 | Météo pour tous les joueurs en jeu (pluie, neige, temps clair) ; sans argument, la météo et l'heure du jeu. Commande propre à ArchCore |
| `/title ["nom"]` | 1 | Titres d'un joueur en jeu (soi-même sans nom) : titre actif, charisme et son rang (`charisma_title`), titres possédés, objectifs et valeurs de leurs conditions. Commande propre à ArchCore |
| `/shrine [open\| `/siege [start|end <château> | owner <château> <guilde|-> | hp <château> <PV>]` | 3 | Sans argument, l'état des châteaux ; `start` clôt les candidatures et ouvre le combat, `end` le termine (la guilde du dernier carving prend le château), `owner` donne le château à une guilde (`-` : aucune) et remet son calendrier au statut 0, `hp` ramène les PV des objets intacts d'un château en guerre (essais). Commande propre à ArchCore |
|close <n>]` | 3 | Sans argument, l'état des Shrines ; `open` ouvre le combat du Shrine `n` sans attendre son horaire, `close` le ferme sans vainqueur. Commande propre à ArchCore |
| ✅ `/siege [start|end|owner|hp …]` | État des châteaux, début et fin forcés d'un siège, propriétaire, PV des objets (ajout d'ArchCore, absent du panneau) |
| `/tax [pay [château]]` | 3 | Sans argument, taux et recettes des régions de chaque château ; `pay` verse aussitôt les recettes d'un château (ou de tous) au chef de la guilde propriétaire, moins le tribut de l'ArchLord. Commande propre à ArchCore |
| `/archlord [set <nom> \| clear]` | 3 | Désigne l'ArchLord (ops 0 et 1 à tous) ou le retire ; sans argument, le nom de l'ArchLord. `/archlord <message>` de l'ArchLord : message à tout le serveur. `/archlordbegin`, `/archlordend` (guerre de Rensphere), `/nextstep`, `/archlordactivate <nom>`, `/archlorddeactivate`, `/castleowner <château> <guilde>`, `/deletecastleowner <château>` : commandes d'Archonia (sysstr 622 à 629, 636, 657, 658) |
| `/durability <valeur\|-1>` | 3 | Fixe la durabilité de l'équipement porté qui s'use (`-1` : pleine, 0 : cassé) ; pour essayer l'usure et la réparation. Commande propre à ArchCore |
| `/questreset <TID>` | 3 | Efface la quête TID (en cours ou terminée) du journal du MJ ; visible à la prochaine connexion. Commande propre à ArchCore |
| `/recast` | 3 | Remet à zéro, côté serveur, les délais de relance des compétences du MJ (le client peut encore afficher le sien). Commande propre à ArchCore |
| `/last_refine` | 3 | Relance la dernière recette raffinée avec succès depuis la connexion (« Use last refine recipe » du panneau), comme l'op 6 du client : ingrédients pris dans l'inventaire, or payé, résultat dans la fenêtre de raffinage. Refusée sans raffinage réussi |
| `/level <niveau> ["nom"]` | 3 | Fixe le niveau du personnage connecté (1 à 120) : expérience remise à zéro, PV et PM rendus, points de compétence recalculés ; sans nom, sur soi. Commande propre à ArchCore |
| `/exp <points> ["nom"]` | 3 | Donne de l'expérience au personnage connecté comme un monstre tué (64 bits, montées de niveau comprises) ; sans nom, sur soi. Commande propre à ArchCore |
| `/money <montant> ["nom"]` | 3 | Fixe l'or du personnage connecté (de 0 à 1 000 000 000) ; sans nom, sur soi. Commande propre à ArchCore |
| `/charisma <points> ["nom"]` | 3 | Donne des points de charisme au personnage connecté (1 à 1 000 000, multipliés par le taux `Rate.Charisma` et les bonus de charisme de ses effets, sauf celui des boss) ; sans nom, à soi. Commande propre à ArchCore (Archonia n'en a pas), en attendant le combat entre joueurs |
| `/announce_restart <minutes> <secondes>` | 3 | Annonce à tous les joueurs, rappel chaque minute, compte à rebours sur les dernières secondes. `0 0` annule |
| `/move x,z ["nom"]` | 1 | Téléporte le MJ en (x, z) et envoie les PNJ du nouvel endroit ; avec un nom (niveau 2), un autre joueur en jeu : c'est ainsi qu'on pose ailleurs un joueur, le serveur refusant qu'un client se téléporte lui-même (anti-triche) |
| `/anomalies "nom"` | 1 | Anomalies relevées par l'anti-triche pour ce personnage : nombre en base et dans la fenêtre en cours, dix dernières (voir la section 5). Commande propre à ArchCore |

Le niveau requis se change commande par commande dans `config/world.conf` (voir
[[Configuration|FR-Configuration]]) :

```ini
Rate.Exp = 2
Rate.Drop = 1.5
GM.CommandLevel.kill = 3
GM.CommandLevel.move = 2
World.ExitOnRestart = 0
```

- `Rate.*` : taux au démarrage (`Exp`, `Gold`, `Drop`, `UniqueDrop`, `Charisma`, `Prestige`,
  `ChatBuff`). Les commandes `Set…` les changent **en mémoire** : au redémarrage, ils reprennent
  ces valeurs.
- `World.ExitOnRestart` : avec `1`, le serveur s'arrête à la fin du compte à rebours de
  `/announce_restart`, pour qu'un superviseur le relance. Par défaut, il se contente d'annoncer.

Limites actuelles :

- Les taux d'expérience, d'or et de butin s'appliquent ; ceux du butin unique, du charisme, du
  prestige et le bonus de discussion sont gardés mais sans effet, faute des systèmes correspondants.
- Les interdictions de discussion sont gardées en mémoire et disparaissent au redémarrage.
- Avec `/create`, le dernier nombre (« non échangeable ») n'est pas encore enregistré.
- À vérifier avec le vrai client : l'animation de mort de `/kill`. Après `/move`, le MJ est posé à la
  hauteur du sol (`map_height`) ; hors de la carte des hauteurs, il garde son altitude.
- Le client envoie lui-même `/move` quand on tape un numéro de point de passage dans la discussion.

## 4. Liste des commandes

Notation : `"nom"` = nom de personnage entre guillemets ; `<n>` = nombre entier ; `<x>` = nombre décimal.

✅ = gérée par ArchCore (détails dans la section 3).

### Taux du serveur

Un taux est un multiplicateur : `1.0` = normal, `2.0` = double.

| Commande | Effet |
|---|---|
| ✅ `/SetExp <x>` | Multiplicateur d'expérience |
| ✅ `/SetGold <x>` | Multiplicateur d'or gagné |
| ✅ `/SetDrop <x>` | Multiplicateur de butin |
| ✅ `/SetUniqueDrop <x>` | Multiplicateur de butin unique : gardé et affiché, mais sans effet sur le butin pour l'instant (sens d'« unique » dans l'épisode 8 à établir) |
| ✅ `/SetCharisma <x>` | Multiplicateur de points de charisme |
| ✅ `/SetPrestigeRate <x>` | Multiplicateur de prestige |
| ✅ `/SetChatBuff <0\|1>` | Désactive (0) ou active (1) le bonus de discussion |
| ✅ `/GetExp`, `/GetGold`, `/GetDrop`, `/GetUniqueDrop`, `/GetCharisma` | Affiche la valeur actuelle du taux |

Exemple : `/SetExp 2.0` puis `/GetExp` pour vérifier.

### Modération

| Commande | Effet |
|---|---|
| ✅ `/chatban "nom" <minutes>` | Interdit la discussion pendant N minutes ; `0` lève l'interdiction |
| ✅ `/kill "nom"` | Tue le personnage |
| `/kill_explosion <effet> "nom"` | Pose un effet d'explosion à la mort d'un personnage ; sans nom, sur soi |
| `/kill_explosion -1 "nom"` | Retire cet effet d'un personnage |
| `/kill_explosion` | Retire l'effet sur soi |
| ✅ `/rename_char "ancien" "nouveau"` | Renomme un personnage |
| ✅ `/anomalies "nom"` | Anomalies de l'anti-triche d'un personnage (ajout d'ArchCore) |

### Arène

| Commande | Effet |
|---|---|
| `/arena_ban_day "nom" <jours>` | Exclut de l'arène pendant N jours ; `0` lève l'exclusion |
| `/arena_ban_hour "nom" <heures>` | Exclut de l'arène pendant N heures ; `0` lève l'exclusion |
| `/arena_ranked_ban_day "nom" <jours>` | Exclut de l'arène classée pendant N jours |
| `/spectate_arena ["nom"]` | Observe une arène ; sans nom, en choisit une |
| `/spectate_ranked_arena ["nom"]` | Observe une arène classée |
| `/spectate_normal_arena ["nom"]` | Observe une arène normale |
| `/spectate_shuffle_arena ["nom"]` | Observe une arène mélangée |
| `/leave_spectate` | Quitte le mode observateur |
| `/arena_win_reward <type> <pièces>` | Pièces gagnées par victoire pour un type de partie |
| `/arena_lose_reward <type> <pièces>` | Pièces gagnées par défaite pour un type de partie |
| `/set_chat_season_winners` | Importe les gagnants de la saison |

### Événements

| Commande | Effet |
|---|---|
| `/tournament round` | Passe au tour suivant du tournoi |
| `/tournament disable` | Désactive le tournoi |
| `/tournament recovery <pourcentage> <0\|1>` | Soin entre les tours ; le second nombre active l'invocation de la base |
| `/tournament ban "nom" <jours>` / `/tournament unban "nom"` | Exclut ou réintègre un joueur |
| `/death_match freeze` / `break` / `reload` / `disable` | Fige, interrompt, recharge ou désactive le match à mort |
| `/death_match ban "nom" <jours>` / `unban "nom"` | Exclut ou réintègre un joueur |
| `/racewar reload` / `disable` | Recharge ou désactive la guerre des races |
| `/racewar ban "nom"` / `unban "nom"` | Exclut ou réintègre un joueur |
| `/game_event set_only_solo` | Tournoi et match à mort en solo uniquement |
| `/game_event !set_only_solo` | Retire cette restriction |
| `/game_event ban "nom" <jours>` / `unban "nom"` | Exclut ou réintègre un joueur |
| `/counter open "nom" <secondes>` | Ouvre un compteur d'événement |
| `/counter close` / `freeze` / `break` | Ferme, fige ou interrompt le compteur |
| `/battlepass reload` | Recharge les saisons du passe de combat |
| `/battlepass_premium "nom"` | Débloque le passe premium d'un personnage |
| `/reload_events`, `/reload_class_events`, `/reload_event_effect` | Recharge les événements bonus |
| `/enable_events` / `/disable_events` | Active ou désactive les événements bonus |

### Objets et personnages

| Commande | Effet |
|---|---|
| ✅ `/create "nom" <TID> <quantité> <0\|1>` | Donne un objet au personnage. `TID` = identifiant de l'objet ; le dernier nombre rend l'objet non échangeable (1) |
| ✅ `/additem <TID> [quantité]` | Ajoute un objet à son propre inventaire (ajout d'ArchCore, absent du panneau) |
| ✅ `/simloot <TID du monstre> <nombre> [taux de butin] [taux d'or]` | Simule le butin d'un nombre de monstres et liste les objets obtenus (ajout d'ArchCore, absent du panneau) |
| ✅ `/speed <1-10>` | Multiplie sa vitesse de déplacement (ajout d'ArchCore, absent du panneau) |
| ✅ `/weather [clear\|rain\|snow]` | Météo de tout le monde, ou météo et heure du jeu (ajout d'ArchCore, absent du panneau) |
| ✅ `/title ["nom"]` | Titres, objectifs et rang de charisme d'un joueur (ajout d'ArchCore, absent du panneau) |
| ✅ `/shrine [open\|close <n>]` | État des Shrines, ouverture ou fermeture forcée d'un combat (ajout d'ArchCore, absent du panneau) |
| ✅ `/durability <valeur\|-1>` | Fixe la durabilité de son équipement porté (ajout d'ArchCore, absent du panneau) |
| ✅ `/money <montant> ["nom"]` | Fixe l'or d'un personnage connecté (ajout d'ArchCore, absent du panneau) |
| ✅ `/charisma <points> ["nom"]` | Donne des points de charisme à un personnage connecté (ajout d'ArchCore, absent du panneau) |
| ✅ `/level <niveau> ["nom"]` | Fixe le niveau d'un personnage connecté (ajout d'ArchCore, absent du panneau) |
| ✅ `/exp <points> ["nom"]` | Donne de l'expérience à un personnage connecté (ajout d'ArchCore, absent du panneau) |
| ✅ `/questreset <TID>` | Efface une quête du journal du MJ (ajout d'ArchCore, absent du panneau) |
| ✅ `/recast` | Remet à zéro les délais de relance des compétences du MJ (ajout d'ArchCore, absent du panneau) |
| ✅ `/last_refine` | Relance la dernière recette de raffinage réussie |
| `/refresh_prestige_db` | Recharge les données de prestige |
| `/refinetest` / `/refinetest stop` | Lance ou arrête le test de charge du raffinage |

Exemple : `/create "Testeur" 1234 5 0` donne 5 exemplaires de l'objet 1234, échangeables.
Le panneau propose une recherche d'objet par nom ou par TID.

### Monde et failles

| Commande | Effet |
|---|---|
| `/go_rift ["nom"]` | Se rend dans une faille ; le nom est facultatif |
| `/close rift` ou `/close_rift` | Ferme la faille |
| `/next_rift_objectives` | Passe aux objectifs suivants de la faille |
| `/bg_off` | Retire les bonus de champ de bataille |
| `/spawn_instance "nom" <nombre> <rayon>` | Fait apparaître un groupe d'événement autour de soi |
| ✅ `/move x,z` | Se téléporte en (x, z) |
| `/region …` | Modifie les réglages d'une région (réduction de dégâts, de potions, limite d'incantation). Utiliser le panneau, qui construit la commande |

### Système

| Commande | Effet |
|---|---|
| ✅ `/announce_restart <minutes> <secondes>` | Annonce un redémarrage, avec un compte à rebours final |
| `/refresh_lua_scripts` | Recharge les scripts LUA du serveur |
| `/reload_ip_players` | Recharge la liste d'adresses IP |
| `/log_activity` | Écrit le journal d'activité |
| `/cast_cap <0-100>` | Limite de vitesse d'incantation |
| `/cooldown_cap <0-100>` | Limite de réduction des temps de recharge |
| `/test <minutes>` | Commande de test : donne des points et règle le délai d'apparition des boss |

### Commandes sans système dans ArchCore

Le panneau « Admin Commands » d'Archonia ne fait que composer ces commandes ; leur effet est décidé par le
serveur. Elles restent sans effet (« Commande inconnue ») tant que le système qu'elles pilotent n'existe pas :

| Commandes | Système absent |
|---|---|
| `/arena_*`, `/spectate_*`, `/leave_spectate`, `/set_chat_season_winners` | Arène, saisons et récompenses de sanctuaire |
| `/tournament …`, `/death_match …`, `/racewar …`, `/game_event …`, `/counter …` | Événements de jeu (tournoi, match à mort, guerre des races, compteur) |
| `/battlepass …`, `/battlepass_premium` | Passe de combat |
| `/reload_events`, `/reload_class_events`, `/reload_event_effect`, `/enable_events`, `/disable_events` | Événements bonus |
| `/kill_explosion …` | Effet d'explosion à la mort |
| `/refresh_prestige_db` | Prestige |
| `/refinetest` | Test de charge du raffinage (outil du serveur d'origine) |
| `/go_rift`, `/close rift`, `/close_rift`, `/next_rift_objectives` | Failles |
| `/bg_off` | Champs de bataille |
| `/spawn_instance` | Groupes d'apparition d'événement (« event spawn name ») ; ArchCore n'a que les points d'apparition des tables d'origine |
| `/region …` | Réglages de région (réduction de dégâts, de potions, limite d'incantation, par classe) |
| `/cast_cap`, `/cooldown_cap` | Bonus de vitesse d'incantation et de réduction des recharges, que ces limites plafonnent |
| `/refresh_lua_scripts`, `/reload_ip_players`, `/log_activity`, `/test` | Scripts LUA, liste d'adresses IP, journal d'activité et commande de test du serveur d'origine |

## 5. Anti-triche

ArchCore ne croit pas le client sur parole, sans punir un joueur honnête à cause du lag ; il n'y a
aucun contrôle côté client. Réglages `AntiCheat.*` : voir [[Configuration|FR-Configuration]].

- **Déplacements.** Le client d'origine n'envoie que sa destination et le serveur fait marcher le
  joueur lui-même, à sa vitesse : un speedhack local n'y change rien. Une position fournie par le
  client (champ 1 du paquet 0x40 : simulateur, client modifié) est comparée à un crédit de distance :
  vitesse de course courante (monture, renforcements, `/speed`) × `AntiCheat.SpeedTolerance`, plus la
  marge `AntiCheat.MoveSlack`, jusqu'à 5 s de course accumulées pour les paquets retardés. Au-delà, le
  serveur garde la dernière position valide et la renvoie au client comme une téléportation ;
  anomalie `vitesse`, ou `téléportation` pour un saut impossible même avec le crédit plein. Portails,
  parchemins, `/move`, résurrections, sièges, Shrines, Battle Square, entrée en jeu et reconnexion
  repartent de la position posée par le serveur. Les MJ ne sont pas contrôlés.
- **Coups normaux.** Le serveur porte les coups au rythme de la vitesse d'attaque ; une nouvelle cible
  ou une reprise après un déplacement n'avance plus le coup suivant au-delà de
  `AntiCheat.AttackTolerance`. Le coup en avance est retardé ; trois demandes en avance avant le coup
  suivant font une anomalie `cadence`.
- **Paquets.** Au-delà de `AntiCheat.MaxPacketsPerSecond` (rafale `AntiCheat.PacketBurst`), les
  paquets d'une connexion sont ignorés ; une inondation qui dure ferme la connexion (anomalie
  `inondation`, le personnage quitte le monde sans attendre de reconnexion), quel que soit le réglage.
- **Sanctions.** Chaque anomalie est notée dans le journal du serveur (`[anti-triche]`) et dans la table
  `log_anomaly` de la base des journaux (20 lignes par minute et par personnage au plus). Avec
  `AntiCheat.Action = kick` ou `ban`, `AntiCheat.Threshold` anomalies en `AntiCheat.WindowMinutes`
  minutes déconnectent le joueur, ou bannissent son compte (`account_banned`, auteur `anti-triche`,
  `AntiCheat.BanHours` heures). Par défaut : journal seul.
- **Consulter.** `/anomalies "nom"` : nombre d'anomalies en base et dans la fenêtre en cours, puis les
  dix dernières (date, type, détail, sanction).

## 6. Interface SOAP (outils d'AzerothCore et de TrinityCore)

Le serveur monde peut recevoir des commandes d'administration par **SOAP**, avec le même protocole
qu'AzerothCore et TrinityCore : leurs panneaux web, bots Discord et scripts (PHP `SoapClient`,
`acore-soap` en Python, cURL…) fonctionnent sans changement. Méthode `executeCommand`, élément
`command`, espace de noms quelconque (`urn:AC`, `urn:TC`, `urn:ArchCore`…), authentification HTTP
Basic avec un compte du jeu.

### Activation

Dans `config/world.conf` (ou par variables d'environnement, voir [[Configuration|FR-Configuration]]) :

```ini
SOAP.Enabled = 1
SOAP.IP = "127.0.0.1"
SOAP.Port = 7878
SOAP.MinGmLevel = 3
```

Au démarrage du monde, le journal montre `[soap] en écoute sur http://127.0.0.1:7878`. Le compte qui
se connecte doit avoir un niveau MJ d'au moins `SOAP.MinGmLevel` (`account.gm_level`, voir la
section 1) ; chaque commande demande en plus son propre niveau, exactement comme en jeu.

### Sécurité

- Les identifiants passent **en clair** (HTTP Basic, sans chiffrement). Laisser `SOAP.IP = "127.0.0.1"`,
  ou n'ouvrir le port que derrière un pare-feu, un réseau privé ou un tunnel SSH
  (`ssh -L 7878:127.0.0.1:7878 serveur`). Ne jamais ouvrir 7878 sur Internet.
- Prendre un compte dédié à l'outil, avec un mot de passe solide et le niveau MJ juste nécessaire.
- Une requête par connexion, corps de 64 Ko au plus, 10 s pour l'envoyer. Chaque commande est notée dans
  le journal du serveur (`[soap] compte (IP, MJ n) : commande`, puis le résultat) et dans la table
  `log_gm` de la base des journaux (personnage 0, nom `SOAP <adresse IP>`) ; les mots de passe de
  `account create` et `account set password` y sont remplacés par `***`.

### Réponses

| Cas | HTTP | Corps |
|---|---|---|
| Commande réussie | 200 | `<ns1:executeCommandResponse><result>…</result>` ; chaque ligne finit par `&#xD;` + saut de ligne, comme la console d'AzerothCore |
| Commande refusée ou en échec (syntaxe, droits, commande inconnue, vide) | 500 | `SOAP-ENV:Fault`, `faultcode` `SOAP-ENV:Client`, `faultstring` et `detail` = message |
| Identifiants absents ou faux, compte inconnu ou banni | 401 | Fault `HTTP Error: 401 Unauthorized` (+ `WWW-Authenticate: Basic`) |
| Niveau MJ sous `SOAP.MinGmLevel` | 403 | Fault `HTTP Error: 403 Forbidden` |

`ns1` reprend l'espace de noms de la requête, `xsi` et `xsd` aussi (1999 par défaut, comme gSOAP).

### Exemple avec curl

```sh
curl -s -u admin:MotDePasse -H "Content-Type: text/xml; charset=utf-8" http://127.0.0.1:7878/ \
  -d '<?xml version="1.0" encoding="utf-8"?><SOAP-ENV:Envelope xmlns:SOAP-ENV="http://schemas.xmlsoap.org/soap/envelope/" xmlns:ns1="urn:AC"><SOAP-ENV:Body><ns1:executeCommand><command>server info</command></ns1:executeCommand></SOAP-ENV:Body></SOAP-ENV:Envelope>'
```

Sous Windows PowerShell, appeler `curl.exe` (pas l'alias `curl`) et mettre l'enveloppe dans un fichier
(`--data-binary "@requete.xml"`) pour éviter les soucis de guillemets. Réponse :

```xml
<?xml version="1.0" encoding="UTF-8"?>
<SOAP-ENV:Envelope xmlns:SOAP-ENV="http://schemas.xmlsoap.org/soap/envelope/" xmlns:SOAP-ENC="http://schemas.xmlsoap.org/soap/encoding/" xmlns:xsi="http://www.w3.org/1999/XMLSchema-instance" xmlns:xsd="http://www.w3.org/1999/XMLSchema" xmlns:ns1="urn:AC"><SOAP-ENV:Body SOAP-ENV:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><ns1:executeCommandResponse><result>ArchCore rev. 0.2.0 (windows, Release)&#xD;
Connected players: 1. Characters in world: 1.&#xD;
Connection peak: 2.&#xD;
Server uptime: 1 Hour(s) 5 Minute(s) 12 Second(s)&#xD;
</result></ns1:executeCommandResponse></SOAP-ENV:Body></SOAP-ENV:Envelope>
```

### Commandes

Une commande s'écrit avec ou sans `.` ou `/` devant, sans tenir compte de la casse. Les commandes de
gestion d'AzerothCore répondent avec leurs textes anglais d'origine, que les outils analysent :

| Commande | Niveau | Effet |
|---|---|---|
| `account create <compte> <mot de passe> [courriel]` | 3 | Crée un compte (lettres, chiffres, `_ - .`, 32 caractères au plus ; mot de passe de 64 au plus). Le courriel est accepté et ignoré. Réponse `Account created: <compte>` |
| `account set password <compte> <mdp> [<mdp>]` | 3 | Change le mot de passe (le second, s'il est donné, doit être identique). Réponse `The password was changed` |
| `account set gmlevel <compte> <niveau> [royaume]` | 3 | Niveau MJ du compte (jamais au-dessus du sien ; le royaume, `-1` chez AzerothCore, est ignoré). Un joueur en jeu le reçoit tout de suite |
| `account onlinelist` | 1 | Tableau des joueurs en ligne : compte, personnage, IP, niveau, niveau MJ (`Empty list` sans personne) |
| `ban account <compte> <durée> [raison]` | 3 | Bannit le compte (`account_banned`, auteur `SOAP <compte>`) et déconnecte ses personnages. Durée : `1d2h30m10s`, un nombre de secondes, `-1` ou `0` sans fin |
| `unban account <compte>` | 3 | Lève les bannissements en cours |
| `kick <personnage> [raison]` | 2 | Déconnecte un joueur en jeu, sans attente de reconnexion. Réponse `Player <nom> kicked.` |
| `announce <texte>` / `notify <texte>` | 2 | Annonce à tous les joueurs en jeu (sans sortie, comme AzerothCore) |
| `server info` | 1 | Version, `Connected players: n. Characters in world: n.`, pic de connexions, temps de fonctionnement |
| `server shutdown <délai>\|cancel`, `server restart <délai>\|cancel` | 3 | Compte à rebours de `/announce_restart` (délai en secondes ou `1h30m`) ; le serveur ne s'arrête à la fin qu'avec `World.ExitOnRestart = 1` |
| `commands` / `help` | 0 | Commandes permises au compte |

Toute autre commande passe par l'interpréteur des commandes MJ du jeu (sections 3 et 4), sans le `/` :
`SetExp 2`, `GetExp`, `chatban "Nom" 10`, `money 1000 "Nom"`, `move -479091,-66202 "Nom"`,
`announce_restart 5`, `siege`, `simloot 450 100`… Celles qui agissent sur le personnage du MJ
(`speed`, `additem`, `invincible`, `durability`, `recast`, `questreset`, `last_refine`, et `kill`,
`level`, `money`, `charisma`, `move` sans nom de personnage) répondent par un Fault « commande
réservée au jeu ». Les niveaux se changent avec `GM.CommandLevel.<commande>` ; pour une commande de
console, mots séparés par des points : `GM.CommandLevel.account.create = 3`.

Les commandes s'exécutent sous le verrou du monde, comme un paquet de joueur : elles ne bloquent pas
la boucle du jeu plus longtemps qu'une commande tapée en jeu. Une commande commencée va à son terme
même si le client se déconnecte.

## 7. Bonnes pratiques

- Garder les comptes MJ séparés des comptes de jeu, avec un mot de passe solide.
- Tester une commande sur un personnage de test avant de l'utiliser sur un joueur.
- Les noms de personnages doivent être exacts, accents compris.
