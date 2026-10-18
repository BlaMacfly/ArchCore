# Administration (MJ / Admin)

Cette page décrit les commandes de maître du jeu (MJ) que le client sait envoyer, et comment les utiliser.

> ⚠️ **État actuel d'ArchCore** : le serveur ne gère pas encore la discussion ni les commandes MJ.
> Les commandes ci-dessous sont celles que le **client** envoie. Elles ne produisent aucun effet tant
> qu'ArchCore ne les a pas implémentées. Le suivi se fait sur la page [[Feuille de route|FR-Feuille-de-route]].

## 1. Donner les droits MJ à un compte

Le niveau MJ est stocké dans la colonne `gm_level` de la table `archcore_auth.account`. La valeur 0
correspond à un joueur normal. Plus le nombre est élevé, plus le compte a de droits.

Créer un compte MJ, ou changer le niveau d'un compte existant :

```bash
node tools/db-init.mjs --account <identifiant> <mot_de_passe> <niveau>
# exemple : node tools/db-init.mjs --account admin MotDePasseSolide 3
```

Ou directement en SQL :

```sql
UPDATE archcore_auth.account SET gm_level = 3 WHERE username = 'admin';
```

Le compte doit se reconnecter pour que le changement compte. C'est le serveur qui décide si une
commande est autorisée selon ce niveau : le client se contente de l'envoyer.

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

Le panneau lit le niveau MJ envoyé par le serveur. Tant qu'ArchCore ne l'envoie pas, il affiche 0.
Le client lit aussi un argument de lancement `admin` ; son effet exact reste à confirmer.

## 3. Liste des commandes

Notation : `"nom"` = nom de personnage entre guillemets ; `<n>` = nombre entier ; `<x>` = nombre décimal.

### Taux du serveur

Un taux est un multiplicateur : `1.0` = normal, `2.0` = double.

| Commande | Effet |
|---|---|
| `/SetExp <x>` | Multiplicateur d'expérience |
| `/SetGold <x>` | Multiplicateur d'or gagné |
| `/SetDrop <x>` | Multiplicateur de butin |
| `/SetUniqueDrop <x>` | Multiplicateur de butin unique |
| `/SetCharisma <x>` | Multiplicateur de points de charisme |
| `/SetPrestigeRate <x>` | Multiplicateur de prestige |
| `/SetChatBuff <0\|1>` | Désactive (0) ou active (1) le bonus de discussion |
| `/GetExp`, `/GetGold`, `/GetDrop`, `/GetUniqueDrop`, `/GetCharisma` | Affiche la valeur actuelle du taux |

Exemple : `/SetExp 2.0` puis `/GetExp` pour vérifier.

### Modération

| Commande | Effet |
|---|---|
| `/chatban "nom" <minutes>` | Interdit la discussion pendant N minutes ; `0` lève l'interdiction |
| `/kill "nom"` | Tue le personnage |
| `/kill_explosion <effet> "nom"` | Pose un effet d'explosion à la mort d'un personnage ; sans nom, sur soi |
| `/kill_explosion -1 "nom"` | Retire cet effet d'un personnage |
| `/kill_explosion` | Retire l'effet sur soi |
| `/rename_char "ancien" "nouveau"` | Renomme un personnage |

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
| `/create "nom" <TID> <quantité> <0\|1>` | Donne un objet au personnage. `TID` = identifiant de l'objet ; le dernier nombre rend l'objet non échangeable (1) |
| `/last_refine` | Réutilise la dernière recette de raffinage |
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
| `/region …` | Modifie les réglages d'une région (réduction de dégâts, de potions, limite d'incantation). Utiliser le panneau, qui construit la commande |

### Système

| Commande | Effet |
|---|---|
| `/announce_restart <minutes> <secondes>` | Annonce un redémarrage, avec un compte à rebours final |
| `/refresh_lua_scripts` | Recharge les scripts LUA du serveur |
| `/reload_ip_players` | Recharge la liste d'adresses IP |
| `/log_activity` | Écrit le journal d'activité |
| `/cast_cap <0-100>` | Limite de vitesse d'incantation |
| `/cooldown_cap <0-100>` | Limite de réduction des temps de recharge |
| `/test <minutes>` | Commande de test : donne des points et règle le délai d'apparition des boss |

## 4. Bonnes pratiques

- Garder les comptes MJ séparés des comptes de jeu, avec un mot de passe solide.
- Tester une commande sur un personnage de test avant de l'utiliser sur un joueur.
- Les noms de personnages doivent être exacts, accents compris.
