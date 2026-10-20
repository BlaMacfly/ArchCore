//! Compétences côté moteur : livre (`K`, `/competences`), instructeurs, barre de raccourcis (touches
//! 1 à =) et lancement. Protocole : `archrust_netclient::skills` ; fenêtres : `hud_skills.rs`.
//!
//! * Données du client : noms et TID dans `SKILLTEMPLATE.ini`, niveaux dans `SKILL_CONST.txt` (classes
//!   admises `require_class_*`, niveau et points exigés, prix d'apprentissage et d'amélioration).
//! * Un instructeur enseigne les compétences dont le niveau 1 admet la classe de sa vue (race et
//!   classe de la fonction 25), comme la règle du serveur.
//! * Lancement : sur la cible choisie, sinon sur soi ; le serveur mène la marche jusqu'à portée et
//!   annonce le lancement (op 7), d'où vient le délai de relance affiché.
//! * Barre : le serveur ne garde pas son contenu (module UI_STATUS absent d'ArchCore), elle est
//!   enregistrée par personnage dans `saved-variables/raccourcis-<personnage>.txt`.

use super::App;
use crate::assets::Assets;
use crate::hud::{BookEntry, BookView, HudState, Shortcut, SkillUi, SlotView, TrainerEntry, TrainerView};
use crate::net::NetLink;
use archrust_netclient::items::ItemCommand;
use archrust_netclient::npc::function;
use archrust_netclient::skills::{learn_result, SkillCommand, SkillEvent};
use archrust_netclient::Command;
use glam::Vec3;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use winit::keyboard::KeyCode;

/// « Normal Attack » : connue de tous, pas dans le livre.
const NORMAL_ATTACK: i32 = 222;
/// Colonnes « require_class_* » de SKILL_CONST, dans l'ordre.
const CLASS_COLUMNS: [&str; 13] =
    ["knight", "archer", "wizard", "berserker", "hunter", "sorcerer", "swashbuckler", "ranger", "elementaler", "slayer", "obiter", "summoner", "scion"];
/// Distance à laquelle l'instructeur reste ouvert.
const CLOSE_RANGE: f32 = 2200.0;

/// Colonne de classe (race 1 humain, 2 orc, 3 elfe de lune, 4 scion ; classe 1 guerrier, 2 archer,
/// 4 mage).
fn class_column(race: i32, class: i32) -> Option<usize> {
    let col = match (race, class) {
        (1, 1) => "knight",
        (1, 2) => "archer",
        (1, 4) => "wizard",
        (2, 1) => "berserker",
        (2, 2) => "hunter",
        (2, 4) => "sorcerer",
        (3, 1) => "swashbuckler",
        (3, 2) => "ranger",
        (3, 4) => "elementaler",
        (4, 1) => "slayer",
        (4, 2) => "obiter",
        (4, 4) => "summoner",
        (4, _) => "scion",
        _ => return None,
    };
    CLASS_COLUMNS.iter().position(|c| *c == col)
}

/// Un niveau de compétence (`SKILL_CONST.txt`).
#[derive(Debug, Clone, Default)]
struct Level {
    classes: u16,
    require_level: i32,
    require_point: i32,
    cost: i64,
    upgrade_cost: i64,
}

#[derive(Debug, Clone, Default)]
struct SkillData {
    name: String,
    /// Niveaux 1, 2… dans l'ordre.
    levels: Vec<Level>,
}

#[derive(Default)]
struct SkillTables {
    by_tid: HashMap<i32, SkillData>,
}

fn load_tables(assets: &Assets) -> SkillTables {
    // SKILL_CONST.txt : une ligne par niveau, le nom sur la première.
    let text = assets
        .read_file("ini/SKILL_CONST.txt")
        .map(|raw| {
            let plain = archrust_data::ini::decrypt_ini(&raw);
            if plain.starts_with("name") { plain } else { String::from_utf8_lossy(&raw).into_owned() }
        })
        .unwrap_or_default();
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().unwrap_or("").split('\t').map(str::trim).collect();
    let col = |n: &str| header.iter().position(|h| h.eq_ignore_ascii_case(n));
    let class_cols: Vec<Option<usize>> = CLASS_COLUMNS.iter().map(|c| col(&format!("require_class_{c}"))).collect();
    let (c_level, c_point, c_cost, c_up) = (col("require_level"), col("require_point"), col("skill_cost"), col("skill_upgrade_cost"));
    let mut by_name: HashMap<String, Vec<Level>> = HashMap::new();
    let mut current: Option<String> = None;
    for line in lines {
        let f: Vec<&str> = line.split('\t').collect();
        let get = |c: Option<usize>| c.and_then(|i| f.get(i)).map(|s| s.trim()).unwrap_or("");
        let num = |c: Option<usize>| get(c).parse::<f64>().unwrap_or(0.0);
        let name = f.first().map(|s| s.trim()).unwrap_or("");
        if !name.is_empty() {
            current = Some(name.to_string());
        }
        let Some(n) = &current else { continue };
        if get(col("skill_level")).is_empty() {
            continue;
        }
        let mut classes = 0u16;
        for (i, c) in class_cols.iter().enumerate() {
            if num(*c) != 0.0 {
                classes |= 1 << i;
            }
        }
        by_name.entry(n.clone()).or_default().push(Level {
            classes,
            require_level: num(c_level) as i32,
            require_point: num(c_point) as i32,
            cost: num(c_cost) as i64,
            upgrade_cost: num(c_up) as i64,
        });
    }
    // SKILLTEMPLATE.ini : TID → nom.
    let mut tables = SkillTables::default();
    if let Ok(t) = assets.table("ini", "SKILLTEMPLATE.ini") {
        let name_col = t.section("").and_then(|h| h.entries.iter().find(|(_, v)| v.eq_ignore_ascii_case("Name")).map(|(k, _)| k.clone()));
        for s in &t.sections {
            let (Ok(tid), Some(name)) = (s.name.parse::<i32>(), name_col.as_deref().and_then(|c| s.get(c))) else { continue };
            let name = name.trim().to_string();
            let levels = by_name.get(&name).cloned().unwrap_or_default();
            tables.by_tid.insert(tid, SkillData { name, levels });
        }
    }
    log::info!("compétences : {} modèle(s), {} décrite(s) par SKILL_CONST", tables.by_tid.len(), by_name.len());
    tables
}

#[derive(Default)]
pub struct SkillsGlue {
    tables: Option<SkillTables>,
    /// Compétences du joueur : identifiant → (TID, niveau).
    known: HashMap<i32, (i32, i32)>,
    pub show_book: bool,
    book_first: usize,
    /// Instructeur ouvert et race/classe enseignées.
    trainer: Option<(i32, (i32, i32))>,
    trainer_first: usize,
    trainer_selected: Option<i32>,
    /// Fin du délai de relance par identifiant (et durée totale).
    cooldowns: HashMap<i32, (Instant, Duration)>,
    slots: [Option<Shortcut>; 12],
    /// Personnage dont la barre est chargée.
    slots_owner: Option<String>,
    /// Demandes des fenêtres, traitées à l'image suivante (la passe egui tient le GPU emprunté).
    pending: Vec<SkillUi>,
}

impl SkillsGlue {
    fn tables(&mut self, assets: &Assets) -> &SkillTables {
        self.tables.get_or_insert_with(|| load_tables(assets))
    }

    fn name(&mut self, assets: &Assets, tid: i32) -> String {
        self.tables(assets).by_tid.get(&tid).map(|d| d.name.clone()).unwrap_or_else(|| format!("Compétence {tid}"))
    }

    fn id_of(&self, tid: i32) -> Option<i32> {
        self.known.iter().find(|(_, (t, _))| *t == tid).map(|(id, _)| *id)
    }

    /// Secondes de relance restantes.
    fn left_s(&self, id: i32) -> u32 {
        self.cooldowns.get(&id).map(|(end, _)| end.saturating_duration_since(Instant::now()).as_secs_f32().ceil() as u32).unwrap_or(0)
    }

    fn cooldown(&self, id: i32) -> f32 {
        match self.cooldowns.get(&id) {
            Some((end, total)) if *end > Instant::now() && !total.is_zero() => (*end - Instant::now()).as_secs_f32() / total.as_secs_f32(),
            _ => 0.0,
        }
    }
}

fn slots_path(dir: &Path, name: &str) -> PathBuf {
    let safe: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    dir.join(format!("raccourcis-{safe}.txt"))
}

/// Barre enregistrée : une ligne « case sorte TID » (sorte : compétence ou objet).
fn load_slots(dir: &Path, name: &str) -> [Option<Shortcut>; 12] {
    let mut slots = [None; 12];
    for line in std::fs::read_to_string(slots_path(dir, name)).unwrap_or_default().lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if let [slot, kind, tid] = f.as_slice() {
            if let (Ok(slot), Ok(tid)) = (slot.parse::<usize>(), tid.parse::<i32>()) {
                if slot < 12 {
                    slots[slot] = match *kind {
                        "competence" => Some(Shortcut::Skill(tid)),
                        "objet" => Some(Shortcut::Item(tid)),
                        _ => None,
                    };
                }
            }
        }
    }
    slots
}

fn save_slots(dir: &Path, name: &str, slots: &[Option<Shortcut>; 12]) {
    let mut text = String::from("# Barre de raccourcis d'ArchRust : case (0 à 11) sorte TID\n");
    for (i, s) in slots.iter().enumerate() {
        match s {
            Some(Shortcut::Skill(t)) => text.push_str(&format!("{i} competence {t}\n")),
            Some(Shortcut::Item(t)) => text.push_str(&format!("{i} objet {t}\n")),
            None => {}
        }
    }
    let _ = std::fs::create_dir_all(dir);
    if let Err(e) = std::fs::write(slots_path(dir, name), text) {
        log::warn!("barre de raccourcis non enregistrée : {e}");
    }
}

/// Données des fenêtres pour cette image.
#[derive(Default)]
pub struct SkillViews {
    book: Vec<BookEntry>,
    trainer: Option<(String, Vec<TrainerEntry>)>,
    slots: [SlotView; 12],
    points: Option<i32>,
}

/// Fenêtres et barre ; les demandes attendent `App::skills_frame`.
pub fn panel(ctx: &egui::Context, glue: &mut SkillsGlue, views: &SkillViews, hud: &mut HudState, assets: &Assets) {
    let mut out = Vec::new();
    crate::hud::draw_quickbar(ctx, hud, assets, &views.slots, &mut out);
    if glue.show_book {
        crate::hud::draw_skill_book(ctx, hud, assets, BookView { entries: &views.book, points: views.points, first: &mut glue.book_first }, &mut out);
    }
    if let Some((title, entries)) = &views.trainer {
        let view = TrainerView { title: title.clone(), entries, selected: glue.trainer_selected, points: views.points, first: &mut glue.trainer_first };
        crate::hud::draw_trainer(ctx, hud, assets, view, &mut out);
    }
    glue.pending.extend(out);
}

fn send(net: Option<&NetLink>, cmd: SkillCommand) {
    if let Some(net) = net {
        net.send(Command::Skill(cmd));
    }
}

impl App {
    /// Compétences du joueur et réponses des instructeurs.
    pub(super) fn on_skill_event(&mut self, ev: SkillEvent) {
        let assets = self.assets.clone();
        let me = self.player.as_ref().map(|p| p.cid).unwrap_or(0);
        match ev {
            SkillEvent::Known { owner, id, tid, level } => {
                if owner == 0 || owner == me || me == 0 {
                    self.skills.known.insert(id, (tid, level));
                }
            }
            SkillEvent::Level { id, level } => {
                if let Some(tid) = self.skills.known.get_mut(&id).map(|s| {
                    s.1 = level;
                    s.0
                }) {
                    let name = self.skills.name(&assets, tid);
                    self.ui.push_chat(format!("[compétence] {name} passe au niveau {level}."));
                }
            }
            SkillEvent::Cast { caster, id, tid, cast_ms, recast_ms, .. } if caster == me => {
                let total = Duration::from_millis(cast_ms as u64 + recast_ms as u64);
                self.skills.cooldowns.insert(id, (Instant::now() + total, total));
                let name = self.skills.name(&assets, tid);
                self.ui.push_chat(format!("[compétence] {name}."));
            }
            SkillEvent::Cast { .. } => {}
            SkillEvent::MasterOpened { npc } => {
                let teach = self.npc_teach(npc);
                match teach {
                    Some(t) => {
                        self.skills.trainer = Some((npc, t));
                        self.skills.trainer_first = 0;
                        self.skills.trainer_selected = None;
                    }
                    None => self.ui.push_chat("[compétence] Cet instructeur n'enseigne rien que le client connaisse."),
                }
            }
            SkillEvent::Learned { tid, result } => {
                let name = self.skills.name(&assets, tid);
                self.ui.push_chat(format!(
                    "[compétence] {}",
                    match result {
                        learn_result::SUCCESS => format!("Vous apprenez {name} !"),
                        learn_result::ALREADY_LEARNED => format!("Vous connaissez déjà {name}."),
                        learn_result::NOT_ENOUGH_POINTS => format!("Pas assez de points de compétence pour {name}."),
                        learn_result::LOW_LEVEL => format!("Niveau trop bas pour {name}."),
                        learn_result::WRONG_CLASS => format!("{name} n'est pas pour votre classe."),
                        _ => format!("Impossible d'apprendre {name} (or insuffisant ?)."),
                    }
                ));
            }
            SkillEvent::Upgraded { .. } => {}
        }
    }

    /// Race et classe enseignées par un PNJ (fonction 25 de sa vue).
    fn npc_teach(&self, npc: i32) -> Option<(i32, i32)> {
        self.npc_functions(npc).iter().find(|f| f.function == function::SKILL_MASTER).and_then(|f| f.teach)
    }

    /// Lance la compétence `tid` sur la cible choisie, sinon sur soi.
    pub(super) fn skills_cast(&mut self, tid: i32) {
        let assets = self.assets.clone();
        let name = self.skills.name(&assets, tid);
        let Some(id) = self.skills.id_of(tid) else {
            self.ui.push_chat(format!("[compétence] Vous ne connaissez pas {name}."));
            return;
        };
        if self.skills.cooldown(id) > 0.0 {
            let left = self.skills.cooldowns.get(&id).map(|(e, _)| (*e - Instant::now()).as_secs()).unwrap_or(0);
            self.ui.push_chat(format!("[compétence] {name} sera prête dans {left} s."));
            return;
        }
        let Some(me) = self.player.as_ref().map(|p| p.cid) else { return };
        let target = self.addons.target_cid().unwrap_or(me);
        // Le serveur mène la marche jusqu'à portée : pas d'arrêt du client, qui interromprait.
        self.walking = false;
        send(self.net.as_ref(), SkillCommand::Cast { id, target });
    }

    fn skills_use_slot(&mut self, slot: usize) {
        match self.skills.slots.get(slot).copied().flatten() {
            Some(Shortcut::Skill(tid)) if tid == NORMAL_ATTACK => {
                if let Some(cid) = self.addons.target_cid() {
                    self.combat_attack(cid);
                }
            }
            Some(Shortcut::Skill(tid)) => self.skills_cast(tid),
            Some(Shortcut::Item(tid)) => {
                let item = self.items.values().filter(|i| i.template_id == Some(tid) && i.status == Some(2)).min_by_key(|i| i.id).map(|i| i.id);
                match (item, &self.net) {
                    (Some(iid), Some(net)) => net.send(Command::Item(ItemCommand::Use(iid))),
                    (None, _) => self.ui.push_chat("[objet] Plus aucun exemplaire dans l'inventaire."),
                    _ => {}
                }
            }
            None => {}
        }
    }

    /// Touches 1 à = : cases de la barre (potions sur 1 et 2).
    pub(super) fn skills_key(&mut self, code: KeyCode) -> bool {
        let slot = match code {
            KeyCode::Digit1 => 0,
            KeyCode::Digit2 => 1,
            KeyCode::Digit3 => 2,
            KeyCode::Digit4 => 3,
            KeyCode::Digit5 => 4,
            KeyCode::Digit6 => 5,
            KeyCode::Digit7 => 6,
            KeyCode::Digit8 => 7,
            KeyCode::Digit9 => 8,
            KeyCode::Digit0 => 9,
            KeyCode::Minus => 10,
            KeyCode::Equal => 11,
            _ => return false,
        };
        self.skills_use_slot(slot);
        true
    }

    /// Demandes des fenêtres et de la barre.
    fn skills_apply(&mut self, requests: Vec<SkillUi>) {
        let mut changed = false;
        for r in requests {
            match r {
                SkillUi::CloseBook => self.skills.show_book = false,
                SkillUi::CloseTrainer => self.skills.trainer = None,
                SkillUi::Cast(tid) => self.skills_cast(tid),
                SkillUi::SelectTrainer(tid) => self.skills.trainer_selected = Some(tid),
                SkillUi::Learn(tid) => {
                    if let Some((npc, _)) = self.skills.trainer {
                        send(self.net.as_ref(), SkillCommand::Learn { npc, tid });
                    }
                }
                SkillUi::Upgrade(tid) => {
                    if let (Some((npc, _)), Some(id)) = (self.skills.trainer, self.skills.id_of(tid)) {
                        send(self.net.as_ref(), SkillCommand::Upgrade { npc, id });
                    }
                }
                SkillUi::SlotSet { slot, shortcut } => {
                    self.skills.slots[slot] = Some(shortcut);
                    changed = true;
                }
                SkillUi::SlotSwap(a, b) => {
                    self.skills.slots.swap(a, b);
                    changed = true;
                }
                SkillUi::SlotClear(slot) => {
                    self.skills.slots[slot] = None;
                    changed = true;
                }
                SkillUi::SlotUse(slot) => self.skills_use_slot(slot),
            }
        }
        if changed {
            if let Some(name) = self.skills.slots_owner.clone() {
                save_slots(&self.config.saved_dir, &name, &self.skills.slots);
            }
        }
    }

    /// À chaque image : demandes des fenêtres, barre du personnage chargée, instructeur fermé en
    /// s'éloignant.
    pub(super) fn skills_frame(&mut self) {
        let requests = std::mem::take(&mut self.skills.pending);
        if !requests.is_empty() {
            self.skills_apply(requests);
        }
        let Some((name, me)) = self.player.as_ref().filter(|p| p.cid != 0 && !p.name.is_empty()).map(|p| (p.name.clone(), p.position)) else { return };
        if self.skills.slots_owner.as_deref() != Some(name.as_str()) {
            self.skills.slots = load_slots(&self.config.saved_dir, &name);
            self.skills.slots_owner = Some(name);
        }
        if let Some((npc, _)) = self.skills.trainer {
            let far = self.entities.iter().find(|e| e.cid == npc).is_none_or(|e| Vec3::new(e.position.x - me.x, 0.0, e.position.z - me.z).length() > CLOSE_RANGE);
            if far {
                self.skills.trainer = None;
            }
        }
    }

    /// Données des fenêtres et de la barre.
    pub(super) fn skills_views(&mut self) -> SkillViews {
        let assets = self.assets.clone();
        let mut v = SkillViews { points: self.hud.stats.skill_points, ..Default::default() };
        let known: Vec<(i32, (i32, i32))> = self.skills.known.iter().map(|(id, s)| (*id, *s)).collect();
        if self.skills.show_book {
            for (id, (tid, level)) in &known {
                if *tid == NORMAL_ATTACK {
                    continue;
                }
                let name = self.skills.name(&assets, *tid);
                v.book.push(BookEntry { tid: *tid, name, level: *level, cooldown: self.skills.cooldown(*id), left_s: self.skills.left_s(*id) });
            }
            v.book.sort_by(|a, b| a.name.cmp(&b.name));
        }
        if let Some((npc, (race, class))) = self.skills.trainer {
            let col = class_column(race, class);
            let (level, money, points) = (self.hud.stats.level.unwrap_or(1), self.hud.stats.money.unwrap_or(0), self.hud.stats.skill_points.unwrap_or(0));
            let mut seen_names = std::collections::HashSet::new();
            let mut tids: Vec<(i32, SkillData)> = self
                .skills
                .tables(&assets)
                .by_tid
                .iter()
                // Compétences ordinaires de la classe : niveau requis et prix renseignés (les
                // héroïques et spéciales n'en ont pas) ; les variantes « <ASY>… » sont écartées.
                .filter(|(tid, d)| {
                    **tid != NORMAL_ATTACK
                        && !d.name.starts_with('<')
                        && d.levels.first().is_some_and(|l| l.require_level > 0 && l.cost > 0 && col.is_some_and(|c| l.classes & (1 << c) != 0))
                })
                .map(|(t, d)| (*t, d.clone()))
                .collect();
            tids.sort_by_key(|(t, _)| *t);
            let mut entries = Vec::new();
            for (tid, d) in tids {
                if !seen_names.insert(d.name.clone()) {
                    continue;
                }
                let owned = known.iter().find(|(_, (t, _))| *t == tid).map(|(_, (_, l))| *l).unwrap_or(0);
                let next = d.levels.get(owned as usize);
                let cost = next.map(|l| if owned == 0 { l.cost } else { l.upgrade_cost }).unwrap_or(0);
                let require_level = next.map(|l| l.require_level).unwrap_or(0);
                let possible = next.is_some_and(|l| level >= l.require_level && points >= l.require_point && money >= cost);
                entries.push(TrainerEntry { tid, name: d.name.clone(), level: owned, max_level: d.levels.len() as i32, require_level, cost, possible });
            }
            entries.sort_by_key(|e| (e.require_level, e.tid));
            let npc_name = self.entities.iter().find(|e| e.cid == npc).map(|e| e.name.clone()).unwrap_or_default();
            v.trainer = Some((npc_name, entries));
        }
        for (i, s) in self.skills.slots.iter().enumerate() {
            let Some(sc) = s else { continue };
            let (count, cooldown, left_s) = match sc {
                Shortcut::Skill(t) => {
                    let id = self.skills.id_of(*t);
                    (None, id.map(|id| self.skills.cooldown(id)).unwrap_or(0.0), id.map(|id| self.skills.left_s(id)).unwrap_or(0))
                }
                Shortcut::Item(t) => {
                    let n: i32 = self.items.values().filter(|it| it.template_id == Some(*t) && it.status == Some(2)).map(|it| it.count.unwrap_or(1)).sum();
                    (Some(n), 0.0, 0)
                }
            };
            v.slots[i] = SlotView { shortcut: Some(*sc), count, cooldown, left_s };
        }
        v
    }

    /// `/competences` (`/skills`), `/lancer nom`.
    pub(super) fn skills_slash(&mut self, cmd: &str, arg: &str) -> bool {
        match cmd {
            "/competences" | "/compétences" | "/skills" => self.skills.show_book = !self.skills.show_book,
            "/lancer" | "/cast" => {
                let wanted = arg.trim().to_lowercase();
                let assets = self.assets.clone();
                let known: Vec<i32> = self.skills.known.values().map(|(t, _)| *t).collect();
                let pick = known.into_iter().find(|t| t.to_string() == wanted || self.skills.name(&assets, *t).to_lowercase().contains(&wanted));
                match pick {
                    Some(tid) if !wanted.is_empty() => self.skills_cast(tid),
                    _ => self.ui.push_chat("[compétence] usage : /lancer nom de la compétence"),
                }
            }
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_et_barre() {
        assert_eq!(class_column(1, 1), Some(0));
        assert_eq!(class_column(2, 4), Some(5));
        let dir = std::env::temp_dir().join("archrust-test-raccourcis");
        let mut slots = [None; 12];
        slots[2] = Some(Shortcut::Skill(4));
        slots[0] = Some(Shortcut::Item(190));
        save_slots(&dir, "Gm Test", &slots);
        assert_eq!(load_slots(&dir, "Gm Test"), slots);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
