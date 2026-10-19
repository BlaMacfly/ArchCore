# Extensions ArchRust ↔ ArchCore

> **Spécification, version 1.1 — validée le 2026-10-19** (décisions en section 11). Les deux côtés sont en
> place (section 10). Ce document fait foi pour les deux projets : le client
> [ArchRust](https://github.com/BlaMacfly/ArchRust) et le serveur ArchCore.

ArchRust, le client réécrit en Rust, prend en charge des **AddOns Lua** sur le modèle de World of Warcraft.
Certains AddOns ont besoin du serveur : échanger des données entre joueurs, ou interroger le serveur.
Ce document définit un **protocole d'extension** qui s'ajoute au protocole d'ArchLord sans le modifier.
La version 1 couvre trois fonctions :

1. la **négociation de capacités** : le client et le serveur conviennent des extensions qu'ils utilisent ;
2. le **canal de messages d'AddOn** : l'équivalent de `C_ChatInfo.SendAddonMessage` et de l'événement
   `CHAT_MSG_ADDON` de WoW ;
3. depuis la version 1.1, la **langue du client** : le serveur sert ses textes (noms de PNJ, de monstres,
   de quêtes, d'objets…) dans la langue choisie par le joueur (section 6).

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
| 15 | UINT8 | `result` | code de résultat (section 7) |
| 16 | MEMORY_BLOCK | `language` | langue, code BCP 47 en ASCII (`fr`, `pt-BR`, 16 octets au plus) : souhaitée par le client, appliquée par le serveur — v1.1 |
| 17 | MEMORY_BLOCK | `languages` | langues proposées par le serveur, séparées par `;` — v1.1 |
| 18 | MEMORY_BLOCK | `text_kind` | sorte de texte demandée, en ASCII (section 6) — v1.1 |
| 19 | MEMORY_BLOCK | `text_ids` | identifiants demandés : UINT32 petit-boutistes à la suite — v1.1 |
| 20 | MEMORY_BLOCK | `texts` | textes rendus : pour chacun, UINT32 identifiant, UINT16 longueur, UTF-8 — v1.1 |
| 21–31 | — | — | réservés aux versions futures |

Règles de décodage :
- un champ absent (bit de drapeau à 0) vaut sa valeur par défaut : 0 ou vide ;
- un bloc mémoire vide n'active pas son drapeau : une charge vide est donc un champ 14 absent ;
- un champ réservé présent est ignoré, ce qui permet aux versions futures d'en ajouter.

## 3. Opérations

| `op` | Nom | Sens | Champs |
|---|---|---|---|
| 0 | `HELLO` | client → serveur | 2, 3, 4, 5, 16 (facultatif, v1.1) |
| 1 | `HELLO_ACK` | serveur → client | 2, 3, 4, 5, 6, 7, 8, 16 et 17 (si `language` est accordée) |
| 2 | `ADDON_SEND` | client → serveur | 1 (facultatif), 9, 10, 11 (selon la portée), 14 |
| 3 | `ADDON_RECV` | serveur → client | 9, 10, 11 (murmure), 12, 13, 14 |
| 4 | `RESULT` | serveur → client | 1, 15 |
| 5 | `SET_LANGUAGE` | client → serveur | 1 (facultatif), 16 — v1.1 |
| 6 | `LANGUAGE_ACK` | serveur → client | 1, 16 (langue appliquée), 17 — v1.1 |
| 7 | `TEXT_REQUEST` | client → serveur | 1 (facultatif), 18, 19 — v1.1 |
| 8 | `TEXT_REPLY` | serveur → client | 1, 16 (langue servie), 18, 20 — v1.1 |

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

- **Version du protocole** (`version_major.version_minor`), actuellement **1.1** (la 1.1 ajoute les champs
  16 à 20, les opérations 5 à 8 et le résultat 9 ; un client ou un serveur 1.0 les ignore). Une version
  mineure n'ajoute que des champs ou des opérations facultatifs. Une version majeure différente rend les
  deux côtés incompatibles : le serveur répond alors `HELLO_ACK` avec une liste de capacités vide.
- **Version d'une capacité** : un entier après le nom (`addon-msg/1`). Il augmente quand le comportement
  de la capacité change.

### Capacités

| Capacité | Version 1 | Rôle |
|---|---|---|
| `addon-msg` | définie ici | canal de messages d'AddOn |
| `language` | définie ici (v1.1, section 6) | langue du client et textes localisés du serveur |
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
| 2 | `SAY` | les joueurs à portée de discussion de l'émetteur, émetteur compris : le même rayon que la discussion normale (`World.ViewRadius`, 8000 unités par défaut) | ✅ |
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
2. que le joueur n'est pas sous le coup d'une interdiction de discussion (sinon `NOT_ALLOWED`) ;
3. le préfixe ;
4. la taille de la charge ;
5. la limite de débit ;
6. la portée et le destinataire.

Il envoie ensuite un `ADDON_RECV` à chaque destinataire, avec `sender_cid` et `sender_name` qu'il remplit
lui-même. Pour un murmure, le champ `target` de `ADDON_RECV` reprend le nom du destinataire.

### Limite de débit

- Un **seau à jetons** par connexion : `rate_burst` jetons au plus (**20** en version 1), rechargé de
  `rate_per_sec` jetons par seconde (**10** en version 1). Chaque `ADDON_SEND` coûte un jeton.
- Sans jeton, le message est abandonné et le serveur répond `THROTTLED`.
- Les valeurs sont annoncées dans `HELLO_ACK` : le client doit lisser ses envois lui-même, à la manière de
  ChatThrottleLib, plutôt que de compter sur les refus du serveur.

## 6. Langue et textes localisés (`language/1`)

Le protocole d'ArchLord ne transporte pas la langue du joueur : le client d'origine lit ses textes dans
son dossier de langue (`Game/ini`, `Game/ini_fr`…) et le serveur ne sait rien de ce choix. Les textes que le
serveur envoie lui-même (noms des PNJ et des monstres dans les paquets de vue, et demain les quêtes, les
objets…) sont donc dans une seule langue, celle de `World.Language`. La capacité `language` corrige cela.

### Langue de la connexion

- Le client joint sa langue au `HELLO` (champ 16) : un code **BCP 47** (ISO 639-1, région facultative :
  `fr`, `de`, `pt-BR`, `zh-CN`). Le serveur le normalise (`FR-ca` → `fr-CA`).
- `HELLO_ACK` rend au champ 16 la **langue appliquée** et au champ 17 les **langues proposées** par le
  serveur (table `language` de la base monde, voir [[Traduction|FR-Traduction]]). La langue appliquée est
  le code demandé s'il est proposé, sinon sa langue de base (`fr-CA` → `fr`), sinon la langue par défaut du
  serveur (`World.Language`).
- `SET_LANGUAGE` (champ 16) change la langue en cours de connexion, sur le login comme sur le monde ; le
  serveur répond `LANGUAGE_ACK` (langue appliquée, langues proposées). Un code mal formé vaut `RESULT`
  `INVALID_REQUEST` ; une langue inconnue n'est pas une erreur : le serveur applique sa langue par défaut
  et le dit dans `LANGUAGE_ACK`.
- Le souhait **suit le compte** : le serveur l'enregistre (`account.language`) à l'authentification et à
  chaque `SET_LANGUAGE`. Une connexion sans langue (client d'origine, ArchRust en « automatique ») reçoit
  la dernière langue enregistrée sur le compte, sinon `World.Language`. Comme le `HELLO` du monde précède
  `GAME_ENTER`, son `HELLO_ACK` ne peut annoncer que la langue par défaut : si la langue du compte s'applique
  finalement, le serveur monde envoie un `LANGUAGE_ACK` (champ 1 à 0) à l'entrée en jeu avec la langue
  réellement appliquée.
- Les textes déjà envoyés ne sont pas renvoyés : un changement en jeu vaut pour les PNJ et les monstres
  qui apparaîtront ensuite, et pour les `TEXT_REQUEST` suivants.

### Repli d'un texte

Pour chaque texte, le serveur cherche dans l'ordre : la ligne `localized_text` dans la langue appliquée,
dans sa langue de base, la traduction des titres de PNJ (`data/i18n/npc-titles-<langue>.json`, noms de PNJ
et de monstres seulement), la ligne en anglais, enfin le texte d'origine du client. Un texte qu'il ne
connaît pas du tout est rendu vide.

### Textes à la demande (`TEXT_REQUEST` → `TEXT_REPLY`)

Le client demande des textes par **sorte** (champ 18) et **identifiants** (champ 19, 1 à 64 UINT32) ; le
serveur rend chaque identifiant demandé, dans l'ordre, avec son texte dans la langue appliquée (champs 16,
18, 20). Sortes de la version 1 (même nom que la colonne `kind` de `localized_text`) :

| `text_kind` | Identifiant | Texte d'origine |
|---|---|---|
| `npc_name` | `npc_spawn.id` (CID du PNJ − `0x40000000`) | `npc.ini` |
| `monster_name` | TID du monstre | `CharacterTemplatePublic.ini` |
| `item_name`, `item_desc` | TID de l'objet | `itemdatatable.txt` |
| `quest_name`, `quest_desc` | TID de la quête | `questtemplate.ini` |
| `skill_name` | TID de la compétence | `SKILLTEMPLATE.ini` |
| `region_name` | identifiant de région | `RegionTemplate.ini` |
| `npc_dialog` | réservé | — |

Une sorte inconnue, une liste vide ou de plus de 64 identifiants, ou une demande sans la capacité
`language`, valent `RESULT` `INVALID_REQUEST`. Sur la connexion de login, `TEXT_REQUEST` répond
`NOT_AVAILABLE` : les textes se demandent au monde.

## 7. Résultats (`RESULT`)

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
| 7 | `NOT_ALLOWED` | préfixe réservé, ou joueur sous le coup d'une interdiction de discussion (`/chatban`) |
| 8 | `TARGET_NO_EXTENSION` | le destinataire n'a pas la capacité `addon-msg` |
| 9 | `INVALID_REQUEST` | v1.1 : code de langue mal formé, sorte de texte inconnue, trop d'identifiants, ou capacité `language` non accordée |

## 8. Côté ArchRust : API Lua

Elle reprend les noms de WoW pour que les AddOns existants s'adaptent facilement :

```lua
C_ChatInfo.RegisterAddonMessagePrefix(prefix)      -- true si accepté (128 préfixes au plus)
C_ChatInfo.IsAddonMessagePrefixRegistered(prefix)  -- booléen
C_ChatInfo.GetRegisteredAddonMessagePrefixes()     -- liste
C_ChatInfo.SendAddonMessage(prefix, text, chatType, target)
  -- chatType : "WHISPER", "SAY", "PARTY", "GUILD" ; rend un code (Enum.SendAddonMessageResult)
C_ArchCore.IsAvailable()                           -- extensions négociées avec ce serveur ?
C_ArchCore.GetCapabilities()                       -- { ["addon-msg"] = 1, ["language"] = 1, … }
C_ArchCore.GetLimits()                             -- { maxPayload = 255, burst = 20, perSecond = 10 }
C_ArchCore.GetLanguage()                           -- langue appliquée par le serveur ("fr"), nil sans la capacité
C_ArchCore.GetLanguages()                          -- langues proposées : { "en", "fr", "de", … }
C_ArchCore.RequestTexts(kind, ids)                 -- demande de textes (section 6) ; rend le numéro de demande, nil si refusée
```

Événements :
- `CHAT_MSG_ADDON(prefix, text, channel, sender, target)`, remis uniquement pour les préfixes enregistrés ;
- `ARCHCORE_EXTENSIONS_READY(serverSoftware, major, minor)`, une fois `HELLO_ACK` reçu sur le serveur monde ;
- `ARCHCORE_ADDON_MESSAGE_FAILED(prefix, result, resultName, requestId)`, à la réception d'un `RESULT`
  (pour une demande de textes, `prefix` vaut `<texts:kind>`) ;
- `ARCHCORE_LANGUAGE_CHANGED(language, languages)`, après un `LANGUAGE_ACK` (langues séparées par `;`) ;
- `ARCHCORE_TEXTS(requestId, language, kind, texts)`, à la réception d'un `TEXT_REPLY` ; `texts` est une
  table identifiant → texte.

`SendAddonMessage` rend un succès dès que le message est mis en file. Le client applique la limite de débit
annoncée avant d'envoyer, et rend `AddonMessageThrottle` s'il doit refuser.

Côté interface, la fenêtre **Options** d'ArchRust (bouton « Options » de l'écran de connexion, ou Échap en
jeu) propose la langue parmi celles de la page [[Traduction|FR-Traduction]], ou « Automatique ». Le choix
est enregistré dans `saved-variables/options.toml`, annoncé au `HELLO` des deux connexions, et envoyé par
`SET_LANGUAGE` quand il change en jeu ; la fenêtre affiche la langue appliquée et celles que le serveur propose.

## 9. Sécurité et vie privée

- Le serveur ne fait confiance à aucun champ d'identité envoyé par le client : l'émetteur est toujours le
  personnage de la connexion.
- Les préfixes `AC.` sont réservés aux messages du serveur lui-même, pour qu'aucun joueur ne puisse se faire passer pour lui.
- Les charges ne sont ni lues, ni stockées, ni journalisées par le serveur.
- Une interdiction de discussion (`/chatban`) bloque aussi les messages d'AddOn : sinon un AddOn de discussion
  permettrait de la contourner.
- Un abus répété de la limite de débit est journalisé, pour que les MJ puissent intervenir.

## 10. Mise en œuvre

**ArchCore** — fait le 2026-10-19
- Schéma `ARCHCORE_EXT` et constantes dans `crates/archcore-net/src/schema.rs` ; encodage des champs 19 et
  20 et normalisation des codes de langue dans `crates/archcore-net/src/ext_text.rs` (partagés avec ArchRust).
- Traitement de `HELLO` dans `net.rs`, à côté de la poignée de main `0x48`, pour le login comme pour le
  monde, avec l'accord gardé dans la session (capacités et langue souhaitée).
- Négociation dans `crates/archcore/src/ext.rs`.
- Module `crates/archcore/src/world/addon.rs` : contrôles, seau à jetons, relais.
- Langue : catalogue et repli dans `crates/archcore/src/locale.rs` (tables `language`, `localized_text`,
  `sql/004_locale.sql`) ; `SET_LANGUAGE` et `TEXT_REQUEST` dans `crates/archcore/src/world/lang.rs` (monde)
  et `login.rs` (login) ; noms des PNJ et des monstres localisés dans les paquets de vue (`world/mod.rs`).
- Tests unitaires, et scénarios `archcore sim addon` et `archcore sim lang` (page [[Outils|FR-Outils]]) :
  deux clients simulés s'échangent des messages en murmure et en `SAY`, chaque refus de la section 7 est
  provoqué, un client d'origine simulé ne reçoit aucun paquet `0xAC` ; le même personnage entre en français
  puis en anglais et reçoit des noms de PNJ différents, `TEXT_REQUEST` et `SET_LANGUAGE` sont vérifiés, le
  client d'origine obtient la langue du compte.

**ArchRust** — fait le 2026-10-19
- `HELLO` après la poignée de main, avec la langue des Options ; `HELLO_ACK` intercepté (3 s au plus)
  (`archrust-netclient`, `connection.rs` et `ext.rs`).
- API Lua et événements de la section 8 dans `archrust-addons` (`comm.rs`), lissage des envois selon les
  limites annoncées.
- Sélecteur de langue dans la fenêtre Options (`archrust-engine`, `ui.rs`), persistance dans
  `options.toml`, `SET_LANGUAGE` au changement en jeu, `C_ArchCore.RequestTexts` → `ARCHCORE_TEXTS`.
- `archrust login --port 14002 --lang fr` rejoue le flux sans affichage et affiche la langue appliquée.

**Code commun.** ArchRust dépend de la crate `archcore-net` (dépendance git sur la branche `Core`), qui
porte le schéma `0xAC` et `ext_text`. Les licences le permettent : les deux projets sont sous
GPL-3.0-or-later.

## 11. Décisions (2026-10-19)

1. **Licence** : ArchCore passe sous **GPL-3.0-or-later**, comme ArchRust. Le partage de `archcore-net`
   entre les deux projets est donc possible.
2. **`/chatban` et AddOns** : un joueur réduit au silence ne peut pas non plus envoyer de messages d'AddOn
   (`NOT_ALLOWED`).
3. **Portée `SAY`** : même rayon que la discussion normale (`World.ViewRadius`). Un message d'AddOn en `SAY`
   atteint exactement les joueurs qui verraient un message de discussion ; un seul réglage pour les deux.
4. **Messages au serveur** : les premiers usages de la capacité `addon-data` seront les statistiques
   détaillées du personnage, les quêtes et l'état du monde. La portée `SERVER` reste fermée en version 1.
5. **AddOns officiels** : il n'en existe pas. Le protocole ne prévoit ni annonce ni distribution d'AddOns ;
   les préfixes `AC.` restent réservés aux messages du serveur.
6. **Langue** (v1.1) : une capacité `language` plutôt qu'un changement du protocole d'ArchLord, pour que le
   client d'origine continue de fonctionner ; codes BCP 47 ; le souhait suit le compte ; repli vers la
   langue de base, l'anglais puis le texte d'origine ; langues proposées d'après les versions officielles
   du jeu et le client multilingue de Webzen (page [[Traduction|FR-Traduction]]).
