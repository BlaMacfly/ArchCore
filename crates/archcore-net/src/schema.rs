//! Schémas des paquets ArchLord connus d'ArchCore.
//!
//! Définitions relevées dans l'exécutable du client x64 (Ghidra via REA) : chaque module Agpm*/Agcm*
//! appelle `AuPacket::SetFieldType` dans son constructeur avec la liste (type, taille) de ses champs.
//! Le relevé complet de tous les modules est dans `data/re/packet-schemas.json`.

use crate::packet::{Field, Schema, Ty};

const fn f(ty: Ty) -> Field {
    Field(ty, 1)
}
const fn fs(ty: Ty, size: u16) -> Field {
    Field(ty, size)
}
const fn rep<const N: usize>(ty: Ty) -> [Field; N] {
    [Field(ty, 1); N]
}

use Ty::*;

// ---------------------------------------------------------------------------------------------
// Types de paquets principaux
// ---------------------------------------------------------------------------------------------
pub mod ptype {
    pub const SYSTEM: u8 = 0x00;
    pub const CONFIG: u8 = 0x01;
    pub const CHARACTER: u8 = 0x02;
    pub const CHAR_MANAGER: u8 = 0x03;
    pub const ITEM: u8 = 0x04;
    pub const SKILL: u8 = 0x05;
    pub const PRIVATE_TRADE: u8 = 0x07;
    pub const PARTY: u8 = 0x08;
    pub const PARTY_MANAGEMENT: u8 = 0x09;
    pub const LOGIN: u8 = 0x0d;
    pub const CHATTING: u8 = 0x0f;
    pub const TIMER: u8 = 0x11;
    pub const ADMIN: u8 = 0x12;
    pub const QUEST: u8 = 0x13;
    pub const EVENT_QUEST: u8 = 0x3e;
    pub const UI_STATUS: u8 = 0x14;
    pub const GUILD: u8 = 0x15;
    pub const WORLD: u8 = 0x18;
    pub const RETURN_TO_LOGIN: u8 = 0x2f;
    pub const EVENT_TELEPORT: u8 = 0x31;
    pub const EVENT_NPC_TRADE: u8 = 0x32;
    pub const EVENT_GUILD: u8 = 0x39;
    pub const EVENT_BANK: u8 = 0x36;
    pub const GUILD_WAREHOUSE: u8 = 0x54;
    pub const ITEM_CONVERT: u8 = 0x16;
    pub const REFINERY: u8 = 0x4b;
    pub const EVENT_REFINERY: u8 = 0x3f;
    pub const EVENT_ITEM_CONVERT: u8 = 0x38;
    pub const EVENT_NPC_DIALOG: u8 = 0x37;
    pub const EVENT_SKILL_MASTER: u8 = 0x3b;
    pub const EVENT_MANAGER: u8 = 0x3c;
    pub const OPT_CHAR_MOVE: u8 = 0x40;
    pub const OPT_CHAR_ACTION: u8 = 0x41;
    pub const OPT_VIEW: u8 = 0x42;
    pub const SYSTEM_MESSAGE: u8 = 0x44;
    pub const STARTUP_ENCRYPTION: u8 = 0x48;
    pub const TITLE: u8 = 0x60;
    pub const RECONNECT: u8 = 0x63;
    pub const REMISSION: u8 = 0x2a;
    pub const PVP: u8 = 0x4a;
    pub const PRODUCT: u8 = 0x17;
    pub const EVENT_PRODUCT: u8 = 0x3a;
    pub const BUDDY: u8 = 0x2b;
    pub const MAILBOX: u8 = 0x2d;
    pub const EVENT_ITEM_REPAIR: u8 = 0x34;
    pub const RIDE: u8 = 0x4d;
    pub const AUCTION: u8 = 0x0b;
    pub const EVENT_NATURE: u8 = 0x30;
    pub const NATURE_EFFECT: u8 = 0x47;
    pub const ITEM_SET: u8 = 0x6a;
    pub const SUMMONS: u8 = 0x4f;
    pub const CHANNEL: u8 = 0x2c;
    pub const AREA_CHATTING: u8 = 0x4c;
    pub const EVENT_CHAR_CUSTOMIZE: u8 = 0x43;
    pub const WANTED_CRIMINAL: u8 = 0x46;
    /// Extensions ArchRust ↔ ArchCore. Hors de la plage 0x01-0x77 acceptée par le répartiteur du
    /// client d'origine, qui l'ignore donc sans erreur.
    pub const ARCHCORE_EXT: u8 = 0xac;
}

// AgpmConfig — drapeaux 4 : drop d'objet à la mort, pénalité d'XP, serveur de test
pub static CONFIG: Schema = Schema { name: "AGPMCONFIG", id: ptype::CONFIG, flag_len: 4, fields: &[f(I32), f(I32), f(I8)] };

/// AgpmCharacter — drapeaux 4.
pub static CHARACTER: Schema = Schema {
    name: "AGPMCHARACTER",
    id: ptype::CHARACTER,
    flag_len: 4,
    fields: &[
        f(I8),       // 0 opération
        f(I32),      // 1 CID
        f(I32),      // 2 TID
        f(MemBlock), // 3 nom
        f(I8),       // 4 statut
        f(Packet),   // 5 mini déplacement (CHAR_MOVE)
        f(Packet),   // 6 mini action
        f(Packet),   // 7 mini CHAR_FACTORS
        f(I64),      // 8 argent inventaire
        f(I64),      // 9 argent banque
        f(I64),      // 10 cash
        f(I8),       // 11 statut d'action
        f(I8),       // 12 statut criminel
        f(I32),      // 13 attaquant
        f(I8),       // 14 nouveau perso
        f(U8),       // 15 dernier point de liaison
        f(U8),       // 16 animation sociale
        f(U64),      // 17 statut spécial
        f(I8),       // 18 transformation
        fs(Char, 32), // 19 skill init
        f(I8),       // 20 visage
        f(I8),       // 21 cheveux
        f(I32),      // 22 option flag
        f(I8),       // 23 taille de banque
        f(U16),      // 24 event status flag
        f(I64),      // 25 temps criminel restant
        f(I64),      // 26 temps meurtrier restant
        fs(Char, 16), // 27 signature
        f(MemBlock), // 28 données GameGuard
        f(U64),      // 29 dernière mort en Battle Square
    ],
};

/// AgcmCharacter (« AgsmCharManager » côté serveur) — drapeaux 4.
pub static CHAR_MANAGER: Schema = Schema {
    name: "AGSMCHARMANAGER",
    id: ptype::CHAR_MANAGER,
    flag_len: 4,
    fields: &[
        f(I8),        // 0 opération
        fs(Char, 12), // 1
        f(I32),       // 2
        fs(Char, 49), // 3 nom de personnage (« Nom#Monde » côté login)
        f(I32),       // 4 CID
        f(Vec3),      // 5 position
        f(I32),       // 6 clé d'authentification
        fs(Char, 248), // 7
    ],
};

/// AgpmLogin — drapeaux 2.
pub static LOGIN: Schema = Schema {
    name: "AGPMLOGIN",
    id: ptype::LOGIN,
    flag_len: 2,
    fields: &[
        f(I8),          // 0 opération
        fs(Char, 32),   // 1 clé de hachage (8 octets utiles)
        fs(Char, 49),   // 2 compte (chiffré RC4 côté client)
        f(I8),          // 3 longueur du compte
        fs(Char, 65),   // 4 mot de passe (chiffré RC4)
        f(I8),          // 5 longueur du mot de passe
        f(I32),         // 6 CID
        fs(Char, 32),   // 7 nom du monde
        f(Packet),      // 8 mini LOGIN_CHAR_INFO
        f(Packet),      // 9 mini LOGIN_VERSION_INFO / SERVER_INFO (entrée dans le monde)
        f(I32),         // 10 résultat
        f(Packet),      // 11 mini LOGIN_SERVER_INFO
        fs(Char, 2049), // 12 extra
        fs(Char, 5),    // 13 code / challenge
        f(I32),         // 14 compte limité
        f(I32),         // 15 compte protégé
    ],
};

/// AgpmTimer — drapeaux 1, du serveur au client : 0 vitesse du temps (multiplicateur, 6 par défaut dans Archonia ;
/// ignorée si 0), 1 temps du jeu en ms (heure = (ms / 3 600 000) % 24 ; ignoré si 0), 2 et 3 sans lecteur connu.
pub static TIMER: Schema = Schema { name: "AGPMTIMER", id: ptype::TIMER, flag_len: 1, fields: &[f(U8), f(U64), f(U32), f(U32)] };

/// AgpmWorld — drapeaux 1.
pub static WORLD: Schema = Schema {
    name: "AGPMWORLD",
    id: ptype::WORLD,
    flag_len: 1,
    fields: &[f(I8), fs(Char, 32), f(I16), f(MemBlock), fs(Char, 32)],
};

/// AgpmChatting — drapeaux 1.
pub static CHATTING: Schema = Schema {
    name: "AGPMCHATTING",
    id: ptype::CHATTING,
    flag_len: 1,
    fields: &[
        f(I8),        // 0 opération (0 = message)
        f(I32),       // 1 CID émetteur
        f(I8),        // 2 type de canal
        f(I32),       // 3 CID cible
        fs(Char, 49), // 4 nom émetteur
        fs(Char, 49), // 5 nom cible
        f(MemBlock),  // 6 message
    ],
};

/// AgpmAdmin — drapeaux 2 : op, CID, puis 13 mini-paquets (2 = ADMIN_CHAR).
pub static ADMIN: Schema = Schema {
    name: "AGPMADMIN",
    id: ptype::ADMIN,
    flag_len: 2,
    fields: &[
        f(I8), f(I32),
        f(Packet), f(Packet), f(Packet), f(Packet), f(Packet), f(Packet), f(Packet),
        f(Packet), f(Packet), f(Packet), f(Packet), f(Packet), f(Packet),
    ],
};

/// AgpmItem (type 4) — drapeaux 4. op 0 ajout, 1 retrait, 2 mise à jour ; case au champ 7.
pub static ITEM: Schema = Schema {
    name: "AGPMITEM",
    id: ptype::ITEM,
    flag_len: 4,
    fields: &[
        f(I8),       // 0 opération
        f(I8),       // 1 statut
        f(I32),      // 2 IID
        f(I32),      // 3 TID
        f(I32),      // 4 CID du propriétaire
        f(I32),      // 5 quantité
        f(Packet),   // 6 position au sol
        f(Packet),   // 7 case d'inventaire / de boutique (ITEM_GRID)
        f(Packet),   // 8 case de banque
        f(Packet),   // 9 équipement
        f(Packet),   // 10 facteurs
        f(Packet),   // 11 facteurs en pourcentage
        f(I32),      // 12 personnage ciblé
        f(Packet),   // 13 amélioration
        f(Packet),   // 14 restrictions
        f(Packet),   // 15 objet à âme
        f(Packet),   // 16 objet de quête
        f(I32),      // 17 compétence
        f(U64),      // 18 temps de recharge
        f(I32),      // 19 drapeaux de statut
        f(Packet),   // 20 options
        f(Packet),   // 21 bonus de compétence
        f(U64),      // 22 temps de recharge (transformation)
        f(Packet),   // 23 boutique en argent réel
        f(Packet),   // 24 informations supplémentaires
        f(MemBlock), // 25
        f(U64),      // 26
    ],
};

/// AgcmEventNPCTrade (0x32) — drapeaux 2.
pub static EVENT_NPC_TRADE: Schema = Schema {
    name: "AGCMEVENTNPCTRADE",
    id: ptype::EVENT_NPC_TRADE,
    flag_len: 2,
    fields: &[
        f(Packet), // 0 événement (EVENT_BASE)
        f(I8),     // 1 opération
        f(I32),    // 2 CID du joueur
        f(I32),    // 3 IID
        f(Packet), // 4 détail (NPC_TRADE_DETAIL)
        f(I64),    // 5 horodatage
        f(I32),    // 6 résultat
        f(I8),     // 7 sac cible
        f(I8),     // 8 ligne cible
        f(I8),     // 9 colonne cible
    ],
};

/// AgpmSkill (0x05) — drapeaux 2. Compétences d'un personnage, lancement et résultat.
pub static SKILL: Schema = Schema {
    name: "AGPMSKILL",
    id: ptype::SKILL,
    flag_len: 2,
    fields: &[
        f(I8),          // 0 opération
        f(I32),         // 1 identifiant de la compétence
        f(Packet),      // 2 propriétaire (mini SKILL_BASE)
        f(Packet),      // 3 facteurs (AGPMCHAR_FACTORS, sous-paquet « dirt » : niveau, points)
        f(I32),         // 4 TID
        f(I8),          // 5 statut (0 prête, 1 en incantation, 2 lancée)
        f(Packet),      // 6 action (mini SKILL_ACTION)
        f(U8),          // 7 points de compétence
        fs(Packet, 96), // 8 effets actifs (mini SKILL_BUFF)
        f(MemBlock),    // 9
    ],
};

/// AgpmQuest (0x13) — drapeaux 2 (relevé dans le client x64).
pub static QUEST: Schema = Schema {
    name: "AGPMQUEST",
    id: ptype::QUEST,
    flag_len: 2,
    fields: &[
        f(I8),       // 0 opération
        f(I32),      // 1 CID
        f(Packet),   // 2 événement (EVENT_BASE) du PNJ
        f(Packet),   // 3 quête (mini QUEST_INFO)
        f(I32),      // 4 TID de la quête
        f(I32),      // 5
        f(I32),      // 6 résultat (0 refus, 1 réussi)
        f(I32),      // 7 récompense choisie
        f(MemBlock), // 8 drapeaux des quêtes (0x800 octets, TID 1 à 16 383)
        f(MemBlock), // 9 drapeaux (TID 16 384 à 32 767)
        f(MemBlock), // 10 drapeaux (TID 32 768 à 49 151)
    ],
};

/// AgpmEventQuest (0x3E) — drapeaux 1 : op, événement (EVENT_BASE), CID, entier.
pub static EVENT_QUEST: Schema = Schema { name: "AGPMEVENTQUEST", id: ptype::EVENT_QUEST, flag_len: 1, fields: &[f(I8), f(Packet), f(I32), f(I32)] };

/// AgpmGuild (0x15) — drapeaux 4 : op, CID (ou code d'un message système), nom de la guilde, nom du chef
/// (ou d'un joueur), 8 × INT32, rang, date de création (INT64), taille maximale, union, mot de passe,
/// annonce, statut, victoires, nuls, défaites, points, puis 6 paquets (membre GUILD_MEMBER, combat, …,
/// liste des guildes), emblème (TID, couleur), vainqueur.
pub static GUILD: Schema = Schema {
    name: "AGPMGUILD",
    id: ptype::GUILD,
    flag_len: 4,
    fields: &[
        f(I8),        // 0 opération
        f(I32),       // 1 CID (message système : code)
        fs(Char, 33), // 2 nom de la guilde (message système : texte 1)
        fs(Char, 49), // 3 chef (message système : texte 2)
        fs(I32, 8),   // 4 (message système : nombre 1 au premier)
        f(I32),       // 5 rang de la guilde (message système : nombre 2)
        f(I64),       // 6 date de création
        f(I32),       // 7 taille maximale
        f(I32),       // 8 union
        fs(Char, 13), // 9 mot de passe
        f(MemBlock),  // 10 annonce
        f(I8),        // 11 statut
        f(I32),       // 12 victoires
        f(I32),       // 13 nuls
        f(I32),       // 14 défaites
        f(I32),       // 15 points
        f(Packet),    // 16 membre (GUILD_MEMBER)
        f(Packet),    // 17 combat
        f(Packet),    // 18 combattant
        f(Packet),    // 19 membre au combat
        f(Packet),    // 20 liste des membres au combat
        f(Packet),    // 21 liste des guildes
        f(I32),       // 22 emblème : TID
        f(I32),       // 23 emblème : couleur
        f(I32),       // 24 guilde victorieuse
    ],
};

/// AgpmEventGuild (0x39) — drapeaux 1 : op (0 demande, 1 accord), événement (EVENT_BASE), CID.
pub static EVENT_GUILD: Schema = Schema { name: "AGPMEVENTGUILD", id: ptype::EVENT_GUILD, flag_len: 1, fields: &[f(I8), f(Packet), f(I32)] };

/// AgpmEventTeleport (0x31) — drapeaux 1 : op, événement (EVENT_BASE), CID du joueur, nom du point d'arrivée
/// (CP949), position d'arrivée, points d'arrivée désactivés.
pub static EVENT_TELEPORT: Schema = Schema {
    name: "AGPMEVENTTELEPORT",
    id: ptype::EVENT_TELEPORT,
    flag_len: 1,
    fields: &[f(I8), f(Packet), f(I32), fs(Char, 33), f(Vec3), f(MemBlock)],
};

/// AgpmItemConvert (0x16) — drapeaux 2 : op, CID du joueur, IID de l'objet, IID de ce qui le convertit
/// (catalyseur, pierre, rune), résultat, niveau de renforcement, sockets, sockets remplis, TID ajouté, TID
/// des sockets remplis (ITEM_CONVERT_TIDS).
pub static ITEM_CONVERT: Schema = Schema {
    name: "AGPMITEMCONVERT",
    id: ptype::ITEM_CONVERT,
    flag_len: 2,
    fields: &[f(I8), f(I32), f(I32), f(I32), f(I8), f(I8), f(I8), f(I8), f(I32), f(Packet)],
};

/// AgpmRefinery (0x4B) — drapeaux 2 : op, CID, paramètre (identifiant de recette), IID des 20 cases de la
/// grille, résultat, TID produit, prix.
pub static REFINERY: Schema = Schema {
    name: "AGPMREFINERY",
    id: ptype::REFINERY,
    flag_len: 2,
    fields: &[f(I8), f(I32), f(I32), fs(I32, 20), f(I32), f(I32), f(I64)],
};

/// AgpmEventRefinery (0x3F) — drapeaux 1 : op (0 demande, 1 accord), événement (EVENT_BASE), CID du joueur.
pub static EVENT_REFINERY: Schema = Schema { name: "AGPMEVENTREFINERY", id: ptype::EVENT_REFINERY, flag_len: 1, fields: &[f(I8), f(Packet), f(I32)] };

/// AgpmEventItemConvert (0x38) — drapeaux 1 : op, événement (EVENT_BASE), CID du joueur.
pub static EVENT_ITEM_CONVERT: Schema = Schema { name: "AGPMEVENTITEMCONVERT", id: ptype::EVENT_ITEM_CONVERT, flag_len: 1, fields: &[f(I8), f(Packet), f(I32)] };

/// AgpmEventBank (0x36) — drapeaux 1 : op (0 demande, 1 accord), événement (EVENT_BASE), CID du joueur.
pub static EVENT_BANK: Schema = Schema { name: "AGPMEVENTBANK", id: ptype::EVENT_BANK, flag_len: 1, fields: &[f(I8), f(Packet), f(I32)] };

/// AgpmGuildWarehouse (0x54) — drapeaux 2 : opération, CID, argent, pages en plus, IID, page, ligne, colonne,
/// résultat (FUN_140309ad0).
pub static GUILD_WAREHOUSE: Schema = Schema {
    name: "AGPMGUILDWAREHOUSE",
    id: ptype::GUILD_WAREHOUSE,
    flag_len: 2,
    fields: &[f(I8), f(I32), f(I64), f(I32), f(I32), f(I32), f(I32), f(I32), f(I32)],
};

/// AgpmPrivateTrade (0x07) — drapeaux 2 : échange entre joueurs.
/// AgpmParty — drapeaux 2.
pub static PARTY: Schema = Schema {
    name: "AGPMPARTY",
    id: ptype::PARTY,
    flag_len: 2,
    fields: &[
        f(I8),       // 0 opération
        f(I32),      // 1 identifiant du groupe
        f(I8),       // 2 membres au plus
        f(I8),       // 3 membres
        f(I32),      // 4 CID du membre ajouté ou retiré
        f(MemBlock), // 5 liste des CID (u32)
        f(Packet),   // 6 facteurs
        f(Packet),   // 7 zone d'effet
        f(I8),       // 8 répartition de l'expérience
        f(Packet),   // 9 bonus
        f(I8),       // 10 répartition des objets
    ],
};

/// AgpmParty (gestion) — drapeaux 1.
pub static PARTY_MANAGEMENT: Schema = Schema {
    name: "AGPMPARTYMANAGEMENT",
    id: ptype::PARTY_MANAGEMENT,
    flag_len: 1,
    fields: &[
        f(I8),        // 0 opération
        f(I32),       // 1 CID de l'auteur
        f(I32),       // 2 CID de la cible
        fs(Char, 49), // 3 nom
    ],
};

pub static PRIVATE_TRADE: Schema = Schema {
    name: "AGPMPRIVATETRADE",
    id: ptype::PRIVATE_TRADE,
    flag_len: 2,
    fields: &[
        f(I8),     // 0 opération
        f(I32),    // 1 CID
        f(I32),    // 2 CID de l'autre joueur
        f(I32),    // 3 IID
        f(I32),    // 4 TID
        f(I64),    // 5 argent
        f(Packet), // 6 case d'origine (ITEM_GRID)
        f(Packet), // 7 case (ITEM_GRID)
        f(I32),    // 8 argument (résultat…)
    ],
};

/// AgpmEventSkillMaster (0x3B) — drapeaux 1 : maître des compétences.
pub static EVENT_SKILL_MASTER: Schema = Schema {
    name: "AGPMEVENTSKILLMASTER",
    id: ptype::EVENT_SKILL_MASTER,
    flag_len: 1,
    fields: &[
        f(I8),     // 0 opération
        f(Packet), // 1 événement (EVENT_BASE)
        f(I32),    // 2 CID du joueur
        f(I32),    // 3 identifiant de compétence
        f(I32),    // 4 TID de compétence
        f(I8),     // 5 résultat
        f(I8),     // 6 points de compétence
        f(I8),     // 7 réinitialisation consommée
    ],
};

/// AgpmEventNPCDialog (0x37) — drapeaux 1 : op, événement (EVENT_BASE), CID du joueur.
pub static EVENT_NPC_DIALOG: Schema =
    Schema { name: "AGPMEVENTNPCDIALOG", id: ptype::EVENT_NPC_DIALOG, flag_len: 1, fields: &[f(I8), f(Packet), f(I32)] };

/// ApmEventManager (0x3C) — drapeaux 1 : événement (EVENT_BASE), données de la fonction.
pub static EVENT_MANAGER: Schema =
    Schema { name: "APMEVENTMANAGER", id: ptype::EVENT_MANAGER, flag_len: 1, fields: &[f(Packet), f(Packet)] };

/// AgpmSystemMessage — drapeaux 1.
pub static SYSTEM_MESSAGE: Schema = Schema {
    name: "AGPMSYSTEMMESSAGE",
    id: ptype::SYSTEM_MESSAGE,
    flag_len: 1,
    fields: &[f(I8), f(I32), f(I32), f(I32), fs(Char, 65), fs(Char, 65)],
};

/// AgpmReturnToLogin — drapeaux 1.
pub static RETURN_TO_LOGIN: Schema = Schema {
    name: "AGPMRETURNTOLOGIN",
    id: ptype::RETURN_TO_LOGIN,
    flag_len: 1,
    fields: &[f(I8), f(I32), fs(Char, 49), fs(Char, 32), fs(Char, 25), f(I32)],
};

/// AgpmOptimizedPacket2 (0x40) — déplacement, drapeaux 1.
pub static OPT_CHAR_MOVE: Schema = Schema {
    name: "AGPMOPTIMIZEDCHARMOVE",
    id: ptype::OPT_CHAR_MOVE,
    flag_len: 1,
    fields: &[
        f(I32),  // 0 CID
        f(Vec3), // 1 position
        f(Vec3), // 2 destination
        f(I32),  // 3 cible suivie
        f(U16),  // 4 distance de suivi
        f(I8),   // 5 drapeaux de déplacement
        f(I8),   // 6 direction
        f(I8),   // 7
        f(I32),  // 8
    ],
};

/// AgpmOptimizedPacket2 (0x41) — action, drapeaux 2.
pub static OPT_CHAR_ACTION: Schema = Schema {
    name: "AGPMOPTIMIZEDCHARACTION",
    id: ptype::OPT_CHAR_ACTION,
    flag_len: 2,
    fields: &[f(I32), f(I32), f(I8), f(Packet), f(I32), f(I8), f(I8), f(U32), f(U8)],
};

/// AgpmOptimizedPacket2 (0x42) — vue, drapeaux 2.
pub static OPT_VIEW: Schema = Schema {
    name: "AGPMOPTIMIZEDVIEW",
    id: ptype::OPT_VIEW,
    flag_len: 2,
    fields: &[
        f(I8),         // 0 opération
        f(Packet),     // 1 vue d'un personnage
        fs(Packet, 32), // 2 liste de vues
        f(MemBlock),   // 3
        f(MemBlock),   // 4 fonctions des PNJ (paquets 0x3C complets)
        fs(Char, 33),  // 5
        f(MemBlock),   // 6 tous les objets (paquets AGPMITEM complets)
        f(MemBlock),   // 7 améliorations
        f(MemBlock),   // 8 compétences
        f(I32),        // 9
        f(I32),        // 10
        f(I32),        // 11
        f(I32),        // 12
        f(MemBlock),   // 13
    ],
};

/// AgpmStartupEncryption (0x48) — drapeaux 1 : op, clé.
pub static STARTUP_ENCRYPTION: Schema =
    Schema { name: "AGPMSTARTUPENCRYPTION", id: ptype::STARTUP_ENCRYPTION, flag_len: 1, fields: &[f(I8), f(MemBlock)] };

/// Extensions ArchRust ↔ ArchCore (type 0xAC) — drapeaux 4. Voir la page « Extensions ArchRust » du wiki.
pub static ARCHCORE_EXT: Schema = Schema {
    name: "ARCHCORE_EXT",
    id: ptype::ARCHCORE_EXT,
    flag_len: 4,
    fields: &[
        f(U8),       // 0 opération (ext_op)
        f(U32),      // 1 numéro de requête (choisi par le client)
        f(U16),      // 2 version majeure du protocole d'extension
        f(U16),      // 3 version mineure
        f(MemBlock), // 4 logiciel (UTF-8, 64 octets au plus)
        f(MemBlock), // 5 capacités « nom/version;… » (ASCII)
        f(U16),      // 6 charge maximale d'un message d'AddOn
        f(U16),      // 7 limite de débit : messages permis d'un coup
        f(U16),      // 8 limite de débit : messages rendus par seconde
        f(MemBlock), // 9 préfixe d'AddOn
        f(U8),       // 10 portée (ext_channel)
        f(MemBlock), // 11 destinataire (nom de personnage)
        f(I32),      // 12 CID de l'émetteur (rempli par le serveur)
        f(MemBlock), // 13 nom de l'émetteur (rempli par le serveur)
        f(MemBlock), // 14 charge
        f(U8),       // 15 résultat (ext_result)
        f(MemBlock), // 16 langue (BCP 47, ASCII) : souhaitée par le client, appliquée par le serveur — v1.1
        f(MemBlock), // 17 langues disponibles sur le serveur, « fr;en;de » — v1.1
        f(MemBlock), // 18 sorte de texte demandée (ext_text_kind, ASCII) — v1.1
        f(MemBlock), // 19 identifiants demandés (UINT32 petit-boutiste, à la suite) — v1.1
        f(MemBlock), // 20 textes rendus : UINT32 identifiant, UINT16 longueur, UTF-8, à la suite — v1.1
    ],
};

/// AgpmUIStatus (0x14) — drapeaux 2 (Archonia : `INT8 INT32 PACKET[50] INT8 PACKET MEMORY_BLOCK INT32 INT32 INT8
/// INT8 INT8 MEMORY_BLOCK`).
pub static UI_STATUS: Schema = Schema {
    name: "AGPMUISTATUS",
    id: ptype::UI_STATUS,
    flag_len: 2,
    fields: &[
        f(I8),          // 0 opération (0 envoi complet, 1 changement, 2 barre encodée, 3 casque)
        f(I32),         // 1 CID
        fs(Packet, 50), // 2 barre de raccourcis
        f(I8),          // 3 case changée
        f(Packet),      // 4 contenu de la case
        f(MemBlock),    // 5 barre encodée (texte)
        f(I32),         // 6 TID de la potion de vie
        f(I32),         // 7 TID de la potion de mana
        f(I8),          // 8 affichage du casque
        f(I8),          // 9 seuil de prise automatique des PV (%)
        f(I8),          // 10 seuil de prise automatique des PM (%)
        f(MemBlock),    // 11 temps de recharge encodés (texte)
    ],
};

/// AgpmReconnect — drapeaux 4 (constructeur FUN_14027c3f0 d'Archonia).
pub static RECONNECT: Schema = Schema {
    name: "AGPMRECONNECT",
    id: ptype::RECONNECT,
    flag_len: 4,
    fields: &[
        f(I8),          // 0 opération (1 demande du client, 2 réponse du serveur)
        f(I32),         // 1 CID
        fs(Char, 48),   // 2 nom du personnage
        fs(Char, 1024), // 3 compte
        fs(Char, 1024), // 4 mot de passe
        f(I32),         // 5 clé de reconnexion
        f(I8),          // 6 accord (1 : reconnexion acceptée)
    ],
};

/// AgpmPvP (0x4A) — drapeaux 2, du serveur au client seulement (réception FUN_1402d2c70) : opération, CID (code du
/// message pour l'op 18), CID cible, victoires (paramètre 1), défaites (paramètre 2), mode (1 sûr, 2 libre,
/// 3 combat), statut (bit 1 : invincible), octet de l'op 0, guilde (chaîne 1), nom (chaîne 2).
pub static PVP: Schema = Schema {
    name: "AGPMPVP",
    id: ptype::PVP,
    flag_len: 2,
    fields: &[f(I8), f(I32), f(I32), f(I32), f(I32), f(I8), f(I8), f(I8), fs(Char, 33), fs(Char, 49)],
};

/// AgpmRemission (0x2A) — drapeaux 1 : opération (0 demande au PNJ, 1 accord, 2 rémission), CID, mini-paquet
/// (événement du PNJ pour les op 0-1, `mini::REMISSION` pour l'op 2).
pub static REMISSION: Schema = Schema { name: "AGPMREMISSION", id: ptype::REMISSION, flag_len: 1, fields: &[f(I8), f(I32), f(Packet)] };

/// AgpmProduct (0x17) — drapeaux 2 : production (récolte sur cadavre, cuisine, alchimie). Op 0 fabrication
/// (client : 3 recette, 4 IID de l'objet recette), 1 recettes connues (serveur : 7 liste d'INT32), 2 récolte
/// (client : 3 CID du cadavre), 3 résultat (serveur : 3 TID obtenu, 4 quantité, 5 code, 6 expérience gagnée).
pub static PRODUCT: Schema = Schema {
    name: "AGPMPRODUCT",
    id: ptype::PRODUCT,
    flag_len: 2,
    fields: &[
        f(I8),       // 0 opération
        f(I32),      // 1 CID du personnage
        f(I32),      // 2 identifiant de la compétence (pas son TID)
        f(I32),      // 3 recette, cadavre ou TID obtenu
        f(I32),      // 4 IID de l'objet recette ou quantité obtenue
        f(I32),      // 5 code de résultat
        f(I32),      // 6 expérience de production gagnée
        f(MemBlock), // 7 recettes connues (INT32 bout à bout)
    ],
};

/// AgpmEventProduct (0x3A) — drapeaux 1 : op (0 demande au PNJ, 1 accord, 2 / 3 apprendre une compétence et son
/// résultat, 4 / 5 acheter une recette et son résultat), événement (EVENT_BASE, fonction 24), CID, a (TID de la
/// compétence ou recette), b (code de résultat). Données de la fonction 24 : `mini::EVENT_DATA_INT32` (catégorie).
pub static EVENT_PRODUCT: Schema =
    Schema { name: "AGPMEVENTPRODUCT", id: ptype::EVENT_PRODUCT, flag_len: 1, fields: &[f(I8), f(Packet), f(I32), f(I32), f(I32)] };

/// AgpmBuddy (0x2B) — drapeaux 4 (constructeur FUN_1402ab0c0 d'Archonia) : listes d'amis et d'ignorés. Champs de
/// taille fixe ; le CID est celui du joueur à qui appartient la liste (le destinataire, du serveur au client).
pub static BUDDY: Schema = Schema {
    name: "AGPMBUDDY",
    id: ptype::BUDDY,
    flag_len: 4,
    fields: &[
        f(I8),        // 0 opération (buddy_op)
        f(I32),       // 1 CID du propriétaire de la liste
        fs(Char, 33), // 2 nom de l'autre joueur
        f(U32),       // 3 drapeaux de l'entrée (buddy_flag)
        f(I32),       // 4 op 7 : région (RegionTemplate) ; ailleurs -1 ou 0
        f(I32),       // 5 relation « Buddy » (mentor) : 0 aucune, 1 mon mentor, 2 mon disciple
        f(I32),       // 6 op 8 : code de résultat (buddy_result)
    ],
};

/// AgpmBuddy, op 15 seulement — même type 0x2B, second AuPacket du module, drapeaux 4 : liste d'ignorés lue par le
/// client dans `Ini\IgnoreList.txt` après l'op 9. À relire avec ce schéma à partir des octets bruts du paquet.
pub static BUDDY_LIST: Schema = Schema {
    name: "AGPMBUDDY_LIST",
    id: ptype::BUDDY,
    flag_len: 4,
    fields: &[
        f(I8),         // 0 opération (15)
        f(I32),        // 1 CID du propriétaire
        f(I16),        // 2 nombre d'entrées (30 au plus)
        fs(Char, 990), // 3 30 noms de 33 octets à la suite
        fs(U32, 30),   // 4 drapeaux de chaque nom
    ],
};

/// AgpmMailBox (0x2D) — drapeaux 2 (constructeur FUN_1402ead30 d'Archonia) : courrier entre joueurs.
pub static MAILBOX: Schema = Schema {
    name: "AGPMMAILBOX",
    id: ptype::MAILBOX,
    flag_len: 2,
    fields: &[
        f(I8),     // 0 opération (mail_op)
        f(I32),    // 1 CID du joueur concerné
        f(Packet), // 2 courrier (mini MAIL)
        f(I32),    // 3 op 3 : IID de l'objet joint
        f(I32),    // 4 code de résultat (0 succès)
    ],
};

/// AgpmEventItemRepair (0x34) — drapeaux 1 (constructeur FUN_1402b3b50 d'Archonia) : réparation chez un PNJ de
/// fonction 17. Op 3 demande d'ouverture et op 4 accord (champs 0 à 2), op 0 demande de réparation (tous les
/// champs, résultat 0), op 1 résultat (champ 5 : `repair_result`).
pub static EVENT_ITEM_REPAIR: Schema = Schema {
    name: "AGPMEVENTITEMREPAIR",
    id: ptype::EVENT_ITEM_REPAIR,
    flag_len: 1,
    fields: &[
        f(I8),        // 0 opération
        f(Packet),    // 1 événement (EVENT_BASE, fonction 17)
        f(I32),       // 2 CID du joueur
        fs(I32, 20),  // 3 IID des objets de la grille de réparation (ligne × 4 + colonne, 0 : case vide)
        f(I64),       // 4 coût total calculé par le client
        f(I32),       // 5 code de résultat
    ],
};

/// AgpmRide (0x4D) — drapeaux 1 (constructeur FUN_14027ad80 d'Archonia) : montures, du serveur au client seulement
/// (le client n'émet jamais ce paquet ; il monte en équipant l'objet de partie 14). Op 2 monté, op 4 descendu,
/// op 5 fin de durée (sans effet dans le client), op 6 monté sur un modèle donné par son TID.
pub static RIDE: Schema = Schema {
    name: "AGPMRIDE",
    id: ptype::RIDE,
    flag_len: 1,
    fields: &[
        f(I8),  // 0 opération
        f(I32), // 1 CID du cavalier
        f(I32), // 2 IID de l'objet monture (op 2) ou TID du modèle de monture (op 6)
        f(I32), // 3 temps restant (s), ignoré par le client
    ],
};

/// AgpmAuction (0x0B) — drapeaux 2 (constructeur 0x140268eb0 d'Archonia) : maison des ventes (achat immédiat). Le
/// sous-paquet du champ 2 dépend de l'op (`auction_op`) : `mini::AUCTION_SALES`, `AUCTION_SELL`, `AUCTION_CANCEL`,
/// `AUCTION_CONFIRM`, `AUCTION_BUY`, `AUCTION_SELECT2` ou `EVENT_BASE` (ops 9 et 10).
pub static AUCTION: Schema = Schema {
    name: "AGPMAUCTION",
    id: ptype::AUCTION,
    flag_len: 2,
    fields: &[
        f(I8),     // 0 opération (auction_op)
        f(I32),    // 1 CID du joueur
        f(Packet), // 2 sous-paquet selon l'op
        f(I32),    // 3 code de résultat (0 succès, auction_result)
    ],
};

/// AgpmEventNature (0x30) — drapeaux 1 (constructeur FUN_1401e99b0 d'Archonia), du serveur au client : 0 météo
/// (`weather` ; -1 absent), 1 et 2 sans lecteur connu.
pub static EVENT_NATURE: Schema = Schema { name: "AGPMEVENTNATURE", id: ptype::EVENT_NATURE, flag_len: 1, fields: &[f(I8), f(I64), f(I64)] };

/// AgpmNatureEffect (0x47) — drapeaux 2 (constructeur FUN_1402c24d0 d'Archonia), du serveur au client : identifiant
/// d'effet de nature (`NatureEffect.ini` : 0 neige, 1 pluie, 2 spores…), -1 : arrêt en fondu.
pub static NATURE_EFFECT: Schema = Schema { name: "AGPMNATUREEFFECT", id: ptype::NATURE_EFFECT, flag_len: 2, fields: &[f(I32)] };

/// AgpmItemSet (0x6A) — drapeaux 1 (constructeur FUN_14024fb10 d'Archonia) : commutateur d'équipement (fenêtre
/// UI_SwitchEditor), 100 emplacements de jeux d'équipement. Le client envoie toujours les 4 champs.
pub static ITEM_SET: Schema = Schema {
    name: "AGPMITEMSET",
    id: ptype::ITEM_SET,
    flag_len: 1,
    fields: &[
        f(I8),          // 0 opération (item_set_op)
        f(I32),         // 1 emplacement 0-99 (op 4 : emplacement actif, -1 aucun)
        f(I8),          // 2 op 3 : rempli (1) ou vide (0) ; op 6 : (de << 4) | vers ; op 7 : position
        fs(Char, 4000), // 3 op 5 : objets du jeu, « E,<partie>,<identifiant>:… » (10 au plus)
    ],
};

/// AgpmSummons (0x4F) — drapeaux 2 (constructeur FUN_14028f6d0 d'Archonia) : invocations (esprits, créatures des
/// compétences, familiers). Ops 0 ajout, 1 retrait, 3 limite des invocations, 4 limite des créatures apprivoisées, du
/// serveur au client ; op 2 propension, du client au serveur (champs 1, 2, 5, 8) puis renvoyée au maître. Tous les
/// constructeurs du client posent le champ 8 : le serveur l'envoie toujours.
pub static SUMMONS: Schema = Schema {
    name: "AGPMSUMMONS",
    id: ptype::SUMMONS,
    flag_len: 2,
    fields: &[
        f(I8),  // 0 opération (summons_op)
        f(I32), // 1 CID du maître
        f(I32), // 2 CID de la créature invoquée
        f(I32), // 3 TID de la créature
        f(I8),  // 4 op 0 : type d'invocation (1 invocation, 2 apprivoisée, 4 fixe, 8 familier) ; ops 3 et 4 : nouvelle limite
        f(I8),  // 5 propension : 0 attaque, 1 défense, 2 « ombre »
        f(U64), // 6 horodatage 1, non nul (ArchCore : apparition, ms Unix)
        f(U64), // 7 horodatage 2, non nul (ArchCore : fin prévue, ms Unix)
        f(U8),  // 8 « forcée » : 1 hors limite, sans occuper d'emplacement ordinaire (familiers)
    ],
};

/// AgpmChannel (0x2C) — drapeaux 4 (constructeur FUN_1402af5d0 d'Archonia, réception FUN_1402afaf0) : salons de
/// discussion (fenêtres UI_ChatRoom0..4), ops `channel_op`. Le client lit la liste de l'op 3 sans vérifier qu'elle
/// existe : le serveur l'envoie toujours.
pub static CHANNEL: Schema = Schema {
    name: "AGPMCHANNEL",
    id: ptype::CHANNEL,
    flag_len: 4,
    fields: &[
        f(I8),         // 0 opération (channel_op)
        f(I32),        // 1 CID du demandeur
        fs(Char, 41),  // 2 nom du salon (40 au plus)
        fs(Char, 9),   // 3 mot de passe (8 au plus)
        fs(Char, 33),  // 4 nom de personnage (membre, invitant, cible, auteur)
        f(I32),        // 5 genre (channel_kind)
        f(I32),        // 6 code de résultat (op 11, channel_result)
        fs(Char, 121), // 7 texte d'un message (120 au plus)
        f(MemBlock),   // 8 op 3 : membres « nom1;nom2;…; » terminés par un zéro
    ],
};

/// AgpmAreaChatting (0x4C) — drapeaux 1 (constructeur FUN_1402b49c0 d'Archonia) : cris de zone, du serveur au client
/// seulement (le client n'en envoie jamais). Seule l'op 0 compte ; le champ 4 choisit l'affichage comme le type
/// d'AGPMCHATTING.
pub static AREA_CHATTING: Schema = Schema {
    name: "AGPMAREACHATTING",
    id: ptype::AREA_CHATTING,
    flag_len: 1,
    fields: &[
        f(I8),        // 0 opération (0 message)
        f(I32),       // 1 CID de l'émetteur
        f(I8),        // 2 genre de cri (1 race, 2 monde, 3 tous les mondes), ignoré par le client
        f(I8),        // 3 ignoré par le client (1)
        f(U32),       // 4 type AGPMCHATTING du message (chat_type)
        fs(Char, 49), // 5 nom de l'émetteur
        f(MemBlock),  // 6 texte, sans zéro final
    ],
};

/// AgpmEventCharCustomize (0x43) — drapeaux 1 (constructeur 0x14022ceb0 d'Archonia) : changement d'apparence chez les
/// coiffeurs (fonction de PNJ 29), ops `customize_op` ; le protocole n'a aucun code de résultat.
pub static EVENT_CHAR_CUSTOMIZE: Schema = Schema {
    name: "AGPMEVENTCHARCUSTOMIZE",
    id: ptype::EVENT_CHAR_CUSTOMIZE,
    flag_len: 1,
    fields: &[
        f(I8),     // 0 opération
        f(Packet), // 1 événement (EVENT_BASE, fonction 29)
        f(I32),    // 2 CID du joueur
        f(I8),     // 3 visage choisi (-1 : inchangé)
        f(I8),     // 4 coiffure choisie (-1 : inchangée)
    ],
};

/// AgpmWantedCriminal (0x46) — drapeaux 2 (constructeur FUN_14028a080 d'Archonia, réception FUN_14028a700) : avis de
/// recherche (fonction de PNJ 32 et fenêtre de mort), ops `wanted_op`. Mini-paquet `EVENT_BASE` (ops 0, 1) ou
/// `mini::WANTED_CRIMINAL` (autres ops).
pub static WANTED_CRIMINAL: Schema = Schema {
    name: "AGPMWANTEDCRIMINAL",
    id: ptype::WANTED_CRIMINAL,
    flag_len: 2,
    fields: &[
        f(I8),     // 0 opération
        f(I32),    // 1 CID du personnage concerné
        f(I32),    // 2 page (ops 3, 4 ; -100 : fin de page)
        f(I32),    // 3 rang (ops 4, 5)
        f(I32),    // 4 résultat (op 2), drapeau « recherché » (op 7), genre de notification (op 8)
        f(Packet), // 5 EVENT_BASE ou mini WANTED_CRIMINAL
    ],
};

/// Tous les schémas principaux, pour l'aiguillage par type.
pub static MAIN: &[&Schema] = &[
    &CONFIG, &CHARACTER, &CHAR_MANAGER, &LOGIN, &TIMER, &WORLD, &CHATTING, &ADMIN, &ITEM, &EVENT_NPC_TRADE,
    &EVENT_NPC_DIALOG, &EVENT_MANAGER, &SKILL, &EVENT_SKILL_MASTER, &EVENT_TELEPORT, &EVENT_BANK, &ITEM_CONVERT, &EVENT_ITEM_CONVERT, &REFINERY, &EVENT_REFINERY, &GUILD, &EVENT_GUILD, &GUILD_WAREHOUSE, &PRIVATE_TRADE, &PARTY, &PARTY_MANAGEMENT, &QUEST, &EVENT_QUEST, &UI_STATUS, &SYSTEM_MESSAGE, &RETURN_TO_LOGIN, &OPT_CHAR_MOVE, &OPT_CHAR_ACTION, &OPT_VIEW,
    &STARTUP_ENCRYPTION, &ARCHCORE_EXT, &RECONNECT, &PVP, &REMISSION, &PRODUCT, &EVENT_PRODUCT, &BUDDY, &MAILBOX,
    &EVENT_ITEM_REPAIR, &RIDE, &AUCTION, &EVENT_NATURE, &NATURE_EFFECT, &ITEM_SET, &SUMMONS, &CHANNEL, &AREA_CHATTING,
    &EVENT_CHAR_CUSTOMIZE, &WANTED_CRIMINAL,
];

pub fn main_by_id(id: u8) -> Option<&'static Schema> {
    MAIN.iter().copied().find(|s| s.id == id)
}

// ---------------------------------------------------------------------------------------------
// Mini-paquets (identifiants internes, jamais transmis)
// ---------------------------------------------------------------------------------------------
pub mod mini {
    use super::*;

    pub static LOGIN_SERVER_ADDR: Schema =
        Schema { name: "AGPMLOGIN_SERVER_ADDR", id: 100, flag_len: 1, fields: &[fs(Char, 23), fs(Char, 23), fs(Char, 23)] };

    pub static LOGIN_CHAR_INFO: Schema = Schema {
        name: "AGPMLOGIN_CHAR_INFO",
        id: 101,
        flag_len: 2,
        fields: &[
            f(I32),       // 0 TID
            fs(Char, 49), // 1 nom
            f(I32),       // 2 nombre de personnages / max
            f(I32),       // 3 slot
            f(I32),       // 4 union
            f(I32),       // 5 race
            f(I32),       // 6 cheveux
            f(I32),       // 7 visage
            fs(Char, 49), // 8
            f(I32),       // 9
        ],
    };

    pub static LOGIN_SERVER_INFO: Schema = Schema { name: "AGPMLOGIN_SERVER_INFO", id: 102, flag_len: 1, fields: &[fs(Char, 23)] };

    pub static LOGIN_VERSION_INFO: Schema =
        Schema { name: "AGPMLOGIN_VERSION_INFO", id: 103, flag_len: 1, fields: &[f(I32), f(I32)] };

    /// Déplacement (AgpmCharacter, drapeaux 1).
    pub static CHAR_MOVE: Schema = Schema {
        name: "AGPMCHAR_MOVE",
        id: 105,
        flag_len: 1,
        fields: &[f(Vec3), f(Vec3), f(I32), f(I32), f(F32), f(F32), f(I8), f(I8)],
    };

    pub static CHAR_ACTION: Schema = Schema {
        name: "AGPMCHAR_ACTION",
        id: 106,
        flag_len: 2,
        fields: &[f(I8), f(I32), f(I32), f(I8), f(Packet), f(Vec3), f(U8), f(I8), f(U32), f(U8)],
    };

    pub static CHAR_FACTORS: Schema =
        Schema { name: "AGPMCHAR_FACTORS", id: 107, flag_len: 2, fields: &rep::<13>(Packet) };
    pub static FACTOR_STATUS: Schema = Schema { name: "AGPMCHAR_FACTOR_STATUS", id: 108, flag_len: 2, fields: &rep::<13>(I32) };
    pub static FACTOR_TYPE: Schema = Schema { name: "AGPMCHAR_FACTOR_TYPE", id: 109, flag_len: 1, fields: &rep::<3>(I32) };
    pub static FACTOR_POINT: Schema = Schema { name: "AGPMCHAR_FACTOR_POINT", id: 110, flag_len: 4, fields: &rep::<20>(I32) };
    pub static FACTOR_POINTMAX: Schema = Schema { name: "AGPMCHAR_FACTOR_POINTMAX", id: 111, flag_len: 2, fields: &rep::<11>(I32) };
    pub static FACTOR_RECOVERY: Schema = Schema { name: "AGPMCHAR_FACTOR_RECOVERY", id: 112, flag_len: 1, fields: &rep::<3>(I32) };
    pub static FACTOR_ATTRIBUTE: Schema = Schema { name: "AGPMCHAR_FACTOR_ATTRIBUTE", id: 113, flag_len: 2, fields: &rep::<15>(I32) };
    pub static FACTOR_DMG: Schema = Schema { name: "AGPMCHAR_FACTOR_DMG", id: 114, flag_len: 1, fields: &rep::<2>(Packet) };
    pub static FACTOR_DEFENSE: Schema = Schema { name: "AGPMCHAR_FACTOR_DEFENSE", id: 115, flag_len: 1, fields: &rep::<2>(Packet) };
    pub static FACTOR_ATTACK: Schema = Schema { name: "AGPMCHAR_FACTOR_ATTACK", id: 116, flag_len: 1, fields: &rep::<8>(I32) };
    pub static FACTOR_ITEM: Schema = Schema { name: "AGPMCHAR_FACTOR_ITEM", id: 117, flag_len: 1, fields: &rep::<5>(I32) };
    pub static FACTOR_DIRT: Schema = Schema { name: "AGPMCHAR_FACTOR_DIRT", id: 118, flag_len: 1, fields: &rep::<8>(I32) };
    pub static FACTOR_PRICE: Schema = Schema { name: "AGPMCHAR_FACTOR_PRICE", id: 119, flag_len: 1, fields: &rep::<4>(I32) };

    /// Vue d'un personnage (AgpmOptimizedPacket2) — drapeaux 4.
    pub static OPT_VIEW_CHAR: Schema = Schema {
        name: "AGPMOPTIMIZEDPACKET_VIEW",
        id: 120,
        flag_len: 4,
        fields: &[
            f(I32), f(I32), f(MemBlock), f(I8), f(I8), f(Packet), f(I8), f(I8), f(Vec3), f(Vec3), f(I32), f(U16),
            f(I16), f(I16), f(U64), f(I8), f(I8), f(I8), f(I8), f(U16), f(I8), f(I32), fs(Char, 16), f(I32), f(I32),
            f(I32),
        ],
    };

    /// Désignation d'un événement (ApmEventManager) — drapeaux 1 : type de la source, identifiant de la
    /// source, identifiant d'événement, fonction.
    /// Rémission (AgpmRemission op 2) — drapeaux 1 : type (4 lettres de pardon), résultat.
    pub static REMISSION: Schema = Schema { name: "AGPMREMISSION_MINI", id: 90, flag_len: 1, fields: &[f(I8), f(I32)] };

    pub static EVENT_BASE: Schema =
        Schema { name: "APMEVENTMANAGER_BASE", id: 60, flag_len: 1, fields: &[f(I8), f(I32), f(I32), f(I32)] };

    /// Données d'une fonction d'événement réduites à un entier.
    pub static EVENT_DATA_INT32: Schema = Schema { name: "APMEVENT_DATA_INT32", id: 61, flag_len: 1, fields: &[f(I32)] };

    /// Données MJ d'un personnage (AgpmAdmin) — drapeaux 1.
    pub static ADMIN_CHAR: Schema = Schema {
        name: "AGPMADMIN_CHAR",
        id: 122,
        flag_len: 1,
        fields: &[f(I8), f(I32), fs(Char, 49), f(I16), f(I8), fs(Char, 49)],
    };

    /// Case dans une grille (inventaire, boutique, banque) — drapeaux 1.
    pub static ITEM_GRID: Schema = Schema { name: "AGPMITEM_GRID", id: 123, flag_len: 1, fields: &rep::<3>(I16) };

    /// Position d'un objet au sol (AGPMITEM champ 6), drapeaux 1 : un VEC3F (relevé dans le client x64).
    pub static ITEM_FIELD: Schema = Schema { name: "AGPMITEM_FIELD", id: 140, flag_len: 1, fields: &[f(Vec3)] };

    /// TID de ce qui remplit les sockets d'un objet (AGPMITEMCONVERT champ 9), dans l'ordre — drapeaux 1.
    pub static ITEM_CONVERT_TIDS: Schema = Schema { name: "AGPMITEMCONVERT_TID", id: 141, flag_len: 1, fields: &rep::<8>(I32) };

    /// Options d'un objet (AGPMITEM champ 20) : TID de 5 options au plus — drapeaux 1.
    pub static ITEM_OPTION: Schema = Schema { name: "AGPMITEM_OPTION", id: 142, flag_len: 1, fields: &rep::<5>(U16) };

    /// Désignation dans AGPMSKILL (type d'objet, identifiant) — drapeaux 1.
    pub static SKILL_BASE: Schema = Schema { name: "AGPMSKILL_BASE", id: 150, flag_len: 1, fields: &[f(I8), f(I32)] };

    /// Action d'une compétence — drapeaux 2.
    pub static SKILL_ACTION: Schema = Schema {
        name: "AGPMSKILL_ACTION",
        id: 151,
        flag_len: 2,
        fields: &[
            f(I8),       // 0 type d'action (0 début, 1 lancement, 2 résultat, 5 raté…)
            f(Packet),   // 1 cible (SKILL_BASE)
            f(Packet),   // 2 facteurs du résultat (AGPMCHAR_FACTORS)
            f(Vec3),     // 3 position visée
            f(I8),       // 4 attaque forcée
            f(U32),      // 5 durée d'incantation (ms)
            f(U32),      // 6 durée de l'effet (ms)
            f(U32),      // 7 délai de relance (ms)
            f(U8),       // 8 niveau de la compétence
            f(U8),       // 9 facteurs du résultat à mettre en file
            f(U32),      // 10 effet supplémentaire
            f(MemBlock), // 11 CID des cibles
        ],
    };

    /// Effet actif d'une compétence — drapeaux 2.
    pub static SKILL_BUFF: Schema =
        Schema { name: "AGPMSKILL_BUFF", id: 152, flag_len: 2, fields: &[f(I32), f(U64), f(I32), f(U8), f(U64), f(MemBlock)] };

    /// Quête du journal (AGPMQUEST champ 3) — drapeaux 1 : TID, monstres A et B tués, objectifs remplis,
    /// valeur libre, texte.
    pub static QUEST_INFO: Schema =
        Schema { name: "AGPMQUEST_INFO", id: 154, flag_len: 1, fields: &[f(I32), f(I32), f(I32), f(I32), f(I32), fs(Char, 49)] };

    /// Membre d'une guilde (AGPMGUILD champ 16) — drapeaux 1 : nom, rang, date d'adhésion, niveau, TID,
    /// statut (0 hors ligne, 1 en ligne).
    pub static GUILD_MEMBER: Schema =
        Schema { name: "AGPMGUILD_MEMBER", id: 155, flag_len: 1, fields: &[fs(Char, 49), f(I32), f(I64), f(U64), f(I32), f(I8)] };

    /// Liste des guildes (AGPMGUILD champ 21) — drapeaux 4 : nombre total de guildes, page, nombre de
    /// guildes de la page (INT16), puis 19 entrées GUILD_LIST_ITEM.
    pub static GUILD_LIST: Schema = Schema {
        name: "AGPMGUILD_LIST",
        id: 156,
        flag_len: 4,
        fields: &[
            f(I32),
            f(I32),
            f(I16),
            f(Packet),
            f(Packet),
            f(Packet),
            f(Packet),
            f(Packet),
            f(Packet),
            f(Packet),
            f(Packet),
            f(Packet),
            f(Packet),
            f(Packet),
            f(Packet),
            f(Packet),
            f(Packet),
            f(Packet),
            f(Packet),
            f(Packet),
            f(Packet),
            f(Packet),
        ],
    };

    /// Combat de guildes (AGPMGUILD champ 17) — drapeaux 2 : guilde adverse, chef adverse, acceptation, délai,
    /// début (secondes), type, durée (s), nombre de combattants, horloge, score, score adverse, gain, gain
    /// adverse, résultat, manche.
    pub static GUILD_BATTLE: Schema = Schema {
        name: "AGPMGUILD_BATTLE",
        id: 158,
        flag_len: 2,
        fields: &[fs(Char, 33), fs(Char, 49), f(U64), f(U64), f(U64), f(U32), f(U32), f(U32), f(U64), f(I32), f(I32), f(I32), f(I32), f(I8), f(U32)],
    };

    /// Entrée de la liste des guildes — drapeaux 2 : nom, chef, second, niveau, (inconnu), membres, taille,
    /// (inconnu), emblème (TID, couleur), points de combat.
    pub static GUILD_LIST_ITEM: Schema = Schema {
        name: "AGPMGUILD_LIST_ITEM",
        id: 157,
        flag_len: 2,
        fields: &[fs(Char, 33), fs(Char, 49), fs(Char, 49), f(I32), f(U32), f(I32), f(I32), f(I32), f(I32), f(I32), f(I32)],
    };

    /// Courrier (AGPMMAILBOX champ 2) — drapeaux 2 : identifiant, expéditeur, destinataire (nom exact du
    /// personnage, comparé avec la casse), objet, date (secondes Unix), drapeaux (0x01 lu, 0x10 pièce récupérée),
    /// TID et quantité de la pièce jointe, texte (zéro final compris, 1 à 1000 octets).
    pub static MAIL: Schema = Schema {
        name: "AGPMMAILBOX_MAIL",
        id: 159,
        flag_len: 2,
        fields: &[f(I32), fs(Char, 33), fs(Char, 33), fs(Char, 51), f(I32), f(I32), f(I32), f(I16), f(MemBlock)],
    };

    /// Vente de la maison des ventes (AGPMAUCTION champ 2, ops 1 à 3) — drapeaux 2 : 0 identifiant de vente, 1 clé de
    /// recherche (docID), 2 numéro de l'objet en base, 3 IID, 4 prix en or, 5 prix en CC, 6 quantité, 7 statut (0 en
    /// vente, 1 vendu), 8 date d'enregistrement, 9 TID.
    pub static AUCTION_SALES: Schema = Schema {
        name: "AGPMAUCTION_SALES",
        id: 160,
        flag_len: 2,
        fields: &[f(I32), f(U64), f(U64), f(I32), f(I64), f(I64), f(I16), f(I16), fs(Char, 33), f(I32)],
    };

    /// Recherche (op 12) — drapeaux 1 : 0 TID, 1 curseur (docID), 2 sens (0 avant le curseur, 1 après), 3 nombre de
    /// ventes du TID (réponse), 4 résultats (mini AUCTION_ROWSET, réponse).
    pub static AUCTION_SELECT2: Schema =
        Schema { name: "AGPMAUCTION_SELECT2", id: 161, flag_len: 1, fields: &[f(I32), f(U64), f(I16), f(I32), f(Packet)] };

    /// Mise en vente (op 4) — drapeaux 1 : 0 IID, 1 quantité, 2 prix en or, 3 prix en CC, 4 date, 5 IID d'un parchemin
    /// Marvel (0 aucun).
    pub static AUCTION_SELL: Schema =
        Schema { name: "AGPMAUCTION_SELL", id: 162, flag_len: 1, fields: &[f(I32), f(I16), f(I64), f(I64), fs(Char, 33), f(I32)] };

    /// Annulation (op 5) — drapeaux 1 : 0 identifiant de vente, 1 TID, 2 quantité.
    pub static AUCTION_CANCEL: Schema = Schema { name: "AGPMAUCTION_CANCEL", id: 163, flag_len: 1, fields: &[f(I32), f(I32), f(I16)] };

    /// Encaissement (op 6) — drapeaux 1 : 0 identifiant de vente, 1 TID, 2 quantité vendue, 3 gain en or, 4 gain en CC.
    pub static AUCTION_CONFIRM: Schema =
        Schema { name: "AGPMAUCTION_CONFIRM", id: 164, flag_len: 1, fields: &[f(I32), f(I32), f(I16), f(I64), f(I64)] };

    /// Achat (op 7) — drapeaux 1 : 0 docID, 1 quantité, 2 TID, 3 prix en or, 4 prix en CC (0 dans la demande du client).
    pub static AUCTION_BUY: Schema = Schema { name: "AGPMAUCTION_BUY", id: 165, flag_len: 1, fields: &[f(I64), f(I16), f(I32), f(I64), f(I64)] };

    /// Résultats d'une recherche — drapeaux 2 : 0 index de requête, 1 texte de requête, 2 en-têtes, 3 nombre de lignes,
    /// 4 nombre de colonnes, 5 taille d'une ligne, 6 tampon (lignes × taille), 7 décalage de chaque colonne (U32). Chaque
    /// valeur est une chaîne terminée par un zéro.
    pub static AUCTION_ROWSET: Schema = Schema {
        name: "AGPMAUCTION_ROWSET",
        id: 166,
        flag_len: 2,
        fields: &[f(I32), f(MemBlock), f(MemBlock), f(U32), f(U32), f(U32), f(MemBlock), f(MemBlock)],
    };

    /// Avis de recherche (AgpdWantedCriminal) — drapeaux 2 : nom (criminel, tueur ou chasseur selon l'op), montant (prime
    /// cumulée, prime posée, or gagné ou expérience perdue), niveau, TID du modèle, en ligne (≠ 0).
    pub static WANTED_CRIMINAL: Schema =
        Schema { name: "AGPMWANTEDCRIMINAL_MINI", id: 167, flag_len: 2, fields: &[fs(Char, 33), f(I64), f(I32), f(I32), f(I32)] };

    /// Données d'un maître des compétences (race, classe enseignées) — drapeaux 1.
    pub static SKILL_MASTER_DATA: Schema = Schema { name: "AGPMEVENTSKILLMASTER_DATA", id: 153, flag_len: 1, fields: &[f(I8), f(I8)] };

    /// Détail d'un objet de boutique (AgcmEventNPCTrade) — drapeaux 2 : TID, prix, quantité, statut,
    /// sac, ligne, colonne, puis trois entiers.
    pub static NPC_TRADE_DETAIL: Schema = Schema {
        name: "AGCMEVENTNPCTRADE_DETAIL",
        id: 124,
        flag_len: 2,
        fields: &[f(I32), f(I32), f(I32), f(I16), f(I16), f(I16), f(I16), f(I32), f(I32), f(I32)],
    };
}

// ---------------------------------------------------------------------------------------------
// Opérations
// ---------------------------------------------------------------------------------------------
pub mod login_op {
    pub const HASHKEY: i64 = 0;
    pub const USER_LOGIN: i64 = 1;
    pub const UNION_INFO: i64 = 3;
    pub const CHAR_NAME: i64 = 4;
    pub const CHAR_NAME_FINISH: i64 = 5;
    pub const CHAR_LIST: i64 = 6;
    pub const CHAR_LIST_FINISH: i64 = 7;
    pub const WORLD_ENTER: i64 = 8;
    pub const CHAR_CREATION_ENTER: i64 = 10;
    pub const CREATE_CHAR: i64 = 11;
    pub const CREATE_CHAR_FINISH: i64 = 12;
    pub const DELETE_CHAR: i64 = 13;
    pub const LOGIN_RESULT: i64 = 15;
}

pub mod login_result {
    pub const INCORRECT_USER: i64 = 0;
    pub const GIBBERISH: i64 = 1;
    pub const TOO_MANY_ATTEMPTS: i64 = 2;
    pub const TOS_AGREEMENT: i64 = 3;
    pub const NAME_TAKEN: i64 = 7;
    pub const CANT_DELETE_GUILD_MASTER: i64 = 19;
}

pub mod crypt_op {
    pub const REQUEST: i64 = 0;
    pub const PUBLIC_KEY: i64 = 1;
    pub const PRIVATE_KEY: i64 = 2;
    pub const COMPLETE: i64 = 3;
    pub const SET_ALGO: i64 = 6;
}

pub mod world_op {
    pub const SERVER_LIST_REQ: i64 = 0;
    pub const SERVER_LIST_ANS: i64 = 2;
    pub const SERVER_EXTRA: i64 = 3;
}

/// Types de canal de discussion (champ 2 d'AGPMCHATTING).
///
/// Noms d'après l'affichage d'Archonia (`FUN_140511e20`) : 7, 11 et 12 sont montrés en « [Notice] », 8 à 10 en
/// « [System] » (une ligne répétée en moins de 500 ms y est ignorée) ; 17, 19 et 20 n'ont ni en-tête ni nom : le
/// serveur compose tout le texte.
pub mod chat_type {
    pub const NORMAL: i64 = 0;
    /// Cri local « [Shout] » (`/s`).
    pub const WORD_BALLOON: i64 = 1;
    pub const UNION: i64 = 2;
    pub const GUILD: i64 = 3;
    pub const PARTY: i64 = 4;
    pub const WHISPER: i64 = 5;
    pub const WHISPER2: i64 = 6;
    /// Monde entier, affiché en « [Notice] » : type des messages système d'ArchCore (`send_system`) et des
    /// avertissements `/warn`.
    pub const WHOLE_WORLD: i64 = 7;
    pub const SYSTEM_LEVEL1: i64 = 8;
    pub const SYSTEM_LEVEL2: i64 = 9;
    /// Système 3, affiché en « [System] » : type des annonces d'ArchCore (`broadcast_notice`).
    pub const SYSTEM_LEVEL3: i64 = 10;
    /// Annonce 1, affichée en « [Notice] ».
    pub const NOTICE_LEVEL1: i64 = 11;
    pub const NOTICE_LEVEL2: i64 = 12;
    pub const GUILD_NOTICE: i64 = 13;
    pub const GUILD_JOINT: i64 = 14;
    pub const EMPHASIS: i64 = 15;
    pub const SIEGEWAR: i64 = 16;
    /// Système sans préfixe, en jaune.
    pub const SYSTEM_NO_PREFIX: i64 = 17;
    /// Trompette de race (`/race`), onglet « Race Trumpet ».
    pub const RACE: i64 = 19;
    /// Cri des éléments (`/scream`), en jaune.
    pub const SCREAM: i64 = 20;
}

/// Opérations d'AgpmChannel (champ 0) ; « C » : du client, « S » : du serveur. Les ops 0 et 2 ne servent pas.
pub mod channel_op {
    /// C : créer un salon (nom, mot de passe d'un salon privé, genre).
    pub const CREATE: i64 = 1;
    /// C : entrer dans un salon (nom). S : entrée faite (nom, mot de passe, genre, liste des membres).
    pub const JOIN: i64 = 3;
    /// C : quitter un salon. S : sortie faite (le client ferme la fenêtre).
    pub const LEAVE: i64 = 4;
    /// S : un membre est arrivé (champ 4).
    pub const ADD_MEMBER: i64 = 5;
    /// S : un membre est parti (champ 4).
    pub const REMOVE_MEMBER: i64 = 6;
    /// C : inviter le personnage du champ 4. S : invitation reçue (invitant au champ 4, genre).
    pub const INVITE: i64 = 7;
    /// S : mot de passe demandé. C : mot de passe saisi (champ 3).
    pub const PASSWORD: i64 = 8;
    /// C : message (champ 7). S : message relayé à tous les membres, auteur compris (auteur au champ 4).
    pub const MESSAGE: i64 = 9;
    /// C : chercher un joueur (champ 4). S : joueur en ligne trouvé.
    pub const FIND_USER: i64 = 10;
    /// S : code de résultat (champ 6, channel_result).
    pub const RESULT: i64 = 11;
}

/// Genres de salon (champ 5 d'AgpmChannel).
pub mod channel_kind {
    pub const PUBLIC: i64 = 1;
    /// Mot de passe exigé à l'entrée, sauf sur invitation.
    pub const PRIVATE: i64 = 2;
    pub const GUILD: i64 = 3;
}

/// Codes de résultat d'AgpmChannel (op 11, champ 6) et fenêtre affichée par le client.
pub mod channel_result {
    /// `ChannelAlreadyExist_MsgUI`.
    pub const ALREADY_EXIST: i64 = 1;
    /// `ChannelNotExist_MsgUI`.
    pub const NOT_EXIST: i64 = 2;
    /// `NotExistCharacter_MsgUI` : joueur introuvable (invitation, recherche).
    pub const NO_CHARACTER: i64 = 3;
    /// `ChannelCountFull_MsgUI` : 5 salons déjà, ou salon plein.
    pub const FULL: i64 = 4;
    /// `NotGuildMaster_MsgUI`.
    pub const NOT_GUILD_MASTER: i64 = 5;
    /// `NOtGuildMember_MsgUI`.
    pub const NOT_GUILD_MEMBER: i64 = 6;
    /// `GuildChannelAlreadyExist_MsgUI`.
    pub const GUILD_CHANNEL_EXIST: i64 = 7;
    /// `InvalidPassword_MsgUI`.
    pub const INVALID_PASSWORD: i64 = 8;
    /// sysstr 620 « You already joined the channel ».
    pub const ALREADY_JOINED: i64 = 11;
}

/// Opérations d'AgpmEventCharCustomize (champ 0).
pub mod customize_op {
    /// Client : ouvrir la fenêtre du coiffeur (événement du PNJ).
    pub const REQUEST: i64 = 0;
    /// Serveur : accord (événement renvoyé, CID du joueur) ; le client ouvre `UI_Customizing`.
    pub const GRANT: i64 = 1;
    /// Client : achat du visage (champ 3) et de la coiffure (champ 4), -1 pour ce qui ne change pas.
    pub const BUY: i64 = 2;
}

/// Opérations d'AgpmWantedCriminal (champ 0) ; « C » : du client, « S » : du serveur.
pub mod wanted_op {
    /// C : ouvrir le tableau auprès d'un PNJ de fonction 32. S (op 1) : accord ; le client demande la page 0.
    pub const REQUEST: i64 = 0;
    pub const GRANT: i64 = 1;
    /// C : mettre à prix (mini {nom, prime}). S : résultat (champ 4, wanted_result ; mini avec le nom).
    pub const ARRANGE: i64 = 2;
    /// C : demande d'une page (champ 2, 0 à 9).
    pub const PAGE: i64 = 3;
    /// S : une ligne (page, rang, mini) ; page -100 sans mini : fin de page.
    pub const ENTRY: i64 = 4;
    /// C : son propre rang (toutes les 30 s quand il est recherché). S : rang et prime cumulée.
    pub const MY_RANK: i64 = 5;
    /// S : nom du tueur à la victime (champ 1), qui peut alors le mettre à prix depuis la fenêtre de mort.
    pub const KILLER: i64 = 6;
    /// S : drapeau « recherché » (champ 4 : 1 icône `WantedAlarm`, 0 retirée).
    pub const WANTED: i64 = 7;
    /// S : notification (champ 4, wanted_notice), mini {nom, montant}.
    pub const NOTIFY: i64 = 8;
    /// Page -100 de l'op 4 : fin de la page envoyée.
    pub const END_OF_PAGE: i64 = -100;
}

/// Codes de résultat de la mise à prix (op 2, champ 4).
pub mod wanted_result {
    /// `WC_SUCCEEDED` : le client efface le nom du tueur.
    pub const SUCCESS: i64 = 0;
    /// `WC_NOT_ENOUGH_BASE` : prime inférieure à la prime de base.
    pub const NOT_ENOUGH_BASE: i64 = 1;
    /// `WC_NOT_ENOUGH_MONEY`.
    pub const NOT_ENOUGH_MONEY: i64 = 2;
    /// `WC_ALREADY_ARRANGED` (« The criminal has already been caught. »).
    pub const ALREADY_ARRANGED: i64 = 3;
}

/// Genres de notification de l'op 8 d'AgpmWantedCriminal (champ 4).
pub mod wanted_notice {
    /// `WC_KILL` : le chasseur a attrapé le criminel (nom du criminel, or gagné).
    pub const KILL: i64 = 4;
    /// `WC_DEAD` : le criminel a été tué au nom de la justice (nom du chasseur, expérience perdue).
    pub const DEAD: i64 = 5;
}

pub mod char_mgr_op {
    pub const GAME_ENTER: i64 = 2;
    pub const FINISH_ENTER: i64 = 7;
    pub const LOADING_POS: i64 = 8;
    pub const ENTER_WORLD: i64 = 9;
    pub const AUTH_KEY: i64 = 11;
    pub const KEEPALIVE: i64 = 13;
}

/// Opérations d'AGPMSUMMONS (champ 0).
pub mod summons_op {
    /// Serveur : invocation ajoutée chez le maître (champs 1 à 8), après la vue de la créature et son état spécial.
    pub const ADD: i64 = 0;
    /// Serveur : invocation retirée (champs 1, 2), avant le retrait de la vue de la créature.
    pub const REMOVE: i64 = 1;
    /// Client : propension demandée (champs 1, 2, 5, 8) ; serveur : propension appliquée, renvoyée au maître.
    pub const SET_PROPENSITY: i64 = 2;
    /// Serveur : limite des invocations (champ 4).
    pub const SET_MAX_SUMMONS: i64 = 3;
    /// Serveur : limite des créatures apprivoisées (champ 4).
    pub const SET_MAX_TAME: i64 = 4;
}

/// Opérations d'AGPMBUDDY (champ 0).
pub mod buddy_op {
    /// Client : ajout d'un ignoré, ou acceptation d'une demande d'ami. Serveur : entrée ajoutée.
    pub const ADD: i64 = 1;
    /// Demande d'ami : du demandeur au serveur, puis du serveur à la cible (nom = demandeur).
    pub const REQUEST: i64 = 2;
    /// Demande refusée : de la cible au serveur, puis du serveur au demandeur.
    pub const REJECT: i64 = 3;
    pub const REMOVE: i64 = 4;
    /// Cases de blocage d'un ignoré.
    pub const OPTIONS: i64 = 5;
    /// Serveur : un joueur de la liste entre en jeu ou en sort (bit `ONLINE`).
    pub const ONLINE: i64 = 6;
    /// Région d'un ami (champ 4).
    pub const REGION: i64 = 7;
    /// Serveur : code de résultat (champ 6).
    pub const RESULT: i64 = 8;
    /// Serveur : fin de la liste initiale ; le client répond par l'op 15.
    pub const END_OF_LIST: i64 = 9;
    /// Relation « Buddy » (mentor) : 10 demande, 11 fin, 12 acceptée, 13 refusée, 14 ouvrir la fenêtre.
    pub const MENTOR_REQUEST: i64 = 10;
    pub const MENTOR_OPEN: i64 = 14;
    /// Client : ignorés de `Ini\IgnoreList.txt` (schéma `BUDDY_LIST`).
    pub const LIST: i64 = 15;
}

/// Drapeaux d'une entrée d'AGPMBUDDY (champ 3).
pub mod buddy_flag {
    pub const BLOCK_CHAT: u32 = 0x0000_0001;
    pub const BLOCK_TRADE: u32 = 0x0000_0002;
    /// Invitations de groupe et de guilde.
    pub const BLOCK_INVITE: u32 = 0x0000_0004;
    pub const BLOCKS: u32 = BLOCK_CHAT | BLOCK_TRADE | BLOCK_INVITE;
    pub const ONLINE: u32 = 0x0010_0000;
    /// Entrée de la liste d'amis ; absent : liste d'ignorés.
    pub const FRIEND: u32 = 0x0200_0000;
}

/// Codes de résultat d'AGPMBUDDY (op 8, champ 6) ; le client n'affiche que 1, 2, 3/4, 12 et 14.
pub mod buddy_result {
    /// « déjà dans une liste » (`BuddyAlreayExist_MsgBox`).
    pub const ALREADY: i64 = 1;
    /// « hors ligne ou inexistant » (`BuddyOfflineOrNotExist_MsgBox`).
    pub const OFFLINE_OR_UNKNOWN: i64 = 2;
    /// Liste pleine (sysstr 715 « Cannot add player in the list. »).
    pub const FULL: i64 = 3;
    /// Soi-même ou nom invalide (même message).
    pub const INVALID: i64 = 4;
    /// Demande Buddy invalide (`MentorInvalidProcess`).
    pub const MENTOR_INVALID: i64 = 12;
}

/// Opérations d'AgpmAuction (champ 0). Archonia n'a pas d'op « SELECT » : la recherche est l'op 12.
pub mod auction_op {
    /// Serveur → client : vente ajoutée à la boîte de ventes (SALES complet).
    pub const ADD_SALES: i64 = 1;
    /// Serveur → client : vente retirée (SALES : identifiant seul).
    pub const REMOVE_SALES: i64 = 2;
    /// Serveur → client : statut d'une vente (SALES : identifiant et statut ; 1 vendu).
    pub const UPDATE_SALES: i64 = 3;
    pub const SELL: i64 = 4;
    pub const CANCEL: i64 = 5;
    pub const CONFIRM: i64 = 6;
    pub const BUY: i64 = 7;
    /// Client → serveur : ouverture chez un PNJ de fonction 28 (EVENT_BASE) ; accord par l'op 10.
    pub const EVENT_REQUEST: i64 = 9;
    pub const EVENT_GRANT: i64 = 10;
    pub const SELECT2: i64 = 12;
}

/// Codes de résultat d'AgpmAuction (champ 3) que le client affiche ; tout autre code non nul n'affiche rien.
pub mod auction_result {
    pub const OK: i64 = 0;
    /// Refus sans message du client (accompagné d'un message système d'ArchCore).
    pub const FAILED: i64 = 1;
    /// Achat : quantité incorrecte (sysstr 274).
    pub const QUANTITY: i64 = 5;
    /// Mise en vente : plus de place (sysstr 279).
    pub const TOO_MANY_SALES: i64 = 6;
    /// Achat, annulation : inventaire plein (sysstr 275, 280).
    pub const INVENTORY_FULL: i64 = 7;
    /// Achat : vente plus disponible (sysstr 276).
    pub const NOT_AVAILABLE: i64 = 12;
}

/// Opérations d'AgpmItemSet (champ 0).
pub mod item_set_op {
    /// Client → serveur : enregistrer l'équipement porté dans l'emplacement.
    pub const SAVE: i64 = 0;
    /// Client → serveur : porter le jeu de l'emplacement (vide : tout retirer).
    pub const EQUIP: i64 = 1;
    /// Client → serveur : vider l'emplacement.
    pub const CLEAR: i64 = 2;
    /// Serveur → client : emplacement rempli ou vide (champ 2).
    pub const FLAG: i64 = 3;
    /// Serveur → client : emplacement actif (champ 1, -1 aucun).
    pub const ACTIVE: i64 = 4;
    /// Serveur → client : objets de l'emplacement (champ 3).
    pub const CONTENT: i64 = 5;
    /// Client → serveur : déplacer un objet du jeu (champ 2 : (de << 4) | vers).
    pub const MOVE: i64 = 6;
    /// Client → serveur : retirer un objet du jeu (champ 2 : position).
    pub const REMOVE: i64 = 7;
}

/// Météo d'AgpmEventNature (champ 0) qui a un effet dans le client ; toute autre valeur ne montre rien.
pub mod weather {
    /// Fin de l'effet de nature, en fondu.
    pub const CLEAR: i64 = 0;
    /// Pluie (effet de nature 1, ciel 99).
    pub const RAIN: i64 = 3;
    /// Neige (effet de nature 0, ciel 98).
    pub const SNOW: i64 = 5;
}

/// Opérations d'AGPMMAILBOX (champ 0).
pub mod mail_op {
    /// Serveur : un courrier arrive dans la boîte (en tête de liste).
    pub const ADD: i64 = 0;
    pub const REMOVE: i64 = 1;
    /// Client : courrier ouvert (sans réponse attendue).
    pub const READ: i64 = 2;
    pub const WRITE: i64 = 3;
    /// Pièce jointe récupérée dans l'inventaire.
    pub const ITEM_SAVE: i64 = 4;
    /// Serveur : boîte vidée.
    pub const REMOVE_ALL: i64 = 5;
}

/// Codes de résultat d'AGPMMAILBOX (champ 4) : le client ne distingue que 0 (succès) du reste. 5, 7, 8 et 10
/// reprennent les codes du contrôle local de la pièce jointe (FUN_1402ece60) ; les autres sont d'ArchCore.
pub mod mail_result {
    pub const OK: i64 = 0;
    pub const FAILED: i64 = 1;
    pub const NO_RECIPIENT: i64 = 2;
    pub const BOX_FULL: i64 = 3;
    pub const NO_PAPER: i64 = 4;
    pub const FORBIDDEN_ITEM: i64 = 5;
    pub const INVENTORY_FULL: i64 = 6;
    pub const BOUND_ITEM: i64 = 7;
    pub const NOT_IN_INVENTORY: i64 = 8;
    pub const CASH_ITEM: i64 = 10;
    /// Suppression d'un courrier dont la pièce jointe n'est pas récupérée.
    pub const ITEM_PENDING: i64 = 11;
}

/// Opérations du paquet d'extension (champ 0 d'ARCHCORE_EXT).
pub mod ext_op {
    pub const HELLO: i64 = 0;
    pub const HELLO_ACK: i64 = 1;
    pub const ADDON_SEND: i64 = 2;
    pub const ADDON_RECV: i64 = 3;
    pub const RESULT: i64 = 4;
    /// Capacité `language/1` (v1.1) : le client change de langue (champ 16).
    pub const SET_LANGUAGE: i64 = 5;
    /// Réponse : langue appliquée (16) et langues disponibles (17).
    pub const LANGUAGE_ACK: i64 = 6;
    /// Textes localisés demandés par le client (18 sorte, 19 identifiants).
    pub const TEXT_REQUEST: i64 = 7;
    /// Textes rendus (16 langue servie, 18 sorte, 20 textes).
    pub const TEXT_REPLY: i64 = 8;
}

/// Sortes de textes localisés (champ 18 d'ARCHCORE_EXT) ; même nom que la colonne `kind` de
/// `localized_text`. L'identifiant (champ 19) est celui de la table d'origine.
pub mod ext_text_kind {
    /// `npc_spawn.id`
    pub const NPC_NAME: &str = "npc_name";
    /// TID du monstre (`monster_spawn.tid`)
    pub const MONSTER_NAME: &str = "monster_name";
    /// `item_data.tid`
    pub const ITEM_NAME: &str = "item_name";
    pub const ITEM_DESC: &str = "item_desc";
    /// `quest_template.tid`
    pub const QUEST_NAME: &str = "quest_name";
    pub const QUEST_DESC: &str = "quest_desc";
    /// `skill_template.tid`
    pub const SKILL_NAME: &str = "skill_name";
    /// `region_template.id`
    pub const REGION_NAME: &str = "region_name";
    /// Identifiants d'un dialogue de PNJ (`npcdialog` du client) — réservé
    pub const NPC_DIALOG: &str = "npc_dialog";
    pub const ALL: &[&str] = &[NPC_NAME, MONSTER_NAME, ITEM_NAME, ITEM_DESC, QUEST_NAME, QUEST_DESC, SKILL_NAME, REGION_NAME, NPC_DIALOG];
    /// Identifiants au plus par `TEXT_REQUEST`.
    pub const MAX_IDS: usize = 64;
}

/// Portées d'un message d'AddOn (champ 10 d'ARCHCORE_EXT).
pub mod ext_channel {
    pub const WHISPER: i64 = 1;
    pub const SAY: i64 = 2;
    pub const PARTY: i64 = 3;
    pub const GUILD: i64 = 4;
    pub const RAID: i64 = 5;
    pub const CHANNEL: i64 = 6;
    pub const SERVER: i64 = 7;
}

/// Codes de résultat (champ 15 d'ARCHCORE_EXT).
pub mod ext_result {
    pub const OK: i64 = 0;
    pub const INVALID_PREFIX: i64 = 1;
    pub const MESSAGE_TOO_LONG: i64 = 2;
    pub const THROTTLED: i64 = 3;
    pub const TARGET_OFFLINE: i64 = 4;
    pub const NOT_AVAILABLE: i64 = 5;
    pub const NOT_IN_GROUP: i64 = 6;
    pub const NOT_ALLOWED: i64 = 7;
    pub const TARGET_NO_EXTENSION: i64 = 8;
    /// v1.1 : demande mal formée (langue invalide, sorte de texte inconnue, trop d'identifiants) ou
    /// capacité `language` non accordée.
    pub const INVALID_REQUEST: i64 = 9;
}
