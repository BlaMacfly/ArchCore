//! Dessin des frames Lua des AddOns avec egui : fonds et bordures, textures (fichiers des AddOns ou
//! textures du client, coordonnées de texture, teinte), textes avec codes couleur WoW et polices
//! `.ttf`/`.otf` des AddOns, barres, curseurs, cases à cocher, champs de saisie. Rassemble aussi
//! l'entrée souris et les événements des EditBox, rendus au gestionnaire après l'image.

use crate::assets::Assets;
use archrust_addons::ui::{
    color_segments, strata_rank, strip_codes, FrameKind, FrameNode, Rect, TextureSource,
};
use archrust_addons::{AddonManager, EditEvent, PointerInput};
use egui::text::{LayoutJob, TextFormat};
use egui::{Align, Align2, Color32, FontFamily, FontId, Pos2, Stroke};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

/// Couleurs de l'interface native modifiables par `ArchRust.SetUIColor(clé, r, g, b, a)`.
pub const UI_COLOR_KEYS: &[(&str, &str)] = &[
    ("windowFill", "fond des fenêtres"),
    ("panelFill", "fond des panneaux"),
    ("windowStroke", "bordure des fenêtres"),
    ("text", "texte (remplace la couleur par défaut)"),
    ("accent", "sélection, éléments actifs"),
    ("hyperlink", "liens"),
    ("widgetFill", "fond des boutons et champs"),
    ("widgetHover", "boutons survolés"),
    ("extremeBg", "fond des champs de saisie"),
    ("faintBg", "lignes alternées"),
];

/// Sorties d'une image d'interface pour le gestionnaire d'AddOns.
#[derive(Default)]
pub struct AddonUiOutput {
    pub pointer: PointerInput,
    pub edits: Vec<(usize, EditEvent)>,
}

#[derive(Default)]
pub struct AddonUi {
    textures: HashMap<TextureSource, Option<egui::TextureHandle>>,
    /// Polices des AddOns : chemin → famille egui (prête à l'image suivante).
    fonts: HashMap<PathBuf, Option<String>>,
    fonts_ready: HashSet<PathBuf>,
    font_defs: Option<egui::FontDefinitions>,
    default_visuals: Option<egui::Visuals>,
    /// Capture demandée par `Screenshot()` : chemin du fichier.
    pub screenshot: Option<PathBuf>,
}

fn color(c: [f32; 4], alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (c[0].clamp(0.0, 1.0) * 255.0) as u8,
        (c[1].clamp(0.0, 1.0) * 255.0) as u8,
        (c[2].clamp(0.0, 1.0) * 255.0) as u8,
        ((c[3] * alpha).clamp(0.0, 1.0) * 255.0) as u8,
    )
}

fn to_egui(r: Rect, sh: f32) -> egui::Rect {
    egui::Rect::from_min_size(
        Pos2::new(r.left, sh - r.bottom - r.height),
        egui::vec2(r.width.max(0.0), r.height.max(0.0)),
    )
}

impl AddonUi {
    /// Applique les couleurs d'interface demandées par les AddOns (vide = thème d'origine).
    pub fn apply_ui_colors(&mut self, ctx: &egui::Context, colors: &BTreeMap<String, [f32; 4]>) {
        let base = self
            .default_visuals
            .get_or_insert_with(|| ctx.style().visuals.clone())
            .clone();
        let mut v = base;
        for (k, c) in colors {
            let col = color(*c, 1.0);
            match k.as_str() {
                "windowFill" => v.window_fill = col,
                "panelFill" => v.panel_fill = col,
                "windowStroke" => v.window_stroke.color = col,
                "text" => v.override_text_color = Some(col),
                "accent" => {
                    v.selection.bg_fill = col;
                    v.widgets.active.bg_fill = col;
                }
                "hyperlink" => v.hyperlink_color = col,
                "widgetFill" => {
                    v.widgets.inactive.bg_fill = col;
                    v.widgets.inactive.weak_bg_fill = col;
                }
                "widgetHover" => {
                    v.widgets.hovered.bg_fill = col;
                    v.widgets.hovered.weak_bg_fill = col;
                }
                "extremeBg" => v.extreme_bg_color = col,
                "faintBg" => v.faint_bg_color = col,
                other => log::debug!("SetUIColor : clé inconnue {other}"),
            }
        }
        ctx.set_visuals(v);
    }

    fn family_for(&self, f: &FrameNode) -> FontFamily {
        match &f.font_path {
            Some(p) if self.fonts_ready.contains(p) => match self.fonts.get(p) {
                Some(Some(name)) => FontFamily::Name(name.as_str().into()),
                _ => FontFamily::Proportional,
            },
            _ => FontFamily::Proportional,
        }
    }

    /// Enregistre les polices `.ttf`/`.otf` nouvellement demandées (actives à l'image suivante).
    fn register_fonts(&mut self, ctx: &egui::Context, mgr: &AddonManager) {
        // Les polices enregistrées à l'image précédente sont maintenant utilisables : les textes
        // qui les emploient sont à remesurer.
        let mut newly_ready = Vec::new();
        for (p, fam) in &self.fonts {
            if fam.is_some() && self.fonts_ready.insert(p.clone()) {
                newly_ready.push(p.clone());
            }
        }
        if !newly_ready.is_empty() {
            let mut s = mgr.shared.borrow_mut();
            for f in s.ui.frames.iter_mut() {
                if f.font_path.as_ref().is_some_and(|p| newly_ready.contains(p)) {
                    f.measured = None;
                }
            }
        }
        let wanted: Vec<PathBuf> = {
            let s = mgr.shared.borrow();
            s.ui.frames
                .iter()
                .filter_map(|f| f.font_path.clone())
                .filter(|p| !self.fonts.contains_key(p))
                .collect::<HashSet<_>>()
                .into_iter()
                .collect()
        };
        if wanted.is_empty() {
            return;
        }
        let defs = self
            .font_defs
            .get_or_insert_with(egui::FontDefinitions::default);
        let fallback = defs
            .families
            .get(&FontFamily::Proportional)
            .cloned()
            .unwrap_or_default();
        let mut changed = false;
        for p in wanted {
            match std::fs::read(&p) {
                Ok(bytes) => {
                    let name = format!("addon:{}", p.display());
                    defs.font_data
                        .insert(name.clone(), Arc::new(egui::FontData::from_owned(bytes)));
                    let mut fam = vec![name.clone()];
                    fam.extend(fallback.iter().cloned());
                    defs.families
                        .insert(FontFamily::Name(name.as_str().into()), fam);
                    log::info!("police d'AddOn chargée : {}", p.display());
                    self.fonts.insert(p, Some(name));
                    changed = true;
                }
                Err(e) => {
                    log::warn!("police {} : {e}", p.display());
                    self.fonts.insert(p, None);
                }
            }
        }
        if changed {
            ctx.set_fonts(defs.clone());
        }
    }

    fn texture(
        &mut self,
        ctx: &egui::Context,
        assets: &Assets,
        src: &TextureSource,
    ) -> Option<egui::TextureId> {
        if let Some(t) = self.textures.get(src) {
            return t.as_ref().map(|t| t.id());
        }
        let img: Option<egui::ColorImage> = match src {
            TextureSource::File(p) => match image::open(p) {
                Ok(i) => {
                    let i = i.to_rgba8();
                    Some(egui::ColorImage::from_rgba_unmultiplied(
                        [i.width() as usize, i.height() as usize],
                        i.as_raw(),
                    ))
                }
                Err(e) => {
                    log::warn!("texture d'AddOn {} : {e}", p.display());
                    None
                }
            },
            TextureSource::Game(name) => {
                let decoded = if name.contains('/') && name.contains('.') {
                    assets
                        .read_file(name)
                        .ok()
                        .and_then(|b| image::load_from_memory(&b).ok())
                        .map(|i| {
                            let i = i.to_rgba8();
                            egui::ColorImage::from_rgba_unmultiplied(
                                [i.width() as usize, i.height() as usize],
                                i.as_raw(),
                            )
                        })
                } else {
                    None
                };
                decoded.or_else(|| {
                    assets.texture(name).ok().map(|t| {
                        egui::ColorImage::from_rgba_unmultiplied(
                            [t.width as usize, t.height as usize],
                            &t.data,
                        )
                    })
                })
            }
        };
        let handle = img.map(|i| {
            ctx.load_texture(
                format!("addon-tex:{src:?}"),
                i,
                egui::TextureOptions::LINEAR,
            )
        });
        let id = handle.as_ref().map(|h| h.id());
        self.textures.insert(src.clone(), handle);
        id
    }

    /// Texte coloré (codes `|cAARRGGBB…|r`) en LayoutJob.
    fn job(&self, f: &FrameNode, text: &str, base: Color32, wrap: Option<f32>) -> LayoutJob {
        let font = FontId::new(f.font_size.max(1.0), self.family_for(f));
        let mut job = LayoutJob::default();
        for (seg, c) in color_segments(text) {
            let col = c.map_or(base, |c| color(c, base.a() as f32 / 255.0));
            job.append(
                &seg,
                0.0,
                TextFormat {
                    font_id: font.clone(),
                    color: col,
                    ..Default::default()
                },
            );
        }
        if let Some(w) = wrap {
            job.wrap.max_width = w.max(1.0);
        }
        // Le bloc est placé selon `justify_h` par l'appelant.
        job.halign = Align::LEFT;
        job
    }

    /// Mesure les FontStrings avec la police réelle (pour la mise en page et `GetStringWidth`).
    fn measure(&self, ctx: &egui::Context, mgr: &AddonManager) {
        let todo: Vec<(usize, LayoutJob)> = {
            let s = mgr.shared.borrow();
            s.ui.frames
                .iter()
                .filter(|f| {
                    matches!(f.kind, FrameKind::FontString | FrameKind::Button)
                        && f.measured.is_none()
                })
                .map(|f| {
                    (f.id, self.job(f, &f.text, Color32::WHITE, None))
                })
                .collect()
        };
        if todo.is_empty() {
            return;
        }
        let sizes: Vec<(usize, (f32, f32))> = ctx.fonts(|fonts| {
            todo.into_iter()
                .map(|(id, job)| {
                    let g = fonts.layout_job(job);
                    (id, (g.size().x.ceil(), g.size().y.ceil()))
                })
                .collect()
        });
        let mut s = mgr.shared.borrow_mut();
        for (id, size) in sizes {
            if let Some(f) = s.ui.frames.get_mut(id) {
                f.measured = Some(size);
            }
        }
    }

    /// Dessine les frames et rassemble l'entrée de l'image.
    pub fn draw(
        &mut self,
        ctx: &egui::Context,
        mgr: &AddonManager,
        assets: &Assets,
        out: &mut AddonUiOutput,
    ) {
        self.register_fonts(ctx, mgr);
        // Une police qui vient d'être prête change les mesures.
        self.measure(ctx, mgr);
        let screen = ctx.screen_rect();
        let (sw, sh) = (screen.width(), screen.height());
        mgr.layout(sw, sh, &|f| {
            f.measured.unwrap_or_else(|| f.estimated_text_size())
        });

        // Entrée souris : une fenêtre native egui sous le curseur masque les strates basses.
        let over_native = ctx.is_pointer_over_area();
        out.pointer.native_over = over_native;
        ctx.input(|i| {
            out.pointer.pos = i
                .pointer
                .hover_pos()
                .map(|p| (p.x, sh - p.y));
            for (b, name) in [
                (egui::PointerButton::Primary, "LeftButton"),
                (egui::PointerButton::Secondary, "RightButton"),
                (egui::PointerButton::Middle, "MiddleButton"),
            ] {
                if i.pointer.button_pressed(b) {
                    out.pointer.pressed.push(name.to_string());
                }
                if i.pointer.button_released(b) {
                    out.pointer.released.push(name.to_string());
                }
            }
            {
                let d = i.raw_scroll_delta.y;
                out.pointer.wheel = if d > 0.0 {
                    1.0
                } else if d < 0.0 {
                    -1.0
                } else {
                    0.0
                };
            }
        });

        // Strates basses (jusqu'à MEDIUM) sous les fenêtres natives, HIGH à FULLSCREEN_DIALOG
        // au-dessus, TOOLTIP tout en haut.
        let painters = [
            ctx.layer_painter(egui::LayerId::new(
                egui::Order::Background,
                egui::Id::new("addon-frames-low"),
            )),
            ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("addon-frames-high"),
            )),
            ctx.layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                egui::Id::new("addon-frames-tooltip"),
            )),
        ];
        let hovered = mgr.hovered_frame();
        let s = mgr.shared.borrow();
        let tree = &s.ui;
        let order = tree.draw_order();
        // Travail différé hors de l'emprunt de l'arbre : EditBox (widgets egui).
        let mut edit_boxes: Vec<(usize, egui::Rect, String, f32, bool, bool, bool, Color32, bool)> =
            Vec::new();
        for id in order {
            let f = &tree.frames[id];
            if f.builtin || !tree.is_visible(id) {
                continue;
            }
            let Some(r) = tree.rects.get(id).copied().flatten() else {
                continue;
            };
            if r.width <= 0.0 && r.height <= 0.0 && f.kind != FrameKind::FontString {
                continue;
            }
            let rect = to_egui(r, sh);
            if !rect.intersects(screen) {
                continue;
            }
            let alpha = tree.effective_alpha(id);
            if alpha <= 0.001 {
                continue;
            }
            let owner = if f.kind.is_region() {
                f.parent.unwrap_or(id)
            } else {
                id
            };
            let rank = strata_rank(tree.effective_strata(owner));
            let painter = &painters[match rank {
                r if r >= strata_rank("TOOLTIP") => 2,
                r if r >= strata_rank("HIGH") => 1,
                _ => 0,
            }];
            // Fond et bordure (backdrop ou couleur de frame).
            if !f.kind.is_region() {
                let bg = f.background();
                if bg[3] > 0.0 {
                    let bd_tex = f
                        .backdrop
                        .as_ref()
                        .and_then(|b| b.bg_texture.clone());
                    match bd_tex.and_then(|t| self.texture(ctx, assets, &t)) {
                        Some(tid) => {
                            painter.image(
                                tid,
                                rect,
                                egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                color(bg, alpha),
                            );
                        }
                        None => {
                            painter.rect_filled(rect, 3.0, color(bg, alpha));
                        }
                    }
                }
                if let Some(b) = &f.backdrop {
                    if b.border[3] > 0.0 && b.edge_size > 0.0 {
                        painter.rect_stroke(
                            rect,
                            3.0,
                            Stroke::new(b.edge_size.min(4.0), color(b.border, alpha)),
                            egui::StrokeKind::Inside,
                        );
                    }
                }
            }
            match f.kind {
                FrameKind::Texture => {
                    let uv = egui::Rect::from_min_max(
                        Pos2::new(f.tex_coord[0], f.tex_coord[2]),
                        Pos2::new(f.tex_coord[1], f.tex_coord[3]),
                    );
                    match f
                        .texture_source
                        .clone()
                        .and_then(|t| self.texture(ctx, assets, &t))
                    {
                        Some(tid) => {
                            painter.image(tid, rect, uv, color(f.color, alpha));
                        }
                        None if f.solid => {
                            painter.rect_filled(rect, 0.0, color(f.color, alpha));
                        }
                        None => {}
                    }
                }
                FrameKind::FontString => {
                    if f.text.is_empty() {
                        continue;
                    }
                    let base = color(f.color, alpha);
                    let wrap = (f.word_wrap && f.width > 0.0).then_some(rect.width());
                    let job = self.job(f, &f.text, base, wrap);
                    let galley = ctx.fonts(|fonts| fonts.layout_job(job));
                    let size = galley.size();
                    let x = match f.justify_h.as_str() {
                        "CENTER" => rect.center().x - size.x / 2.0,
                        "RIGHT" => rect.right() - size.x,
                        _ => rect.left(),
                    };
                    let y = match f.justify_v.as_str() {
                        "TOP" => rect.top(),
                        "BOTTOM" => rect.bottom() - size.y,
                        _ => rect.center().y - size.y / 2.0,
                    };
                    let pos = Pos2::new(x, y);
                    let outline = f.font_flags.contains("OUTLINE");
                    let shadow_col = Color32::from_black_alpha((alpha * 200.0) as u8);
                    if outline {
                        for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
                            painter.galley_with_override_text_color(
                                pos + egui::vec2(dx, dy),
                                galley.clone(),
                                shadow_col,
                            );
                        }
                    } else if f.shadow {
                        painter.galley_with_override_text_color(
                            pos + egui::vec2(1.0, 1.0),
                            galley.clone(),
                            shadow_col,
                        );
                    }
                    painter.galley(pos, galley, base);
                }
                FrameKind::StatusBar => {
                    if f.backdrop.is_none() {
                        painter.rect_filled(rect, 2.0, Color32::from_black_alpha((alpha * 128.0) as u8));
                    }
                    let frac = if f.max_value > f.min_value {
                        ((f.value - f.min_value) / (f.max_value - f.min_value)).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let mut fill = rect;
                    if f.orientation == "VERTICAL" {
                        fill.set_top(rect.bottom() - rect.height() * frac);
                    } else {
                        fill.set_width(rect.width() * frac);
                    }
                    match f
                        .texture_source
                        .clone()
                        .and_then(|t| self.texture(ctx, assets, &t))
                    {
                        Some(tid) => {
                            let uv = if f.orientation == "VERTICAL" {
                                egui::Rect::from_min_max(Pos2::new(0.0, 1.0 - frac), Pos2::new(1.0, 1.0))
                            } else {
                                egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(frac, 1.0))
                            };
                            painter.image(tid, fill, uv, color(f.color, alpha));
                        }
                        None => {
                            painter.rect_filled(fill, 2.0, color(f.color, alpha));
                        }
                    }
                }
                FrameKind::Slider => {
                    if f.backdrop.is_none() {
                        let track = egui::Rect::from_center_size(
                            rect.center(),
                            if f.orientation == "VERTICAL" {
                                egui::vec2(4.0, rect.height())
                            } else {
                                egui::vec2(rect.width(), 4.0)
                            },
                        );
                        painter.rect_filled(track, 2.0, Color32::from_rgba_unmultiplied(40, 40, 40, (alpha * 220.0) as u8));
                    }
                    let frac = if f.max_value > f.min_value {
                        ((f.value - f.min_value) / (f.max_value - f.min_value)).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let center = if f.orientation == "VERTICAL" {
                        Pos2::new(rect.center().x, rect.top() + rect.height() * (1.0 - frac))
                    } else {
                        Pos2::new(rect.left() + rect.width() * frac, rect.center().y)
                    };
                    let thumb = egui::Rect::from_center_size(center, egui::vec2(10.0, rect.height().clamp(10.0, 20.0)));
                    painter.rect_filled(thumb, 2.0, Color32::from_rgba_unmultiplied(220, 180, 90, (alpha * 255.0) as u8));
                }
                FrameKind::Button | FrameKind::CheckButton => {
                    let is_hover = hovered == Some(id);
                    let a8 = (alpha * 255.0) as u8;
                    let check = f.kind == FrameKind::CheckButton;
                    let tex = f
                        .texture_source
                        .clone()
                        .and_then(|t| self.texture(ctx, assets, &t));
                    let bx = if check {
                        egui::Rect::from_min_size(rect.min, egui::vec2(rect.height(), rect.height()))
                    } else {
                        rect
                    };
                    match tex {
                        Some(tid) => {
                            let uv = egui::Rect::from_min_max(
                                Pos2::new(f.tex_coord[0], f.tex_coord[2]),
                                Pos2::new(f.tex_coord[1], f.tex_coord[3]),
                            );
                            let tint = if is_hover {
                                color([f.color[0].min(1.0), f.color[1], f.color[2], f.color[3]], alpha)
                            } else {
                                color(f.color, alpha * 0.92)
                            };
                            painter.image(tid, bx, uv, tint);
                        }
                        None if f.backdrop.is_none() => {
                            let base = match (f.enabled, is_hover) {
                                (false, _) => Color32::from_rgba_unmultiplied(45, 40, 35, a8),
                                (true, true) => Color32::from_rgba_unmultiplied(110, 80, 30, a8),
                                (true, false) => Color32::from_rgba_unmultiplied(70, 45, 18, a8),
                            };
                            painter.rect_filled(bx, 4.0, base);
                            painter.rect_stroke(
                                bx,
                                4.0,
                                Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(200, 160, 80, a8)),
                                egui::StrokeKind::Inside,
                            );
                        }
                        None => {
                            if is_hover {
                                painter.rect_filled(bx, 3.0, Color32::from_white_alpha((alpha * 25.0) as u8));
                            }
                        }
                    }
                    if check && f.checked {
                        painter.text(
                            bx.center(),
                            Align2::CENTER_CENTER,
                            "✔",
                            FontId::proportional(bx.height() * 0.8),
                            Color32::from_rgba_unmultiplied(255, 210, 60, a8),
                        );
                    }
                    if !f.text.is_empty() {
                        let col = if f.enabled {
                            Color32::from_rgba_unmultiplied(255, 220, 120, a8)
                        } else {
                            Color32::from_rgba_unmultiplied(140, 140, 140, a8)
                        };
                        let job = self.job(f, &f.text, col, None);
                        let galley = ctx.fonts(|fonts| fonts.layout_job(job));
                        let pos = if check {
                            Pos2::new(bx.right() + 4.0, bx.center().y - galley.size().y / 2.0)
                        } else {
                            rect.center() - galley.size() / 2.0
                        };
                        painter.galley(pos, galley, col);
                    }
                }
                FrameKind::EditBox => {
                    let col = color(f.color, alpha);
                    let text = if f.focused { f.text.clone() } else { strip_codes(&f.text) };
                    edit_boxes.push((
                        id,
                        rect,
                        text,
                        f.font_size,
                        f.focused,
                        f.multi_line,
                        f.password,
                        col,
                        rank >= strata_rank("HIGH"),
                    ));
                }
                _ => {}
            }
        }
        drop(s);

        // EditBox : vrais champs de saisie egui posés sur la frame.
        for (id, rect, mut text, size, focused, multi, password, col, high) in edit_boxes {
            let before = text.clone();
            let area = egui::Area::new(egui::Id::new(("addon-editbox", id)))
                .fixed_pos(rect.min)
                // Au-dessus du fond de sa frame (couche des strates hautes = Foreground).
                .order(if high {
                    egui::Order::Tooltip
                } else {
                    egui::Order::Middle
                })
                .show(ctx, |ui| {
                    ui.set_width(rect.width());
                    let font = FontId::proportional(size.max(8.0));
                    let te = if multi {
                        egui::TextEdit::multiline(&mut text)
                    } else {
                        egui::TextEdit::singleline(&mut text)
                    };
                    ui.add_sized(
                        rect.size(),
                        te.font(font)
                            .text_color(col)
                            .password(password)
                            .frame(false)
                            .desired_width(rect.width()),
                    )
                });
            let resp = area.inner;
            if focused && !resp.has_focus() && !resp.lost_focus() {
                resp.request_focus();
            }
            if resp.gained_focus() {
                out.edits.push((id, EditEvent::FocusGained));
            }
            if text != before {
                out.edits.push((id, EditEvent::Changed(text.clone())));
            }
            if resp.lost_focus() {
                let (enter, esc, tab) = ctx.input(|i| {
                    (
                        i.key_pressed(egui::Key::Enter),
                        i.key_pressed(egui::Key::Escape),
                        i.key_pressed(egui::Key::Tab),
                    )
                });
                if enter {
                    out.edits.push((id, EditEvent::Enter));
                } else if esc {
                    out.edits.push((id, EditEvent::Escape));
                } else if tab {
                    out.edits.push((id, EditEvent::Tab));
                }
                out.edits.push((id, EditEvent::FocusLost));
            }
        }
    }
}
