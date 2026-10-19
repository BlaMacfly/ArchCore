//! Interface : panneaux egui du client (connexion, discussion, console, informations) et rendu
//! des frames Lua des AddOns (rectangles, textes, barres) d'après leur mise en page.

use archrust_addons::ui::{color_segments, Rect};
use archrust_addons::AddonManager;
use archrust_netclient::world::ItemRecord;
use egui::{Align2, Color32, Pos2, Stroke};

/// État des panneaux egui.
pub struct UiState {
    pub host: String,
    pub port: String,
    pub user: String,
    pub password: String,
    /// Dernier personnage joué (présélectionné à l'écran de sélection).
    pub character: String,
    /// Personnages du compte, reçus après l'authentification.
    pub characters: Vec<archrust_netclient::CharacterSummary>,
    /// Indice du personnage sélectionné dans `characters`.
    pub selected_character: Option<usize>,
    /// Écran de création (`None` hors de cet écran).
    pub creation: Option<CreationUi>,
    /// Personnage dont la suppression attend confirmation.
    pub delete_confirm: Option<String>,
    pub create_if_missing: bool,
    /// Case « Enregistrer le compte » (identifiant, mot de passe et personnage chiffrés, voir `account`).
    pub remember: bool,
    pub status: String,
    pub chat_lines: Vec<String>,
    pub chat_input: String,
    pub console_open: bool,
    pub console_input: String,
    pub show_addons: bool,
    pub show_entities: bool,
    pub show_inventory: bool,
    /// Fenêtre Options (volumes), ouverte par le bouton « Options » ou la touche Échap.
    pub show_options: bool,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: "11002".into(),
            user: "test".into(),
            password: "test".into(),
            character: "Testeur".into(),
            characters: Vec::new(),
            selected_character: None,
            creation: None,
            delete_confirm: None,
            create_if_missing: true,
            remember: false,
            status: String::new(),
            chat_lines: Vec::new(),
            chat_input: String::new(),
            console_open: false,
            console_input: String::new(),
            show_addons: false,
            show_entities: true,
            show_inventory: false,
            show_options: false,
        }
    }
}

/// Ce que le moteur audio joue, affiché dans la fenêtre Options.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AudioStatus {
    /// Un périphérique de sortie est ouvert.
    pub active: bool,
    pub music: Option<String>,
    pub ambience: Option<String>,
    /// Région du joueur (indice `RegionTemplate.ini`, nom).
    pub region: Option<(u16, String)>,
}

/// Fenêtre Options : volumes de la musique, de l'ambiance et des effets, coupure du son. Rend
/// `true` si un réglage a changé (l'appelant l'applique et l'enregistre).
pub fn options_panel(
    ctx: &egui::Context,
    st: &mut UiState,
    audio: &mut crate::audio::AudioSettings,
    status: &AudioStatus,
) -> bool {
    if !st.show_options {
        return false;
    }
    let mut changed = false;
    let mut open = true;
    egui::Window::new("Options")
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.heading("Son");
            if !status.active {
                ui.colored_label(
                    Color32::LIGHT_RED,
                    "Aucun périphérique de sortie : le client est muet.",
                );
            }
            egui::Grid::new("options-audio")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    for (label, value) in [
                        ("Musique", &mut audio.music),
                        ("Ambiance", &mut audio.ambience),
                        ("Effets", &mut audio.effects),
                    ] {
                        ui.label(label);
                        let mut percent = (*value * 100.0).round();
                        if ui
                            .add(egui::Slider::new(&mut percent, 0.0..=100.0).suffix(" %"))
                            .changed()
                        {
                            *value = percent / 100.0;
                            changed = true;
                        }
                        ui.end_row();
                    }
                });
            if ui.checkbox(&mut audio.muted, "Couper le son").changed() {
                changed = true;
            }
            ui.add_space(6.0);
            ui.small(format!(
                "Musique : {}",
                status.music.as_deref().unwrap_or("—")
            ));
            ui.small(format!(
                "Ambiance : {}",
                status.ambience.as_deref().unwrap_or("—")
            ));
            if let Some((id, name)) = &status.region {
                ui.small(format!("Région : {name} ({id})"));
            }
            ui.add_space(4.0);
            ui.small("Enregistré dans saved-variables/options.toml. Échap pour fermer.");
        });
    if !open {
        st.show_options = false;
    }
    changed
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
    /// Écran de sélection : personnage à jouer, personnage à créer, retour à la connexion.
    pub enter_character: Option<String>,
    pub back_to_login: bool,
    pub open_creation: bool,
    /// Création demandée : (TID du modèle, nom, visage, coiffure).
    pub create_submit: Option<(i32, String, i32, i32)>,
    pub cancel_creation: bool,
    pub delete_character: Option<String>,
    pub chat: Option<String>,
    pub console: Option<String>,
    pub clicked_frames: Vec<usize>,
}

/// Ligne de discussion avec les codes couleur WoW (`|cffRRGGBB…|r`) rendus en couleur.
pub fn colored_job(ui: &egui::Ui, line: &str) -> egui::text::LayoutJob {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let base = ui.visuals().text_color();
    let mut job = egui::text::LayoutJob::default();
    for (seg, c) in color_segments(line) {
        let color = c.map_or(base, |c| {
            Color32::from_rgb(
                (c[0] * 255.0) as u8,
                (c[1] * 255.0) as u8,
                (c[2] * 255.0) as u8,
            )
        });
        job.append(
            &seg,
            0.0,
            egui::TextFormat {
                font_id: font.clone(),
                color,
                ..Default::default()
            },
        );
    }
    job.wrap.max_width = ui.available_width();
    job
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

/// Image de l'écran de connexion (« ArchRust — Awakening of the Eternal Storm »), intégrée à
/// l'exécutable.
pub const LOGIN_SCREEN_PNG: &[u8] = include_bytes!("../assets/login-screen.png");

/// Texture egui de l'écran de connexion, décodée une fois.
pub fn load_login_screen(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let img = match image::load_from_memory(LOGIN_SCREEN_PNG) {
        Ok(i) => i.to_rgba8(),
        Err(e) => {
            log::warn!("écran de connexion : {e}");
            return None;
        }
    };
    let size = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    Some(ctx.load_texture("login-screen", color, egui::TextureOptions::LINEAR))
}

/// Fond de l'écran de connexion : l'image couvre toute la fenêtre sans déformation (recadrée
/// sur les bords, centrée horizontalement, calée en haut pour garder le logo).
pub fn login_background(ctx: &egui::Context, tex: &egui::TextureHandle) {
    let screen = ctx.screen_rect();
    let [w, h] = tex.size().map(|v| v as f32);
    let scale = (screen.width() / w).max(screen.height() / h);
    let (vis_w, vis_h) = (screen.width() / scale / w, screen.height() / scale / h);
    let u0 = (1.0 - vis_w) / 2.0;
    let uv = egui::Rect::from_min_max(Pos2::new(u0, 0.0), Pos2::new(u0 + vis_w, vis_h));
    ctx.layer_painter(egui::LayerId::background())
        .image(tex.id(), screen, uv, Color32::WHITE);
}

/// Écran de connexion.
pub fn login_panel(ctx: &egui::Context, st: &mut UiState, busy: bool, actions: &mut UiActions) {
    let frame = egui::Frame::window(&ctx.style())
        .fill(Color32::from_rgba_unmultiplied(12, 8, 6, 225))
        .stroke(Stroke::new(1.5_f32, Color32::from_rgb(200, 90, 30)))
        .inner_margin(egui::Margin::same(14));
    egui::Window::new("Connexion")
        .anchor(Align2::CENTER_BOTTOM, [0.0, -70.0])
        .collapsible(false)
        .resizable(false)
        .frame(frame)
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
                    ui.label("");
                    ui.checkbox(&mut st.remember, "Enregistrer le compte")
                        .on_hover_text(
                            "Mémorise identifiant, mot de passe et personnage, chiffrés pour votre                              session Windows (saved-variables/compte.dat). Décocher pour les oublier.",
                        );
                    ui.end_row();
                });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!busy, egui::Button::new("Connexion"))
                    .clicked()
                {
                    actions.connect = true;
                }
                if busy {
                    ui.spinner();
                }
                if ui.button("Options").clicked() {
                    st.show_options = !st.show_options;
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

/// Cadre des fenêtres de la scène de connexion (sombre, bord orangé).
fn scene_frame(ctx: &egui::Context) -> egui::Frame {
    egui::Frame::window(&ctx.style())
        .fill(Color32::from_rgba_unmultiplied(12, 8, 6, 220))
        .stroke(Stroke::new(1.5_f32, Color32::from_rgb(200, 90, 30)))
        .inner_margin(egui::Margin::same(12))
}

fn status_line(ui: &mut egui::Ui, status: &str) {
    if !status.is_empty() {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(status).color(
            if status.starts_with("erreur") || status.contains("refus") {
                Color32::LIGHT_RED
            } else {
                Color32::LIGHT_GRAY
            },
        ));
    }
}

/// Écran de sélection d'Archonia : les personnages du compte sont dans la scène 3D (clic pour
/// choisir) ; en bas, les boutons « Entrer », « Créer », « Supprimer » et « Retour »
/// (`LG_LoginMainSelectButtonClick`, `…CreateButtonClick`, `…DeleteButtonClick`).
pub fn character_select_panel(
    ctx: &egui::Context,
    st: &mut UiState,
    busy: bool,
    actions: &mut UiActions,
) {
    egui::Window::new("Sélection du personnage")
        .anchor(Align2::CENTER_BOTTOM, [0.0, -24.0])
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
        .frame(scene_frame(ctx))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Emplacements (trois au plus, comme `LG_Slot01…03`).
                for (i, ch) in st.characters.iter().enumerate() {
                    let selected = st.selected_character == Some(i);
                    let text =
                        egui::RichText::new(format!("{}\nniveau {}", ch.name, ch.level)).size(15.0);
                    if ui
                        .add_sized([150.0, 44.0], egui::SelectableLabel::new(selected, text))
                        .clicked()
                    {
                        st.selected_character = Some(i);
                    }
                }
                if st.characters.is_empty() {
                    ui.label("Aucun personnage : créez-en un.");
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let chosen = st
                    .selected_character
                    .and_then(|i| st.characters.get(i))
                    .map(|c| c.name.clone());
                if ui
                    .add_enabled(
                        !busy && chosen.is_some(),
                        egui::Button::new("Entrer dans le monde"),
                    )
                    .clicked()
                {
                    actions.enter_character = chosen.clone();
                }
                if ui
                    .add_enabled(
                        !busy && st.characters.len() < 3,
                        egui::Button::new("Créer un personnage"),
                    )
                    .clicked()
                {
                    actions.open_creation = true;
                }
                if ui
                    .add_enabled(!busy && chosen.is_some(), egui::Button::new("Supprimer"))
                    .clicked()
                {
                    st.delete_confirm = chosen.clone();
                }
                if ui.add_enabled(!busy, egui::Button::new("Retour")).clicked() {
                    actions.back_to_login = true;
                }
                if busy {
                    ui.spinner();
                }
            });
            status_line(ui, &st.status);
        });
    // Confirmation de suppression (`LG_QUESTION_REMOVE_SELECTED_CHARACTER`).
    if let Some(name) = st.delete_confirm.clone() {
        egui::Window::new("Supprimer le personnage")
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .frame(scene_frame(ctx))
            .show(ctx, |ui| {
                ui.label(format!("Supprimer définitivement {name} ?"));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Supprimer").clicked() {
                        actions.delete_character = Some(name.clone());
                        st.delete_confirm = None;
                    }
                    if ui.button("Annuler").clicked() {
                        st.delete_confirm = None;
                    }
                });
            });
    }
}

/// Modèle de base proposé à la création (paquet CHARACTER reçu à l'entrée de l'écran).
#[derive(Debug, Clone, PartialEq)]
pub struct CreationTemplate {
    pub tid: i32,
    /// Nom de classe du modèle (Knight, Archer, Mage, Berserker…).
    pub class_name: String,
    /// Race (`login_settings::RACES`).
    pub race: &'static str,
    pub face_count: i32,
    pub hair_count: i32,
}

/// État de l'écran de création : race, classe (modèle), visage, coiffure, rotation, zoom, nom.
#[derive(Debug, Clone, Default)]
pub struct CreationUi {
    pub templates: Vec<CreationTemplate>,
    pub race: &'static str,
    /// Indice dans `templates`.
    pub template: usize,
    pub face: i32,
    pub hair: i32,
    /// Rotation ajoutée par les boutons (degrés).
    pub rotate: f32,
    pub zoom: bool,
    pub name: String,
}

impl CreationUi {
    pub fn current(&self) -> Option<&CreationTemplate> {
        self.templates.get(self.template)
    }
    /// Choisit une race : premier modèle de cette race, visage et coiffure à zéro.
    pub fn select_race(&mut self, race: &'static str) {
        if let Some(i) = self.templates.iter().position(|t| t.race == race) {
            self.race = race;
            self.template = i;
            self.face = 0;
            self.hair = 0;
        }
    }
}

fn cycle(value: &mut i32, count: i32, step: i32) {
    if count > 0 {
        *value = (*value + step).rem_euclid(count);
    }
}

/// Écran de création d'Archonia (`UI_LG_CHARCREATE`) : boutons de race et de classe, visage et
/// coiffure précédents / suivants, rotation, zoom, nom, « Créer » et « Annuler »
/// (`CreateCharacter_OnSelectRace…`, `…OnBtnSelectHeadPrev/Next`, `LG_RotateLeftStart`,
/// `LG_ToggleZoom`, `LG_CharacterCreateOK`, `LG_CharacterCreateCancel`).
pub fn character_create_panel(
    ctx: &egui::Context,
    st: &mut UiState,
    busy: bool,
    actions: &mut UiActions,
) {
    let Some(c) = st.creation.as_mut() else {
        return;
    };
    egui::Window::new("Création du personnage")
        .anchor(Align2::LEFT_CENTER, [24.0, 0.0])
        .collapsible(false)
        .resizable(false)
        .frame(scene_frame(ctx))
        .show(ctx, |ui| {
            ui.set_width(280.0);
            ui.label(egui::RichText::new("Race").strong());
            ui.horizontal_wrapped(|ui| {
                for race in crate::login_settings::RACES {
                    let available = c.templates.iter().any(|t| t.race == race);
                    let r = ui.add_enabled(
                        available,
                        egui::SelectableLabel::new(
                            c.race == race,
                            crate::login_settings::race_label(race),
                        ),
                    );
                    if r.clicked() {
                        c.select_race(race);
                    }
                }
            });
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Classe").strong());
            ui.horizontal_wrapped(|ui| {
                let race = c.race;
                let mut chosen = None;
                for (i, t) in c
                    .templates
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| t.race == race)
                {
                    if ui
                        .add(egui::SelectableLabel::new(c.template == i, &t.class_name))
                        .clicked()
                    {
                        chosen = Some(i);
                    }
                }
                if let Some(i) = chosen {
                    c.template = i;
                    c.face = 0;
                    c.hair = 0;
                }
            });
            ui.add_space(6.0);
            let (faces, hairs) = c
                .current()
                .map(|t| (t.face_count, t.hair_count))
                .unwrap_or((0, 0));
            egui::Grid::new("customize").num_columns(4).show(ui, |ui| {
                ui.label("Visage");
                if ui.button("◀").clicked() {
                    cycle(&mut c.face, faces, -1);
                }
                ui.label(format!("{} / {}", c.face + 1, faces.max(1)));
                if ui.button("▶").clicked() {
                    cycle(&mut c.face, faces, 1);
                }
                ui.end_row();
                ui.label("Coiffure");
                if ui.button("◀").clicked() {
                    cycle(&mut c.hair, hairs, -1);
                }
                ui.label(format!("{} / {}", c.hair + 1, hairs.max(1)));
                if ui.button("▶").clicked() {
                    cycle(&mut c.hair, hairs, 1);
                }
                ui.end_row();
                ui.label("Tourner");
                if ui.button("⟲").clicked() {
                    c.rotate -= 30.0;
                }
                if ui.button("⟳").clicked() {
                    c.rotate += 30.0;
                }
                ui.checkbox(&mut c.zoom, "Zoom");
                ui.end_row();
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Nom");
                ui.add(egui::TextEdit::singleline(&mut c.name).desired_width(200.0));
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let name = c.name.trim().to_string();
                let ok = !busy && !name.is_empty() && c.current().is_some();
                if ui.add_enabled(ok, egui::Button::new("Créer")).clicked() {
                    if let Some(t) = c.current() {
                        actions.create_submit = Some((t.tid, name, c.face, c.hair));
                    }
                }
                if ui
                    .add_enabled(!busy, egui::Button::new("Annuler"))
                    .clicked()
                {
                    actions.cancel_creation = true;
                }
                if busy {
                    ui.spinner();
                }
            });
        });
    if !st.status.is_empty() {
        egui::Area::new(egui::Id::new("creation-status"))
            .anchor(Align2::CENTER_BOTTOM, [0.0, -30.0])
            .show(ctx, |ui| status_line(ui, &st.status));
    }
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
                        ui.label(colored_job(ui, l));
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
            ui.checkbox(
                &mut st.show_addons,
                format!("{} AddOns", mgr.addons().len()),
            );
            if st.show_addons {
                for a in &mgr.addons() {
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
            ui.horizontal(|ui| {
                if ui.small_button("Console Lua").clicked() {
                    st.console_open = !st.console_open;
                }
                if ui.small_button("Options").clicked() {
                    st.show_options = !st.show_options;
                }
            });
        });
}

/// Rectangle utilitaire pour les tests.
pub fn rect_to_egui(r: Rect, sh: f32) -> egui::Rect {
    egui::Rect::from_min_size(
        Pos2::new(r.left, sh - r.bottom - r.height),
        egui::vec2(r.width, r.height),
    )
}
