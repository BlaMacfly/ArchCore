//! Vérifie la structure de toutes les entrées `.ean` du pack `character/animation`.
//!
//! Usage : `cargo run -p archrust-data --example inspect_ean -- [dossier Game]`

use archrust_data::{ean::EanFile, pack::PackIndex};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    path::PathBuf,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let game = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../client/ArchLord/Game"));
    let pack_dir = game.join("character/animation");
    let index = PackIndex::parse_encrypted(&fs::read(pack_dir.join("Reference.Dat"))?)?;
    let mut data = File::open(pack_dir.join("Data.Dat"))?;
    let entries: Vec<_> = index
        .entries
        .iter()
        .filter(|e| e.name.ends_with(".ean"))
        .collect();
    let mut block_counts = BTreeMap::<u32, usize>::new();
    let mut sequences = BTreeMap::<String, usize>::new();
    let mut min_keys = usize::MAX;
    let mut max_keys = 0usize;
    let mut bad_animation_chunks = Vec::new();
    let mut bad_times = 0usize;
    let mut bad_rotations = 0usize;
    let mut bad_translations = 0usize;
    let mut atypical_files = BTreeMap::<String, usize>::new();
    let mut track_count_histogram = BTreeMap::<usize, usize>::new();
    let mut bad_track_files = Vec::new();

    for entry in &entries {
        let bytes = index.read_entry(&mut data, entry)?;
        let file = EanFile::parse(&bytes)?;
        let ids: Vec<_> = file.chunks.iter().map(|c| c.id).collect();
        let sequence = ids
            .iter()
            .map(|id| format!("{id:#x}"))
            .collect::<Vec<_>>()
            .join(" ");
        *sequences.entry(sequence).or_default() += 1;
        for chunk in &file.chunks {
            *block_counts.entry(chunk.id).or_default() += 1;
            if chunk.id == 0x1b {
                let Some(layout) = chunk.animation_layout() else {
                    bad_animation_chunks.push(entry.name.clone());
                    continue;
                };
                min_keys = min_keys.min(layout.key_count);
                max_keys = max_keys.max(layout.key_count);
                let tracks = chunk.linked_tracks().expect("layout vérifiée");
                *track_count_histogram.entry(tracks.len()).or_default() += 1;
                if tracks
                    .iter()
                    .any(|track| track.windows(2).any(|pair| pair[0] >= pair[1]))
                {
                    bad_track_files.push(entry.name.clone());
                }
                let mut atypical_here = 0;
                let keys: Vec<_> = chunk.h_anim_keys().expect("layout vérifiée").collect();
                if [
                    "a0001011.ean",
                    "a0060000.ean",
                    "a0060010.ean",
                    "a0060011.ean",
                ]
                .contains(&entry.name.as_str())
                {
                    let times: Vec<_> = keys.iter().map(|k| k.time).collect();
                    let mut track_lengths = BTreeMap::<usize, usize>::new();
                    for track in &tracks {
                        *track_lengths.entry(track.len()).or_default() += 1;
                    }
                    let links: std::collections::BTreeSet<_> =
                        keys.iter().map(|k| k.previous).collect();
                    let monotonic = times.windows(2).filter(|w| w[0] <= w[1]).count();
                    let backward_links = keys
                        .iter()
                        .enumerate()
                        .filter(|(i, k)| k.previous % 36 == 0 && (k.previous / 36) < *i as u32)
                        .count();
                    let same_time_links = keys
                        .iter()
                        .enumerate()
                        .filter(|(i, k)| {
                            k.previous % 36 == 0
                                && (k.previous / 36) < keys.len() as u32
                                && times[(k.previous / 36) as usize] == times[*i]
                        })
                        .count();
                    let mut link_deltas = BTreeMap::<i64, usize>::new();
                    for (i, key) in keys.iter().enumerate() {
                        if key.previous % 36 == 0 && key.previous < (keys.len() * 36) as u32 {
                            *link_deltas
                                .entry(i as i64 - (key.previous / 36) as i64)
                                .or_default() += 1;
                        }
                    }
                    let mut time_counts = BTreeMap::<u32, usize>::new();
                    for time in &times {
                        *time_counts.entry(time.to_bits()).or_default() += 1;
                    }
                    println!("{} préfixe={:?}, clés={}, temps initiaux={:?}, temps finaux={:?}, transitions monotones={monotonic}/{}, liens uniques={}, liens nuls={}, liens alignés dans le bloc={}", entry.name, layout.prefix, keys.len(), &times[..times.len().min(8)], &times[times.len().saturating_sub(8)..], times.len().saturating_sub(1), links.len(), keys.iter().filter(|k| k.previous == 0).count(), keys.iter().filter(|k| k.previous < (keys.len() * 36) as u32 && k.previous % 36 == 0).count());
                    println!("{} liens vers clés antérieures={backward_links}, lien antérieur avec même temps={same_time_links}, temps uniques={}", entry.name, times.iter().map(|t| t.to_bits()).collect::<std::collections::BTreeSet<_>>().len());
                    println!(
                        "{} écarts des liens (courant - cible) : {:?}",
                        entry.name,
                        link_deltas.iter().take(12).collect::<Vec<_>>()
                    );
                    println!(
                        "{} pistes={} longueurs={track_lengths:?}",
                        entry.name,
                        tracks.len()
                    );
                    println!(
                        "{} effectifs des temps : {:?}",
                        entry.name,
                        time_counts
                            .iter()
                            .map(|(bits, n)| (f32::from_bits(*bits), n))
                            .collect::<Vec<_>>()
                    );
                    let middle = times
                        .iter()
                        .copied()
                        .filter(|t| t.is_finite())
                        .fold(0.0f32, f32::max)
                        * 0.5;
                    let pose = chunk
                        .sample_linked_tracks(middle)
                        .expect("échantillonnage de pistes");
                    let invalid_samples = pose
                        .iter()
                        .filter(|sample| {
                            sample
                                .rotation
                                .iter()
                                .chain(sample.translation.iter())
                                .any(|v| !v.is_finite())
                                || (sample.rotation.iter().map(|v| v * v).sum::<f32>() - 1.0).abs()
                                    > 1e-4
                        })
                        .count();
                    println!("{} pose interpolée à {middle}s : {} pistes, invalides={}, premier échantillon {:?}", entry.name, pose.len(), invalid_samples, pose.first());
                }
                for key in keys {
                    if !key.time.is_finite() {
                        bad_times += 1;
                    }
                    let q2: f32 = key.rotation.iter().map(|v| v * v).sum();
                    let bad_q = !q2.is_finite() || !(0.5..=1.5).contains(&q2);
                    let bad_t = key.translation.iter().any(|v| !v.is_finite());
                    if bad_q {
                        bad_rotations += 1;
                    }
                    if bad_t {
                        bad_translations += 1;
                    }
                    atypical_here += usize::from(bad_q || bad_t);
                }
                if atypical_here > 0 {
                    atypical_files.insert(entry.name.clone(), atypical_here);
                }
            }
        }
    }

    println!("{} entrées `.ean`", entries.len());
    println!("blocs : {block_counts:#?}");
    println!("séquences : {sequences:#?}");
    println!("nombre de clés par bloc 0x1b : min={min_keys}, max={max_keys}");
    println!(
        "pistes candidates : {} formes, min={}, max={}",
        track_count_histogram.len(),
        track_count_histogram.keys().next().copied().unwrap_or(0),
        track_count_histogram
            .keys()
            .next_back()
            .copied()
            .unwrap_or(0)
    );
    println!(
        "fichiers avec ordre de piste incohérent : {}",
        bad_track_files.len()
    );
    println!("blocs 0x1b non reconnus : {}", bad_animation_chunks.len());
    for name in bad_animation_chunks.iter().take(20) {
        println!("  {name}");
    }
    println!("temps non finis : {bad_times}; quaternions non unitaires : {bad_rotations}; translations non finies : {bad_translations}");
    println!("fichiers avec clés atypiques : {}", atypical_files.len());
    for (name, count) in atypical_files.iter().take(20) {
        println!("  {name}: {count}");
    }
    Ok(())
}
