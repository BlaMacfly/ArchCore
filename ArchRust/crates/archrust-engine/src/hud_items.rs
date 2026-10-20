//! Objets dans les fenêtres d'Archonia : grille de l'inventaire (`UI_Inventory`) et emplacements
//! d'équipement de la fenêtre du personnage (`Character_Status`, grilles `EquipSlotGrid…`).
//!
//! Le glisser-déposer passe par egui (charge [`ItemDrag`]) ; les demandes reviennent au moteur en
//! [`ItemUi`], qui les traduit en paquets AGPMITEM (voir `app_items.rs`).

use super::*;

/// Objet saisi à la souris.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemDrag {
    pub iid: i32,
    pub tid: i32,
    /// Objet porté (pris dans la fenêtre du personnage).
    pub worn: bool,
    /// Objet en banque (pris dans la fenêtre de la banque).
    pub bank: bool,
}

/// Demande de l'interface sur un objet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemUi {
    /// Clic droit : utiliser ou porter un objet du sac, ranger un objet porté.
    Activate { iid: i32, worn: bool },
    /// Objet lâché sur une case de l'inventaire.
    DropOnBag { drag: ItemDrag, grid: [i16; 3] },
    /// Objet lâché sur un emplacement d'équipement.
    DropOnEquip(ItemDrag),
    /// Maj + clic : séparer une pile.
    Split(i32),
}

/// Objet porté et sa partie du corps (`Part` de `ItemTemplateAll.ini`).
#[derive(Debug, Clone)]
pub struct EquippedItem {
    pub item: ItemRecord,
    pub name: String,
    pub part: Option<i32>,
}

/// Emplacements de `Character_Status` et parties du corps qu'ils montrent (2 corps, 3 tête,
/// 5 mains, 6 jambes, 7 pieds, 9 main gauche, 10 main droite, 11 anneau, 13 collier).
const SLOTS: &[(&str, i32)] = &[
    ("EquipSlotGridHead", 3),
    ("EquipSlotGridBody", 2),
    ("EquipSlotGridHands", 5),
    ("EquipSlotGridLegs", 6),
    ("EquipSlotGridFoot", 7),
    ("EquipSlotGridHandLeft", 9),
    ("EquipSlotGridHandRight", 10),
    ("EquipSlotGridRing1", 11),
    ("EquipSlotGridRing2", 11),
    ("EquipSlotGridNecklace", 13),
];

/// Icône d'un objet (`l%07x` de la colonne `TEXTURE`), sinon une case teintée.
pub(super) fn icon(ctx: &egui::Context, hud: &mut HudState, assets: &Assets, painter: &egui::Painter, cell: Rect, tid: Option<i32>, tint: Color32) {
    let icon = tid.and_then(|tid| hud.item_icons.get_or_insert_with(|| load_item_icons(assets)).get(&tid).cloned());
    match icon.and_then(|n| texture_icon(ctx, hud, assets, &n)) {
        Some(t) => {
            painter.image(t, cell.shrink(1.0), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), tint);
        }
        None => {
            painter.rect_filled(cell.shrink(3.0), 3.0, Color32::from_rgb(70, 60, 45).gamma_multiply(tint.a() as f32 / 255.0));
        }
    }
}

fn dragged_now(ctx: &egui::Context) -> Option<ItemDrag> {
    egui::DragAndDrop::payload::<ItemDrag>(ctx).map(|p| *p)
}

/// Grille `CONTROL_Inventory_Grid` du sac affiché : objets, glisser-déposer, clic droit, Maj + clic.
#[allow(clippy::too_many_arguments)]
pub(super) fn bag_grid(
    ctx: &egui::Context,
    ui: &mut egui::Ui,
    hud: &mut HudState,
    assets: &Assets,
    painter: &egui::Painter,
    layout: &Layout,
    origin: Pos2,
    c: &UiControl,
    items: &[(ItemRecord, String)],
    font: f32,
    out: &mut Vec<ItemUi>,
) {
    let s = layout.scale;
    let [gx, gy, cw, ch] = c.grid.unwrap_or([2.0, 2.0, 50.0, 50.0]);
    let cols = ((c.size[0] + gx) / (cw + gx)).floor().max(1.0) as i16;
    let rows = ((c.size[1] + gy) / (ch + gy)).floor().max(1.0) as i16;
    let layer = hud.inventory_layer;
    let dragging = dragged_now(ctx);
    let shift = ui.input(|i| i.modifiers.shift);
    for row in 0..rows {
        for col in 0..cols {
            let cell = layout.rect(origin, [c.pos[0] + col as f32 * (cw + gx), c.pos[1] + row as f32 * (ch + gy)], [cw, ch]);
            let here = items
                .iter()
                .find(|(i, _)| i.status == Some(2) && i.grid == Some([layer, row, col]));
            let resp = ui.interact(cell, egui::Id::new(("inv-cell", layer, row, col)), egui::Sense::click_and_drag());
            if let Some((item, name)) = here {
                let lifted = dragging.is_some_and(|d| d.iid == item.id);
                let tint = if lifted { Color32::from_white_alpha(70) } else { Color32::WHITE };
                icon(ctx, hud, assets, painter, cell, item.template_id, tint);
                let count = item.count.unwrap_or(1);
                if count > 1 && !lifted {
                    shadow_text(painter, cell.right_bottom() - Vec2::new(3.0, 2.0) * s, Align2::RIGHT_BOTTOM, &count.to_string(), font, Color32::WHITE);
                }
                if resp.dragged() {
                    resp.dnd_set_drag_payload(ItemDrag { iid: item.id, tid: item.template_id.unwrap_or(0), worn: false, bank: false });
                }
                if resp.secondary_clicked() {
                    out.push(ItemUi::Activate { iid: item.id, worn: false });
                } else if resp.clicked() && shift && count > 1 {
                    out.push(ItemUi::Split(item.id));
                }
                if dragging.is_none() {
                    let label = if count > 1 { format!("{name} ×{count}") } else { name.clone() };
                    let hint = if count > 1 {
                        "Clic droit : utiliser ou porter · glisser : déplacer · Maj + clic : séparer"
                    } else {
                        "Clic droit : utiliser ou porter · glisser : déplacer, hors fenêtre : détruire"
                    };
                    resp.clone().on_hover_text(format!("{label}\n{hint}"));
                }
            }
            if let Some(drag) = resp.dnd_release_payload::<ItemDrag>() {
                out.push(ItemUi::DropOnBag { drag: *drag, grid: [layer, row, col] });
            }
            let target = dragging.is_some() && resp.contains_pointer();
            if resp.hovered() || target {
                let color = if target { Color32::from_rgb(255, 210, 110) } else { Color32::from_rgb(120, 200, 255) };
                painter.rect_stroke(cell, 2.0, egui::Stroke::new(1.0_f32, color), egui::StrokeKind::Inside);
            }
        }
    }
}

/// Icône de l'objet saisi, sous le curseur.
pub fn draw_drag(ctx: &egui::Context, hud: &mut HudState, assets: &Assets) {
    let (Some(drag), Some(pos)) = (dragged_now(ctx), ctx.pointer_latest_pos()) else { return };
    let size = 44.0 * Layout::new(ctx.screen_rect()).scale.max(0.6);
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("archrust-item-drag")));
    let cell = Rect::from_center_size(pos, Vec2::splat(size));
    icon(ctx, hud, assets, &painter, cell, Some(drag.tid), Color32::from_white_alpha(220));
}

/// Fenêtre du personnage d'Archonia (`Character_Status`) : nom, niveau, PV, PM, expérience et
/// emplacements d'équipement (glisser depuis l'inventaire ou clic droit pour porter ; clic droit
/// ou glisser vers l'inventaire pour ranger).
#[allow(clippy::too_many_arguments)]
pub fn draw_character(
    ctx: &egui::Context,
    hud: &mut HudState,
    assets: &Assets,
    open: &mut bool,
    name: &str,
    equipped: &[EquippedItem],
    out: &mut Vec<ItemUi>,
) {
    if !*open {
        return;
    }
    let Some(w) = hud.windows(assets).iter().find(|w| w.name == "Character_Status").cloned() else {
        return;
    };
    let layout = Layout::new(ctx.screen_rect());
    let s = layout.scale;
    let size = Vec2::new(w.size[0], w.size[1]) * s;
    let stats = hud.stats.clone();
    let exp = hud.exp_ratio(assets);
    // Emplacement de chaque objet porté : la partie du corps, le second anneau à droite.
    let mut slot_of: HashMap<&str, &EquippedItem> = HashMap::new();
    for e in equipped {
        let Some(part) = e.part else { continue };
        if let Some((slot, _)) = SLOTS.iter().find(|(slot, p)| *p == part && !slot_of.contains_key(slot)) {
            slot_of.insert(slot, e);
        }
    }
    egui::Area::new(egui::Id::new("archonia-character"))
        .default_pos(layout.window_origin(&w))
        .movable(true)
        .constrain(true)
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            let origin = rect.min;
            let painter = ui.painter().clone();
            match w.textures.first() {
                Some((_, tex)) if hud.image(ctx, assets, tex).is_some() => control_image(ctx, hud, assets, &painter, rect, w.size, tex, 1.0),
                _ => {
                    painter.rect_filled(rect, 4.0, Color32::from_black_alpha(210));
                }
            }
            let font = (11.0 * s).max(9.0);
            let dragging = dragged_now(ctx);
            for c in &w.controls {
                let r = layout.rect(origin, c.pos, c.size);
                let display = c.display.as_deref().unwrap_or("");
                let value = |key: &str| display.contains(key);
                match c.kind {
                    1 if c.name == "CONTROL_29628" => {
                        shadow_text(&painter, r.left_center(), Align2::LEFT_CENTER, "Personnage", font * 1.1, Color32::from_rgb(235, 225, 200));
                    }
                    1 if display == "1:HP" || display == "1:MP" => {
                        shadow_text(&painter, r.left_center(), Align2::LEFT_CENTER, &display[2..], font, Color32::from_rgb(235, 225, 200));
                    }
                    1 if value("현재_레벨") => {
                        // Le fond dessine déjà « Lv ».
                        let text = stats.level.map_or("—".to_string(), |l| l.to_string());
                        shadow_text(&painter, r.center(), Align2::CENTER_CENTER, &text, font, Color32::from_rgb(255, 230, 160));
                    }
                    1 if value("캐릭터_이름") => {
                        shadow_text(&painter, r.left_center(), Align2::LEFT_CENTER, name, font * 1.05, Color32::WHITE);
                    }
                    1 if value("HP_All") || value("MP_All") => {
                        let (cur, max) = if value("HP_All") { (stats.hp, stats.max_hp) } else { (stats.mp, stats.max_mp) };
                        let text = match (cur, max) {
                            (Some(c), Some(m)) => format!("{c} / {m}"),
                            (Some(c), None) => c.to_string(),
                            _ => "—".to_string(),
                        };
                        shadow_text(&painter, r.left_center(), Align2::LEFT_CENTER, &text, font, Color32::WHITE);
                    }
                    9 if c.name == "Bar_CharacterExp" => {
                        if let (Some(f), Some((_, tex))) = (exp, c.textures.first()) {
                            control_image(ctx, hud, assets, &painter, r, c.size, tex, f);
                        }
                    }
                    1 if value("경험치") => {
                        if let Some(f) = exp {
                            shadow_text(&painter, r.center(), Align2::CENTER_CENTER, &format!("{:.1} %", f * 100.0), font, Color32::WHITE);
                        }
                    }
                    3 if c.name == "CONTROL_26949" => {
                        let resp = ui.interact(r, egui::Id::new("archonia-character-close"), egui::Sense::click());
                        if let Some(tex) = button_texture(c).map(str::to_string) {
                            control_image(ctx, hud, assets, &painter, r, c.size, &tex, 1.0);
                        }
                        if resp.clicked() {
                            *open = false;
                        }
                    }
                    8 => {
                        let Some(slot) = c.grid_name.as_deref().and_then(|g| SLOTS.iter().find(|(n, _)| *n == g)).map(|(n, _)| *n) else {
                            continue;
                        };
                        let resp = ui.interact(r, egui::Id::new(("archonia-equip-slot", slot)), egui::Sense::click_and_drag());
                        if let Some(e) = slot_of.get(slot) {
                            let lifted = dragging.is_some_and(|d| d.iid == e.item.id);
                            let tint = if lifted { Color32::from_white_alpha(70) } else { Color32::WHITE };
                            icon(ctx, hud, assets, &painter, r, e.item.template_id, tint);
                            if resp.dragged() {
                                resp.dnd_set_drag_payload(ItemDrag { iid: e.item.id, tid: e.item.template_id.unwrap_or(0), worn: true, bank: false });
                            }
                            if resp.secondary_clicked() {
                                out.push(ItemUi::Activate { iid: e.item.id, worn: true });
                            }
                            if dragging.is_none() {
                                resp.clone().on_hover_text(format!("{}\nClic droit ou glisser vers l'inventaire : ranger", e.name));
                            }
                        }
                        if let Some(drag) = resp.dnd_release_payload::<ItemDrag>() {
                            if !drag.worn {
                                out.push(ItemUi::DropOnEquip(*drag));
                            }
                        }
                        let target = dragging.is_some_and(|d| !d.worn) && resp.contains_pointer();
                        if resp.hovered() || target {
                            let color = if target { Color32::from_rgb(255, 210, 110) } else { Color32::from_rgb(120, 200, 255) };
                            painter.rect_stroke(r, 2.0, egui::Stroke::new(1.0_f32, color), egui::StrokeKind::Inside);
                        }
                    }
                    _ => {}
                }
            }
        });
}
