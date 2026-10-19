//! Exporte les sons et musiques du client dans un dossier rangé par famille.
//!
//! ```text
//! cargo run -p archrust-data --example dump_soundpak -- <Game>/sound <dossier de sortie>
//! ```
//!
//! Contenu du dossier `sound/` du client et ce qui en est fait :
//!
//! * `bgm/*.mp3` (musiques) et `stream/*.mp3` (ambiances de zone, eau, orage) : copiés tels quels ;
//! * `ui/` (pack `Reference.Dat` + `Data.Dat`) : chaque entrée WAV écrite sur disque ;
//! * `effect/*.pk` et `effect/mono/*.pk` : des WAV mis bout à bout, indexés par `soundpak.info`
//!   (format décrit dans `archrust_data::soundpak`). Les packs absents de l'index (dossier
//!   `mono/`) sont découpés en balayant les en-têtes `RIFF` ;
//! * `effect/*.wav` : copiés dans `divers/`.

use anyhow::{bail, Context, Result};
use archrust_data::pack::PackIndex;
use archrust_data::soundpak::{scan_riff, SoundEntry, SoundIndex};
use std::fs;
use std::path::{Path, PathBuf};

fn write_entries(bytes: &[u8], entries: &[SoundEntry], out_dir: &Path) -> Result<(usize, usize)> {
    fs::create_dir_all(out_dir)?;
    let (mut ok, mut bad) = (0, 0);
    for e in entries {
        match e.slice(bytes) {
            Ok(data) => {
                fs::write(out_dir.join(e.name.to_ascii_lowercase()), data)?;
                ok += 1;
            }
            Err(err) => {
                eprintln!("  {err}");
                bad += 1;
            }
        }
    }
    Ok((ok, bad))
}

fn copy_dir(src: &Path, dst: &Path, ext: &str) -> Result<usize> {
    if !src.is_dir() {
        return Ok(0);
    }
    fs::create_dir_all(dst)?;
    let mut n = 0;
    for entry in fs::read_dir(src)? {
        let p = entry?.path();
        if p.is_file() && p.extension().map(|e| e.eq_ignore_ascii_case(ext)).unwrap_or(false) {
            fs::copy(&p, dst.join(p.file_name().unwrap()))?;
            n += 1;
        }
    }
    Ok(n)
}

fn dump_pack(pack_dir: &Path, out: &Path) -> Result<usize> {
    let find = |name: &str| fs::read_dir(pack_dir).ok()?.flatten().map(|e| e.path()).find(|p| p.file_name().map(|f| f.to_string_lossy().eq_ignore_ascii_case(name)).unwrap_or(false));
    let (Some(r), Some(d)) = (find("Reference.Dat"), find("Data.Dat")) else { return Ok(0) };
    let index = PackIndex::parse_encrypted(&fs::read(&r)?)?;
    let mut data = fs::File::open(&d)?;
    fs::create_dir_all(out)?;
    let mut n = 0;
    for e in &index.entries {
        let bytes = index.read_entry(&mut data, e)?;
        fs::write(out.join(e.name.to_ascii_lowercase()), bytes)?;
        n += 1;
    }
    Ok(n)
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [sound, out] = args.as_slice() else {
        bail!("usage : dump_soundpak <Game>/sound <dossier de sortie>");
    };
    let sound = PathBuf::from(sound);
    let out = PathBuf::from(out);
    fs::create_dir_all(&out)?;

    let n = copy_dir(&sound.join("bgm"), &out.join("01-musiques"), "mp3")?;
    println!("01-musiques : {n} MP3");
    let n = copy_dir(&sound.join("stream"), &out.join("02-ambiances"), "mp3")?;
    println!("02-ambiances : {n} MP3");
    let n = dump_pack(&sound.join("ui"), &out.join("03-interface"))?;
    println!("03-interface : {n} fichiers du pack sound/ui");

    let info_path = sound.join("soundpak.info");
    let info = if info_path.is_file() { SoundIndex::parse(&fs::read(&info_path)?).context("soundpak.info")? } else { SoundIndex::default() };
    println!("index soundpak.info : {} packs, {} entrées", info.packs().count(), info.entry_count());

    for (sub, label) in [("effect", "04-effets"), ("effect/mono", "05-effets-mono")] {
        let dir = sound.join(sub);
        if !dir.is_dir() {
            continue;
        }
        let mut paks: Vec<PathBuf> = fs::read_dir(&dir)?.flatten().map(|e| e.path()).filter(|p| p.extension().map(|e| e.eq_ignore_ascii_case("pk")).unwrap_or(false)).collect();
        paks.sort();
        let (mut files, mut indexed, mut scanned, mut bad) = (0, 0, 0, 0);
        for pak in &paks {
            let file = pak.file_name().unwrap().to_string_lossy().to_ascii_lowercase();
            let stem = file.trim_end_matches(".pk").to_string();
            let bytes = fs::read(pak)?;
            let (entries, how) = match info.pack(&file) {
                Some(p) if !p.entries.is_empty() => (p.entries.clone(), "index"),
                _ => (scan_riff(&bytes, &stem), "riff"),
            };
            if how == "index" {
                indexed += 1;
            } else {
                scanned += 1;
            }
            let (ok, ko) = write_entries(&bytes, &entries, &out.join(label).join(&stem))?;
            files += ok;
            bad += ko;
        }
        println!("{label} : {} packs ({indexed} via l'index, {scanned} par balayage RIFF), {files} WAV, {bad} entrées invalides", paks.len());
        let n = copy_dir(&dir, &out.join(label).join("divers"), "wav")?;
        if n > 0 {
            println!("{label}/divers : {n} WAV en clair");
        }
    }
    Ok(())
}
