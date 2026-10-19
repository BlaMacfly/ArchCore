//! Lecture audio : musique (MP3 en boucle, fondu enchaîné), ambiance et effets ponctuels (WAV).
//!
//! Tous les octets passent par [`Assets::read_file`] (patchs `[[files]]`, packs, disque), puis par
//! les packs de sons `.pk` indexés par `soundpak.info` ([`archrust_data::soundpak`]) : un patch
//! peut donc remplacer n'importe quel son, musique ou pack du client.
//!
//! Choix de `rodio` plutôt que `kira` : la sortie est un simple mélangeur (une musique, une
//! ambiance, quelques effets) sans espace sonore ni horloge musicale ; `rodio` décode MP3 et WAV par
//! Symphonia à partir d'un tampon mémoire (ce qui convient aux fichiers servis par les packs et les
//! patchs), s'appuie sur `cpal` déjà présent dans l'écosystème winit/wgpu, et la gestion des volumes
//! par piste suffit à réaliser le fondu enchaîné depuis la boucle de rendu. `kira` apporte des
//! courbes d'enveloppe et un graphe de pistes qui n'ont pas d'usage ici pour l'instant.
//!
//! Sans périphérique de sortie (ou sans client et sans patch fournissant les sons), tout est
//! silencieux et aucun appel n'échoue : les sons manquants sont journalisés une fois chacun.

use crate::assets::Assets;
use anyhow::{anyhow, Context, Result};
use archrust_data::soundpak::{scan_riff, SoundEntry, SoundIndex};
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Musique des écrans de connexion, de sélection et de création.
pub const DEFAULT_LOGIN_MUSIC: &str = "sound/bgm/htt_001.mp3";
/// Musique en jeu quand la région du joueur n'en indique pas (voir [`crate::zones`]).
pub const DEFAULT_WORLD_MUSIC: &str = "sound/bgm/hft_001.mp3";
/// Ambiance en jeu par défaut (cité, jour : le joueur débute à Anchorville).
pub const DEFAULT_WORLD_AMBIENCE: &str = "sound/stream/a_cid_d.mp3";
/// Dossier des packs d'effets quand le chemin ne précise rien (`sd0000.wav`).
const EFFECT_DIR: &str = "sound/effect";
/// Durée du fondu enchaîné entre deux musiques, en secondes.
const CROSSFADE_SECS: f32 = 1.5;

/// Volumes de l'utilisateur, de 0 à 1 (panneau Options, `saved-variables/options.toml`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSettings {
    pub music: f32,
    pub ambience: f32,
    pub effects: f32,
    /// Coupe tout sans toucher aux volumes.
    pub muted: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            music: 0.6,
            ambience: 0.5,
            effects: 0.8,
            muted: false,
        }
    }
}

impl AudioSettings {
    fn clamped(&self) -> Self {
        Self {
            music: self.music.clamp(0.0, 1.0),
            ambience: self.ambience.clamp(0.0, 1.0),
            effects: self.effects.clamp(0.0, 1.0),
            muted: self.muted,
        }
    }

    /// Amplitude d'un curseur : courbe quadratique, plus naturelle à l'oreille qu'une droite.
    fn gain(v: f32, muted: bool) -> f32 {
        if muted {
            0.0
        } else {
            v.clamp(0.0, 1.0).powi(2)
        }
    }
}

/// D'où vient un son résolu par la bibliothèque.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SoundSource {
    /// Fichier d'un AddOn (ou chemin absolu).
    AddonFile(PathBuf),
    /// Fichier servi par les patchs ou le client (pack `sound/ui`, MP3 de `sound/bgm`…).
    File(String),
    /// Entrée d'un pack `.pk` trouvée dans `soundpak.info`.
    Indexed { pack: String, entry: SoundEntry },
    /// Entrée d'un pack `.pk` absent de l'index, découpé par balayage RIFF.
    Scanned { pack: String, entry: SoundEntry },
}

impl std::fmt::Display for SoundSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SoundSource::AddonFile(p) => write!(f, "fichier {}", p.display()),
            SoundSource::File(p) => write!(f, "fichier client ou patch {p}"),
            SoundSource::Indexed { pack, entry } => write!(
                f,
                "pack {pack} (index : entrée {} « {} », décalage {}, {} octets, id {})",
                entry.index, entry.name, entry.offset, entry.size, entry.id
            ),
            SoundSource::Scanned { pack, entry } => write!(
                f,
                "pack {pack} (balayage RIFF : entrée {}, décalage {}, {} octets)",
                entry.index, entry.offset, entry.size
            ),
        }
    }
}

/// Un son résolu : sa provenance et ses octets (WAV ou MP3).
#[derive(Debug, Clone)]
pub struct LocatedSound {
    pub source: SoundSource,
    pub bytes: Vec<u8>,
}

/// Résolution des chemins de sons vers des octets. Partagée par le client et par
/// `archrust inspect sound` / `archrust extract sound`.
pub struct SoundLibrary {
    assets: Arc<Assets>,
    addons_dir: Option<PathBuf>,
    /// `sound/soundpak.info` (`None` = absent ou illisible), lu à la première demande.
    index: Mutex<Option<Option<Arc<SoundIndex>>>>,
    /// Octets des packs `.pk` déjà lus, par chemin client normalisé.
    packs: Mutex<HashMap<String, Arc<[u8]>>>,
    /// Découpages RIFF des packs hors index.
    scanned: Mutex<HashMap<String, Arc<Vec<SoundEntry>>>>,
}

/// Chemin client normalisé : séparateurs `/`, sans espaces autour, sans `./` initial.
pub fn normalize_sound_path(path: &str) -> String {
    let p = path.trim().replace('\\', "/");
    p.trim_start_matches("./").to_string()
}

/// Nom de fichier et dossier (`sound/effect`) d'un chemin normalisé.
fn split(path: &str) -> (&str, &str) {
    match path.rsplit_once('/') {
        Some((dir, file)) => (dir, file),
        None => ("", path),
    }
}

impl SoundLibrary {
    pub fn new(assets: Arc<Assets>, addons_dir: Option<PathBuf>) -> Self {
        Self {
            assets,
            addons_dir,
            index: Mutex::new(None),
            packs: Mutex::new(HashMap::new()),
            scanned: Mutex::new(HashMap::new()),
        }
    }

    pub fn assets(&self) -> &Assets {
        &self.assets
    }

    /// Index `sound/soundpak.info` (via les patchs puis le client).
    pub fn index(&self) -> Option<Arc<SoundIndex>> {
        let mut guard = self.index.lock().unwrap();
        if let Some(cached) = guard.as_ref() {
            return cached.clone();
        }
        let loaded = match self.assets.read_file("sound/soundpak.info") {
            Ok(bytes) => match SoundIndex::parse(&bytes) {
                Ok(idx) => {
                    log::info!(
                        "soundpak.info : {} packs, {} sons",
                        idx.packs().count(),
                        idx.entry_count()
                    );
                    Some(Arc::new(idx))
                }
                Err(e) => {
                    log::warn!("soundpak.info illisible : {e}");
                    None
                }
            },
            Err(e) => {
                log::debug!("soundpak.info absent : {e}");
                None
            }
        };
        *guard = Some(loaded.clone());
        loaded
    }

    /// Octets d'un pack `.pk` (chemin client), mis en cache.
    pub fn pack_bytes(&self, rel: &str) -> Result<Arc<[u8]>> {
        let key = rel.to_ascii_lowercase();
        if let Some(b) = self.packs.lock().unwrap().get(&key) {
            return Ok(b.clone());
        }
        let bytes: Arc<[u8]> = self.assets.read_file(rel)?.into();
        self.packs.lock().unwrap().insert(key, bytes.clone());
        Ok(bytes)
    }

    /// Entrées d'un pack hors index, par balayage RIFF (cache).
    pub fn scanned_entries(&self, rel: &str, bytes: &[u8]) -> Arc<Vec<SoundEntry>> {
        let key = rel.to_ascii_lowercase();
        if let Some(e) = self.scanned.lock().unwrap().get(&key) {
            return e.clone();
        }
        let (_, file) = split(&key);
        let stem = file.strip_suffix(".pk").unwrap_or(file);
        let entries = Arc::new(scan_riff(bytes, stem));
        self.scanned
            .lock()
            .unwrap()
            .insert(key, entries.clone());
        entries
    }

    /// Fichier d'AddOn correspondant à `path`, s'il existe : chemin absolu, chemin relatif
    /// existant, ou `Interface\AddOns\<AddOn>\…` façon WoW (résolu dans le dossier des AddOns).
    fn addon_file(&self, path: &str) -> Option<PathBuf> {
        if path.contains("..") {
            return None;
        }
        let p = Path::new(path);
        if p.is_absolute() {
            return p.is_file().then(|| p.to_path_buf());
        }
        let lower = path.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("interface/addons/") {
            let dir = self.addons_dir.as_ref()?;
            let candidate = dir.join(&path[path.len() - rest.len()..]);
            return candidate.is_file().then_some(candidate);
        }
        if let Some(dir) = &self.addons_dir {
            let candidate = dir.join(path);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        p.is_file().then(|| p.to_path_buf())
    }

    /// Résout un son : fichier d'AddOn, puis patchs/client (`sound/ui/x.wav`, `sound/bgm/x.mp3`,
    /// fichier en clair), puis entrée d'un pack `.pk` (`sound/effect/sd0000.wav`, par l'index ou
    /// par balayage ; sans dossier, `sound/effect` est supposé).
    pub fn read(&self, path: &str) -> Result<LocatedSound> {
        let norm = normalize_sound_path(path);
        if norm.is_empty() {
            return Err(anyhow!("chemin de son vide"));
        }
        if let Some(file) = self.addon_file(&norm) {
            let bytes = std::fs::read(&file).with_context(|| file.display().to_string())?;
            return Ok(LocatedSound {
                source: SoundSource::AddonFile(file),
                bytes,
            });
        }
        let (dir, file) = split(&norm);
        let has_dir = !dir.is_empty();
        // Sans dossier : pack d'effets ; dossier relatif à `sound/` (`effect/mono/maa0003.wav`) :
        // complété ; sinon le chemin client est pris tel quel.
        let client_path = if !has_dir {
            format!("{EFFECT_DIR}/{file}")
        } else if !dir.to_ascii_lowercase().starts_with("sound") {
            format!("sound/{norm}")
        } else {
            norm.clone()
        };
        let (dir, _) = split(&client_path);
        match self.assets.read_file(&client_path) {
            Ok(bytes) => {
                return Ok(LocatedSound {
                    source: SoundSource::File(client_path),
                    bytes,
                })
            }
            Err(e) => log::trace!("{client_path} : {e} ; essai des packs .pk"),
        }
        if !file.to_ascii_lowercase().ends_with(".wav") {
            return Err(anyhow!("{norm} introuvable (patchs, client)"));
        }
        let pack_dir = dir;
        // 1. L'index `soundpak.info` connaît l'entrée : son pack est lu dans le même dossier.
        if let Some((pack, entry)) = self.index().as_ref().and_then(|i| {
            i.find(file)
                .map(|(p, e)| (format!("{pack_dir}/{}", p.file.to_ascii_lowercase()), e.clone()))
        }) {
            let bytes = self.pack_bytes(&pack)?;
            let data = entry.slice(&bytes).with_context(|| pack.clone())?.to_vec();
            return Ok(LocatedSound {
                source: SoundSource::Indexed { pack, entry },
                bytes: data,
            });
        }
        // 2. Repli : `<préfixe><numéro>.wav` → pack `<préfixe>.pk` découpé par balayage RIFF.
        let stem = file
            .strip_suffix(".wav")
            .or_else(|| file.strip_suffix(".WAV"))
            .unwrap_or(file);
        let digits = stem
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .count();
        if digits == 0 || digits == stem.len() {
            return Err(anyhow!(
                "{norm} : absent de soundpak.info et sans numéro de pack"
            ));
        }
        let (prefix, number) = stem.split_at(stem.len() - digits);
        let n: usize = number.parse().unwrap_or(usize::MAX);
        let pack = format!("{pack_dir}/{}.pk", prefix.to_ascii_lowercase());
        let bytes = self
            .pack_bytes(&pack)
            .with_context(|| format!("{norm} : ni dans soundpak.info ni pack {pack}"))?;
        let entries = self.scanned_entries(&pack, &bytes);
        let entry = entries
            .get(n)
            .cloned()
            .ok_or_else(|| anyhow!("{norm} : le pack {pack} n'a que {} sons", entries.len()))?;
        let data = entry.slice(&bytes).with_context(|| pack.clone())?.to_vec();
        Ok(LocatedSound {
            source: SoundSource::Scanned { pack, entry },
            bytes: data,
        })
    }
}

/// Une couche d'une piste : un lecteur et son gain en cours de fondu.
struct Layer {
    path: String,
    player: Player,
    gain: f32,
    target: f32,
}

/// Piste à une seule musique « voulue », avec fondu enchaîné entre l'ancienne et la nouvelle.
struct Track {
    label: &'static str,
    layers: Vec<Layer>,
    /// Chemin demandé (même s'il n'a pas pu être lu, pour ne pas réessayer à chaque image).
    wanted: Option<String>,
}

impl Track {
    fn new(label: &'static str) -> Self {
        Self {
            label,
            layers: Vec::new(),
            wanted: None,
        }
    }

    fn fade_out_all(&mut self) {
        for l in &mut self.layers {
            l.target = 0.0;
        }
    }

    fn apply_volume(&self, volume: f32) {
        for l in &self.layers {
            l.player.set_volume(l.gain * volume);
        }
    }

    fn update(&mut self, dt: f32, volume: f32) {
        let step = if CROSSFADE_SECS > 0.0 {
            dt / CROSSFADE_SECS
        } else {
            1.0
        };
        for l in &mut self.layers {
            if l.gain < l.target {
                l.gain = (l.gain + step).min(l.target);
            } else if l.gain > l.target {
                l.gain = (l.gain - step).max(l.target);
            }
            l.player.set_volume(l.gain * volume);
        }
        self.layers.retain(|l| {
            let done = l.target <= 0.0 && l.gain <= 0.0;
            if done {
                l.player.stop();
            }
            !done
        });
    }

    fn current(&self) -> Option<&str> {
        self.layers
            .iter()
            .filter(|l| l.target > 0.0)
            .map(|l| l.path.as_str())
            .next()
    }
}

/// Moteur audio du client.
pub struct Audio {
    device: Option<MixerDeviceSink>,
    library: SoundLibrary,
    settings: AudioSettings,
    music: Track,
    ambience: Track,
    /// Chemins déjà signalés comme introuvables (un avertissement chacun).
    missing: HashSet<String>,
}

impl Audio {
    /// Ouvre le périphérique de sortie par défaut ; sans périphérique, le moteur reste muet.
    pub fn new(assets: Arc<Assets>, addons_dir: Option<PathBuf>, settings: AudioSettings) -> Self {
        let device = match DeviceSinkBuilder::open_default_sink() {
            Ok(mut d) => {
                d.log_on_drop(false);
                log::info!(
                    "audio : sortie {} Hz, {} canaux (rodio/cpal)",
                    d.config().sample_rate(),
                    d.config().channel_count()
                );
                Some(d)
            }
            Err(e) => {
                log::warn!("audio : aucun périphérique de sortie ({e}) : client muet");
                None
            }
        };
        Self {
            device,
            library: SoundLibrary::new(assets, addons_dir),
            settings: settings.clamped(),
            music: Track::new("musique"),
            ambience: Track::new("ambiance"),
            missing: HashSet::new(),
        }
    }

    /// Un périphérique de sortie est ouvert.
    pub fn is_active(&self) -> bool {
        self.device.is_some()
    }

    pub fn library(&self) -> &SoundLibrary {
        &self.library
    }

    pub fn settings(&self) -> &AudioSettings {
        &self.settings
    }

    /// Applique de nouveaux volumes (immédiatement aux pistes en cours).
    pub fn set_settings(&mut self, settings: &AudioSettings) {
        self.settings = settings.clamped();
        self.music
            .apply_volume(AudioSettings::gain(self.settings.music, self.settings.muted));
        self.ambience
            .apply_volume(AudioSettings::gain(self.settings.ambience, self.settings.muted));
    }

    /// Musique en cours (chemin), s'il y en a une.
    pub fn current_music(&self) -> Option<&str> {
        self.music.current()
    }

    pub fn current_ambience(&self) -> Option<&str> {
        self.ambience.current()
    }

    fn report_missing(&mut self, path: &str, err: &anyhow::Error) {
        if self.missing.insert(path.to_string()) {
            if self.library.assets().has_client() {
                log::warn!("son {path} : {err:#}");
            } else {
                log::info!("son {path} indisponible sans client (patch absent)");
            }
        }
    }

    /// Lecture en boucle sur une piste, avec fondu enchaîné.
    fn set_track(&mut self, which: fn(&mut Audio) -> &mut Track, path: Option<&str>) {
        let path = path
            .map(normalize_sound_path)
            .filter(|p| !p.is_empty());
        if which(self).wanted == path {
            return;
        }
        which(self).wanted = path.clone();
        which(self).fade_out_all();
        let Some(path) = path else {
            return;
        };
        let Some(device) = &self.device else {
            return;
        };
        let located = match self.library.read(&path) {
            Ok(l) => l,
            Err(e) => {
                self.report_missing(&path, &e);
                return;
            }
        };
        let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        let len = located.bytes.len() as u64;
        let source = Decoder::builder()
            .with_data(Cursor::new(located.bytes))
            .with_hint(&ext)
            .with_byte_len(len)
            .with_seekable(true)
            .build_looped();
        let source = match source {
            Ok(s) => s,
            Err(e) => {
                let err = anyhow!("décodage {ext} : {e}");
                self.report_missing(&path, &err);
                return;
            }
        };
        let player = Player::connect_new(device.mixer());
        player.set_volume(0.0);
        player.append(source);
        let track = which(self);
        log::info!("{} : {path} ({})", track.label, located.source);
        track.layers.push(Layer {
            path,
            player,
            gain: 0.0,
            target: 1.0,
        });
    }

    /// Musique de fond (`None` = silence en fondu). Idempotent : rappeler avec le même chemin ne
    /// relance rien.
    pub fn play_music(&mut self, path: Option<&str>) {
        self.set_track(|a| &mut a.music, path);
    }

    /// Ambiance de zone (`sound/stream/*.mp3`), même fonctionnement.
    pub fn play_ambience(&mut self, path: Option<&str>) {
        self.set_track(|a| &mut a.ambience, path);
    }

    /// Effet ponctuel : `sound/ui/x.wav`, `sound/effect/sd0000.wav`, fichier d'AddOn… Rend `false`
    /// si le son n'a pas été joué (introuvable, muet, pas de périphérique).
    pub fn play_effect(&mut self, path: &str) -> bool {
        let norm = normalize_sound_path(path);
        let Some(device) = &self.device else {
            return false;
        };
        let gain = AudioSettings::gain(self.settings.effects, self.settings.muted);
        if gain <= 0.0 {
            return false;
        }
        let located = match self.library.read(&norm) {
            Ok(l) => l,
            Err(e) => {
                self.report_missing(&norm, &e);
                return false;
            }
        };
        let ext = norm.rsplit('.').next().unwrap_or("wav").to_ascii_lowercase();
        let len = located.bytes.len() as u64;
        match Decoder::builder()
            .with_data(Cursor::new(located.bytes))
            .with_hint(&ext)
            .with_byte_len(len)
            .build()
        {
            Ok(source) => {
                log::debug!("effet : {norm} ({})", located.source);
                device.mixer().add(source.amplify(gain));
                true
            }
            Err(e) => {
                let err = anyhow!("décodage {ext} : {e}");
                self.report_missing(&norm, &err);
                false
            }
        }
    }

    /// À appeler chaque image : fondus et nettoyage des pistes terminées.
    pub fn update(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.5);
        let m = &self.settings;
        self.music
            .update(dt, AudioSettings::gain(m.music, m.muted));
        self.ambience
            .update(dt, AudioSettings::gain(m.ambience, m.muted));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_paths() {
        assert_eq!(
            normalize_sound_path(" .\\sound\\ui\\Click.wav "),
            "sound/ui/Click.wav"
        );
        assert_eq!(split("sound/effect/sd0000.wav"), ("sound/effect", "sd0000.wav"));
        assert_eq!(split("sd0000.wav"), ("", "sd0000.wav"));
    }

    #[test]
    fn settings_gain_is_quadratic_and_muted_is_silent() {
        assert_eq!(AudioSettings::gain(1.0, false), 1.0);
        assert!((AudioSettings::gain(0.5, false) - 0.25).abs() < 1e-6);
        assert_eq!(AudioSettings::gain(0.5, true), 0.0);
        let s = AudioSettings {
            music: 2.0,
            ambience: -1.0,
            effects: 0.3,
            muted: false,
        }
        .clamped();
        assert_eq!((s.music, s.ambience, s.effects), (1.0, 0.0, 0.3));
    }

    #[test]
    fn library_without_client_reports_missing_sounds_as_errors() {
        let lib = SoundLibrary::new(Arc::new(Assets::new(None, &[])), None);
        assert!(lib.index().is_none());
        assert!(lib.read("sound/ui/click.wav").is_err());
        assert!(lib.read("sd0000.wav").is_err());
        assert!(lib.read("").is_err());
    }

    #[test]
    fn library_reads_scanned_pack_from_a_patch() {
        // Un patch `[[files]]` fournit `sound/effect/mono/zz.pk` : deux WAV bout à bout, hors index.
        let tmp = std::env::temp_dir().join(format!("archrust-audio-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let sound = tmp.join("files/sound/effect/mono");
        std::fs::create_dir_all(&sound).unwrap();
        let wav = |n: u8| {
            let data = vec![n; 8];
            let mut v = b"RIFF".to_vec();
            v.extend_from_slice(&((36 + data.len()) as u32).to_le_bytes());
            v.extend_from_slice(b"WAVEfmt ");
            v.extend_from_slice(&16u32.to_le_bytes());
            for x in [1u16, 1] {
                v.extend_from_slice(&x.to_le_bytes());
            }
            v.extend_from_slice(&22_050u32.to_le_bytes());
            v.extend_from_slice(&44_100u32.to_le_bytes());
            for x in [2u16, 16] {
                v.extend_from_slice(&x.to_le_bytes());
            }
            v.extend_from_slice(b"data");
            v.extend_from_slice(&(data.len() as u32).to_le_bytes());
            v.extend_from_slice(&data);
            v
        };
        let mut pack = wav(1);
        pack.extend_from_slice(&wav(2));
        std::fs::write(sound.join("zz.pk"), &pack).unwrap();
        std::fs::write(tmp.join("files/sound/effect/mono/loose.wav"), wav(3)).unwrap();
        std::fs::write(
            tmp.join("patch.toml"),
            "[patch]\nid = \"test-sons\"\nname = \"Sons de test\"\nversion = \"1\"\npriority = 5\n\n[[files]]\npath = \"sound/effect/mono/zz.pk\"\nfile = \"files/sound/effect/mono/zz.pk\"\n\n[[files]]\npath = \"sound/effect/mono/loose.wav\"\nfile = \"files/sound/effect/mono/loose.wav\"\n",
        )
        .unwrap();
        let assets = Arc::new(Assets::new(None, &[tmp.clone()]));
        let lib = SoundLibrary::new(assets, None);
        let s = lib.read("sound\\effect\\mono\\zz0001.wav").expect("entrée 1");
        assert!(matches!(s.source, SoundSource::Scanned { ref pack, ref entry } if pack == "sound/effect/mono/zz.pk" && entry.index == 1));
        assert_eq!(s.bytes, wav(2));
        let loose = lib.read("sound/effect/mono/loose.wav").expect("fichier en clair");
        assert!(matches!(loose.source, SoundSource::File(_)));
        assert!(lib.read("sound/effect/mono/zz0002.wav").is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
