//! Serveur monde (port 11008) : entrée en jeu, PNJ, keepalive, événements pour le moteur.
//!
//! Séquence relevée dans le client (fonctions d'`AgcmCharacter`) et validée avec ArchCore :
//!
//! ```text
//! CHAR_MANAGER op 2  (champ 2 = 17, "Nom#Monde", clé d'auth, CHAR[248])   client → monde
//! CHAR_MANAGER op 13 (keepalive, champ 2 = 0), puis toutes les 15 s
//! CONFIG, CHARACTER op 25 (le joueur), CHAR_MANAGER op 8 (position, champ 5), TIMER
//! CHAR_MANAGER op 9  (CID champ 4)                                         client → monde
//! CHARACTER op 0, OPT_VIEW op 1 par PNJ/personnage en vue (mini vue champ 1)
//! CHAR_MANAGER op 7  : le joueur est en jeu
//! ```

use crate::connection::{Connection, Incoming, NetError};
use crate::ext::{self, ExtMessage, Extensions};
use crate::login::WorldTicket;
use archrust_protocol::packet::{build_mini, decode_mini, sparse, Value};
use archrust_protocol::schema::{char_mgr_op, character_op, main, mini, packet_type, view_op};
use glam::Vec3;
use std::time::Duration;
use tokio::sync::mpsc;

pub const KEEPALIVE: Duration = Duration::from_secs(15);

/// Entité vue par le client (PNJ, monstre, joueur).
#[derive(Debug, Clone, PartialEq)]
pub struct SeenCharacter {
    pub cid: i32,
    pub tid: i32,
    pub name: String,
    pub position: Vec3,
    pub destination: Vec3,
    /// Orientation autour de Y en degrés (champ 13 de la vue, INT16), `None` si absente. Archonia
    /// (`FUN_140260d40`) passe les champs 12/13 comme angles X/Y du mini-paquet `CHAR_MOVE`.
    pub turn_y: Option<f32>,
    pub equipment: Vec<EquipmentView>,
    /// Statut d'action (champ 3 : 2 = mort, cadavre en vue).
    pub action_status: Option<i8>,
    /// PV, maximum et niveau lus dans les facteurs de la vue (champ 5).
    pub stats: Option<CharacterStats>,
    /// Fonctions d'un PNJ (champ 4 de la vue) : dialogue, boutique, banque, quêtes.
    pub npc_functions: Vec<crate::npc::NpcFunction>,
}

/// Objet équipé fourni dans la liste `OPT_VIEW` d'un personnage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EquipmentView {
    pub item_id: i32,
    pub template_id: i32,
    pub count: i16,
}

/// Champs d'un paquet AgpmItem utiles à l'état client. Le statut suit l'enum du client
/// (0 non placé, 1 terrain, 2 inventaire, 3 équipé, 100 banque).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemRecord {
    pub operation: u8,
    pub id: i32,
    pub template_id: Option<i32>,
    pub owner_cid: Option<i32>,
    pub count: Option<i32>,
    pub status: Option<i8>,
    pub grid: Option<[i16; 3]>,
}

/// Champs utiles du mini-paquet `AGPMCHAR_FACTOR_POINT` joint à une action.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionFactorPoints {
    pub hp: Option<i32>,
    pub mp: Option<i32>,
    pub normal_damage: Option<i32>,
    pub magic_damage: Option<i32>,
    pub water_damage: Option<i32>,
    pub fire_damage: Option<i32>,
    pub earth_damage: Option<i32>,
    pub air_damage: Option<i32>,
    pub poison_damage: Option<i32>,
    pub lightning_damage: Option<i32>,
    pub ice_damage: Option<i32>,
    pub heroic_damage: Option<i32>,
}

/// Caractéristiques d'un personnage lues dans ses facteurs complets (`AGPMCHAR_FACTORS`) :
/// niveau (`FACTOR_STATUS` 7), PV, PM et expérience (`FACTOR_POINT` 0, 1, 3 + 4 << 32),
/// maxima (`FACTOR_POINTMAX` 0, 1). Indices relevés par ArchCore sur le client x64.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CharacterStats {
    pub level: Option<i32>,
    pub hp: Option<i32>,
    pub mp: Option<i32>,
    pub max_hp: Option<i32>,
    pub max_mp: Option<i32>,
    pub exp: Option<u64>,
    /// Argent de l'inventaire (paquet CHARACTER, champ 8 ; hors facteurs).
    pub money: Option<i64>,
    /// Argent en banque (champ 9).
    pub bank_money: Option<i64>,
    /// Pages de banque achetées en plus de la première (champ 23).
    pub bank_pages: Option<i8>,
    /// Points de compétence (facteurs, sous-paquet « dirt » indice 5).
    pub skill_points: Option<i32>,
}

/// Effet actif d'une compétence sur un personnage (AGPMSKILL op 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuffInfo {
    /// CID du personnage qui porte l'effet.
    pub owner: i32,
    /// Identifiant de la compétence (objet côté serveur).
    pub skill_id: i32,
    /// TID de la compétence (SKILLTEMPLATE), qui donne l'icône.
    pub skill_tid: i32,
    /// Durée restante annoncée, en millisecondes (0 = inconnue ou permanente).
    pub duration_ms: u64,
}

/// Événements remontés au moteur.
#[derive(Debug, Clone)]
pub enum WorldEvent {
    Connected,
    /// Paquet CHARACTER complet (op 25 ou 0) : CID, TID, nom, position (mini déplacement du
    /// champ 5), visage et cheveux (champs 20/21). Le joueur reçoit le sien (op 25 puis 0) ; un
    /// op 0 d'un autre CID est un autre joueur qui entre dans la vue (le moteur en fait une entité).
    SelfCharacter {
        cid: i32,
        tid: i32,
        name: String,
        position: Option<Vec3>,
        face: Option<i32>,
        hair: Option<i32>,
        stats: Option<CharacterStats>,
    },
    CharacterUpdated {
        cid: i32,
        position: Option<Vec3>,
        destination: Option<Vec3>,
        factors: Option<ActionFactorPoints>,
        stats: Option<CharacterStats>,
        /// Statut d'action (champ 11) : 2 = mort, 0 = vivant (résurrection).
        action_status: Option<i8>,
    },
    /// Effet de compétence ajouté (AGPMSKILL op 4).
    BuffAdded(BuffInfo),
    /// Effet de compétence retiré (AGPMSKILL op 5) : porteur et TID de l'effet.
    BuffRemoved { owner: i32, skill_tid: i32 },
    CharacterRemoved {
        cid: i32,
        for_view: bool,
    },
    ItemChanged(ItemRecord),
    /// Inventaire/équipement complet transmis dans OPT_VIEW op 2 à l'entrée en jeu.
    ItemsSnapshot(Vec<ItemRecord>),
    LoadingPosition(Vec3),
    GameTime(u64),
    CharacterSeen(SeenCharacter),
    CharacterMoved {
        cid: i32,
        position: Vec3,
        destination: Vec3,
    },
    /// Action reçue sur le canal optimisé 0x41 (attaque, résultat et données de cible).
    CharacterAction {
        cid: i32,
        target_cid: i32,
        result: Option<i8>,
        factor_packet: Option<Vec<u8>>,
        factors: Option<ActionFactorPoints>,
        target_hp: Option<i32>,
        combo: Option<bool>,
        force_attack: Option<bool>,
        additional_effect: Option<u32>,
        hit_index: Option<u8>,
        /// Dégâts du coup (sous-paquet « résultat » des facteurs), en positif.
        damage: Option<i32>,
    },
    /// Objet posé au sol (AGPMITEM op 0, statut 1), voir `combat`.
    GroundItem(crate::combat::GroundItem),
    /// Résultat d'un ramassage (AGPMITEM op 12).
    PickupResult {
        iid: i32,
        success: bool,
        tid: Option<i32>,
        count: Option<i32>,
    },
    /// Montée de niveau d'un personnage (AGPMCHARACTER op 5).
    LevelUp { cid: i32 },
    /// Objet utilisé (AGPMITEM op 4) : modèle, utilisateur, délai de réutilisation en ms.
    ItemUsed { tid: Option<i32>, user: Option<i32>, cooldown_ms: Option<i32> },
    /// Retour d'un parchemin aller-retour possible (AGPMITEM op 9) ou fini (op 10).
    ReturnPoint { enabled: bool },
    Chat {
        from_cid: i32,
        channel: i8,
        from: String,
        message: String,
    },
    SystemMessage(String),
    /// Message système sans texte (0x44 op 0) : code au champ 1 (29 banque au maximum, 30 page ajoutée…).
    SystemCode(i32),
    /// Entrée en jeu confirmée (op 7).
    EnteredWorld,
    /// Paquet non interprété (type, opération).
    Unhandled {
        packet_type: u8,
        op: Option<i64>,
    },
    /// Extensions ArchRust négociées sur la connexion au monde (`HELLO_ACK`).
    ExtensionsReady(Extensions),
    /// Message d'AddOn relayé par le serveur (`ADDON_RECV`) ; l'émetteur est renseigné par lui.
    AddonMessage {
        prefix: String,
        /// Portée (`ext::channel`) : 1 murmure, 2 dire…
        channel: u8,
        target: Option<String>,
        sender_cid: i32,
        sender: String,
        payload: Vec<u8>,
    },
    /// Refus d'un envoi ou d'une demande (`RESULT`) : numéro de la demande et code `ext::result`.
    AddonMessageFailed { request_id: u32, result: u8 },
    /// Langue appliquée par le serveur après un `SET_LANGUAGE` (`LANGUAGE_ACK`), et langues proposées.
    LanguageChanged {
        request_id: u32,
        language: String,
        languages: Vec<String>,
    },
    /// Textes localisés rendus par le serveur (`TEXT_REPLY`), dans l'ordre demandé.
    Texts {
        request_id: u32,
        language: String,
        kind: String,
        texts: Vec<(u32, String)>,
    },
    /// Paquet de groupe (0x08) : groupe, membres, réglages (voir `party`).
    Party(crate::party::PartyPacket),
    /// Paquet de gestion de groupe (0x09) : invitation, réponse, exclusion, chef, échecs.
    PartyManagement(crate::party::PartyManagement),
    /// Dialogue, boutique, banque, quêtes (voir `npc`).
    Npc(crate::npc::NpcEvent),
    /// Compétences : liste, niveau, lancement, instructeur (voir `skills`).
    Skill(crate::skills::SkillEvent),
    /// Portails : accord, départ, chargement fini (voir `teleport`).
    Teleport(crate::teleport::TeleportEvent),
    Disconnected(String),
}

/// Commandes du moteur vers le serveur.
#[derive(Debug, Clone)]
pub enum Command {
    MoveTo {
        position: Vec3,
        destination: Vec3,
    },
    /// Arrêt à la position donnée (drapeau 0x04).
    Stop {
        position: Vec3,
    },
    Say(String),
    /// Message d'AddOn (`ADDON_SEND`) ; ignoré si le serveur n'a pas accordé `addon-msg`.
    AddonSend {
        request_id: u32,
        prefix: String,
        channel: u8,
        target: Option<String>,
        payload: Vec<u8>,
    },
    /// Changement de langue (`SET_LANGUAGE`) ; ignoré si le serveur n'a pas accordé `language`.
    SetLanguage { request_id: u32, language: String },
    /// Textes localisés (`TEXT_REQUEST`) : sorte (`ext::text_kind`) et identifiants (64 au plus).
    RequestTexts {
        request_id: u32,
        kind: String,
        ids: Vec<u32>,
    },
    /// Demande de groupe (paquets 0x08 / 0x09).
    Party(crate::party::PartyCommand),
    /// Attaque, ramassage, résurrection (voir `combat`).
    Combat(crate::combat::CombatCommand),
    /// Objets de l'inventaire et équipement (voir `items`).
    Item(crate::items::ItemCommand),
    /// Dialogue, boutique, banque, quêtes (voir `npc`).
    Npc(crate::npc::NpcCommand),
    /// Lancement et apprentissage des compétences (voir `skills`).
    Skill(crate::skills::SkillCommand),
    /// Demande au portail, destination choisie (voir `teleport`).
    Teleport(crate::teleport::TeleportCommand),
    Quit,
}

fn vec3(v: Option<&Value>) -> Option<Vec3> {
    v.and_then(Value::as_vec3).map(Vec3::from_array)
}
fn text(v: Option<&Value>) -> String {
    v.and_then(Value::as_text).unwrap_or_default()
}

fn decode_action_factors(bytes: &[u8]) -> Option<ActionFactorPoints> {
    let (_, values) = decode_mini(mini::FACTOR_POINT, bytes).ok()?;
    let field = |index: usize| {
        values
            .get(index)
            .and_then(Option::as_ref)
            .and_then(Value::as_i64)
            .map(|value| value as i32)
    };
    Some(ActionFactorPoints {
        hp: field(0),
        mp: field(1),
        normal_damage: field(9),
        magic_damage: field(10),
        water_damage: field(11),
        fire_damage: field(12),
        earth_damage: field(13),
        air_damage: field(14),
        poison_damage: field(15),
        lightning_damage: field(16),
        ice_damage: field(17),
        heroic_damage: field(19),
    })
}

/// Niveau, points et maxima d'un personnage dans ses facteurs complets.
pub fn decode_character_stats(bytes: &[u8]) -> Option<CharacterStats> {
    let (_, values) = decode_mini(mini::CHAR_FACTORS, bytes).ok()?;
    let sub = |index: usize, schema: &'static archrust_protocol::schema::Schema| {
        values
            .get(index)
            .and_then(Option::as_ref)
            .and_then(Value::as_bytes)
            .and_then(|b| decode_mini(schema, b).ok())
            .map(|(_, v)| v)
    };
    let int = |v: &Option<Vec<Option<Value>>>, i: usize| {
        v.as_ref()
            .and_then(|v| v.get(i))
            .and_then(Option::as_ref)
            .and_then(Value::as_i64)
    };
    let status = sub(1, mini::FACTOR_STATUS);
    let point = sub(3, mini::FACTOR_POINT);
    let max = sub(4, mini::FACTOR_POINTMAX);
    if status.is_none() && point.is_none() && max.is_none() && values.get(11).and_then(Option::as_ref).is_none() {
        return None;
    }
    let exp = match (int(&point, 3), int(&point, 4)) {
        (None, None) => None,
        (lo, hi) => Some((lo.unwrap_or(0) as u32 as u64) | ((hi.unwrap_or(0) as u32 as u64) << 32)),
    };
    Some(CharacterStats {
        level: int(&status, 7).map(|v| v as i32),
        hp: int(&point, 0).map(|v| v as i32),
        mp: int(&point, 1).map(|v| v as i32),
        max_hp: int(&max, 0).map(|v| v as i32),
        max_mp: int(&max, 1).map(|v| v as i32),
        exp,
        skill_points: crate::skills::factors_dirt(bytes, crate::skills::DIRT_SKILL_POINT),
        ..Default::default()
    })
}

/// Caractéristiques d'un paquet CHARACTER : facteurs (champ 7) et argent (champ 8).
fn character_stats(p: &Incoming) -> Option<CharacterStats> {
    let mut stats = p.get(7).and_then(Value::as_bytes).and_then(decode_character_stats);
    if let Some(money) = p.get(8).and_then(Value::as_i64) {
        stats.get_or_insert_with(CharacterStats::default).money = Some(money);
    }
    if let Some(bank) = p.get(9).and_then(Value::as_i64) {
        stats.get_or_insert_with(CharacterStats::default).bank_money = Some(bank);
    }
    if let Some(pages) = p.get(23).and_then(Value::as_i64) {
        stats.get_or_insert_with(CharacterStats::default).bank_pages = Some(pages as i8);
    }
    stats
}

/// Paquet AGPMSKILL d'effet (op 4 ajout, 5 retrait) en événement, sinon `None`.
fn decode_skill_buff(p: &Incoming) -> Option<WorldEvent> {
    let op = p.op()?;
    if op != 4 && op != 5 {
        return None;
    }
    let skill_id = p.get(1).and_then(Value::as_i64).unwrap_or(0) as i32;
    let owner = p
        .get(2)
        .and_then(Value::as_bytes)
        .and_then(|b| decode_mini(mini::SKILL_BASE, b).ok())
        .and_then(|(_, v)| v.get(1).and_then(Option::as_ref).and_then(Value::as_i64))? as i32;
    let buffs: Vec<Vec<u8>> = match p.get(8) {
        Some(Value::PacketList(l)) => l.clone(),
        Some(Value::Packet(b)) => vec![b.clone()],
        _ => Vec::new(),
    };
    let first = buffs.first().and_then(|b| decode_mini(mini::SKILL_BUFF, b).ok()).map(|(_, v)| v);
    let field = |i: usize| first.as_ref().and_then(|v| v.get(i)).and_then(Option::as_ref);
    let skill_tid = field(0).and_then(Value::as_i64).unwrap_or(0) as i32;
    if op == 5 {
        return Some(WorldEvent::BuffRemoved { owner, skill_tid });
    }
    let duration_ms = match field(1) {
        Some(Value::U64(v)) => *v,
        Some(v) => v.as_i64().unwrap_or(0).max(0) as u64,
        None => 0,
    };
    Some(WorldEvent::BuffAdded(BuffInfo { owner, skill_id, skill_tid, duration_ms }))
}

fn decode_character_factors(bytes: &[u8]) -> Option<ActionFactorPoints> {
    let (_, values) = decode_mini(mini::CHAR_FACTORS, bytes).ok()?;
    let point_packet = values
        .get(3)
        .and_then(Option::as_ref)
        .and_then(Value::as_bytes)?;
    decode_action_factors(point_packet)
}

/// Décode une vue de personnage (mini OPT_VIEW_CHAR).
pub fn decode_view(mini_bytes: &[u8]) -> Option<SeenCharacter> {
    let (_, v) = decode_mini(mini::OPT_VIEW_CHAR, mini_bytes).ok()?;
    let g = |i: usize| v.get(i).and_then(|x| x.as_ref());
    Some(SeenCharacter {
        cid: g(0)?.as_i64()? as i32,
        tid: g(1).and_then(Value::as_i64).unwrap_or(0) as i32,
        name: text(g(2)),
        position: vec3(g(8)).unwrap_or(Vec3::ZERO),
        destination: vec3(g(9)).unwrap_or(Vec3::ZERO),
        turn_y: g(13).and_then(Value::as_i64).map(|v| v as f32),
        equipment: Vec::new(),
        action_status: g(3).and_then(Value::as_i64).map(|v| v as i8),
        stats: g(5).and_then(Value::as_bytes).and_then(decode_character_stats),
        npc_functions: Vec::new(),
    })
}

/// Décode la vue du personnage et les mini-paquets `OPT_VIEW_ITEM` du champ 2.
fn decode_view_with_equipment(mini_bytes: &[u8], items: Option<&Value>) -> Option<SeenCharacter> {
    let mut character = decode_view(mini_bytes)?;
    if let Some(Value::PacketList(packets)) = items {
        character.equipment = packets
            .iter()
            .filter_map(|bytes| {
                let (_, values) = decode_mini(&mini::OPT_VIEW_ITEM, bytes).ok()?;
                Some(EquipmentView {
                    item_id: values.get(0)?.as_ref()?.as_i64()? as i32,
                    template_id: values.get(1)?.as_ref()?.as_i64()? as i32,
                    count: values.get(2)?.as_ref()?.as_i64()? as i16,
                })
            })
            .collect();
    }
    Some(character)
}

/// Événements en plus de celui de `translate` : compétences connues, au champ 8 de la vue optimisée
/// op 2 (l'inventaire du champ 6 donne l'événement principal).
pub fn extra_events(p: &Incoming) -> Vec<WorldEvent> {
    if p.packet_type == packet_type::OPT_VIEW && p.op() == Some(view_op::ADD_CHARACTER_ITEMS as i64) {
        if let Some(b) = p.get(8).and_then(Value::as_bytes) {
            return crate::skills::decode_skill_block(b);
        }
    }
    Vec::new()
}

/// Traduit un paquet reçu en événement.
pub fn translate(p: &Incoming) -> WorldEvent {
    // Dialogue, boutique, banque, quêtes (voir `npc`).
    if let Some(ev) = p.packet.as_ref().and_then(|packet| crate::npc::translate(p.packet_type, &packet.values)) {
        return WorldEvent::Npc(ev);
    }
    // Portails (voir `teleport`).
    if let Some(ev) = p.packet.as_ref().and_then(|packet| crate::teleport::translate(p.packet_type, &packet.values)) {
        return WorldEvent::Teleport(ev);
    }
    // Compétences : liste, niveau, lancement, résultat, instructeur (voir `skills`).
    if let Some(ev) = p.packet.as_ref().and_then(|packet| crate::skills::translate(p.packet_type, &packet.values)) {
        return ev;
    }
    match p.packet_type {
        packet_type::ITEM
            if p.op() == Some(0)
                && p.get(1).and_then(Value::as_i64) == Some(archrust_protocol::schema::ITEM_STATUS_FIELD as i64) =>
        {
            match p.packet.as_ref().and_then(|packet| crate::combat::decode_ground_item(&packet.values)) {
                Some(item) => WorldEvent::GroundItem(item),
                None => WorldEvent::Unhandled { packet_type: p.packet_type, op: p.op() },
            }
        }
        packet_type::ITEM if p.op() == Some(archrust_protocol::schema::item_op::PICKUP_RESULT as i64) => WorldEvent::PickupResult {
            iid: p.get(2).and_then(Value::as_i64).unwrap_or(0) as i32,
            success: p.get(1).and_then(Value::as_i64) == Some(0),
            tid: p.get(3).and_then(Value::as_i64).map(|v| v as i32),
            count: p.get(5).and_then(Value::as_i64).map(|v| v as i32),
        },
        packet_type::ITEM if matches!(p.op(), Some(0..=2)) => {
            let operation = p.op().unwrap_or_default() as u8;
            match p
                .packet
                .as_ref()
                .and_then(|packet| item_record(&packet.values, operation))
            {
                Some(item) => WorldEvent::ItemChanged(item),
                None => WorldEvent::Unhandled {
                    packet_type: p.packet_type,
                    op: p.op(),
                },
            }
        }
        packet_type::ITEM if p.op() == Some(crate::items::op::USE_RESULT as i64) => WorldEvent::ItemUsed {
            tid: p.get(3).and_then(Value::as_i64).map(|v| v as i32),
            user: p.get(4).or(p.get(12)).and_then(Value::as_i64).map(|v| v as i32),
            cooldown_ms: p.get(18).and_then(Value::as_i64).map(|v| v as i32),
        },
        packet_type::ITEM if matches!(p.op(), Some(9 | 10)) => WorldEvent::ReturnPoint {
            enabled: p.op() == Some(crate::items::op::RETURN_ENABLED as i64),
        },
        packet_type::ITEM => WorldEvent::Unhandled {
            packet_type: p.packet_type,
            op: p.op(),
        },
        packet_type::OPT_VIEW if p.op() == Some(view_op::ADD_CHARACTER_ITEMS as i64) => {
            match p
                .get(6)
                .and_then(Value::as_bytes)
                .and_then(decode_item_batch)
            {
                Some(items) => WorldEvent::ItemsSnapshot(items),
                None if p.get(6).is_none() => WorldEvent::ItemsSnapshot(Vec::new()),
                None => WorldEvent::Unhandled {
                    packet_type: p.packet_type,
                    op: p.op(),
                },
            }
        }
        packet_type::SKILL => decode_skill_buff(p).unwrap_or(WorldEvent::Unhandled {
            packet_type: p.packet_type,
            op: p.op(),
        }),
        packet_type::CHARACTER
            if p.op() == Some(character_op::ADD as i64) || p.op() == Some(25) =>
        {
            WorldEvent::SelfCharacter {
                cid: p.get(1).and_then(Value::as_i64).unwrap_or(0) as i32,
                tid: p.get(2).and_then(Value::as_i64).unwrap_or(0) as i32,
                name: text(p.get(3)),
                position: p
                    .get(5)
                    .and_then(Value::as_bytes)
                    .and_then(|bytes| decode_mini(mini::CHAR_MOVE, bytes).ok())
                    .and_then(|(_, values)| vec3(values.first().and_then(Option::as_ref))),
                face: p.get(20).and_then(Value::as_i64).map(|v| v as i32),
                hair: p.get(21).and_then(Value::as_i64).map(|v| v as i32),
                stats: character_stats(p),
            }
        }
        packet_type::CHARACTER if p.op() == Some(character_op::UPDATE as i64) => {
            let move_fields = p
                .get(5)
                .and_then(Value::as_bytes)
                .and_then(|bytes| decode_mini(mini::CHAR_MOVE, bytes).ok())
                .map(|(_, values)| values);
            let field = |index: usize| {
                move_fields
                    .as_ref()
                    .and_then(|values| values.get(index))
                    .and_then(Option::as_ref)
            };
            WorldEvent::CharacterUpdated {
                cid: p.get(1).and_then(Value::as_i64).unwrap_or(0) as i32,
                position: vec3(field(0)),
                destination: vec3(field(1)),
                factors: p
                    .get(7)
                    .and_then(Value::as_bytes)
                    .and_then(decode_character_factors),
                stats: character_stats(p),
                action_status: p.get(11).and_then(Value::as_i64).map(|v| v as i8),
            }
        }
        packet_type::CHARACTER if p.op() == Some(character_op::LEVEL_UP as i64) => WorldEvent::LevelUp {
            cid: p.get(1).and_then(Value::as_i64).unwrap_or(0) as i32,
        },
        packet_type::CHARACTER
            if p.op() == Some(character_op::REMOVE as i64)
                || p.op() == Some(character_op::REMOVE_FOR_VIEW as i64) =>
        {
            WorldEvent::CharacterRemoved {
                cid: p.get(1).and_then(Value::as_i64).unwrap_or(0) as i32,
                for_view: p.op() == Some(character_op::REMOVE_FOR_VIEW as i64),
            }
        }
        packet_type::CHAR_MANAGER if p.op() == Some(char_mgr_op::LOADING_POS as i64) => {
            WorldEvent::LoadingPosition(vec3(p.get(5)).unwrap_or(Vec3::ZERO))
        }
        packet_type::CHAR_MANAGER if p.op() == Some(char_mgr_op::FINISH_ENTER as i64) => {
            WorldEvent::EnteredWorld
        }
        packet_type::TIMER => {
            WorldEvent::GameTime(p.get(1).and_then(Value::as_i64).unwrap_or(0) as u64)
        }
        packet_type::OPT_VIEW if p.op() == Some(view_op::ADD_CHAR as i64) => {
            match p
                .get(1)
                .and_then(Value::as_bytes)
                .and_then(|bytes| decode_view_with_equipment(bytes, p.get(2)))
            {
                Some(mut c) => {
                    c.npc_functions = p.get(4).and_then(Value::as_bytes).map(crate::npc::decode_functions).unwrap_or_default();
                    WorldEvent::CharacterSeen(c)
                }
                None => WorldEvent::Unhandled {
                    packet_type: p.packet_type,
                    op: p.op(),
                },
            }
        }
        packet_type::OPT_CHAR_MOVE => WorldEvent::CharacterMoved {
            cid: p.get(0).and_then(Value::as_i64).unwrap_or(0) as i32,
            position: vec3(p.get(1)).unwrap_or(Vec3::ZERO),
            destination: vec3(p.get(2)).unwrap_or(Vec3::ZERO),
        },
        packet_type::OPT_CHAR_ACTION => {
            let factor_packet = match p.get(3) {
                Some(Value::Packet(bytes)) => Some(bytes.clone()),
                _ => None,
            };
            let hit = factor_packet.as_deref().and_then(crate::combat::decode_hit_factors);
            WorldEvent::CharacterAction {
                // Mapping verified against ap_optimized_packet2_on_receive_char_action in the
                // reconstructed Archonia source: CID, target, result, factor, target HP, combo,
                // force attack, additional effect, hit index.
                cid: p.get(0).and_then(Value::as_i64).unwrap_or(0) as i32,
                target_cid: p.get(1).and_then(Value::as_i64).unwrap_or(0) as i32,
                result: p.get(2).and_then(Value::as_i64).map(|v| v as i8),
                factors: hit.as_ref().map(|h| h.0.clone()),
                damage: hit.and_then(|h| h.1),
                factor_packet,
                target_hp: p.get(4).and_then(Value::as_i64).map(|v| v as i32),
                combo: p.get(5).and_then(Value::as_i64).map(|v| v != 0),
                force_attack: p.get(6).and_then(Value::as_i64).map(|v| v != 0),
                additional_effect: p.get(7).and_then(Value::as_i64).map(|v| v as u32),
                hit_index: p.get(8).and_then(Value::as_i64).map(|v| v as u8),
            }
        }
        packet_type::CHATTING => WorldEvent::Chat {
            from_cid: p.get(1).and_then(Value::as_i64).unwrap_or(0) as i32,
            channel: p.get(2).and_then(Value::as_i64).unwrap_or(0) as i8,
            from: text(p.get(4)),
            message: text(p.get(6)),
        },
        packet_type::SYSTEM_MESSAGE => match (text(p.get(4)), p.get(1).and_then(Value::as_i64)) {
            (t, Some(code)) if t.is_empty() => WorldEvent::SystemCode(code as i32),
            (t, _) => WorldEvent::SystemMessage(t),
        },
        packet_type::PARTY | packet_type::PARTY_MANAGEMENT => {
            let ev = if p.packet_type == packet_type::PARTY {
                crate::party::decode_party(p).map(WorldEvent::Party)
            } else {
                crate::party::decode_management(p).map(WorldEvent::PartyManagement)
            };
            ev.unwrap_or(WorldEvent::Unhandled {
                packet_type: p.packet_type,
                op: p.op(),
            })
        }
        ext::PACKET_TYPE => match ext::decode(&p.raw) {
            Some(ExtMessage::AddonRecv {
                prefix,
                channel,
                target,
                sender_cid,
                sender_name,
                payload,
            }) => WorldEvent::AddonMessage {
                prefix,
                channel,
                target,
                sender_cid,
                sender: sender_name,
                payload,
            },
            Some(ExtMessage::Result { request_id, result }) => {
                WorldEvent::AddonMessageFailed { request_id, result }
            }
            Some(ExtMessage::LanguageAck {
                request_id,
                language,
                languages,
            }) => WorldEvent::LanguageChanged {
                request_id,
                language,
                languages,
            },
            Some(ExtMessage::Texts {
                request_id,
                language,
                kind,
                texts,
            }) => WorldEvent::Texts {
                request_id,
                language,
                kind,
                texts,
            },
            other => {
                log::debug!("extension ignorée : {other:?}");
                WorldEvent::Unhandled {
                    packet_type: p.packet_type,
                    op: None,
                }
            }
        },
        _ => WorldEvent::Unhandled {
            packet_type: p.packet_type,
            op: p.op(),
        },
    }
}

/// Décode la concaténation d'entrées AgpmItem dans le bloc « All Item » d'OPT_VIEW.
///
/// Deux formes sont acceptées : des paquets AGPMITEM complets mis bout à bout (garde `0xD6`, longueur
/// totale u32 ; forme envoyée par ArchCore et décrite dans son wiki), ou des mini-paquets préfixés
/// par leur longueur u32 (forme relevée par Codex sur le client x64). Le premier octet les distingue.
fn decode_item_batch(bytes: &[u8]) -> Option<Vec<ItemRecord>> {
    if bytes.first() == Some(&archrust_protocol::packet::GUARD_HEAD) {
        return decode_item_packets(bytes);
    }
    decode_item_minis(bytes)
}

/// Paquets AGPMITEM complets bout à bout.
fn decode_item_packets(mut bytes: &[u8]) -> Option<Vec<ItemRecord>> {
    let mut items = Vec::new();
    while !bytes.is_empty() {
        let len = u32::from_le_bytes(bytes.get(1..5)?.try_into().ok()?) as usize;
        if len < archrust_protocol::packet::HEADER_SIZE || len > bytes.len() {
            return None;
        }
        let (one, rest) = bytes.split_at(len);
        let packet = archrust_protocol::packet::decode_packet(&main::ITEM, one).ok()?;
        let operation = packet.op()? as u8;
        items.push(item_record(&packet.values, operation)?);
        bytes = rest;
    }
    Some(items)
}

/// Mini-paquets AGPMITEM préfixés par leur longueur u32.
fn decode_item_minis(mut bytes: &[u8]) -> Option<Vec<ItemRecord>> {
    let mut items = Vec::new();
    while !bytes.is_empty() {
        let header: [u8; 4] = bytes.get(..4)?.try_into().ok()?;
        let len = u32::from_le_bytes(header) as usize;
        bytes = bytes.get(4..)?;
        if len == 0 {
            return None;
        }
        if len > bytes.len() {
            return None;
        }
        let (payload, rest) = bytes.split_at(len);
        let (_, values) = decode_mini(&main::ITEM, payload).ok()?;
        let operation = values.first()?.as_ref()?.as_i64()? as u8;
        items.push(item_record(&values, operation)?);
        bytes = rest;
    }
    Some(items)
}

fn item_record(values: &[Option<Value>], operation: u8) -> Option<ItemRecord> {
    let field = |index: usize| values.get(index).and_then(Option::as_ref);
    let status = field(1).and_then(Value::as_i64).map(|v| v as i8);
    let grid_field = match status {
        Some(2) => 7,   // Inventory
        Some(100) => 8, // Bank
        _ => usize::MAX,
    };
    let grid = field(grid_field)
        .and_then(Value::as_bytes)
        .and_then(|bytes| decode_mini(&mini::ITEM_GRID, bytes).ok())
        .and_then(|(_, values)| {
            Some([
                values.get(0)?.as_ref()?.as_i64()? as i16,
                values.get(1)?.as_ref()?.as_i64()? as i16,
                values.get(2)?.as_ref()?.as_i64()? as i16,
            ])
        });
    Some(ItemRecord {
        operation,
        id: field(2)?.as_i64()? as i32,
        template_id: field(3).and_then(Value::as_i64).map(|v| v as i32),
        owner_cid: field(4).and_then(Value::as_i64).map(|v| v as i32),
        count: field(5).and_then(Value::as_i64).map(|v| v as i32),
        status,
        grid,
    })
}

pub struct WorldClient {
    pub conn: Connection,
    pub ticket: WorldTicket,
    pub cid: i32,
    pub position: Vec3,
    pub seen: Vec<SeenCharacter>,
}

impl WorldClient {
    fn track_view_event(seen: &mut Vec<SeenCharacter>, event: &WorldEvent) {
        match event {
            WorldEvent::CharacterSeen(character) => {
                if let Some(existing) = seen.iter_mut().find(|c| c.cid == character.cid) {
                    *existing = character.clone();
                } else {
                    seen.push(character.clone());
                }
            }
            WorldEvent::CharacterUpdated {
                cid,
                position,
                destination,
                ..
            } => {
                if let Some(character) = seen.iter_mut().find(|c| c.cid == *cid) {
                    if let Some(position) = position {
                        character.position = *position;
                    }
                    if let Some(destination) = destination {
                        character.destination = *destination;
                    }
                }
            }
            WorldEvent::CharacterMoved {
                cid,
                position,
                destination,
            } => {
                if let Some(character) = seen.iter_mut().find(|c| c.cid == *cid) {
                    character.position = *position;
                    character.destination = *destination;
                }
            }
            WorldEvent::CharacterRemoved { cid, .. } => {
                seen.retain(|character| character.cid != *cid);
            }
            _ => {}
        }
    }

    /// Se connecte au monde, s'annonce (op 2 + keepalive) et attend la position de chargement.
    pub async fn connect(ticket: WorldTicket) -> Result<Self, NetError> {
        let mut conn = Connection::connect("world", &ticket.host, ticket.port).await?;
        conn.language = ticket.language.clone();
        conn.handshake().await?;
        let who = format!("{}#{}", ticket.character, ticket.world);
        conn.send(
            main::CHAR_MANAGER,
            &sparse(
                main::CHAR_MANAGER,
                &[
                    (0, Value::I8(char_mgr_op::GAME_ENTER)),
                    (2, Value::I32(17)),
                    (3, Value::str(&who)),
                    (6, Value::I32(ticket.auth_key)),
                    (7, Value::Str(Vec::new())),
                ],
            ),
        )
        .await?;
        conn.send(
            main::CHAR_MANAGER,
            &sparse(
                main::CHAR_MANAGER,
                &[(0, Value::I8(char_mgr_op::KEEPALIVE)), (2, Value::I32(0))],
            ),
        )
        .await?;
        Ok(Self {
            conn,
            ticket,
            cid: 0,
            position: Vec3::ZERO,
            seen: Vec::new(),
        })
    }

    /// Déroule l'entrée en jeu jusqu'à l'op 7 ; rend les événements dans l'ordre.
    pub async fn enter(&mut self) -> Result<Vec<WorldEvent>, NetError> {
        let mut events = vec![WorldEvent::Connected];
        self.conn.expect(packet_type::CONFIG, None).await?;
        let me = self.conn.expect(packet_type::CHARACTER, Some(25)).await?;
        if let WorldEvent::SelfCharacter { cid, .. } = translate(&me) {
            self.cid = cid;
        }
        events.push(translate(&me));
        let loading = self
            .conn
            .expect(
                packet_type::CHAR_MANAGER,
                Some(char_mgr_op::LOADING_POS as i64),
            )
            .await?;
        if let WorldEvent::LoadingPosition(p) = translate(&loading) {
            self.position = p;
        }
        events.push(translate(&loading));
        let timer = self.conn.expect(packet_type::TIMER, None).await?;
        events.push(translate(&timer));
        self.conn
            .send(
                main::CHAR_MANAGER,
                &sparse(
                    main::CHAR_MANAGER,
                    &[
                        (0, Value::I8(char_mgr_op::ENTER_WORLD)),
                        (4, Value::I32(self.cid)),
                    ],
                ),
            )
            .await?;
        loop {
            let p = self.conn.expect_where("entrée en jeu", |_| true).await?;
            let ev = translate(&p);
            Self::track_view_event(&mut self.seen, &ev);
            events.extend(extra_events(&p));
            match &ev {
                WorldEvent::EnteredWorld => {
                    events.push(ev);
                    break;
                }
                _ => {}
            }
            events.push(ev);
        }
        log::info!(
            "[world] en jeu : cid {} à {:?}, {} personnages en vue",
            self.cid,
            self.position,
            self.seen.len()
        );
        // HELLO_ACK arrive juste après la poignée de main, avant tout le reste.
        if let Some(e) = self.conn.take_new_extensions() {
            events.insert(1, WorldEvent::ExtensionsReady(e));
        }
        Ok(events)
    }

    /// Envoie un déplacement (type 0x40, format du client).
    pub async fn send_move(&mut self, position: Vec3, destination: Vec3) -> Result<(), NetError> {
        self.send_move_flags(position, destination, 0).await
    }

    /// Envoie un déplacement avec des drapeaux explicites (`0x04` arrêt, `0x08` synchronisation).
    pub async fn send_move_flags(
        &mut self,
        position: Vec3,
        destination: Vec3,
        flags: i8,
    ) -> Result<(), NetError> {
        let values = sparse(
            main::OPT_CHAR_MOVE,
            &[
                (0, Value::I32(self.cid)),
                (1, Value::Vec3(position.to_array())),
                (2, Value::Vec3(destination.to_array())),
                (5, Value::I8(flags)),
                (6, Value::I8(0)),
            ],
        );
        self.conn.send(main::OPT_CHAR_MOVE, &values).await
    }

    /// Signale l'arrêt du personnage (position finale, drapeau 0x04).
    pub async fn send_stop(&mut self, position: Vec3) -> Result<(), NetError> {
        self.send_move_flags(position, position, 0x04).await
    }

    /// Envoie un message sur le canal « dire » (type 0x0F).
    pub async fn send_say(&mut self, message: &str) -> Result<(), NetError> {
        let values = sparse(
            main::CHATTING,
            &[
                (0, Value::I8(0)),
                (1, Value::I32(self.cid)),
                (2, Value::I8(0)),
                (6, Value::Block(message.as_bytes().to_vec())),
            ],
        );
        self.conn.send(main::CHATTING, &values).await
    }

    /// Envoie un message d'AddOn (`ADDON_SEND`) si le serveur a accordé `addon-msg` ; sinon le
    /// message est abandonné (le serveur l'ignorerait de toute façon).
    pub async fn send_addon(
        &mut self,
        request_id: u32,
        prefix: &str,
        channel: u8,
        target: Option<&str>,
        payload: &[u8],
    ) -> Result<(), NetError> {
        if !self
            .conn
            .extensions
            .as_ref()
            .is_some_and(|e| e.has(ext::ADDON_MSG))
        {
            log::debug!("[world] message d'AddOn {prefix} abandonné : pas de capacité addon-msg");
            return Ok(());
        }
        let raw = ext::addon_send_packet(request_id, prefix, channel, target, payload);
        self.conn.send_raw(&raw).await
    }

    /// Accord d'extensions de la connexion au monde, s'il y en a un.
    pub fn extensions(&self) -> Option<&Extensions> {
        self.conn.extensions.as_ref()
    }

    fn has_capability(&self, capability: &str) -> bool {
        self.conn
            .extensions
            .as_ref()
            .is_some_and(|e| e.has(capability))
    }

    /// Change la langue des textes du serveur (`SET_LANGUAGE`) ; sans la capacité `language`, ou
    /// avec un code mal formé, rien n'est envoyé.
    pub async fn send_set_language(&mut self, request_id: u32, language: &str) -> Result<(), NetError> {
        if !self.has_capability(ext::LANGUAGE) {
            log::debug!("[world] langue {language} non envoyée : pas de capacité language");
            return Ok(());
        }
        match ext::set_language_packet(request_id, language) {
            Some(raw) => self.conn.send_raw(&raw).await,
            None => {
                log::warn!("[world] code de langue {language:?} mal formé, non envoyé");
                Ok(())
            }
        }
    }

    /// Demande des textes localisés (`TEXT_REQUEST`) ; la réponse arrive en `WorldEvent::Texts`.
    pub async fn request_texts(&mut self, request_id: u32, kind: &str, ids: &[u32]) -> Result<(), NetError> {
        if !self.has_capability(ext::LANGUAGE) {
            log::debug!("[world] textes {kind} non demandés : pas de capacité language");
            return Ok(());
        }
        match ext::text_request_packet(request_id, kind, ids) {
            Some(raw) => self.conn.send_raw(&raw).await,
            None => {
                log::warn!("[world] demande de textes {kind:?} ({} identifiant(s)) invalide, non envoyée", ids.len());
                Ok(())
            }
        }
    }

    async fn keepalive(&mut self) -> Result<(), NetError> {
        self.conn
            .send(
                main::CHAR_MANAGER,
                &sparse(
                    main::CHAR_MANAGER,
                    &[(0, Value::I8(char_mgr_op::KEEPALIVE)), (2, Value::I32(0))],
                ),
            )
            .await
    }

    /// Boucle de jeu réseau : relaie les paquets en événements et exécute les commandes,
    /// avec le keepalive toutes les 15 s. Se termine sur `Command::Quit` ou déconnexion.
    pub async fn run(
        mut self,
        events: mpsc::Sender<WorldEvent>,
        mut commands: mpsc::Receiver<Command>,
    ) {
        let mut tick = tokio::time::interval(KEEPALIVE);
        tick.tick().await; // le premier keepalive a déjà été envoyé
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    if let Err(e) = self.keepalive().await {
                        let _ = events.send(WorldEvent::Disconnected(e.to_string())).await;
                        return;
                    }
                }
                cmd = commands.recv() => {
                    let r = match cmd {
                        Some(Command::MoveTo { position, destination }) => self.send_move(position, destination).await,
                        Some(Command::Stop { position }) => self.send_stop(position).await,
                        Some(Command::Say(m)) => self.send_say(&m).await,
                        Some(Command::AddonSend { request_id, prefix, channel, target, payload }) => {
                            self.send_addon(request_id, &prefix, channel, target.as_deref(), &payload).await
                        }
                        Some(Command::SetLanguage { request_id, language }) => self.send_set_language(request_id, &language).await,
                        Some(Command::RequestTexts { request_id, kind, ids }) => self.request_texts(request_id, &kind, &ids).await,
                        Some(Command::Party(c)) => {
                            let (schema, values) = crate::party::command_packet(&c, self.cid);
                            self.conn.send(schema, &values).await
                        }
                        Some(Command::Combat(c)) => {
                            let (schema, values) = crate::combat::command_packet(&c, self.cid);
                            self.conn.send(schema, &values).await
                        }
                        Some(Command::Item(c)) => {
                            let (schema, values) = crate::items::command_packet(&c, self.cid);
                            self.conn.send(schema, &values).await
                        }
                        Some(Command::Npc(c)) => {
                            let (schema, values) = crate::npc::command_packet(&c, self.cid);
                            self.conn.send(schema, &values).await
                        }
                        Some(Command::Skill(c)) => {
                            let (schema, values) = crate::skills::command_packet(&c, self.cid);
                            self.conn.send(schema, &values).await
                        }
                        Some(Command::Teleport(c)) => {
                            let (schema, values) = crate::teleport::command_packet(&c, self.cid);
                            self.conn.send(schema, &values).await
                        }
                        Some(Command::Quit) | None => { self.conn.close().await; return; }
                    };
                    if let Err(e) = r {
                        let _ = events.send(WorldEvent::Disconnected(e.to_string())).await;
                        return;
                    }
                }
                p = self.conn.next() => {
                    match p {
                        Ok(p) => {
                            if let Some(e) = self.conn.take_new_extensions() {
                                if events.send(WorldEvent::ExtensionsReady(e)).await.is_err() { return; }
                            }
                            let ev = translate(&p);
                            Self::track_view_event(&mut self.seen, &ev);
                            if events.send(ev).await.is_err() { return; }
                            for extra in extra_events(&p) {
                                if events.send(extra).await.is_err() { return; }
                            }
                        }
                        Err(e) => {
                            let _ = events.send(WorldEvent::Disconnected(e.to_string())).await;
                            return;
                        }
                    }
                }
            }
        }
    }
}

/// Mini-paquet de vue (pour les tests et les outils serveur).
pub fn build_view(c: &SeenCharacter) -> Vec<u8> {
    build_mini(
        mini::OPT_VIEW_CHAR,
        &sparse(
            mini::OPT_VIEW_CHAR,
            &[
                (0, Value::I32(c.cid)),
                (1, Value::I32(c.tid)),
                (2, Value::Block(c.name.as_bytes().to_vec())),
                (8, Value::Vec3(c.position.to_array())),
                (9, Value::Vec3(c.destination.to_array())),
                (13, Value::I16(c.turn_y.unwrap_or(0.0).round() as i16)),
            ],
        ),
    )
    .expect("schéma statique")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_add_decodes_inventory_slot_from_agpmitem_packet() {
        use archrust_protocol::packet::{build_mini, build_packet, decode_packet, sparse};

        let slot = build_mini(
            &mini::ITEM_GRID,
            &sparse(
                &mini::ITEM_GRID,
                &[(0, Value::I16(1)), (1, Value::I16(2)), (2, Value::I16(3))],
            ),
        )
        .unwrap();
        let raw = build_packet(
            &main::ITEM,
            &sparse(
                &main::ITEM,
                &[
                    (0, Value::U8(0)),
                    (1, Value::I8(2)),
                    (2, Value::I32(9001)),
                    (3, Value::I32(1234)),
                    (4, Value::I32(77)),
                    (5, Value::I32(5)),
                    (7, Value::Packet(slot)),
                ],
            ),
        )
        .unwrap();
        let packet = decode_packet(&main::ITEM, &raw).unwrap();
        assert_eq!(main::by_id(packet_type::ITEM).unwrap().name, "AGPMITEM");
        let event = translate(&Incoming {
            packet_type: packet_type::ITEM,
            packet: Some(packet),
            raw,
        });
        let WorldEvent::ItemChanged(item) = event else {
            panic!("paquet AgpmItem non traduit")
        };
        assert_eq!(item.operation, 0);
        assert_eq!(item.id, 9001);
        assert_eq!(item.template_id, Some(1234));
        assert_eq!(item.owner_cid, Some(77));
        assert_eq!(item.count, Some(5));
        assert_eq!(item.status, Some(2));
        assert_eq!(item.grid, Some([1, 2, 3]));
    }

    #[test]
    fn optimized_view_decodes_full_item_snapshot() {
        use archrust_protocol::packet::{build_mini, build_packet, decode_packet, sparse};

        let mut block = Vec::new();
        for (id, tid, count) in [(11, 101, 3), (12, 202, 1)] {
            let item = build_mini(
                &main::ITEM,
                &sparse(
                    &main::ITEM,
                    &[
                        (0, Value::U8(0)),
                        (1, Value::I8(2)),
                        (2, Value::I32(id)),
                        (3, Value::I32(tid)),
                        (5, Value::I32(count)),
                    ],
                ),
            )
            .unwrap();
            block.extend_from_slice(&(item.len() as u32).to_le_bytes());
            block.extend_from_slice(&item);
        }
        let raw = build_packet(
            &main::OPT_VIEW,
            &sparse(
                &main::OPT_VIEW,
                &[
                    (0, Value::I8(view_op::ADD_CHARACTER_ITEMS)),
                    (6, Value::Block(block)),
                ],
            ),
        )
        .unwrap();
        let packet = decode_packet(&main::OPT_VIEW, &raw).unwrap();
        let event = translate(&Incoming {
            packet_type: packet_type::OPT_VIEW,
            packet: Some(packet),
            raw,
        });
        let WorldEvent::ItemsSnapshot(items) = event else {
            panic!("inventaire complet non décodé")
        };
        assert_eq!(items.len(), 2);
        assert_eq!(
            (items[0].id, items[0].template_id, items[0].count),
            (11, Some(101), Some(3))
        );
        assert_eq!(
            (items[1].id, items[1].template_id, items[1].count),
            (12, Some(202), Some(1))
        );
    }

    #[test]
    fn view_roundtrip() {
        let c = SeenCharacter {
            npc_functions: Vec::new(),
            cid: 0x4000_0001,
            tid: 584,
            name: "<Merchant> Darryl".into(),
            position: Vec3::new(-479097.9, 4753.4, -66021.9),
            destination: Vec3::ZERO,
            turn_y: Some(135.0),
            equipment: Vec::new(),
            action_status: None,
            stats: None,
        };
        let bytes = build_view(&c);
        let back = decode_view(&bytes).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn character_view_decodes_equipped_item_packets() {
        use archrust_protocol::packet::{build_mini, build_packet, decode_packet, sparse};

        let character = SeenCharacter {
            npc_functions: Vec::new(),
            cid: 123,
            tid: 96,
            name: "Joueur".into(),
            position: Vec3::ZERO,
            destination: Vec3::ZERO,
            turn_y: Some(0.0),
            equipment: Vec::new(),
            action_status: None,
            stats: None,
        };
        let view = build_view(&character);
        let equipped = build_mini(
            &mini::OPT_VIEW_ITEM,
            &sparse(
                &mini::OPT_VIEW_ITEM,
                &[
                    (0, Value::I32(9001)),
                    (1, Value::I32(1)),
                    (2, Value::I16(1)),
                ],
            ),
        )
        .unwrap();
        let raw = build_packet(
            &main::OPT_VIEW,
            &sparse(
                &main::OPT_VIEW,
                &[
                    (0, Value::I8(view_op::ADD_CHAR)),
                    (1, Value::Packet(view)),
                    (2, Value::PacketList(vec![equipped])),
                ],
            ),
        )
        .unwrap();
        let packet = decode_packet(&main::OPT_VIEW, &raw).unwrap();
        let event = translate(&Incoming {
            packet_type: packet_type::OPT_VIEW,
            packet: Some(packet),
            raw,
        });
        let WorldEvent::CharacterSeen(character) = event else {
            panic!("vue de personnage avec équipement non traduite")
        };
        assert_eq!(character.cid, 123);
        assert_eq!(
            character.equipment,
            vec![EquipmentView {
                item_id: 9001,
                template_id: 1,
                count: 1,
            }]
        );
    }

    #[test]
    fn optimized_character_action_decodes_server_fields() {
        use archrust_protocol::packet::{build_mini, build_packet, decode_packet, sparse};

        // Forme du client x64 : AGPMCHAR_FACTORS complet, sous-paquet 0 « résultat » (dégâts en
        // négatif), sous-paquet 3 PV et PM restants.
        let shown = build_mini(
            &mini::FACTOR_POINT,
            &sparse(
                &mini::FACTOR_POINT,
                &[(9, Value::I32(-7)), (12, Value::I32(-3)), (19, Value::I32(-2))],
            ),
        )
        .unwrap();
        let result = build_mini(&mini::CHAR_FACTORS, &sparse(&mini::CHAR_FACTORS, &[(3, Value::Packet(shown))])).unwrap();
        let current = build_mini(&mini::FACTOR_POINT, &sparse(&mini::FACTOR_POINT, &[(0, Value::I32(780)), (1, Value::I32(20))])).unwrap();
        let factor_packet_bytes = build_mini(
            &mini::CHAR_FACTORS,
            &sparse(&mini::CHAR_FACTORS, &[(0, Value::Packet(result)), (3, Value::Packet(current))]),
        )
        .unwrap();

        let raw = build_packet(
            &archrust_protocol::schema::main::OPT_CHAR_ACTION,
            &sparse(
                &archrust_protocol::schema::main::OPT_CHAR_ACTION,
                &[
                    (0, Value::I32(123)),
                    (1, Value::I32(456)),
                    (2, Value::I8(2)),
                    (3, Value::Packet(factor_packet_bytes.clone())),
                    (4, Value::I32(789)),
                    (5, Value::I8(1)),
                    (6, Value::I8(1)),
                    (7, Value::U32(0x20)),
                    (8, Value::U8(3)),
                ],
            ),
        )
        .unwrap();
        let packet =
            decode_packet(&archrust_protocol::schema::main::OPT_CHAR_ACTION, &raw).unwrap();
        let event = translate(&Incoming {
            packet_type: packet_type::OPT_CHAR_ACTION,
            packet: Some(packet),
            raw,
        });
        let WorldEvent::CharacterAction {
            cid,
            target_cid,
            result,
            factor_packet,
            factors,
            target_hp,
            combo,
            force_attack,
            additional_effect,
            hit_index,
            damage,
        } = event
        else {
            panic!("paquet 0x41 non traduit en action")
        };
        assert_eq!((cid, target_cid, result), (123, 456, Some(2)));
        assert_eq!(
            factor_packet.as_deref(),
            Some(factor_packet_bytes.as_slice())
        );
        assert_eq!(
            factors,
            Some(ActionFactorPoints {
                hp: Some(780),
                mp: Some(20),
                normal_damage: Some(-7),
                fire_damage: Some(-3),
                heroic_damage: Some(-2),
                ..Default::default()
            })
        );
        assert_eq!(damage, Some(12));
        assert_eq!(
            (target_hp, combo, force_attack),
            (Some(789), Some(true), Some(true))
        );
        assert_eq!((additional_effect, hit_index), (Some(0x20), Some(3)));
    }

    #[test]
    fn character_remove_for_view_is_not_a_self_character() {
        use archrust_protocol::packet::{build_packet, decode_packet, sparse};

        let raw = build_packet(
            &archrust_protocol::schema::main::CHARACTER,
            &sparse(
                &archrust_protocol::schema::main::CHARACTER,
                &[
                    (0, Value::I8(character_op::REMOVE_FOR_VIEW)),
                    (1, Value::I32(0x4000_1234)),
                ],
            ),
        )
        .unwrap();
        let packet = decode_packet(&archrust_protocol::schema::main::CHARACTER, &raw).unwrap();
        assert!(matches!(
            translate(&Incoming {
                packet_type: packet_type::CHARACTER,
                packet: Some(packet),
                raw,
            }),
            WorldEvent::CharacterRemoved {
                cid: 0x4000_1234,
                for_view: true
            }
        ));
    }

    #[test]
    fn removing_character_for_view_updates_world_client_cache() {
        let mut seen = vec![SeenCharacter {
            npc_functions: Vec::new(),
            cid: 42,
            tid: 96,
            name: "Cible".into(),
            position: Vec3::ZERO,
            destination: Vec3::ZERO,
            turn_y: Some(0.0),
            equipment: Vec::new(),
            action_status: None,
            stats: None,
        }];
        WorldClient::track_view_event(
            &mut seen,
            &WorldEvent::CharacterRemoved {
                cid: 42,
                for_view: true,
            },
        );
        assert!(seen.is_empty());
    }

    #[test]
    fn character_update_decodes_movement_and_factor_point_hp() {
        use archrust_protocol::packet::{build_mini, build_packet, decode_packet, sparse};

        let movement = build_mini(
            &mini::CHAR_MOVE,
            &sparse(
                &mini::CHAR_MOVE,
                &[
                    (0, Value::Vec3([1.0, 2.0, 3.0])),
                    (1, Value::Vec3([4.0, 5.0, 6.0])),
                ],
            ),
        )
        .unwrap();
        let point = build_mini(
            &mini::FACTOR_POINT,
            &sparse(&mini::FACTOR_POINT, &[(0, Value::I32(321))]),
        )
        .unwrap();
        let factors = build_mini(
            &mini::CHAR_FACTORS,
            &sparse(&mini::CHAR_FACTORS, &[(3, Value::Packet(point))]),
        )
        .unwrap();
        let raw = build_packet(
            &archrust_protocol::schema::main::CHARACTER,
            &sparse(
                &archrust_protocol::schema::main::CHARACTER,
                &[
                    (0, Value::I8(character_op::UPDATE)),
                    (1, Value::I32(77)),
                    (5, Value::Packet(movement)),
                    (7, Value::Packet(factors)),
                ],
            ),
        )
        .unwrap();
        let packet = decode_packet(&archrust_protocol::schema::main::CHARACTER, &raw).unwrap();
        let WorldEvent::CharacterUpdated {
            cid,
            position,
            destination,
            factors,
            ..
        } = translate(&Incoming {
            packet_type: packet_type::CHARACTER,
            packet: Some(packet),
            raw,
        })
        else {
            panic!("mise à jour personnage non décodée")
        };
        assert_eq!(cid, 77);
        assert_eq!(position, Some(Vec3::new(1.0, 2.0, 3.0)));
        assert_eq!(destination, Some(Vec3::new(4.0, 5.0, 6.0)));
        assert_eq!(factors.and_then(|f| f.hp), Some(321));
    }

    #[test]
    fn full_character_packet_of_another_player_keeps_position_and_head() {
        use archrust_protocol::packet::{build_packet, decode_packet};

        // Forme de l'op 0 qu'ArchCore envoie quand un joueur entre dans la vue.
        let movement = build_mini(
            &mini::CHAR_MOVE,
            &sparse(&mini::CHAR_MOVE, &[(0, Value::Vec3([-476_000.0, 4_422.0, -61_000.0]))]),
        )
        .unwrap();
        let point = build_mini(&mini::FACTOR_POINT, &sparse(&mini::FACTOR_POINT, &[(0, Value::I32(96))])).unwrap();
        let factors = build_mini(&mini::CHAR_FACTORS, &sparse(&mini::CHAR_FACTORS, &[(3, Value::Packet(point))])).unwrap();
        let c = &main::CHARACTER;
        let raw = build_packet(
            c,
            &sparse(
                c,
                &[
                    (0, Value::I8(character_op::ADD)),
                    (1, Value::I32(1)),
                    (2, Value::I32(96)),
                    (3, Value::Block(b"SimA\0".to_vec())),
                    (5, Value::Packet(movement)),
                    (7, Value::Packet(factors)),
                    (20, Value::I8(2)),
                    (21, Value::I8(1)),
                ],
            ),
        )
        .unwrap();
        let packet = decode_packet(c, &raw).unwrap();
        let WorldEvent::SelfCharacter { cid, tid, name, position, face, hair, stats } =
            translate(&Incoming { packet_type: packet_type::CHARACTER, packet: Some(packet), raw })
        else {
            panic!("fiche de personnage non traduite")
        };
        assert_eq!((cid, tid, name.as_str()), (1, 96, "SimA"));
        assert_eq!(position, Some(Vec3::new(-476_000.0, 4_422.0, -61_000.0)));
        assert_eq!((face, hair), (Some(2), Some(1)));
        assert_eq!(stats.and_then(|s| s.hp), Some(96));
    }

    #[test]
    fn item_snapshot_accepts_full_packets_like_archcore() {
        use archrust_protocol::packet::build_packet;
        let grid = build_mini(&mini::ITEM_GRID, &sparse(&mini::ITEM_GRID, &[(0, Value::I16(2)), (1, Value::I16(1)), (2, Value::I16(3))])).unwrap();
        let one = |iid: i32, tid: i32| {
            build_packet(
                &main::ITEM,
                &sparse(
                    &main::ITEM,
                    &[(0, Value::I8(0)), (1, Value::I8(2)), (2, Value::I32(iid)), (3, Value::I32(tid)), (5, Value::I32(1)), (7, Value::Packet(grid.clone()))],
                ),
            )
            .unwrap()
        };
        let block = [one(10, 4), one(11, 1)].concat();
        let items = decode_item_batch(&block).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!((items[0].id, items[0].template_id, items[0].grid), (10, Some(4), Some([2, 1, 3])));
        assert_eq!(items[1].template_id, Some(1));
    }

    #[test]
    fn character_stats_from_full_factors() {
        let status = build_mini(&mini::FACTOR_STATUS, &sparse(&mini::FACTOR_STATUS, &[(7, Value::I32(5))])).unwrap();
        let point = build_mini(
            &mini::FACTOR_POINT,
            &sparse(&mini::FACTOR_POINT, &[(0, Value::I32(80)), (1, Value::I32(30)), (3, Value::I32(1234)), (4, Value::I32(1))]),
        )
        .unwrap();
        let max = build_mini(&mini::FACTOR_POINTMAX, &sparse(&mini::FACTOR_POINTMAX, &[(0, Value::I32(120)), (1, Value::I32(40))])).unwrap();
        let factors = build_mini(
            &mini::CHAR_FACTORS,
            &sparse(&mini::CHAR_FACTORS, &[(1, Value::Packet(status)), (3, Value::Packet(point)), (4, Value::Packet(max))]),
        )
        .unwrap();
        let stats = decode_character_stats(&factors).unwrap();
        assert_eq!(
            stats,
            CharacterStats { level: Some(5), hp: Some(80), mp: Some(30), max_hp: Some(120), max_mp: Some(40), exp: Some(1234 + (1u64 << 32)), ..Default::default() }
        );
    }

    #[test]
    fn skill_buff_add_and_remove() {
        use archrust_protocol::packet::{build_packet, decode_packet};
        let base = build_mini(&mini::SKILL_BASE, &sparse(&mini::SKILL_BASE, &[(0, Value::I8(1)), (1, Value::I32(42))])).unwrap();
        let buff = build_mini(
            &mini::SKILL_BUFF,
            &sparse(&mini::SKILL_BUFF, &[(0, Value::I32(1201)), (1, Value::U64(30_000)), (2, Value::I32(96))]),
        )
        .unwrap();
        let skill = &archrust_protocol::schema::main::SKILL;
        let make = |op: i8| {
            let raw = build_packet(
                skill,
                &sparse(skill, &[(0, Value::I8(op)), (1, Value::I32(7)), (2, Value::Packet(base.clone())), (8, Value::PacketList(vec![buff.clone()]))]),
            )
            .unwrap();
            let packet = decode_packet(skill, &raw).unwrap();
            translate(&Incoming { packet_type: packet_type::SKILL, packet: Some(packet), raw })
        };
        match make(4) {
            WorldEvent::BuffAdded(b) => assert_eq!(b, BuffInfo { owner: 42, skill_id: 7, skill_tid: 1201, duration_ms: 30_000 }),
            other => panic!("ajout attendu, reçu {other:?}"),
        }
        match make(5) {
            WorldEvent::BuffRemoved { owner, skill_tid } => assert_eq!((owner, skill_tid), (42, 1201)),
            other => panic!("retrait attendu, reçu {other:?}"),
        }
    }
}
