<p align="center">
  <img src="docs/assets/ArchRust.png" alt="Illustration du client ArchRust" width="340">
</p>

<h1 align="center">ArchLord Client Rust</h1>

**Client ArchLord 64 bits réécrit en Rust**, avec rendu 3D `wgpu`, interface `egui`, connexion au
serveur ArchCore, AddOns Lua et prise en charge des patchs graphiques HD. Le code source du client
est dans [`ArchRust/`](ArchRust/).

Cette branche `client` ajoute le client Rust au dépôt [ArchCore](https://github.com/BlaMacfly/ArchCore).
Elle conserve le code du serveur Node.js dans le dépôt; la documentation du serveur et son guide
d’installation sont dans le [README de la branche `main`](https://github.com/BlaMacfly/ArchCore/blob/main/README.md)
et dans le [wiki ArchCore](https://github.com/BlaMacfly/ArchCore/wiki).

> ArchRust ne contient aucune donnée propriétaire du jeu. Il faut fournir une installation du
> client ArchLord 64 bits que vous possédez.

## Fonctionnalités

| Domaine | État |
|---|---|
| Connexion réseau : poignée de main, compte, personnages et entrée dans le monde | ✅ validée avec ArchCore |
| Packs `Reference.Dat` / `Data.Dat`, tables INI et cryptographie | ✅ formats principaux pris en charge |
| Modèles RenderWare `.ecl` / `.dff`, matériaux, textures et peaux | ✅ lecteurs et rendu de base |
| Textures `.tx1` : DXT1/3/5, BGRA et palettes | ✅ décodage pris en charge |
| Monde : MagPack, aPLib, attributs, textures et géométrie de secteur | ✅ 256/256 secteurs lus dans la tuile d’Anchorville |
| Rendu `wgpu`, caméra orbitale et interface `egui` | 🟡 moteur et affichage de base en place |
| AddOns Lua : manifestes `.toc`, frames, événements et variables sauvegardées | ✅ API de base |
| Patchs HD : textures, modèles, fichiers et archives `.ahp` | ✅ format et chargement pris en charge |
| Déplacement des joueurs (type 0x40) : position en base, apparition mutuelle, rediffusion à portée | ✅ validé avec deux clients |
| Animations, combat, objets et compétences | ⏳ à développer |

Le client est un projet en cours. Les fonctionnalités marquées « de base » ne représentent pas
encore une parité complète avec le client officiel.

## Prérequis

- Windows 64 bits et Rust stable avec la cible MSVC (voir [`ArchRust/rust-toolchain.toml`](ArchRust/rust-toolchain.toml)).
- Un client ArchLord 64 bits extrait dans `client/ArchLord/Game` à la racine du dépôt, ou un autre
  dossier fourni avec l’option `--game`.
- Pour jouer en ligne, un serveur ArchCore en fonctionnement. Le guide serveur décrit MariaDB,
  l’import des données et les ports réseau.

Le dépôt ne publie ni les archives du jeu, ni les exécutables de l’éditeur, ni les données déchiffrées.

## Compiler et lancer

Dans un terminal PowerShell, placez-vous dans le projet Rust :

```powershell
cd ArchRust
cargo build --release
```

Lancer l’interface graphique :

```powershell
cargo run --release -- play
```

Le chemin par défaut du client est `../client/ArchLord/Game`, relatif au dossier `ArchRust`.
Si votre installation est ailleurs, transmettez son chemin :

```powershell
cargo run --release -- --game "D:\Jeux\ArchLord\Game" play
```

Pour se connecter automatiquement à un serveur local, avec un compte et un personnage déjà créés :

```powershell
cargo run --release -- play --connect "test:test@127.0.0.1:11002/Testeur"
```

La forme de `--connect` est `utilisateur:motdepasse@hôte:port/personnage`. Le port de connexion
par défaut est `11002`. Ne placez pas un vrai mot de passe dans l’historique d’un terminal partagé.

## Commandes

Toutes les commandes suivantes s’exécutent depuis `ArchRust/`.

```powershell
# Compiler le workspace
cargo build --release

# Lister les ressources et examiner un modèle
cargo run --release -- inspect pack character
cargo run --release -- inspect model character a0000005.ecl

# Examiner une texture, une table ou une tuile du monde
cargo run --release -- inspect texture a0057aab
cargo run --release -- inspect table charactertemplateclient.ini --dir ini
cargo run --release -- inspect tile b002024x.ma2

# Exporter une texture décodée en PNG
cargo run --release -- extract a0057aab .\a0057aab.png

# Tester le protocole login et l’entrée dans le monde sans ouvrir la fenêtre 3D
cargo run --release -- login test test Testeur

# Lister les AddOns et gérer les patchs
cargo run --release -- addons
cargo run --release -- patch list
cargo run --release -- patch check patches\mon-patch
cargo run --release -- patch build patches\mon-patch mon-patch.ahp
```

Pour relire la géométrie de tous les secteurs d’une tuile avec les données locales :

```powershell
cargo run -p archrust-data --example inspect_sector -- "..\client\ArchLord\Game" 002024 all
```

Remplacez `002024` par le numéro de tuile voulu. Pour n’inspecter qu’un secteur, indiquez ses
coordonnées, par exemple `325,389`.

## Contrôles dans la fenêtre 3D

| Action | Contrôle |
|---|---|
| Faire pivoter la caméra | Maintenir le bouton droit et déplacer la souris |
| Zoomer ou dézoomer | Molette |
| Déplacer la caméra | `W`, `A`, `S`, `D` ou `Z`, `Q`, `S`, `D` |
| Déplacer la caméra avec la souris | Maintenir le bouton du milieu et déplacer la souris |
| Ouvrir ou fermer la console | touche accent grave `` ` `` |
| Fermer la console | `Échap` |

La discussion accepte les commandes commençant par `/`. Les commandes disponibles dépendent de
l’implémentation actuelle et des réponses du serveur.

## Organisation du code

```text
ArchRust/
  apps/archrust/          exécutable : play, login, inspect, extract, addons, patch
  crates/
    archrust-crypto/      RC4, Blowfish et AES-GCM
    archrust-protocol/    paquets AuPacket, trames et poignée de main
    archrust-data/        packs, tables INI, aPLib et lecteurs RenderWare
    archrust-netclient/   client réseau Tokio : login et événements du monde
    archrust-engine/      fenêtre winit, rendu wgpu, caméra et interface egui
    archrust-addons/      hôte Lua 5.4 et API d’AddOns
    archrust-patch/       manifestes, priorités et archives de patch
  addons/HelloArch/       petit AddOn d’exemple
  patches/                patchs locaux chargés par le client
  docs/FORMATS.md         notes de rétro-ingénierie sur les formats
```

## AddOns Lua

Un AddOn est un dossier dans `addons/` avec un fichier `.toc` qui énumère les scripts à charger :

```text
## Title: Hello ArchLord
## Interface: 10000
## SavedVariables: HelloArchDB
core.lua
```

L’API reprend certains noms de World of Warcraft : `CreateFrame`, `SetPoint`, `SetScript`,
`RegisterEvent`, `SlashCmdList`, `C_Timer.After`, `UnitName` et `SendChatMessage`. Les variables
annoncées par `SavedVariables` sont sauvegardées à la fermeture. Le bac à sable Lua ne reçoit pas
les bibliothèques `io`, `os` et `package`.

## Patchs graphiques HD

Les patchs sont des dossiers sous `patches/` ou des archives `.ahp`. Un manifeste `patch.toml`
décrit les ressources remplacées :

```toml
[patch]
id = "anchorville-hd"
name = "Anchorville HD"
version = "1.0.0"
priority = 100

[[textures]]
match = "a0057aab"
file = "textures/a0057aab.png"

[[models]]
match = "character/a0000001.ecl"
file = "models/a0000001.glb"

[[files]]
path = "ini/npc.ini"
file = "npc.ini"
```

Les ressources des patchs sont recherchées par priorité; les données d’origine servent de repli.
Les formats admis et les règles de priorité sont documentés dans [`ArchRust/README.md`](ArchRust/README.md).

## Documentation et contribution

- [Guide détaillé d’ArchRust](ArchRust/README.md)
- [Formats de fichiers reconstruits](ArchRust/docs/FORMATS.md)
- [README du serveur ArchCore sur `main`](https://github.com/BlaMacfly/ArchCore/blob/main/README.md)
- [Wiki ArchCore](https://github.com/BlaMacfly/ArchCore/wiki)

Les changements au client se font dans `ArchRust/`. Avant une demande de fusion, indiquez la
version du client ArchLord utilisée, les données nécessaires pour reproduire le problème et les
commandes de compilation ou de validation exécutées. N’ajoutez pas de données du jeu, de fichiers
`target/`, de mots de passe ni de tables client déchiffrées.

## Licence et marques

Le client ArchRust est distribué sous **GPL-3.0-or-later**; voir [`ArchRust/LICENSE.md`](ArchRust/LICENSE.md).
ArchLord et ses éléments restent la propriété de leurs détenteurs de droits. ArchRust et ArchCore
sont des projets indépendants, non affiliés à l’éditeur du jeu, dédiés à l’étude et à la préservation.
