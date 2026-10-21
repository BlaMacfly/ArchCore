//! Fenêtres des portails et de l'agrandissement de la banque, d'après Archonia (sha256 e9c7609b…) et
//! `UI_1024X768.ini` (textes fixes de `ini_fr`) :
//!
//! * `UI_Teleport` (AgcmUIEventTeleport, FUN_1406ea7c0) : point de départ (`Teleport_Self`,
//!   `CONTROL_32370`), liste `Teleport_Targets` (`CONTROL_699`, `List=8` : 8 lignes de 21 px),
//!   chaque ligne un bouton `PORTAL_BAR.png` avec le nom (`Teleport_Name`, `CONTROL_30286`) et le prix
//!   (`Teleport_Fee`, `CONTROL_13306`, « %d %s » avec `Money_Name`) ; la couleur suit le statut posé
//!   par `TELEPORT_DisplayNormal / NoMoney / Disable` (`Status2` 0, 1, 2) ; boutons de page
//!   `Common_Button_PGDN/PGUP_Large` ; fermeture `CONTROL_9904`.
//! * `UI_LG_Loading` : écran de chargement (`TELEPORT_LOADING_WINDOW_OPEN/CLOSE`), de l'op 1 à l'op 8.
//! * `UI_BuyBank` : titre `Item_BuyBankSlotTitle` (pages achetées + 1, FUN_1406f6d70), prix dans
//!   l'édition `CONTROL_6373` au format « %s : <C16711680>%I64d<C16773320> %s » (`Item_SetBankPrice`,
//!   FUN_1406f71e0 : rouge 0xFF0000 si l'or porté ne suffit pas, 0x99FF00 sinon, reste 0xFFF0C8),
//!   boutons « Acheter » (`Item_BuyBankSlot`) et « Annuler ».

use super::npc_ui::{button, frame, window};
use super::*;

/// Demande des fenêtres de portail et d'agrandissement de la banque.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortalUi {
    CloseTeleport,
    /// Ligne choisie (indice dans la liste complète).
    Choose(usize),
    BuyBank,
    CancelBuyBank,
}

/// Destination affichée : nom, prix (déjà mis en forme) et statut (0 normal, 1 or insuffisant,
/// 2 indisponible).
pub struct TeleportRow {
    pub name: String,
    pub fee: String,
    pub status: usize,
}

pub struct TeleportView<'a> {
    /// Nom du point de départ.
    pub here: &'a str,
    pub rows: &'a [TeleportRow],
    /// Première ligne affichée.
    pub first: &'a mut usize,
}

pub struct BuyBankView {
    pub title: String,
    /// `Item_BuyBankSlotPrice`, prix, `Money_Name`.
    pub price_label: String,
    pub price: i64,
    pub money_name: String,
    /// L'or porté suffit (prix en vert, sinon en rouge).
    pub affordable: bool,
}

/// Texte fixe d'un contrôle (`CtrlDisplay=1:texte`).
fn static_text(c: &UiControl) -> &str {
    c.display.as_deref().and_then(|d| d.strip_prefix("1:")).unwrap_or("")
}

fn argb(c: u32) -> Color32 {
    Color32::from_rgba_unmultiplied((c >> 16) as u8, (c >> 8) as u8, c as u8, (c >> 24) as u8)
}

/// Texte placé selon l'alignement de `CtrlFont` (16 à gauche, 17 centré, 18 à droite).
fn control_text(p: &egui::Painter, r: Rect, c: &UiControl, text: &str, size: f32, color: Option<Color32>) {
    let (font_color, align) = c.font.unwrap_or((u32::MAX, 16));
    let color = color.unwrap_or(argb(font_color));
    let (pos, a) = match align {
        17 => (r.center(), Align2::CENTER_CENTER),
        18 => (r.right_center(), Align2::RIGHT_CENTER),
        _ => (r.left_center(), Align2::LEFT_CENTER),
    };
    shadow_text(p, pos, a, text, size, color);
}

/// Bouton d'après `Button=` : texture de l'état (repos, survol, appui, inactif), aucune pour
/// l'indice 0 (les lignes de la liste n'ont `PORTAL_BAR.png` qu'au survol et à l'appui) ; sans
/// `Button=`, le bouton commun.
#[allow(clippy::too_many_arguments)]
fn state_button(ctx: &egui::Context, ui: &mut egui::Ui, hud: &mut HudState, assets: &Assets, painter: &egui::Painter, r: Rect, c: &UiControl, label: &str, font: f32, enabled: bool, id: impl std::hash::Hash) -> egui::Response {
    let Some(states) = c.button_states else { return button(ctx, ui, hud, assets, painter, r, c, label, font, enabled, id) };
    let resp = ui.interact(r, egui::Id::new(id), if enabled { egui::Sense::click() } else { egui::Sense::hover() });
    let state = if !enabled {
        3
    } else if resp.is_pointer_button_down_on() {
        2
    } else if resp.hovered() {
        1
    } else {
        0
    };
    if let Some((_, tex)) = c.textures.iter().find(|(i, _)| states[state] != 0 && *i == states[state]).cloned() {
        control_image(ctx, hud, assets, painter, r, c.size, &tex, 1.0);
    }
    if !label.is_empty() {
        shadow_text(painter, r.center(), Align2::CENTER_CENTER, label, font, if enabled { Color32::WHITE } else { Color32::GRAY });
    }
    resp
}

/// Liste des destinations d'un portail.
pub fn draw_teleport(ctx: &egui::Context, hud: &mut HudState, assets: &Assets, view: TeleportView, out: &mut Vec<PortalUi>) {
    let Some(w) = window(hud, assets, "UI_Teleport") else { return };
    let visible = 8usize;
    let row_h = w.control("CONTROL_9496").map_or(21.0, |c| c.size[1]);
    frame(ctx, hud, assets, &w, "archonia-teleport", |ui, hud, layout, origin, painter| {
        let s = layout.scale;
        let font = (11.0 * s).max(9.0);
        let last_first = view.rows.len().saturating_sub(visible);
        *view.first = (*view.first).min(last_first);
        for c in &w.controls {
            let r = layout.rect(origin, c.pos, c.size);
            match c.name.as_str() {
                "CONTROL_9904" => {
                    if state_button(ctx, ui, hud, assets, painter, r, c, "", font, true, "teleport-x").clicked() {
                        out.push(PortalUi::CloseTeleport);
                    }
                }
                "CONTROL_22424" | "CONTROL_9957" | "CONTROL_23219" => control_text(painter, r, c, static_text(c), font, None),
                "CONTROL_32370" => control_text(painter, r, c, view.here, font, None),
                "CONTROL_31369" => {
                    let name_c = w.control("CONTROL_30286");
                    let fee_c = w.control("CONTROL_13306");
                    for i in 0..visible {
                        let idx = *view.first + i;
                        let Some(row) = view.rows.get(idx) else { break };
                        let dy = i as f32 * row_h;
                        let rr = layout.rect(origin, [c.pos[0], c.pos[1] + dy], c.size);
                        if state_button(ctx, ui, hud, assets, painter, rr, c, "", font, row.status != 2, ("teleport-row", idx)).clicked() {
                            out.push(PortalUi::Choose(idx));
                        }
                        for (tc, text) in [(name_c, &row.name), (fee_c, &row.fee)] {
                            let Some(tc) = tc else { continue };
                            let tr = layout.rect(origin, [tc.pos[0], tc.pos[1] + dy], tc.size);
                            let color = tc.status_colors.get(row.status).map(|v| argb(*v));
                            control_text(painter, tr, tc, text, font, color);
                        }
                    }
                }
                // Boutons de page sous la liste.
                "CONTROL_25406" | "CONTROL_28128" => {
                    let down = c.name == "CONTROL_25406";
                    let enabled = if down { *view.first < last_first } else { *view.first > 0 };
                    if state_button(ctx, ui, hud, assets, painter, r, c, "", font, enabled, ("teleport-page", down)).clicked() {
                        *view.first = if down { (*view.first + visible).min(last_first) } else { view.first.saturating_sub(visible) };
                    }
                }
                _ => {}
            }
        }
    });
}

/// Écran de chargement d'Archonia (`UI_LG_Loading`, `LG_Loading_1024.png`) étiré sur l'écran.
pub fn draw_loading(ctx: &egui::Context, hud: &mut HudState, assets: &Assets) {
    let Some(w) = window(hud, assets, "UI_LG_Loading") else { return };
    let Some((_, tex)) = w.textures.first().cloned() else { return };
    let screen = ctx.screen_rect();
    egui::Area::new(egui::Id::new("archonia-loading")).fixed_pos(screen.min).order(egui::Order::Foreground).interactable(true).show(ctx, |ui| {
        let (rect, _) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
        let painter = ui.painter().clone();
        painter.rect_filled(rect, 0.0, Color32::BLACK);
        control_image(ctx, hud, assets, &painter, rect, w.size, &tex, 1.0);
    });
}

/// Achat d'une page de banque.
pub fn draw_buy_bank(ctx: &egui::Context, hud: &mut HudState, assets: &Assets, view: &BuyBankView, out: &mut Vec<PortalUi>) {
    let Some(w) = window(hud, assets, "UI_BuyBank") else { return };
    frame(ctx, hud, assets, &w, "archonia-buy-bank", |ui, hud, layout, origin, painter| {
        let s = layout.scale;
        let font = (11.0 * s).max(9.0);
        for c in &w.controls {
            let r = layout.rect(origin, c.pos, c.size);
            match c.name.as_str() {
                "CONTROL_25701" => control_text(painter, r, c, &view.title, font, None),
                "CONTROL_6373" => {
                    let cream = argb(0xFF_FF_F0_C8);
                    let price = if view.affordable { argb(0xFF_99_FF_00) } else { argb(0xFF_FF_00_00) };
                    let mut job = egui::text::LayoutJob::default();
                    let f = |color| egui::TextFormat { font_id: FontId::proportional(font), color, ..Default::default() };
                    job.append(&format!("{} : ", view.price_label), 0.0, f(cream));
                    job.append(&view.price.to_string(), 0.0, f(price));
                    job.append(&format!(" {}", view.money_name), 0.0, f(cream));
                    let galley = painter.layout_job(job);
                    let at = r.center() - galley.size() / 2.0;
                    painter.galley(at, galley, cream);
                }
                "CONTROL_23125" | "CONTROL_27388" => {
                    let buy = c.name == "CONTROL_23125";
                    if state_button(ctx, ui, hud, assets, painter, r, c, static_text(c), font, true, ("buy-bank", buy)).clicked() {
                        out.push(if buy { PortalUi::BuyBank } else { PortalUi::CancelBuyBank });
                    }
                }
                _ => {}
            }
        }
    });
}
