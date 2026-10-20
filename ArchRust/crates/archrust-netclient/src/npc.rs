//! Fonctions des PNJ : dialogue, boutique (achat, vente), banque (objets, argent), quêtes.
//!
//! Dialogue validé avec ArchCore (scénarios `sim shop`, `sim bank`, `sim quest`) :
//!
//! ```text
//! vue d'un PNJ (0x42), champ 4 : paquets 0x3C mis bout à bout, un par fonction
//!     champ 0 mini EVENT_BASE (3, CID du PNJ, -, fonction), champ 1 mini à un entier (modèle)
//!     fonctions : 21 dialogue (modèle de npcdialog.txt), 9 boutique, 20 banque, 27 quêtes (groupe)
//! 0x37 op 0 (1 EVENT_BASE, 2 CID)            → op 1 : dialogue accordé (PNJ à moins de 1500)
//! 0x32 op 0 (0 EVENT_BASE, 2 CID)            → par objet : AGPMITEM statut 10, puis 0x32 op 0
//!                                              (3 IID, 4 détail : TID, prix, quantité, statut,
//!                                              sac, ligne, colonne) ; op 1 : fin de la liste
//! 0x32 op 2 (3 IID, 4 détail : quantité au champ 2) → op 2, résultat 2 réussi, 3 or, 4 plein
//! 0x32 op 3 (3 IID d'un objet du sac)        → op 3, résultat 0 réussi, 1 trop d'or
//! 0x36 op 0 (1 EVENT_BASE, 2 CID)            → op 1 : banque ouverte
//! AGPMITEM op 2 statut 100, champ 8 = case   déposer ; op 2 statut 2 (case au champ 7 ou rien) : retirer
//! AGPMCHARACTER op 7 (1 CID, 9 montant)      déposer de l'argent (négatif : retirer) → op 1 champs 8, 9
//! 0x3E op 0 (1 EVENT_BASE, 2 CID)            → op 0 : fenêtre des quêtes du PNJ
//! AGPMQUEST op 0 prendre, 2 terminer (7 récompense), 6 abandonner (1 CID, 2 EVENT_BASE, 4 TID)
//!     → op 1, 3, 7 (4 TID, 6 résultat 1 réussi / 0 refus) ; op 4 et 10 : quête du journal
//!     (3 mini QUEST_INFO) ; op 5 : quêtes terminées (8 à 10 : bits, TID % 16 384 du bloc TID / 16 384)
//! ```

use archrust_protocol::packet::{build_mini, decode_mini, decode_packet, sparse, Value};
use archrust_protocol::schema::{main, mini, Schema};

/// Fonctions d'événement d'un PNJ (`Function` de npc.ini).
pub mod function {
    pub const TRADE: i32 = 9;
    pub const BANK: i32 = 20;
    pub const DIALOG: i32 = 21;
    pub const SKILL_MASTER: i32 = 25;
    pub const QUEST: i32 = 27;
}

/// Type d'objet « personnage » d'une désignation d'événement.
const BASE_TYPE_CHARACTER: i8 = 3;

/// Résultats d'achat (0x32 op 2) et de vente (op 3).
pub mod trade_result {
    pub const BUY_OK: i32 = 2;
    pub const NOT_ENOUGH_MONEY: i32 = 3;
    pub const INVENTORY_FULL: i32 = 4;
    pub const SELL_OK: i32 = 0;
    pub const TOO_MUCH_MONEY: i32 = 1;
}

/// Statut AgpmItem d'un objet en banque (sa case est au champ 8).
pub const STATUS_BANK: i8 = 100;

/// Fonction d'un PNJ et sa donnée (modèle de dialogue, de boutique, groupe de quêtes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NpcFunction {
    pub function: i32,
    pub data: Option<i32>,
    /// Maître des compétences (fonction 25) : race et classe enseignées.
    pub teach: Option<(i32, i32)>,
}

fn int(values: &[Option<Value>], i: usize) -> Option<i64> {
    values.get(i).and_then(Option::as_ref).and_then(Value::as_i64)
}

/// Fonctions d'un PNJ : paquets 0x3C complets mis bout à bout (champ 4 de sa vue).
pub fn decode_functions(mut bytes: &[u8]) -> Vec<NpcFunction> {
    let mut out = Vec::new();
    while bytes.len() > 5 {
        let Ok(len) = bytes[1..5].try_into().map(u32::from_le_bytes) else { break };
        let len = len as usize;
        if len < archrust_protocol::packet::HEADER_SIZE || len > bytes.len() {
            break;
        }
        let (one, rest) = bytes.split_at(len);
        bytes = rest;
        let Ok(p) = decode_packet(main::EVENT_MANAGER, one) else { continue };
        let base = p.values.first().and_then(Option::as_ref).and_then(Value::as_bytes).and_then(|b| decode_mini(mini::EVENT_BASE, b).ok());
        let Some(function) = base.as_ref().and_then(|(_, v)| int(v, 3)) else { continue };
        let raw = p.values.get(1).and_then(Option::as_ref).and_then(Value::as_bytes);
        if function as i32 == self::function::SKILL_MASTER {
            let teach = raw.and_then(|b| decode_mini(mini::SKILL_MASTER_DATA, b).ok()).map(|(_, v)| (int(&v, 0).unwrap_or(0) as i32, int(&v, 1).unwrap_or(0) as i32));
            out.push(NpcFunction { function: function as i32, data: None, teach });
            continue;
        }
        let data = raw.and_then(|b| decode_mini(mini::EVENT_DATA_INT32, b).ok()).and_then(|(_, v)| int(&v, 0));
        out.push(NpcFunction { function: function as i32, data: data.map(|d| d as i32), teach: None });
    }
    out
}

/// Mini EVENT_BASE désignant la fonction `function` du PNJ `npc`.
pub fn event_base(npc: i32, function: i32) -> Value {
    let s = mini::EVENT_BASE;
    let values = sparse(s, &[(0, Value::I8(BASE_TYPE_CHARACTER)), (1, Value::I32(npc)), (3, Value::I32(function))]);
    Value::Packet(build_mini(s, &values).unwrap_or_default())
}

fn grid_value(grid: [i16; 3]) -> Value {
    let values: Vec<Option<Value>> = grid.iter().map(|v| Some(Value::I16(*v))).collect();
    Value::Packet(build_mini(mini::ITEM_GRID, &values).unwrap_or_default())
}

/// Demandes aux PNJ.
#[derive(Debug, Clone, PartialEq)]
pub enum NpcCommand {
    /// Ouvrir le dialogue d'un PNJ.
    Dialog(i32),
    /// Demander la liste de la boutique.
    ShopList(i32),
    /// Acheter `count` exemplaires d'un objet de la boutique.
    Buy { npc: i32, iid: i32, count: i32 },
    /// Vendre un objet du sac.
    Sell { npc: i32, iid: i32 },
    /// Ouvrir la banque auprès d'un gardien.
    BankOpen(i32),
    /// Déposer un objet du sac dans une case de la banque (4 pages de 7 × 4).
    BankStore { iid: i32, grid: [i16; 3] },
    /// Retirer un objet de la banque, vers une case du sac ou la première libre.
    BankTake { iid: i32, grid: Option<[i16; 3]> },
    /// Déposer de l'argent (négatif : retirer).
    BankMoney(i64),
    /// Ouvrir la fenêtre des quêtes d'un PNJ.
    QuestWindow(i32),
    /// Prendre une quête (auprès d'un PNJ, ou depuis le journal).
    QuestAccept { npc: Option<i32>, tid: i32 },
    /// Terminer une quête, avec la récompense choisie.
    QuestComplete { npc: Option<i32>, tid: i32, reward: i32 },
    /// Abandonner une quête.
    QuestAbandon(i32),
}

/// Paquet d'une demande aux PNJ pour le personnage `me`.
pub fn command_packet(cmd: &NpcCommand, me: i32) -> (&'static Schema, Vec<Option<Value>>) {
    let quest = |op: i8, npc: Option<i32>, tid: i32, reward: Option<i32>| {
        let mut v = vec![(0, Value::I8(op)), (1, Value::I32(me)), (4, Value::I32(tid))];
        if let Some(n) = npc {
            v.push((2, event_base(n, function::QUEST)));
        }
        if let Some(r) = reward {
            v.push((7, Value::I32(r)));
        }
        (main::QUEST, sparse(main::QUEST, &v))
    };
    match cmd {
        NpcCommand::Dialog(npc) => (
            main::EVENT_NPC_DIALOG,
            sparse(main::EVENT_NPC_DIALOG, &[(0, Value::I8(0)), (1, event_base(*npc, function::DIALOG)), (2, Value::I32(me))]),
        ),
        NpcCommand::ShopList(npc) => (
            main::EVENT_NPC_TRADE,
            sparse(main::EVENT_NPC_TRADE, &[(0, event_base(*npc, function::TRADE)), (1, Value::I8(0)), (2, Value::I32(me))]),
        ),
        NpcCommand::Buy { npc, iid, count } => {
            let d = mini::NPC_TRADE_DETAIL;
            let detail = Value::Packet(build_mini(d, &sparse(d, &[(2, Value::I32(*count))])).unwrap_or_default());
            (
                main::EVENT_NPC_TRADE,
                sparse(
                    main::EVENT_NPC_TRADE,
                    &[(0, event_base(*npc, function::TRADE)), (1, Value::I8(2)), (2, Value::I32(me)), (3, Value::I32(*iid)), (4, detail)],
                ),
            )
        }
        NpcCommand::Sell { npc, iid } => (
            main::EVENT_NPC_TRADE,
            sparse(main::EVENT_NPC_TRADE, &[(0, event_base(*npc, function::TRADE)), (1, Value::I8(3)), (2, Value::I32(me)), (3, Value::I32(*iid))]),
        ),
        NpcCommand::BankOpen(npc) => (
            main::EVENT_BANK,
            sparse(main::EVENT_BANK, &[(0, Value::I8(0)), (1, event_base(*npc, function::BANK)), (2, Value::I32(me))]),
        ),
        NpcCommand::BankStore { iid, grid } => (
            main::ITEM,
            sparse(main::ITEM, &[(0, Value::U8(2)), (1, Value::I8(STATUS_BANK)), (2, Value::I32(*iid)), (4, Value::I32(me)), (8, grid_value(*grid))]),
        ),
        NpcCommand::BankTake { iid, grid } => {
            let mut v = vec![(0, Value::U8(2)), (1, Value::I8(2)), (2, Value::I32(*iid)), (4, Value::I32(me))];
            if let Some(g) = grid {
                v.push((7, grid_value(*g)));
            }
            (main::ITEM, sparse(main::ITEM, &v))
        }
        NpcCommand::BankMoney(amount) => (
            main::CHARACTER,
            sparse(main::CHARACTER, &[(0, Value::I8(7)), (1, Value::I32(me)), (9, Value::I64(*amount))]),
        ),
        NpcCommand::QuestWindow(npc) => (
            main::EVENT_QUEST,
            sparse(main::EVENT_QUEST, &[(0, Value::I8(0)), (1, event_base(*npc, function::QUEST)), (2, Value::I32(me))]),
        ),
        NpcCommand::QuestAccept { npc, tid } => quest(0, *npc, *tid, None),
        NpcCommand::QuestComplete { npc, tid, reward } => quest(2, *npc, *tid, Some(*reward)),
        NpcCommand::QuestAbandon(tid) => quest(6, None, *tid, None),
    }
}

/// Quête du journal (mini QUEST_INFO).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuestProgress {
    pub tid: i32,
    pub monster_a: i32,
    pub monster_b: i32,
    /// Objectifs remplis (la quête peut être rendue).
    pub done: bool,
}

/// Réponses des PNJ.
#[derive(Debug, Clone, PartialEq)]
pub enum NpcEvent {
    /// Dialogue accordé (0x37 op 1).
    DialogGranted { npc: i32 },
    /// Objet de la boutique (0x32 op 0) : IID, TID, prix, quantité, case.
    ShopItem { npc: i32, iid: i32, tid: i32, price: i64, count: i32, grid: [i16; 3] },
    /// Fin de la liste de la boutique (0x32 op 1).
    ShopEnd { npc: i32 },
    /// Résultat d'un achat (0x32 op 2).
    Bought { result: i32 },
    /// Résultat d'une vente (0x32 op 3).
    Sold { result: i32 },
    /// Banque ouverte (0x36 op 1).
    BankOpened { npc: i32 },
    /// Fenêtre des quêtes d'un PNJ (0x3E op 0).
    QuestWindow { npc: i32 },
    /// Réponse à une prise (op 1), une fin (op 3) ou un abandon (op 7).
    QuestResult { op: i8, tid: i32, success: bool },
    /// Quête du journal (op 4) ou progression (op 10).
    QuestJournal { progress: QuestProgress, update: bool },
    /// Quêtes terminées (op 5).
    QuestsDone(Vec<u32>),
}

fn npc_of(values: &[Option<Value>], field: usize) -> i32 {
    values
        .get(field)
        .and_then(Option::as_ref)
        .and_then(Value::as_bytes)
        .and_then(|b| decode_mini(mini::EVENT_BASE, b).ok())
        .and_then(|(_, v)| int(&v, 1))
        .unwrap_or(0) as i32
}

/// TID des quêtes terminées, d'après les trois blocs de bits.
fn done_quests(values: &[Option<Value>]) -> Vec<u32> {
    let mut out = Vec::new();
    for (block, field) in [8usize, 9, 10].into_iter().enumerate() {
        let Some(bytes) = values.get(field).and_then(Option::as_ref).and_then(Value::as_bytes) else { continue };
        for (i, b) in bytes.iter().enumerate() {
            for bit in 0..8 {
                if b & (1 << bit) != 0 {
                    out.push((block * 16_384 + i * 8 + bit) as u32);
                }
            }
        }
    }
    out
}

/// Traduit un paquet de PNJ (types 0x37, 0x32, 0x36, 0x3E, 0x13), sinon `None`.
pub fn translate(packet_type: u8, values: &[Option<Value>]) -> Option<NpcEvent> {
    use archrust_protocol::schema::packet_type as T;
    let op = int(values, 0);
    Some(match packet_type {
        T::EVENT_NPC_DIALOG if op == Some(1) => NpcEvent::DialogGranted { npc: npc_of(values, 1) },
        T::EVENT_NPC_TRADE => {
            let npc = npc_of(values, 0);
            match int(values, 1)? {
                0 => {
                    let bytes = values.get(4).and_then(Option::as_ref).and_then(Value::as_bytes)?;
                    let (_, d) = decode_mini(mini::NPC_TRADE_DETAIL, bytes).ok()?;
                    NpcEvent::ShopItem {
                        npc,
                        iid: int(values, 3)? as i32,
                        tid: int(&d, 0)? as i32,
                        price: int(&d, 1).unwrap_or(0),
                        count: int(&d, 2).unwrap_or(1) as i32,
                        grid: [int(&d, 4).unwrap_or(0) as i16, int(&d, 5).unwrap_or(0) as i16, int(&d, 6).unwrap_or(0) as i16],
                    }
                }
                1 => NpcEvent::ShopEnd { npc },
                2 => NpcEvent::Bought { result: int(values, 6).unwrap_or(-1) as i32 },
                3 => NpcEvent::Sold { result: int(values, 6).unwrap_or(-1) as i32 },
                _ => return None,
            }
        }
        T::EVENT_BANK if op == Some(1) => NpcEvent::BankOpened { npc: npc_of(values, 1) },
        T::EVENT_QUEST if op == Some(0) => NpcEvent::QuestWindow { npc: npc_of(values, 1) },
        T::QUEST => match op? {
            o @ (1 | 3 | 7) => NpcEvent::QuestResult { op: o as i8, tid: int(values, 4)? as i32, success: int(values, 6) == Some(1) },
            o @ (4 | 10) => {
                let bytes = values.get(3).and_then(Option::as_ref).and_then(Value::as_bytes)?;
                let (_, q) = decode_mini(mini::QUEST_INFO, bytes).ok()?;
                NpcEvent::QuestJournal {
                    progress: QuestProgress {
                        tid: int(&q, 0)? as i32,
                        monster_a: int(&q, 1).unwrap_or(0) as i32,
                        monster_b: int(&q, 2).unwrap_or(0) as i32,
                        done: int(&q, 3).unwrap_or(0) != 0,
                    },
                    update: o == 10,
                }
            }
            5 => NpcEvent::QuestsDone(done_quests(values)),
            _ => return None,
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use archrust_protocol::packet::build_packet as encode_packet;

    #[test]
    fn fonctions_d_un_pnj() {
        let em = main::EVENT_MANAGER;
        let data = |v: i32| Value::Packet(build_mini(mini::EVENT_DATA_INT32, &[Some(Value::I32(v))]).unwrap());
        let mut block = encode_packet(em, &[Some(event_base(0x4000_0003, function::DIALOG)), Some(data(1))]).unwrap();
        block.extend(encode_packet(em, &[Some(event_base(0x4000_0003, function::TRADE)), Some(data(104))]).unwrap());
        block.extend(encode_packet(em, &[Some(event_base(0x4000_0003, function::BANK)), None]).unwrap());
        assert_eq!(
            decode_functions(&block),
            vec![
                NpcFunction { function: 21, data: Some(1), teach: None },
                NpcFunction { function: 9, data: Some(104), teach: None },
                NpcFunction { function: 20, data: None, teach: None },
            ]
        );
    }

    #[test]
    fn demandes_et_reponses() {
        let (s, v) = command_packet(&NpcCommand::Buy { npc: 7, iid: 42, count: 3 }, 1);
        assert_eq!(s.id, 0x32);
        assert_eq!((int(&v, 1), int(&v, 3)), (Some(2), Some(42)));
        let (_, v) = command_packet(&NpcCommand::BankMoney(-500), 1);
        assert_eq!((int(&v, 0), int(&v, 9)), (Some(7), Some(-500)));
        let (_, v) = command_packet(&NpcCommand::QuestAccept { npc: None, tid: 4632 }, 1);
        assert!(v[2].is_none());

        let d = mini::NPC_TRADE_DETAIL;
        let detail = build_mini(d, &sparse(d, &[(0, Value::I32(190)), (1, Value::I32(25)), (2, Value::I32(1)), (5, Value::I16(2))])).unwrap();
        let shop = sparse(main::EVENT_NPC_TRADE, &[(0, event_base(7, 9)), (1, Value::I8(0)), (3, Value::I32(42)), (4, Value::Packet(detail))]);
        assert_eq!(
            translate(0x32, &shop),
            Some(NpcEvent::ShopItem { npc: 7, iid: 42, tid: 190, price: 25, count: 1, grid: [0, 2, 0] })
        );
        let mut bits = vec![0u8; 0x800];
        bits[4632 / 8] |= 1 << (4632 % 8);
        let done = sparse(main::QUEST, &[(0, Value::I8(5)), (8, Value::Block(bits))]);
        assert_eq!(translate(0x13, &done), Some(NpcEvent::QuestsDone(vec![4632])));
    }
}
