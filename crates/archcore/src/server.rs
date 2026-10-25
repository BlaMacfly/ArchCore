//! Serveur ArchCore : connexion (login) et monde, sur les bases MariaDB du projet.

use crate::{info, warn};
use std::sync::Arc;
use std::time::Duration;

/// Lance le serveur de connexion, le serveur monde, ou les deux.
pub async fn run(mode: &str) -> anyhow::Result<()> {
    let config = Arc::new(crate::config::load_mode(mode)?);
    let db = crate::db::Databases::new(&config.database);
    crate::world::set_slow_packet_ms(config.world.slow_packet_ms);
    db.check().await?;
    // Contrôle de version : refuse une base en retard ou en avance sur cet exécutable.
    crate::migrations::verify(&db, [config.database.auth.as_str(), config.database.characters.as_str(), config.database.world.as_str(), config.database.logs.as_str()]).await?;
    // Cohérence des bases (références entre bases, plages d'identifiants) : signalée, pas bloquante.
    let d = &config.database;
    crate::check::at_startup(&db, &crate::check::Names { auth: &d.auth, characters: &d.characters, world: &d.world }).await;
    let templates = Arc::new(crate::character::Templates::load(&db).await?);
    // Langues proposées et textes localisés ; le résumé sert au HELLO_ACK des deux serveurs.
    let locale = crate::locale::Catalog::load(&db, &config.root, &config.world.language).await;
    crate::locale::install(locale.summary.clone());

    // Inscription du monde avant le serveur de connexion : en mode `all`, sa première liste des
    // mondes porte déjà la bonne adresse (sinon, elle est relue toutes les 60 s).
    if mode == "world" || mode == "all" {
        register_world(&db, &config.world).await?;
    }

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
        let portals = crate::world::portal::Portals::load(&db).await?;
        let bank_slot_prices = crate::world::bank::slot_prices(&db).await;
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
            guilds: crate::world::guild::Guilds::load(&db).await?,
            rates: crate::world::gm::ServerRates::new(&config.world.rates),
            ..Default::default()
        };
        let w = Arc::new(crate::world::World {
            ctx: crate::world::WorldCtx { db: db.clone(), templates: templates.clone(), config: config.clone(), npcs, catalog, map, level_exp, loot, regions, skills, quests, locale, portals, bank_slot_prices, view_radius: config.world.view_radius },
            state: tokio::sync::Mutex::new(state),
        });
        crate::world::presence::spawn_autosave(w.clone());
        crate::world::combat::spawn_ticker(w.clone());
        let listen = crate::config::Listen { port: config.world.port, bind: config.world.bind.clone() };
        crate::net::serve("world", &listen, Arc::new(crate::world::WorldService(w.clone()))).await?;
    }

    // Serveur prêt : la mesure des requêtes lentes commence (le chargement du monde n'est pas compté).
    crate::db::set_slow_query_ms(config.database.slow_query_ms);
    tokio::signal::ctrl_c().await?;
    info!("arrêt…");
    db.close().await;
    Ok(())
}

/// Inscrit ce monde dans la liste du serveur de connexion d'après la configuration :
/// `WorldServer.Name`, `WorldServer.Group` et `WorldServer.PublicAddress`:`WorldServer.Port`.
/// La ligne écrite par `db-init` gardait `127.0.0.1:11008` quelle que soit la configuration, si
/// bien qu'un monde sur un autre port ou une autre machine renvoyait les clients au mauvais endroit.
async fn register_world(db: &crate::db::Databases, w: &crate::config::WorldConfig) -> anyhow::Result<()> {
    let address = format!("{}:{}", w.public_address.trim(), w.port);
    // Le paquet de connexion porte l'adresse sur 23 octets au plus (colonne `world.address`).
    if address.len() > 23 {
        anyhow::bail!("WorldServer.PublicAddress : « {address} » dépasse 23 caractères");
    }
    let auth = &db.auth;
    let group_id: u32 = match auth.first("SELECT id FROM server_group WHERE name = ?", (w.group.clone(),)).await? {
        Some(r) => crate::db::col(&r, "id"),
        None => {
            let id: u32 = auth.first("SELECT COALESCE(MAX(id), 0) + 1 AS id FROM server_group", ()).await?.map(|r| crate::db::col(&r, "id")).unwrap_or(1);
            auth.exec("INSERT INTO server_group (id, name) VALUES (?, ?)", (id, w.group.clone())).await?;
            id
        }
    };
    if auth.first("SELECT id FROM world WHERE name = ?", (w.name.clone(),)).await?.is_some() {
        auth.exec("UPDATE world SET group_id = ?, address = ? WHERE name = ?", (group_id, address.clone(), w.name.clone())).await?;
    } else {
        let id: u32 = auth.first("SELECT COALESCE(MAX(id), 0) + 1 AS id FROM world", ()).await?.map(|r| crate::db::col(&r, "id")).unwrap_or(1);
        auth.exec("INSERT INTO world (id, group_id, name, address) VALUES (?, ?, ?, ?)", (id, group_id, w.name.clone(), address.clone())).await?;
    }
    info!("[world] inscrit dans la liste des mondes : {} ({}) à {address}", w.name, w.group);
    Ok(())
}
