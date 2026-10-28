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
| `/weather [clear\|rain\|snow]` | 2 | Météo pour tous les joueurs en jeu (pluie, neige, temps clair) ; sans argument, la météo et l'heure du jeu. Commande propre à ArchCore |
| `/durability <valeur\|-1>` | 3 | Fixe la durabilité de l'équipement porté qui s'use (`-1` : pleine, 0 : cassé) ; pour essayer l'usure et la réparation. Commande propre à ArchCore |
| `/questreset <TID>` | 3 | Efface la quête TID (en cours ou terminée) du journal du MJ ; visible à la prochaine connexion. Commande propre à ArchCore |
| `/recast` | 3 | Remet à zéro, côté serveur, les délais de relance des compétences du MJ (le client peut encore afficher le sien). Commande propre à ArchCore |
| `/last_refine` | 3 | Relance la dernière recette raffinée avec succès depuis la connexion (« Use last refine recipe » du panneau), comme l'op 6 du client : ingrédients pris dans l'inventaire, or payé, résultat dans la fenêtre de raffinage. Refusée sans raffinage réussi |
| `/level <niveau> ["nom"]` | 3 | Fixe le niveau du personnage connecté (1 à 120) : expérience remise à zéro, PV et PM rendus, points de compétence recalculés ; sans nom, sur soi. Commande propre à ArchCore |
| `/money <montant> ["nom"]` | 3 | Fixe l'or du personnage connecté (de 0 à 1 000 000 000) ; sans nom, sur soi. Commande propre à ArchCore |
| `/charisma <points> ["nom"]` | 3 | Donne des points de charisme au personnage connecté (1 à 1 000 000, multipliés par le taux `Rate.Charisma` et les bonus de charisme de ses effets, sauf celui des boss) ; sans nom, à soi. Commande propre à ArchCore (Archonia n'en a pas), en attendant le combat entre joueurs |
| `/announce_restart <minutes> <secondes>` | 3 | Annonce à tous les joueurs, rappel chaque minute, compte à rebours sur les dernières secondes. `0 0` annule |
| `/move x,z` | 1 | Téléporte le MJ en (x, z) et envoie les PNJ du nouvel endroit |

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
| ✅ `/SetUniqueDrop <x>` | Multiplicateur de butin unique |
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
| ✅ `/speed <1-10>` | Multiplie sa vitesse de déplacement (ajout d'ArchCore, absent du panneau) |
| ✅ `/weather [clear\|rain\|snow]` | Météo de tout le monde, ou météo et heure du jeu (ajout d'ArchCore, absent du panneau) |
| ✅ `/durability <valeur\|-1>` | Fixe la durabilité de son équipement porté (ajout d'ArchCore, absent du panneau) |
| ✅ `/money <montant> ["nom"]` | Fixe l'or d'un personnage connecté (ajout d'ArchCore, absent du panneau) |
| ✅ `/charisma <points> ["nom"]` | Donne des points de charisme à un personnage connecté (ajout d'ArchCore, absent du panneau) |
| ✅ `/level <niveau> ["nom"]` | Fixe le niveau d'un personnage connecté (ajout d'ArchCore, absent du panneau) |
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

## 5. Bonnes pratiques

- Garder les comptes MJ séparés des comptes de jeu, avec un mot de passe solide.
- Tester une commande sur un personnage de test avant de l'utiliser sur un joueur.
- Les noms de personnages doivent être exacts, accents compris.
