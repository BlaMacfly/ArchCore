//! Fenêtres des PNJ d'Archonia : dialogue (`UI_NPCDialog`), boutique (`UI_NPCTrade`), banque
//! (`UI_Bank`), quêtes d'un PNJ (`QuestAccept_UI`) et journal des quêtes (`QuestMain_UI`).
//!
//! Fonds, boutons et grilles viennent d'`UI_1024X768.ini` ; le texte et les listes sont dessinés
//! dans les zones prévues. Les demandes reviennent au moteur en [`NpcUi`] (voir `app_npc.rs`).

use super::items_ui::ItemDrag;
use super::*;

/// Demande d'une fenêtre de PNJ.
#[derive(Debug, Clone, PartialEq)]
pub enum NpcUi {
    CloseDialog,
    /// Bouton du dialogue : fonction du PNJ (9 boutique, 20 banque, 27 quêtes…).
    Function(i32),
    CloseShop,
    /// Achat (clic droit : un exemplaire ; Maj + clic droit : quantité à choisir).
    Buy { iid: i32, ask_count: bool },
    /// Objet du sac lâché sur la boutique : vente.
    Sell(i32),
    CloseBank,
    /// Objet lâché sur une case de la banque (du sac : dépôt ; de la banque : déplacement).
    BankDrop { drag: ItemDrag, grid: [i16; 3] },
    /// Clic droit sur un objet de la banque : retrait vers le sac.
    BankTake(i32),
    /// Bouton « Déposer » ou « Retirer » de l'argent.
    BankMoney { deposit: bool },
    CloseQuests,
    SelectQuest(i32),
    QuestAccept(i32),
    QuestComplete(i32),
    QuestAbandon(i32),
}

/// Dialogue ouvert.
pub struct DialogView {
    pub npc_name: String,
    pub text: String,
    /// Fonctions proposées (fonction, libellé).
    pub buttons: Vec<(i32, String)>,
}

/// Objet de la boutique.
pub struct ShopEntry {
    pub iid: i32,
    pub tid: i32,
    pub price: i64,
    pub count: i32,
    pub grid: [i16; 3],
    pub name: String,
}

pub struct ShopView<'a> {
    pub npc_name: &'a str,
    pub items: &'a [ShopEntry],
    pub loading: bool,
    /// Première ligne affichée (défilement).
    pub first_row: &'a mut i16,
}

pub struct BankView<'a> {
    /// Objets en banque (statut 100) et leur nom.
    pub items: &'a [(ItemRecord, String)],
    pub money: Option<i64>,
    /// Pages ouvertes (la première toujours).
    pub pages: i16,
    pub page: &'a mut i16,
}

/// État d'une quête dans une liste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestState {
    /// Proposée par le PNJ.
    Available,
    /// Au journal, objectifs en cours.
    InProgress,
    /// Au journal, objectifs remplis : à rendre.
    Ready,
    /// Déjà terminée.
    Done,
}

pub struct QuestEntry {
    pub tid: i32,
    pub name: String,
    pub state: QuestState,
}

pub struct QuestView<'a> {
    /// Fenêtre d'un PNJ (`QuestAccept_UI`) ou journal (`QuestMain_UI`).
    pub journal: bool,
    pub title: String,
    pub entries: &'a [QuestEntry],
    pub selected: Option<i32>,
    pub text: String,
}

fn window(hud: &mut HudState, assets: &Assets, name: &str) -> Option<UiWindow> {
    hud.windows(assets).iter().find(|w| w.name == name).cloned()
}

/// Bouton d'Archonia : textures « a » (repos), « b » (survol), « c » (appui), « d » (inactif).
#[allow(clippy::too_many_arguments)]
pub(super) fn button(ctx: &egui::Context, ui: &mut egui::Ui, hud: &mut HudState, assets: &Assets, painter: &egui::Painter, r: Rect, c: &UiControl, label: &str, font: f32, enabled: bool, id: impl std::hash::Hash) -> egui::Response {
    let resp = ui.interact(r, egui::Id::new(id), if enabled { egui::Sense::click() } else { egui::Sense::hover() });
    let pick = |suffix: &str| c.textures.iter().find(|(_, n)| n.to_ascii_lowercase().ends_with(suffix)).map(|(_, n)| n.clone());
    let tex = if !enabled {
        pick("d.png")
    } else if resp.is_pointer_button_down_on() {
        pick("c.png")
    } else if resp.hovered() {
        pick("b.png")
    } else {
        pick("a.png")
    }
    .or_else(|| button_texture(c).map(str::to_string));
    match tex {
        Some(t) if hud.image(ctx, assets, &t).is_some() => control_image(ctx, hud, assets, painter, r, c.size, &t, 1.0),
        _ => {
            painter.rect_filled(r, 3.0, if resp.hovered() && enabled { Color32::from_rgb(110, 80, 40) } else { Color32::from_rgb(70, 50, 30) });
        }
    }
    if !label.is_empty() {
        shadow_text(painter, r.center(), Align2::CENTER_CENTER, label, font, if enabled { Color32::WHITE } else { Color32::GRAY });
    }
    resp
}

/// Zone de texte défilante dans le rectangle `r`.
fn text_box(ui: &mut egui::Ui, r: Rect, id: &str, text: &str, font: f32, color: Color32) {
    ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
        egui::ScrollArea::vertical().id_salt(id).max_height(r.height()).show(ui, |ui| {
            ui.set_width(r.width() - 12.0);
            ui.label(egui::RichText::new(text).size(font).color(color).strong());
        });
    });
}

/// Fenêtre déplaçable aux dimensions d'Archonia, avec son fond.
fn frame<R>(ctx: &egui::Context, hud: &mut HudState, assets: &Assets, w: &UiWindow, id: &str, add: impl FnOnce(&mut egui::Ui, &mut HudState, &Layout, Pos2, &egui::Painter) -> R) -> Option<R> {
    let layout = Layout::new(ctx.screen_rect());
    let size = Vec2::new(w.size[0], w.size[1]) * layout.scale;
    egui::Area::new(egui::Id::new(id))
        .default_pos(layout.window_origin(w))
        .movable(true)
        .constrain(true)
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            let painter = ui.painter().clone();
            match w.textures.first() {
                Some((_, tex)) if hud.image(ctx, assets, tex).is_some() => control_image(ctx, hud, assets, &painter, rect, w.size, tex, 1.0),
                _ => {
                    painter.rect_filled(rect, 4.0, Color32::from_black_alpha(220));
                }
            }
            add(ui, hud, &layout, rect.min, &painter)
        })
        .inner
        .into()
}

/// Dialogue d'un PNJ : texte, boutons des fonctions, fermeture.
pub fn draw_npc_dialog(ctx: &egui::Context, hud: &mut HudState, assets: &Assets, view: &DialogView, out: &mut Vec<NpcUi>) {
    let Some(w) = window(hud, assets, "UI_NPCDialog") else { return };
    frame(ctx, hud, assets, &w, "archonia-npc-dialog", |ui, hud, layout, origin, painter| {
        let s = layout.scale;
        let font = (11.0 * s).max(9.0);
        for c in &w.controls {
            let r = layout.rect(origin, c.pos, c.size);
            match c.name.as_str() {
                "CONTROL_31156" => shadow_text(painter, r.left_center(), Align2::LEFT_CENTER, &view.npc_name, font, Color32::from_rgb(255, 230, 160)),
                "CONTROL_NPCDialogEdit" => text_box(ui, r, "npc-dialog-text", &view.text, font * 1.05, Color32::from_rgb(230, 222, 200)),
                "CONTROL_NPCDialogQuitButton" => {
                    if button(ctx, ui, hud, assets, painter, r, c, "", font, true, "npc-dialog-x").clicked() {
                        out.push(NpcUi::CloseDialog);
                    }
                }
                "CONTROL_NPCDialogButton" => {
                    let mut labels: Vec<(Option<i32>, &str)> = view.buttons.iter().map(|(f, l)| (Some(*f), l.as_str())).collect();
                    labels.push((None, "Au revoir"));
                    for (i, (f, label)) in labels.into_iter().enumerate() {
                        let rr = layout.rect(origin, [c.pos[0], c.pos[1] + i as f32 * 28.0], c.size);
                        if button(ctx, ui, hud, assets, painter, rr, c, label, font, true, ("npc-dialog-fn", i)).clicked() {
                            out.push(f.map_or(NpcUi::CloseDialog, NpcUi::Function));
                        }
                    }
                }
                _ => {}
            }
        }
    });
}

/// Grille d'objets (boutique ou banque) : rend la case (sac, ligne, colonne) de chaque case dessinée.
fn grid_cells(c: &UiControl, layout: &Layout, origin: Pos2, page: i16, first_row: i16) -> Vec<(Rect, [i16; 3])> {
    let [gx, gy, cw, ch] = c.grid.unwrap_or([2.0, 2.0, 50.0, 50.0]);
    let cols = ((c.size[0] + gx) / (cw + gx)).floor().max(1.0) as i16;
    let rows = ((c.size[1] + gy) / (ch + gy)).floor().max(1.0) as i16;
    (0..rows)
        .flat_map(|row| (0..cols).map(move |col| (row, col)))
        .map(|(row, col)| (layout.rect(origin, [c.pos[0] + col as f32 * (cw + gx), c.pos[1] + row as f32 * (ch + gy)], [cw, ch]), [page, row + first_row, col]))
        .collect()
}

/// Boutique d'un PNJ : objets et prix (clic droit : acheter ; objet du sac lâché ici : vendre).
pub fn draw_shop(ctx: &egui::Context, hud: &mut HudState, assets: &Assets, view: ShopView, out: &mut Vec<NpcUi>) {
    let Some(w) = window(hud, assets, "UI_NPCTrade") else { return };
    let money = hud.stats.money;
    frame(ctx, hud, assets, &w, "archonia-npc-shop", |ui, hud, layout, origin, painter| {
        let s = layout.scale;
        let font = (11.0 * s).max(9.0);
        let dragging = egui::DragAndDrop::payload::<ItemDrag>(ctx).map(|d| *d);
        let shift = ui.input(|i| i.modifiers.shift);
        for c in &w.controls {
            let r = layout.rect(origin, c.pos, c.size);
            match c.name.as_str() {
                "CONTROL_23250" => shadow_text(painter, r.left_center(), Align2::LEFT_CENTER, &format!("Boutique — {}", view.npc_name), font, Color32::from_rgb(255, 230, 160)),
                "CONTROL_8009" => {
                    if button(ctx, ui, hud, assets, painter, r, c, "", font, true, "npc-shop-x").clicked() {
                        out.push(NpcUi::CloseShop);
                    }
                }
                "CONTROL_30471" => {
                    if button(ctx, ui, hud, assets, painter, r, c, "Fermer", font, true, "npc-shop-close").clicked() {
                        out.push(NpcUi::CloseShop);
                    }
                }
                "CONTROL_20194" | "CONTROL_23924" => {
                    let up = c.name == "CONTROL_20194";
                    if button(ctx, ui, hud, assets, painter, r, c, "", font, true, ("npc-shop-page", up)).clicked() {
                        *view.first_row = if up { (*view.first_row - 7).max(0) } else { (*view.first_row + 7).min(13) };
                    }
                }
                "CONTROL_NPCTradeGrid" => {
                    for (cell, grid) in grid_cells(c, layout, origin, 0, *view.first_row) {
                        let resp = ui.interact(cell, egui::Id::new(("npc-shop-cell", grid)), egui::Sense::click());
                        if let Some(e) = view.items.iter().find(|e| e.grid == grid) {
                            items_ui::icon(ctx, hud, assets, painter, cell, Some(e.tid), Color32::WHITE);
                            if e.count > 1 {
                                shadow_text(painter, cell.right_bottom() - Vec2::new(3.0, 2.0) * s, Align2::RIGHT_BOTTOM, &e.count.to_string(), font, Color32::WHITE);
                            }
                            if resp.secondary_clicked() {
                                out.push(NpcUi::Buy { iid: e.iid, ask_count: shift });
                            }
                            if dragging.is_none() {
                                resp.clone().on_hover_text(format!("{}\nPrix : {} pièce(s)\nClic droit : acheter · Maj + clic droit : quantité", e.name, format_money(e.price)));
                            }
                        }
                        if let Some(drag) = resp.dnd_release_payload::<ItemDrag>() {
                            if !drag.worn && !drag.bank {
                                out.push(NpcUi::Sell(drag.iid));
                            }
                        }
                        let target = dragging.is_some_and(|d| !d.worn && !d.bank) && resp.contains_pointer();
                        if resp.hovered() || target {
                            let color = if target { Color32::from_rgb(255, 210, 110) } else { Color32::from_rgb(120, 200, 255) };
                            painter.rect_stroke(cell, 2.0, egui::Stroke::new(1.0_f32, color), egui::StrokeKind::Inside);
                        }
                    }
                    if view.loading {
                        shadow_text(painter, r.center(), Align2::CENTER_CENTER, "Chargement…", font, Color32::WHITE);
                    }
                }
                _ => {}
            }
        }
        // Or du joueur, sous la grille.
        let at = layout.rect(origin, [14.0, 415.0], [200.0, 16.0]);
        shadow_text(painter, at.left_center(), Align2::LEFT_CENTER, &format!("Or : {}", format_money(money.unwrap_or(0))), font, Color32::from_rgb(255, 220, 120));
    });
}

/// Banque : quatre pages de 7 × 4 cases, argent, dépôt et retrait.
pub fn draw_bank(ctx: &egui::Context, hud: &mut HudState, assets: &Assets, view: BankView, out: &mut Vec<NpcUi>) {
    let Some(w) = window(hud, assets, "UI_Bank") else { return };
    frame(ctx, hud, assets, &w, "archonia-bank", |ui, hud, layout, origin, painter| {
        let s = layout.scale;
        let font = (11.0 * s).max(9.0);
        let dragging = egui::DragAndDrop::payload::<ItemDrag>(ctx).map(|d| *d);
        let tabs = ["CONTROL_6581", "CONTROL_8957", "CONTROL_3703", "CONTROL_830530"];
        for c in &w.controls {
            let r = layout.rect(origin, c.pos, c.size);
            if let Some(page) = tabs.iter().position(|t| *t == c.name).map(|p| p as i16) {
                let open = page < view.pages;
                let selected = page == *view.page;
                let tex = c.textures.iter().find(|(i, _)| *i == if !open { 3 } else if selected { 2 } else { 1 }).or(c.textures.first()).map(|(_, n)| n.clone());
                if let Some(t) = tex {
                    control_image(ctx, hud, assets, painter, r, c.size, &t, 1.0);
                }
                let color = if !open { Color32::GRAY } else if selected { Color32::from_rgb(255, 230, 160) } else { Color32::from_rgb(190, 190, 190) };
                shadow_text(painter, r.center(), Align2::CENTER_CENTER, &(page + 1).to_string(), font, color);
                let resp = ui.interact(r, egui::Id::new(("bank-tab", page)), if open { egui::Sense::click() } else { egui::Sense::hover() });
                if resp.clicked() {
                    *view.page = page;
                }
                if !open {
                    resp.on_hover_text("Page fermée (agrandissement de la banque pas encore disponible)");
                }
                continue;
            }
            match c.name.as_str() {
                "CONTROL_32725" => shadow_text(painter, r.left_center(), Align2::LEFT_CENTER, "Banque", font, Color32::from_rgb(255, 230, 160)),
                "CONTROL_20738" => {
                    if button(ctx, ui, hud, assets, painter, r, c, "", font, true, "bank-x").clicked() {
                        out.push(NpcUi::CloseBank);
                    }
                }
                "CONTROL_28474" => {
                    let text = view.money.map_or("—".to_string(), format_money);
                    shadow_text(painter, r.right_center(), Align2::RIGHT_CENTER, &text, font, Color32::from_rgb(255, 220, 120));
                }
                "CONTROL_481" | "CONTROL_20471" => {
                    let deposit = c.name == "CONTROL_481";
                    let label = if deposit { "Déposer" } else { "Retirer" };
                    if button(ctx, ui, hud, assets, painter, r, c, label, font * 0.95, true, ("bank-money", deposit)).clicked() {
                        out.push(NpcUi::BankMoney { deposit });
                    }
                }
                "CONTROL_26485" => {
                    button(ctx, ui, hud, assets, painter, r, c, "Agrandir", font * 0.95, false, "bank-expand").on_hover_text("Pas encore disponible");
                }
                "CONTROL_16826" => {
                    for (cell, grid) in grid_cells(c, layout, origin, *view.page, 0) {
                        let resp = ui.interact(cell, egui::Id::new(("bank-cell", grid)), egui::Sense::click_and_drag());
                        if let Some((item, name)) = view.items.iter().find(|(i, _)| i.grid == Some(grid)) {
                            let lifted = dragging.is_some_and(|d| d.iid == item.id);
                            items_ui::icon(ctx, hud, assets, painter, cell, item.template_id, if lifted { Color32::from_white_alpha(70) } else { Color32::WHITE });
                            let count = item.count.unwrap_or(1);
                            if count > 1 && !lifted {
                                shadow_text(painter, cell.right_bottom() - Vec2::new(3.0, 2.0) * s, Align2::RIGHT_BOTTOM, &count.to_string(), font, Color32::WHITE);
                            }
                            if resp.dragged() {
                                resp.dnd_set_drag_payload(ItemDrag { iid: item.id, tid: item.template_id.unwrap_or(0), worn: false, bank: true });
                            }
                            if resp.secondary_clicked() {
                                out.push(NpcUi::BankTake(item.id));
                            }
                            if dragging.is_none() {
                                let label = if count > 1 { format!("{name} ×{count}") } else { name.clone() };
                                resp.clone().on_hover_text(format!("{label}\nClic droit ou glisser vers l'inventaire : retirer"));
                            }
                        }
                        if let Some(drag) = resp.dnd_release_payload::<ItemDrag>() {
                            if !drag.worn {
                                out.push(NpcUi::BankDrop { drag: *drag, grid });
                            }
                        }
                        let target = dragging.is_some_and(|d| !d.worn) && resp.contains_pointer();
                        if resp.hovered() || target {
                            let color = if target { Color32::from_rgb(255, 210, 110) } else { Color32::from_rgb(120, 200, 255) };
                            painter.rect_stroke(cell, 2.0, egui::Stroke::new(1.0_f32, color), egui::StrokeKind::Inside);
                        }
                    }
                }
                _ => {}
            }
        }
    });
}

/// Quêtes d'un PNJ ou journal : liste à gauche, texte de la quête choisie à droite, boutons.
pub fn draw_quests(ctx: &egui::Context, hud: &mut HudState, assets: &Assets, view: QuestView, out: &mut Vec<NpcUi>) {
    let name = if view.journal { "QuestMain_UI" } else { "QuestAccept_UI" };
    let Some(w) = window(hud, assets, name) else { return };
    let id = if view.journal { "archonia-quest-journal" } else { "archonia-quest-npc" };
    frame(ctx, hud, assets, &w, id, |ui, hud, layout, origin, painter| {
        let s = layout.scale;
        let font = (11.0 * s).max(9.0);
        let selected = view.entries.iter().find(|e| Some(e.tid) == view.selected);
        let state = selected.map(|e| e.state);
        for c in &w.controls {
            let r = layout.rect(origin, c.pos, c.size);
            match c.name.as_str() {
                "CONTROL_16487" | "CTL_ST_TITLE" => shadow_text(painter, r.center(), Align2::CENTER_CENTER, &view.title, font * 1.1, Color32::from_rgb(255, 230, 160)),
                "CONTROL_QUEST_EXIT" => {
                    if button(ctx, ui, hud, assets, painter, r, c, "", font, true, (id, "x")).clicked() {
                        out.push(NpcUi::CloseQuests);
                    }
                }
                "CONTROL_QUEST_CLOSE" => {
                    if button(ctx, ui, hud, assets, painter, r, c, "Fermer", font, true, (id, "close")).clicked() {
                        out.push(NpcUi::CloseQuests);
                    }
                }
                // Fenêtre d'un PNJ : « Accepter » (quête proposée) ou « Terminer » (quête à rendre).
                "CONTROL_QUEST_ACCEPT_NEW" if !view.journal => {
                    let (label, enabled) = match state {
                        Some(QuestState::Ready | QuestState::InProgress) => ("Terminer", true),
                        Some(QuestState::Available) => ("Accepter", true),
                        _ => ("Accepter", false),
                    };
                    if button(ctx, ui, hud, assets, painter, r, c, label, font, enabled, (id, "accept")).clicked() {
                        if let Some(e) = selected {
                            out.push(if e.state == QuestState::Available { NpcUi::QuestAccept(e.tid) } else { NpcUi::QuestComplete(e.tid) });
                        }
                    }
                }
                // Journal : « Abandonner » et « Terminer » (quêtes personnelles).
                "CONTROL_QUEST_DEL" if view.journal => {
                    let enabled = matches!(state, Some(QuestState::InProgress | QuestState::Ready));
                    if button(ctx, ui, hud, assets, painter, r, c, "Abandonner", font, enabled, (id, "del")).clicked() {
                        if let Some(e) = selected {
                            out.push(NpcUi::QuestAbandon(e.tid));
                        }
                    }
                }
                "CONTROL_QUEST_SHARE" if view.journal => {
                    let enabled = matches!(state, Some(QuestState::Ready));
                    if button(ctx, ui, hud, assets, painter, r, c, "Terminer", font, enabled, (id, "done")).clicked() {
                        if let Some(e) = selected {
                            out.push(NpcUi::QuestComplete(e.tid));
                        }
                    }
                }
                "CONTROL_QUEST_TREE" => {
                    ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
                        egui::ScrollArea::vertical().id_salt((id, "tree")).max_height(r.height()).show(ui, |ui| {
                            ui.set_width(r.width() - 24.0 * s);
                            if view.entries.is_empty() {
                                ui.label(egui::RichText::new("Aucune quête.").size(font).color(Color32::GRAY));
                            }
                            for e in view.entries {
                                let (mark, color) = match e.state {
                                    QuestState::Available => ("◆", Color32::from_rgb(255, 220, 120)),
                                    QuestState::InProgress => ("…", Color32::from_rgb(200, 200, 200)),
                                    QuestState::Ready => ("✔", Color32::from_rgb(140, 230, 120)),
                                    QuestState::Done => ("✓", Color32::GRAY),
                                };
                                let text = egui::RichText::new(format!("{mark} {}", e.name)).size(font * 1.05).color(color);
                                if ui.selectable_label(Some(e.tid) == view.selected, text).clicked() {
                                    out.push(NpcUi::SelectQuest(e.tid));
                                }
                            }
                        });
                    });
                }
                // Texte sur parchemin : brun foncé.
                "CONTROL_QUEST_TEXT" => text_box(ui, r.shrink(8.0 * s), &format!("{id}-text"), &view.text, font * 1.05, Color32::from_rgb(35, 22, 8)),
                _ => {}
            }
        }
    });
}
