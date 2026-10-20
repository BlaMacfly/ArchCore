//! Compétences dans les fenêtres d'Archonia : livre des compétences (`UI_Skill`), instructeur
//! (`UI_SkillBuy`) et barre de raccourcis de `UI_CharInfo` (grilles `Quick_Belt_0(L)`/`(R)` et cases
//! des potions de vie et de mana).
//!
//! Glisser une compétence du livre, ou un objet du sac, sur une case de la barre l'y range ; glisser
//! une case hors de la barre la vide. Les demandes reviennent au moteur en [`SkillUi`] (voir
//! `app_skills.rs`).

use super::items_ui::{icon as item_icon, ItemDrag};
use super::*;

/// Raccourci d'une case de la barre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    Skill(i32),
    Item(i32),
}

/// Compétence saisie dans le livre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkillDrag {
    pub tid: i32,
}

/// Case de la barre saisie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotDrag {
    pub slot: usize,
}

/// Demande des fenêtres de compétences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillUi {
    CloseBook,
    CloseTrainer,
    Cast(i32),
    SelectTrainer(i32),
    Learn(i32),
    Upgrade(i32),
    SlotSet { slot: usize, shortcut: Shortcut },
    SlotSwap(usize, usize),
    SlotClear(usize),
    SlotUse(usize),
}

/// Compétence du livre.
pub struct BookEntry {
    pub tid: i32,
    pub name: String,
    pub level: i32,
    /// Part du délai de relance restante (0 : prête) et secondes restantes.
    pub cooldown: f32,
    pub left_s: u32,
}

pub struct BookView<'a> {
    pub entries: &'a [BookEntry],
    pub points: Option<i32>,
    pub first: &'a mut usize,
}

/// Compétence enseignée par un instructeur.
pub struct TrainerEntry {
    pub tid: i32,
    pub name: String,
    /// Niveau connu (0 : pas apprise) et niveau maximal.
    pub level: i32,
    pub max_level: i32,
    /// Niveau du personnage exigé et prix de la prochaine étape (apprentissage ou amélioration).
    pub require_level: i32,
    pub cost: i64,
    /// Le personnage remplit les conditions (niveau, points, argent).
    pub possible: bool,
}

pub struct TrainerView<'a> {
    pub title: String,
    pub entries: &'a [TrainerEntry],
    pub selected: Option<i32>,
    pub points: Option<i32>,
    pub first: &'a mut usize,
}

/// Case de la barre : raccourci, nombre d'objets, part de relance restante.
#[derive(Debug, Clone, Copy, Default)]
pub struct SlotView {
    pub shortcut: Option<Shortcut>,
    pub count: Option<i32>,
    pub cooldown: f32,
    pub left_s: u32,
}

fn shortcut_icon(ctx: &egui::Context, hud: &mut HudState, assets: &Assets, painter: &egui::Painter, cell: Rect, s: Shortcut, tint: Color32) {
    match s {
        Shortcut::Item(tid) => item_icon(ctx, hud, assets, painter, cell, Some(tid), tint),
        Shortcut::Skill(tid) => skill_icon(ctx, hud, assets, painter, cell, tid, tint),
    }
}

fn skill_icon(ctx: &egui::Context, hud: &mut HudState, assets: &Assets, painter: &egui::Painter, cell: Rect, tid: i32, tint: Color32) {
    match hud.skill_icon(assets, tid).and_then(|n| texture_icon(ctx, hud, assets, &n)) {
        Some(t) => {
            painter.image(t, cell.shrink(1.0), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), tint);
        }
        None => {
            painter.rect_filled(cell.shrink(3.0), 3.0, Color32::from_rgb(40, 60, 90));
        }
    }
}

/// Voile de relance : la part restante du délai assombrit l'icône depuis le haut, avec les
/// secondes restantes au centre.
fn cooldown_veil(painter: &egui::Painter, cell: Rect, part: f32, left_s: u32) {
    if part > 0.0 {
        let h = cell.height() * part.clamp(0.0, 1.0);
        painter.rect_filled(Rect::from_min_size(cell.min, Vec2::new(cell.width(), h)), 0.0, Color32::from_black_alpha(200));
        painter.hline(cell.x_range(), cell.min.y + h, egui::Stroke::new(1.0_f32, Color32::from_rgb(255, 210, 110)));
        if left_s > 0 {
            let text = if left_s >= 60 { format!("{}m", left_s.div_ceil(60)) } else { left_s.to_string() };
            shadow_text(painter, cell.center(), Align2::CENTER_CENTER, &text, (cell.height() * 0.32).max(9.0), Color32::from_rgb(255, 235, 170));
        }
    }
}

/// Grille de deux colonnes d'éléments de 208 × 56 dans `list`, avec défilement à la molette.
fn list_cells(ui: &egui::Ui, layout: &Layout, origin: Pos2, list: &UiControl, item: &UiControl, count: usize, first: &mut usize) -> Vec<(usize, Rect)> {
    let list_rect = layout.rect(origin, list.pos, list.size);
    let rows = ((list.size[1] / item.size[1]).floor() as usize).max(1);
    let per_page = rows * 2;
    if ui.rect_contains_pointer(list_rect) {
        let wheel = ui.input(|i| i.raw_scroll_delta.y);
        if wheel < 0.0 && *first + per_page < count {
            *first += 2;
        } else if wheel > 0.0 {
            *first = first.saturating_sub(2);
        }
    }
    *first = (*first).min(count.saturating_sub(1) / 2 * 2);
    (*first..count.min(*first + per_page))
        .map(|i| {
            let k = i - *first;
            let (col, row) = (k % 2, k / 2);
            (i, layout.rect(origin, [list.pos[0] + col as f32 * (item.size[0] + 2.0), list.pos[1] + row as f32 * item.size[1]], item.size))
        })
        .collect()
}

fn frame_window<R>(ctx: &egui::Context, hud: &mut HudState, assets: &Assets, w: &UiWindow, id: &str, add: impl FnOnce(&mut egui::Ui, &mut HudState, &Layout, Pos2, &egui::Painter) -> R) {
    let layout = Layout::new(ctx.screen_rect());
    let size = Vec2::new(w.size[0], w.size[1]) * layout.scale;
    egui::Area::new(egui::Id::new(id)).default_pos(layout.window_origin(w)).movable(true).constrain(true).order(egui::Order::Middle).show(ctx, |ui| {
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let painter = ui.painter().clone();
        match w.textures.first() {
            Some((_, tex)) if hud.image(ctx, assets, tex).is_some() => control_image(ctx, hud, assets, &painter, rect, w.size, tex, 1.0),
            _ => {
                painter.rect_filled(rect, 4.0, Color32::from_black_alpha(220));
            }
        }
        add(ui, hud, &layout, rect.min, &painter);
    });
}

fn x_button(ctx: &egui::Context, ui: &mut egui::Ui, hud: &mut HudState, assets: &Assets, painter: &egui::Painter, r: Rect, c: &UiControl, id: &str) -> bool {
    let resp = ui.interact(r, egui::Id::new(id), egui::Sense::click());
    if let Some(tex) = button_texture(c).map(str::to_string) {
        control_image(ctx, hud, assets, painter, r, c.size, &tex, 1.0);
    }
    resp.clicked()
}

/// Livre des compétences : icône, nom et niveau ; clic droit pour lancer, glisser vers la barre.
pub fn draw_skill_book(ctx: &egui::Context, hud: &mut HudState, assets: &Assets, view: BookView, out: &mut Vec<SkillUi>) {
    let Some(w) = hud.windows(assets).iter().find(|w| w.name == "UI_Skill").cloned() else { return };
    let ctl = |n: &str| w.controls.iter().find(|c| c.name == n).cloned();
    let (Some(list), Some(item)) = (ctl("CONTROL_SkillList"), ctl("CONTROL_SkillListItem")) else { return };
    let back = item.textures.first().map(|(_, t)| t.clone());
    let box_tex = ctl("CONTROL_16100").and_then(|c| c.textures.first().map(|(_, t)| t.clone()));
    frame_window(ctx, hud, assets, &w, "archonia-skill-book", |ui, hud, layout, origin, painter| {
        let s = layout.scale;
        let font = (11.0 * s).max(9.0);
        for c in &w.controls {
            let r = layout.rect(origin, c.pos, c.size);
            match c.name.as_str() {
                "CONTROL_2968" => shadow_text(painter, r.left_center(), Align2::LEFT_CENTER, "Compétences", font * 1.1, Color32::from_rgb(255, 230, 160)),
                "CONTROL_21134" => {
                    if x_button(ctx, ui, hud, assets, painter, r, c, "skill-book-x") {
                        out.push(SkillUi::CloseBook);
                    }
                }
                "CONTROL_28776" => shadow_text(painter, r.left_center(), Align2::LEFT_CENTER, "Points", font, Color32::from_rgb(235, 225, 200)),
                "CONTROL_12489" => shadow_text(painter, r.center(), Align2::CENTER_CENTER, &view.points.map_or("—".into(), |p| p.to_string()), font, Color32::from_rgb(255, 230, 160)),
                _ => {}
            }
        }
        let dragging = egui::DragAndDrop::has_any_payload(ctx);
        for (i, cell) in list_cells(ui, layout, origin, &list, &item, view.entries.len(), view.first) {
            let e = &view.entries[i];
            if let Some(t) = &back {
                control_image(ctx, hud, assets, painter, cell, item.size, t, 1.0);
            }
            let icon_rect = Rect::from_min_size(cell.min + Vec2::new(3.0, 3.0) * s, Vec2::splat(50.0 * s));
            if let Some(t) = &box_tex {
                control_image(ctx, hud, assets, painter, Rect::from_min_size(cell.min + Vec2::splat(s), Vec2::splat(52.0 * s)), [52.0, 52.0], t, 1.0);
            }
            skill_icon(ctx, hud, assets, painter, icon_rect, e.tid, Color32::WHITE);
            cooldown_veil(painter, icon_rect, e.cooldown, e.left_s);
            shadow_text(painter, cell.min + Vec2::new(67.0, 10.0) * s, Align2::LEFT_TOP, &e.name, font, Color32::WHITE);
            shadow_text(painter, cell.min + Vec2::new(67.0, 30.0) * s, Align2::LEFT_TOP, &format!("Niveau {}", e.level), font, Color32::from_rgb(255, 230, 160));
            let resp = ui.interact(cell, egui::Id::new(("skill-book", e.tid)), egui::Sense::click_and_drag());
            if resp.dragged() {
                resp.dnd_set_drag_payload(SkillDrag { tid: e.tid });
            }
            if resp.secondary_clicked() || resp.double_clicked() {
                out.push(SkillUi::Cast(e.tid));
            }
            if resp.hovered() && !dragging {
                painter.rect_stroke(cell, 2.0, egui::Stroke::new(1.0_f32, Color32::from_rgb(120, 200, 255)), egui::StrokeKind::Inside);
                resp.on_hover_text(format!("{}\nClic droit ou double clic : lancer · glisser vers la barre de raccourcis", e.name));
            }
        }
        if view.entries.is_empty() {
            let r = layout.rect(origin, list.pos, list.size);
            shadow_text(painter, r.center(), Align2::CENTER_CENTER, "Aucune compétence.", font, Color32::GRAY);
        }
    });
}

/// Instructeur : compétences de sa classe, niveau requis, prix ; « Apprendre » ou « Améliorer ».
pub fn draw_trainer(ctx: &egui::Context, hud: &mut HudState, assets: &Assets, view: TrainerView, out: &mut Vec<SkillUi>) {
    let Some(w) = hud.windows(assets).iter().find(|w| w.name == "UI_SkillBuy").cloned() else { return };
    let ctl = |n: &str| w.controls.iter().find(|c| c.name == n).cloned();
    let (Some(list), Some(item)) = (ctl("CONTROL_22502"), ctl("CONTROL_10080")) else { return };
    let back = item.textures.first().map(|(_, t)| t.clone());
    let disabled = ctl("CONTROL_Disable").and_then(|c| c.textures.first().map(|(_, t)| t.clone()));
    let selected = view.entries.iter().find(|e| Some(e.tid) == view.selected);
    frame_window(ctx, hud, assets, &w, "archonia-skill-trainer", |ui, hud, layout, origin, painter| {
        let s = layout.scale;
        let font = (11.0 * s).max(9.0);
        for c in &w.controls {
            let r = layout.rect(origin, c.pos, c.size);
            match c.name.as_str() {
                "CONTROL_1140" => shadow_text(painter, r.center(), Align2::CENTER_CENTER, &view.title, font, Color32::from_rgb(255, 230, 160)),
                "CONTROL_20244" => {
                    if x_button(ctx, ui, hud, assets, painter, r, c, "skill-trainer-x") {
                        out.push(SkillUi::CloseTrainer);
                    }
                }
                "CONTROL_24921" => shadow_text(painter, r.left_center(), Align2::LEFT_CENTER, "Points", font, Color32::from_rgb(235, 225, 200)),
                "CONTROL_9296" => shadow_text(painter, r.center(), Align2::CENTER_CENTER, &view.points.map_or("—".into(), |p| p.to_string()), font, Color32::from_rgb(255, 230, 160)),
                // Bouton d'action : apprendre ou améliorer la compétence choisie.
                "CONTROL_18446" => {
                    let (label, enabled) = match selected {
                        Some(e) if e.level == 0 => ("Apprendre", e.possible),
                        Some(e) if e.level < e.max_level => ("Améliorer", e.possible),
                        Some(_) => ("Niveau max", false),
                        None => ("Apprendre", false),
                    };
                    let resp = super::npc_ui::button(ctx, ui, hud, assets, painter, r, c, label, font, enabled, "skill-trainer-do");
                    if resp.clicked() {
                        if let Some(e) = selected {
                            out.push(if e.level == 0 { SkillUi::Learn(e.tid) } else { SkillUi::Upgrade(e.tid) });
                        }
                    }
                }
                "CONTROL_17539" => {
                    if super::npc_ui::button(ctx, ui, hud, assets, painter, r, c, "Fermer", font, true, "skill-trainer-close").clicked() {
                        out.push(SkillUi::CloseTrainer);
                    }
                }
                _ => {}
            }
        }
        for (i, cell) in list_cells(ui, layout, origin, &list, &item, view.entries.len(), view.first) {
            let e = &view.entries[i];
            if let Some(t) = &back {
                control_image(ctx, hud, assets, painter, cell, item.size, t, 1.0);
            }
            let icon_rect = Rect::from_min_size(cell.min + Vec2::new(3.0, 3.0) * s, Vec2::splat(50.0 * s));
            skill_icon(ctx, hud, assets, painter, icon_rect, e.tid, Color32::WHITE);
            let state = if e.level == 0 { format!("Niv. requis {}", e.require_level) } else { format!("Niveau {}/{}", e.level, e.max_level) };
            shadow_text(painter, cell.min + Vec2::new(62.0, 8.0) * s, Align2::LEFT_TOP, &e.name, font, Color32::WHITE);
            shadow_text(painter, cell.min + Vec2::new(62.0, 24.0) * s, Align2::LEFT_TOP, &state, font * 0.95, Color32::from_rgb(255, 230, 160));
            if e.level < e.max_level {
                shadow_text(painter, cell.min + Vec2::new(62.0, 38.0) * s, Align2::LEFT_TOP, &format!("{} or", format_money(e.cost)), font * 0.95, Color32::from_rgb(255, 220, 120));
            }
            if !e.possible {
                if let Some(t) = &disabled {
                    control_image(ctx, hud, assets, painter, Rect::from_min_size(cell.min + Vec2::splat(2.0 * s), Vec2::new(196.0, 52.0) * s), [196.0, 52.0], t, 1.0);
                }
            }
            let resp = ui.interact(cell, egui::Id::new(("skill-trainer", e.tid)), egui::Sense::click());
            if resp.clicked() {
                out.push(SkillUi::SelectTrainer(e.tid));
            }
            if Some(e.tid) == view.selected {
                painter.rect_stroke(cell, 2.0, egui::Stroke::new(1.5_f32, Color32::from_rgb(255, 210, 110)), egui::StrokeKind::Inside);
            } else if resp.hovered() {
                painter.rect_stroke(cell, 2.0, egui::Stroke::new(1.0_f32, Color32::from_rgb(120, 200, 255)), egui::StrokeKind::Inside);
            }
        }
        if view.entries.is_empty() {
            let r = layout.rect(origin, list.pos, list.size);
            shadow_text(painter, r.center(), Align2::CENTER_CENTER, "Aucune compétence à apprendre ici.", font, Color32::GRAY);
        }
    });
}

/// Barre de raccourcis : douze cases posées sur les grilles de `UI_CharInfo`.
pub fn draw_quickbar(ctx: &egui::Context, hud: &mut HudState, assets: &Assets, slots: &[SlotView; 12], out: &mut Vec<SkillUi>) {
    let Some(w) = hud.windows(assets).iter().find(|w| w.name == "UI_CharInfo").cloned() else { return };
    let layout = Layout::new(ctx.screen_rect());
    let origin = layout.window_origin(&w);
    let s = layout.scale;
    // Cases : potions (1, 2), puis 5 à gauche et 5 à droite.
    let mut cells: Vec<Rect> = Vec::new();
    for name in ["CONTROL_20778", "CONTROL_28144"] {
        if let Some(c) = w.controls.iter().find(|c| c.name == name) {
            cells.push(layout.rect(origin, c.pos, c.size));
        }
    }
    for name in ["CONTROL_GRID_LEFT_0", "CONTROL_GRID_RIGHT_0"] {
        if let Some(c) = w.controls.iter().find(|c| c.name == name) {
            let [gx, _, cw, ch] = c.grid.unwrap_or([2.0, 0.0, 50.0, 50.0]);
            for k in 0..5 {
                cells.push(layout.rect(origin, [c.pos[0] + k as f32 * (cw + gx), c.pos[1]], [cw, ch]));
            }
        }
    }
    if cells.len() != 12 {
        return;
    }
    let font = (10.0 * s).max(8.0);
    let item_drag = egui::DragAndDrop::payload::<ItemDrag>(ctx).map(|d| *d);
    let skill_drag = egui::DragAndDrop::payload::<SkillDrag>(ctx).map(|d| *d);
    let slot_drag = egui::DragAndDrop::payload::<SlotDrag>(ctx).map(|d| *d);
    for (slot, cell) in cells.iter().enumerate() {
        egui::Area::new(egui::Id::new(("archrust-quickbar", slot))).fixed_pos(cell.min).order(egui::Order::Foreground).show(ctx, |ui| {
            let (_, resp) = ui.allocate_exact_size(cell.size(), egui::Sense::click_and_drag());
            let painter = ui.painter();
            let v = slots[slot];
            if let Some(sc) = v.shortcut {
                let lifted = slot_drag.is_some_and(|d| d.slot == slot);
                shortcut_icon(ctx, hud, assets, painter, *cell, sc, if lifted { Color32::from_white_alpha(70) } else { Color32::WHITE });
                cooldown_veil(painter, *cell, v.cooldown, v.left_s);
                if let Some(n) = v.count {
                    shadow_text(painter, cell.right_bottom() - Vec2::new(2.0, 1.0) * s, Align2::RIGHT_BOTTOM, &n.to_string(), font, Color32::WHITE);
                }
                if resp.dragged() {
                    resp.dnd_set_drag_payload(SlotDrag { slot });
                }
                if resp.clicked() {
                    out.push(SkillUi::SlotUse(slot));
                }
            }
            if let Some(d) = resp.dnd_release_payload::<SkillDrag>() {
                out.push(SkillUi::SlotSet { slot, shortcut: Shortcut::Skill(d.tid) });
            } else if let Some(d) = resp.dnd_release_payload::<ItemDrag>() {
                if !d.bank {
                    out.push(SkillUi::SlotSet { slot, shortcut: Shortcut::Item(d.tid) });
                }
            } else if let Some(d) = resp.dnd_release_payload::<SlotDrag>() {
                if d.slot != slot {
                    out.push(SkillUi::SlotSwap(d.slot, slot));
                }
            }
            let target = (item_drag.is_some() || skill_drag.is_some() || slot_drag.is_some()) && resp.contains_pointer();
            if target {
                painter.rect_stroke(*cell, 2.0, egui::Stroke::new(1.5_f32, Color32::from_rgb(255, 210, 110)), egui::StrokeKind::Inside);
            }
        });
    }
    // Case glissée hors de la barre : vidée.
    if ctx.input(|i| i.pointer.any_released()) {
        if let Some(d) = egui::DragAndDrop::take_payload::<SlotDrag>(ctx) {
            let over_bar = ctx.pointer_latest_pos().is_some_and(|p| cells.iter().any(|c| c.contains(p)));
            if !over_bar {
                out.push(SkillUi::SlotClear(d.slot));
            }
        }
    }
}
