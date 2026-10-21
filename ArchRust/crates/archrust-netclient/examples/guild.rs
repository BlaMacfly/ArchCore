//! Guildes et discussion de guilde et de groupe contre ArchCore, sans affichage.
//!
//! ```text
//! cargo run -p archrust-netclient --example guild -- duo [hôte[:port]]
//!     GmTest (compte MJ gmtest) et SimA : gestionnaire de guilde (Mohol, rejoint par /move), création,
//!     invitation acceptée, discussion de guilde, groupe et discussion de groupe, annonce, demande de
//!     départ acceptée par le chef, retour puis exclusion, dissolution (mauvais mot de passe d'abord).
//!     L'état (`GuildState`) est vérifié chez les deux à chaque étape.
//! cargo run -p archrust-netclient --example guild -- partenaire [compte mdp perso] [hôte[:port]]
//!     Un joueur qui accepte toute invitation de guilde ou de groupe, répond « reçu » sur le canal
//!     où on lui parle et journalise sa guilde (pour essayer le client graphique).
//! cargo run -p archrust-netclient --example guild -- chef [invité] [hôte[:port]]
//!     GmTest crée RustGuild, invite ce joueur (SimA par défaut), répond « reçu » sur le canal de
//!     guilde, accepte sa demande de départ puis dissout la guilde (côté invité du client graphique).
//! ```
//! Les personnages sont créés au besoin (humain, TID 96).

use archrust_netclient::guild::{self, GuildCommand, GuildEvent, GuildNotice, GuildState};
use archrust_netclient::party::{PartyCommand, PartyNotice, PartyState};
use archrust_netclient::{Command, LoginClient, WorldClient, WorldEvent};
use glam::Vec3;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;

const ANCHORVILLE: Vec3 = Vec3::new(-476_000.0, 4_422.0, -61_000.0);
/// Gestionnaire de guilde Mohol (npc_spawn 45).
const MOHOL: (f32, f32) = (-341_649.0, 87_452.0);
const NAME: &str = "RustGuild";
const PASSWORD: &str = "rustpass";

/// Ce qu'un joueur a reçu, pour les attentes.
#[derive(Debug, Clone)]
enum Seen {
    Guild(GuildNotice),
    Raw(GuildEvent),
    Party(PartyNotice),
    Chat { kind: i8, from: String, message: String },
    Npc { cid: i32, functions: Vec<i32> },
}

struct Player {
    name: String,
    cid: i32,
    commands: mpsc::Sender<Command>,
    events: mpsc::Receiver<WorldEvent>,
    guild: GuildState,
    party: PartyState,
}

async fn enter(host: &str, user: &str, pass: &str, name: &str) -> anyhow::Result<Player> {
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
    let (etx, erx) = mpsc::channel(512);
    let (ctx, crx) = mpsc::channel(64);
    tokio::spawn(wc.run(etx, crx));
    println!("{name} en jeu (cid {cid})");
    Ok(Player { name: name.into(), cid, commands: ctx, events: erx, guild: GuildState::default(), party: PartyState::default() })
}

impl Player {
    async fn cmd(&self, c: Command) {
        println!("  {} → {c:?}", self.name);
        self.commands.send(c).await.expect("liaison fermée");
    }

    async fn guild_cmd(&self, c: GuildCommand) {
        self.cmd(Command::Guild(c)).await;
    }

    /// Applique un événement ; rend ce qui a été vu.
    fn apply(&mut self, ev: WorldEvent) -> Vec<Seen> {
        let mut out = Vec::new();
        match ev {
            WorldEvent::Guild(g) => {
                if let Some(n) = self.guild.apply(&g, &self.name, self.cid) {
                    out.push(Seen::Guild(n));
                }
                out.push(Seen::Raw(g));
            }
            WorldEvent::Party(p) => out.extend(self.party.apply_party(&p, self.cid).map(Seen::Party)),
            WorldEvent::PartyManagement(m) => out.extend(self.party.apply_management(&m).map(Seen::Party)),
            WorldEvent::Chat { channel, from, message, .. } => out.push(Seen::Chat { kind: channel, from, message }),
            WorldEvent::CharacterSeen(c) if !c.npc_functions.is_empty() => {
                out.push(Seen::Npc { cid: c.cid, functions: c.npc_functions.iter().map(|f| f.function).collect() })
            }
            WorldEvent::Disconnected(e) => println!("  {} déconnecté : {e}", self.name),
            _ => {}
        }
        out
    }

    fn log(&self, s: &Seen) {
        match s {
            Seen::Raw(GuildEvent::System { code, text, number, .. }) => println!("  {} ← message de guilde {code} ({text:?}, {number})", self.name),
            Seen::Raw(GuildEvent::Invited { guild, from }) => println!("  {} ← invitation dans {guild} par {from}", self.name),
            Seen::Raw(GuildEvent::ManagerGranted { npc }) => println!("  {} ← gestionnaire {npc:#x} : accord", self.name),
            Seen::Guild(n) => println!("  {} ← {n:?}", self.name),
            Seen::Party(n) => println!("  {} ← {}", self.name, n.message(&self.party)),
            Seen::Chat { kind, from, message } => println!("  {} ← discussion {kind} [{from}] {message}", self.name),
            _ => {}
        }
    }

    /// Attend ce qui satisfait `want` (3 s au plus), en appliquant tout ce qui arrive.
    async fn expect(&mut self, what: &str, want: impl Fn(&Seen) -> bool) -> anyhow::Result<Seen> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            let ev = timeout(left, self.events.recv())
                .await
                .map_err(|_| anyhow::anyhow!("{} : {what} non reçu", self.name))?
                .ok_or_else(|| anyhow::anyhow!("{} déconnecté", self.name))?;
            for s in self.apply(ev) {
                self.log(&s);
                if want(&s) {
                    return Ok(s);
                }
            }
        }
    }

    async fn expect_system(&mut self, code: i32) -> anyhow::Result<Seen> {
        self.expect(&format!("message de guilde {code}"), |s| matches!(s, Seen::Raw(GuildEvent::System { code: c, .. }) if *c == code)).await
    }

    /// Applique ce qui arrive pendant `ms` millisecondes.
    async fn drain(&mut self, ms: u64) {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(ms);
        while let Ok(Some(ev)) = timeout(deadline.saturating_duration_since(tokio::time::Instant::now()), self.events.recv()).await {
            for s in self.apply(ev) {
                self.log(&s);
            }
        }
    }
}

fn step(s: &str) {
    println!("\n== {s}");
}

async fn duo(host: &str) -> anyhow::Result<()> {
    let mut a = enter(host, "gmtest", "gmtest", "GmTest").await?;
    let mut b = enter(host, "sima", "sima", "SimA").await?;
    a.drain(800).await;
    b.drain(800).await;

    // Restes d'un essai interrompu : SimA quitte sa guilde, GmTest dissout la sienne.
    if b.guild.in_guild() && b.guild.name() != a.guild.name() {
        anyhow::bail!("SimA est dans une autre guilde ({}) : à retirer à la main", b.guild.name());
    }
    if a.guild.in_guild() {
        step("reste d'un essai précédent : dissolution");
        a.guild_cmd(GuildCommand::Destroy { guild: a.guild.name().into(), password: PASSWORD.into() }).await;
        a.expect("dissolution", |s| matches!(s, Seen::Guild(GuildNotice::Left))).await?;
        b.drain(500).await;
    }
    anyhow::ensure!(!a.guild.in_guild() && !b.guild.in_guild(), "toujours en guilde");

    step("GmTest rejoint le gestionnaire de guilde Mohol");
    a.cmd(Command::Say("/level 20".into())).await;
    a.cmd(Command::Say("/money 20000".into())).await;
    a.cmd(Command::Say(format!("/move {:.0},{:.0}", MOHOL.0, MOHOL.1))).await;
    let npc = match a.expect("gestionnaire en vue", |s| matches!(s, Seen::Npc { functions, .. } if functions.contains(&guild::FUNCTION_GUILD))).await? {
        Seen::Npc { cid, .. } => cid,
        _ => unreachable!(),
    };
    a.drain(500).await;
    a.cmd(Command::Stop { position: Vec3::new(MOHOL.0 + 100.0, 8_873.0, MOHOL.1) }).await;
    a.guild_cmd(GuildCommand::TalkToManager(npc)).await;
    a.expect("accord du gestionnaire", |s| matches!(s, Seen::Raw(GuildEvent::ManagerGranted { npc: n }) if *n == npc)).await?;
    println!("  ✓ gestionnaire {npc:#x} : accord (la fenêtre de création s'ouvrirait)");

    step("création : nom refusé, puis RustGuild");
    a.guild_cmd(GuildCommand::Create { name: "Nom invalide".into(), password: PASSWORD.into() }).await;
    a.expect_system(guild::code::USE_SPECIAL_CHAR).await?;
    a.guild_cmd(GuildCommand::Create { name: NAME.into(), password: PASSWORD.into() }).await;
    a.expect("guilde", |s| matches!(s, Seen::Guild(GuildNotice::Joined))).await?;
    a.expect_system(guild::code::CREATE_COMPLETE).await?;
    a.drain(300).await;
    let info = a.guild.info.clone().unwrap();
    anyhow::ensure!(info.name == NAME && info.master == a.name && info.max_members == 50, "guilde {info:?}");
    anyhow::ensure!(a.guild.member(&a.name).is_some_and(|m| m.rank == guild::rank::MASTER), "chef absent de la liste");
    println!("  ✓ {NAME} créée, GmTest chef, 50 places");

    step("GmTest invite SimA, qui accepte");
    a.guild_cmd(GuildCommand::Invite { guild: NAME.into(), name: b.name.clone() }).await;
    b.expect("invitation", |s| matches!(s, Seen::Guild(GuildNotice::Invited(i)) if i.guild == NAME && i.from == "GmTest")).await?;
    b.guild_cmd(GuildCommand::Join { guild: NAME.into() }).await;
    b.expect("guilde reçue", |s| matches!(s, Seen::Guild(GuildNotice::Joined))).await?;
    a.expect_system(guild::code::JOIN).await?;
    a.drain(400).await;
    b.drain(400).await;
    for p in [&a, &b] {
        let m = p.guild.member("SimA");
        anyhow::ensure!(m.is_some_and(|m| m.rank == guild::rank::NORMAL && m.online), "{} : SimA {:?}", p.name, m);
    }
    anyhow::ensure!(b.guild.links.get(&b.cid).map(String::as_str) == Some(NAME), "lien de SimA");
    println!("  ✓ SimA membre (rang 1) chez les deux, lien reçu");

    step("discussion de guilde");
    b.cmd(Command::Chat { kind: 3, message: "Bonjour la guilde, ça va ?".into() }).await;
    let heard = a.expect("message de guilde", |s| matches!(s, Seen::Chat { kind: 3, .. })).await?;
    anyhow::ensure!(matches!(&heard, Seen::Chat { from, message, .. } if from == "SimA" && message == "Bonjour la guilde, ça va ?"), "{heard:?}");
    println!("  ✓ GmTest reçoit le message de guilde de SimA (accents compris)");

    step("groupe et discussion de groupe");
    a.cmd(Command::Party(PartyCommand::Invite { cid: None, name: Some(b.name.clone()) })).await;
    let inviter = match b.expect("invitation de groupe", |s| matches!(s, Seen::Party(PartyNotice::Invited(_)))).await? {
        Seen::Party(PartyNotice::Invited(i)) => i.cid,
        _ => unreachable!(),
    };
    b.cmd(Command::Party(PartyCommand::Accept(inviter))).await;
    b.expect("groupe", |s| matches!(s, Seen::Party(PartyNotice::Joined))).await?;
    a.drain(300).await;
    a.cmd(Command::Chat { kind: 4, message: "En route".into() }).await;
    let heard = b.expect("message de groupe", |s| matches!(s, Seen::Chat { kind: 4, .. })).await?;
    anyhow::ensure!(matches!(&heard, Seen::Chat { from, message, .. } if from == "GmTest" && message == "En route"), "{heard:?}");
    b.cmd(Command::Party(PartyCommand::Leave)).await;
    b.expect("groupe quitté", |s| matches!(s, Seen::Party(PartyNotice::Left))).await?;
    a.drain(300).await;
    println!("  ✓ SimA reçoit le message de groupe de GmTest");

    step("annonce");
    a.guild_cmd(GuildCommand::Notice { guild: NAME.into(), text: "Rendez-vous à Anchorville".into() }).await;
    b.expect("annonce", |s| matches!(s, Seen::Guild(GuildNotice::NoticeChanged))).await?;
    a.drain(300).await;
    for p in [&a, &b] {
        anyhow::ensure!(p.guild.info.as_ref().is_some_and(|i| i.notice == "Rendez-vous à Anchorville"), "{} : annonce", p.name);
    }
    println!("  ✓ annonce reçue chez les deux");

    step("SimA demande à partir, GmTest accepte");
    b.guild_cmd(GuildCommand::Leave { guild: NAME.into(), name: b.name.clone() }).await;
    b.expect_system(guild::code::LEAVE_REQUEST_SELF).await?;
    a.expect("demande de départ", |s| matches!(s, Seen::Guild(GuildNotice::Roster))).await?;
    a.drain(300).await;
    anyhow::ensure!(a.guild.member("SimA").is_some_and(|m| m.rank == guild::rank::LEAVE_REQUEST), "rang {:?}", a.guild.member("SimA"));
    a.guild_cmd(GuildCommand::AllowLeave { guild: NAME.into(), name: b.name.clone() }).await;
    b.expect("départ", |s| matches!(s, Seen::Guild(GuildNotice::Left))).await?;
    b.expect_system(guild::code::LEAVE).await?;
    a.drain(400).await;
    anyhow::ensure!(a.guild.member("SimA").is_none() && !b.guild.in_guild(), "SimA encore là");
    println!("  ✓ demande (rang 3) acceptée, SimA n'est plus membre");

    step("SimA revient puis est exclue");
    a.guild_cmd(GuildCommand::Invite { guild: NAME.into(), name: b.name.clone() }).await;
    b.expect("invitation", |s| matches!(s, Seen::Guild(GuildNotice::Invited(_)))).await?;
    b.guild_cmd(GuildCommand::Join { guild: NAME.into() }).await;
    b.expect("guilde reçue", |s| matches!(s, Seen::Guild(GuildNotice::Joined))).await?;
    a.expect_system(guild::code::JOIN).await?;
    b.drain(300).await;
    a.guild_cmd(GuildCommand::Kick { guild: NAME.into(), name: b.name.clone() }).await;
    b.expect("exclusion", |s| matches!(s, Seen::Raw(GuildEvent::Left { forced: true, .. }))).await?;
    b.expect_system(guild::code::FORCED_LEAVE).await?;
    a.drain(400).await;
    anyhow::ensure!(a.guild.member("SimA").is_none() && !b.guild.in_guild(), "exclusion incomplète");
    println!("  ✓ SimA exclue");

    step("dissolution");
    a.guild_cmd(GuildCommand::Destroy { guild: NAME.into(), password: "mauvais".into() }).await;
    a.expect_system(guild::code::INVALID_PASSWORD).await?;
    a.guild_cmd(GuildCommand::Destroy { guild: NAME.into(), password: PASSWORD.into() }).await;
    a.expect("dissolution", |s| matches!(s, Seen::Guild(GuildNotice::Left))).await?;
    a.expect_system(guild::code::DESTROY).await?;
    anyhow::ensure!(!a.guild.in_guild() && a.guild.links.is_empty(), "guilde encore là");
    println!("  ✓ mauvais mot de passe refusé, guilde dissoute");

    a.cmd(Command::Say(format!("/move {:.0},{:.0}", ANCHORVILLE.x, ANCHORVILLE.z))).await;
    a.drain(500).await;
    a.cmd(Command::Quit).await;
    b.cmd(Command::Quit).await;
    println!("\nguilde : OK");
    Ok(())
}

/// Chef pour le client graphique : GmTest crée RustGuild, invite `invite`, répond sur le canal de guilde,
/// accepte sa demande de départ, puis dissout la guilde et s'arrête.
async fn chief(host: &str, invite: &str) -> anyhow::Result<()> {
    let mut a = enter(host, "gmtest", "gmtest", "GmTest").await?;
    a.drain(800).await;
    if a.guild.in_guild() {
        a.guild_cmd(GuildCommand::Destroy { guild: a.guild.name().into(), password: PASSWORD.into() }).await;
        a.drain(800).await;
    }
    a.cmd(Command::Say("/level 20".into())).await;
    a.cmd(Command::Say("/money 20000".into())).await;
    a.drain(800).await;
    a.guild_cmd(GuildCommand::Create { name: NAME.into(), password: PASSWORD.into() }).await;
    a.expect("guilde", |s| matches!(s, Seen::Guild(GuildNotice::Joined))).await?;
    a.guild_cmd(GuildCommand::Notice { guild: NAME.into(), text: "Bienvenue dans RustGuild".into() }).await;
    let mut invited = false;
    let mut joined = false;
    loop {
        if !invited {
            a.drain(2000).await;
            a.guild_cmd(GuildCommand::Invite { guild: NAME.into(), name: invite.into() }).await;
            invited = true;
            continue;
        }
        let Some(ev) = a.events.recv().await else { return Ok(()) };
        for s in a.apply(ev) {
            a.log(&s);
            match s {
                Seen::Chat { kind: 3, from, .. } if from != a.name => {
                    a.cmd(Command::Chat { kind: 3, message: format!("reçu, {from}") }).await;
                }
                Seen::Raw(GuildEvent::System { code, .. }) if code == guild::code::JOIN => joined = true,
                Seen::Raw(GuildEvent::Member { member, .. }) if member.name.eq_ignore_ascii_case(invite) && member.rank == guild::rank::LEAVE_REQUEST => {
                    tokio::time::sleep(Duration::from_millis(2500)).await;
                    a.guild_cmd(GuildCommand::AllowLeave { guild: NAME.into(), name: member.name }).await;
                }
                Seen::Raw(GuildEvent::Left { name, .. }) if joined && name.eq_ignore_ascii_case(invite) => {
                    a.drain(3000).await;
                    a.guild_cmd(GuildCommand::Destroy { guild: NAME.into(), password: PASSWORD.into() }).await;
                    a.drain(1000).await;
                    println!("chef : guilde dissoute, fin");
                    return Ok(());
                }
                _ => {}
            }
        }
    }
}

/// Partenaire pour le client graphique : accepte les invitations et répond sur le canal reçu.
async fn partner(host: &str, user: &str, pass: &str, name: &str) -> anyhow::Result<()> {
    let mut p = enter(host, user, pass, name).await?;
    loop {
        let Some(ev) = p.events.recv().await else { return Ok(()) };
        for s in p.apply(ev) {
            p.log(&s);
            match s {
                Seen::Guild(GuildNotice::Invited(i)) => {
                    tokio::time::sleep(Duration::from_millis(1500)).await;
                    p.guild_cmd(GuildCommand::Join { guild: i.guild }).await;
                }
                Seen::Party(PartyNotice::Invited(i)) => {
                    tokio::time::sleep(Duration::from_millis(1500)).await;
                    p.cmd(Command::Party(PartyCommand::Accept(i.cid))).await;
                }
                Seen::Chat { kind: kind @ (3 | 4), from, .. } if from != p.name => {
                    p.cmd(Command::Chat { kind, message: format!("reçu, {from}") }).await;
                }
                _ => {}
            }
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let host = args.iter().skip(1).find(|a| a.contains('.') || a.contains(':')).cloned().unwrap_or_else(|| "127.0.0.1".into());
    match args.first().map(String::as_str) {
        Some("duo") => duo(&host).await,
        Some("partenaire") => {
            let rest: Vec<&String> = args.iter().skip(1).filter(|a| !(a.contains('.') || a.contains(':'))).collect();
            let get = |i: usize, d: &str| rest.get(i).map(|s| s.to_string()).unwrap_or_else(|| d.to_string());
            partner(&host, &get(0, "sima"), &get(1, "sima"), &get(2, "SimA")).await
        }
        Some("chef") => {
            let who = args.iter().skip(1).find(|a| !(a.contains('.') || a.contains(':'))).cloned().unwrap_or_else(|| "SimA".into());
            chief(&host, &who).await
        }
        _ => anyhow::bail!("usage : guild duo|partenaire [compte mdp perso]|chef [invité] [hôte[:port]]"),
    }
}
