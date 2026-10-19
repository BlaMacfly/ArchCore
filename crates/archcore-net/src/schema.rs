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
    pub const PARTY: u8 = 0x08;
    pub const LOGIN: u8 = 0x0d;
    pub const CHATTING: u8 = 0x0f;
    pub const TIMER: u8 = 0x11;
    pub const ADMIN: u8 = 0x12;
    pub const QUEST: u8 = 0x13;
    pub const UI_STATUS: u8 = 0x14;
    pub const GUILD: u8 = 0x15;
    pub const WORLD: u8 = 0x18;
    pub const RETURN_TO_LOGIN: u8 = 0x2f;
    pub const EVENT_NPC_TRADE: u8 = 0x32;
    pub const EVENT_NPC_DIALOG: u8 = 0x37;
    pub const EVENT_MANAGER: u8 = 0x3c;
    pub const OPT_CHAR_MOVE: u8 = 0x40;
    pub const OPT_CHAR_ACTION: u8 = 0x41;
    pub const OPT_VIEW: u8 = 0x42;
    pub const SYSTEM_MESSAGE: u8 = 0x44;
    pub const STARTUP_ENCRYPTION: u8 = 0x48;
    pub const TITLE: u8 = 0x60;
    pub const RECONNECT: u8 = 0x63;
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

/// AgpmTimer — drapeaux 1.
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
    ],
};

/// Tous les schémas principaux, pour l'aiguillage par type.
pub static MAIN: &[&Schema] = &[
    &CONFIG, &CHARACTER, &CHAR_MANAGER, &LOGIN, &TIMER, &WORLD, &CHATTING, &ADMIN, &ITEM, &EVENT_NPC_TRADE,
    &EVENT_NPC_DIALOG, &EVENT_MANAGER, &SYSTEM_MESSAGE, &RETURN_TO_LOGIN, &OPT_CHAR_MOVE, &OPT_CHAR_ACTION, &OPT_VIEW,
    &STARTUP_ENCRYPTION, &ARCHCORE_EXT,
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
pub mod chat_type {
    pub const NORMAL: i64 = 0;
    pub const WORD_BALLOON: i64 = 1;
    pub const UNION: i64 = 2;
    pub const GUILD: i64 = 3;
    pub const PARTY: i64 = 4;
    pub const WHISPER: i64 = 5;
    pub const SYSTEM: i64 = 7;
    pub const NOTICE: i64 = 10;
}

pub mod char_mgr_op {
    pub const GAME_ENTER: i64 = 2;
    pub const FINISH_ENTER: i64 = 7;
    pub const LOADING_POS: i64 = 8;
    pub const ENTER_WORLD: i64 = 9;
    pub const AUTH_KEY: i64 = 11;
    pub const KEEPALIVE: i64 = 13;
}

/// Opérations du paquet d'extension (champ 0 d'ARCHCORE_EXT).
pub mod ext_op {
    pub const HELLO: i64 = 0;
    pub const HELLO_ACK: i64 = 1;
    pub const ADDON_SEND: i64 = 2;
    pub const ADDON_RECV: i64 = 3;
    pub const RESULT: i64 = 4;
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
}
