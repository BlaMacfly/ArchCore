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
    /// Onglets de chaque ligne (masques de FUN_140511e20 : 0x1 normal, 0x2 guilde, 0x4 groupe…).
    pub chat_masks: Vec<u16>,
    /// Onglets de la discussion : nom (`Chat_Tab_*` de uimessage) et masque (`u16::MAX` = tout).
    pub chat_tabs: Vec<(String, u16)>,
    pub chat_tab: usize,
    pub chat_input: String,
    /// Lignes remontées à la molette dans la discussion (0 : les plus récentes).
    pub chat_scroll: usize,
    /// Donner le focus à la saisie de discussion à la prochaine image (touche Entrée).
    pub chat_focus: bool,
    pub console_open: bool,
    pub console_input: String,
    pub show_addons: bool,
    pub show_entities: bool,
    pub show_inventory: bool,
    /// Fenêtre Options (volumes, langue), ouverte par le bouton « Options » ou la touche Échap.
    pub show_options: bool,
    /// Langue appliquée par le serveur et langues qu'il propose (`HELLO_ACK`, `LANGUAGE_ACK`).
    pub server_language: Option<(String, Vec<String>)>,
    /// Les packs de ressources ont changé : ils s'appliqueront au prochain démarrage.
    pub packs_restart: bool,
}

/// Changement demandé dans la section « Packs de ressources » des Options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PackAction {
    #[default]
    None,
    /// Un pack activé, désactivé ou déplacé : enregistrer `packs.toml`.
    Changed,
    /// « Rétablir la configuration par défaut » : supprimer `packs.toml`.
    Reset,
}

/// Langues proposées dans les Options : versions officielles d'ArchLord (Corée 2005, Codemasters
/// 2006, NHN Japan 2007, Shanda 2006) et client multilingue Webzen de 2010 (allemand, français,
/// espagnol, portugais). Code BCP 47 et libellé ; voir la page « Traduction » du wiki d'ArchCore.
pub const LANGUAGES: &[(&str, &str)] = &[
    ("en", "English"),
    ("fr", "Français"),
    ("de", "Deutsch"),
    ("es", "Español"),
    ("pt-BR", "Português (Brasil)"),
    ("ko", "Coréen"),
    ("ja", "Japonais"),
    ("zh-CN", "Chinois simplifié"),
];

/// Libellé d'un code de langue (le code lui-même s'il n'est pas dans la liste).
pub fn language_label(code: &str) -> String {
    LANGUAGES
        .iter()
        .find(|(c, _)| c.eq_ignore_ascii_case(code))
        .map(|(c, n)| format!("{n} ({c})"))
        .unwrap_or_else(|| code.to_string())
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
            chat_masks: Vec::new(),
            chat_tabs: Vec::new(),
            chat_tab: 0,
            chat_input: String::new(),
            chat_scroll: 0,
            chat_focus: false,
            console_open: false,
            console_input: String::new(),
            show_addons: false,
            show_entities: true,
            show_inventory: false,
            show_options: false,
            server_language: None,
            packs_restart: false,
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

/// Fenêtre Options : volumes de la musique, de l'ambiance et des effets, coupure du son, langue des
/// textes du serveur. Rend `true` si un réglage a changé (l'appelant l'applique et l'enregistre).
pub fn options_panel(
    ctx: &egui::Context,
    st: &mut UiState,
    audio: &mut crate::audio::AudioSettings,
    status: &AudioStatus,
    language: &mut Option<String>,
    packs: &mut archrust_patch::PatchStack,
    pack_action: &mut PackAction,
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
            ui.add_space(8.0);
            ui.heading("Langue");
            let selected_label = match language.as_deref() {
                None => "Automatique".to_string(),
                Some(code) => language_label(code),
            };
            egui::ComboBox::from_id_salt("options-language")
                .selected_text(selected_label)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(language.is_none(), "Automatique").clicked() && language.is_some() {
                        *language = None;
                        changed = true;
                    }
                    for (code, name) in LANGUAGES {
                        let selected = language.as_deref() == Some(*code);
                        if ui.selectable_label(selected, format!("{name} ({code})")).clicked() && !selected {
                            *language = Some(code.to_string());
                            changed = true;
                        }
                    }
                });
            ui.small(
                "Langue des textes envoyés par le serveur (PNJ, monstres, quêtes…), annoncée à la \
                 connexion et à chaque changement. Automatique : dernière langue du compte, sinon \
                 celle du serveur.",
            );
            match &st.server_language {
                Some((applied, list)) => ui.small(format!(
                    "Serveur : {} ; proposées : {}",
                    language_label(applied),
                    list.join(", ")
                )),
                None => ui.small("Serveur : inconnu (extensions ArchCore non négociées)"),
            };
            ui.add_space(8.0);
            *pack_action = packs_section(ui, st.packs_restart, packs);
            ui.add_space(4.0);
            ui.small("Enregistré dans saved-variables/options.toml. Échap pour fermer.");
        });
    if !open {
        st.show_options = false;
    }
    changed
}

/// Section « Packs de ressources » : graphismes et sons, activés ou non, ordre, détails et problèmes.
/// Les packs de la communauté (`patches/`) passent avant le pack officiel (`free-assets/`), puis les
/// données du jeu ; un pack mixte figure dans les deux listes.
fn packs_section(ui: &mut egui::Ui, restart: bool, packs: &mut archrust_patch::PatchStack) -> PackAction {
    use archrust_patch::Category;
    let mut action = PackAction::None;
    egui::CollapsingHeader::new("Packs de ressources").default_open(true).show(ui, |ui| {
        if packs.patches.is_empty() {
            ui.small("Aucun pack installé (dossiers free-assets/ et patches/).");
            return;
        }
        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
            for (title, category) in [("Graphismes", Category::Graphics), ("Sons", Category::Sounds)] {
                ui.label(egui::RichText::new(title).strong());
                let rows = packs.in_category(category);
                if rows.is_empty() {
                    ui.small("Aucun pack.");
                }
                for i in rows {
                    let p = &packs.patches[i];
                    let m = &p.manifest;
                    let (id, mut enabled, official) = (m.patch.id.clone(), p.enabled, p.official);
                    let title = format!("{}{}{}", m.patch.name, if m.patch.version.is_empty() { String::new() } else { format!(" {}", m.patch.version) }, if official { " (officiel)" } else { "" });
                    let details = format!(
                        "{}\nIdentifiant : {id}\nAuteur : {}\nCatégorie : {}\nPriorité : {}\nContenu : {} texture(s), {} modèle(s), {} fichier(s), {} son(s)",
                        if m.patch.description.is_empty() { "—" } else { m.patch.description.as_str() },
                        if m.patch.author.is_empty() { "—" } else { m.patch.author.as_str() },
                        m.category().label(),
                        p.priority,
                        m.textures.len(),
                        m.models.len(),
                        m.files.len(),
                        m.sounds.len(),
                    );
                    let problems = p.problems.clone();
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut enabled, "").on_hover_text("Activer ou désactiver ce pack").changed() {
                            packs.set_enabled(&id, enabled);
                            action = PackAction::Changed;
                        }
                        if ui.small_button("Haut").on_hover_text("Plus prioritaire").clicked() && packs.move_pack(&id, true) {
                            action = PackAction::Changed;
                        }
                        if ui.small_button("Bas").on_hover_text("Moins prioritaire").clicked() && packs.move_pack(&id, false) {
                            action = PackAction::Changed;
                        }
                        let label = if enabled { egui::RichText::new(title) } else { egui::RichText::new(title).weak() };
                        ui.label(label).on_hover_text(details);
                    });
                    for problem in problems {
                        ui.colored_label(Color32::LIGHT_RED, format!("    {problem}"));
                    }
                }
                ui.add_space(4.0);
            }
        });
        if ui.button("Rétablir la configuration par défaut").clicked() {
            action = PackAction::Reset;
        }
        if restart {
            ui.colored_label(Color32::YELLOW, "Redémarrez le client pour appliquer les changements.");
        }
        ui.small("Ordre : packs de la communauté (patches/), puis pack officiel (free-assets/), puis données du jeu. Enregistré dans saved-variables/packs.toml.");
    });
    action
}

/// Bouton d'inventaire (nombre d'objets du sac) ; il ouvre la fenêtre d'Archonia dessinée par le HUD.
pub fn inventory_panel(ctx: &egui::Context, st: &mut UiState, items: &[(ItemRecord, String)]) {
    // À gauche de la minicarte du HUD, sous la ligne du bouton « Groupe ».
    let (minimap_w, _) = crate::hud::reserved(ctx);
    egui::Area::new(egui::Id::new("inventory-toggle"))
        .anchor(Align2::RIGHT_TOP, [-10.0 - minimap_w, 40.0])
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
    // La fenêtre elle-même est celle d'Archonia (`hud::draw_inventory`).
}

impl UiState {
    pub fn push_chat(&mut self, line: impl Into<String>) {
        self.push_chat_in(0x1, line);
    }

    /// Ligne de discussion rangée dans les onglets du masque `mask` (et toujours dans « Universel »).
    pub fn push_chat_in(&mut self, mask: u16, line: impl Into<String>) {
        self.chat_lines.push(line.into());
        self.chat_masks.push(mask);
        if self.chat_lines.len() > 200 {
            self.chat_lines.remove(0);
            self.chat_masks.remove(0);
        }
    }

    /// Lignes visibles dans l'onglet choisi.
    fn chat_view(&self) -> Vec<&String> {
        let mask = self.chat_tabs.get(self.chat_tab).map_or(u16::MAX, |t| t.1);
        self.chat_lines.iter().zip(self.chat_masks.iter().chain(std::iter::repeat(&0x1))).filter(|(_, m)| mask == u16::MAX || *m & mask != 0).map(|(l, _)| l).collect()
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
    colored_job_with(font, ui.visuals().text_color(), ui.available_width(), line)
}

/// Comme `colored_job`, avec police, couleur par défaut et largeur données.
pub fn colored_job_with(
    font: egui::FontId,
    base: Color32,
    max_width: f32,
    line: &str,
) -> egui::text::LayoutJob {
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
    job.wrap.max_width = max_width;
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

/// Panneau de connexion illustré (cadre, titre, libellés, champs, case et boutons dessinés),
/// intégré à l'exécutable. 1710 × 919 px, fond transparent (version rouge et or).
pub const LOGIN_PANEL_PNG: &[u8] = include_bytes!("../assets/login-panel.png");

/// Zones de l'image du panneau, en pixels de l'image (intérieur des champs et des boutons).
mod login_layout {
    pub const SIZE: [f32; 2] = [1710.0, 919.0];
    pub const HOST: [f32; 4] = [472.0, 272.0, 1110.0, 339.0];
    pub const PORT: [f32; 4] = [1162.0, 272.0, 1408.0, 339.0];
    pub const USER: [f32; 4] = [472.0, 377.0, 1600.0, 443.0];
    pub const PASSWORD: [f32; 4] = [472.0, 480.0, 1600.0, 549.0];
    pub const REMEMBER: [f32; 4] = [467.0, 588.0, 525.0, 647.0];
    pub const REMEMBER_LABEL: [f32; 4] = [545.0, 588.0, 1000.0, 647.0];
    pub const CONNECT: [f32; 4] = [94.0, 694.0, 420.0, 786.0];
    pub const OPTIONS: [f32; 4] = [467.0, 694.0, 760.0, 786.0];
    pub const STATUS: [f32; 4] = [800.0, 700.0, 1630.0, 792.0];
}

/// Texture du panneau de connexion, décodée une fois puis gardée dans la mémoire d'egui.
fn login_panel_texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let id = egui::Id::new("archrust-login-panel");
    if let Some(tex) = ctx.data(|d| d.get_temp::<Option<egui::TextureHandle>>(id)) {
        return tex;
    }
    let tex = match image::load_from_memory(LOGIN_PANEL_PNG) {
        Ok(img) => {
            let img = img.to_rgba8();
            let size = [img.width() as usize, img.height() as usize];
            let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
            Some(ctx.load_texture("login-panel", color, egui::TextureOptions::LINEAR))
        }
        Err(e) => {
            log::warn!("panneau de connexion : {e}");
            None
        }
    };
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    tex
}

/// Écran de connexion : panneau illustré, ou panneau egui simple si l'image est illisible.
pub fn login_panel(ctx: &egui::Context, st: &mut UiState, busy: bool, actions: &mut UiActions) {
    let Some(tex) = login_panel_texture(ctx) else {
        return login_panel_plain(ctx, st, busy, actions);
    };
    let screen = ctx.screen_rect();
    let [iw, ih] = login_layout::SIZE;
    // Largeur à l'écran : environ 45 % de la fenêtre, entre 520 et 860 px, en gardant le
    // panneau sous le logo de l'image de fond.
    let width = (screen.width() * 0.45).clamp(520.0, 860.0).min(screen.width() - 16.0);
    let scale = width / iw;
    let size = egui::vec2(width, ih * scale);
    let top_left = Pos2::new(
        screen.center().x - size.x / 2.0,
        (screen.bottom() - size.y - 40.0).max(screen.top() + 8.0),
    );
    let at = |r: [f32; 4]| {
        egui::Rect::from_min_max(
            top_left + egui::vec2(r[0], r[1]) * scale,
            top_left + egui::vec2(r[2], r[3]) * scale,
        )
    };
    egui::Area::new(egui::Id::new("login-panel-area"))
        .fixed_pos(top_left)
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            ui.set_min_size(size);
            let painter = ui.painter();
            painter.image(
                tex.id(),
                egui::Rect::from_min_size(top_left, size),
                egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
            let font = egui::FontId::proportional((at(login_layout::USER).height() * 0.62).max(13.0));
            let field = |ui: &mut egui::Ui, rect: egui::Rect, text: &mut String, password: bool| {
                let margin = egui::vec2(rect.height() * 0.25, (rect.height() - font.size * 1.25) / 2.0);
                ui.put(
                    rect,
                    egui::TextEdit::singleline(text)
                        .frame(false)
                        .password(password)
                        .font(font.clone())
                        .text_color(Color32::from_rgb(235, 240, 250))
                        .margin(margin.max(egui::Vec2::ZERO))
                        .desired_width(rect.width()),
                )
            };
            field(ui, at(login_layout::HOST), &mut st.host, false);
            field(ui, at(login_layout::PORT), &mut st.port, false);
            field(ui, at(login_layout::USER), &mut st.user, false);
            let pw = field(ui, at(login_layout::PASSWORD), &mut st.password, true);
            if pw.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !busy {
                actions.connect = true;
            }

            // Case « Enregistrer le compte » : la case et son libellé dessinés sont cliquables.
            let check = at(login_layout::REMEMBER);
            let check_zone = check.union(at(login_layout::REMEMBER_LABEL));
            let r = ui
                .interact(check_zone, egui::Id::new("login-remember"), egui::Sense::click())
                .on_hover_text(
                    "Mémorise identifiant, mot de passe et personnage, chiffrés pour votre session \
                     Windows (saved-variables/compte.dat). Décocher pour les oublier.",
                );
            if r.clicked() {
                st.remember = !st.remember;
            }
            if st.remember {
                ui.painter().text(
                    check.center(),
                    Align2::CENTER_CENTER,
                    "✔",
                    egui::FontId::proportional(check.height() * 0.8),
                    Color32::from_rgb(120, 200, 255),
                );
            }
            if r.hovered() {
                ui.painter().rect_filled(check, 3.0, Color32::from_white_alpha(18));
            }

            // Boutons dessinés : zone cliquable, reflet au survol, voile quand indisponible.
            let button = |ui: &mut egui::Ui, rect: egui::Rect, id: &str, enabled: bool| -> bool {
                let sense = if enabled { egui::Sense::click() } else { egui::Sense::hover() };
                let r = ui.interact(rect, egui::Id::new(id), sense);
                let rounding = rect.height() * 0.12;
                if !enabled {
                    ui.painter().rect_filled(rect, rounding, Color32::from_black_alpha(110));
                } else if r.is_pointer_button_down_on() {
                    ui.painter().rect_filled(rect, rounding, Color32::from_black_alpha(50));
                } else if r.hovered() {
                    ui.painter().rect_filled(rect, rounding, Color32::from_white_alpha(28));
                }
                enabled && r.clicked()
            };
            if button(ui, at(login_layout::CONNECT), "login-connect", !busy) {
                actions.connect = true;
            }
            if button(ui, at(login_layout::OPTIONS), "login-options", true) {
                st.show_options = !st.show_options;
            }

            // État de la connexion, à droite des boutons.
            let status_rect = at(login_layout::STATUS);
            let status_font = egui::FontId::proportional((font.size * 0.8).max(11.0));
            if busy {
                let spin = egui::Rect::from_center_size(
                    Pos2::new(status_rect.left() + status_rect.height() * 0.4, status_rect.center().y),
                    egui::vec2(status_rect.height() * 0.45, status_rect.height() * 0.45),
                );
                ui.put(spin, egui::Spinner::new());
            }
            if !st.status.is_empty() {
                let color = if st.status.starts_with("erreur") {
                    Color32::from_rgb(255, 140, 120)
                } else {
                    Color32::from_rgb(210, 220, 235)
                };
                let left = if busy { status_rect.left() + status_rect.height() * 0.8 } else { status_rect.left() };
                ui.painter().text(
                    Pos2::new(left, status_rect.center().y),
                    Align2::LEFT_CENTER,
                    &st.status,
                    status_font,
                    color,
                );
            }
        });
}

/// Panneau de connexion egui simple (secours si l'image du panneau est illisible).
fn login_panel_plain(ctx: &egui::Context, st: &mut UiState, busy: bool, actions: &mut UiActions) {
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
///
/// Avec la barre `Chat_Input_Normal` d'Archonia (`bar`), la saisie se pose sans cadre sur son
/// contrôle `Input_Message` et les lignes sont dessinées au-dessus, sans fenêtre, avec une ombre ;
/// un voile sombre apparaît quand la souris les survole ou qu'on écrit. Entrée ouvre la saisie,
/// Entrée envoie, Échap la quitte, la molette remonte l'historique. Sans données d'interface,
/// une fenêtre egui la remplace.
pub fn chat_panel(
    ctx: &egui::Context,
    st: &mut UiState,
    actions: &mut UiActions,
    bar: Option<crate::hud::ChatBar>,
) {
    if let Some(bar) = bar {
        archonia_chat(ctx, st, actions, &bar);
        return;
    }
    // Au-dessus de la barre du bas du HUD.
    let (_, bar_h) = crate::hud::reserved(ctx);
    egui::Window::new("Discussion")
        .anchor(Align2::LEFT_BOTTOM, [10.0, -10.0 - bar_h])
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

/// Onglets au-dessus des lignes (`MainChat_Tab` repos, survol, choisi ; noms `Chat_Tab_*`). Leur
/// place exacte dans Archonia (FUN_1407a30a0 et suivantes) n'est pas établie : ArchRust les aligne
/// au-dessus des lignes, à leur taille d'origine.
fn chat_tabs(ctx: &egui::Context, st: &mut UiState, bar: &crate::hud::ChatBar, lines_rect: egui::Rect, font: &egui::FontId) {
    if st.chat_tabs.is_empty() {
        return;
    }
    // Taille d'origine de la texture, élargie au libellé (étirée).
    let base = bar.tabs[0].map_or(egui::Vec2::new(60.0, 18.0), |(_, s)| egui::Vec2::new(s[0], s[1])) * bar.scale;
    let label_font = egui::FontId::proportional(font.size * 0.9);
    let widths: Vec<f32> = st.chat_tabs.iter().map(|(n, _)| ctx.fonts(|f| f.layout_no_wrap(n.clone(), label_font.clone(), Color32::WHITE).size().x + 12.0).max(base.x)).collect();
    let top = lines_rect.top() - base.y - 2.0;
    egui::Area::new(egui::Id::new("archonia-chat-tabs")).fixed_pos(egui::Pos2::new(lines_rect.left(), top)).order(egui::Order::Background).show(ctx, |ui| {
        let painter = ui.painter().clone();
        let mut x = lines_rect.left();
        for i in 0..st.chat_tabs.len() {
            let r = egui::Rect::from_min_size(egui::Pos2::new(x, top), egui::Vec2::new(widths[i], base.y));
            x += widths[i];
            let resp = ui.interact(r, egui::Id::new(("archonia-chat-tab", i)), egui::Sense::click());
            if resp.clicked() {
                st.chat_tab = i;
                st.chat_scroll = 0;
            }
            let state = if st.chat_tab == i { 2 } else if resp.hovered() { 1 } else { 0 };
            match bar.tabs[state] {
                Some((tex, _)) => {
                    painter.image(tex, r, egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)), Color32::WHITE);
                }
                None => {
                    painter.rect_filled(r, 2.0, Color32::from_black_alpha(if state == 2 { 200 } else { 120 }));
                }
            }
            let f = label_font.clone();
            painter.text(r.center() + egui::Vec2::splat(1.0), Align2::CENTER_CENTER, &st.chat_tabs[i].0, f.clone(), Color32::from_black_alpha(210));
            painter.text(r.center(), Align2::CENTER_CENTER, &st.chat_tabs[i].0, f, if state == 2 { Color32::WHITE } else { Color32::from_rgb(200, 200, 200) });
        }
    });
}

fn archonia_chat(
    ctx: &egui::Context,
    st: &mut UiState,
    actions: &mut UiActions,
    bar: &crate::hud::ChatBar,
) {
    const VISIBLE_LINES: f32 = 9.0;
    let s = bar.scale;
    let font = egui::FontId::proportional((12.0 * s).max(10.0));
    let line_h = font.size + 3.0;
    let bottom = bar.window.top() - 4.0 * s;
    let lines_rect = egui::Rect::from_min_max(
        egui::Pos2::new(bar.window.left() + 4.0 * s, bottom - VISIBLE_LINES * line_h - 4.0),
        egui::Pos2::new(bar.window.left() + (520.0 * s).max(bar.window.width()), bottom),
    );
    let edit_id = egui::Id::new("archonia-chat-edit");
    let typing = ctx.memory(|m| m.has_focus(edit_id));
    let hovered = ctx
        .pointer_hover_pos()
        .is_some_and(|p| lines_rect.contains(p));
    chat_tabs(ctx, st, bar, lines_rect, &font);
    let lines = st.chat_view().into_iter().cloned().collect::<Vec<String>>();
    if hovered {
        let dy = ctx.input(|i| i.raw_scroll_delta.y);
        if dy != 0.0 {
            let steps = ((dy.abs() / 16.0).round() as isize).max(1) * dy.signum() as isize;
            let max = lines.len().saturating_sub(1) as isize;
            st.chat_scroll = (st.chat_scroll as isize + steps).clamp(0, max) as usize;
        }
    }
    st.chat_scroll = st.chat_scroll.min(lines.len().saturating_sub(1));
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new("archonia-chat-lines"),
    ));
    if hovered || typing {
        painter.rect_filled(lines_rect.expand(2.0), 3.0, Color32::from_black_alpha(110));
    }
    let end = lines.len() - st.chat_scroll.min(lines.len());
    let mut y = bottom - 2.0;
    for l in lines[..end].iter().rev() {
        let job = colored_job_with(
            font.clone(),
            Color32::from_rgb(235, 235, 235),
            lines_rect.width() - 6.0,
            l,
        );
        let galley = ctx.fonts(|f| f.layout_job(job));
        y -= galley.size().y + 1.0;
        if y < lines_rect.top() {
            break;
        }
        let pos = egui::Pos2::new(lines_rect.left() + 3.0, y);
        painter.galley_with_override_text_color(
            pos + egui::Vec2::splat(1.0),
            galley.clone(),
            Color32::from_black_alpha(210),
        );
        painter.galley(pos, galley, Color32::WHITE);
    }
    if st.chat_scroll > 0 {
        painter.text(
            egui::Pos2::new(lines_rect.right() - 4.0, bottom - 2.0),
            Align2::RIGHT_BOTTOM,
            format!("▼ {}", st.chat_scroll),
            egui::FontId::proportional(font.size * 0.9),
            Color32::from_rgb(255, 210, 120),
        );
    }
    egui::Area::new(egui::Id::new("archonia-chat-input"))
        .fixed_pos(bar.input.min)
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            let resp = ui.add_sized(
                bar.input.size(),
                egui::TextEdit::singleline(&mut st.chat_input)
                    .id(edit_id)
                    .frame(false)
                    .font(font.clone())
                    .text_color(Color32::WHITE)
                    .hint_text(if typing { "" } else { "Entrée pour écrire" }),
            );
            if std::mem::take(&mut st.chat_focus) {
                resp.request_focus();
            }
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                if !st.chat_input.trim().is_empty() {
                    actions.chat = Some(std::mem::take(&mut st.chat_input));
                }
                st.chat_scroll = 0;
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
