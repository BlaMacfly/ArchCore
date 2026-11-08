# Retour de bug n° 2 — inventaire, équipement et serveur

Travail du 4 novembre 2026. Aucun commit ni push. Les modifications antérieures non publiées sont conservées.

## Bilan point par point

| Point | État | Correction ou preuve |
|---|---|---|
| 01 | Corrigé pour les runes ; On/Off conservés | Rendu des ailes/capes serties : ExtraType → ID commun → variante race/classe → effet attaché. Les objets On/Off de type 12 restent des buffs : les compétences 628/629/662 et leurs données ne démontrent aucun visuel distinct. Aucun modèle arbitraire ajouté à ces consommables. |
| 02 | Corrigé | Les runes Cash existaient en EP8 et le client pouvait déjà les glisser. Le serveur ne les cherchait que dans le sac normal. La requête op 3 cherche maintenant aussi dans Cash ; mêmes règles de sertissage, refus d'un objet allumé, consommation depuis son sac. |
| 03 | Complété ; compteur déjà présent | Le compteur [occupés/total] et les sockets existaient déjà. Ajout du nom des runes visuelles serties même sans OptionTID. Pas de fenêtre inventée : UI_Necklace_Put_04.WAV est un son. |
| 04 | Déjà conforme, vérifié | 20091 → TEXTURE 62362 → l000f39a. Texture 50 × 50, UV 1, sans collision dans les dossiers contrôlés ; le test compare les pixels réellement chargés aux pixels du pack et passe. Aucun remplacement arbitraire. |
| 07 | Corrigé | Cinq sacs de cinq lignes et cinq colonnes, 125 places, côté serveur et recherche de case libre côté client. Coordonnées existantes conservées. La capture désigne la cinquième colonne, pas une sixième. |
| 08 | Cycle client retrouvé ; EP8 indéterminée | Même requête AGPMITEM 0x24/masque 0x2000 pour les deux intentions, puis état reçu par les mises à jour d’objet. Le silence d’une liste locale arrêtée à 0x21 ne prouve pas une absence EP8. [Étude approfondie](RE-POINT-08-VERROU.md). |
| 09 | Corrigé | Le dépôt sur CONTROL_531 ouvre Item_ConfirmDestroyItem ; envoi AGPMITEM op 21 seulement après accord. Objet porté ou banque non accepté dans cette corbeille. |
| 12 | Refus normal, message précisé | 22050 exige le niveau 75 en EP8. Le refus indique désormais le niveau requis et le niveau actuel. |
| 13 | Corrigé | Le casque 22116 avait été classé « corps » par l'estimation des os du modèle. La partie déclarée tête prime ; l'armure 22117 ne supprime plus le casque. |
| 23 | Corrigé | La promenade utilise le centre du point d'apparition, pas la position dispersée du monstre ; pas de nouvelle promenade pendant un trajet. Le dépassement de la limite de retour ramène le monstre d'un coup et le soigne. Suiveurs vérifiés séparément. |
| 26 | Règle d'origine activée (décision du 8 novembre 2026) | Rules.LootLevelGap vaut 11 par défaut : au-delà de 11 niveaux d'écart entre le tueur et le monstre, dans un sens comme dans l'autre, le monstre ne donne qu'une pièce d'or et aucun objet. 0 coupe la règle. |
| 31 | Corrigé ; dépôt déjà fonctionnel | Les deux grilles acceptaient déjà le glisser. Ajout de l'auto-sélection de potion à l'ouverture et après dépôt, comme Archonia : potions EP8 d'usage 8, sous-type 0 ; sélection compatible conservée, remplacement après consommation. |

## Preuves utilisées

- Archonia.exe, SHA-256 `e9c7609b02c30a17959aac37321750f6785e64f5b582f660a7c5bfef6cddc98f` : lecture statique, aucune exécution du client graphique.
- Ailes/capes : `FUN_140753370`, `FUN_14032d080`, `FUN_14082de30`, `FUN_1408af6c0` et `FUN_1408af9d0`. Tables EP8 : runes 4571/4607, colonne Necklace autorisée, exclusion 60 ; les 215 runes visuelles EP8 (156 de sous-type 22, 6 de sous-type 23 et 53 de sous-type 33) ont toutes Cash=1 et Necklace autorisée. Scarlet Wings 14565/14575/14592 : Type 9, SubType 22, ExtraType 502. On/Off 6011 de type 12, compétence 628, sans ExtraType. Les rappels de dépôt du forgeron d'Archonia refusent eux-mêmes le statut 51, alors que ses règles communes `FUN_14032b9c0` l'acceptent. L'acceptation Cash dans les fenêtres existantes est donc une correction explicite d'ArchRust/ArchCore fondée sur ces règles communes.
- Grille : UI_Inventory, cinq onglets Layer0 à Layer4 ; contrôle 258 × 258, cases 50 × 50, espacement 2 : cinq cases par axe.
- Forgeron : Convert_AddItem → `0x140607f80`, Convert_AddItemSocket → `0x140608240` ; auto-sélection à `0x1408825c2`, grille matériau `this+0x1808`, réévaluation après dépôt à `0x140608140`. Les données EP8 contiennent les potions 486, 4873 et 6360, sans variante de sous-type 1.
- Icône : SHA-256 du fichier l000f39a.tx1 `0eaa7880b62a264393fa6bf4dce2e844612f66f009b5a11c5d431cabd93ce954`. Test `icone_karkaen_reelle` réussi : nom du modèle, ressource et pixels concordent.
- Verrou : [étude approfondie](RE-POINT-08-VERROU.md) et [relevé des octets/instructions](preuves-point-08/archonia-verrou.json). Le cycle client est désormais détaillé, y compris le déverrouillage intentionnel et les voies de réception de l’état. Le serveur exact et la présence historique EP8 restent non démontrés ; aucune origine exclusive Archonia n’est affirmée.
- Niveau : 22050 Level 75 ; 22116 Part 3 et 22117 Part 2, tous deux niveau 60.
- Butin : les tables de taux et de groupes ne suffisent pas à fixer un seuil entre niveau du joueur et niveau du monstre. Le réglage optionnel ne remplace aucune valeur démontrée.

## Tests

- Moteur client : 327 tests réussis, aucun échec, trois tests de ressources exclus par défaut : tous trois réussis lors du lancement séparé avec les packs réels.
- Réseau client : 136 tests réussis ; protocole : 8 tests réussis.
- Colliers : trois tests réussis avec les données du client (résolution race/classe, distinction On/Off, tables).
- Serveur : dernière suite complète de 352 tests réussis, aucun échec (elle inclut les 279 tests monde et 6 tests configuration précédemment validés), dont la recherche de rune Cash et les limites de promenade.
- Simulateur : combat, skill, party, trade, quest, pk, aggro, product, buddy, mail, repair et ride réussis. MariaDB a été redémarrée pour les reprises combat/skill ; repair prépare désormais son personnage au niveau 2 requis pour son arme. Les reprises passent.
- Scénarios supplémentaires : bag, convert, patrol et bug2 (saturation des 125 cases, refus du 126e objet, destruction et réemploi de la dernière case) réussis. Rune Cash 4571 → collier 5018, consommation et persistance après reconnexion : réussi (journal sim-bug2.log).
- Aucune base principale modifiée ; simulateur limité aux bases archcore_mig_*. Exécutable principal préservé par un dossier de compilation isolé. Aucune migration de schéma nécessaire.

## À essayer en jeu

1. Remplir les cinq colonnes et le cinquième sac ; séparer une pile lorsque seule la dernière case reste libre.
2. Glisser un objet à la corbeille, annuler puis confirmer ; vérifier que l'annulation le conserve.
3. Chez le forgeron, ouvrir l'amélioration avec des potions dans le sac : potion déjà sélectionnée ; déposer une arme et améliorer ; vérifier le remplacement de la dernière potion consommée.
4. Glisser une rune ailes Cash 4571 sur la case rune du forgeron avec un collier 5018 dans la case objet ; confirmer, équiper le collier et vérifier le visuel puis son retrait. Les On/Off restent des buffs distincts.
5. Survoler le collier : compteur et noms des runes ; reconnecter et vérifier la persistance.
6. Tenter 22050 au niveau 50 : message « niveau 75 requis ». Équiper 22116 puis 22117, et dans l'ordre inverse : casque toujours visible.
7. Observer les cerfs au repos, puis les entraîner loin de leur apparition : promenade dans leur zone et retour cohérent. Le combat peut toujours déclencher une poursuite avant retour.
8. Vérifier qu'un monstre de plus de 11 niveaux d'écart (plus faible ou plus fort) ne donne qu'une pièce d'or ; `Rules.LootLevelGap = 0` rétablit le tirage complet.
