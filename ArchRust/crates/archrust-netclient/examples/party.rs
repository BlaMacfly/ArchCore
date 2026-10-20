//! Groupes contre ArchCore, sans affichage.
//!
//! ```text
//! cargo run -p archrust-netclient --example party -- duo [hôte[:port]]
//!     SimA et SimB : invitation par nom inconnu, puis par nom, acceptation, réglage, passage de chef,
//!     exclusion, nouveau groupe et départ ; l'état (`PartyState`) est vérifié à chaque étape.
//! cargo run -p archrust-netclient --example party -- partenaire [compte mdp perso] [--banish-when-leader] [--inviter=NOM] [--marcher] [hôte[:port]]
//!     Un joueur qui accepte toute invitation et journalise son groupe (pour essayer le client
//!     graphique) ; avec --banish-when-leader, exclut l'autre membre 3 s après être devenu chef.
//!     Avec --inviter=NOM, invite ce joueur 6 s après l'entrée en jeu. Avec --marcher, fait des
//!     allers-retours de 600 unités vers l'est toutes les 8 s, le premier 4 s après l'entrée (pour voir
//!     un autre joueur bouger).
//! ```
//! Les personnages sont créés au besoin (humain, TID 96) et placés à Anchorville.

use archrust_netclient::party::{self, PartyCommand, PartyNotice, PartyState};
use archrust_netclient::{Command, LoginClient, WorldClient, WorldEvent};
use glam::Vec3;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;

const SPOT: Vec3 = Vec3::new(-476_000.0, 4_422.0, -61_000.0);

/// Un joueur en jeu : commandes, événements et état du groupe.
struct Player {
    name: String,
    cid: i32,
    commands: mpsc::Sender<Command>,
    events: mpsc::Receiver<WorldEvent>,
    party: PartyState,
}

async fn enter(host: &str, user: &str, pass: &str, name: &str) -> anyhow::Result<Player> {
    // `hôte` ou `hôte:port` (11002 par défaut ; 11102 pour le serveur de test).
    let (host, port) = host.split_once(':').map_or((host, 11002), |(h, p)| (h, p.parse().unwrap_or(11002)));
    let mut login = LoginClient::connect(host, port).await?;
    login.authenticate(user, pass).await?;
    let world = login.worlds.first().map(|w| w.name.clone()).unwrap_or_else(|| "ArchCore".into());
    let names = login.select_world(&world).await?;
    if !names.iter().any(|n| n.eq_ignore_ascii_case(name)) {
        login.enter_creation().await?;
        login.create_character(name, 96, 1, 2).await?;
    }
    let ticket = login.enter_world(name, &world).await?;
    login.close().await;
    let mut wc = WorldClient::connect(ticket).await?;
    wc.enter().await?;
    let cid = wc.cid;
    wc.send_stop(SPOT).await?;
    let (etx, erx) = mpsc::channel(256);
    let (ctx, crx) = mpsc::channel(64);
    tokio::spawn(wc.run(etx, crx));
    println!("{name} en jeu (cid {cid})");
    Ok(Player { name: name.into(), cid, commands: ctx, events: erx, party: PartyState::default() })
}

impl Player {
    async fn send(&self, c: PartyCommand) {
        println!("  {} → {c:?}", self.name);
        self.commands.send(Command::Party(c)).await.expect("liaison fermée");
    }

    /// Applique un événement de groupe ; rend l'avis éventuel.
    fn apply(&mut self, ev: &WorldEvent) -> Option<PartyNotice> {
        match ev {
            WorldEvent::SelfCharacter { cid, name, .. } => {
                self.party.learn_name(*cid, name);
                None
            }
            WorldEvent::Party(p) => self.party.apply_party(p, self.cid),
            WorldEvent::PartyManagement(m) => self.party.apply_management(m),
            _ => None,
        }
    }

    /// Attend l'avis voulu (3 s au plus), en appliquant tout ce qui arrive.
    async fn expect(&mut self, what: &str, want: impl Fn(&PartyNotice) -> bool) -> anyhow::Result<PartyNotice> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            let ev = timeout(left, self.events.recv())
                .await
                .map_err(|_| anyhow::anyhow!("{} : {what} non reçu", self.name))?
                .ok_or_else(|| anyhow::anyhow!("{} déconnecté", self.name))?;
            if let Some(n) = self.apply(&ev) {
                println!("  {} ← {}", self.name, n.message(&self.party));
                if want(&n) {
                    return Ok(n);
                }
            }
        }
    }

    /// Applique ce qui est arrivé pendant `ms` millisecondes.
    async fn drain(&mut self, ms: u64) {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(ms);
        while let Ok(Some(ev)) = timeout(deadline.saturating_duration_since(tokio::time::Instant::now()), self.events.recv()).await {
            if let Some(n) = self.apply(&ev) {
                println!("  {} ← {}", self.name, n.message(&self.party));
            }
        }
    }
}

fn step(s: &str) {
    println!("\n== {s}");
}

async fn duo(host: &str) -> anyhow::Result<()> {
    let mut a = enter(host, "sima", "sima", "SimA").await?;
    let mut b = enter(host, "simb", "simb", "SimB").await?;
    let (acid, bcid) = (a.cid, b.cid);
    a.drain(500).await;
    b.drain(500).await;
    // Un reste de groupe d'un essai précédent ? Le serveur le défait à la déconnexion.
    anyhow::ensure!(!a.party.in_party() && !b.party.in_party(), "déjà en groupe");

    step("invitation d'un nom inconnu");
    a.send(PartyCommand::Invite { cid: None, name: Some("PersonneIci".into()) }).await;
    a.expect("échec 10", |n| matches!(n, PartyNotice::Failed { code: party::mgmt::FAILED_NO_LOGIN, .. })).await?;

    step("SimA invite SimB par son nom ; SimB accepte");
    a.send(PartyCommand::Invite { cid: None, name: Some(b.name.clone()) }).await;
    b.expect("invitation", |n| matches!(n, PartyNotice::Invited(i) if i.name == "SimA")).await?;
    let inviter = b.party.invitations.last().unwrap().cid;
    anyhow::ensure!(inviter == a.cid, "inviteur {inviter}");
    b.send(PartyCommand::Accept(inviter)).await;
    b.expect("groupe", |n| *n == PartyNotice::Joined).await?;
    a.expect("groupe", |n| *n == PartyNotice::Joined).await?;
    a.expect("acceptation", |n| matches!(n, PartyNotice::Accepted(name) if name == "SimB")).await?;
    b.drain(300).await;
    for p in [&a, &b] {
        anyhow::ensure!(p.party.members == vec![a.cid, b.cid], "{} : membres {:?}", p.name, p.party.members);
        anyhow::ensure!(p.party.is_leader(a.cid), "{} : chef", p.name);
    }
    println!("  ✓ membres {:?}, SimA chef, chez les deux", a.party.members);

    step("le chef règle l'expérience selon le niveau");
    a.send(PartyCommand::SetExpType(party::EXP_BY_LEVEL)).await;
    a.expect("réglage", |n| *n == PartyNotice::ExpTypeChanged(party::EXP_BY_LEVEL)).await?;
    b.expect("réglage", |n| *n == PartyNotice::ExpTypeChanged(party::EXP_BY_LEVEL)).await?;

    step("SimA passe la main à SimB");
    a.send(PartyCommand::Delegate(b.cid)).await;
    a.expect("chef", |n| *n == PartyNotice::LeaderChanged(bcid)).await?;
    b.expect("chef", |n| *n == PartyNotice::LeaderChanged(bcid)).await?;
    anyhow::ensure!(a.party.members == vec![b.cid, a.cid] && b.party.members == vec![b.cid, a.cid], "ordre après passage");
    println!("  ✓ SimB chef, en tête de liste chez les deux");

    step("SimB exclut SimA : groupe dissous");
    b.send(PartyCommand::Banish(a.cid)).await;
    a.expect("exclusion", |n| *n == PartyNotice::Banished).await?;
    b.expect("dissolution", |n| *n == PartyNotice::Left).await?;
    anyhow::ensure!(!a.party.in_party() && !b.party.in_party(), "groupe resté");
    println!("  ✓ plus de groupe chez les deux");

    step("SimB invite SimA (par CID), qui refuse, puis accepte une nouvelle invitation");
    b.send(PartyCommand::Invite { cid: Some(a.cid), name: None }).await;
    a.expect("invitation", |n| matches!(n, PartyNotice::Invited(_))).await?;
    a.send(PartyCommand::Reject(b.cid)).await;
    b.expect("refus", |n| matches!(n, PartyNotice::Rejected(name) if name == "SimA")).await?;
    b.send(PartyCommand::Invite { cid: Some(a.cid), name: None }).await;
    a.expect("invitation", |n| matches!(n, PartyNotice::Invited(_))).await?;
    a.send(PartyCommand::Accept(b.cid)).await;
    a.expect("groupe", |n| *n == PartyNotice::Joined).await?;
    b.expect("groupe", |n| *n == PartyNotice::Joined).await?;

    step("SimA quitte le groupe");
    a.send(PartyCommand::Leave).await;
    a.expect("départ", |n| *n == PartyNotice::Left).await?;
    b.expect("départ du membre", |n| *n == PartyNotice::MemberLeft(acid)).await?;
    b.expect("dissolution", |n| *n == PartyNotice::Left).await?;
    println!("\ngroupes avec archrust-netclient : OK");
    let _ = a.commands.send(Command::Quit).await;
    let _ = b.commands.send(Command::Quit).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    Ok(())
}

async fn partner(host: &str, user: &str, pass: &str, name: &str, banish_when_leader: bool, invite: Option<String>, walk: bool) -> anyhow::Result<()> {
    let mut p = enter(host, user, pass, name).await?;
    let mut banish_at: Option<tokio::time::Instant> = None;
    let mut invite_at = invite.map(|n| (tokio::time::Instant::now() + Duration::from_secs(6), n));
    let mut walk_at = walk.then(|| tokio::time::Instant::now() + Duration::from_secs(4));
    let mut at = SPOT;
    loop {
        if walk_at.is_some_and(|t| tokio::time::Instant::now() >= t) {
            walk_at = Some(tokio::time::Instant::now() + Duration::from_secs(8));
            let to = if at == SPOT { SPOT + Vec3::new(600.0, 0.0, 0.0) } else { SPOT };
            println!("[{name}] marche vers {:.0}, {:.0}", to.x, to.z);
            p.commands.send(Command::MoveTo { position: at, destination: to }).await?;
            at = to;
        }
        let ev = match timeout(Duration::from_millis(200), p.events.recv()).await {
            Ok(Some(ev)) => Some(ev),
            Ok(None) => anyhow::bail!("déconnecté"),
            Err(_) => None,
        };
        if let Some(ev) = ev {
            if let WorldEvent::Disconnected(why) = &ev {
                anyhow::bail!("déconnecté : {why}");
            }
            if let Some(n) = p.apply(&ev) {
                println!("[{name}] {} (membres {:?})", n.message(&p.party), p.party.members);
                match &n {
                    PartyNotice::Invited(i) => {
                        let cid = i.cid;
                        p.party.take_invitation(cid);
                        p.send(PartyCommand::Accept(cid)).await;
                    }
                    PartyNotice::LeaderChanged(c) if *c == p.cid && banish_when_leader => {
                        banish_at = Some(tokio::time::Instant::now() + Duration::from_secs(3));
                    }
                    _ => {}
                }
            }
        }
        if invite_at.as_ref().is_some_and(|(t, _)| tokio::time::Instant::now() >= *t) {
            let (_, n) = invite_at.take().unwrap();
            p.send(PartyCommand::Invite { cid: None, name: Some(n) }).await;
        }
        if banish_at.is_some_and(|t| tokio::time::Instant::now() >= t) {
            banish_at = None;
            if let Some(other) = p.party.members.iter().copied().find(|c| *c != p.cid) {
                p.send(PartyCommand::Banish(other)).await;
            }
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let banish = args.iter().any(|a| a == "--banish-when-leader");
    let invite = args.iter().find_map(|a| a.strip_prefix("--inviter=")).map(str::to_string);
    let rest: Vec<&str> = args.iter().map(String::as_str).filter(|a| !a.starts_with("--")).collect();
    match rest.first().copied() {
        Some("partenaire") => {
            let a = |i: usize, d: &'static str| rest.get(i).copied().unwrap_or(d);
            let walk = args.iter().any(|a| a == "--marcher");
            partner(a(4, "127.0.0.1"), a(1, "sima"), a(2, "sima"), a(3, "SimA"), banish, invite, walk).await
        }
        _ => duo(rest.get(1).copied().unwrap_or("127.0.0.1")).await,
    }
}
