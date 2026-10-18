//! `archrust` : point d'entrée du client et de ses outils.
//!
//! ```text
//! archrust play      [--game <Game>] [--preview a0000005.ecl]   lance le jeu
//! archrust login     [--host 127.0.0.1] <user> <pass> <perso>   rejoue login → monde sans affichage
//! archrust inspect   [--game <Game>] <pack|texture|table|tile> … inspecte les données du client
//! archrust extract   [--game <Game>] texture <nom> <sortie.png>  exporte une texture décodée
//! archrust addons    [--dir addons]                              vérifie et liste les AddOns
//! archrust patch     build <dossier> <sortie.ahp> | check <patch> construit / vérifie un patch HD
//! ```

use anyhow::{anyhow, Context, Result};
use archrust_crypto::{capi_crypt, KEY_TEXTURE};
use archrust_data::rw::{Clump, TextureDictionary};
use archrust_engine::{Assets, Config};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "archrust", version, about = "Client ArchLord réécrit en Rust (x86-64)")]
struct Cli {
    /// Dossier `Game` du client ArchLord 64 bits.
    #[arg(long, global = true, default_value = "../client/ArchLord/Game")]
    game: PathBuf,
    /// Dossier des AddOns Lua.
    #[arg(long, global = true, default_value = "addons")]
    addons: PathBuf,
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
    Pack { dir: String, #[arg(long, default_value_t = 20)] limit: usize },
    /// Décrit un modèle d'un pack.
    Model { dir: String, name: String },
    /// Décrit une texture par nom.
    Texture { name: String },
    /// Affiche une table ini (sections et premières clés).
    Table { file: String, #[arg(long, default_value = "ini")] dir: String, #[arg(long, default_value_t = 5)] limit: usize },
    /// Liste une tuile du monde (`a001717x.ma2`).
    Tile { file: String },
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
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).format_timestamp_millis().init();
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Play { preview, connect } => archrust_engine::run(Config {
            game_dir: cli.game,
            addons_dir: cli.addons,
            patches_dir: cli.patches,
            saved_dir: PathBuf::from("saved-variables"),
            title: "ArchRust".into(),
            preview_model: preview,
            auto_connect: connect,
        }),
        Cmd::Login { host, user, password, character } => tokio::runtime::Runtime::new()?.block_on(login(&host, &user, &password, &character)),
        Cmd::Inspect { what } => inspect(&cli.game, &cli.patches, what),
        Cmd::Extract { name, out } => {
            let assets = Assets::new(&cli.game, Some(&cli.patches));
            let img = assets.texture(&name)?;
            image::RgbaImage::from_raw(img.width, img.height, img.data.clone()).ok_or_else(|| anyhow!("image invalide"))?.save(&out)?;
            println!("{name} ({}x{}) → {}", img.width, img.height, out.display());
            Ok(())
        }
        Cmd::Addons => {
            let mut mgr = archrust_addons::AddonManager::new("saved-variables").map_err(|e| anyhow!("AddOns : {e}"))?;
            mgr.load_dir(&cli.addons).map_err(|e| anyhow!("AddOns : {e}"))?;
            for a in &mgr.addons {
                let m = &a.manifest;
                println!("{} {} {} — {} fichier(s){}", if a.enabled { "✔" } else { "✘" }, m.title, m.version, m.files.len(), a.error.as_ref().map(|e| format!(" — {e}")).unwrap_or_default());
            }
            Ok(())
        }
        Cmd::Patch { op } => match op {
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
                    println!("{:>5}  {}  {} ({})", p.manifest.patch.priority, p.manifest.patch.id, p.manifest.patch.name, p.manifest.patch.version);
                }
                if stack.is_empty() {
                    println!("aucun patch dans {}", cli.patches.display());
                }
                Ok(())
            }
        },
    }
}

async fn login(host: &str, user: &str, password: &str, character: &str) -> Result<()> {
    use archrust_netclient::{LoginClient, WorldClient, WorldEvent};
    let mut login = LoginClient::connect(host, 11002).await?;
    println!("mondes : {}", login.worlds.iter().map(|w| w.name.as_str()).collect::<Vec<_>>().join(", "));
    login.authenticate(user, password).await?;
    let world = login.worlds.first().map(|w| w.name.clone()).unwrap_or_else(|| "ArchCore".into());
    let names = login.select_world(&world).await?;
    for c in login.list_characters().await? {
        println!("  {} (cid {}, tid {}, niveau {})", c.name, c.cid, c.tid, c.level);
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
    let seen = events.iter().filter(|e| matches!(e, WorldEvent::CharacterSeen(_))).count();
    println!("en jeu : cid {}, position {:?}, {seen} entités en vue — flux complet OK", wc.cid, wc.position);
    Ok(())
}

fn inspect(game: &PathBuf, patches: &PathBuf, what: Inspect) -> Result<()> {
    let assets = Assets::new(game, Some(patches));
    match what {
        Inspect::Pack { dir, limit } => {
            let names = assets.list_packed(&dir)?;
            println!("{dir} : {} entrées", names.len());
            for n in names.iter().take(limit) {
                println!("  {n}");
            }
        }
        Inspect::Model { dir, name } => {
            let c: std::sync::Arc<Clump> = assets.clump(&dir, &name)?;
            println!("{dir}/{name} : {} frames, {} géométries, {} atomics", c.frames.len(), c.geometries.len(), c.atomics.len());
            for (i, g) in c.geometries.iter().enumerate() {
                println!("  géométrie {i} : {} sommets, {} triangles, {} UV, peau {} ; matériaux : {}", g.vertices.len(), g.triangles.len(), g.uv_sets.len(), g.skin.as_ref().map(|s| s.bone_count as usize).unwrap_or(0), g.materials.iter().map(|m| m.texture.as_ref().map(|t| t.name.clone()).unwrap_or_else(|| "(uni)".into())).collect::<Vec<_>>().join(", "));
            }
            for f in c.frames.iter().filter(|f| !f.name.is_empty()).take(12) {
                println!("  frame {:?} parent {}", f.name, f.parent);
            }
        }
        Inspect::Texture { name } => {
            let file = format!("{name}.tx1");
            let raw = std::fs::read(game.join("texture/notpacked").join(&file)).or_else(|_| {
                archrust_engine::assets::TEXTURE_GROUPS.iter().find_map(|g| assets.read_packed(&format!("texture/{g}"), &file).ok()).ok_or_else(|| anyhow!("{file} introuvable"))
            })?;
            let dict = TextureDictionary::parse(&capi_crypt(KEY_TEXTURE, &raw)).context("dictionnaire")?;
            for t in &dict.textures {
                println!("{} : {}x{} {:?} profondeur {} alpha {} {} niveau(x)", t.name, t.width, t.height, t.format, t.depth, t.has_alpha, t.mips.len());
            }
        }
        Inspect::Table { file, dir, limit } => {
            let t = assets.table(&dir, &file)?;
            println!("{dir}/{file} : {} sections", t.len());
            for s in t.sections.iter().take(limit) {
                println!("[{}] {}", s.name, s.entries.iter().take(6).map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(" "));
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
