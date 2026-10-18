//! Point d'entrée ArchCore : `archcore [login|world|all]`.
//!
//! Lit `config/archcore.json` (ou `ARCHCORE_CONFIG`) depuis le dossier courant (ou `ARCHCORE_ROOT`),
//! comme le serveur Node d'origine, et utilise les mêmes bases MariaDB.

mod character;
mod config;
mod db;
mod log;
mod login;
mod net;
mod world;

use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() {
    log::init();
    if let Err(e) = run().await {
        error!("{e:#}");
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "all".into());
    if !matches!(mode.as_str(), "login" | "world" | "all") {
        anyhow::bail!("mode inconnu {mode:?} : login, world ou all");
    }
    let config = Arc::new(config::load()?);
    let db = db::Databases::new(&config.database);
    db.check().await?;
    let templates = Arc::new(character::Templates::load(&db).await?);

    if mode == "login" || mode == "all" {
        let svc = Arc::new(login::LoginService::new(db.clone(), templates.clone(), config.clone()));
        svc.refresh_world_list().await?;
        net::serve("login", &config.login, Arc::new(svc.clone())).await?;
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(60));
            tick.tick().await;
            loop {
                tick.tick().await;
                if let Err(e) = svc.refresh_world_list().await {
                    warn!("liste des mondes : {e:#}");
                }
            }
        });
    }

    if mode == "world" || mode == "all" {
        let translator = world::npcs::Translator::load(&config.root, &config.world.language);
        let npcs = world::npcs::NpcRegistry::load(&db, &translator).await?;
        let catalog = world::items::ItemCatalog::load(&db).await?;
        let state = world::WorldState {
            view_radius: config.world.view_radius,
            rates: world::gm::ServerRates::new(&config.world.rates),
            ..Default::default()
        };
        let w = Arc::new(world::World {
            ctx: world::WorldCtx { db: db.clone(), templates: templates.clone(), config: config.clone(), npcs, catalog, view_radius: config.world.view_radius },
            state: tokio::sync::Mutex::new(state),
        });
        world::presence::spawn_autosave(w.clone());
        let listen = config::Listen { port: config.world.port, bind: config.world.bind.clone() };
        net::serve("world", &listen, Arc::new(world::WorldService(w.clone()))).await?;
    }

    tokio::signal::ctrl_c().await?;
    info!("arrêt…");
    db.close().await;
    Ok(())
}
