<p align="center">
  <img src="docs/assets/archcore-logo.png" alt="ArchCore" width="220">
</p>

<h1 align="center">ArchCore</h1>

<p align="center">
  <b>ArchLord server emulator — Émulateur serveur ArchLord</b><br>
  Rust · MariaDB · client reverse engineering / rétro-ingénierie du client
</p>

<p align="center">
  <a href="#-english">🇬🇧 English</a> ·
  <a href="#-français">🇫🇷 Français</a> ·
  <a href="https://github.com/BlaMacfly/ArchCore/wiki">📖 Wiki</a> ·
  <a href="https://discord.gg/JCyrsTm7fR">💬 Discord</a>
</p>

<p align="center">
  <a href="https://discord.gg/JCyrsTm7fR"><img src="https://img.shields.io/badge/Discord-ArchCore-5865F2?logo=discord&logoColor=white" alt="Discord ArchCore"></a>
</p>

---

## 🇬🇧 English

**ArchCore** is a server emulator (login + world) for the **64-bit ArchLord** client. It is written in **Rust** and stores its data in **MariaDB**. The protocol was rebuilt by
reverse engineering the client executable with Ghidra, through [REA](https://github.com/morluto/rea), and
from the client's data tables.

> ArchCore is based on the 64-bit client of the Archonia server.
>
> **Episode:** ArchCore loads the world of **Episode 8, "The Dark Revelation"** (2012), plus Archonia's own
> extensions. Every original system the Archonia client knows is programmed, up to sieges, the ArchLord and
> Shrines; Archonia's own additions (race war, arena, rifts, battle pass…) are left out. See the wiki's [Episode](https://github.com/BlaMacfly/ArchCore/wiki/EN-Episode) page.

### Project status

| Feature | Status |
|---|---|
| Encrypted handshake (Blowfish) | ✅ verified with the real client |
| World list, account login | ✅ verified |
| Character list, creation and deletion | ✅ list and creation screen verified; all 10 starting classes (Dragon Scion included) |
| World entry, per-race starting village | ✅ verified |
| NPC spawning around the player (567 NPCs) | ✅ verified |
| French translation of the client and NPCs | 🟡 in progress |
| Player movement, position saved | ✅ verified |
| NPC dialog | ✅ verified |
| Players seeing each other | 🟡 verified with the simulator, to confirm in game |
| NPC shops (list, buy) | ✅ verified in game; selling verified with the simulator |
| Character inventory | 🟡 shown in game |
| Monsters, combat, drops, equipment, potions, scrolls, skills, masteries | 🟡 verified with the simulator, to confirm in game |
| Chat (party, guild, shouts, chat rooms), GM level and commands | 🟡 verified with the simulator, to confirm in game |
| Bank, trading, quests, parties, teleport portals, automatic reconnection | 🟡 verified with the simulator, to confirm in game |
| Guilds, guild battles, alliances, guild warehouse | 🟡 verified with the simulator, to confirm in game |
| Item conversion, runes, refining, wear and repair | 🟡 verified with the simulator, to confirm in game |
| Player versus player, remission, wanted criminals | 🟡 verified with the simulator, to confirm in game |
| Production (gathering, cooking, alchemy) | 🟡 verified with the simulator, to confirm in game |
| Friends and ignore list, mail, auction house, stylists | 🟡 verified with the simulator, to confirm in game |
| Mounts, summons and pets, world time and weather | 🟡 verified with the simulator, to confirm in game |
| Shrines, Battle Square, Battleground | 🟡 verified with the simulator, to confirm in game |
| Castle sieges, ArchLord, region taxes | 🟡 verified with the simulator, to confirm in game |
| Scion evolution at level 20, metamorphosis | ⏳ to do |

### Quick start

Requirements: Rust 1.85 or later ([rustup](https://rustup.rs)), MariaDB, and a 64-bit ArchLord client extracted to
`../client/ArchLord/Game`. Everything goes through a single executable, `archcore`.

```bash
cargo build --release                         # builds target/release/archcore
db\start-mariadb.cmd                          # local MariaDB instance (Windows)
archcore db-init --account test test          # schema + test account
archcore decrypt                              # decrypts the client tables
archcore import                               # imports the tables into MariaDB
archcore                                      # login on 11002, world on 11008
archcore sim client test test Testeur         # replays a full client, without the game
```

Run the commands from the repository root (where `config/` lives), using
`target/release/archcore` or `cargo run --release --` in place of `archcore`.

To connect the game, see the wiki's [Installation](https://github.com/BlaMacfly/ArchCore/wiki/EN-Installation) page.

### Documentation

The [wiki](https://github.com/BlaMacfly/ArchCore/wiki) covers installation, configuration, code
architecture, the network protocol, the database, the tools and the translation. The same pages
are in [`docs/wiki`](docs/wiki).

### Ecosystem

| Project | Role |
|---|---|
| [ArchRust](https://github.com/BlaMacfly/ArchRust) (own repository) | 64-bit ArchLord client rewritten in Rust, with Lua add-ons |
| [ArchCoreAdmin](https://github.com/BlaMacfly/ArchCoreAdmin) | In-game GM panel: a Lua add-on for ArchRust with a button for every ArchCore GM command, server replies read back into the panel |
| [ArchHome](https://github.com/BlaMacfly/ArchHome) | HomeRPG ArchLord: single-player, offline ArchLord in one folder — local ArchCore, portable MariaDB, ArchRust and a one-button Rust launcher |
| [ArchForge](https://github.com/BlaMacfly/ArchForge) | Database editor: creatures, NPCs, items, shops, loot, quests, skills, teleports, with SQL preview and export to the content layer (`content/*.sql`) |

### Community

Join the official Discord to follow the project, ask questions and offer help:
https://discord.gg/JCyrsTm7fR

### How to contribute

Contributions to ArchCore and the [ArchRust](https://github.com/BlaMacfly/ArchRust) client are welcome!
You don't need to be an expert in Rust to help with the project.

**Areas where you can help**

- **Reverse engineering**: investigate the original client, network protocol, packet structures and
  game data formats.
- **Server development**: help implement authentication, world systems, NPCs, combat and other
  gameplay mechanics.
- **Client development**: improve rendering, resource loading, networking, UI and compatibility with
  original game assets.
- **Testing**: reproduce bugs, test multiplayer interactions and report issues with clear steps.
- **Documentation**: document discoveries, protocols, file formats and existing systems.

**Getting started**

1. Explore the repository and read the existing documentation (the
   [wiki](https://github.com/BlaMacfly/ArchCore/wiki) and [`docs/wiki`](docs/wiki)).
2. Check the open [GitHub issues](https://github.com/BlaMacfly/ArchCore/issues) for known bugs and
   planned features.
3. If you have an idea or want to work on a specific system, open an issue or start a discussion
   (on GitHub or on the [Discord](https://discord.gg/JCyrsTm7fR)) before making major changes.
4. For code contributions, create a dedicated branch and submit a pull request describing your
   changes and testing.

**Development guidelines**

- Keep contributions focused and easy to review.
- Avoid committing proprietary game assets or copyrighted client files.
- Document reverse-engineering findings whenever possible.
- Discuss significant architectural changes before implementing them.

If you're interested in contributing but aren't familiar with Rust, don't hesitate to get in touch.
Experience with networking, software architecture, reverse engineering or game development can be
just as valuable.

Every contribution helps move the project forward!

### License

ArchCore is free software under the [GNU GPL v3 or later](LICENSE) (`GPL-3.0-or-later`), like the
ArchRust client.


---

## 🇫🇷 Français

**ArchCore** est un émulateur de serveur (login + monde) pour le client **ArchLord 64 bits**. Il est écrit en **Rust** et stocke ses données dans **MariaDB**. Le protocole a été
reconstruit par rétro-ingénierie de l'exécutable du client avec Ghidra, via [REA](https://github.com/morluto/rea),
et à partir des tables de données du client.

> ArchCore est basé sur le client 64 bits du serveur Archonia.
>
> **Épisode :** ArchCore charge le monde de l'**épisode 8, « The Dark Revelation »** (2012), avec les
> extensions propres à Archonia. Tous les systèmes d'origine que connaît le client Archonia sont programmés,
> jusqu'aux sièges, à l'ArchLord et aux Shrines ; les ajouts propres à Archonia (guerre des races, arène, failles,
> passe de combat…) sont laissés de côté. Voir la page [Épisode](https://github.com/BlaMacfly/ArchCore/wiki/FR-Episode) du wiki.

### État du projet

| Fonction | État |
|---|---|
| Poignée de main chiffrée (Blowfish) | ✅ validée avec le vrai client |
| Liste des mondes, connexion au compte | ✅ validée |
| Liste, création et suppression de personnages | ✅ liste et écran de création validés ; les 10 classes de départ (Dragon Scion compris) |
| Entrée dans le monde, village de départ par race | ✅ validée |
| Apparition des PNJ autour du joueur (567 PNJ) | ✅ validée |
| Traduction française du client et des PNJ | 🟡 en cours |
| Déplacement du joueur, position sauvegardée | ✅ validé |
| Dialogue avec les PNJ | ✅ validé |
| Joueurs visibles entre eux | 🟡 validé au simulateur, à confirmer en jeu |
| Boutiques des PNJ (liste, achat) | ✅ validées en jeu ; revente validée au simulateur |
| Inventaire du personnage | 🟡 affiché en jeu |
| Monstres, combat, butin, équipement, potions, parchemins, compétences, maîtrises | 🟡 validés au simulateur, à confirmer en jeu |
| Discussion (groupe, guilde, cris, salons), niveau et commandes MJ | 🟡 validés au simulateur, à confirmer en jeu |
| Banque, échange, quêtes, groupes, portails de téléportation, reconnexion automatique | 🟡 validés au simulateur, à confirmer en jeu |
| Guildes, combats de guildes, alliances, entrepôt de guilde | 🟡 validés au simulateur, à confirmer en jeu |
| Conversion des objets, runes, raffinage, usure et réparation | 🟡 validés au simulateur, à confirmer en jeu |
| Combat entre joueurs, rémission, avis de recherche | 🟡 validés au simulateur, à confirmer en jeu |
| Production (récolte, cuisine, alchimie) | 🟡 validée au simulateur, à confirmer en jeu |
| Amis et ignorés, courrier, maison des ventes, coiffeurs | 🟡 validés au simulateur, à confirmer en jeu |
| Montures, invocations et familiers, heure du monde et météo | 🟡 validés au simulateur, à confirmer en jeu |
| Shrines, Battle Square, Battleground | 🟡 validés au simulateur, à confirmer en jeu |
| Sièges des châteaux, ArchLord, taxes des régions | 🟡 validés au simulateur, à confirmer en jeu |
| Évolution du Scion au niveau 20, métamorphose | ⏳ à faire |

### Démarrage rapide

Prérequis : Rust 1.85 ou plus ([rustup](https://rustup.rs)), MariaDB, et un client ArchLord 64 bits extrait dans
`../client/ArchLord/Game`. Tout passe par un seul exécutable, `archcore`.

```bash
cargo build --release                         # produit target/release/archcore
db\start-mariadb.cmd                          # instance MariaDB locale (Windows)
archcore db-init --account test test          # schéma + compte de test
archcore decrypt                              # déchiffre les tables du client
archcore import                               # importe les tables dans MariaDB
archcore                                      # login sur 11002, monde sur 11008
archcore sim client test test Testeur         # rejoue un client complet, sans le jeu
```

Les commandes se lancent depuis la racine du dépôt (là où se trouve `config/`), avec
`target/release/archcore` ou `cargo run --release --` à la place de `archcore`.

Pour connecter le jeu, voir la page [Installation](https://github.com/BlaMacfly/ArchCore/wiki/FR-Installation) du wiki.

### Documentation

Le [wiki](https://github.com/BlaMacfly/ArchCore/wiki) détaille l'installation, la configuration,
l'architecture du code, le protocole réseau, la base de données, les outils et la traduction.
Les mêmes pages sont dans [`docs/wiki`](docs/wiki).

### Écosystème

| Projet | Rôle |
|---|---|
| [ArchRust](https://github.com/BlaMacfly/ArchRust) (dépôt à part) | Client ArchLord 64 bits réécrit en Rust, avec AddOns Lua |
| [ArchCoreAdmin](https://github.com/BlaMacfly/ArchCoreAdmin) | Panneau MJ en jeu : un AddOn Lua pour ArchRust, avec un bouton pour chaque commande MJ d'ArchCore et les réponses du serveur relues dans le panneau |
| [ArchHome](https://github.com/BlaMacfly/ArchHome) | HomeRPG ArchLord : ArchLord en solo, hors ligne, dans un seul dossier — ArchCore en local, MariaDB portable, ArchRust et un lanceur Rust à un seul bouton |
| [ArchForge](https://github.com/BlaMacfly/ArchForge) | Éditeur de la base de données : créatures, PNJ, objets, boutiques, butin, quêtes, compétences, téléportation, avec aperçu du SQL et export vers la couche de contenu (`content/*.sql`) |

### Communauté

Rejoignez le Discord officiel pour suivre le projet, poser vos questions et proposer votre aide :
https://discord.gg/JCyrsTm7fR

### Contribuer

Les contributions à ArchCore et au client [ArchRust](https://github.com/BlaMacfly/ArchRust) sont les
bienvenues ! Pas besoin d'être expert en Rust pour aider le projet.

**Où vous pouvez aider**

- **Rétro-ingénierie** : étudier le client d'origine, le protocole réseau, la structure des paquets
  et les formats des données du jeu.
- **Développement serveur** : aider à implémenter l'authentification, les systèmes du monde, les PNJ,
  le combat et les autres mécaniques de jeu.
- **Développement client** : améliorer le rendu, le chargement des ressources, le réseau, l'interface
  et la compatibilité avec les ressources du jeu d'origine.
- **Tests** : reproduire des bugs, tester les interactions multijoueur et signaler les problèmes avec
  des étapes claires.
- **Documentation** : documenter les découvertes, les protocoles, les formats de fichiers et les
  systèmes existants.

**Premiers pas**

1. Explorez le dépôt et lisez la documentation existante (le
   [wiki](https://github.com/BlaMacfly/ArchCore/wiki) et [`docs/wiki`](docs/wiki)).
2. Consultez les [*issues* GitHub](https://github.com/BlaMacfly/ArchCore/issues) ouvertes pour les
   bugs connus et les fonctions prévues.
3. Si vous avez une idée ou voulez travailler sur un système précis, ouvrez une *issue* ou lancez la
   discussion (sur GitHub ou sur le [Discord](https://discord.gg/JCyrsTm7fR)) avant tout changement
   majeur.
4. Pour une contribution de code, créez une branche dédiée et proposez une *pull request* décrivant
   vos changements et vos tests.

**Règles de développement**

- Gardez les contributions ciblées et faciles à relire.
- Ne commitez jamais de ressources propriétaires du jeu ni de fichiers du client protégés par le
  droit d'auteur.
- Documentez autant que possible les résultats de rétro-ingénierie.
- Discutez des changements d'architecture importants avant de les implémenter.

Si contribuer vous intéresse sans que Rust vous soit familier, n'hésitez pas à nous contacter. Une
expérience en réseau, en architecture logicielle, en rétro-ingénierie ou en développement de jeux
est tout aussi précieuse.

Chaque contribution fait avancer le projet !

### Licence

ArchCore est un logiciel libre sous licence [GNU GPL v3 ou ultérieure](LICENSE) (`GPL-3.0-or-later`),
comme le client ArchRust.

