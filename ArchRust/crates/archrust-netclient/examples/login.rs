//! Rejoue le flux complet d'un client contre ArchCore (login puis monde), sans affichage.
//! `cargo run -p archrust-netclient --example login -- [utilisateur] [motdepasse] [personnage] [hôte]`
use archrust_netclient::{LoginClient, WorldClient, WorldEvent};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args: Vec<String> = std::env::args().collect();
    let user = args.get(1).map(String::as_str).unwrap_or("test");
    let pass = args.get(2).map(String::as_str).unwrap_or("test");
    let name = args.get(3).map(String::as_str).unwrap_or("Testeur");
    let host = args.get(4).map(String::as_str).unwrap_or("127.0.0.1");

    let mut login = LoginClient::connect(host, 11002).await?;
    println!("mondes : {:?}", login.worlds);
    login.authenticate(user, pass).await?;
    let world = login
        .worlds
        .first()
        .map(|w| w.name.clone())
        .unwrap_or_else(|| "ArchCore".into());
    let names = login.select_world(&world).await?;
    println!("personnages : {names:?}");
    for c in login.list_characters().await? {
        println!(
            "  {} (cid {}, tid {}, niveau {})",
            c.name, c.cid, c.tid, c.level
        );
    }
    if !names.iter().any(|n| n == name) {
        let bases = login.enter_creation().await?;
        println!("écran de création : {bases} modèles de base");
        let cid = login.create_character(name, 96, 1, 2).await?;
        println!("personnage {name} créé, cid {cid}");
    }
    let ticket = login.enter_world(name, &world).await?;
    login.close().await;
    println!("billet : {ticket:?}");

    let mut world = WorldClient::connect(ticket).await?;
    let events = world.enter().await?;
    let seen = events
        .iter()
        .filter(|e| matches!(e, WorldEvent::CharacterSeen(_)))
        .count();
    println!(
        "en jeu : cid {}, position {:?}, {} personnages en vue",
        world.cid, world.position, seen
    );
    for e in events.iter().take(12) {
        println!("  {e:?}");
    }
    println!("flux complet OK");
    Ok(())
}
