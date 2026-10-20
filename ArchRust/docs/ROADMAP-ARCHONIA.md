# Reconstitution du client Archonia dans ArchRust

## Objectif

Reproduire progressivement dans ArchRust le comportement observable du client Archonia 64 bits,
en vérifiant chaque étape contre l'exécutable, les données d'un vrai client et ArchCore. Les
constats de rétro-ingénierie doivent distinguer les comportements observés, les hypothèses et les
points qui restent à examiner.

## État actuel

| Domaine | Couverture actuelle | Écart principal |
|---|---|---|
| Démarrage, packs et ressources | Résolution packs/disque/`etc`, clés AES, textures `.tx1`, qualité et modèles GLB natifs | Options client, chemins et cas rares encore à comparer au runtime Archonia |
| Connexion et entrée en jeu | Poignée de main, compte, mondes, sélection et création de personnage dans la scène d'origine (`LoginSettings.txt`), suppression, entrée dans le monde | Interface d'origine (`UI_LG_*`, textures), renommage, choix du serveur |
| Monde et réseau | Paquets de vue personnage et équipement, déplacement 0x40, cycle CHARACTER, décodage 0x41, objets AgpmItem et snapshot OPT_VIEW op 2 | Effets des actions, attachement des équipements aux os, événements et autres modules |
| Rendu | Terrain multi-couches (base + couches fondues par masque, mipmaps), objets du monde (`obj%05d.ini`), modèles RenderWare, PNJ, joueur, personnages animés (attente, course) et tournés vers leur marche, caméra et interface de base | Effets, éclairage, eau, herbe, LOD et animations des objets, fidélité des interfaces |
| Personnages | Résolution des modèles par `charactertemplateclient.ini`, corps, armure par défaut, visage et cheveux par défaut (`charactertemplatecustomize.ini`), clumps d'équipement de `ItemTemplateAll.ini` | Attachement des pièces au squelette, visage/coiffure des autres joueurs, statistiques et états du personnage |
| Systèmes de jeu | AddOns Lua, commandes de développement et inventaire AgpmItem nommé | Icônes/options d'objets, compétences, combat, quêtes, groupes, guildes et interactions PNJ |
| Audio et présentation | Musique et ambiance de région (`.amf` → `RegionTemplate.ini` → `skyset.ini` → jeux d'effets `efffile_set.eff`) à l'heure du jeu (`AGPMTIMER`), effets des packs `.pk`/`sound/ui`, volumes persistés, `PlaySound`/`PlayMusic` des AddOns | Sons 3D des objets, plusieurs ambiances simultanées, zones `AgpmEventNature`, kits sonores, vidéos, options graphiques et écrans secondaires |

Les schémas réseau des 86 modules extraits sont référencés dans
`../ArchCore/data/re/packet-schemas.json`. ArchRust ne décode actuellement que les paquets utilisés
par ses parcours déjà implémentés; connaître un schéma ne signifie pas que son comportement est
reproduit.

## Jalons

1. **Personnages animés** — établi sur Archonia.exe et les données du client (détails dans
   `docs/FORMATS.md`, section « Animations des personnages ») : choix du clip par
   `charactertemplateanimation.ini` (`type:équipement:variante:fichier`, taux de tirage),
   pistes `.ean` dans l'ordre du tableau de nœuds HAnim, quaternions appliqués tels quels, matrices
   inverses de bind à quatrième ligne nulle corrigée et retrait de la frame de l'atomic (pose de
   bind à 4e-6 près). Le rendu rejoue l'attente et la course selon le déplacement ; vérifié en jeu
   sur ArchCore avec le TID 96. Le modèle regarde vers sa destination (avant = +Z, angle
   `atan2(dx, dz)` en degrés, `FUN_14016bf60` / `FUN_14016a070`) et part du champ 13 de la vue
   optimisée ; visage et cheveux par défaut viennent de `character\defaulthead` (`V%07x.ecl`,
   `TID × 0x200 + i` / `TID × 0x100 + i`), skinnés sur leur copie du squelette (sections
   « Orientation des personnages » et « Têtes par défaut » de `docs/FORMATS.md`). Restent :
   animations selon l'arme, attaque/coup reçu/mort déclenchés par les actions réseau,
   visage/coiffure des autres joueurs, et comparaison visuelle avec Archonia en marche.
2. **Actions et cycle de vie du monde** — les champs de l'action optimisée 0x41 sont décodés en
   événement, y compris les champs connus du paquet `FACTOR_POINT` (PV, PM et dégâts par type),
   d'après le code Archonia `ap_factors_make_damage_packet` et `ap_optimized_packet2_on_receive_char_action`.
   Les facteurs restent également disponibles sous forme brute ; les PV courants de la cible sont
   conservés sur l'entité et affichés dans le panneau du monde ; une variation connue de PV apparaît
   maintenant dans le journal de combat. Les barres de vie, animations et effets restent à appliquer.
   Les opérations `CHARACTER` 0x02 sont maintenant
   distinguées : ajout, mise à jour, retrait et retrait hors de vue ; la position, les facteurs PV
   et la liste des entités sont tenus à jour. `OPT_VIEW` op 2 (snapshot d'objets) et les paquets
   `AgpmItem` 0x04 d'ajout/retrait/mise à jour sont décodés ; l'instantané alimente la fenêtre
   d'inventaire. `OPT_VIEW` op 1 décode aussi les mini-paquets `OPT_VIEW_ITEM` d'équipement, et
   charge les clumps `d…` résolus via `BASE_DFF`. Leur attachement aux os reste à vérifier. Il reste
   les autres opérations 0x42 et l'orientation/état. ArchCore ne produit pas encore de 0x41 pour
   validation de bout en bout.
3. **Équipement et inventaire** — décodage 0x04 et des champs équipés d'`OPT_VIEW` op 1, snapshot
   op 2, noms et `BASE_DFF` chargés depuis `ItemTemplateAll.ini`, fenêtre d'inventaire. À compléter :
   valider visuellement le placement des clumps `d…` sur les os, afficher les icônes et appliquer
   les conversions/options d'équipement.
4. **Compétences et combat** — décoder le module 0x05, les actions, cibles, dégâts, effets et états
   persistants; valider les échanges client-serveur avec ArchCore.
5. **Quêtes et interactions** — décoder les modules 0x13 et 0x3C, dialogues PNJ, objectifs et
   récompenses, avec l'interface associée.
6. **Social et monde complet** — groupes, guildes, titres, événements, reconnexion, objets au sol,
   zones et règles de visibilité.
7. **Audio et interface** — sons 2D/3D et musique, options, fenêtres et écrans secondaires en
   fonction des données disponibles.

Chaque jalon doit ajouter un état lisible dans ArchRust, des traces utiles pour les paquets ou
ressources non compris, une validation sur des données client réelles ou ArchCore, et une mise à
jour de `README.md` et `docs/FORMATS.md` lorsque les résultats changent la documentation.

## Prochaine étape

Brancher les animations d'attaque, de coup reçu et de mort sur les actions 0x41 et les états
réseau, et choisir les clips selon l'arme équipée.

## Limites de l'inventaire

Cette feuille de route résume l'état du code et des documents disponibles dans l'espace de travail.
La session actuelle n'expose pas les outils REA; les résultats Ghidra déjà consignés dans les
documents et données du projet restent utilisables, mais une nouvelle affirmation dépendant du
désassemblage doit être vérifiée lorsque REA sera accessible. Les étapes runtime Archonia restent
à distinguer des validations faites uniquement avec les fichiers du client.
