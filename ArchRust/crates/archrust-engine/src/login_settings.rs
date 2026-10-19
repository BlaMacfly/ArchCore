//! Mise en scène de la sélection et de la création de personnage, d'après `ini/LoginSettings.txt`
//! (lu par AgcmUILoginSetting / …Camera / …Character d'Archonia.exe).
//!
//! ```text
//! [ LoginDivision ]               DivisionNumber=1817   (tuile du monde chargée pour la scène)
//! [ CharacterSelect_Position ]    Pos : X=…, Y=…, Z=…, Rotate : -90.0   (centre, gauche, droite)
//! [ CharacterSelect_MoveOffSet ]  Race=Human Class=Knight OffSet=196.f  (pas en avant à la sélection)
//! [ CharacterCreate_Position ]    Race=Human X=… Y=… Z=… Rotate=-90
//! [ CameraTM_CharacterSelect ]    Pos : X=…  /  LookAt : X=…  /  Perspective=0.735
//! [ CameraTM_CharacterCreate ]    Race=Human, puis Pos / LookAt / Perspective
//! [ CameraTM_CharacterCreateZoom ] Race=Human, Class=Knight, puis Pos / LookAt / Perspective
//! ```
//!
//! Les angles `Rotate` suivent la convention des personnages (`scene::turn_towards` : -90 = face
//! à −X, vers la caméra de sélection). `Perspective` est la demi-largeur de la fenêtre de vue
//! RenderWare (`tan(champ horizontal / 2)`).

use glam::Vec3;
use std::collections::HashMap;

/// Caméra fixe de la scène de connexion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneCamera {
    pub pos: Vec3,
    pub look_at: Vec3,
    /// Demi-largeur de la fenêtre de vue (`tan(champ horizontal / 2)`).
    pub perspective: f32,
}

impl SceneCamera {
    /// Champ vertical (radians) pour un rapport largeur / hauteur donné.
    pub fn fov_y(&self, aspect: f32) -> f32 {
        2.0 * (self.perspective / aspect.max(0.1)).atan()
    }
}

/// Emplacement d'un personnage : position monde et angle (degrés).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spot {
    pub pos: Vec3,
    pub rotate: f32,
}

/// Contenu de `LoginSettings.txt`.
#[derive(Debug, Clone, Default)]
pub struct LoginSettings {
    pub division: Option<u32>,
    /// Emplacements de la sélection : centre, gauche, droite.
    pub select_spots: Vec<Spot>,
    /// Pas en avant à la sélection, par (race, classe).
    pub move_offsets: HashMap<(String, String), f32>,
    pub create_spots: HashMap<String, Spot>,
    pub camera_select: Option<SceneCamera>,
    pub camera_create: HashMap<String, SceneCamera>,
    pub camera_create_zoom: HashMap<(String, String), SceneCamera>,
}

/// Valeurs `Clé=val` et `Clé : val` d'une ligne (virgules et suffixe `f` ignorés). `Pos :` et
/// `LookAt :` ne sont suivis d'aucun nombre : ce sont des préfixes, pas des clés.
fn values(line: &str) -> HashMap<String, String> {
    let cleaned = line.replace([',', ':'], " ");
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let is_num = |w: &str| w.trim_end_matches(['f', 'F']).parse::<f32>().is_ok();
    let mut out = HashMap::new();
    let mut i = 0;
    while i < words.len() {
        let w = words[i];
        if let Some((k, v)) = w.split_once('=') {
            if v.is_empty() {
                if let Some(next) = words.get(i + 1) {
                    out.insert(k.to_ascii_lowercase(), next.to_string());
                    i += 1;
                }
            } else {
                out.insert(k.to_ascii_lowercase(), v.to_string());
            }
        } else if words.get(i + 1).is_some_and(|n| is_num(n)) {
            out.insert(w.to_ascii_lowercase(), words[i + 1].to_string());
            i += 1;
        }
        i += 1;
    }
    out
}

fn num(map: &HashMap<String, String>, key: &str) -> Option<f32> {
    map.get(key)?.trim_end_matches(['f', 'F']).parse().ok()
}

fn vec3(map: &HashMap<String, String>) -> Option<Vec3> {
    Some(Vec3::new(num(map, "x")?, num(map, "y")?, num(map, "z")?))
}

impl LoginSettings {
    pub fn parse(text: &str) -> LoginSettings {
        let mut s = LoginSettings::default();
        let mut section = String::new();
        let (mut race, mut class) = (String::new(), String::new());
        let (mut pos, mut look_at): (Option<Vec3>, Option<Vec3>) = (None, None);
        for raw in text.lines() {
            let line = raw.split("//").next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            if line.starts_with('[') {
                section = line.trim_matches(['[', ']', ' ']).to_ascii_lowercase();
                race.clear();
                class.clear();
                pos = None;
                look_at = None;
                continue;
            }
            let v = values(line);
            let lower = line.to_ascii_lowercase();
            match section.as_str() {
                "logindivision" => s.division = num(&v, "divisionnumber").map(|n| n as u32),
                "characterselect_position" => {
                    if let (Some(p), Some(r)) = (vec3(&v), num(&v, "rotate")) {
                        s.select_spots.push(Spot { pos: p, rotate: r });
                    }
                }
                "characterselect_moveoffset" => {
                    if let (Some(r), Some(c), Some(o)) =
                        (v.get("race"), v.get("class"), num(&v, "offset"))
                    {
                        s.move_offsets
                            .insert((r.to_ascii_lowercase(), c.to_ascii_lowercase()), o);
                    }
                }
                "charactercreate_position" => {
                    if let (Some(r), Some(p), Some(rot)) =
                        (v.get("race"), vec3(&v), num(&v, "rotate"))
                    {
                        s.create_spots.insert(
                            r.to_ascii_lowercase(),
                            Spot {
                                pos: p,
                                rotate: rot,
                            },
                        );
                    }
                }
                sec if sec.starts_with("cameratm_") => {
                    if let Some(r) = v.get("race").filter(|_| !lower.starts_with("pos")) {
                        race = r.to_ascii_lowercase();
                    }
                    if let Some(c) = v.get("class").filter(|_| !lower.starts_with("pos")) {
                        class = c.to_ascii_lowercase();
                    }
                    if lower.starts_with("pos") {
                        pos = vec3(&v);
                    } else if lower.starts_with("lookat") {
                        look_at = vec3(&v);
                    } else if let (Some(p), Some(l), Some(per)) =
                        (pos, look_at, num(&v, "perspective"))
                    {
                        let cam = SceneCamera {
                            pos: p,
                            look_at: l,
                            perspective: per,
                        };
                        match sec {
                            "cameratm_characterselect" => s.camera_select = Some(cam),
                            "cameratm_charactercreate" => {
                                s.camera_create.insert(race.clone(), cam);
                            }
                            "cameratm_charactercreatezoom" => {
                                s.camera_create_zoom
                                    .insert((race.clone(), class.clone()), cam);
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        s
    }
}

/// Charge `ini/LoginSettings.txt` du client (texte CP949 en clair).
pub fn load(assets: &crate::assets::Assets) -> Option<LoginSettings> {
    let bytes = assets.read_file("ini/LoginSettings.txt").ok()?;
    let text =
        archrust_data::ini::decode_text(&bytes, archrust_data::ini::CodePage::for_dir("ini"));
    let s = LoginSettings::parse(&text);
    (s.camera_select.is_some() && !s.select_spots.is_empty()).then_some(s)
}

/// Race et nom de classe d'un modèle (`CharacterTemplatePublic.ini` : colonnes `Name`, `Race`).
pub fn template_identity(assets: &crate::assets::Assets, tid: i32) -> (&'static str, String) {
    let Ok(table) = assets.table("ini", "CharacterTemplatePublic.ini") else {
        return ("Human", String::new());
    };
    let column = |name: &str| {
        table.section("").and_then(|h| {
            h.entries
                .iter()
                .find(|(_, v)| v.eq_ignore_ascii_case(name))
                .map(|(k, _)| k.clone())
        })
    };
    let sec = table.section(&tid.to_string());
    let get = |name: &str| {
        let key = column(name)?;
        sec.and_then(|s| s.get(&key)).map(str::to_string)
    };
    let class_name = get("Name").unwrap_or_default();
    let race = race_of(get("Race").and_then(|r| r.trim().parse().ok()), &class_name);
    (race, class_name)
}

/// Races jouables, dans l'ordre des boutons de l'écran de création d'Archonia.
pub const RACES: [&str; 4] = ["Human", "Orc", "MoonElf", "DragonScion"];

/// Race d'un modèle : facteur `Race` de `CharacterTemplatePublic.ini` (1 Human, 2 Orc,
/// 3 MoonElf, 4 DragonScion), sinon d'après le nom de classe (le TID 9 Elementalist n'a pas de
/// facteurs de type), comme les boutons de classe de l'écran de création.
pub fn race_of(race_factor: Option<i64>, class_name: &str) -> &'static str {
    match race_factor {
        Some(1) => return "Human",
        Some(2) => return "Orc",
        Some(3) => return "MoonElf",
        Some(4) => return "DragonScion",
        _ => {}
    }
    match class_name.to_ascii_lowercase().as_str() {
        "knight" | "archer" | "mage" => "Human",
        "berserker" | "hunter" | "sorcerer" => "Orc",
        "swashbuckler" | "ranger" | "elementalist" => "MoonElf",
        _ => "DragonScion",
    }
}

/// Classe générique de `LoginSettings` (Knight, Ranger, Mage, Scion) d'un nom de classe.
pub fn class_kind(class_name: &str) -> &'static str {
    match class_name.to_ascii_lowercase().as_str() {
        "knight" | "berserker" | "swashbuckler" => "Knight",
        "archer" | "hunter" | "ranger" => "Ranger",
        "mage" | "sorcerer" | "elementalist" => "Mage",
        _ => "Scion",
    }
}

/// Nom français d'une race, pour l'interface.
pub fn race_label(race: &str) -> &'static str {
    match race {
        "Human" => "Humain",
        "Orc" => "Orc",
        "MoonElf" => "Elfe de lune",
        _ => "Dragon Scion",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "// commentaire
[ LoginDivision ]
DivisionNumber=1817

[ CharacterSelect_Position ]
Pos : X=-677800.000000, Y=10765.30000, Z=-768200.000000, Rotate : -90.0
Pos : X=-677800.000000, Y=10765.30000, Z=-768374.000000, Rotate : -70.5

[ CharacterSelect_MoveOffSet ]
Race=Human Class=Knight OffSet=196.f

[ CharacterCreate_Position ]
Race=Orc X=-678999.000000 Y=10425.80000 Z=-730107.000000 Rotate=-90.000000

[ CameraTM_CharacterSelect ]
Pos : X=-678288.812500, Y=10618.820313, Z=-768201.062500
LookAt : X=-668444.437500, Y=12375.190430, Z=-768197.500000
Perspective=0.735

[ CameraTM_CharacterCreateZoom ]
Race=Human
Class=Knight
Pos : X=1, Y=2, Z=3
LookAt : X=4, Y=5, Z=6
Perspective=0.31
";

    #[test]
    fn parses_login_settings() {
        let s = LoginSettings::parse(SAMPLE);
        assert_eq!(s.division, Some(1817));
        assert_eq!(s.select_spots.len(), 2);
        assert_eq!(s.select_spots[1].rotate, -70.5);
        assert_eq!(
            s.select_spots[0].pos,
            Vec3::new(-677800.0, 10765.3, -768200.0)
        );
        assert_eq!(s.move_offsets[&("human".into(), "knight".into())], 196.0);
        assert_eq!(s.create_spots["orc"].pos.z, -730107.0);
        let cam = s.camera_select.unwrap();
        assert_eq!(cam.perspective, 0.735);
        assert_eq!(cam.look_at.x, -668444.44);
        let zoom = s.camera_create_zoom[&("human".into(), "knight".into())];
        assert_eq!(zoom.pos, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(race_of(None, "Elementalist"), "MoonElf");
        assert_eq!(class_kind("Hunter"), "Ranger");
    }
}
