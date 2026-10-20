//! Combat côté moteur : attaque de la cible, coups et PV, animations, mort et résurrection au
//! village, objets au sol et ramassage, niveau. Protocole : `archrust_netclient::combat`.
//!
//! Commandes et touches :
//! * clic droit (sans glisser) sur un monstre : il devient la cible et le joueur l'attaque ; clic
//!   gauche sur la cible déjà choisie : attaque ; `F` : attaque la cible ; `/attaquer` (`/attack`) ;
//! * clic (gauche ou droit) sur un objet au sol, `Espace` (« Collect item » d'Archonia, DIK 57,
//!   `ItemAutoPickup`) pour l'objet le plus proche, `/ramasser` (`/loot`, `/pickup`) ;
//! * mort : fenêtre `UI_Resurrection` (« Ressusciter au village »), ou `/ressusciter` (`/release`).
//!
//! Le serveur porte les coups : le client n'envoie qu'une demande d'attaque, et un déplacement
//! demandé par le joueur l'interrompt. Les animations suivent `charactertemplateanimation.ini`
//! (types 3 attaque, 4 coup reçu, 5 mort, voir `animation.rs`).

use super::App;
use crate::animation::{AnimType, Animator};
use crate::assets::Assets;
use crate::hud::HudState;
use crate::net::NetLink;
use crate::render::{DrawItem, Mesh};
use crate::scene::{box_mesh, build_clump_meshes, item_name_for_tid, patched_model_meshes, turn_towards};
use crate::ui::UiState;
use archrust_netclient::combat::{outcome, CombatCommand, GroundItem, ATTACKABLE_TYPE};
use archrust_netclient::{Command, WorldEvent};
use archrust_protocol::schema::action_status;
use egui::{Align2, Color32, FontId, Pos2, Vec2};
use glam::{Mat4, Vec3};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::keyboard::KeyCode;

/// Distance de ramassage (le serveur accepte 2000 unités, depuis la destination de la marche).
const PICKUP_RANGE: f32 = 1800.0;
/// Durée d'affichage d'un chiffre de dégâts.
const FLOATER_LIFE: f32 = 1.6;
/// Délai avant de pouvoir redemander la résurrection ou le même ramassage.
const RETRY: Duration = Duration::from_secs(4);

/// Chiffre ou mot affiché au-dessus d'un personnage (dégâts, raté, niveau…).
struct Floater {
    cid: i32,
    text: String,
    color: Color32,
    big: bool,
    born: Instant,
}

/// Objet au sol et son modèle (`FIELD_DFF` de `ItemTemplateAll.ini`).
struct Loot {
    item: GroundItem,
    name: String,
    meshes: Vec<(Arc<Mesh>, Mat4)>,
    /// Hauteur du terrain sous l'objet (le serveur donne celle du monstre).
    ground_y: Option<f32>,
}

#[derive(Default)]
pub struct CombatGlue {
    /// Cible de l'attaque demandée au serveur.
    attacking: Option<i32>,
    pub player_dead: bool,
    /// Résurrection demandée (pas de nouvel envoi avant `RETRY`).
    resurrect_sent: Option<Instant>,
    /// Personnages morts (statut d'action 2), cadavres compris.
    dead: HashSet<i32>,
    /// Cadavres entrés dans la vue déjà morts : leur animateur prend la pose finale.
    corpses_in_view: HashSet<i32>,
    /// PV et maximum lus dans la vue d'un personnage, appliqués à l'entité une fois créée.
    view_stats: HashMap<i32, (Option<i32>, Option<i32>)>,
    ground: HashMap<i32, Loot>,
    floaters: Vec<Floater>,
    /// Objet vers lequel le joueur marche pour le ramasser.
    pickup: Option<i32>,
    /// Ramassages envoyés, en attente de la réponse.
    requested: HashMap<i32, Instant>,
    /// Position du curseur à l'appui du bouton droit (un clic sans glisser attaque).
    right_press: Option<(f64, f64)>,
    /// `CharacterType` par TID (`CharacterTemplatePublic.ini`).
    types: HashMap<i32, u32>,
}

/// Ce que la fenêtre egui dessine : textes projetés à l'écran (points egui).
#[derive(Default)]
pub struct CombatView {
    floaters: Vec<(Pos2, String, Color32, f32, f32)>,
    labels: Vec<(Pos2, String, Color32)>,
    dead: bool,
    can_resurrect: bool,
}

/// Type de modèle d'un TID (`CharacterType`), 0 s'il est inconnu.
fn character_type(assets: &Assets, tid: i32) -> u32 {
    let Ok(table) = assets.table("ini", "CharacterTemplatePublic.ini") else {
        return 0;
    };
    let column = table.section("").and_then(|h| {
        h.entries
            .iter()
            .find(|(_, v)| v.eq_ignore_ascii_case("CharacterType"))
            .map(|(k, _)| k.clone())
    });
    column
        .and_then(|c| table.section(&tid.to_string())?.get(&c)?.trim().parse::<i64>().ok())
        .unwrap_or(0) as u32
}

/// Modèle posé au sol d'un objet : colonne `FIELD_DFF` (sinon `BASE_DFF`), clump `d%07x.ecl`.
fn field_model(assets: &Assets, tid: i32) -> Option<String> {
    let table = assets.table("ini", "ItemTemplateAll.ini").ok()?;
    let header = table.section("")?;
    let item = table.section(&tid.to_string())?;
    ["FIELD_DFF", "BASE_DFF"].iter().find_map(|name| {
        let col = header.entries.iter().find(|(_, v)| v.eq_ignore_ascii_case(name))?.0.as_str();
        let dff = item.get(col)?.trim().parse::<u32>().ok()?;
        (dff > 0).then(|| format!("d{dff:07x}.ecl"))
    })
}

/// Maillages d'un modèle d'objet au sol (`character/<file>`). Certains (l'or, `d000010c.ecl`) portent
/// des couleurs de sommets pré-éclairées presque noires : elles sont ignorées, sinon l'objet serait
/// noir sous l'éclairage du moteur.
fn field_meshes(renderer: &mut crate::render::Renderer, assets: &Assets, file: &str) -> anyhow::Result<Vec<(Arc<Mesh>, Mat4)>> {
    if let Some(m) = patched_model_meshes(renderer, assets, &format!("character/{file}")) {
        return Ok(m);
    }
    let clump = assets.clump("character", file)?;
    let dark = |g: &archrust_data::rw::Geometry| {
        !g.colors.is_empty() && g.colors.iter().map(|c| c[0] as u32 + c[1] as u32 + c[2] as u32).sum::<u32>() / (g.colors.len() as u32 * 3) < 40
    };
    if !clump.geometries.iter().any(dark) {
        return Ok(build_clump_meshes(renderer, assets, &clump));
    }
    let mut lit = (*clump).clone();
    for g in lit.geometries.iter_mut().filter(|g| dark(g)) {
        g.colors.clear();
    }
    Ok(build_clump_meshes(renderer, assets, &Arc::new(lit)))
}

/// Libellé d'un objet au sol.
fn loot_name(assets: &Assets, item: &GroundItem) -> String {
    if item.is_gold() {
        return format!("{} Or", item.count);
    }
    let name = item_name_for_tid(assets, item.tid).unwrap_or_else(|| format!("Objet {}", item.tid));
    if item.count > 1 {
        format!("{name} × {}", item.count)
    } else {
        name
    }
}

/// Message de discussion d'un coup qui concerne le joueur, sinon `None`.
fn hit_line(by_me: bool, on_me: bool, actor: &str, target: &str, result: Option<i8>, damage: Option<i32>, hp: Option<i32>) -> Option<String> {
    let rest = hp.map(|h| format!(" (reste {h} PV)")).unwrap_or_default();
    let d = damage.unwrap_or(0);
    let text = match (by_me, on_me, result) {
        (true, _, Some(outcome::SUCCESS)) => format!("Vous frappez {target} : {d} dégât(s){rest}."),
        (true, _, Some(outcome::CRITICAL)) => format!("Coup critique sur {target} : {d} dégât(s){rest} !"),
        (true, _, Some(outcome::MISS)) => format!("Vous manquez {target}."),
        (true, _, Some(outcome::EVADE)) => format!("{target} esquive votre coup."),
        (true, _, Some(outcome::BLOCK)) => format!("{target} bloque votre coup."),
        (_, true, Some(outcome::SUCCESS)) => format!("{actor} vous frappe : {d} dégât(s){rest}."),
        (_, true, Some(outcome::CRITICAL)) => format!("{actor} vous porte un coup critique : {d} dégât(s){rest} !"),
        (_, true, Some(outcome::MISS)) => format!("{actor} vous manque."),
        (_, true, Some(outcome::EVADE)) => format!("Vous esquivez le coup de {actor}."),
        (_, true, Some(outcome::BLOCK)) => format!("Vous bloquez le coup de {actor}."),
        (true, _, other) | (_, true, other) => format!("{actor} → {target} : résultat {other:?}{rest}."),
        _ => return None,
    };
    Some(format!("[combat] {text}"))
}

/// Texte flottant d'un coup : chiffre des dégâts ou mot du résultat, et s'il est grossi (critique).
fn floater_text(result: Option<i8>, damage: Option<i32>) -> Option<(String, bool)> {
    Some(match result {
        Some(outcome::SUCCESS) => (format!("-{}", damage?), false),
        Some(outcome::CRITICAL) => (format!("-{} !", damage?), true),
        Some(outcome::MISS) => ("Raté".into(), false),
        Some(outcome::EVADE) => ("Esquive".into(), false),
        Some(outcome::BLOCK) => ("Bloqué".into(), false),
        _ => return None,
    })
}

impl App {
    fn combat_me(&self) -> i32 {
        self.player.as_ref().map_or(0, |p| p.cid)
    }

    fn combat_name(&self, cid: i32) -> String {
        self.player
            .as_ref()
            .filter(|p| p.cid == cid)
            .map(|p| p.name.clone())
            .or_else(|| self.entities.iter().find(|e| e.cid == cid).map(|e| e.name.clone()))
            .unwrap_or_else(|| format!("CID {cid}"))
    }

    fn combat_animator(&mut self, cid: i32) -> Option<&mut Animator> {
        if self.player.as_ref().is_some_and(|p| p.cid == cid) {
            return self.player_animator.as_mut().and_then(|(_, a)| a.as_mut());
        }
        self.animators.get_mut(&cid).and_then(|(_, a)| a.as_mut())
    }

    /// Vrai si le modèle porte les bits « attaquable » et « ciblable ».
    fn combat_attackable(&mut self, tid: i32) -> bool {
        let assets = &self.assets;
        let t = *self.combat.types.entry(tid).or_insert_with(|| character_type(assets, tid));
        t & ATTACKABLE_TYPE == ATTACKABLE_TYPE
    }

    fn combat_float(&mut self, cid: i32, text: String, color: Color32, big: bool) {
        self.combat.floaters.push(Floater { cid, text, color, big, born: Instant::now() });
        let excess = self.combat.floaters.len().saturating_sub(40);
        self.combat.floaters.drain(..excess);
    }

    /// Relevé des événements avant leur traitement par `on_world_event` (statuts de mort, vues,
    /// téléportation, objets retirés du sol).
    pub(super) fn combat_observe(&mut self, ev: &WorldEvent) {
        let me = self.combat_me();
        match ev {
            WorldEvent::Connected => self.combat_reset(),
            WorldEvent::Disconnected(_) => self.combat_reset(),
            WorldEvent::CharacterSeen(c) => {
                let s = c.stats.as_ref();
                self.combat.view_stats.insert(c.cid, (s.and_then(|s| s.hp), s.and_then(|s| s.max_hp)));
                if c.action_status == Some(action_status::DEAD) {
                    self.combat.dead.insert(c.cid);
                    self.combat.corpses_in_view.insert(c.cid);
                } else {
                    self.combat.dead.remove(&c.cid);
                }
            }
            WorldEvent::CharacterUpdated { cid, stats, action_status: status, .. } => {
                if *cid == me {
                    if let Some(s) = stats {
                        if s.exp.is_some() && s.exp != self.hud.stats.exp {
                            self.addons.fire("PLAYER_XP_UPDATE", "player");
                        }
                        if s.money.is_some() && s.money != self.hud.stats.money {
                            self.addons.fire_event("PLAYER_MONEY", &[]);
                        }
                    }
                }
                if let Some(status) = *status {
                    self.combat_status(*cid, status);
                }
            }
            WorldEvent::CharacterRemoved { cid, .. } if *cid != me => {
                self.combat.dead.remove(cid);
                self.combat.corpses_in_view.remove(cid);
                self.combat.view_stats.remove(cid);
                self.combat.floaters.retain(|f| f.cid != *cid);
                if self.combat.attacking == Some(*cid) {
                    self.combat.attacking = None;
                }
            }
            // Téléportation (résurrection au village) : la caméra suit le joueur.
            WorldEvent::CharacterMoved { cid, position, .. } if *cid == me => {
                if self.player.as_ref().is_some_and(|p| p.position.distance(*position) > 3000.0) {
                    self.camera.target = *position + Vec3::new(0.0, 100.0, 0.0);
                }
            }
            WorldEvent::ItemChanged(item) if item.operation == 1 => {
                if self.combat.ground.remove(&item.id).is_some() {
                    self.combat.requested.remove(&item.id);
                    if self.combat.pickup == Some(item.id) {
                        self.combat.pickup = None;
                    }
                    self.addons.fire("ARCHRUST_LOOT_REMOVED", item.id);
                }
            }
            _ => {}
        }
    }

    fn combat_reset(&mut self) {
        let types = std::mem::take(&mut self.combat.types);
        self.combat = CombatGlue { types, ..Default::default() };
    }

    /// Statut d'action reçu (AGPMCHARACTER op 1 champ 11) : mort ou retour à la vie.
    fn combat_status(&mut self, cid: i32, status: i8) {
        let me = self.combat_me();
        let dead = status == action_status::DEAD;
        if cid == me {
            if dead && !self.combat.player_dead {
                self.combat.player_dead = true;
                self.combat.attacking = None;
                self.combat.pickup = None;
                self.walking = false;
                if let Some(p) = self.player.as_mut() {
                    p.hp = Some(0);
                    p.destination = p.position;
                }
                self.hud.stats.hp = Some(0);
                self.ui.push_chat("[combat] Vous êtes mort. Ressuscitez au village.");
                self.hud.push_system("Vous êtes mort.");
                self.addons.fire_event("PLAYER_DEAD", &[]);
            } else if !dead && self.combat.player_dead {
                self.combat.player_dead = false;
                self.combat.resurrect_sent = None;
                self.ui.push_chat("[combat] Vous revenez à la vie au village.");
                self.addons.fire_event("PLAYER_ALIVE", &[]);
                self.addons.fire_event("PLAYER_UNGHOST", &[]);
            }
            return;
        }
        if dead {
            if self.combat.dead.insert(cid) {
                let name = self.combat_name(cid);
                if self.combat.attacking == Some(cid) || self.addons.target_cid() == Some(cid) {
                    self.ui.push_chat(format!("[combat] {name} est vaincu."));
                }
                if self.combat.attacking == Some(cid) {
                    self.combat.attacking = None;
                    self.addons.fire_event("PLAYER_REGEN_ENABLED", &[]);
                }
            }
            if let Some(e) = self.entities.iter_mut().find(|e| e.cid == cid) {
                e.hp = Some(0);
                e.destination = e.position;
            }
        } else {
            self.combat.dead.remove(&cid);
            self.combat.corpses_in_view.remove(&cid);
        }
    }

    /// Coup reçu sur le canal 0x41 : PV, messages, chiffres flottants, animations, AddOns.
    pub(super) fn on_combat_action(&mut self, ev: WorldEvent) {
        let WorldEvent::CharacterAction { cid, target_cid, result, factors, target_hp, damage, .. } = ev else {
            return;
        };
        let me = self.combat_me();
        let (by_me, on_me) = (cid == me, target_cid == me);
        let hp = factors.as_ref().and_then(|f| f.hp).or(target_hp);
        let previous = if on_me {
            self.player.as_ref().and_then(|p| p.hp).or(self.hud.stats.hp)
        } else {
            self.entities.iter().find(|e| e.cid == target_cid).and_then(|e| e.hp)
        };
        let damage = damage
            .filter(|d| *d > 0)
            .or_else(|| match (previous, hp) {
                (Some(a), Some(b)) if a > b => Some(a - b),
                _ => None,
            });
        if let Some(hp) = hp {
            if on_me {
                if let Some(p) = self.player.as_mut() {
                    p.hp = Some(hp);
                }
                self.hud.stats.hp = Some(hp);
            } else if let Some(e) = self.entities.iter_mut().find(|e| e.cid == target_cid) {
                e.hp = Some(hp);
            }
        }
        // L'attaquant se tourne vers sa cible et joue son attaque ; la cible touchée accuse le coup.
        let target_pos = if on_me {
            self.player.as_ref().map(|p| p.position)
        } else {
            self.entities.iter().find(|e| e.cid == target_cid).map(|e| e.position)
        };
        if let Some(to) = target_pos {
            let attacker = if by_me { self.player.as_mut() } else { self.entities.iter_mut().find(|e| e.cid == cid) };
            if let Some(a) = attacker {
                if let Some(turn) = turn_towards(a.position, to) {
                    a.turn_y = turn;
                }
            }
        }
        let assets = self.assets.clone();
        if let Some(a) = self.combat_animator(cid) {
            a.play_once(&assets, AnimType::Attack);
        }
        let touched = matches!(result, Some(outcome::SUCCESS | outcome::CRITICAL)) && damage.unwrap_or(0) > 0;
        if touched {
            if let Some(a) = self.combat_animator(target_cid) {
                a.play_once(&assets, AnimType::Hit);
            }
        }
        if let Some((text, big)) = floater_text(result, damage) {
            let color = if on_me {
                Color32::from_rgb(255, 80, 60)
            } else if by_me {
                Color32::from_rgb(255, 230, 120)
            } else {
                Color32::from_rgb(210, 210, 210)
            };
            self.combat_float(target_cid, text, color, big);
        }
        let (actor, target) = (self.combat_name(cid), self.combat_name(target_cid));
        if let Some(line) = hit_line(by_me, on_me, &actor, &target, result, damage, hp) {
            self.ui.push_chat(line);
        }
        // Riposte : sans cible, le joueur prend pour cible le monstre qui l'attaque.
        if on_me && self.addons.target_cid().is_none() && !self.combat.player_dead {
            self.addons.set_target(Some(cid));
        }
        if let Some(d) = damage {
            if touched {
                self.addons.combat(cid, target_cid, -(d as i64), hp.map(i64::from));
            }
        }
        log::debug!("action 0x41 : {actor} ({cid}) → {target} ({target_cid}), résultat {result:?}, {damage:?} dégât(s), PV {hp:?}");
    }

    /// Objets au sol, résultats de ramassage, montées de niveau.
    pub(super) fn on_combat_event(&mut self, ev: WorldEvent) {
        let me = self.combat_me();
        match ev {
            WorldEvent::GroundItem(item) => {
                let name = loot_name(&self.assets, &item);
                let ground_y = self.terrain.height_at(item.position.x, item.position.z);
                let meshes = match self.gpu.as_mut() {
                    Some(gpu) => {
                        let model = field_model(&self.assets, item.tid);
                        let loaded = model.as_deref().and_then(|file| match field_meshes(&mut gpu.renderer, &self.assets, file) {
                            Ok(m) if !m.is_empty() => Some(m),
                            Ok(_) => None,
                            Err(e) => {
                                log::debug!("butin TID {} (modèle {file}) : {e:#}", item.tid);
                                None
                            }
                        });
                        loaded.unwrap_or_else(|| {
                            let color = if item.is_gold() { [1.0, 0.85, 0.2, 1.0] } else { [0.8, 0.8, 0.9, 1.0] };
                            vec![(Arc::new(box_mesh(&gpu.renderer, Vec3::splat(12.0), color)), Mat4::from_translation(Vec3::Y * 12.0))]
                        })
                    }
                    None => Vec::new(),
                };
                if item.owner_cid == Some(me) {
                    self.ui.push_chat(format!("[butin] {name} tombe au sol."));
                }
                self.addons.fire("ARCHRUST_LOOT_SPAWNED", (item.iid, item.tid, item.count));
                self.combat.ground.insert(item.iid, Loot { item, name, meshes, ground_y });
            }
            WorldEvent::PickupResult { iid, success, tid, count } => {
                self.combat.requested.remove(&iid);
                let gold = tid == Some(archrust_netclient::combat::GOLD_TID);
                let name = self
                    .combat
                    .ground
                    .get(&iid)
                    .map(|l| l.name.clone())
                    .or_else(|| tid.and_then(|t| item_name_for_tid(&self.assets, t)))
                    .unwrap_or_else(|| "l'objet".into());
                if success {
                    let n = count.unwrap_or(1);
                    let line = if gold {
                        format!("Vous ramassez {n} pièce(s) d'or.")
                    } else {
                        format!("Vous ramassez {}.", item_name_for_tid(&self.assets, tid.unwrap_or(0)).map_or(name, |t| if n > 1 { format!("{t} × {n}") } else { t }))
                    };
                    self.ui.push_chat(format!("[butin] {line}"));
                    let event = if gold { "CHAT_MSG_MONEY" } else { "CHAT_MSG_LOOT" };
                    self.addons.fire_event_str(event, &[&line]);
                } else {
                    self.ui.push_chat(format!("[butin] Impossible de ramasser {name} (réservé à un autre joueur ou trop loin)."));
                }
            }
            WorldEvent::LevelUp { cid } => {
                if cid == me {
                    let level = self.hud.stats.level.unwrap_or(0);
                    self.ui.push_chat(format!("[système] Vous passez au niveau {level} !"));
                    self.hud.push_system(&format!("Niveau {level} atteint !"));
                    self.combat_float(me, format!("Niveau {level} !"), Color32::from_rgb(120, 220, 255), true);
                    self.addons.fire("PLAYER_LEVEL_UP", level);
                } else {
                    let name = self.combat_name(cid);
                    self.ui.push_chat(format!("[système] {name} passe au niveau supérieur."));
                }
            }
            _ => {}
        }
    }

    /// Demande d'attaque de `cid` (clic, touche, commande).
    pub(super) fn combat_attack(&mut self, cid: i32) -> bool {
        if self.combat.player_dead {
            self.ui.push_chat("[combat] Vous êtes mort.");
            return false;
        }
        let Some((tid, name, hp)) = self.entities.iter().find(|e| e.cid == cid).map(|e| (e.tid, e.name.clone(), e.hp)) else {
            self.ui.push_chat("[combat] Pas de cible à attaquer.");
            return false;
        };
        if self.combat.dead.contains(&cid) || hp == Some(0) {
            self.ui.push_chat(format!("[combat] {name} est déjà mort."));
            return false;
        }
        if !self.combat_attackable(tid) {
            self.ui.push_chat(format!("[combat] {name} ne peut pas être attaqué."));
            return false;
        }
        let Some(net) = &self.net else { return false };
        net.send(Command::Combat(CombatCommand::Attack { target: cid, forced: true }));
        self.addons.set_target(Some(cid));
        if self.combat.attacking != Some(cid) {
            self.ui.push_chat(format!("[combat] Vous attaquez {name}."));
            if self.combat.attacking.is_none() {
                self.addons.fire_event("PLAYER_REGEN_DISABLED", &[]);
            }
        }
        self.combat.attacking = Some(cid);
        self.combat.pickup = None;
        // Le serveur mène la marche jusqu'à la cible : pas de paquet d'arrêt du client, qui
        // interromprait l'attaque.
        self.walking = false;
        true
    }

    /// Le joueur se déplace lui-même : l'attaque et le ramassage en cours sont abandonnés (le
    /// serveur arrête l'attaque au premier déplacement demandé).
    pub(super) fn combat_interrupt(&mut self) {
        if self.combat.attacking.take().is_some() {
            self.addons.fire_event("PLAYER_REGEN_ENABLED", &[]);
        }
        self.combat.pickup = None;
    }

    /// Ramassage d'un objet au sol : tout de suite s'il est à portée, sinon après la marche.
    pub(super) fn combat_pick_up(&mut self, iid: i32) -> bool {
        if self.combat.player_dead {
            return false;
        }
        let (Some(loot), Some(player)) = (self.combat.ground.get(&iid), self.player.as_ref()) else {
            return false;
        };
        let at = loot.item.position;
        let flat = |a: Vec3, b: Vec3| Vec3::new(a.x - b.x, 0.0, a.z - b.z).length();
        if flat(player.position, at) <= PICKUP_RANGE {
            self.combat_send_pickup(iid);
            return true;
        }
        let from = player.position;
        let dest = Vec3::new(at.x, self.terrain.height_at(at.x, at.z).unwrap_or(at.y), at.z);
        if let Some(p) = self.player.as_mut() {
            p.destination = dest;
        }
        if let Some(net) = &self.net {
            net.send(Command::MoveTo { position: from, destination: dest });
        }
        self.combat.attacking = None;
        self.walking = true;
        self.combat.pickup = Some(iid);
        true
    }

    fn combat_send_pickup(&mut self, iid: i32) {
        if self.combat.requested.get(&iid).is_some_and(|t| t.elapsed() < RETRY) {
            return;
        }
        if let Some(net) = &self.net {
            net.send(Command::Combat(CombatCommand::PickUp(iid)));
            self.combat.requested.insert(iid, Instant::now());
        }
    }

    /// Monstre vivant et attaquable le plus proche (à 8000 unités au plus) dont le nom contient
    /// `wanted` (en minuscules ; vide = n'importe lequel).
    fn combat_nearest_prey(&mut self, wanted: &str) -> Option<i32> {
        let p = self.player.as_ref()?.position;
        let mut cands: Vec<(i32, i32, f32)> = self
            .entities
            .iter()
            .filter(|e| !e.is_player && e.hp != Some(0) && !self.combat.dead.contains(&e.cid))
            .filter(|e| wanted.is_empty() || e.name.to_lowercase().contains(wanted))
            .map(|e| (e.cid, e.tid, e.position.distance(p)))
            .filter(|(_, _, d)| *d <= 8000.0)
            .collect();
        cands.sort_by(|a, b| a.2.total_cmp(&b.2));
        cands.into_iter().find(|(_, tid, _)| self.combat_attackable(*tid)).map(|(cid, _, _)| cid)
    }

    /// Objet au sol le plus proche du joueur (pour `Espace` et `/ramasser`).
    fn combat_nearest_loot(&self) -> Option<i32> {
        let p = self.player.as_ref()?.position;
        self.combat
            .ground
            .values()
            .map(|l| (l.item.iid, Vec3::new(l.item.position.x - p.x, 0.0, l.item.position.z - p.z).length()))
            .filter(|(_, d)| *d <= 6000.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(iid, _)| iid)
    }

    /// Objet au sol sous le curseur (à moins de 30 px de sa projection).
    fn combat_loot_under_cursor(&self) -> Option<i32> {
        let (Some(gpu), Some((cx, cy))) = (&self.gpu, self.last_cursor) else {
            return None;
        };
        let (w, h) = (gpu.renderer.config.width as f32, gpu.renderer.config.height as f32);
        let vp = self.camera.view_proj(gpu.renderer.aspect());
        self.combat
            .ground
            .values()
            .filter_map(|l| {
                let pos = Vec3::new(l.item.position.x, l.ground_y.unwrap_or(l.item.position.y) + 15.0, l.item.position.z);
                let c = vp * pos.extend(1.0);
                (c.w > 1e-3).then(|| {
                    let (sx, sy) = (((c.x / c.w) + 1.0) * 0.5 * w, (1.0 - c.y / c.w) * 0.5 * h);
                    (l.item.iid, ((sx - cx as f32).powi(2) + (sy - cy as f32).powi(2)).sqrt())
                })
            })
            .filter(|(_, d)| *d <= 30.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(iid, _)| iid)
    }

    /// Clic gauche en jeu, avant le déplacement : rend `true` s'il a été pris (mort, objet au sol,
    /// attaque de la cible déjà choisie).
    pub(super) fn combat_left_click(&mut self) -> bool {
        if self.combat.player_dead {
            return true;
        }
        if let Some(iid) = self.combat_loot_under_cursor() {
            return self.combat_pick_up(iid);
        }
        match self.entity_under_cursor() {
            Some(cid) if self.addons.target_cid() == Some(cid) => {
                let tid = self.entities.iter().find(|e| e.cid == cid).map(|e| e.tid);
                if tid.is_some_and(|t| self.combat_attackable(t)) {
                    self.combat_attack(cid);
                    return true;
                }
                // PNJ déjà ciblé : lui parler (voir `app_npc.rs`).
                self.npc_talk(cid)
            }
            _ => false,
        }
    }

    /// Bouton droit : un clic sans glisser sur un monstre l'attaque, sur un objet le ramasse.
    pub(super) fn combat_right_button(&mut self, pressed: bool) {
        if pressed {
            self.combat.right_press = self.last_cursor;
            return;
        }
        let (Some(start), Some(now)) = (self.combat.right_press.take(), self.last_cursor) else {
            return;
        };
        if (start.0 - now.0).abs() > 5.0 || (start.1 - now.1).abs() > 5.0 || self.combat.player_dead {
            return;
        }
        if let Some(cid) = self.entity_under_cursor() {
            let tid = self.entities.iter().find(|e| e.cid == cid).map(|e| e.tid);
            if tid.is_some_and(|t| self.combat_attackable(t)) {
                self.combat_attack(cid);
            } else if !self.npc_talk(cid) {
                // PNJ : dialogue (voir `app_npc.rs`) ; autre personnage : il devient la cible.
                self.addons.set_target(Some(cid));
            }
        } else if let Some(iid) = self.combat_loot_under_cursor() {
            self.combat_pick_up(iid);
        }
    }

    /// Touches du combat : `F` attaque la cible, `Espace` ramasse l'objet le plus proche.
    pub(super) fn combat_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::KeyF => {
                match self.addons.target_cid() {
                    Some(cid) => {
                        self.combat_attack(cid);
                    }
                    None => self.ui.push_chat("[combat] Pas de cible (Tab ou clic sur un monstre)."),
                }
                true
            }
            KeyCode::Space => {
                match self.combat_nearest_loot() {
                    Some(iid) => {
                        self.combat_pick_up(iid);
                    }
                    None => self.ui.push_chat("[butin] Aucun objet au sol à proximité."),
                }
                true
            }
            _ => false,
        }
    }

    /// `/attaquer`, `/ramasser`, `/ressusciter` (et alias anglais).
    pub(super) fn combat_slash(&mut self, cmd: &str, arg: &str) -> bool {
        match cmd {
            // `/attaquer` : la cible ; `/attaquer nom` (ou sans cible) : le monstre vivant le plus
            // proche dont le nom contient `nom`.
            "/attaquer" | "/attack" => {
                let wanted = arg.trim().to_lowercase();
                let pick = if wanted.is_empty() { self.addons.target_cid() } else { None };
                match pick.or_else(|| self.combat_nearest_prey(&wanted)) {
                    Some(cid) => {
                        self.combat_attack(cid);
                    }
                    None => self.ui.push_chat("[combat] Aucun monstre à attaquer à proximité."),
                }
            }
            "/ramasser" | "/loot" | "/pickup" => {
                self.combat_key(KeyCode::Space);
            }
            "/ressusciter" | "/release" | "/resurrect" => self.combat_resurrect(),
            _ => return false,
        }
        true
    }

    fn combat_resurrect(&mut self) {
        if !self.combat.player_dead {
            self.ui.push_chat("[combat] Vous n'êtes pas mort.");
            return;
        }
        if let Some(net) = &self.net {
            send_resurrect(net, &mut self.combat, &mut self.ui);
        }
    }

    /// Chaque image, avant la simulation : PV des vues, morts, marche vers un objet, chiffres.
    pub(super) fn combat_frame(&mut self) {
        // PV et maximum de la vue, une fois l'entité créée par `on_world_event`.
        let pending: Vec<i32> = self.combat.view_stats.keys().copied().collect();
        for cid in pending {
            let Some(e) = self.entities.iter_mut().find(|e| e.cid == cid) else { continue };
            let (hp, max) = self.combat.view_stats.remove(&cid).unwrap_or_default();
            if e.hp.is_none() {
                e.hp = if self.combat.dead.contains(&cid) { Some(0) } else { hp };
            }
            if let Some(max) = max {
                self.hud.note_max_hp(cid, max);
            }
        }
        // Animations de mort (les animateurs sont recréés quand les maillages changent).
        let assets = self.assets.clone();
        let dead: Vec<i32> = self.combat.dead.iter().copied().collect();
        for cid in dead {
            let at_end = self.combat.corpses_in_view.contains(&cid);
            if let Some(a) = self.combat_animator(cid) {
                if !a.is_dead() {
                    a.set_dead(&assets, true, at_end);
                }
            }
        }
        let me = self.combat_me();
        let player_dead = self.combat.player_dead;
        if let Some(a) = self.combat_animator(me) {
            if a.is_dead() != player_dead {
                a.set_dead(&assets, player_dead, false);
            }
        }
        for (cid, (_, a)) in self.animators.iter_mut() {
            if let Some(a) = a.as_mut().filter(|a| a.is_dead() && !self.combat.dead.contains(cid)) {
                a.set_dead(&assets, false, false);
            }
        }
        // Marche vers un objet : ramassage dès qu'il est à portée.
        if let (Some(iid), Some(p)) = (self.combat.pickup, self.player.as_ref()) {
            match self.combat.ground.get(&iid) {
                Some(l) if Vec3::new(l.item.position.x - p.position.x, 0.0, l.item.position.z - p.position.z).length() <= PICKUP_RANGE => {
                    self.combat.pickup = None;
                    self.combat_send_pickup(iid);
                }
                Some(_) => {}
                None => self.combat.pickup = None,
            }
        }
        let now = Instant::now();
        self.combat.requested.retain(|_, t| now.duration_since(*t) < RETRY * 2);
        self.combat.floaters.retain(|f| now.duration_since(f.born).as_secs_f32() < FLOATER_LIFE);
        if let Some(cid) = self.combat.attacking {
            if !self.entities.iter().any(|e| e.cid == cid) {
                self.combat.attacking = None;
            }
        }
    }

    /// Modèles des objets au sol.
    pub(super) fn combat_draw_items(&self) -> Vec<DrawItem> {
        let mut out = Vec::new();
        for l in self.combat.ground.values() {
            let pos = Vec3::new(l.item.position.x, l.ground_y.unwrap_or(l.item.position.y), l.item.position.z);
            for (mesh, local) in &l.meshes {
                out.push(DrawItem { mesh: mesh.clone(), model: Mat4::from_translation(pos) * *local, tint: [1.0; 4] });
            }
        }
        out
    }

    /// Textes à dessiner (avant l'emprunt du GPU par la fermeture egui).
    pub(super) fn combat_view(&self) -> CombatView {
        let mut view = CombatView {
            dead: self.combat.player_dead,
            can_resurrect: self.combat.resurrect_sent.is_none_or(|t| t.elapsed() >= RETRY),
            ..Default::default()
        };
        let Some(gpu) = &self.gpu else { return view };
        let ppp = gpu.window.scale_factor() as f32;
        let (w, h) = (gpu.renderer.config.width as f32 / ppp, gpu.renderer.config.height as f32 / ppp);
        let vp = self.camera.view_proj(gpu.renderer.aspect());
        let to_screen = |p: Vec3| {
            let c = vp * p.extend(1.0);
            (c.w > 1e-3 && c.z / c.w <= 1.0).then(|| Pos2::new(((c.x / c.w) + 1.0) * 0.5 * w, (1.0 - c.y / c.w) * 0.5 * h))
        };
        let me = self.combat_me();
        let mut stack: HashMap<i32, usize> = HashMap::new();
        for f in self.combat.floaters.iter().rev() {
            let pos = if f.cid == me {
                self.player.as_ref().map(|p| p.position)
            } else {
                self.entities.iter().find(|e| e.cid == f.cid).map(|e| e.position)
            };
            let Some(pos) = pos else { continue };
            let age = f.born.elapsed().as_secs_f32();
            let n = stack.entry(f.cid).or_default();
            let lift = 200.0 + age * 60.0;
            if let Some(p) = to_screen(pos + Vec3::Y * lift) {
                let alpha = (1.0 - (age / FLOATER_LIFE).powi(2)).clamp(0.0, 1.0);
                let size = if f.big { 26.0 } else { 19.0 };
                view.floaters.push((p - Vec2::new(0.0, *n as f32 * 18.0), f.text.clone(), f.color, size, alpha));
            }
            *n += 1;
        }
        let player_pos = self.player.as_ref().map(|p| p.position);
        for l in self.combat.ground.values() {
            let pos = Vec3::new(l.item.position.x, l.ground_y.unwrap_or(l.item.position.y) + 45.0, l.item.position.z);
            if player_pos.is_some_and(|pp| pp.distance(pos) > 8000.0) {
                continue;
            }
            if let Some(p) = to_screen(pos) {
                let mine = l.item.owner_cid.is_none_or(|o| o == me);
                let color = match (l.item.is_gold(), mine) {
                    (_, false) => Color32::from_rgb(150, 150, 150),
                    (true, true) => Color32::from_rgb(255, 210, 60),
                    (false, true) => Color32::from_rgb(235, 235, 235),
                };
                view.labels.push((p, l.name.clone(), color));
            }
        }
        view
    }
}

fn send_resurrect(net: &NetLink, glue: &mut CombatGlue, ui: &mut UiState) {
    if glue.resurrect_sent.is_some_and(|t| t.elapsed() < RETRY) {
        return;
    }
    net.send(Command::Combat(CombatCommand::ResurrectTown));
    glue.resurrect_sent = Some(Instant::now());
    ui.push_chat("[combat] Résurrection au village demandée.");
}

/// Dessin egui : chiffres de dégâts, noms des objets au sol, fenêtre de résurrection. Fonction libre :
/// appelée dans la fermeture egui, qui n'emprunte que certains champs de l'application.
pub fn panel(
    ctx: &egui::Context,
    glue: &mut CombatGlue,
    view: &CombatView,
    hud: &mut HudState,
    assets: &Assets,
    net: Option<&NetLink>,
    ui: &mut UiState,
) {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::new("archrust-combat")));
    for (pos, text, color) in &view.labels {
        let font = FontId::proportional(12.0);
        painter.text(*pos + Vec2::splat(1.0), Align2::CENTER_BOTTOM, text, font.clone(), Color32::from_black_alpha(200));
        painter.text(*pos, Align2::CENTER_BOTTOM, text, font, *color);
    }
    for (pos, text, color, size, alpha) in &view.floaters {
        let font = FontId::proportional(*size);
        let a = (alpha * 255.0) as u8;
        painter.text(*pos + Vec2::splat(2.0), Align2::CENTER_BOTTOM, text, font.clone(), Color32::from_black_alpha((alpha * 220.0) as u8));
        painter.text(*pos, Align2::CENTER_BOTTOM, text, font, Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), a));
    }
    if view.dead && crate::hud::draw_resurrection(ctx, hud, assets, view.can_resurrect) {
        match net {
            Some(net) => send_resurrect(net, glue, ui),
            None => ui.push_chat("[combat] hors ligne"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_des_coups() {
        assert_eq!(
            hit_line(true, false, "Moi", "Ver vert", Some(outcome::SUCCESS), Some(12), Some(80)).as_deref(),
            Some("[combat] Vous frappez Ver vert : 12 dégât(s) (reste 80 PV).")
        );
        assert_eq!(hit_line(false, true, "Ver vert", "Moi", Some(outcome::MISS), None, None).as_deref(), Some("[combat] Ver vert vous manque."));
        assert_eq!(hit_line(false, false, "A", "B", Some(outcome::SUCCESS), Some(3), Some(1)), None);
    }

    #[test]
    fn textes_flottants() {
        assert_eq!(floater_text(Some(outcome::SUCCESS), Some(7)), Some(("-7".into(), false)));
        assert_eq!(floater_text(Some(outcome::CRITICAL), Some(20)), Some(("-20 !".into(), true)));
        assert_eq!(floater_text(Some(outcome::MISS), None), Some(("Raté".into(), false)));
        assert_eq!(floater_text(Some(outcome::SUCCESS), None), None);
    }
}
