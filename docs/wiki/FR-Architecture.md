# Architecture du code

ArchCore est un seul programme Node.js (modules ES) qui peut lancer le serveur de login, le serveur
monde, ou les deux : `node src/index.js [login|world|all]`. Sa seule dépendance est le pilote `mariadb`.

## Arborescence

```text
src/
  index.js            point d'entrée : charge la config, les modèles, lance les serveurs
  net/                couche réseau commune
    blowfish.js       Blowfish standard (mots en petit-boutiste)
    packet.js         codec AuPacket : en-tête de 19 octets, champs, mini-paquets
    framing.js        trames chiffrées A1/B1 … AF/BF et découpage du flux TCP
    schema.js         types de paquets, schémas de champs, codes d'opération
    session.js        une connexion : chiffrement, envoi, état
    server.js         serveur TCP et routage par type de paquet
    startup-encryption.js  poignée de main (type 0x48)
  login/handlers.js   comptes, liste des mondes, personnages, entrée dans le monde
  world/handlers.js   entrée en jeu, keepalive, envoi du personnage et des PNJ
  world/npcs.js       PNJ chargés de npc_spawn, recherche par distance, paquet de vue
  common/             config, journal, RC4 CryptoAPI, paquets personnage, modèles
  db/pool.js          un pool MariaDB par base
sql/                  schémas des trois bases
tools/                outils hors serveur (voir [[Outils|FR-Outils]])
test/                 tests du protocole (node --test)
data/i18n/            mémoire de traduction
data/re/              relevés de rétro-ingénierie (schémas de paquets)
```

## Cheminement d'un paquet

1. `server.js` accepte la connexion et crée une `Session`.
2. `framing.js` découpe le flux TCP en trames et les déchiffre une fois la poignée de main faite.
3. `packet.js` décode l'en-tête et les champs selon le schéma de `schema.js`.
4. `server.js` appelle le gestionnaire enregistré pour ce type de paquet, par exemple `LoginHandlers.onLogin`.
5. Le gestionnaire répond avec `session.send(schéma, valeurs)`, qui encode, chiffre et envoie.

## Deux serveurs

- **Login (11002)** : poignée de main, liste des mondes, authentification (SHA-256 en base),
  liste, création et suppression de personnages. À l'entrée dans le monde, il remet une clé
  d'authentification et l'adresse du serveur monde.
- **Monde (11008)** : vérifie la clé, envoie le personnage, sa position, l'heure du jeu et les PNJ
  proches, puis confirme l'entrée en jeu.

Le détail des échanges est sur la page [[Protocole réseau|FR-Protocole-reseau]].

## Tests

```bash
npm test
```

Les tests couvrent le codec de paquets, Blowfish, le découpage des trames, les drapeaux sur 4 octets
du type 3 et le paquet d'apparition des PNJ.
