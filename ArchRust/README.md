# ArchRust

**Réécriture en Rust (x86-64) du client ArchLord 64 bits**, avec un système d'AddOns Lua façon
World of Warcraft et un format de patch graphique HD pour les développeurs.

Les formats de fichiers et le protocole réseau ont été reconstruits par rétro-ingénierie de
l'exécutable du client (Ghidra piloté par [REA](https://github.com/morluto/rea)) et validés contre
les vrais fichiers du jeu et l'émulateur serveur [ArchCore](../ArchCore). Ce dépôt ne contient
**aucune donnée du jeu** : il faut posséder son propre client. Licence : [GPL-3.0-or-later](LICENSE.md).

## État

| Domaine | État |
|---|---|
| Chiffrement (RC4 CryptoAPI, Blowfish petit-boutiste, AES-128-GCM des index) | ✅ validé |
| Protocole AuPacket (en-tête 19 octets, schémas, trames, poignée de main) | ✅ validé avec ArchCore |
| Connexion → liste des mondes → compte → personnages → entrée dans le monde → PNJ en vue | ✅ `archrust login` |
| Packs `Reference.Dat`/`Data.Dat` (character, object, textures, octree…) | ✅ tous les index lus |
| Modèles RenderWare (`.ecl`/`.dff` : frames, géométries, peaux, matériaux) | ✅ 200/200 testés |
| Textures `.tx1` (TXD RenderWare, DXT1/3/5, BGRA, palette) | ✅ 66/66 testées |
| Tuiles du monde (`MagPack` + aPLib) : secteurs, textures, attributs | ✅ décompression ; géométrie en cours |
| Rendu wgpu (terrain, modèles, caméra orbitale), interface egui | 🟡 premier affichage |
| AddOns Lua (`.toc`, frames, événements, slash, minuteurs, SavedVariables) | ✅ API de base |
| Patchs HD (`patch.toml`, dossier ou `.ahp`, priorités, motifs) | ✅ textures/modèles/fichiers |
| Animations `.ean`, déplacement, combat, objets, compétences | ⏳ à faire |

## Démarrage rapide

Prérequis : Rust stable (MSVC), un client ArchLord 64 bits dans `../client/ArchLord/Game`, et pour
jouer, ArchCore lancé (MariaDB + `npm start`).

```bash
cargo build --release
cargo run --release -- inspect pack character              # liste un pack
cargo run --release -- inspect model character a0000005.ecl
cargo run --release -- extract a0057aab a0057aab.png       # texture décodée
cargo run --release -- login test test Testeur             # flux complet sans affichage
cargo run --release -- play --connect test:test@127.0.0.1:11002/Testeur
cargo run --release -- addons                              # vérifie les AddOns
cargo run --release -- patch build patches/mon-patch mon-patch.ahp
```

Dans le jeu : clic droit pour orbiter, molette pour zoomer, ZQSD/WASD pour déplacer la caméra,
`` ` `` ouvre la console Lua, la fenêtre de discussion accepte les commandes `/`.

## Architecture

```text
crates/
  archrust-crypto     RC4 (MD5 CryptoAPI), Blowfish LE, AES-128-GCM
  archrust-protocol   AuPacket, schémas de champs, trames A1/B1, poignée de main
  archrust-data       tables ini, packs, MagPack + aPLib, RenderWare (DFF/TXD/secteurs)
  archrust-netclient  client tokio : login, monde, événements, commandes
  archrust-addons     Lua 5.4 (mlua), .toc, frames, événements, slash, SavedVariables
  archrust-patch      manifeste TOML, pile de patchs, archives .ahp
  archrust-engine     winit + wgpu + egui : rendu, terrain, entités, interface, liaison réseau
apps/archrust         exécutable : play / login / inspect / extract / addons / patch
addons/HelloArch      AddOn d'exemple
docs/FORMATS.md       formats de fichiers reconstruits
```

## AddOns

Un AddOn vit dans `addons/<Nom>/` avec un fichier `<Nom>.toc` :

```text
## Title: Hello ArchLord
## Interface: 10000
## SavedVariables: HelloArchDB
core.lua
```

L'API reprend les noms de WoW : `CreateFrame`, `frame:SetPoint`, `frame:SetScript("OnEvent", f)`,
`frame:RegisterEvent("PLAYER_LOGIN")`, `SlashCmdList`, `SLASH_X1`, `print`, `GetTime`, `UnitName`,
`UnitHealth`, `SendChatMessage`, `C_Timer.After`… Les événements émis : `ADDON_LOADED`,
`ADDONS_LOADED`, `PLAYER_LOGIN`, `PLAYER_ENTERING_WORLD`, `PLAYER_LOGOUT`, `CHAT_MSG_SAY`,
`CHAT_MSG_WHISPER`, `CHAT_MSG_PARTY`. Le bac à sable n'expose ni `io`, ni `os`, ni `package`.

## Patchs HD

Un patch est un dossier (ou une archive `.ahp`) avec un `patch.toml` :

```toml
[patch]
id = "anchorville-hd"
name = "Anchorville HD"
version = "1.0.0"
priority = 100

[[textures]]
match = "a0057aab"            # nom RenderWare, motif * accepté
file = "textures/a0057aab.png" # PNG/DDS de n'importe quelle taille

[[models]]
match = "character/a0000001.ecl"
file = "models/a0000001.glb"

[[files]]
path = "ini/npc.ini"
file = "npc.ini"
```

Les patchs du dossier `patches/` sont empilés par priorité décroissante ; le premier qui fournit une
ressource l'emporte, sinon les données d'origine sont utilisées.
