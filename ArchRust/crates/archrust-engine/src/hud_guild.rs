//! Fenêtres de guilde d'Archonia (AgcmUIGuild, sha256 e9c7609b…) dessinées d'après `UI_1024X768.ini`
//! (textes fixes de `ini_fr`), et boîte de confirmation OK / Annuler (AcUIOKCancelDialog).
//!
//! * `Guild_Main` (gestionnaire de guilde), `Guild_Create`, `Guild_Destroy`, `Guild_Create_Request`,
//!   `Guild_Create_Complete`, `Guild_Destroy_Confirm`, `Guild_Destroy_Complete`, `Guild_Notice`
//!   (onglet « Infos ») et `Guild_Member` (onglet « Membres ») : chaque contrôle est dessiné à sa place,
//!   avec ses textures ; les textes variables (affichages `CtrlDisplay=2:…`) et les champs de saisie
//!   viennent de la vue préparée par `app_guild.rs` ; un bouton inactif prend sa texture « d ».
//! * Alignement de `CtrlFont` (5ᵉ valeur) : bits 0-1 horizontal (0 gauche, 1 centre, 2 droite), bit
//!   0x10 centré verticalement (sinon en haut) ; `___` est un saut de ligne des textes d'Archonia.
//! * Liste des membres (`GD_Member_List`, `List=19`) : 19 lignes de `GD_Member_List_Item` (15 px), barre
//!   `GD_Bar.png` sur la ligne choisie, état `GD_On.png` / `GD_Off.png` (`Status2` 0 et 1) ; numéros de
//!   page `<C…>%d` (FUN_1408bf760) en `Guild_SelectPageColor` / `Guild_DeselectPageColor`.
//! * Boîte OK / Annuler : `Common_Message_Base2.png` 412 × 180 (FUN_1408ea820), boutons
//!   `Common_Button_Small` 64 × 17 en (135, 140) et (213, 140) (FUN_14078b940), textes `UI_Name_OK` /
//!   `UI_Name_Cancel` ; message coupé à 311 px, 10 lignes au plus, lignes centrées sur x = 206, bloc
//!   centré sur y = 77, interligne 14 px, blanc (FUN_14078c390, FUN_14078bc60). Sa position à l'écran
//!   n'est pas établie : ArchRust la centre.

use super::npc_ui::{button, frame, window};
use super::portal_ui::argb;
use super::*;
use std::collections::{HashMap, HashSet};

/// Clic dans une fenêtre de guilde.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuildUi {
    /// Bouton `control` de la fenêtre `window`.
    Click { window: String, control: String },
    /// Ligne de la liste des membres (indice dans la page).
    Row(usize),
    /// Bouton de la boîte OK / Annuler : `true` = OK.
    MessageBox(bool),
}

/// Champ de saisie : contrôle, texte, mot de passe, longueur maximale, plusieurs lignes.
pub struct GuildEdit<'a> {
    pub control: &'static str,
    pub text: &'a mut String,
    pub password: bool,
    pub max: usize,
    pub multiline: bool,
}

/// Ligne de la liste des membres.
pub struct GuildRow {
    pub name: String,
    /// Niveau, ou « Rejoindre » / « Se retirer » pour une demande.
    pub level: String,
    pub class: String,
    pub race: String,
    pub online: bool,
}

/// Contenu variable d'une fenêtre.
#[derive(Default)]
pub struct GuildView<'a> {
    /// Texte des contrôles dont l'affichage est variable.
    pub texts: HashMap<&'static str, String>,
    pub edits: Vec<GuildEdit<'a>>,
    /// Boutons inactifs (texture « d », sans clic).
    pub disabled: HashSet<&'static str>,
    /// Onglets du moment (texture « c »).
    pub active: HashSet<&'static str>,
    /// Liste des membres de la page et ligne choisie.
    pub rows: Vec<GuildRow>,
    pub selected: Option<usize>,
    /// Numéros de page (1 à n) et page affichée (0 à n-1).
    pub pages: usize,
    pub page: usize,
}

/// Couleurs des numéros de page (`Guild_SelectPageColor`, `Guild_DeselectPageColor` de uimessage).
pub struct PageColors {
    pub selected: u32,
    pub other: u32,
}

/// Texte d'un contrôle placé selon `CtrlFont` ; `___` = saut de ligne. Seules les zones de texte
/// (type 2) passent à la ligne d'elles-mêmes.
fn area_text(p: &egui::Painter, r: Rect, c: &UiControl, text: &str, size: f32, color: Option<Color32>) {
    let wrap = if c.kind == 2 { r.width().max(1.0) } else { f32::INFINITY };
    if text.is_empty() {
        return;
    }
    let (font_color, align) = c.font.unwrap_or((u32::MAX, 0));
    let color = color.unwrap_or(argb(font_color));
    let mut job = egui::text::LayoutJob::simple(text.replace("___", "\n"), FontId::proportional(size), color, wrap);
    job.halign = match align & 3 {
        1 => egui::Align::Center,
        2 => egui::Align::RIGHT,
        _ => egui::Align::LEFT,
    };
    let galley = p.layout_job(job);
    let x = match align & 3 {
        1 => r.center().x,
        2 => r.right(),
        _ => r.left(),
    };
    let y = if align & 0x10 != 0 { r.center().y - galley.size().y / 2.0 } else { r.top() };
    let pos = Pos2::new(x, y);
    p.galley_with_override_text_color(pos + Vec2::splat(1.0), galley.clone(), Color32::from_black_alpha(210));
    p.galley(pos, galley, color);
}

/// Texte fixe d'un contrôle (`CtrlDisplay=1:texte`).
fn fixed_text(c: &UiControl) -> &str {
    c.display.as_deref().and_then(|d| d.strip_prefix("1:")).unwrap_or("")
}

/// Bouton dessiné comme appuyé (onglet du moment).
fn pressed_tab(ctx: &egui::Context, hud: &mut HudState, assets: &Assets, painter: &egui::Painter, r: Rect, c: &UiControl, label: &str, font: f32) {
    if let Some((_, t)) = c.textures.iter().find(|(_, n)| n.to_ascii_lowercase().ends_with("c.png")).cloned() {
        control_image(ctx, hud, assets, painter, r, c.size, &t, 1.0);
    }
    shadow_text(painter, r.center(), Align2::CENTER_CENTER, label, font, Color32::WHITE);
}

/// Une fenêtre de guilde d'Archonia.
pub fn draw_guild_window(ctx: &egui::Context, hud: &mut HudState, assets: &Assets, name: &str, mut view: GuildView, colors: &PageColors, out: &mut Vec<GuildUi>) {
    let Some(w) = window(hud, assets, name) else { return };
    let id = format!("archonia-{}", name.to_ascii_lowercase());
    frame(ctx, hud, assets, &w, &id, |ui, hud, layout, origin, painter| {
        let s = layout.scale;
        let font = (11.0 * s).max(9.0);
        let click = |out: &mut Vec<GuildUi>, c: &UiControl| out.push(GuildUi::Click { window: w.name.clone(), control: c.name.clone() });
        for c in &w.controls {
            let r = layout.rect(origin, c.pos, c.size);
            // Champs de saisie.
            if let Some(e) = view.edits.iter_mut().find(|e| e.control == c.name) {
                let edit = if e.multiline { egui::TextEdit::multiline(e.text) } else { egui::TextEdit::singleline(e.text) };
                let edit = edit
                    .id(egui::Id::new((&id, e.control)))
                    .frame(false)
                    .password(e.password)
                    .char_limit(e.max)
                    .font(FontId::proportional(font))
                    .text_color(Color32::WHITE)
                    .vertical_align(if e.multiline { egui::Align::Min } else { egui::Align::Center })
                    .horizontal_align(if c.font.is_some_and(|(_, a)| a & 3 == 1) { egui::Align::Center } else { egui::Align::Min });
                ui.put(r, edit);
                continue;
            }
            match c.kind {
                // Liste des membres : les lignes et leurs colonnes.
                10 if c.name == "GD_Member_List" => draw_rows(ctx, ui, hud, assets, painter, &w, layout, origin, &view, font, out),
                3 if c.name == "GD_Member_Button" => {}
                3 => {
                    let label = view.texts.get(c.name.as_str()).cloned().unwrap_or_else(|| fixed_text(c).to_string());
                    if view.active.contains(c.name.as_str()) {
                        pressed_tab(ctx, hud, assets, painter, r, c, &label, font);
                    } else if button(ctx, ui, hud, assets, painter, r, c, &label, font, !view.disabled.contains(c.name.as_str()), (&id, &c.name)).clicked() {
                        click(out, c);
                    }
                }
                1 | 2 => {
                    // Contrôles de ligne de la liste : dessinés avec les lignes.
                    if c.name.starts_with("GD_Member_") && !matches!(c.name.as_str(), "GD_Member_Button_X") {
                        continue;
                    }
                    if c.name == "CONTROL_19921" {
                        page_numbers(painter, r, &view, colors, font);
                        continue;
                    }
                    let text = match view.texts.get(c.name.as_str()) {
                        Some(t) => t.clone(),
                        None => fixed_text(c).to_string(),
                    };
                    area_text(painter, r, c, &text, font, None);
                }
                _ => {}
            }
        }
    });
}

/// Numéros de page « 1 2 3 » (10 au plus à partir de la page affichée), la page affichée en clair.
fn page_numbers(p: &egui::Painter, r: Rect, view: &GuildView, colors: &PageColors, font: f32) {
    if view.pages == 0 {
        return;
    }
    let first = view.page.saturating_sub(view.page % 10);
    let last = (first + 10).min(view.pages - 1);
    let mut job = egui::text::LayoutJob::default();
    for n in first..=last {
        let color = argb(if n == view.page { colors.selected } else { colors.other });
        let fmt = egui::TextFormat { font_id: FontId::proportional(font), color, ..Default::default() };
        job.append(&format!("{}{}", if n == first { "" } else { " " }, n + 1), 0.0, fmt);
    }
    let galley = p.layout_job(job);
    p.galley(Pos2::new(r.center().x - galley.size().x / 2.0, r.center().y - galley.size().y / 2.0), galley, Color32::WHITE);
}

/// Lignes de la liste des membres (`GD_Member_List_Item` et ses colonnes).
#[allow(clippy::too_many_arguments)]
fn draw_rows(ctx: &egui::Context, ui: &mut egui::Ui, hud: &mut HudState, assets: &Assets, painter: &egui::Painter, w: &UiWindow, layout: &Layout, origin: Pos2, view: &GuildView, font: f32, out: &mut Vec<GuildUi>) {
    let Some(item) = w.control("GD_Member_List_Item") else { return };
    let row_h = item.size[1];
    let col = |n: &str| w.control(n);
    for (i, row) in view.rows.iter().enumerate().take(19) {
        let dy = i as f32 * row_h;
        let at = |c: &UiControl| layout.rect(origin, [c.pos[0], c.pos[1] + dy], c.size);
        let ir = at(item);
        let resp = ui.interact(ir, egui::Id::new(("guild-member-row", i)), egui::Sense::click());
        if resp.clicked() {
            out.push(GuildUi::Row(i));
        }
        if view.selected == Some(i) || resp.hovered() {
            if let Some(bar) = col("GD_Member_Button_Static") {
                if let Some((_, t)) = bar.textures.first().cloned() {
                    control_image(ctx, hud, assets, painter, at(bar), bar.size, &t, 1.0);
                }
            }
        }
        for (name, text) in [("GD_Member_Name", &row.name), ("GD_Member_Level", &row.level), ("GD_Member_Class", &row.class), ("GD_Member_Race", &row.race)] {
            if let Some(c) = col(name) {
                area_text(painter, at(c), c, text, font, None);
            }
        }
        if let Some(c) = col("GD_Member_Status") {
            // Status2 0 (« ON ») : texture 1, Status2 1 (« OFF ») : texture 2.
            let want = if row.online { 1 } else { 2 };
            if let Some((_, t)) = c.textures.iter().find(|(k, _)| *k == want).cloned() {
                control_image(ctx, hud, assets, painter, at(c), c.size, &t, 1.0);
            }
        }
    }
}

/// Boîte OK / Annuler d'Archonia (AcUIOKCancelDialog), centrée à l'écran.
pub fn draw_message_box(ctx: &egui::Context, hud: &mut HudState, assets: &Assets, text: &str, ok: &str, cancel: &str, out: &mut Vec<GuildUi>) {
    const W: f32 = 412.0;
    const H: f32 = 180.0;
    let layout = Layout::new(ctx.screen_rect());
    let s = layout.scale;
    let size = Vec2::new(W, H) * s;
    let pos = ctx.screen_rect().center() - size / 2.0;
    egui::Area::new(egui::Id::new("archonia-ok-cancel")).fixed_pos(pos).order(egui::Order::Foreground).show(ctx, |ui| {
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let painter = ui.painter().clone();
        control_image(ctx, hud, assets, &painter, rect, [W, H], "Common_Message_Base2.png", 1.0);
        let font = (12.0 * s).max(9.0);
        // Lignes coupées à 311 px, 10 au plus.
        let mut lines: Vec<String> = Vec::new();
        for part in text.split("___") {
            let galley = painter.layout(part.trim().to_string(), FontId::proportional(font), Color32::WHITE, 311.0 * s);
            for row in &galley.rows {
                lines.push(row.glyphs.iter().map(|g| g.chr).collect::<String>().trim().to_string());
            }
        }
        lines.truncate(10);
        let n = lines.len() as f32;
        for (i, l) in lines.iter().enumerate() {
            let y = 77.0 - 7.0 * n + 14.0 * i as f32 + 7.0;
            shadow_text(&painter, rect.min + Vec2::new(206.0, y) * s, Align2::CENTER_CENTER, l, font, Color32::WHITE);
        }
        let small = UiControl {
            name: "msgbox".into(),
            kind: 3,
            size: [64.0, 17.0],
            textures: ["A", "B", "C", "D"].iter().enumerate().map(|(i, x)| (i as u32 + 1, format!("Common_Button_Small_{x}.png"))).collect(),
            ..Default::default()
        };
        for (x, label, value) in [(135.0, ok, true), (213.0, cancel, false)] {
            let r = Rect::from_min_size(rect.min + Vec2::new(x, 140.0) * s, Vec2::new(64.0, 17.0) * s);
            if button(ctx, ui, hud, assets, &painter, r, &small, label, font * 0.95, true, ("archonia-ok-cancel", value)).clicked() {
                out.push(GuildUi::MessageBox(value));
            }
        }
    });
}
