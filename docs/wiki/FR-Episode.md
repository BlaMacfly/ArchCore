# Épisode du jeu

Chaque grande mise à jour d'ArchLord sortait sous forme d'**épisode**. Cette page dit où se situe
ArchCore, en séparant deux choses : le **monde** chargé (données du jeu) et les **systèmes** déjà
programmés.

> **Repère.** ArchCore charge le monde de l'**épisode 8, « The Dark Revelation »** (Webzen,
> 26 septembre 2012), avec en plus les extensions propres au serveur Archonia, dont le client 64 bits
> sert de base. Les systèmes de jeu programmés couvrent pour l'instant le socle de la sortie d'origine ;
> ceux des épisodes suivants sont à rattraper (voir plus bas).

## Les épisodes d'ArchLord

| Épisode | Nom | Date | Apport principal |
|---|---|---|---|
| — | Sortie d'origine | 2005 (Corée), 2006 (Europe) | Humains, orcs, elfes de lune ; combat, quêtes, titre d'ArchLord |
| 2 | Season of Siege | 2007 | Sièges de châteaux ; passage en accès gratuit |
| 3 | Spirits Awakening | août 2008 | 3 zones, plus de 80 compétences, pouvoirs de l'ArchLord, panoplies |
| 4 | Chaotic Frontier | mars 2010 | Donjons (6 mondes), niveaux 31 à 70 |
| 5 | nom non retrouvé | 2009-2010 | Classe Swashbuckler (octobre 2009, rattachement à l'épisode 5 non confirmé) |
| 6 | DragonScion | 16 décembre 2010 | 4ᵉ race : Scion, puis Slayer, Orbiter ou Summoner au niveau 20 |
| 7 | Ascent From Hell | juillet 2011 | Niveau maximal de 99 à 120, zone Heaven's Hell, siège revu |
| 8 | The Dark Revelation | 26 septembre 2012 | Shrines (combats d'équipes, pièces de Shrine), villes humaine et orque refaites, emplacement de compétence en plus |

Le service officiel a fermé le 1ᵉʳ janvier 2014. Les noms des épisodes 1 et 5 n'ont pas été retrouvés.

## Ce que contiennent les données

Données du client (`data/client-ini`, client Archonia) et tables du serveur (`--server`) :

| Marqueur | Épisode | Présent |
|---|---|---|
| Classe Swashbuckler | 5 | ✅ objets et classe connus d'ArchCore |
| Race Dragon Scion et ses classes | 6 | ✅ modèles, zones « 드래곤시온 », compétences |
| Barème d'expérience jusqu'au niveau 120, monstres jusqu'au niveau 120 | 7 | ✅ `level_exp` vient du client (le barème des tables du serveur s'arrête à 100) |
| Zone Heaven's Hell (« 헤븐즈 헬의 옥좌 ») | 7 | ✅ client et serveur |
| Cinq Shrines élémentaires (feu, eau, vent, foudre, poison), chacune découpée par race ; potions « Shrine » | 8 | ✅ client et serveur |
| Battle Ground (deux terrains) | mise à jour Webzen 2012 | ✅ régions |

**Ajouts d'Archonia**, absents du jeu d'origine et des tables du serveur : 31 régions propres au client
(noms anglais ou provisoires : The Shattered Firmament, extension Archonia du 24 avril 2026 ; Omega Reactor, The Elemental Forge,
Spookywood Forest, Throne of Fire, Lava Temple Ruins, Guild vs Guild Area…), les failles
(`Rifts.xml`), le passe de combat (`battlepass.xml`), l'arène de Battle Royale, les Oreads et Elixirs,
l'armure Nightfall. ArchCore ne les programme pas : ils restent hors du périmètre tant qu'aucune décision
ne les y fait entrer.

## Systèmes programmés, par épisode

| Épisode | Système | État dans ArchCore |
|---|---|---|
| Origine | Connexion, personnages, entrée en jeu, PNJ, déplacement, vue | ✅ validés en jeu |
| Origine | Combat, monstres, butin, équipement, potions, parchemins, compétences | 🟡 validés au simulateur |
| Origine | Quêtes, groupes, banque, échange, boutiques, portails de téléportation | 🟡 validés au simulateur |
| Origine | Guildes et combats de guildes, discussion de groupe | ⏳ à faire |
| Origine | Battle Square | ⏳ à faire |
| 2 | Sièges de châteaux | ⏳ à faire |
| 3 | Pouvoirs de l'ArchLord | ⏳ à faire |
| 4 | Donjons de Chaotic Frontier | 🟡 zones et monstres chargés ; règles propres aux donjons à vérifier |
| 6 | Dragon Scion jouable | 🟡 données et compétences présentes ; création à confirmer en jeu |
| 7 | Niveau 120, Heaven's Hell | 🟡 barème et monstres chargés ; siège revu à faire |
| 8 | Shrines et pièces de Shrine | ⏳ à faire |
| — | Maîtrises, compétences héroïques | ⏳ à faire |

L'ordre de rattrapage suit la [[feuille de route|FR-Feuille-de-route]] : guildes, puis sièges, puis
pouvoirs de l'ArchLord et Shrines.

## Sources

- [Wikipedia — ArchLord](https://en.wikipedia.org/wiki/ArchLord)
- [Engadget — passage en gratuit, Season of Siege](https://www.engadget.com/2006-12-06-archlord-moves-to-free-play-codemasters-desperate.html)
- [RPGWatch — actualités ArchLord (Spirits Awakening)](https://rpgwatch.com/games/archlord-127/news.html)
- [elitepvpers — Chaotic Frontier, épisode 4](https://www.elitepvpers.com/forum/archlord/502635-webzen-archlord-chaotic-frontier-episode-4-info.html)
- [Webzen — Swashbuckler (2009)](https://company.webzen.com/en/pr/pressview?page=93&seq=274)
- [Archlord: Episode VI – DragonScion](https://mmorpguk.wordpress.com/2010/12/09/archlord-episode-vi-dragonscion/)
- [MMOBomb — Episode 7 Ascent From Hell](https://www.mmobomb.com/news/webzen-all-in-one-updates-and-events)
- [Webzen — The Dark Revelation (2012)](https://company.webzen.com/en/pr/pressview?page=8&seq=311)
- [Archonia — The Shattered Firmament](https://archonia.net/news/150/archonia%20expansion%20release%20-%20the%20shattered%20firmament?culture=en)
