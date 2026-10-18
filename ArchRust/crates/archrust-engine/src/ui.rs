//! Interface : panneaux egui du client (connexion, discussion, console, informations) et rendu
//! des frames Lua des AddOns (rectangles, textes, barres) d'après leur mise en page.

use archrust_addons::ui::{FrameKind, Rect, UiTree};
use archrust_addons::AddonManager;
use archrust_netclient::world::ItemRecord;
use egui::{Align2, Color32, FontId, Pos2, Stroke};

/// État des panneaux egui.
pub struct UiState {
    pub host: String,
    pub port: String,
    pub user: String,
    pub password: String,
    pub character: String,
    pub create_if_missing: bool,
    pub status: String,
    pub chat_lines: Vec<String>,
    pub chat_input: String,
    pub console_open: bool,
    pub console_input: String,
    pub show_addons: bool,
    pub show_entities: bool,
    pub show_inventory: bool,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: "11002".into(),
            user: "test".into(),
            password: "test".into(),
            character: "Testeur".into(),
            create_if_missing: true,
            status: String::new(),
            chat_lines: Vec::new(),
            chat_input: String::new(),
            console_open: false,
            console_input: String::new(),
            show_addons: false,
            show_entities: true,
            show_inventory: false,
        }
    }
}

/// Fenêtre d'inventaire alimentée par les paquets AgpmItem reçus du serveur.
pub fn inventory_panel(ctx: &egui::Context, st: &mut UiState, items: &[(ItemRecord, String)]) {
    egui::Area::new(egui::Id::new("inventory-toggle"))
        .anchor(Align2::RIGHT_TOP, [-10.0, 10.0])
        .show(ctx, |ui| {
            if ui
                .button(format!(
                    "Inventaire ({})",
                    items.iter().filter(|(i, _)| i.status == Some(2)).count()
                ))
                .clicked()
            {
                st.show_inventory = !st.show_inventory;
            }
        });
    if !st.show_inventory {
        return;
    }
    egui::Window::new("Inventaire")
        .open(&mut st.show_inventory)
        .default_width(300.0)
        .show(ctx, |ui| {
            let inventory: Vec<_> = items
                .iter()
                .filter(|(i, _)| matches!(i.status, Some(2 | 3)))
                .collect();
            if inventory.is_empty() {
                ui.label("Aucun objet reçu du serveur.");
            }
            egui::Grid::new("inventory-grid")
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Objet");
                    ui.strong("Qté");
                    ui.strong("Emplacement");
                    ui.end_row();
                    for (item, name) in inventory {
                        ui.label(format!("{name} (ID {})", item.id));
                        ui.label(
                            item.count
                                .map(|n| n.to_string())
                                .unwrap_or_else(|| "—".into()),
                        );
                        ui.label(match item.status {
                            Some(3) => "Équipé".into(),
                            _ => item
                                .grid
                                .map(|[tab, row, col]| format!("{tab} / {row} / {col}"))
                                .unwrap_or_else(|| "—".into()),
                        });
                        ui.end_row();
                    }
                });
        });
}

impl UiState {
    pub fn push_chat(&mut self, line: impl Into<String>) {
        self.chat_lines.push(line.into());
        if self.chat_lines.len() > 200 {
            self.chat_lines.remove(0);
        }
    }
}

/// Résultat des interactions de l'image.
#[derive(Default)]
pub struct UiActions {
    pub connect: bool,
    pub chat: Option<String>,
    pub console: Option<String>,
    pub clicked_frames: Vec<usize>,
}

/// Dessine les frames Lua. `tree` est l'arbre des AddOns, `screen` la taille de l'écran.
pub fn draw_addon_frames(
    ctx: &egui::Context,
    tree: &UiTree,
    mgr: &AddonManager,
    actions: &mut UiActions,
) {
    let screen = ctx.screen_rect();
    let (sw, sh) = (screen.width(), screen.height());
    let rects = tree.layout(sw, sh);
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::new("addon-frames"),
    ));
    let pointer = ctx.input(|i| i.pointer.interact_pos());
    let clicked = ctx.input(|i| i.pointer.primary_clicked());
    for f in &tree.frames {
        if f.name.as_deref() == Some("UIParent") || !tree.is_visible(f.id) {
            continue;
        }
        let Some(r) = rects[f.id] else { continue };
        // Conversion repère WoW (origine bas-gauche) → egui (origine haut-gauche).
        let rect = egui::Rect::from_min_size(
            Pos2::new(r.left, sh - r.bottom - r.height),
            egui::vec2(r.width, r.height),
        );
        let alpha = (f.alpha * 255.0) as u8;
        let col = |c: [f32; 4]| {
            Color32::from_rgba_unmultiplied(
                (c[0] * 255.0) as u8,
                (c[1] * 255.0) as u8,
                (c[2] * 255.0) as u8,
                ((c[3] * f.alpha) * 255.0) as u8,
            )
        };
        match f.kind {
            FrameKind::FontString => {
                let anchor = match f.justify_h.as_str() {
                    "CENTER" => Align2::CENTER_CENTER,
                    "RIGHT" => Align2::RIGHT_CENTER,
                    _ => Align2::LEFT_CENTER,
                };
                let pos = match anchor {
                    Align2::CENTER_CENTER => rect.center(),
                    Align2::RIGHT_CENTER => rect.right_center(),
                    _ => rect.left_center(),
                };
                let text = strip_color_codes(&f.text);
                painter.text(
                    pos + egui::vec2(1.0, 1.0),
                    anchor,
                    &text,
                    FontId::proportional(f.font_size),
                    Color32::from_black_alpha(alpha),
                );
                painter.text(
                    pos,
                    anchor,
                    &text,
                    FontId::proportional(f.font_size),
                    col(f.color),
                );
            }
            FrameKind::Texture => {
                if f.color[3] > 0.0 {
                    painter.rect_filled(rect, 2.0, col(f.color));
                }
            }
            FrameKind::StatusBar => {
                painter.rect_filled(rect, 2.0, Color32::from_black_alpha(alpha / 2));
                let frac = if f.max_value > f.min_value {
                    ((f.value - f.min_value) / (f.max_value - f.min_value)).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let mut fill = rect;
                fill.set_width(rect.width() * frac);
                painter.rect_filled(fill, 2.0, col(f.color));
            }
            FrameKind::Button | FrameKind::CheckButton => {
                let hovered = pointer.map(|p| rect.contains(p)).unwrap_or(false);
                let base = if hovered {
                    Color32::from_rgba_unmultiplied(90, 70, 30, alpha)
                } else {
                    Color32::from_rgba_unmultiplied(60, 45, 20, alpha)
                };
                painter.rect_filled(rect, 4.0, base);
                painter.rect_stroke(
                    rect,
                    4.0,
                    Stroke::new(1.0, Color32::from_rgba_unmultiplied(200, 170, 90, alpha)),
                    egui::StrokeKind::Inside,
                );
                if !f.text.is_empty() {
                    painter.text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        strip_color_codes(&f.text),
                        FontId::proportional(f.font_size),
                        Color32::from_rgba_unmultiplied(255, 220, 120, alpha),
                    );
                }
                if f.kind == FrameKind::CheckButton && f.checked {
                    painter.text(
                        rect.left_center() + egui::vec2(4.0, 0.0),
                        Align2::LEFT_CENTER,
                        "✔",
                        FontId::proportional(f.font_size),
                        Color32::from_rgba_unmultiplied(120, 255, 120, alpha),
                    );
                }
                if hovered && clicked {
                    actions.clicked_frames.push(f.id);
                }
            }
            _ => {
                if f.color[3] > 0.0 {
                    painter.rect_filled(rect, 4.0, col(f.color));
                }
                if f.mouse_enabled {
                    let hovered = pointer.map(|p| rect.contains(p)).unwrap_or(false);
                    if hovered && clicked {
                        actions.clicked_frames.push(f.id);
                    }
                }
            }
        }
    }
    let _ = mgr;
}

/// Retire les codes de couleur WoW (`|cffRRGGBB…|r`) et `|n`.
pub fn strip_color_codes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '|' {
            match chars.next() {
                Some('c') => {
                    for _ in 0..8 {
                        chars.next();
                    }
                }
                Some('r') => {}
                Some('n') => out.push('\n'),
                Some('|') => out.push('|'),
                Some(o) => {
                    out.push('|');
                    out.push(o);
                }
                None => out.push('|'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Écran de connexion.
pub fn login_panel(ctx: &egui::Context, st: &mut UiState, busy: bool, actions: &mut UiActions) {
    egui::Window::new("ArchRust — connexion")
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            egui::Grid::new("login")
                .num_columns(2)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Serveur");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut st.host).desired_width(140.0));
                        ui.add(egui::TextEdit::singleline(&mut st.port).desired_width(50.0));
                    });
                    ui.end_row();
                    ui.label("Compte");
                    ui.text_edit_singleline(&mut st.user);
                    ui.end_row();
                    ui.label("Mot de passe");
                    ui.add(egui::TextEdit::singleline(&mut st.password).password(true));
                    ui.end_row();
                    ui.label("Personnage");
                    ui.text_edit_singleline(&mut st.character);
                    ui.end_row();
                    ui.label("");
                    ui.checkbox(&mut st.create_if_missing, "créer s'il n'existe pas");
                    ui.end_row();
                });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!busy, egui::Button::new("Entrer dans le monde"))
                    .clicked()
                {
                    actions.connect = true;
                }
                if busy {
                    ui.spinner();
                }
            });
            if !st.status.is_empty() {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(&st.status).color(
                    if st.status.starts_with("erreur") {
                        Color32::LIGHT_RED
                    } else {
                        Color32::LIGHT_GRAY
                    },
                ));
            }
        });
}

/// Fenêtre de discussion + saisie (commandes `/` incluses).
pub fn chat_panel(ctx: &egui::Context, st: &mut UiState, actions: &mut UiActions) {
    egui::Window::new("Discussion")
        .anchor(Align2::LEFT_BOTTOM, [10.0, -10.0])
        .default_width(420.0)
        .default_height(180.0)
        .resizable(true)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .max_height(140.0)
                .show(ui, |ui| {
                    for l in &st.chat_lines {
                        ui.label(strip_color_codes(l));
                    }
                });
            let resp = ui.add(
                egui::TextEdit::singleline(&mut st.chat_input)
                    .hint_text("Message ou /commande")
                    .desired_width(f32::INFINITY),
            );
            if resp.lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                && !st.chat_input.trim().is_empty()
            {
                actions.chat = Some(std::mem::take(&mut st.chat_input));
                resp.request_focus();
            }
        });
}

/// Console Lua de développement.
pub fn console_panel(ctx: &egui::Context, st: &mut UiState, actions: &mut UiActions) {
    if !st.console_open {
        return;
    }
    egui::Window::new("Console Lua")
        .anchor(Align2::RIGHT_TOP, [-10.0, 10.0])
        .default_width(360.0)
        .show(ctx, |ui| {
            ui.label("Exécuté dans l'état des AddOns (Ctrl+` pour fermer)");
            let resp = ui.add(
                egui::TextEdit::multiline(&mut st.console_input)
                    .desired_rows(4)
                    .desired_width(f32::INFINITY)
                    .code_editor(),
            );
            if ui.button("Exécuter").clicked()
                || (resp.has_focus()
                    && ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Enter)))
            {
                actions.console = Some(st.console_input.clone());
            }
        });
}

/// Petit tableau de bord (position, entités, AddOns).
pub fn info_panel(
    ctx: &egui::Context,
    st: &mut UiState,
    fps: f32,
    player: Option<(&str, [f32; 3], Option<i32>)>,
    entities: &[(String, [f32; 3], Option<i32>)],
    mgr: &AddonManager,
    sectors: usize,
) {
    egui::Window::new("ArchRust")
        .anchor(Align2::LEFT_TOP, [10.0, 10.0])
        .default_width(260.0)
        .show(ctx, |ui| {
            ui.label(format!("{fps:.0} ips — {sectors} secteur(s) chargé(s)"));
            if let Some((name, p, hp)) = player {
                let hp = hp.map(|hp| format!(" — {hp} PV")).unwrap_or_default();
                ui.label(format!(
                    "{name}{hp} : {:.0}, {:.0}, {:.0}",
                    p[0], p[1], p[2]
                ));
            }
            ui.checkbox(
                &mut st.show_entities,
                format!("{} entités en vue", entities.len()),
            );
            if st.show_entities {
                egui::ScrollArea::vertical()
                    .max_height(160.0)
                    .show(ui, |ui| {
                        for (n, p, hp) in entities.iter().take(80) {
                            let hp = hp.map(|hp| format!("  {hp} PV")).unwrap_or_default();
                            ui.small(format!("{n}{hp}  ({:.0}, {:.0})", p[0], p[2]));
                        }
                    });
            }
            ui.checkbox(&mut st.show_addons, format!("{} AddOns", mgr.addons.len()));
            if st.show_addons {
                for a in &mgr.addons {
                    let m = &a.manifest;
                    let status = match (&a.error, a.enabled) {
                        (Some(e), _) => format!("✘ {e}"),
                        (None, true) => "✔".into(),
                        (None, false) => "–".into(),
                    };
                    ui.small(format!(
                        "{} {} {} — {}",
                        status, m.title, m.version, m.notes
                    ));
                }
            }
            if ui.small_button("Console Lua").clicked() {
                st.console_open = !st.console_open;
            }
        });
}

/// Rectangle utilitaire pour les tests.
pub fn rect_to_egui(r: Rect, sh: f32) -> egui::Rect {
    egui::Rect::from_min_size(
        Pos2::new(r.left, sh - r.bottom - r.height),
        egui::vec2(r.width, r.height),
    )
}
