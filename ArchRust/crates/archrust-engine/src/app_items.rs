//! Objets côté moteur : inventaire actif et équipement. Protocole : `archrust_netclient::items`.
//!
//! Souris (fenêtres d'Archonia, voir `hud_items.rs`) :
//! * clic droit sur un objet du sac : le porter (`Type` 0 d'`ItemTemplateAll.ini`) ou l'utiliser
//!   (potion, parchemin) ; clic droit sur un objet porté : le ranger ;
//! * glisser dans le sac : déplacer (sur une pile du même objet : les réunir) ; glisser vers un
//!   emplacement de la fenêtre du personnage : porter ; glisser un objet porté vers le sac : le
//!   ranger à cette place ; lâcher un objet du sac hors des fenêtres : le détruire (après accord) ;
//! * Maj + clic sur une pile : la séparer (quantité choisie, case libre trouvée par le client).
//!
//! Touches et commandes : `C` fenêtre du personnage (`/perso`) ; `/utiliser nom` (`/use`),
//! `/porter nom` (`/equip`), `/ranger nom` (`/unequip`), `/detruire nom` (`/destroy`),
//! `/retour` (`/return`) après un parchemin aller-retour.
//!
//! L'équipement porté s'affiche aussi sur le modèle du joueur, sauf les armes et boucliers (mains,
//! `Part` 9 et 10) : leurs modèles ne sont pas encore rattachés aux os de la main et tomberaient aux
//! pieds du personnage.

use super::App;
use crate::assets::Assets;
use crate::hud::{EquippedItem, HudState, ItemDrag, ItemUi};
use crate::net::NetLink;
use crate::scene::{entity_meshes_with_equipment, item_name_for_tid};
use crate::ui::UiState;
use archrust_netclient::items::{status, Grid, ItemCommand};
use archrust_netclient::world::{EquipmentView, ItemRecord};
use archrust_netclient::{Command, WorldEvent};
use glam::Mat4;
use std::collections::HashMap;

/// Cases de l'inventaire acceptées par le serveur : 4 sacs de 4 × 4 (la grille d'Archonia en dessine
/// 5 × 5 ; une case hors de ces limites est refusée et l'objet revient à sa place).
const BAGS: i16 = 4;
const ROWS: i16 = 4;
const COLS: i16 = 4;

/// `Type` d'`ItemTemplateAll.ini` : objet qui se porte.
const TYPE_EQUIP: i32 = 0;
/// `Type` d'`ItemTemplateAll.ini` : objet qui s'utilise (potion, parchemin…).
const TYPE_USABLE: i32 = 1;
/// Parties du corps des mains (`Part` 9 gauche, 10 droite) : armes et boucliers.
const HAND_PARTS: [i32; 2] = [9, 10];

/// Colonnes utiles d'un modèle d'objet.
#[derive(Debug, Clone, Copy, Default)]
struct ItemTemplate {
    kind: Option<i32>,
    part: Option<i32>,
}

#[derive(Default)]
pub struct ItemsGlue {
    templates: Option<HashMap<i32, ItemTemplate>>,
    /// Fenêtre du personnage (`Character_Status`) ouverte.
    pub show_character: bool,
    /// Objet dont la destruction attend l'accord du joueur.
    destroy: Option<i32>,
    /// Pile à séparer et quantité choisie.
    split: Option<(i32, i32)>,
    /// Point de retour d'un parchemin aller-retour mémorisé par le serveur.
    return_enabled: bool,
}

impl ItemsGlue {
    fn template(&mut self, assets: &Assets, tid: Option<i32>) -> ItemTemplate {
        let map = self.templates.get_or_insert_with(|| load_templates(assets));
        tid.and_then(|t| map.get(&t).copied()).unwrap_or_default()
    }
}

/// `Type` et `Part` de chaque modèle (colonnes nommées par l'en-tête `[]`).
fn load_templates(assets: &Assets) -> HashMap<i32, ItemTemplate> {
    let Ok(t) = assets.table("ini", "ItemTemplateAll.ini") else {
        return HashMap::new();
    };
    let column = |name: &str| {
        t.section("")
            .and_then(|h| h.entries.iter().find(|(_, v)| v.eq_ignore_ascii_case(name)).map(|(k, _)| k.clone()))
    };
    let (kind, part) = (column("Type"), column("Part"));
    let int = |s: &archrust_data::ini::IniSection, col: &Option<String>| {
        col.as_ref().and_then(|c| s.get(c)).and_then(|v| v.trim().parse::<i32>().ok())
    };
    t.sections
        .iter()
        .filter_map(|s| Some((s.name.parse::<i32>().ok()?, ItemTemplate { kind: int(s, &kind), part: int(s, &part) })))
        .collect()
}

fn item_label(assets: &Assets, item: &ItemRecord) -> String {
    item.template_id
        .and_then(|tid| item_name_for_tid(assets, tid))
        .unwrap_or_else(|| format!("Objet TID {}", item.template_id.unwrap_or_default()))
}

/// Première case libre de l'inventaire (sac, ligne, colonne).
fn free_cell<'a>(items: impl Iterator<Item = &'a ItemRecord>) -> Option<Grid> {
    let taken: Vec<Grid> = items.filter(|i| i.status == Some(status::INVENTORY)).filter_map(|i| i.grid).collect();
    (0..BAGS)
        .flat_map(|b| (0..ROWS).flat_map(move |r| (0..COLS).map(move |c| [b, r, c])))
        .find(|g| !taken.contains(g))
}

/// Demande pour un objet activé (clic droit, `/utiliser`) : porter, utiliser, ou rien.
fn activate(glue: &mut ItemsGlue, assets: &Assets, item: &ItemRecord) -> Result<ItemCommand, String> {
    if item.status == Some(status::EQUIP) {
        return Ok(ItemCommand::Unequip { iid: item.id, grid: None });
    }
    match glue.template(assets, item.template_id).kind {
        Some(TYPE_EQUIP) => Ok(ItemCommand::Equip(item.id)),
        Some(TYPE_USABLE) | None => Ok(ItemCommand::Use(item.id)),
        Some(_) => Err(format!("« {} » ne s'utilise pas.", item_label(assets, item))),
    }
}

/// Traduit une demande de l'interface en commande, ou en message pour le joueur.
fn request(glue: &mut ItemsGlue, assets: &Assets, items: &HashMap<i32, ItemRecord>, ui_req: ItemUi) -> Option<Result<ItemCommand, String>> {
    match ui_req {
        ItemUi::Activate { iid, .. } => Some(activate(glue, assets, items.get(&iid)?)),
        ItemUi::DropOnBag { drag: ItemDrag { iid, worn: true, .. }, grid } => {
            // Case occupée : le serveur choisit la place.
            let free = !items.values().any(|i| i.status == Some(status::INVENTORY) && i.grid == Some(grid));
            Some(Ok(ItemCommand::Unequip { iid, grid: free.then_some(grid) }))
        }
        ItemUi::DropOnBag { drag, grid } => {
            let item = items.get(&drag.iid)?;
            (item.grid != Some(grid)).then_some(Ok(ItemCommand::Move { iid: drag.iid, grid }))
        }
        ItemUi::DropOnEquip(drag) => {
            let item = items.get(&drag.iid)?;
            match glue.template(assets, item.template_id).kind {
                Some(TYPE_EQUIP) | None => Some(Ok(ItemCommand::Equip(drag.iid))),
                Some(_) => Some(Err(format!("« {} » ne se porte pas.", item_label(assets, item)))),
            }
        }
        ItemUi::Split(iid) => {
            let count = items.get(&iid)?.count.unwrap_or(1);
            if count > 1 {
                glue.split = Some((iid, (count / 2).max(1)));
            }
            None
        }
    }
}

fn send(net: Option<&NetLink>, cmd: ItemCommand) {
    if let Some(net) = net {
        net.send(Command::Item(cmd));
    }
}

/// Fenêtres et demandes des objets, après l'inventaire et la fenêtre du personnage : demandes de
/// la souris, objet lâché hors des fenêtres, accord de destruction, séparation, retour, icône saisie.
#[allow(clippy::too_many_arguments)]
pub fn panel(
    ctx: &egui::Context,
    glue: &mut ItemsGlue,
    requests: Vec<ItemUi>,
    items: &HashMap<i32, ItemRecord>,
    hud: &mut HudState,
    assets: &Assets,
    net: Option<&NetLink>,
    ui: &mut UiState,
) {
    for r in requests {
        match request(glue, assets, items, r) {
            Some(Ok(cmd)) => send(net, cmd),
            Some(Err(text)) => ui.push_chat(format!("[objet] {text}")),
            None => {}
        }
    }
    // Objet du sac lâché ailleurs que sur une fenêtre : destruction, après accord.
    if ctx.input(|i| i.pointer.any_released()) && !ctx.is_pointer_over_area() {
        if let Some(drag) = egui::DragAndDrop::payload::<ItemDrag>(ctx) {
            if !drag.worn && items.contains_key(&drag.iid) {
                glue.destroy = Some(drag.iid);
            }
        }
    }
    crate::hud::draw_drag(ctx, hud, assets);

    if let Some(iid) = glue.destroy {
        let name = items.get(&iid).map(|i| item_label(assets, i));
        let mut answer = None;
        egui::Window::new("Détruire un objet")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |w| {
                w.label(format!("Détruire « {} » ? C'est définitif.", name.as_deref().unwrap_or("?")));
                w.horizontal(|w| {
                    if w.button("Détruire").clicked() {
                        answer = Some(true);
                    }
                    if w.button("Annuler").clicked() {
                        answer = Some(false);
                    }
                });
            });
        match (answer, name) {
            (Some(true), Some(_)) => {
                send(net, ItemCommand::Destroy(iid));
                glue.destroy = None;
            }
            (Some(_), _) | (None, None) => glue.destroy = None,
            _ => {}
        }
    }

    if let Some((iid, mut count)) = glue.split {
        let total = items.get(&iid).and_then(|i| i.count).unwrap_or(1);
        let mut answer = None;
        egui::Window::new("Séparer la pile")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |w| {
                w.horizontal(|w| {
                    w.label("Quantité :");
                    w.add(egui::DragValue::new(&mut count).range(1..=(total - 1).max(1)));
                    w.label(format!("sur {total}"));
                });
                w.horizontal(|w| {
                    if w.button("Séparer").clicked() {
                        answer = Some(true);
                    }
                    if w.button("Annuler").clicked() {
                        answer = Some(false);
                    }
                });
            });
        glue.split = Some((iid, count));
        match answer {
            Some(true) if total > 1 => {
                match free_cell(items.values()) {
                    Some(grid) => send(net, ItemCommand::Split { iid, count: count.clamp(1, total - 1), grid }),
                    None => ui.push_chat("[objet] Aucune case libre dans l'inventaire."),
                }
                glue.split = None;
            }
            Some(_) => glue.split = None,
            None if total <= 1 => glue.split = None,
            None => {}
        }
    }

    if glue.return_enabled {
        let top = crate::hud::reserved(ctx).0;
        egui::Area::new(egui::Id::new("archrust-return-scroll"))
            .anchor(egui::Align2::CENTER_TOP, egui::Vec2::new(-top / 2.0, 8.0))
            .show(ctx, |w| {
                if w.button("↩ Revenir au point de départ (parchemin)").clicked() {
                    send(net, ItemCommand::Return);
                }
            });
    }
}

impl App {
    /// Objets portés par le joueur, avec leur partie du corps, pour la fenêtre du personnage.
    pub(super) fn items_equipped(&mut self) -> Vec<EquippedItem> {
        let mut worn: Vec<&ItemRecord> = self.items.values().filter(|i| i.status == Some(status::EQUIP)).collect();
        worn.sort_by_key(|i| i.id);
        worn.into_iter()
            .map(|i| EquippedItem {
                item: i.clone(),
                name: item_label(&self.assets, i),
                part: self.item_ui.template(&self.assets, i.template_id).part,
            })
            .collect()
    }

    /// Confirmation d'utilisation et point de retour d'un parchemin aller-retour.
    pub(super) fn on_item_event(&mut self, ev: WorldEvent) {
        match ev {
            WorldEvent::ItemUsed { tid, user, .. } => {
                let mine = self.player.as_ref().is_some_and(|p| Some(p.cid) == user);
                if let (true, Some(tid)) = (mine || user.is_none(), tid) {
                    let name = item_name_for_tid(&self.assets, tid).unwrap_or_else(|| format!("Objet TID {tid}"));
                    self.ui.push_chat(format!("[objet] « {name} » utilisé."));
                }
            }
            WorldEvent::ReturnPoint { enabled } => {
                self.item_ui.return_enabled = enabled;
                if enabled {
                    self.ui.push_chat("[objet] Point de départ mémorisé : /retour (ou le bouton en haut) pour y revenir.");
                }
            }
            _ => {}
        }
    }

    /// L'équipement porté s'affiche sur le modèle du joueur : modèle rechargé quand il change. Les
    /// armes et boucliers sont laissés de côté tant qu'ils ne suivent pas les os de la main.
    pub(super) fn items_frame(&mut self) {
        let (glue, assets) = (&mut self.item_ui, &self.assets);
        let mut worn: Vec<EquipmentView> = self
            .items
            .values()
            .filter(|i| i.status == Some(status::EQUIP))
            .filter(|i| glue.template(assets, i.template_id).part.is_none_or(|p| !HAND_PARTS.contains(&p)))
            .filter_map(|i| Some(EquipmentView { item_id: i.id, template_id: i.template_id?, count: i.count.unwrap_or(1) as i16 }))
            .collect();
        worn.sort_by_key(|e| e.item_id);
        let Some(player) = self.player.as_ref() else { return };
        if player.cid == 0 || player.equipment == worn {
            return;
        }
        let Some(gpu) = self.gpu.as_mut() else { return };
        let fallback = gpu.player_marker.clone();
        let meshes = entity_meshes_with_equipment(&mut gpu.renderer, &self.assets, player.tid, player.head, &worn, &fallback)
            .unwrap_or_else(|err| {
                log::warn!("équipement du joueur (TID {}) : {err:#}", player.tid);
                vec![(fallback.clone(), Mat4::IDENTITY)]
            });
        log::info!("équipement du joueur : {} objet(s) porté(s), {} maillage(s)", worn.len(), meshes.len());
        if let Some(p) = self.player.as_mut() {
            p.meshes = meshes;
            p.equipment = worn;
        }
    }

    /// Objet du joueur dont le nom contient `wanted` (porté si `worn`), le plus petit identifiant
    /// d'abord.
    fn find_item(&self, wanted: &str, worn: Option<bool>) -> Option<ItemRecord> {
        let wanted = wanted.trim().to_lowercase();
        let mut found: Vec<&ItemRecord> = self
            .items
            .values()
            .filter(|i| matches!(i.status, Some(status::INVENTORY | status::EQUIP)))
            .filter(|i| worn.is_none_or(|w| (i.status == Some(status::EQUIP)) == w))
            .filter(|i| {
                wanted.is_empty()
                    || i.template_id.map(|t| t.to_string()) == Some(wanted.clone())
                    || item_label(&self.assets, i).to_lowercase().contains(&wanted)
            })
            .collect();
        found.sort_by_key(|i| i.id);
        found.first().map(|i| (*i).clone())
    }

    /// `/perso`, `/utiliser`, `/porter`, `/ranger`, `/detruire`, `/retour`.
    pub(super) fn items_slash(&mut self, cmd: &str, arg: &str) -> bool {
        let need = |s: &mut Self, usage: &str| s.ui.push_chat(format!("[objet] usage : {usage}"));
        match cmd {
            "/perso" | "/char" | "/personnage" => self.item_ui.show_character = !self.item_ui.show_character,
            "/utiliser" | "/use" | "/porter" | "/equip" | "/ranger" | "/unequip" | "/detruire" | "/détruire" | "/destroy" => {
                if arg.trim().is_empty() {
                    need(self, &format!("{cmd} nom de l'objet (ou TID)"));
                    return true;
                }
                let worn = match cmd {
                    "/ranger" | "/unequip" => Some(true),
                    "/porter" | "/equip" | "/detruire" | "/détruire" | "/destroy" => Some(false),
                    _ => None,
                };
                let Some(item) = self.find_item(arg, worn) else {
                    self.ui.push_chat(format!("[objet] Aucun objet « {} » dans l'inventaire.", arg.trim()));
                    return true;
                };
                let label = item_label(&self.assets, &item);
                let cmd = match cmd {
                    "/ranger" | "/unequip" => Ok(ItemCommand::Unequip { iid: item.id, grid: None }),
                    "/porter" | "/equip" => Ok(ItemCommand::Equip(item.id)),
                    "/detruire" | "/détruire" | "/destroy" => Ok(ItemCommand::Destroy(item.id)),
                    _ => activate(&mut self.item_ui, &self.assets, &item),
                };
                match cmd {
                    Ok(c) => {
                        self.ui.push_chat(format!("[objet] {label} : demande envoyée."));
                        send(self.net.as_ref(), c);
                    }
                    Err(text) => self.ui.push_chat(format!("[objet] {text}")),
                }
            }
            "/retour" | "/return" => {
                if !self.item_ui.return_enabled {
                    self.ui.push_chat("[objet] Aucun point de départ mémorisé (parchemin aller-retour).");
                }
                send(self.net.as_ref(), ItemCommand::Return);
            }
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bag(id: i32, grid: Grid) -> ItemRecord {
        ItemRecord { operation: 0, id, template_id: Some(1), owner_cid: Some(7), count: Some(1), status: Some(2), grid: Some(grid) }
    }

    #[test]
    fn case_libre_et_demandes() {
        let mut items: HashMap<i32, ItemRecord> = [bag(1, [0, 0, 0]), bag(2, [0, 0, 1])].into_iter().map(|i| (i.id, i)).collect();
        assert_eq!(free_cell(items.values()), Some([0, 0, 2]));
        let assets = Assets::new(None, &[]);
        let mut glue = ItemsGlue::default();
        // Déplacement vers sa propre case : rien ; vers une autre : op 2.
        let drag = ItemDrag { iid: 1, tid: 1, worn: false, bank: false };
        assert!(request(&mut glue, &assets, &items, ItemUi::DropOnBag { drag, grid: [0, 0, 0] }).is_none());
        assert_eq!(
            request(&mut glue, &assets, &items, ItemUi::DropOnBag { drag, grid: [1, 2, 3] }),
            Some(Ok(ItemCommand::Move { iid: 1, grid: [1, 2, 3] }))
        );
        // Objet porté lâché sur une case prise : place choisie par le serveur.
        items.insert(3, ItemRecord { status: Some(3), grid: None, ..bag(3, [0, 0, 0]) });
        let worn = ItemDrag { iid: 3, tid: 1, worn: true, bank: false };
        assert_eq!(
            request(&mut glue, &assets, &items, ItemUi::DropOnBag { drag: worn, grid: [0, 0, 1] }),
            Some(Ok(ItemCommand::Unequip { iid: 3, grid: None }))
        );
        assert_eq!(
            request(&mut glue, &assets, &items, ItemUi::Activate { iid: 3, worn: true }),
            Some(Ok(ItemCommand::Unequip { iid: 3, grid: None }))
        );
        // Séparer : fenêtre de quantité ouverte sur la moitié.
        items.get_mut(&1).unwrap().count = Some(10);
        assert!(request(&mut glue, &assets, &items, ItemUi::Split(1)).is_none());
        assert_eq!(glue.split, Some((1, 5)));
    }
}
