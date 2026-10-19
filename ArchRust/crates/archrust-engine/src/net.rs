//! Liaison réseau : fait tourner `archrust-netclient` sur un fil tokio et échange avec la boucle
//! de rendu par canaux.

use archrust_netclient::{
    CharacterSummary, Command, LoginClient, NetError, WorldClient, WorldEvent,
};
use std::sync::mpsc as std_mpsc;
use tokio::sync::mpsc;

/// Identifiants saisis à l'écran de connexion.
#[derive(Debug, Clone)]
pub struct Credentials {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    /// Personnage à jouer directement (`--connect …/Nom`) ; `None` = choix à l'écran de
    /// sélection, après la liste des personnages du compte.
    pub character: Option<String>,
    pub create_if_missing: bool,
}

/// Commandes des écrans de sélection et de création, comme les boutons d'Archonia
/// (`LG_LoginMainSelectButtonClick`, `…CreateButtonClick`, `…DeleteButtonClick`,
/// `LG_CharacterCreateOK`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginCommand {
    /// Entrer dans le monde avec ce personnage du compte.
    Enter(String),
    /// Ouvrir l'écran de création : le serveur envoie ses modèles de base.
    OpenCreation,
    /// Créer un personnage depuis un modèle de base.
    Create {
        tid: i32,
        name: String,
        face: i32,
        hair: i32,
    },
    /// Supprimer un personnage du compte.
    Delete(String),
}

/// Événements de la liaison vers le moteur.
#[derive(Debug)]
pub enum NetEvent {
    Status(String),
    Error(String),
    Worlds(Vec<String>),
    Characters(Vec<CharacterSummary>),
    /// Modèles de base de l'écran de création.
    CreationTemplates(Vec<CharacterSummary>),
    CharacterCreated(CharacterSummary),
    CharacterDeleted(String),
    /// Refus du serveur sans fin de connexion (nom pris, création impossible…).
    Refused(String),
    World(WorldEvent),
}

pub struct NetLink {
    pub events: std_mpsc::Receiver<NetEvent>,
    pub commands: mpsc::Sender<Command>,
    choices: mpsc::Sender<LoginCommand>,
    _thread: std::thread::JoinHandle<()>,
}

impl NetLink {
    /// Lance la connexion complète (login puis monde) sur un fil dédié.
    pub fn start(creds: Credentials) -> Self {
        let (ev_tx, ev_rx) = std_mpsc::channel();
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);
        let (choice_tx, choice_rx) = mpsc::channel::<LoginCommand>(8);
        let thread = std::thread::Builder::new()
            .name("archrust-net".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        let _ = ev_tx.send(NetEvent::Error(format!("tokio : {e}")));
                        return;
                    }
                };
                rt.block_on(async move {
                    if let Err(e) = session(creds, &ev_tx, cmd_rx, choice_rx).await {
                        let _ = ev_tx.send(NetEvent::Error(e.to_string()));
                    }
                });
            })
            .expect("fil réseau");
        NetLink {
            events: ev_rx,
            commands: cmd_tx,
            choices: choice_tx,
            _thread: thread,
        }
    }

    pub fn send(&self, cmd: Command) {
        let _ = self.commands.try_send(cmd);
    }

    /// Transmet une commande des écrans de sélection et de création.
    pub fn choose(&self, choice: LoginCommand) {
        let _ = self.choices.try_send(choice);
    }
}

async fn session(
    creds: Credentials,
    ev: &std_mpsc::Sender<NetEvent>,
    cmd_rx: mpsc::Receiver<Command>,
    mut choice_rx: mpsc::Receiver<LoginCommand>,
) -> anyhow::Result<()> {
    let status = |s: String| {
        let _ = ev.send(NetEvent::Status(s));
    };
    status(format!("connexion à {}:{}…", creds.host, creds.port));
    let mut login = LoginClient::connect(&creds.host, creds.port).await?;
    let _ = ev.send(NetEvent::Worlds(
        login.worlds.iter().map(|w| w.name.clone()).collect(),
    ));
    status("authentification…".into());
    login.authenticate(&creds.user, &creds.password).await?;
    let world = login
        .worlds
        .first()
        .map(|w| w.name.clone())
        .unwrap_or_else(|| "ArchCore".into());
    let names = login.select_world(&world).await?;
    let chars = login.list_characters().await?;
    let _ = ev.send(NetEvent::Characters(chars));
    let known = |name: &str| names.iter().any(|n| n.eq_ignore_ascii_case(name));
    let character = match &creds.character {
        // Entrée directe (`--connect`) : création (humain, TID 96) si le personnage manque.
        Some(name) if known(name) => name.clone(),
        Some(name) if creds.create_if_missing => {
            status(format!("création de {name}…"));
            login.enter_creation().await?;
            login.create_character(name, 96, 1, 2).await?;
            name.clone()
        }
        Some(name) => anyhow::bail!("personnage {name} inconnu sur ce compte"),
        // Écrans de sélection et de création : on suit les commandes jusqu'à l'entrée en jeu.
        None => loop {
            let cmd = choice_rx
                .recv()
                .await
                .ok_or_else(|| anyhow::anyhow!("connexion annulée"))?;
            match cmd {
                LoginCommand::Enter(name) => break name,
                LoginCommand::OpenCreation => {
                    let bases = login.enter_creation().await?;
                    let _ = ev.send(NetEvent::CreationTemplates(bases));
                }
                LoginCommand::Create {
                    tid,
                    name,
                    face,
                    hair,
                } => match login.create_character(&name, tid, hair, face).await {
                    Ok(created) => {
                        let _ = ev.send(NetEvent::CharacterCreated(created));
                    }
                    Err(NetError::Server(msg)) => {
                        let _ = ev.send(NetEvent::Refused(format!("{name} : {msg}")));
                    }
                    Err(e) => return Err(e.into()),
                },
                LoginCommand::Delete(name) => {
                    login.delete_character(&name).await?;
                    let _ = ev.send(NetEvent::CharacterDeleted(name));
                }
            }
        },
    };
    status("entrée dans le monde…".into());
    let ticket = login.enter_world(&character, &world).await?;
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
