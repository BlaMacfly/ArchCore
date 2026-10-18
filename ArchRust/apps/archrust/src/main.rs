//! `archrust` : point d'entrée du client et de ses outils.
//!
//! ```text
//! archrust play      [--game <Game>] [--preview a0000005.ecl]   lance le jeu
//! archrust login     [--host 127.0.0.1] <user> <pass> <perso>   rejoue login → monde sans affichage
//! archrust inspect   [--game <Game>] <pack|model|texture|animation|table|tile> … inspecte les données du client
//! archrust inspect   gltf <fichier.glb>                         décrit un glTF/GLB (sans client)
//! archrust extract   [--game <Game>] texture <nom> <sortie.png>  exporte une texture décodée
//! archrust addons    [--dir addons]                              vérifie et liste les AddOns
//! archrust patch     build <dossier> <sortie.ahp> | check <patch> construit / vérifie un patch HD
//! ```

use anyhow::{anyhow, Context, Result};
use archrust_crypto::{capi_crypt, KEY_TEXTURE};
use archrust_data::rw::{Clump, TextureDictionary};
use archrust_engine::gltf_model::GltfModel;
use archrust_engine::{Assets, Config};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "archrust",
    version,
    about = "Client ArchLord réécrit en Rust (x86-64)"
)]
struct Cli {
    /// Dossier `Game` du client ArchLord 64 bits. Par défaut : `assets-archonia/Game`,
    /// `assets-archonia/` puis `../client/ArchLord/Game`, le premier qui existe.
    #[arg(long, global = true)]
    game: Option<PathBuf>,
    /// Ignore le client : seuls les patchs (dont `free-assets/`) sont utilisés, le reste est remplacé
    /// par des substituts générés.
    #[arg(long, global = true)]
    no_client: bool,
    /// Dossier des AddOns Lua.
    #[arg(long, global = true, default_value = "addons")]
    addons: PathBuf,
    /// Dossier des ressources libres publiées avec le dépôt (un patch de priorité maximale).
    #[arg(long, global = true, default_value = "free-assets")]
    free_assets: PathBuf,
    /// Dossier des patchs HD (`.ahp` ou dossiers).
    #[arg(long, global = true, default_value = "patches")]
    patches: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Lance le client graphique.
    Play {
        /// Modèle du pack `character` à afficher en aperçu (ex. a0000005.ecl).
        #[arg(long)]
        preview: Option<String>,
        /// Connexion automatique : `utilisateur:motdepasse@hôte:port/personnage`.
        #[arg(long)]
        connect: Option<String>,
        /// Enregistre une capture PNG puis quitte.
        #[arg(long)]
        screenshot: Option<PathBuf>,
        /// Délai avant la capture, en secondes.
        #[arg(long, default_value_t = 6.0)]
        screenshot_after: f32,
        /// Commande exécutée à l'entrée en jeu (ex. "/goto -478000 0 -68000").
        #[arg(long)]
        exec: Option<String>,
    },
    /// Rejoue le flux complet login → monde sans affichage.
    Login {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(default_value = "test")]
        user: String,
        #[arg(default_value = "test")]
        password: String,
        #[arg(default_value = "Testeur")]
        character: String,
    },
    /// Inspecte les données du client.
    Inspect {
        #[command(subcommand)]
        what: Inspect,
    },
    /// Exporte une texture décodée en PNG.
    Extract { name: String, out: PathBuf },
    /// Exporte un modèle d'un pack en OBJ + MTL + textures PNG (référence privée, à ne pas publier).
    Export {
        /// Pack (`character`, `object`, `effect/clump`…).
        dir: String,
        /// Nom de l'entrée (`a0000060.ecl`).
        name: String,
        /// Fichier `.obj` de sortie ; le `.mtl` et les PNG sont écrits à côté.
        out: PathBuf,
    },
    /// Vérifie et liste les AddOns.
    Addons,
    /// Outils de patch HD.
    Patch {
        #[command(subcommand)]
        op: PatchOp,
    },
}

#[derive(Subcommand)]
enum Inspect {
    /// Liste un pack (`character`, `object`, `texture/world`…).
    Pack {
        dir: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Décrit un modèle d'un pack.
    Model { dir: String, name: String },
    /// Décrit les blocs, pistes et durée d'une animation `.ean`.
    Animation {
        name: String,
        /// Échantillonne les pistes à ce temps (secondes) et affiche le nombre obtenu.
        #[arg(long, default_value_t = 0.0)]
        time: f32,
        /// Compare les translations initiales aux os HAnim d'un modèle (ex. `b0000060.ecl`).
        #[arg(long)]
        model: Option<String>,
    },
    /// Décrit une texture par nom.
    Texture { name: String },
    /// Affiche une table ini (sections et premières clés).
    Table {
        file: String,
        #[arg(long, default_value = "ini")]
        dir: String,
        #[arg(long, default_value_t = 5)]
        limit: usize,
        /// Clés à afficher (séparées par des virgules), ex. `Name,Race,Gender,Class` ; sinon les 6 premières.
        #[arg(long)]
        keys: Option<String>,
    },
    /// Liste une tuile du monde (`a001717x.ma2`).
    Tile { file: String },
    /// Décrit un fichier glTF/GLB (maillages, images, nœuds, boîte englobante) ; ne demande pas de client.
    Gltf { file: PathBuf },
}

#[derive(Subcommand)]
enum PatchOp {
    /// Construit une archive `.ahp` depuis un dossier de patch.
    Build { dir: PathBuf, out: PathBuf },
    /// Vérifie un patch (dossier ou `.ahp`).
    Check { path: PathBuf },
    /// Liste les patchs actifs et leur priorité.
    List,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();
    let cli = Cli::parse();
    let game_dir = if cli.no_client {
        None
    } else {
        archrust_engine::assets::find_client_dir(cli.game.as_deref())
    };
    let patch_dirs = vec![cli.free_assets.clone(), cli.patches.clone()];
    let require_client = |what: &str| -> Result<PathBuf> {
        game_dir.clone().ok_or_else(|| anyhow!("{what} demande un client ArchLord : --game <Game>, ou le client dans assets-archonia/ (voir assets-archonia/README.md)"))
    };
    match cli.cmd {
        Cmd::Play {
            preview,
            connect,
            screenshot,
            screenshot_after,
            exec,
        } => archrust_engine::run(Config {
            game_dir,
            addons_dir: cli.addons,
            patch_dirs,
            saved_dir: PathBuf::from("saved-variables"),
            title: "ArchRust".into(),
            preview_model: preview,
            auto_connect: connect,
            screenshot: screenshot.map(|p| (p, screenshot_after)),
            exec_on_enter: exec,
        }),
        Cmd::Login {
            host,
            user,
            password,
            character,
        } => tokio::runtime::Runtime::new()?.block_on(login(&host, &user, &password, &character)),
        Cmd::Inspect {
            what: Inspect::Gltf { file },
        } => inspect_gltf(&file),
        Cmd::Inspect { what } => inspect(&require_client("inspect")?, &patch_dirs, what),
        Cmd::Export { dir, name, out } => {
            let assets = Assets::new(Some(require_client("export")?), &patch_dirs);
            export_obj(&assets, &dir, &name, &out)
        }
        Cmd::Extract { name, out } => {
            let assets = Assets::new(Some(require_client("extract")?), &patch_dirs);
            let img = assets.texture(&name)?;
            image::RgbaImage::from_raw(img.width, img.height, img.data.clone())
                .ok_or_else(|| anyhow!("image invalide"))?
                .save(&out)?;
            println!("{name} ({}x{}) → {}", img.width, img.height, out.display());
            Ok(())
        }
        Cmd::Addons => {
            let mut mgr = archrust_addons::AddonManager::new("saved-variables")
                .map_err(|e| anyhow!("AddOns : {e}"))?;
            mgr.load_dir(&cli.addons)
                .map_err(|e| anyhow!("AddOns : {e}"))?;
            for a in &mgr.addons {
                let m = &a.manifest;
                println!(
                    "{} {} {} — {} fichier(s){}",
                    if a.enabled { "✔" } else { "✘" },
                    m.title,
                    m.version,
                    m.files.len(),
                    a.error
                        .as_ref()
                        .map(|e| format!(" — {e}"))
                        .unwrap_or_default()
                );
            }
            Ok(())
        }
        Cmd::Patch { op } => {
            match op {
                PatchOp::Build { dir, out } => {
                    archrust_patch::PatchStack::build_archive(&dir, &out)?;
                    println!("patch construit : {}", out.display());
                    Ok(())
                }
                PatchOp::Check { path } => {
                    let p = archrust_patch::PatchStack::load_one(&path)?;
                    p.validate()?;
                    println!("{} v{} (priorité {}) : {} textures, {} modèles, {} fichiers, {} shaders — OK", p.manifest.patch.name, p.manifest.patch.version, p.manifest.patch.priority, p.manifest.textures.len(), p.manifest.models.len(), p.manifest.files.len(), p.manifest.shaders.len());
                    Ok(())
                }
                PatchOp::List => {
                    let stack = archrust_patch::PatchStack::load_dir(&cli.patches);
                    for p in &stack.patches {
                        println!(
                            "{:>5}  {}  {} ({})",
                            p.manifest.patch.priority,
                            p.manifest.patch.id,
                            p.manifest.patch.name,
                            p.manifest.patch.version
                        );
                    }
                    if stack.is_empty() {
                        println!("aucun patch dans {}", cli.patches.display());
                    }
                    Ok(())
                }
            }
        }
    }
}

async fn login(host: &str, user: &str, password: &str, character: &str) -> Result<()> {
    use archrust_netclient::{LoginClient, WorldClient, WorldEvent};
    let mut login = LoginClient::connect(host, 11002).await?;
    println!(
        "mondes : {}",
        login
            .worlds
            .iter()
            .map(|w| w.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    login.authenticate(user, password).await?;
    let world = login
        .worlds
        .first()
        .map(|w| w.name.clone())
        .unwrap_or_else(|| "ArchCore".into());
    let names = login.select_world(&world).await?;
    for c in login.list_characters().await? {
        println!(
            "  {} (cid {}, tid {}, niveau {})",
            c.name, c.cid, c.tid, c.level
        );
    }
    if !names.iter().any(|n| n == character) {
        login.enter_creation().await?;
        let cid = login.create_character(character, 96, 1, 2).await?;
        println!("personnage {character} créé (cid {cid})");
    }
    let ticket = login.enter_world(character, &world).await?;
    login.close().await;
    let mut wc = WorldClient::connect(ticket).await?;
    let events = wc.enter().await?;
    let seen = events
        .iter()
        .filter(|e| matches!(e, WorldEvent::CharacterSeen(_)))
        .count();
    println!(
        "en jeu : cid {}, position {:?}, {seen} entités en vue — flux complet OK",
        wc.cid, wc.position
    );
    Ok(())
}

/// Écrit un clump en OBJ (une partie par atomic et par matériau, matrices de frames appliquées),
/// un MTL et les textures décodées en PNG dans le dossier de sortie.
fn export_obj(assets: &Assets, dir: &str, name: &str, out: &PathBuf) -> Result<()> {
    use std::fmt::Write as _;
    let clump = assets.clump(dir, name)?;
    let world = clump.world_matrices();
    let stem = out
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "model".into());
    let folder = out
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    std::fs::create_dir_all(&folder)?;
    let mut obj = String::new();
    let mut mtl = String::new();
    writeln!(
        obj,
        "# ArchRust export de {dir}/{name} — référence privée, ne pas redistribuer"
    )?;
    writeln!(obj, "mtllib {stem}.mtl")?;
    let mut base = 1usize;
    let mut textures_done = std::collections::BTreeSet::new();
    let mut parts = 0;
    for (ai, atomic) in clump.atomics.iter().enumerate() {
        let Some(g) = clump.geometries.get(atomic.geometry as usize) else {
            continue;
        };
        if g.vertices.is_empty() {
            continue;
        }
        let m = world
            .get(atomic.frame as usize)
            .copied()
            .unwrap_or(glam::Mat4::IDENTITY);
        let uv0 = g.uv_sets.first();
        for v in &g.vertices {
            let p = m.transform_point3(*v);
            writeln!(obj, "v {} {} {}", p.x, p.y, p.z)?;
        }
        for i in 0..g.vertices.len() {
            let uv = uv0.and_then(|s| s.get(i)).copied().unwrap_or([0.0, 0.0]);
            writeln!(obj, "vt {} {}", uv[0], 1.0 - uv[1])?;
        }
        let has_normals = g.normals.len() == g.vertices.len();
        if has_normals {
            for n in &g.normals {
                let nn = m.transform_vector3(*n).normalize_or_zero();
                writeln!(obj, "vn {} {} {}", nn.x, nn.y, nn.z)?;
            }
        }
        // Triangles regroupés par matériau.
        let mut by_mat: std::collections::BTreeMap<u16, Vec<[u32; 3]>> =
            std::collections::BTreeMap::new();
        if !g.meshes.is_empty() {
            for mesh in &g.meshes {
                let list = by_mat.entry(mesh.material as u16).or_default();
                if g.tristrip {
                    for (i, w) in mesh.indices.windows(3).enumerate() {
                        if w[0] == w[1] || w[1] == w[2] || w[0] == w[2] {
                            continue;
                        }
                        list.push(if i % 2 == 0 {
                            [w[0], w[1], w[2]]
                        } else {
                            [w[1], w[0], w[2]]
                        });
                    }
                } else {
                    for t in mesh.indices.chunks_exact(3) {
                        list.push([t[0], t[1], t[2]]);
                    }
                }
            }
        } else {
            for t in &g.triangles {
                by_mat.entry(t.material).or_default().push([
                    t.v[0] as u32,
                    t.v[1] as u32,
                    t.v[2] as u32,
                ]);
            }
        }
        for (mat_id, tris) in by_mat {
            let material = g.materials.get(mat_id as usize);
            let tex = material
                .and_then(|mm| mm.texture.as_ref())
                .map(|t| t.name.to_ascii_lowercase());
            let mat_name = format!("atomic{ai}_mat{mat_id}");
            writeln!(obj, "o {stem}_atomic{ai}_mat{mat_id}")?;
            writeln!(obj, "usemtl {mat_name}")?;
            writeln!(mtl, "newmtl {mat_name}")?;
            if let Some(c) = material.map(|mm| mm.color) {
                writeln!(
                    mtl,
                    "Kd {} {} {}",
                    c[0] as f32 / 255.0,
                    c[1] as f32 / 255.0,
                    c[2] as f32 / 255.0
                )?;
            }
            if let Some(t) = &tex {
                writeln!(mtl, "map_Kd {t}.png")?;
                if textures_done.insert(t.clone()) {
                    match assets.texture(t) {
                        Ok(img) if img.from_patch.as_deref() != Some("substitut") => {
                            if let Some(buf) =
                                image::RgbaImage::from_raw(img.width, img.height, img.data.clone())
                            {
                                buf.save(folder.join(format!("{t}.png")))?;
                            }
                        }
                        _ => println!("  texture {t} introuvable"),
                    }
                }
            }
            for [a, b, c] in tris {
                let (a, b, c) = (a as usize + base, b as usize + base, c as usize + base);
                if has_normals {
                    writeln!(obj, "f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}")?;
                } else {
                    writeln!(obj, "f {a}/{a} {b}/{b} {c}/{c}")?;
                }
            }
            parts += 1;
        }
        base += g.vertices.len();
    }
    std::fs::write(out, obj)?;
    std::fs::write(folder.join(format!("{stem}.mtl")), mtl)?;
    println!(
        "{dir}/{name} → {} ({parts} parties, {} textures)",
        out.display(),
        textures_done.len()
    );
    Ok(())
}

/// Décrit un glTF/GLB du disque ; les tampons et images voisins sont lus à côté du fichier.
fn inspect_gltf(file: &Path) -> Result<()> {
    let bytes = std::fs::read(file).with_context(|| format!("lecture de {}", file.display()))?;
    let dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
    let neighbour = |rel: &str| -> Result<Vec<u8>> {
        let p = dir.join(rel);
        std::fs::read(&p).with_context(|| format!("lecture de {}", p.display()))
    };
    let model = GltfModel::parse(&bytes, &neighbour)?;
    println!("{} : {}", file.display(), model.describe());
    Ok(())
}

fn inspect(game: &PathBuf, patch_dirs: &[PathBuf], what: Inspect) -> Result<()> {
    let assets = Assets::new(Some(game.clone()), patch_dirs);
    match what {
        Inspect::Gltf { file } => inspect_gltf(&file)?,
        Inspect::Pack { dir, limit } => {
            let names = assets.list_packed(&dir)?;
            println!("{dir} : {} entrées", names.len());
            for n in names.iter().take(limit) {
                println!("  {n}");
            }
        }
        Inspect::Model { dir, name } => {
            let c: std::sync::Arc<Clump> = assets.clump(&dir, &name)?;
            println!(
                "{dir}/{name} : {} frames, {} géométries, {} atomics",
                c.frames.len(),
                c.geometries.len(),
                c.atomics.len()
            );
            for (i, g) in c.geometries.iter().enumerate() {
                println!(
                    "  géométrie {i} : {} sommets, {} triangles, {} UV, peau {} ; matériaux : {}",
                    g.vertices.len(),
                    g.triangles.len(),
                    g.uv_sets.len(),
                    g.skin.as_ref().map(|s| s.bone_count as usize).unwrap_or(0),
                    g.materials
                        .iter()
                        .map(|m| m
                            .texture
                            .as_ref()
                            .map(|t| t.name.clone())
                            .unwrap_or_else(|| "(uni)".into()))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            for f in c.frames.iter().filter(|f| !f.name.is_empty()).take(12) {
                println!("  frame {:?} parent {}", f.name, f.parent);
            }
        }
        Inspect::Animation { name, time, model } => {
            let animation = assets.animation(&name)?;
            println!("{name} : {} bloc(s)", animation.chunks.len());
            let clump = model
                .as_deref()
                .map(|name| assets.clump("character", name))
                .transpose()?;
            for (index, chunk) in animation.chunks.iter().enumerate() {
                print!(
                    "  bloc {index} : type {:#x}, version {:#x}, {} octets",
                    chunk.id,
                    chunk.version,
                    chunk.payload.len()
                );
                if let Some(layout) = chunk.animation_layout() {
                    let keys: Vec<_> = chunk.h_anim_keys().into_iter().flatten().collect();
                    let tracks = chunk.linked_tracks().unwrap_or_default();
                    let track_lengths = tracks.iter().map(Vec::len).collect::<Vec<_>>();
                    let internal_links = chunk
                        .raw_keys()
                        .into_iter()
                        .flatten()
                        .filter(|raw| {
                            let offset =
                                u32::from_le_bytes(raw[32..36].try_into().unwrap()) as usize;
                            offset % 36 == 0 && offset < layout.key_count * 36
                        })
                        .count();
                    let end = keys
                        .iter()
                        .map(|key| key.time)
                        .filter(|value| value.is_finite())
                        .fold(0.0_f32, f32::max);
                    let samples = chunk
                        .sample_linked_tracks(time)
                        .map_or(0, |pose| pose.len());
                    print!(
                        " : préfixe {:08x?}, {} clé(s), {} piste(s) {:?}, {} lien(s) interne(s) aligné(s), fin {:.3}s, {} échantillon(s) à {:.3}s",
                        layout.prefix, layout.key_count, tracks.len(), track_lengths, internal_links, end, samples, time
                    );
                    if let Some(clump) = &clump {
                        let bones: Vec<_> = clump
                            .frames
                            .iter()
                            .filter_map(|frame| frame.hanim.as_ref())
                            .find(|hierarchy| !hierarchy.nodes.is_empty())
                            .into_iter()
                            .flat_map(|hierarchy| hierarchy.nodes.iter())
                            .filter_map(|node| {
                                clump
                                    .frames
                                    .iter()
                                    .enumerate()
                                    .find_map(|(frame_index, frame)| {
                                        frame.hanim.as_ref().filter(|h| h.node_id == node.id).map(
                                            |_| {
                                                (
                                                    frame_index,
                                                    *node,
                                                    clump.frames[frame_index].position,
                                                )
                                            },
                                        )
                                    })
                            })
                            .collect();
                        for (track_index, track) in tracks.iter().enumerate() {
                            let Some(key) = track
                                .iter()
                                .map(|&i| keys[i])
                                .min_by(|a, b| a.time.total_cmp(&b.time))
                            else {
                                continue;
                            };
                            let distance2 = |position: [f32; 3], bone: glam::Vec3| {
                                let dx = position[0] - bone.x;
                                let dy = position[1] - bone.y;
                                let dz = position[2] - bone.z;
                                dx * dx + dy * dy + dz * dz
                            };
                            let nearest = bones.iter().min_by(|a, b| {
                                distance2(key.translation, a.2)
                                    .total_cmp(&distance2(key.translation, b.2))
                            });
                            let ordinal = bones
                                .iter()
                                .find(|(_, node, _)| node.index as usize == track_index);
                            if track_index < 12 {
                                let describe = |bone: Option<&(
                                    usize,
                                    archrust_data::rw::HAnimNode,
                                    glam::Vec3,
                                )>| {
                                    bone.map(|(frame, node, pos)| {
                                        format!(
                                            "frame {frame}/node {:#x}/index {} Δ {:.2}",
                                            node.id,
                                            node.index,
                                            distance2(key.translation, *pos).sqrt()
                                        )
                                    })
                                    .unwrap_or_else(|| "absent".into())
                                };
                                print!("\n    piste {track_index} racine {} t={:.3} pos={:?}: plus proche {}; ordinal {}", track[0], key.time, key.translation, describe(nearest), describe(ordinal));
                            }
                        }
                    }
                }
                println!();
            }
        }
        Inspect::Texture { name } => {
            let file = format!("{name}.tx1");
            let mut found = None;
            for dir in archrust_engine::assets::texture_search_path(&assets.quality) {
                if let Some(raw) = assets.open(&format!("{dir}\\{file}"), true)? {
                    found = Some((dir, raw));
                    break;
                }
            }
            let (dir, raw) = found.ok_or_else(|| anyhow!("texture {file} introuvable"))?;
            let dict =
                TextureDictionary::parse(&capi_crypt(KEY_TEXTURE, &raw)).context("dictionnaire")?;
            for t in &dict.textures {
                println!(
                    "{} : {}x{} {:?} profondeur {} alpha {} {} niveau(x) — {dir}",
                    t.name,
                    t.width,
                    t.height,
                    t.format,
                    t.depth,
                    t.has_alpha,
                    t.mips.len()
                );
            }
        }
        Inspect::Table {
            file,
            dir,
            limit,
            keys,
        } => {
            let t = assets.table(&dir, &file)?;
            println!("{dir}/{file} : {} sections", t.len());
            let keys: Option<Vec<String>> =
                keys.map(|k| k.split(',').map(|s| s.trim().to_string()).collect());
            for s in t.sections.iter().take(limit) {
                let line = match &keys {
                    Some(ks) => ks
                        .iter()
                        .map(|k| format!("{k}={}", s.get(k).unwrap_or("-")))
                        .collect::<Vec<_>>()
                        .join(" "),
                    None => s
                        .entries
                        .iter()
                        .take(6)
                        .map(|(k, v)| format!("{k}={v}"))
                        .collect::<Vec<_>>()
                        .join(" "),
                };
                println!("[{}] {line}", s.name);
            }
        }
        Inspect::Tile { file } => {
            let bytes = std::fs::read(game.join("world").join(&file))?;
            let pack = archrust_data::magpack::MagPack::parse(&bytes)?;
            println!("{file} : {} entrées", pack.entries.len());
            for e in pack.entries.iter().take(8) {
                println!("  {} {} o", e.name, e.size);
            }
        }
    }
    Ok(())
}
