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
| Connexion et entrée en jeu | Poignée de main, compte, mondes, personnages, entrée dans le monde | Suppression et parcours complets de création à comparer au client |
| Monde et réseau | Paquets de vue personnage et équipement, déplacement 0x40, cycle CHARACTER, décodage 0x41, objets AgpmItem et snapshot OPT_VIEW op 2 | Effets des actions, attachement des équipements aux os, événements et autres modules |
| Rendu | Terrain, modèles RenderWare statiques, PNJ, joueur, caméra et interface de base | Animation `.ean`, déformation des personnages, effets, éclairage et fidélité des interfaces |
| Personnages | Résolution des modèles par `charactertemplateclient.ini`, corps, armure par défaut et clumps d'équipement de `ItemTemplateAll.ini` | Attachement des pièces au squelette, apparence, statistiques et états du personnage |
| Systèmes de jeu | AddOns Lua, commandes de développement et inventaire AgpmItem nommé | Icônes/options d'objets, compétences, combat, quêtes, groupes, guildes et interactions PNJ |
| Audio et présentation | Non couvert de façon fonctionnelle | Son, musique, vidéos, options graphiques et écrans secondaires |

Les schémas réseau des 86 modules extraits sont référencés dans
`../ArchCore/data/re/packet-schemas.json`. ArchRust ne décode actuellement que les paquets utilisés
par ses parcours déjà implémentés; connaître un schéma ne signifie pas que son comportement est
reproduit.

## Jalons

1. **Personnages animés** — le lecteur `.ean` préserve tous les blocs et interprète les clés de 36
   octets comme temps, quaternion, translation et mot de liaison opaque. C'est une disposition
   candidate : les en-têtes RenderWare de référence déclarent le pointeur précédent en tête de la
   structure en mémoire, sans établir l'ordre sérialisé par Archonia. Les offsets de liaison
   observés reconstruisent des pistes candidates (`a0060000.ean` : 3 911 liens internes alignés
   sur 36 octets pour 3 997 clés, soit 86 racines candidates). `a0060000.ean` et le squelette
   `a0000060.ecl` ont tous deux 86 éléments. Une comparaison de la première clé avec les frames locales au repos donne
   l'indice ordinal attendu comme frame la plus proche pour 78 pistes sur 86 ; les translations
   correspondent à celles du squelette pour 76 pistes. Ces indices ne suffisent pas à établir
   l'association complète ni la convention des rotations. Le calcul des
   matrices monde animées par indice HAnim et le skinning CPU sont maintenant disponibles dans
   `Clump::world_matrices_with_pose` et `Clump::deform_geometry_positions`. La pose sans animation
   conserve les sommets de `b0000060.ecl` avec moins de `2.5e-5` d'écart. En revanche, l'application
   directe de `a0060000.ean` déplace certaines géométries jusqu'à 180 unités : ne pas encore envoyer
   ces résultats au rendu. Résoudre cet écart sur les vrais rigs et plusieurs clips, puis envoyer
   les sommets validés au GPU et sélectionner repos/marche depuis les états réseau. Comparer des
   poses et séquences à Archonia.
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

Continuer le jalon 1 : la correspondance des pistes TID 96 n'est que partiellement établie, et la
pose animée directe produit des déplacements trop grands pour certaines géométries. Il
faut établir la convention de rotation et vérifier le résultat visuel contre le client, puis
téléverser les sommets animés et choisir repos/marche selon l'état réseau.

## Limites de l'inventaire

Cette feuille de route résume l'état du code et des documents disponibles dans l'espace de travail.
La session actuelle n'expose pas les outils REA; les résultats Ghidra déjà consignés dans les
documents et données du projet restent utilisables, mais une nouvelle affirmation dépendant du
désassemblage doit être vérifiée lorsque REA sera accessible. Les étapes runtime Archonia restent
à distinguer des validations faites uniquement avec les fichiers du client.
