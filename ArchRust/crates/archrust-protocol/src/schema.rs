//! Schémas des paquets ArchLord connus.
//!
//! Relevés dans l'exécutable : chaque module `Agpm*`/`Agcm*` déclare ses champs avec
//! `AuPacket::SetFieldType` dans son constructeur, et la longueur des drapeaux avec `SetFlagLength`.
//! Le relevé exhaustif des 86 modules est dans `ArchCore/data/re/packet-schemas.json` ; ici ne
//! figurent que les schémas utilisés et validés avec le vrai client.
//!
//! Un champ = `(type, taille)`. Pour `Char`, la taille est le nombre d'octets ; pour les scalaires
//! c'est un nombre de répétitions ; pour `Packet` une taille > 1 signifie « liste de mini-paquets
//! précédée d'un compteur u8 ».

/// Types de champs d'`AuPacket` (valeurs internes du client).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FieldType {
    Char = 0,
    Int8 = 1,
    UInt8 = 2,
    Int16 = 3,
    UInt16 = 4,
    Int32 = 5,
    UInt32 = 6,
    Int64 = 7,
    UInt64 = 8,
    Float = 9,
    Vec3F = 10,
    Matrix = 11,
    Packet = 12,
    MemoryBlock = 13,
    PosBaseMeter = 14,
    WChar = 15,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    pub ty: FieldType,
    pub size: u16,
}

#[derive(Debug)]
pub struct Schema {
    pub name: &'static str,
    /// Type de paquet principal, ou identifiant interne (≥ 100) pour un mini-paquet.
    pub id: u8,
    pub flag_len: u8,
    pub fields: &'static [Field],
}

macro_rules! f {
    ($ty:ident) => {
        Field {
            ty: FieldType::$ty,
            size: 1,
        }
    };
    ($ty:ident, $n:expr) => {
        Field {
            ty: FieldType::$ty,
            size: $n,
        }
    };
}

macro_rules! schema {
    ($vis:vis $name:ident = $label:literal, $id:expr, $flags:expr, [$($field:expr),* $(,)?]) => {
        $vis static $name: &Schema = &Schema { name: $label, id: $id, flag_len: $flags, fields: &[$($field),*] };
    };
}

/// Types de paquets principaux.
pub mod packet_type {
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
    pub const EVENT_MANAGER: u8 = 0x3c;
    pub const OPT_CHAR_MOVE: u8 = 0x40;
    pub const OPT_CHAR_ACTION: u8 = 0x41;
    pub const OPT_VIEW: u8 = 0x42;
    pub const SYSTEM_MESSAGE: u8 = 0x44;
    pub const STARTUP_ENCRYPTION: u8 = 0x48;
    pub const TITLE: u8 = 0x60;
    pub const RECONNECT: u8 = 0x63;
}

/// Schémas des paquets principaux.
pub mod main {
    use super::packet_type as T;
    use super::*;

    // AgpmConfig — drapeaux 4
    schema!(pub CONFIG = "AGPMCONFIG", T::CONFIG, 4, [f!(Int32), f!(Int32), f!(Int8)]);

    // AgpmCharacter — drapeaux 4
    schema!(pub CHARACTER = "AGPMCHARACTER", T::CHARACTER, 4, [
        f!(Int8),           // 0 opération
        f!(Int32),          // 1 CID
        f!(Int32),          // 2 TID (modèle)
        f!(MemoryBlock),    // 3 nom
        f!(Int8),           // 4 statut
        f!(Packet),         // 5 mini déplacement (CHAR_MOVE)
        f!(Packet),         // 6 mini action
        f!(Packet),         // 7 mini facteurs
        f!(Int64),          // 8 argent inventaire
        f!(Int64),          // 9 argent banque
        f!(Int64),          // 10 cash
        f!(Int8),           // 11 statut d'action
        f!(Int8),           // 12 statut criminel
        f!(Int32),          // 13 attaquant
        f!(Int8),           // 14 nouveau perso
        f!(UInt8),          // 15 dernier point de liaison
        f!(UInt8),          // 16 animation sociale
        f!(UInt64),         // 17 statut spécial
        f!(Int8),           // 18 transformation
        f!(Char, 32),       // 19 skill init
        f!(Int8),           // 20 visage
        f!(Int8),           // 21 cheveux
        f!(Int32),          // 22 option flag
        f!(Int8),           // 23 taille de banque
        f!(UInt16),         // 24 event status flag
        f!(Int64),          // 25 temps criminel restant
        f!(Int64),          // 26 temps meurtrier restant
        f!(Char, 16),       // 27 signature
        f!(MemoryBlock),    // 28 données GameGuard
        f!(UInt64),         // 29 dernière mort en Battle Square
    ]);

    // AgpmItem — drapeaux 4 (constructeur relevé dans ap_item.c).
    schema!(pub ITEM = "AGPMITEM", T::ITEM, 4, [
        f!(UInt8), f!(Int8), f!(Int32), f!(Int32), f!(Int32), f!(Int32),
        f!(Packet), f!(Packet), f!(Packet), f!(Packet), f!(Packet), f!(Packet),
        f!(Int32), f!(Packet), f!(Packet), f!(Packet), f!(Packet), f!(Int32),
        f!(UInt32), f!(Int32), f!(Packet), f!(Packet), f!(UInt32), f!(Packet), f!(Packet),
    ]);

    // AgcmCharacter (type 3) — drapeaux 4 (SetFlagLength(4))
    schema!(pub CHAR_MANAGER = "AGSMCHARMANAGER", T::CHAR_MANAGER, 4, [
        f!(Int8),           // 0 opération
        f!(Char, 12),       // 1
        f!(Int32),          // 2
        f!(Char, 49),       // 3 nom de personnage
        f!(Int32),          // 4 CID
        f!(Vec3F),          // 5 position
        f!(Int32),          // 6 clé d'authentification
        f!(Char, 248),      // 7 (nouveau dans le client x64)
    ]);

    // AgpmLogin — drapeaux 2
    schema!(pub LOGIN = "AGPMLOGIN", T::LOGIN, 2, [
        f!(Int8),           // 0 opération
        f!(Char, 32),       // 1 clé de hachage (8 octets utiles)
        f!(Char, 49),       // 2 compte (chiffré RC4)
        f!(Int8),           // 3 longueur du compte
        f!(Char, 65),       // 4 mot de passe (chiffré RC4)
        f!(Int8),           // 5 longueur du mot de passe
        f!(Int32),          // 6 CID
        f!(Char, 32),       // 7 nom du monde
        f!(Packet),         // 8 mini CHAR_INFO
        f!(Packet),         // 9 mini VERSION_INFO
        f!(Int32),          // 10 résultat
        f!(Packet),         // 11 mini SERVER_INFO
        f!(Char, 2049),     // 12 extra
        f!(Char, 5),        // 13 code
        f!(Int32),          // 14 compte limité
        f!(Int32),          // 15 compte protégé
    ]);

    // AgpmTimer — drapeaux 1
    schema!(pub TIMER = "AGPMTIMER", T::TIMER, 1, [f!(UInt8), f!(UInt64), f!(UInt32), f!(UInt32)]);

    // AgpmWorld — drapeaux 1
    schema!(pub WORLD = "AGPMWORLD", T::WORLD, 1, [f!(Int8), f!(Char, 32), f!(Int16), f!(MemoryBlock), f!(Char, 32)]);

    // AgpmChatting — drapeaux 1
    schema!(pub CHATTING = "AGPMCHATTING", T::CHATTING, 1, [
        f!(Int8), f!(Int32), f!(Int8), f!(Int32), f!(Char, 49), f!(Char, 49), f!(MemoryBlock),
    ]);

    // AgpmSystemMessage — drapeaux 1
    schema!(pub SYSTEM_MESSAGE = "AGPMSYSTEMMESSAGE", T::SYSTEM_MESSAGE, 1, [
        f!(Int8), f!(Int32), f!(Int32), f!(Int32), f!(Char, 65), f!(Char, 65),
    ]);

    // AgpmReturnToLogin — drapeaux 1
    schema!(pub RETURN_TO_LOGIN = "AGPMRETURNTOLOGIN", T::RETURN_TO_LOGIN, 1, [
        f!(Int8), f!(Int32), f!(Char, 49), f!(Char, 32), f!(Char, 25), f!(Int32),
    ]);

    // AgpmOptimizedPacket2 (0x40) — drapeaux 1
    schema!(pub OPT_CHAR_MOVE = "AGPMOPTIMIZEDCHARMOVE", T::OPT_CHAR_MOVE, 1, [
        f!(Int32),          // 0 CID
        f!(Vec3F),          // 1 position
        f!(Vec3F),          // 2 destination
        f!(Int32),          // 3 cible suivie
        f!(UInt16),         // 4 distance de suivi
        f!(Int8),           // 5 drapeaux de déplacement
        f!(Int8),           // 6 direction
        f!(Int8),           // 7
        f!(Int32),          // 8
    ]);

    // AgpmOptimizedPacket2 (0x41) — drapeaux 2
    schema!(pub OPT_CHAR_ACTION = "AGPMOPTIMIZEDCHARACTION", T::OPT_CHAR_ACTION, 2, [
        f!(Int32), f!(Int32), f!(Int8), f!(Packet), f!(Int32), f!(Int8), f!(Int8), f!(UInt32), f!(UInt8),
    ]);

    // AgpmOptimizedPacket2 (0x42) — drapeaux 2
    schema!(pub OPT_VIEW = "AGPMOPTIMIZEDVIEW", T::OPT_VIEW, 2, [
        f!(Int8),           // 0 opération
        f!(Packet),         // 1 vue d'un personnage
        f!(Packet, 32),     // 2 liste de vues
        f!(MemoryBlock),    // 3
        f!(MemoryBlock),    // 4 fonctions PNJ (ApmEventManager)
        f!(Char, 33),       // 5
        f!(MemoryBlock), f!(MemoryBlock), f!(MemoryBlock),
        f!(Int32), f!(Int32), f!(Int32), f!(Int32),
        f!(MemoryBlock),
    ]);

    // AgpmStartupEncryption — drapeaux 1
    schema!(pub STARTUP_ENCRYPTION = "AGPMSTARTUPENCRYPTION", T::STARTUP_ENCRYPTION, 1, [f!(Int8), f!(MemoryBlock)]);

    /// Schéma principal d'un type de paquet, s'il est connu.
    pub fn by_id(id: u8) -> Option<&'static Schema> {
        Some(match id {
            T::CONFIG => CONFIG,
            T::ITEM => ITEM,
            T::CHARACTER => CHARACTER,
            T::CHAR_MANAGER => CHAR_MANAGER,
            T::LOGIN => LOGIN,
            T::TIMER => TIMER,
            T::WORLD => WORLD,
            T::CHATTING => CHATTING,
            T::SYSTEM_MESSAGE => SYSTEM_MESSAGE,
            T::RETURN_TO_LOGIN => RETURN_TO_LOGIN,
            T::OPT_CHAR_MOVE => OPT_CHAR_MOVE,
            T::OPT_CHAR_ACTION => OPT_CHAR_ACTION,
            T::OPT_VIEW => OPT_VIEW,
            T::STARTUP_ENCRYPTION => STARTUP_ENCRYPTION,
            _ => return None,
        })
    }
}

/// Schémas des mini-paquets (identifiants internes, jamais transmis).
pub mod mini {
    use super::*;

    schema!(pub LOGIN_SERVER_ADDR = "AGPMLOGIN_SERVER_ADDR", 100, 1, [f!(Char, 23), f!(Char, 23), f!(Char, 23)]);

    schema!(pub LOGIN_CHAR_INFO = "AGPMLOGIN_CHAR_INFO", 101, 2, [
        f!(Int32),          // 0 TID
        f!(Char, 49),       // 1 nom
        f!(Int32),          // 2 nombre de personnages
        f!(Int32),          // 3 index de slot
        f!(Int32),          // 4 union / race courante
        f!(Int32),          // 5 race
        f!(Int32),          // 6 cheveux
        f!(Int32),          // 7 visage
        f!(Char, 49),       // 8
        f!(Int32),          // 9 (nouveau dans le client x64)
    ]);

    schema!(pub LOGIN_SERVER_INFO = "AGPMLOGIN_SERVER_INFO", 102, 1, [f!(Char, 23)]);
    schema!(pub LOGIN_VERSION_INFO = "AGPMLOGIN_VERSION_INFO", 103, 1, [f!(Int32), f!(Int32)]);
    schema!(pub LOGIN_COMPEN_INFO = "AGPMLOGIN_COMPEN_INFO", 104, 1, [f!(Int32), f!(Int32), f!(Char, 49), f!(Char, 100), f!(MemoryBlock)]);

    // Déplacement (constructeur AgpmCharacter) — drapeaux 1
    schema!(pub CHAR_MOVE = "AGPMCHAR_MOVE", 105, 1, [
        f!(Vec3F),          // 0 position
        f!(Vec3F),          // 1 destination
        f!(Int32),          // 2 cible suivie
        f!(Int32),          // 3 distance de suivi
        f!(Float),          // 4 axe X
        f!(Float),          // 5 axe Y
        f!(Int8),           // 6 drapeaux
        f!(Int8),           // 7 direction
    ]);

    schema!(pub CHAR_ACTION = "AGPMCHAR_ACTION", 106, 2, [
        f!(Int8), f!(Int32), f!(Int32), f!(Int8), f!(Packet), f!(Vec3F), f!(UInt8), f!(Int8), f!(UInt32), f!(UInt8),
    ]);

    schema!(pub CHAR_FACTORS = "AGPMCHAR_FACTORS", 107, 2, [
        f!(Packet), f!(Packet), f!(Packet), f!(Packet), f!(Packet), f!(Packet), f!(Packet),
        f!(Packet), f!(Packet), f!(Packet), f!(Packet), f!(Packet), f!(Packet),
    ]);
    schema!(pub FACTOR_STATUS = "AGPMCHAR_FACTOR_STATUS", 108, 2, [
        f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32),
    ]);
    schema!(pub FACTOR_TYPE = "AGPMCHAR_FACTOR_TYPE", 109, 1, [f!(Int32), f!(Int32), f!(Int32)]);
    schema!(pub FACTOR_POINT = "AGPMCHAR_FACTOR_POINT", 110, 4, [
        f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32),
        f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32),
    ]);
    schema!(pub FACTOR_POINTMAX = "AGPMCHAR_FACTOR_POINTMAX", 111, 2, [
        f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32),
    ]);
    schema!(pub FACTOR_RECOVERY = "AGPMCHAR_FACTOR_RECOVERY", 112, 1, [f!(Int32), f!(Int32), f!(Int32)]);
    schema!(pub FACTOR_ATTRIBUTE = "AGPMCHAR_FACTOR_ATTRIBUTE", 113, 2, [
        f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32),
    ]);
    schema!(pub FACTOR_DMG = "AGPMCHAR_FACTOR_DMG", 114, 1, [f!(Packet), f!(Packet)]);
    schema!(pub FACTOR_DEFENSE = "AGPMCHAR_FACTOR_DEFENSE", 115, 1, [f!(Packet), f!(Packet)]);
    schema!(pub FACTOR_ATTACK = "AGPMCHAR_FACTOR_ATTACK", 116, 1, [f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32)]);
    schema!(pub FACTOR_ITEM = "AGPMCHAR_FACTOR_ITEM", 117, 1, [f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32)]);
    schema!(pub FACTOR_DIRT = "AGPMCHAR_FACTOR_DIRT", 118, 1, [f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32)]);
    schema!(pub FACTOR_PRICE = "AGPMCHAR_FACTOR_PRICE", 119, 1, [f!(Int32), f!(Int32), f!(Int32), f!(Int32)]);

    // AgpmOptimizedPacket2 : vue d'un personnage — drapeaux 4
    schema!(pub OPT_VIEW_CHAR = "AGPMOPTIMIZEDPACKET_VIEW", 120, 4, [
        f!(Int32),          // 0 CID
        f!(Int32),          // 1 TID
        f!(MemoryBlock),    // 2 nom
        f!(Int8), f!(Int8),
        f!(Packet),         // 5 facteurs
        f!(Int8), f!(Int8),
        f!(Vec3F),          // 8 position
        f!(Vec3F),          // 9 destination
        f!(Int32), f!(UInt16), f!(Int16), f!(Int16), f!(UInt64), f!(Int8), f!(Int8), f!(Int8),
        f!(Int8), f!(UInt16), f!(Int8), f!(Int32), f!(Char, 16), f!(Int32), f!(Int32), f!(Int32),
    ]);

    // AgpmOptimizedPacket2 : vue d'un objet — drapeaux 4
    schema!(pub OPT_VIEW_ITEM = "AGPMOPTIMIZEDPACKET_ITEMVIEW", 121, 4, [
        f!(Int32), f!(Int32), f!(Int16), f!(Int8), f!(Int8), f!(Int8),
        f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32), f!(Int32),
        f!(UInt16), f!(UInt16), f!(UInt16), f!(UInt16), f!(UInt16), f!(MemoryBlock),
    ]);

    // Position dans l'inventaire / banque : trois INT16 (ap_item.c).
    schema!(pub ITEM_GRID = "AGPMITEM_INVENTORY", 123, 1, [f!(Int16), f!(Int16), f!(Int16)]);
}

/// Opérations (champ 0) du paquet de connexion (AgpmLogin).
pub mod login_op {
    pub const HASHKEY: i8 = 0;
    pub const USER_LOGIN: i8 = 1;
    pub const UNION_INFO: i8 = 3;
    pub const CHAR_NAME: i8 = 4;
    pub const CHAR_NAME_FINISH: i8 = 5;
    pub const CHAR_LIST: i8 = 6;
    pub const CHAR_LIST_FINISH: i8 = 7;
    pub const WORLD_ENTER: i8 = 8;
    pub const CHAR_CREATION_ENTER: i8 = 10;
    pub const CREATE_CHAR: i8 = 11;
    pub const CREATE_CHAR_FINISH: i8 = 12;
    pub const DELETE_CHAR: i8 = 13;
    pub const LOGIN_RESULT: i8 = 15;
}

pub mod login_result {
    pub const INCORRECT_USER: i32 = 0;
    pub const GIBBERISH: i32 = 1;
    pub const TOO_MANY_ATTEMPTS: i32 = 2;
    pub const TOS_AGREEMENT: i32 = 3;
    pub const NAME_TAKEN: i32 = 7;
}

/// Opérations de la poignée de main (AgpmStartupEncryption).
pub mod crypt_op {
    pub const REQUEST: i8 = 0;
    pub const PUBLIC_KEY: i8 = 1;
    pub const PRIVATE_KEY: i8 = 2;
    pub const COMPLETE: i8 = 3;
    pub const DYNCODE_PUB: i8 = 4;
    pub const DYNCODE_PRV: i8 = 5;
    pub const SET_ALGO: i8 = 6;
}

pub mod world_op {
    pub const SERVER_LIST_REQ: i8 = 0;
    pub const SERVER_LIST_ANS: i8 = 2;
    pub const SERVER_EXTRA: i8 = 3;
}

/// Opérations du gestionnaire de personnages (type 0x03).
pub mod char_mgr_op {
    /// client → monde : champ 2 = 17, nom (3), clé d'auth (6), CHAR[248] (7).
    pub const GAME_ENTER: i8 = 2;
    pub const FINISH_ENTER: i8 = 7;
    pub const LOADING_POS: i8 = 8;
    /// client → monde après LOADING_POS : CID au champ 4.
    pub const ENTER_WORLD: i8 = 9;
    /// login → client : clé pour le serveur monde.
    pub const AUTH_KEY: i8 = 11;
    /// client → monde toutes les 15 s, champ 2.
    pub const KEEPALIVE: i8 = 13;
}

/// Opérations de la vue optimisée (type 0x42).
pub mod view_op {
    pub const ADD_CHAR: i8 = 1;
    pub const ADD_CHARACTER_ITEMS: i8 = 2;
}

/// Opérations `AGPMCHARACTER` (type 0x02), relevées dans `ap_character.h`.
pub mod character_op {
    pub const ADD: i8 = 0;
    pub const UPDATE: i8 = 1;
    pub const REMOVE: i8 = 2;
    pub const REMOVE_FOR_VIEW: i8 = 3;
}
