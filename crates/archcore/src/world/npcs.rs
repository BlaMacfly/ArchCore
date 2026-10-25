//! PNJ placés dans le monde (importés de npc.ini dans `archcore_world.npc_spawn`).
//!
//! Apparition côté client : paquet AGPMOPTIMIZEDVIEW (0x42) op 1, dont le champ 1 est un mini « vue
//! de personnage » (drapeaux 4 octets) : 0 CID, 1 TID, 2 nom, 6 drapeaux de déplacement, 7 direction,
//! 8 position, 9 destination, 12-13 axes X/Y (INT16). Le champ 4 porte les fonctions du PNJ : une suite
//! de paquets ApmEventManager (0x3C) complets, un par fonction.

use crate::db::{col, Databases};
use crate::info;
use archcore_net::schema::{self, mini};
use archcore_net::{build_mini, build_packet, Fields, Vec3};
use std::collections::HashMap;
use std::path::Path;

/// Fonctions d'événement (ApmEventManager) des PNJ, numéros relevés dans npc.ini.
pub mod event_function {
    pub const NPC_TRADE: i64 = 9;
    pub const BANK: i64 = 20;
    /// Forgeron : conversion des objets (sockets, renforcement).
    pub const ITEM_CONVERT: i64 = 22;
    pub const GUILD: i64 = 23;
    pub const NPC_DIALOG: i64 = 21;
    pub const SKILL_MASTER: i64 = 25;
    pub const QUEST: i64 = 27;
}

/// Type d'objet « personnage » dans les mini EVENT_BASE (0 aucun, 1 objet, 2 modèle d'objet, 3 personnage).
pub const BASE_TYPE_CHARACTER: i64 = 3;
/// Type « objet du décor » dans les mini EVENT_BASE.
pub const BASE_TYPE_OBJECT: i64 = 1;
/// Les PNJ reçoivent des CID dans une plage séparée de celle des joueurs.
pub const NPC_CID_BASE: u32 = 0x4000_0000;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NpcEvent {
    pub function: i64,
    pub event_id: i64,
    pub params: Vec<(String, String)>,
}

impl NpcEvent {
    pub fn param(&self, key: &str) -> Option<&str> {
        self.params.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

#[derive(Clone, Debug, Default)]
pub struct Npc {
    pub cid: u32,
    pub tid: u32,
    pub name: String,
    pub pos: Vec3,
    pub degree_x: f64,
    pub degree_y: f64,
    pub events: Vec<NpcEvent>,
}

/// Lit les fonctions d'un PNJ dans la section npc.ini brute (liste JSON de paires clé/valeur) :
/// chaque « Function » ouvre un événement, suivi de son « EventID » et de ses paramètres.
pub fn parse_events(raw: &str) -> Vec<NpcEvent> {
    let Ok(pairs) = serde_json::from_str::<Vec<(String, String)>>(raw) else { return Vec::new() };
    let mut events: Vec<NpcEvent> = Vec::new();
    for (k, v) in pairs {
        if k == "Function" {
            events.push(NpcEvent { function: v.trim().parse().unwrap_or(0), ..Default::default() });
            continue;
        }
        let Some(cur) = events.last_mut() else { continue };
        if k == "EventID" {
            cur.event_id = v.trim().parse().unwrap_or(0);
        } else if k != "EnumEnd" {
            cur.params.push((k, v));
        }
    }
    events
}

/// Paramètre entier transmis pour chaque fonction, lu dans la section npc.ini.
fn event_int_param(function: i64) -> Option<&'static str> {
    match function {
        event_function::NPC_DIALOG | event_function::NPC_TRADE => Some("Template"),
        event_function::QUEST => Some("QuestGroupID"),
        _ => None,
    }
}

/// Mini EVENT_BASE désignant une fonction d'un PNJ.
pub fn event_base(npc_cid: u32, function: i64) -> Vec<u8> {
    let s = &mini::EVENT_BASE;
    build_mini(s, &Fields::new(s).with(0, BASE_TYPE_CHARACTER).with(1, npc_cid).with(3, function))
}

/// Paquets ApmEventManager (0x3C) d'un PNJ, concaténés pour le champ 4 de sa vue.
pub fn event_block(npc: &Npc) -> Vec<u8> {
    let mut out = Vec::new();
    for ev in &npc.events {
        if ev.function == event_function::SKILL_MASTER {
            // Maître des compétences : race et classe enseignées.
            let int = |k: &str| ev.param(k).and_then(|v| v.trim().parse::<i64>().ok()).unwrap_or(0);
            let sm = &mini::SKILL_MASTER_DATA;
            let data = build_mini(sm, &Fields::new(sm).with(0, int("SkillRaceType")).with(1, int("SkillClassType")));
            let em = &schema::EVENT_MANAGER;
            out.extend(build_packet(em, &Fields::new(em).with(0, event_base(npc.cid, ev.function)).with(1, data)));
            continue;
        }
        if matches!(ev.function, event_function::BANK | event_function::GUILD | event_function::ITEM_CONVERT) {
            // Gardien de banque, gestionnaire de guilde, forgeron : fonctions sans données.
            let em = &schema::EVENT_MANAGER;
            out.extend(build_packet(em, &Fields::new(em).with(0, event_base(npc.cid, ev.function))));
            continue;
        }
        let Some(key) = event_int_param(ev.function) else { continue };
        let value: i64 = ev.param(key).and_then(|v| v.trim().parse().ok()).unwrap_or(0);
        let data = build_mini(&mini::EVENT_DATA_INT32, &Fields::new(&mini::EVENT_DATA_INT32).with(0, value));
        let em = &schema::EVENT_MANAGER;
        out.extend(build_packet(em, &Fields::new(em).with(0, event_base(npc.cid, ev.function)).with(1, data)));
    }
    out
}

/// Traduction « <Titre> Nom » avec data/i18n/npc-titles-<langue>.json (les noms propres restent).
#[derive(Default)]
pub struct Translator {
    titles: HashMap<String, String>,
    names: HashMap<String, String>,
    suffixes: HashMap<String, String>,
}

impl Translator {
    pub fn load(root: &Path, language: &str) -> Self {
        if language.is_empty() || language == "en" {
            return Translator::default();
        }
        let file = root.join("data").join("i18n").join(format!("npc-titles-{language}.json"));
        let Ok(text) = std::fs::read_to_string(&file) else { return Translator::default() };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return Translator::default() };
        let map = |k: &str| -> HashMap<String, String> {
            v.get(k)
                .and_then(|m| m.as_object())
                .map(|o| o.iter().filter_map(|(a, b)| b.as_str().map(|b| (a.clone(), b.to_string()))).collect())
                .unwrap_or_default()
        };
        Translator { titles: map("titles"), names: map("names"), suffixes: map("suffixes") }
    }

    /// Ajoute une traduction de titre (`Merchant` → `Marchand`), pour les tests.
    #[cfg(test)]
    pub fn add_title(&mut self, title: &str, translation: &str) {
        self.titles.insert(title.to_string(), translation.to_string());
    }

    pub fn translate(&self, name: &str) -> String {
        if let Some(n) = self.names.get(name) {
            return n.clone();
        }
        let Some(rest) = name.strip_prefix('<') else { return name.to_string() };
        let Some(end) = rest.find('>') else { return name.to_string() };
        let (title, tail) = (&rest[..end], rest[end + 1..].trim_start());
        let title = self.titles.get(title).map(String::as_str).unwrap_or(title);
        let tail = self.suffixes.get(tail).map(String::as_str).unwrap_or(tail);
        format!("<{title}> {tail}")
    }
}

#[derive(Default)]
pub struct NpcRegistry {
    pub npcs: Vec<Npc>,
    by_cid: HashMap<u32, usize>,
}

impl NpcRegistry {
    pub async fn load(db: &Databases, translator: &Translator) -> anyhow::Result<Self> {
        let rows = db.world.rows("SELECT id, tid, name, pos_x, pos_y, pos_z, degree_x, degree_y, raw FROM npc_spawn", ()).await?;
        let npcs: Vec<Npc> = rows
            .iter()
            .map(|r| {
                let id: u32 = col(r, "id");
                Npc {
                    cid: NPC_CID_BASE + id,
                    tid: col(r, "tid"),
                    name: translator.translate(&col::<String>(r, "name")),
                    pos: Vec3::new(col::<f64>(r, "pos_x") as f32, col::<f64>(r, "pos_y") as f32, col::<f64>(r, "pos_z") as f32),
                    degree_x: col(r, "degree_x"),
                    degree_y: col(r, "degree_y"),
                    events: parse_events(&col::<String>(r, "raw")),
                }
            })
            .collect();
        info!("{} PNJ chargé(s)", npcs.len());
        Ok(Self::from_npcs(npcs))
    }

    pub fn from_npcs(npcs: Vec<Npc>) -> Self {
        let by_cid = npcs.iter().enumerate().map(|(i, n)| (n.cid, i)).collect();
        NpcRegistry { npcs, by_cid }
    }

    pub fn get(&self, cid: u32) -> Option<&Npc> {
        self.by_cid.get(&cid).map(|&i| &self.npcs[i])
    }

    /// PNJ dont la distance horizontale (X/Z) à `pos` est inférieure à `radius`.
    pub fn near(&self, pos: Vec3, radius: f64) -> impl Iterator<Item = &Npc> {
        let r2 = radius * radius;
        self.npcs.iter().filter(move |n| {
            let dx = (n.pos.x - pos.x) as f64;
            let dz = (n.pos.z - pos.z) as f64;
            dx * dx + dz * dz <= r2
        })
    }

    /// Paquet d'apparition d'un PNJ (vue optimisée, opération 1) avec le nom à afficher : `npc.name` dans
    /// la langue du serveur, ou le nom localisé pour le joueur (voir `locale.rs`).
    pub fn view_packet_named(npc: &Npc, name: &str) -> Vec<u8> {
        let clamp16 = |v: f64| v.round().clamp(-32768.0, 32767.0) as i64;
        let vs = &mini::OPT_VIEW_CHAR;
        let view = build_mini(
            vs,
            &Fields::new(vs)
                .with(0, npc.cid)
                .with(1, npc.tid)
                .with(2, archcore_net::text::encode(name))
                .with(6, 0)
                .with(7, 0)
                .with(8, npc.pos)
                .with(9, npc.pos)
                .with(12, clamp16(npc.degree_x))
                .with(13, clamp16(npc.degree_y)),
        );
        let events = event_block(npc);
        let ov = &schema::OPT_VIEW;
        let fields = Fields::new(ov).with(0, 1).with(1, view);
        let fields = if events.is_empty() { fields } else { fields.with(4, events) };
        build_packet(ov, &fields)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use archcore_net::{decode_mini, decode_packet, parse_header};

    /// Section npc.ini de <Merchant> Darryl (Anchorville), telle qu'importée dans npc_spawn.raw.
    pub const DARRYL_RAW: &str = r#"[["ModuleData1","AgpmCharacter"],["Name","<Merchant> Darryl"],["TID","584"],["ModuleData2","ApmEventManager"],
        ["Function","21"],["EventID","1073741841"],["CondStart","0"],["CondEnd","0"],["NPCDialogStart","0"],["Template","1"],["NPCDialogEnd","0"],
        ["Function","9"],["EventID","1073741842"],["NPCTradeStart","0"],["Template","104"],["NPCTradeEnd","0"],
        ["Function","27"],["EventID","1073741843"],["QuestGroupID","1"],["QuestEnd","0"],["EnumEnd","0"]]"#;

    pub fn darryl() -> Npc {
        Npc {
            cid: NPC_CID_BASE + 3,
            tid: 584,
            name: "<Marchand> Darryl".into(),
            pos: Vec3::new(-479097.9, 4753.4, -66021.9),
            events: parse_events(DARRYL_RAW),
            ..Default::default()
        }
    }

    #[test]
    fn lecture_des_fonctions_de_npc_ini() {
        let ev = parse_events(DARRYL_RAW);
        let f: Vec<i64> = ev.iter().map(|e| e.function).collect();
        assert_eq!(f, vec![21, 9, 27]);
        assert_eq!(ev[0].event_id, 0x4000_0011);
        assert_eq!(ev[1].param("Template"), Some("104"));
        assert_eq!(ev[2].param("QuestGroupID"), Some("1"));
    }

    #[test]
    fn la_vue_contient_les_fonctions() {
        let npc = darryl();
        let view = decode_packet(&schema::OPT_VIEW, &NpcRegistry::view_packet_named(&npc, &npc.name)).unwrap();
        let block = view.bytes(4).unwrap().to_vec();
        let mut found = Vec::new();
        let mut off = 0;
        while off < block.len() {
            let h = parse_header(&block[off..]).unwrap();
            let p = decode_packet(&schema::EVENT_MANAGER, &block[off..off + h.size as usize]).unwrap();
            let base = decode_mini(&mini::EVENT_BASE, p.bytes(0).unwrap()).unwrap();
            let data = decode_mini(&mini::EVENT_DATA_INT32, p.bytes(1).unwrap()).unwrap();
            found.push((base.int(0).unwrap(), base.int(1).unwrap() as u32, base.int(3).unwrap(), data.int(0).unwrap()));
            off += h.size as usize;
        }
        assert_eq!(found, vec![(3, npc.cid, 21, 1), (3, npc.cid, 9, 104), (3, npc.cid, 27, 1)]);
    }

    #[test]
    fn traduction_des_titres() {
        let mut t = Translator::default();
        t.add_title("Merchant", "Marchand");
        assert_eq!(t.translate("<Merchant> Darryl"), "<Marchand> Darryl");
        assert_eq!(t.translate("Darryl"), "Darryl");
        let view = decode_packet(&schema::OPT_VIEW, &NpcRegistry::view_packet_named(&darryl(), "<Händler> Darryl")).unwrap();
        let mini = decode_mini(&mini::OPT_VIEW_CHAR, view.bytes(1).unwrap()).unwrap();
        assert_eq!(mini.text(2).as_deref(), Some("<Händler> Darryl"));
    }
}
