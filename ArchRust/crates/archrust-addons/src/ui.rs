//! Arbre de frames décrit par les AddOns et dessiné par le moteur.
//!
//! Le modèle suit celui de WoW : chaque frame a un type, un parent, des ancres (`SetPoint`), une
//! taille, une visibilité, et des scripts (`OnEvent`, `OnUpdate`, `OnClick`…) conservés côté Lua.

use std::collections::{HashMap, HashSet};

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
}

impl FrameKind {
    pub fn parse(s: &str) -> FrameKind {
        match s.to_ascii_lowercase().as_str() {
            "button" => FrameKind::Button,
            "fontstring" => FrameKind::FontString,
            "texture" => FrameKind::Texture,
            "editbox" => FrameKind::EditBox,
            "statusbar" => FrameKind::StatusBar,
            "checkbutton" => FrameKind::CheckButton,
            "slider" => FrameKind::Slider,
            _ => FrameKind::Frame,
        }
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
    pub anchors: Vec<Anchor>,
    pub text: String,
    /// Couleur de fond ou de texte, RGBA 0..1.
    pub color: [f32; 4],
    pub texture: Option<String>,
    pub alpha: f32,
    pub strata: String,
    pub level: i32,
    pub mouse_enabled: bool,
    pub movable: bool,
    pub font_size: f32,
    pub justify_h: String,
    /// Valeur courante / min / max pour StatusBar et Slider.
    pub value: f32,
    pub min_value: f32,
    pub max_value: f32,
    pub checked: bool,
    pub scripts: HashSet<String>,
    pub events: HashSet<String>,
}

impl FrameNode {
    fn new(id: usize, kind: FrameKind, name: Option<String>, parent: Option<usize>) -> Self {
        let text_like = matches!(kind, FrameKind::FontString);
        FrameNode {
            id,
            kind,
            name,
            parent,
            children: Vec::new(),
            shown: true,
            width: 0.0,
            height: 0.0,
            anchors: Vec::new(),
            text: String::new(),
            color: if text_like { [1.0, 0.82, 0.0, 1.0] } else { [0.0, 0.0, 0.0, 0.0] },
            texture: None,
            alpha: 1.0,
            strata: "MEDIUM".into(),
            level: 0,
            mouse_enabled: false,
            movable: false,
            font_size: 12.0,
            justify_h: "LEFT".into(),
            value: 0.0,
            min_value: 0.0,
            max_value: 1.0,
            checked: false,
            scripts: HashSet::new(),
            events: HashSet::new(),
        }
    }
}

/// Rectangle calculé d'une frame, en pixels, origine en bas à gauche comme sous WoW.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub left: f32,
    pub bottom: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Default)]
pub struct UiTree {
    pub frames: Vec<FrameNode>,
    by_name: HashMap<String, usize>,
    /// Frames ayant un `OnUpdate`, pour éviter de parcourir tout l'arbre à chaque image.
    pub on_update: HashSet<usize>,
    pub dirty: bool,
}

impl UiTree {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create(&mut self, kind: FrameKind, name: Option<String>, parent: Option<usize>) -> usize {
        let id = self.frames.len();
        self.frames.push(FrameNode::new(id, kind, name.clone(), parent));
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

    pub fn set_parent(&mut self, id: usize, parent: Option<usize>) {
        if let Some(old) = self.frames[id].parent {
            self.frames[old].children.retain(|&c| c != id);
        }
        self.frames[id].parent = parent;
        if let Some(p) = parent {
            self.frames[p].children.push(id);
        }
        self.dirty = true;
    }

    /// Frames enregistrées pour un événement.
    pub fn listeners(&self, event: &str) -> Vec<usize> {
        self.frames.iter().filter(|f| f.events.contains(event)).map(|f| f.id).collect()
    }

    /// Calcule les rectangles de toutes les frames pour un écran `screen_w × screen_h`.
    pub fn layout(&self, screen_w: f32, screen_h: f32) -> Vec<Option<Rect>> {
        let mut rects: Vec<Option<Rect>> = vec![None; self.frames.len()];
        // Les identifiants croissent avec la création ; un parent est créé avant ses enfants, mais
        // une ancre peut viser une frame plus récente : on itère jusqu'à stabilité (bornée).
        for _ in 0..4 {
            let mut changed = false;
            for f in &self.frames {
                let r = self.rect_of(f, &rects, screen_w, screen_h);
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

    fn rect_of(&self, f: &FrameNode, rects: &[Option<Rect>], sw: f32, sh: f32) -> Option<Rect> {
        let parent_rect = match f.parent {
            Some(p) => rects[p]?,
            None => Rect { left: 0.0, bottom: 0.0, width: sw, height: sh },
        };
        let (w, h) = (f.width, f.height);
        if f.anchors.is_empty() {
            // Sans ancre, une frame racine est centrée comme UIParent (approximation utile).
            return Some(Rect { left: parent_rect.left + (parent_rect.width - w) / 2.0, bottom: parent_rect.bottom + (parent_rect.height - h) / 2.0, width: w, height: h });
        }
        let a = &f.anchors[0];
        let rel = match a.relative_to {
            Some(r) => rects.get(r).copied().flatten().unwrap_or(parent_rect),
            None => parent_rect,
        };
        let (rx, ry) = point_offset(&a.relative_point, rel.width, rel.height);
        let (px, py) = point_offset(&a.point, w, h);
        let target_x = rel.left + rx + a.x;
        let target_y = rel.bottom + ry + a.y;
        let mut rect = Rect { left: target_x - px, bottom: target_y - py, width: w, height: h };
        // Deux ancres opposées définissent la taille (ex. TOPLEFT + BOTTOMRIGHT).
        if let Some(b) = f.anchors.get(1) {
            let relb = match b.relative_to {
                Some(r) => rects.get(r).copied().flatten().unwrap_or(parent_rect),
                None => parent_rect,
            };
            let (bx, by) = point_offset(&b.relative_point, relb.width, relb.height);
            let tx = relb.left + bx + b.x;
            let ty = relb.bottom + by + b.y;
            if w == 0.0 {
                rect.left = target_x.min(tx);
                rect.width = (tx - target_x).abs();
            }
            if h == 0.0 {
                rect.bottom = target_y.min(ty);
                rect.height = (ty - target_y).abs();
            }
        }
        Some(rect)
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

    #[test]
    fn layout_anchor() {
        let mut t = UiTree::new();
        let root = t.create(FrameKind::Frame, Some("Root".into()), None);
        {
            let f = t.get_mut(root).unwrap();
            f.width = 200.0;
            f.height = 100.0;
            f.anchors.push(Anchor { point: "TOPLEFT".into(), relative_to: None, relative_point: "TOPLEFT".into(), x: 10.0, y: -20.0 });
        }
        let child = t.create(FrameKind::FontString, None, Some(root));
        {
            let f = t.get_mut(child).unwrap();
            f.width = 50.0;
            f.height = 10.0;
            f.anchors.push(Anchor { point: "CENTER".into(), relative_to: None, relative_point: "CENTER".into(), x: 0.0, y: 0.0 });
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
}
