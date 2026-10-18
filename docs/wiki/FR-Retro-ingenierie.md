# Rétro-ingénierie du client

Le protocole et les formats de données ont été reconstruits à partir de l'exécutable du client, avec Ghidra 12
piloté par [REA](https://github.com/morluto/rea). Le client est un exécutable PE x86-64 d'environ
70 000 fonctions, sans symboles, mais avec les informations RTTI des classes C++ (`AgcmLogin`,
`SoundManager`…).

## Méthode

1. Repérer une chaîne parlante (« LG_LoginMainCreateButtonClick », « Sound_Option »…).
2. Remonter à la fonction qui l'utilise, puis décompiler ses appels.
3. Pour les paquets : décompiler les constructeurs des modules `Agpm*`, qui déclarent chaque champ
   avec `AuPacket::SetFieldType`. `archcore re extract-schemas` en tire `data/re/packet-schemas.json`.
4. Vérifier chaque hypothèse avec le simulateur, puis avec le vrai client et le journal `ARCHCORE_TRACE=1`.

## Relevés principaux

| Sujet | Résultat |
|---|---|
| Adresse du serveur | Fichier de serveurs chiffré en RC4, clé MD5(« 1111 ») via CryptoAPI ; sa variante `…Test.ini` en clair est lue en priorité |
| Tables `Game/ini` | Même chiffrement ; texte en CP949. Une seconde clé, « asdfqwer », sert à un autre chargeur |
| En-tête de paquet | 19 octets, longueurs en u32 (u16 dans le client 32 bits) |
| Chiffrement | Blowfish ECB, mots petit-boutistes ; trames `A1`/`B1` … `AF`/`BF` |
| Mot de passe | 65 octets, chiffré en RC4 avec une clé de 8 caractères envoyée par le serveur |
| Type 3 | Drapeaux de champs sur 4 octets, posés par un accesseur et non par une écriture directe |
| Création de personnage | Le bouton « Créer » lit la race reçue avec l'op 3 de connexion ; il n'agit que pour la race 1 ou 2 |
| Langue | `Launcher/LanguageInfo.txt` : 17 = français (`ini_fr`), 5 = anglais |
| Son | FMOD moderne, 200 voix ; sons 2D (types 1 et 2) ou 3D à atténuation linéaire (types 3 et 4) |
| Mises à jour | Le jeu se met à jour seul : les adresses de fonctions changent d'une version à l'autre, pas les schémas |

