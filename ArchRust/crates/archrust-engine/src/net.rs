//! Liaison réseau : fait tourner `archrust-netclient` sur un fil tokio et échange avec la boucle
//! de rendu par canaux.

use archrust_netclient::{Command, LoginClient, WorldClient, WorldEvent};
use std::sync::mpsc as std_mpsc;
use tokio::sync::mpsc;

/// Identifiants saisis à l'écran de connexion.
#[derive(Debug, Clone)]
pub struct Credentials {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub character: String,
    pub create_if_missing: bool,
}

/// Événements de la liaison vers le moteur.
#[derive(Debug)]
pub enum NetEvent {
    Status(String),
    Error(String),
    Worlds(Vec<String>),
    Characters(Vec<archrust_netclient::CharacterSummary>),
    World(WorldEvent),
}

pub struct NetLink {
    pub events: std_mpsc::Receiver<NetEvent>,
    pub commands: mpsc::Sender<Command>,
    _thread: std::thread::JoinHandle<()>,
}

impl NetLink {
    /// Lance la connexion complète (login puis monde) sur un fil dédié.
    pub fn start(creds: Credentials) -> Self {
        let (ev_tx, ev_rx) = std_mpsc::channel();
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);
        let thread = std::thread::Builder::new()
            .name("archrust-net".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                    Ok(rt) => rt,
                    Err(e) => {
                        let _ = ev_tx.send(NetEvent::Error(format!("tokio : {e}")));
                        return;
                    }
                };
                rt.block_on(async move {
                    if let Err(e) = session(creds, &ev_tx, cmd_rx).await {
                        let _ = ev_tx.send(NetEvent::Error(e.to_string()));
                    }
                });
            })
            .expect("fil réseau");
        NetLink { events: ev_rx, commands: cmd_tx, _thread: thread }
    }

    pub fn send(&self, cmd: Command) {
        let _ = self.commands.try_send(cmd);
    }
}

async fn session(creds: Credentials, ev: &std_mpsc::Sender<NetEvent>, cmd_rx: mpsc::Receiver<Command>) -> anyhow::Result<()> {
    let status = |s: String| {
        let _ = ev.send(NetEvent::Status(s));
    };
    status(format!("connexion à {}:{}…", creds.host, creds.port));
    let mut login = LoginClient::connect(&creds.host, creds.port).await?;
    let _ = ev.send(NetEvent::Worlds(login.worlds.iter().map(|w| w.name.clone()).collect()));
    status("authentification…".into());
    login.authenticate(&creds.user, &creds.password).await?;
    let world = login.worlds.first().map(|w| w.name.clone()).unwrap_or_else(|| "ArchCore".into());
    let names = login.select_world(&world).await?;
    let chars = login.list_characters().await?;
    let _ = ev.send(NetEvent::Characters(chars));
    if !names.iter().any(|n| n.eq_ignore_ascii_case(&creds.character)) {
        if !creds.create_if_missing {
            anyhow::bail!("personnage {} inconnu sur ce compte", creds.character);
        }
        status(format!("création de {}…", creds.character));
        login.enter_creation().await?;
        login.create_character(&creds.character, 96, 1, 2).await?;
    }
    status("entrée dans le monde…".into());
    let ticket = login.enter_world(&creds.character, &world).await?;
    login.close().await;
    let mut wc = WorldClient::connect(ticket).await?;
    for e in wc.enter().await? {
        let _ = ev.send(NetEvent::World(e));
    }
    status("en jeu".into());
    let (wtx, mut wrx) = mpsc::channel::<WorldEvent>(256);
    let forward = {
        let ev = ev.clone();
        async move {
            while let Some(e) = wrx.recv().await {
                if ev.send(NetEvent::World(e)).is_err() {
                    break;
                }
            }
        }
    };
    tokio::join!(wc.run(wtx, cmd_rx), forward);
    Ok(())
}
