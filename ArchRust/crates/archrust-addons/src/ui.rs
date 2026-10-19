//! Arbre de frames décrit par les AddOns et dessiné par le moteur.
//!
//! Le modèle suit celui de WoW : chaque frame a un type, un parent, des ancres (`SetPoint`), une
//! taille, une échelle, une visibilité, une strate et un niveau, et des scripts (`OnEvent`,
//! `OnUpdate`, `OnClick`…) conservés côté Lua. Les textures et les FontStrings (« régions ») sont
//! dessinées au niveau de leur frame parente, dans l'ordre de leur couche (`BACKGROUND` →
//! `OVERLAY`).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrameKind {
    Frame,
    Button,
    FontString,
    Texture,
    EditBox,
    StatusBar,
    CheckButton,
    Slider,
    ScrollFrame,
    GameTooltip,
}

impl FrameKind {
    pub fn parse(s: &str) -> FrameKind {
        match s.to_ascii_lowercase().as_str() {
            "button" | "itembutton" | "dropdownbutton" => FrameKind::Button,
            "fontstring" => FrameKind::FontString,
            "texture" => FrameKind::Texture,
            "editbox" => FrameKind::EditBox,
            "statusbar" | "cooldown" => FrameKind::StatusBar,
            "checkbutton" => FrameKind::CheckButton,
            "slider" => FrameKind::Slider,
            "scrollframe" => FrameKind::ScrollFrame,
            "gametooltip" => FrameKind::GameTooltip,
            _ => FrameKind::Frame,
        }
    }

    /// Nom WoW du type (`GetObjectType`).
    pub fn name(self) -> &'static str {
        match self {
            FrameKind::Frame => "Frame",
            FrameKind::Button => "Button",
            FrameKind::FontString => "FontString",
            FrameKind::Texture => "Texture",
            FrameKind::EditBox => "EditBox",
            FrameKind::StatusBar => "StatusBar",
            FrameKind::CheckButton => "CheckButton",
            FrameKind::Slider => "Slider",
            FrameKind::ScrollFrame => "ScrollFrame",
            FrameKind::GameTooltip => "GameTooltip",
        }
    }

    /// Régions : dessinées au niveau de leur parent, sans scripts d'interaction propres.
    pub fn is_region(self) -> bool {
        matches!(self, FrameKind::FontString | FrameKind::Texture)
    }
}

/// Strates de WoW, de la plus basse à la plus haute.
pub const STRATA: [&str; 9] = [
    "WORLD",
    "BACKGROUND",
    "LOW",
    "MEDIUM",
    "HIGH",
    "DIALOG",
    "FULLSCREEN",
    "FULLSCREEN_DIALOG",
    "TOOLTIP",
];

pub fn strata_rank(s: &str) -> usize {
    STRATA.iter().position(|x| *x == s).unwrap_or(3)
}

/// Couches de dessin des régions.
pub fn layer_rank(s: &str) -> usize {
    match s {
        "BACKGROUND" => 0,
        "BORDER" => 1,
        "ARTWORK" => 2,
        "OVERLAY" => 3,
        "HIGHLIGHT" => 4,
        _ => 2,
    }
}

/// Point d'ancrage (`TOPLEFT`, `CENTER`…), normalisé en majuscules.
#[derive(Debug, Clone, PartialEq)]
pub struct Anchor {
    pub point: String,
    /// `None` = parent (ou l'écran pour une frame racine).
    pub relative_to: Option<usize>,
    pub relative_point: String,
    pub x: f32,
    pub y: f32,
}

/// Origine d'une texture, résolue à l'appel de `SetTexture`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TextureSource {
    /// Fichier image d'un AddOn (`Interface\AddOns\<Nom>\…` ou chemin relatif à l'AddOn).
    File(PathBuf),
    /// Texture du client par nom RenderWare (`A00005AA`) ou chemin du système de fichiers virtuel.
    Game(String),
}

/// Fond et bordure (`SetBackdrop`, `SetBackdropColor`, `SetBackdropBorderColor`).
#[derive(Debug, Clone, PartialEq)]
pub struct Backdrop {
    pub bg: [f32; 4],
    pub border: [f32; 4],
    pub edge_size: f32,
    pub bg_texture: Option<TextureSource>,
}

impl Default for Backdrop {
    fn default() -> Self {
        Backdrop {
            bg: [0.0, 0.0, 0.0, 0.0],
            border: [0.0, 0.0, 0.0, 0.0],
            edge_size: 0.0,
            bg_texture: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FrameNode {
    pub id: usize,
    pub kind: FrameKind,
    pub name: Option<String>,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub shown: bool,
    pub width: f32,
    pub height: f32,
    pub scale: f32,
    pub anchors: Vec<Anchor>,
    pub text: String,
    /// Couleur de la texture, du texte ou de la barre, RGBA 0..1.
    pub color: [f32; 4],
    /// Texture telle que donnée par l'AddOn (`GetTexture`).
    pub texture: Option<String>,
    pub texture_source: Option<TextureSource>,
    /// Coordonnées de texture : gauche, droite, haut, bas.
    pub tex_coord: [f32; 4],
    pub backdrop: Option<Backdrop>,
    pub alpha: f32,
    /// `None` = strate du parent.
    pub strata: Option<String>,
    /// `None` = niveau du parent + 1.
    pub level: Option<i32>,
    /// Couche de dessin des régions (`BACKGROUND`, `ARTWORK`, `OVERLAY`…).
    pub layer: String,
    pub mouse_enabled: bool,
    pub mouse_wheel_enabled: bool,
    pub keyboard_enabled: bool,
    pub movable: bool,
    pub clamped: bool,
    pub drag_buttons: Vec<String>,
    /// Police : chemin (fichier `.ttf`/`.otf` d'un AddOn, ou `nil` = police du client).
    pub font_path: Option<PathBuf>,
    pub font_size: f32,
    /// `OUTLINE`, `THICKOUTLINE`, `MONOCHROME`… (contour dessiné si `OUTLINE`).
    pub font_flags: String,
    pub shadow: bool,
    pub justify_h: String,
    pub justify_v: String,
    pub word_wrap: bool,
    /// Valeur courante / min / max / pas pour StatusBar et Slider.
    pub value: f32,
    pub min_value: f32,
    pub max_value: f32,
    pub value_step: f32,
    pub orientation: String,
    pub checked: bool,
    pub enabled: bool,
    /// EditBox
    pub focused: bool,
    pub multi_line: bool,
    pub max_letters: usize,
    pub numeric: bool,
    pub password: bool,
    /// Frame du moteur (fenêtre de discussion, panneau d'informations…) : non dessinée par le
    /// moteur d'AddOns, mais `Show`/`Hide`/`SetAlpha` pilotent le panneau natif.
    pub builtin: bool,
    /// Texture unie (`SetColorTexture`, ou texture WoW sans équivalent) : remplie de `color`.
    pub solid: bool,
    /// FontString créée par `GetFontString` sur un bouton : elle reçoit le texte du bouton.
    pub font_string: Option<usize>,
    /// Taille mesurée du texte par le moteur (police réelle) ; sert à `GetStringWidth`.
    pub measured: Option<(f32, f32)>,
    pub id_number: i64,
    pub scripts: HashSet<String>,
    pub events: HashSet<String>,
    /// `RegisterAllEvents`.
    pub all_events: bool,
}

impl FrameNode {
    fn new(id: usize, kind: FrameKind, name: Option<String>, parent: Option<usize>) -> Self {
        let text_like = matches!(kind, FrameKind::FontString | FrameKind::EditBox);
        FrameNode {
            id,
            kind,
            name,
            parent,
            children: Vec::new(),
            shown: true,
            width: 0.0,
            height: 0.0,
            scale: 1.0,
            anchors: Vec::new(),
            text: String::new(),
            color: if text_like {
                [1.0, 0.82, 0.0, 1.0]
            } else if kind == FrameKind::StatusBar {
                [0.0, 0.8, 0.0, 1.0]
            } else if kind == FrameKind::Texture {
                [1.0, 1.0, 1.0, 1.0]
            } else {
                [0.0, 0.0, 0.0, 0.0]
            },
            texture: None,
            texture_source: None,
            tex_coord: [0.0, 1.0, 0.0, 1.0],
            backdrop: None,
            alpha: 1.0,
            strata: None,
            level: None,
            layer: "ARTWORK".into(),
            mouse_enabled: matches!(
                kind,
                FrameKind::Button | FrameKind::CheckButton | FrameKind::Slider | FrameKind::EditBox
            ),
            mouse_wheel_enabled: false,
            keyboard_enabled: false,
            movable: false,
            clamped: false,
            drag_buttons: Vec::new(),
            font_path: None,
            font_size: 12.0,
            font_flags: String::new(),
            shadow: true,
            justify_h: if kind == FrameKind::FontString {
                "CENTER".into()
            } else {
                "LEFT".into()
            },
            justify_v: "MIDDLE".into(),
            word_wrap: true,
            value: 0.0,
            min_value: 0.0,
            max_value: 1.0,
            value_step: 0.0,
            orientation: "HORIZONTAL".into(),
            checked: false,
            enabled: true,
            focused: false,
            multi_line: false,
            max_letters: 0,
            numeric: false,
            password: false,
            builtin: false,
            solid: false,
            font_string: None,
            measured: None,
            id_number: 0,
            scripts: HashSet::new(),
            events: HashSet::new(),
            all_events: false,
        }
    }

    /// Couleur de fond effective (backdrop, ou couleur d'une frame simple).
    pub fn background(&self) -> [f32; 4] {
        match &self.backdrop {
            Some(b) => b.bg,
            None if !self.kind.is_region() && self.kind != FrameKind::StatusBar => self.color,
            None => [0.0; 4],
        }
    }

    /// Largeur estimée du texte sans moteur de polices (≈ 0,55 em par caractère).
    pub fn estimated_text_size(&self) -> (f32, f32) {
        let text = strip_codes(&self.text);
        let lines = text.split('\n').count().max(1) as f32;
        let longest = text
            .split('\n')
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(0) as f32;
        (longest * self.font_size * 0.55, lines * self.font_size * 1.25)
    }
}

/// Retire les séquences d'échappement WoW (`|cffRRGGBB`, `|r`, `|n`, `|T…|t`) d'un texte.
pub fn strip_codes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '|' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('c') => {
                for _ in 0..8 {
                    chars.next();
                }
            }
            Some('r') => {}
            Some('n') => out.push('\n'),
            Some('|') => out.push('|'),
            Some('T') | Some('A') | Some('H') => {
                // |T…|t (texture), |A…|a (atlas), |H…|h (lien) : on saute jusqu'au marqueur fermant.
                while let Some(n) = chars.next() {
                    if n == '|' {
                        chars.next();
                        break;
                    }
                }
            }
            Some('h') | Some('t') | Some('a') => {}
            Some(o) => {
                out.push('|');
                out.push(o);
            }
            None => out.push('|'),
        }
    }
    out
}

/// Morceaux colorés d'un texte avec codes WoW : `(texte, couleur RGBA ou None = couleur par défaut)`.
pub fn color_segments(s: &str) -> Vec<(String, Option<[f32; 4]>)> {
    let mut out: Vec<(String, Option<[f32; 4]>)> = Vec::new();
    let mut cur = String::new();
    let mut color: Option<[f32; 4]> = None;
    let mut chars = s.chars().peekable();
    let flush = |out: &mut Vec<(String, Option<[f32; 4]>)>, cur: &mut String, color| {
        if !cur.is_empty() {
            out.push((std::mem::take(cur), color));
        }
    };
    while let Some(c) = chars.next() {
        if c != '|' {
            cur.push(c);
            continue;
        }
        match chars.peek().copied() {
            Some('c') => {
                chars.next();
                let hex: String = (0..8).filter_map(|_| chars.next()).collect();
                flush(&mut out, &mut cur, color);
                let v = u32::from_str_radix(&hex, 16).unwrap_or(0xffff_ffff);
                color = Some([
                    ((v >> 16) & 0xff) as f32 / 255.0,
                    ((v >> 8) & 0xff) as f32 / 255.0,
                    (v & 0xff) as f32 / 255.0,
                    ((v >> 24) & 0xff) as f32 / 255.0,
                ]);
            }
            Some('r') => {
                chars.next();
                flush(&mut out, &mut cur, color);
                color = None;
            }
            Some('n') => {
                chars.next();
                cur.push('\n');
            }
            Some('|') => {
                chars.next();
                cur.push('|');
            }
            Some('T') | Some('A') | Some('H') => {
                chars.next();
                while let Some(n) = chars.next() {
                    if n == '|' {
                        chars.next();
                        break;
                    }
                }
            }
            Some('h') | Some('t') | Some('a') => {
                chars.next();
            }
            _ => cur.push('|'),
        }
    }
    flush(&mut out, &mut cur, color);
    out
}

/// Rectangle calculé d'une frame, en pixels, origine en bas à gauche comme sous WoW.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub left: f32,
    pub bottom: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.left
            && x <= self.left + self.width
            && y >= self.bottom
            && y <= self.bottom + self.height
    }
}

/// Frame en cours de déplacement (`StartMoving`) : décalage entre le curseur et son coin bas-gauche.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Moving {
    pub id: usize,
    pub grab: (f32, f32),
}

#[derive(Debug, Default)]
pub struct UiTree {
    pub frames: Vec<FrameNode>,
    by_name: HashMap<String, usize>,
    /// Frames ayant un `OnUpdate`, pour éviter de parcourir tout l'arbre à chaque image.
    pub on_update: HashSet<usize>,
    pub dirty: bool,
    /// Dernière mise en page calculée (pour l'entrée souris) et taille d'écran associée.
    pub rects: Vec<Option<Rect>>,
    pub screen: (f32, f32),
    pub moving: Option<Moving>,
}

impl UiTree {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create(
        &mut self,
        kind: FrameKind,
        name: Option<String>,
        parent: Option<usize>,
    ) -> usize {
        let id = self.frames.len();
        self.frames
            .push(FrameNode::new(id, kind, name.clone(), parent));
        if let Some(p) = parent {
            if let Some(pn) = self.frames.get_mut(p) {
                pn.children.push(id);
            }
        }
        if let Some(n) = name {
            self.by_name.insert(n, id);
        }
        self.dirty = true;
        id
    }

    pub fn get(&self, id: usize) -> Option<&FrameNode> {
        self.frames.get(id)
    }
    pub fn get_mut(&mut self, id: usize) -> Option<&mut FrameNode> {
        self.dirty = true;
        self.frames.get_mut(id)
    }
    pub fn by_name(&self, name: &str) -> Option<usize> {
        self.by_name.get(name).copied()
    }

    /// Une frame est visible si elle et tous ses ancêtres sont affichés.
    pub fn is_visible(&self, id: usize) -> bool {
        let mut cur = Some(id);
        while let Some(i) = cur {
            let f = &self.frames[i];
            if !f.shown {
                return false;
            }
            cur = f.parent;
        }
        true
    }

    /// Alpha effectif (produit des alphas des ancêtres).
    pub fn effective_alpha(&self, id: usize) -> f32 {
        let mut a = 1.0;
        let mut cur = Some(id);
        while let Some(i) = cur {
            a *= self.frames[i].alpha;
            cur = self.frames[i].parent;
        }
        a
    }

    /// Échelle effective (produit des échelles des ancêtres).
    pub fn effective_scale(&self, id: usize) -> f32 {
        let mut s = 1.0;
        let mut cur = Some(id);
        while let Some(i) = cur {
            s *= self.frames[i].scale;
            cur = self.frames[i].parent;
        }
        s
    }

    pub fn effective_strata(&self, id: usize) -> &str {
        let mut cur = Some(id);
        while let Some(i) = cur {
            if let Some(s) = &self.frames[i].strata {
                return s;
            }
            cur = self.frames[i].parent;
        }
        "MEDIUM"
    }

    pub fn effective_level(&self, id: usize) -> i32 {
        let f = &self.frames[id];
        match (f.level, f.parent) {
            (Some(l), _) => l,
            (None, Some(p)) => self.effective_level(p) + 1,
            (None, None) => 0,
        }
    }

    /// Ordre de dessin (et inverse de l'ordre de priorité pour la souris) : strate, niveau, la
    /// frame avant ses régions, couche des régions, ordre de création.
    pub fn draw_order(&self) -> Vec<usize> {
        let mut ids: Vec<usize> = (0..self.frames.len()).collect();
        let key = |id: usize| {
            let f = &self.frames[id];
            let owner = if f.kind.is_region() {
                f.parent.unwrap_or(id)
            } else {
                id
            };
            let region = if f.kind.is_region() {
                1 + layer_rank(&f.layer)
            } else {
                0
            };
            (
                strata_rank(self.effective_strata(owner)),
                self.effective_level(owner),
                region,
                id,
            )
        };
        ids.sort_by_key(|&id| key(id));
        ids
    }

    pub fn set_parent(&mut self, id: usize, parent: Option<usize>) {
        if let Some(old) = self.frames[id].parent {
            self.frames[old].children.retain(|&c| c != id);
        }
        // Pas de cycle : un parent ne peut pas être un descendant.
        let mut cur = parent;
        while let Some(p) = cur {
            if p == id {
                return;
            }
            cur = self.frames[p].parent;
        }
        self.frames[id].parent = parent;
        if let Some(p) = parent {
            self.frames[p].children.push(id);
        }
        self.dirty = true;
    }

    /// Frames enregistrées pour un événement.
    pub fn listeners(&self, event: &str) -> Vec<usize> {
        self.frames
            .iter()
            .filter(|f| f.all_events || f.events.contains(event))
            .map(|f| f.id)
            .collect()
    }

    /// Calcule les rectangles de toutes les frames pour un écran `screen_w × screen_h`, avec une
    /// estimation de la taille des textes.
    pub fn layout(&self, screen_w: f32, screen_h: f32) -> Vec<Option<Rect>> {
        self.layout_with(screen_w, screen_h, &|f| {
            f.measured.unwrap_or_else(|| f.estimated_text_size())
        })
    }

    /// Mise en page avec une fonction de mesure des textes (le moteur passe la vraie police).
    pub fn layout_with(
        &self,
        screen_w: f32,
        screen_h: f32,
        measure: &dyn Fn(&FrameNode) -> (f32, f32),
    ) -> Vec<Option<Rect>> {
        let mut rects: Vec<Option<Rect>> = vec![None; self.frames.len()];
        // Un parent est créé avant ses enfants, mais une ancre peut viser une frame plus récente :
        // on itère jusqu'à stabilité (bornée).
        for _ in 0..6 {
            let mut changed = false;
            for f in &self.frames {
                let r = self.rect_of(f, &rects, screen_w, screen_h, measure);
                if r != rects[f.id] {
                    rects[f.id] = r;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        rects
    }

    fn rect_of(
        &self,
        f: &FrameNode,
        rects: &[Option<Rect>],
        sw: f32,
        sh: f32,
        measure: &dyn Fn(&FrameNode) -> (f32, f32),
    ) -> Option<Rect> {
        let parent_rect = match f.parent {
            Some(p) => rects[p]?,
            None => Rect {
                left: 0.0,
                bottom: 0.0,
                width: sw,
                height: sh,
            },
        };
        let scale = self.effective_scale(f.id);
        let (mut w, mut h) = (f.width * scale, f.height * scale);
        if f.kind == FrameKind::FontString && (w == 0.0 || h == 0.0) {
            let (mw, mh) = measure(f);
            if w == 0.0 {
                w = mw;
            }
            if h == 0.0 {
                h = mh;
            }
        }
        if f.anchors.is_empty() {
            // Sans ancre, une frame est centrée sur son parent (approximation utile).
            return Some(Rect {
                left: parent_rect.left + (parent_rect.width - w) / 2.0,
                bottom: parent_rect.bottom + (parent_rect.height - h) / 2.0,
                width: w,
                height: h,
            });
        }
        // Contraintes : bord gauche / centre / droit, bas / milieu / haut.
        let (mut xl, mut xc, mut xr) = (None, None, None);
        let (mut yb, mut ym, mut yt) = (None, None, None);
        for a in &f.anchors {
            let rel = match a.relative_to {
                Some(r) => rects.get(r).copied().flatten().unwrap_or(parent_rect),
                None => parent_rect,
            };
            let (rx, ry) = point_offset(&a.relative_point, rel.width, rel.height);
            let tx = rel.left + rx + a.x * scale;
            let ty = rel.bottom + ry + a.y * scale;
            let p = a.point.as_str();
            if p.ends_with("LEFT") {
                xl = Some(tx);
            } else if p.ends_with("RIGHT") {
                xr = Some(tx);
            } else {
                xc = Some(tx);
            }
            if p.starts_with("TOP") {
                yt = Some(ty);
            } else if p.starts_with("BOTTOM") {
                yb = Some(ty);
            } else {
                ym = Some(ty);
            }
        }
        let (left, width) = match (xl, xr, xc) {
            (Some(l), Some(r), _) => (l.min(r), (r - l).abs()),
            (Some(l), None, _) => (l, w),
            (None, Some(r), _) => (r - w, w),
            (None, None, Some(c)) => (c - w / 2.0, w),
            _ => (parent_rect.left, w),
        };
        let (bottom, height) = match (yb, yt, ym) {
            (Some(b), Some(t), _) => (b.min(t), (t - b).abs()),
            (Some(b), None, _) => (b, h),
            (None, Some(t), _) => (t - h, h),
            (None, None, Some(m)) => (m - h / 2.0, h),
            _ => (parent_rect.bottom, h),
        };
        let mut rect = Rect {
            left,
            bottom,
            width,
            height,
        };
        if f.clamped {
            rect.left = rect.left.clamp(0.0, (sw - rect.width).max(0.0));
            rect.bottom = rect.bottom.clamp(0.0, (sh - rect.height).max(0.0));
        }
        Some(rect)
    }

    /// Frame interactive la plus haute sous le point `(x, y)` (repère WoW, origine en bas).
    pub fn hit_test(&self, x: f32, y: f32) -> Option<usize> {
        self.hit_test_min(x, y, 0)
    }

    /// Comme [`UiTree::hit_test`], en ignorant les strates de rang inférieur à `min_strata`.
    pub fn hit_test_min(&self, x: f32, y: f32, min_strata: usize) -> Option<usize> {
        let order = self.draw_order();
        order.into_iter().rev().find(|&id| {
            let f = &self.frames[id];
            !f.kind.is_region()
                && strata_rank(self.effective_strata(id)) >= min_strata
                && !f.builtin
                && f.mouse_enabled
                && f.name.as_deref() != Some("UIParent")
                && self.is_visible(id)
                && self
                    .rects
                    .get(id)
                    .copied()
                    .flatten()
                    .is_some_and(|r| r.contains(x, y))
        })
    }

    /// Fige la position courante d'une frame en une ancre `BOTTOMLEFT` relative à l'écran
    /// (utilisé par `StartMoving` et le déplacement à la souris).
    pub fn place_at(&mut self, id: usize, left: f32, bottom: f32) {
        let (sw, sh) = self.screen;
        let parent_rect = self.frames[id]
            .parent
            .and_then(|p| self.rects.get(p).copied().flatten())
            .unwrap_or(Rect {
                left: 0.0,
                bottom: 0.0,
                width: sw,
                height: sh,
            });
        let scale = self.effective_scale(id).max(0.0001);
        let cur = self.rects.get(id).copied().flatten();
        let f = &mut self.frames[id];
        if let Some(r) = cur {
            if f.width == 0.0 {
                f.width = r.width / scale;
            }
            if f.height == 0.0 {
                f.height = r.height / scale;
            }
        }
        f.anchors = vec![Anchor {
            point: "BOTTOMLEFT".into(),
            relative_to: None,
            relative_point: "BOTTOMLEFT".into(),
            x: (left - parent_rect.left) / scale,
            y: (bottom - parent_rect.bottom) / scale,
        }];
        self.dirty = true;
    }
}

/// Décalage d'un point d'ancrage par rapport au coin bas-gauche d'un rectangle `w × h`.
pub fn point_offset(point: &str, w: f32, h: f32) -> (f32, f32) {
    match point.to_ascii_uppercase().as_str() {
        "TOPLEFT" => (0.0, h),
        "TOP" => (w / 2.0, h),
        "TOPRIGHT" => (w, h),
        "LEFT" => (0.0, h / 2.0),
        "CENTER" => (w / 2.0, h / 2.0),
        "RIGHT" => (w, h / 2.0),
        "BOTTOMLEFT" => (0.0, 0.0),
        "BOTTOM" => (w / 2.0, 0.0),
        "BOTTOMRIGHT" => (w, 0.0),
        _ => (w / 2.0, h / 2.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor(point: &str, rel: &str, x: f32, y: f32) -> Anchor {
        Anchor {
            point: point.into(),
            relative_to: None,
            relative_point: rel.into(),
            x,
            y,
        }
    }

    #[test]
    fn layout_anchor() {
        let mut t = UiTree::new();
        let root = t.create(FrameKind::Frame, Some("Root".into()), None);
        {
            let f = t.get_mut(root).unwrap();
            f.width = 200.0;
            f.height = 100.0;
            f.anchors.push(anchor("TOPLEFT", "TOPLEFT", 10.0, -20.0));
        }
        let child = t.create(FrameKind::Texture, None, Some(root));
        {
            let f = t.get_mut(child).unwrap();
            f.width = 50.0;
            f.height = 10.0;
            f.anchors.push(anchor("CENTER", "CENTER", 0.0, 0.0));
        }
        let rects = t.layout(1920.0, 1080.0);
        let r = rects[root].unwrap();
        assert_eq!((r.left, r.bottom), (10.0, 1080.0 - 20.0 - 100.0));
        let c = rects[child].unwrap();
        assert_eq!((c.left, c.bottom), (10.0 + 75.0, r.bottom + 45.0));
        assert!(t.is_visible(child));
        t.get_mut(root).unwrap().shown = false;
        assert!(!t.is_visible(child));
    }

    #[test]
    fn two_anchors_scale_and_order() {
        let mut t = UiTree::new();
        let root = t.create(FrameKind::Frame, None, None);
        {
            let f = t.get_mut(root).unwrap();
            f.anchors.push(anchor("TOPLEFT", "TOPLEFT", 10.0, -10.0));
            f.anchors.push(anchor("BOTTOMRIGHT", "BOTTOMRIGHT", -10.0, 10.0));
        }
        let small = t.create(FrameKind::Frame, None, Some(root));
        {
            let f = t.get_mut(small).unwrap();
            f.width = 100.0;
            f.height = 50.0;
            f.scale = 2.0;
            f.anchors.push(anchor("BOTTOMLEFT", "BOTTOMLEFT", 5.0, 5.0));
        }
        let text = t.create(FrameKind::FontString, None, Some(root));
        t.get_mut(text).unwrap().text = "abcd".into();
        t.get_mut(text).unwrap().layer = "OVERLAY".into();
        let bg = t.create(FrameKind::Texture, None, Some(root));
        t.get_mut(bg).unwrap().layer = "BACKGROUND".into();
        let rects = t.layout(800.0, 600.0);
        assert_eq!(
            rects[root].unwrap(),
            Rect {
                left: 10.0,
                bottom: 10.0,
                width: 780.0,
                height: 580.0
            }
        );
        let s = rects[small].unwrap();
        assert_eq!((s.left, s.bottom, s.width, s.height), (20.0, 20.0, 200.0, 100.0));
        assert!(rects[text].unwrap().width > 0.0, "taille du texte estimée");
        let order = t.draw_order();
        let pos = |id| order.iter().position(|&x| x == id).unwrap();
        assert!(pos(root) < pos(bg) && pos(bg) < pos(text) && pos(text) < pos(small));
        t.get_mut(root).unwrap().strata = Some("HIGH".into());
        let order = t.draw_order();
        assert_eq!(*order.last().unwrap(), small, "les enfants héritent la strate");
    }

    #[test]
    fn colors_and_codes() {
        assert_eq!(strip_codes("|cffff0000Rouge|r et |Tx.tga:12|t ok"), "Rouge et  ok");
        let segs = color_segments("a|cff00ff00b|rc");
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[1].0, "b");
        assert_eq!(segs[1].1, Some([0.0, 1.0, 0.0, 1.0]));
    }
}
