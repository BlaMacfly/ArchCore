//! Serveur ArchCore : connexion (login) et monde, sur les bases MariaDB du projet.

use crate::{info, warn};
use std::sync::Arc;
use std::time::Duration;

/// Lance le serveur de connexion, le serveur monde, ou les deux.
pub async fn run(mode: &str) -> anyhow::Result<()> {
    let config = Arc::new(crate::config::load_mode(mode)?);
    let db = crate::db::Databases::new(&config.database);
    db.check().await?;
    // Contrôle de version : refuse une base en retard ou en avance sur cet exécutable.
    crate::migrations::verify(&db, [config.database.auth.as_str(), config.database.characters.as_str(), config.database.world.as_str()]).await?;
    let templates = Arc::new(crate::character::Templates::load(&db).await?);
    // Langues proposées et textes localisés ; le résumé sert au HELLO_ACK des deux serveurs.
    let locale = crate::locale::Catalog::load(&db, &config.root, &config.world.language).await;
    crate::locale::install(locale.summary.clone());

    if mode == "login" || mode == "all" {
        let svc = Arc::new(crate::login::LoginService::new(db.clone(), templates.clone(), config.clone()));
        svc.refresh_world_list().await?;
        crate::net::serve("login", &config.login, Arc::new(svc.clone())).await?;
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
        let translator = crate::world::npcs::Translator::load(&config.root, &config.world.language);
        let npcs = crate::world::npcs::NpcRegistry::load(&db, &translator).await?;
        let catalog = crate::world::items::ItemCatalog::load(&db).await?;
        let map = crate::world::map::WorldMap::load(&db).await?;
        let loot = crate::world::loot::LootTables::load(&db).await?;
        let regions = crate::world::regions::Regions::load(&db).await?;
        let skills = crate::world::skills::SkillCatalog::load(&db).await?;
        let quests = crate::world::quests::QuestCatalog::load(&db).await?;
        let mut level_exp = Vec::new();
        for r in db.world.rows("SELECT level, exp FROM level_exp ORDER BY level", ()).await? {
            let level: usize = crate::db::col(&r, "level");
            if level >= level_exp.len() {
                level_exp.resize(level + 1, 0);
            }
            level_exp[level] = crate::db::col(&r, "exp");
        }
        let monsters = crate::world::monsters::MonsterRegistry::load(&db, &translator, &map, config.world.monster_density).await?;
        let state = crate::world::WorldState {
            view_radius: config.world.view_radius,
            monsters,
            rates: crate::world::gm::ServerRates::new(&config.world.rates),
            ..Default::default()
        };
        let w = Arc::new(crate::world::World {
            ctx: crate::world::WorldCtx { db: db.clone(), templates: templates.clone(), config: config.clone(), npcs, catalog, map, level_exp, loot, regions, skills, quests, locale, view_radius: config.world.view_radius },
            state: tokio::sync::Mutex::new(state),
        });
        crate::world::presence::spawn_autosave(w.clone());
        crate::world::combat::spawn_ticker(w.clone());
        let listen = crate::config::Listen { port: config.world.port, bind: config.world.bind.clone() };
        crate::net::serve("world", &listen, Arc::new(crate::world::WorldService(w.clone()))).await?;
    }

    tokio::signal::ctrl_c().await?;
    info!("arrêt…");
    db.close().await;
    Ok(())
}
