# Extensions ArchRust ↔ ArchCore

> **Spécification, version 1.0 — brouillon.** Rien n'est encore codé. Ce document fait foi pour les deux
> projets : le client [ArchRust](https://github.com/BlaMacfly/ArchRust) et le serveur ArchCore.

ArchRust, le client réécrit en Rust, prend en charge des **AddOns Lua** sur le modèle de World of Warcraft.
Certains AddOns ont besoin du serveur : échanger des données entre joueurs, ou interroger le serveur.
Ce document définit un **protocole d'extension** qui s'ajoute au protocole d'ArchLord sans le modifier.
La version 1 couvre deux fonctions :

1. la **négociation de capacités** : le client et le serveur conviennent des extensions qu'ils utilisent ;
2. le **canal de messages d'AddOn** : l'équivalent de `C_ChatInfo.SendAddonMessage` et de l'événement
   `CHAT_MSG_ADDON` de WoW.

## 1. Principes

- **Compatibilité totale avec le client d'origine.** Le serveur n'envoie jamais de paquet d'extension à une
  connexion qui n'a pas négocié les extensions. Le client d'origine continue de fonctionner sans rien voir.
- **Compatibilité avec les autres serveurs.** ArchRust doit rester utilisable sur un serveur sans
  extensions : faute de réponse à sa demande, il passe en mode classique.
- **Le serveur fait autorité.** Il renseigne lui-même l'émetteur d'un message, applique les limites de
  taille et de débit, et refuse ce qui n'est pas permis. Rien de ce que déclare le client n'est cru sur parole.
- **Versionné et extensible.** Chaque fonction est une capacité nommée et versionnée. Une capacité
  inconnue est ignorée, jamais traitée comme une erreur.

## 2. Transport

Les extensions utilisent un **type de paquet AuPacket dédié : `0xAC`** (« ArchCore »). Il a le même format
que les autres paquets (en-tête de 19 octets, drapeaux, champs, pied `0x6B`). Il est chiffré en Blowfish
comme le reste de la session, et passe sur les connexions habituelles (login 11002, monde 11008).

Pourquoi `0xAC` : le répartiteur de paquets du client d'origine (`FUN_14012c930`) n'accepte que les types
`0x01` à `0x77` et ignore tous les autres sans erreur. Un type au-delà de `0x77` ne peut donc entrer en
collision avec aucun module, présent ou futur, du client d'origine. Même reçu par erreur, il serait ignoré.

### Schéma `ARCHCORE_EXT` (type `0xAC`, drapeaux sur 4 octets)

| N° | Type | Nom | Rôle |
|---|---|---|---|
| 0 | UINT8 | `op` | opération (section 3) |
| 1 | UINT32 | `request_id` | numéro choisi par le client, renvoyé dans la réponse ou l'erreur |
| 2 | UINT16 | `version_major` | version majeure du protocole d'extension |
| 3 | UINT16 | `version_minor` | version mineure |
| 4 | MEMORY_BLOCK | `software` | nom et version du logiciel, en UTF-8, 64 octets au plus (`ArchRust 0.3.0`) |
| 5 | MEMORY_BLOCK | `capabilities` | liste de capacités `nom/version` séparées par `;`, en ASCII |
| 6 | UINT16 | `max_payload` | taille maximale d'une charge de message d'AddOn, en octets |
| 7 | UINT16 | `rate_burst` | limite de débit : messages permis d'un coup |
| 8 | UINT16 | `rate_per_sec` | limite de débit : messages rendus par seconde |
| 9 | MEMORY_BLOCK | `prefix` | préfixe de l'AddOn (section 5) |
| 10 | UINT8 | `channel` | portée du message (section 5) |
| 11 | MEMORY_BLOCK | `target` | destinataire : nom de personnage (Windows-1252) |
| 12 | INT32 | `sender_cid` | CID de l'émetteur, renseigné par le serveur |
| 13 | MEMORY_BLOCK | `sender_name` | nom de l'émetteur (Windows-1252), renseigné par le serveur |
| 14 | MEMORY_BLOCK | `payload` | charge du message, octets libres |
| 15 | UINT8 | `result` | code de résultat (section 6) |
| 16–31 | — | — | réservés aux versions futures |

Règles de décodage :
- un champ absent (bit de drapeau à 0) vaut sa valeur par défaut : 0 ou vide ;
- un bloc mémoire vide n'active pas son drapeau : une charge vide est donc un champ 14 absent ;
- un champ réservé présent est ignoré, ce qui permet aux versions futures d'en ajouter.

## 3. Opérations

| `op` | Nom | Sens | Champs |
|---|---|---|---|
| 0 | `HELLO` | client → serveur | 2, 3, 4, 5 |
| 1 | `HELLO_ACK` | serveur → client | 2, 3, 4, 5, 6, 7, 8 |
| 2 | `ADDON_SEND` | client → serveur | 1 (facultatif), 9, 10, 11 (selon la portée), 14 |
| 3 | `ADDON_RECV` | serveur → client | 9, 10, 11 (murmure), 12, 13, 14 |
| 4 | `RESULT` | serveur → client | 1, 15 |

Une opération inconnue est ignorée et journalisée par celui qui la reçoit.

## 4. Négociation de capacités

### Déroulement

La négociation a lieu **sur chaque connexion** (login puis monde), indépendamment, juste après la
poignée de main chiffrée (type `0x48`) et avant tout autre paquet :

```text
client                                   serveur
  ── 0x48 handshake (inchangé) ──────────▶
  ◀───────────────────── 0x48 COMPLETE ──
  ── 0xAC HELLO (versions, logiciel, capacités demandées) ──▶
  ◀── 0xAC HELLO_ACK (versions, logiciel, capacités accordées, limites) ──
  ── suite normale du protocole (AGPMWORLD, GAME_ENTER…) ──▶
```

- Le client envoie **un seul** `HELLO` par connexion. Un second `HELLO` est ignoré.
- Le serveur répond par `HELLO_ACK` avec **l'intersection** des capacités demandées et de celles qu'il
  sait gérer. Une capacité est accordée à la version la plus haute connue des deux côtés.
- Le serveur n'envoie **jamais** de paquet `0xAC` à une connexion qui n'a pas envoyé de `HELLO`. Les clients
  d'origine, qui n'en envoient pas, ne voient donc jamais d'extension.
- Si le client ne reçoit pas de `HELLO_ACK` dans les **3 secondes**, il considère le serveur sans extensions
  et poursuit normalement. Il peut aussi poursuivre sans attendre : les paquets de jeu reçus entre-temps
  restent valides.
- Les extensions sont valables pour la durée de la connexion. Sur le serveur monde, le `HELLO` peut arriver
  avant `GAME_ENTER` : l'accord est alors rattaché au personnage dès son entrée en jeu.

### Versions

- **Version du protocole** (`version_major.version_minor`), actuellement **1.0**. Une version mineure
  n'ajoute que des champs ou des opérations facultatifs. Une version majeure différente rend les deux
  côtés incompatibles : le serveur répond alors `HELLO_ACK` avec une liste de capacités vide.
- **Version d'une capacité** : un entier après le nom (`addon-msg/1`). Il augmente quand le comportement
  de la capacité change.

### Capacités

| Capacité | Version 1 | Rôle |
|---|---|---|
| `addon-msg` | définie ici | canal de messages d'AddOn |
| `addon-data` | réservée | requêtes de données serveur par les AddOns (équivalent des API `C_*`) |
| `saved-vars` | réservée | `SavedVariables` gardées sur le serveur, communes à plusieurs machines |
| `server-script` | réservée | interfaces d'AddOn pilotées par des scripts côté serveur |

Exemple de `HELLO` : `capabilities = "addon-msg/1;saved-vars/1"`. Si le serveur ne gère que `addon-msg/1`,
il répond `capabilities = "addon-msg/1"` avec ses limites.

## 5. Canal de messages d'AddOn (`addon-msg/1`)

### Préfixe

- 1 à 16 octets ASCII imprimables (`0x21` à `0x7E`), sans espace. La casse compte.
- Les préfixes commençant par `AC.` sont **réservés au serveur** : un client qui en envoie un reçoit
  `NOT_ALLOWED`.
- Le serveur relaie tous les préfixes valides. C'est le client qui ne remet à un AddOn que les préfixes
  qu'il a enregistrés (comme `C_ChatInfo.RegisterAddonMessagePrefix` dans WoW).

### Charge

- De 0 à `max_payload` octets (**255** en version 1, comme dans WoW). Le contenu est libre, binaire compris.
- Le serveur ne lit ni ne modifie la charge. Il ne la journalise pas : seuls l'émetteur, le préfixe, la
  portée et la taille peuvent l'être.

### Portées (`channel`)

| Valeur | Portée | Destinataires | Version 1 |
|---|---|---|---|
| 1 | `WHISPER` | le personnage nommé dans `target` | ✅ |
| 2 | `SAY` | les joueurs à portée de vue de l'émetteur, émetteur compris | ✅ |
| 3 | `PARTY` | le groupe de l'émetteur, émetteur compris | répond `NOT_AVAILABLE` tant que les groupes n'existent pas |
| 4 | `GUILD` | la guilde de l'émetteur, émetteur compris | idem, en attendant les guildes |
| 5 | `RAID` | réservé | `NOT_AVAILABLE` |
| 6 | `CHANNEL` | réservé (canaux nommés) | `NOT_AVAILABLE` |
| 7 | `SERVER` | le serveur lui-même, pour les capacités futures (`addon-data`, `server-script`) | `NOT_AVAILABLE` en version 1 |

Seules les connexions qui ont obtenu `addon-msg` reçoivent des `ADDON_RECV`. Un joueur sur le client
d'origine n'en reçoit jamais : un murmure qui lui est adressé renvoie `TARGET_NO_EXTENSION`.

### Relais

À la réception d'un `ADDON_SEND`, le serveur vérifie, dans l'ordre :

1. que la connexion a obtenu `addon-msg` et que le joueur est en jeu (sinon le paquet est ignoré) ;
2. le préfixe ;
3. la taille de la charge ;
4. la limite de débit ;
5. la portée et le destinataire.

Il envoie ensuite un `ADDON_RECV` à chaque destinataire, avec `sender_cid` et `sender_name` qu'il remplit
lui-même. Pour un murmure, le champ `target` de `ADDON_RECV` reprend le nom du destinataire.

### Limite de débit

- Un **seau à jetons** par connexion : `rate_burst` jetons au plus (**20** en version 1), rechargé de
  `rate_per_sec` jetons par seconde (**10** en version 1). Chaque `ADDON_SEND` coûte un jeton.
- Sans jeton, le message est abandonné et le serveur répond `THROTTLED`.
- Les valeurs sont annoncées dans `HELLO_ACK` : le client doit lisser ses envois lui-même, à la manière de
  ChatThrottleLib, plutôt que de compter sur les refus du serveur.

## 6. Résultats (`RESULT`)

Pour limiter le trafic, le serveur **ne confirme pas** un envoi réussi : il n'envoie `RESULT` qu'en cas
d'erreur, avec le `request_id` du message refusé (0 si le client n'en a pas donné).

| `result` | Nom | Cause |
|---|---|---|
| 0 | `OK` | réservé (non envoyé en version 1) |
| 1 | `INVALID_PREFIX` | préfixe vide, trop long ou avec des caractères interdits |
| 2 | `MESSAGE_TOO_LONG` | charge supérieure à `max_payload` |
| 3 | `THROTTLED` | limite de débit atteinte |
| 4 | `TARGET_OFFLINE` | destinataire du murmure introuvable ou hors ligne |
| 5 | `NOT_AVAILABLE` | portée non gérée par ce serveur |
| 6 | `NOT_IN_GROUP` | portée `PARTY`/`GUILD` sans groupe ou sans guilde |
| 7 | `NOT_ALLOWED` | préfixe réservé ou envoi interdit au joueur |
| 8 | `TARGET_NO_EXTENSION` | le destinataire n'a pas la capacité `addon-msg` |

## 7. Côté ArchRust : API Lua proposée

Elle reprend les noms de WoW pour que les AddOns existants s'adaptent facilement :

```lua
C_ChatInfo.RegisterAddonMessagePrefix(prefix)      -- true si accepté (128 préfixes au plus)
C_ChatInfo.IsAddonMessagePrefixRegistered(prefix)  -- booléen
C_ChatInfo.GetRegisteredAddonMessagePrefixes()     -- liste
C_ChatInfo.SendAddonMessage(prefix, text, chatType, target)
  -- chatType : "WHISPER", "SAY", "PARTY", "GUILD" ; rend un code (Enum.SendAddonMessageResult)
C_ArchCore.IsAvailable()                           -- extensions négociées avec ce serveur ?
C_ArchCore.GetCapabilities()                       -- { ["addon-msg"] = 1, … }
C_ArchCore.GetLimits()                             -- { maxPayload = 255, burst = 20, perSecond = 10 }
```

Événements :
- `CHAT_MSG_ADDON(prefix, text, channel, sender, target)`, remis uniquement pour les préfixes enregistrés ;
- `ARCHCORE_EXTENSIONS_READY(serverSoftware, major, minor)`, une fois `HELLO_ACK` reçu sur le serveur monde ;
- `ARCHCORE_ADDON_MESSAGE_FAILED(prefix, result)`, à la réception d'un `RESULT`.

`SendAddonMessage` rend un succès dès que le message est mis en file. Le client applique la limite de débit
annoncée avant d'envoyer, et rend `AddonMessageThrottle` s'il doit refuser.

## 8. Sécurité et vie privée

- Le serveur ne fait confiance à aucun champ d'identité envoyé par le client : l'émetteur est toujours le
  personnage de la connexion.
- Les préfixes `AC.` permettent au serveur de parler à des AddOns officiels sans risque d'usurpation.
- Les charges ne sont ni lues, ni stockées, ni journalisées par le serveur.
- Une interdiction de discussion (`/chatban`) ne bloque pas les messages d'AddOn en version 1 : ils ne sont
  pas visibles en clair. Ce choix est à confirmer (section 10).
- Un abus répété de la limite de débit est journalisé, pour que les MJ puissent intervenir.

## 9. Mise en œuvre prévue

**ArchCore**
- Schéma `ARCHCORE_EXT` et constantes dans `crates/archcore-net/src/schema.rs`.
- Traitement de `HELLO` dans `net.rs`, à côté de la poignée de main `0x48`, pour le login comme pour le
  monde, avec l'accord gardé dans la session.
- Module `crates/archcore/src/world/addon.rs` : contrôles, seau à jetons, relais.
- Tests unitaires, et scénario `archcore sim addon` : deux clients simulés s'échangent des messages en
  murmure et en `SAY`, et un client d'origine simulé n'en reçoit aucun.

**ArchRust**
- Envoi de `HELLO` après la poignée de main, attente de `HELLO_ACK` (3 s au plus).
- API Lua et événements de la section 7 dans `archrust-addons`.
- Lissage des envois selon les limites annoncées.

**Code commun.** Les deux projets décrivent aujourd'hui le protocole chacun de leur côté (`archcore-net`,
`archrust-protocol`). Pour l'extension, une seule source est préférable : ArchRust pourrait dépendre de la
crate `archcore-net` (dépendance git), qui porterait le schéma `0xAC`. Cela suppose des licences
compatibles (ArchRust est en GPL-3.0).

## 10. Questions ouvertes

1. **Licence d'ArchCore**, préalable au partage de `archcore-net` avec ArchRust.
2. **`/chatban` et AddOns** : faut-il aussi bloquer les messages d'AddOn d'un joueur réduit au silence ?
3. **Portée `SAY`** : rayon de vue complet (8000 unités) ou distance de parole plus courte ?
4. **Messages au serveur (`SERVER`)** : premier usage visé — statistiques détaillées, quêtes, état du monde ?
5. **AddOns officiels** : le serveur doit-il pouvoir annoncer ou proposer des AddOns (liste, versions) ?
