# Architecture du code

ArchCore est écrit en **Rust**. Un espace de travail Cargo regroupe deux crates, qui produisent un seul
exécutable : `archcore [login|world|all]` lance le serveur, les autres sous-commandes sont les outils
(voir [[Outils|FR-Outils]]). Le serveur repose sur `tokio` (réseau asynchrone) et `mysql_async` (MariaDB).

## Arborescence

```text
Cargo.toml                 espace de travail
crates/archcore-net/       protocole, sans dépendance au serveur
  src/packet.rs            codec AuPacket : en-tête de 19 octets, drapeaux, champs, mini-paquets
  src/schema.rs            types de paquets, schémas de champs relevés dans le client, codes d'opération
  src/blowfish.rs          Blowfish standard (mots en petit-boutiste, comme le client)
  src/framing.rs           trames chiffrées A1/B1 … AF/BF et découpage du flux TCP
  src/rc4.rs               RC4 dérivé par CryptoAPI (MD5 de la clé texte)
  src/text.rs              texte du protocole en Windows-1252
crates/archcore/           serveur et outils
  src/main.rs              point d'entrée et aiguillage des sous-commandes
  src/server.rs            charge la config, les modèles, les PNJ, lance les serveurs
  src/net.rs               une tâche par connexion : chiffrement, poignée de main 0x48, aiguillage
  src/ext.rs               extensions ArchRust : négociation HELLO / HELLO_ACK (type 0xAC)
  src/login.rs             comptes, liste des mondes, personnages, entrée dans le monde
  src/character.rs         modèles, lignes `characters`, facteurs, paquet AGPMCHARACTER
  src/config.rs            lecture de auth.conf / world.conf (modèles .dist, variables ARCHCORE_*)
  src/db.rs, log.rs
  src/world/               serveur monde
    mod.rs                 état partagé, entrée en jeu, actions des commandes MJ
    presence.rs            déplacement, visibilité entre joueurs, sauvegarde des positions
    npcs.rs                PNJ (npc_spawn), fonctions de npc.ini, paquet de vue, traduction des titres
    npc_events.rs          dialogue avec les PNJ
    npc_trade.rs           boutiques : liste, achat, vente
    monsters.rs            créatures (monster_spawn) : placement, caractéristiques, cycle de vie, paquet de vue
    combat.rs              attaque des monstres : approche, coups, mort, expérience, réapparition ; mort et résurrection du joueur
    ai.rs                  IA des monstres : agresseurs, riposte, poursuite, retour au point, patrouille
    loot.rs                butin : tirage (or, groupes, objets propres), objets au sol, ramassage
    equipment.rs           équipement : restrictions, remplacement, facteurs ; inventaire : déplacer, réunir, séparer, détruire
    recovery.rs            potions de vie et de mana, régénération hors combat
    travel.rs              parchemins de téléportation : village, aller-retour, destination
    regions.rs             régions sûres, rattachement, points de résurrection
    skills.rs              compétences : catalogue, points, maître des compétences, lancement, zones, dégâts
    effects.rs             effets durables : modificateurs, dégâts sur la durée, soins, passives
    map.rs                 carte serveur (map_segment) : sol infranchissable, régions
    items.rs               catalogue d'objets, inventaire, paquets AGPMITEM
    chat.rs, gm.rs         discussion et commandes MJ
    addon.rs               messages d'AddOn des extensions ArchRust
  src/tools/               outils : conf (migration), db_init, decrypt, import, i18n, sim, re
sql/                       schémas des trois bases
data/i18n/                 mémoire de traduction
data/re/                   relevés de rétro-ingénierie (schémas de paquets)
```

## Cheminement d'un paquet

1. `net.rs` accepte la connexion et lance une tâche qui lit le flux TCP.
2. `framing.rs` découpe le flux en trames et les déchiffre une fois la poignée de main faite ;
   `net.rs` traite lui-même la poignée de main (type 0x48).
3. `packet.rs` décode l'en-tête et les champs selon le schéma de `schema.rs`.
4. La connexion du service appelle le bon gestionnaire, par exemple `login.rs` pour AGPMLOGIN.
5. Le gestionnaire répond avec `session.send(&schéma, champs)`, qui encode, chiffre et envoie.

Les paquets d'une connexion sont traités un à un, dans l'ordre d'arrivée. Côté monde, l'état partagé
(joueurs en jeu, interdictions, taux) est protégé par un verrou : les paquets de tous les joueurs sont
traités l'un après l'autre.

## Deux serveurs

- **Login (11002)** : poignée de main, liste des mondes, authentification (SHA-256 en base),
  liste, création et suppression de personnages. À l'entrée dans le monde, il remet une clé
  d'authentification et l'adresse du serveur monde.
- **Monde (11008)** : vérifie la clé, envoie le personnage, son inventaire, sa position, l'heure du jeu
  et les PNJ proches, puis gère déplacement, discussion, commandes MJ, dialogues et boutiques.

Le détail des échanges est sur la page [[Protocole réseau|FR-Protocole-reseau]].

## Tests

```bash
cargo test
```

Les tests couvrent le codec de paquets (avec des octets de référence), Blowfish et RC4, le découpage
des trames, la configuration, les fonctions des PNJ, la boutique, l'inventaire, les commandes MJ, les extensions ArchRust et
les outils d'import et de traduction. Les scénarios complets se rejouent avec `archcore sim`.
