//! Portail et page de banque contre ArchCore, sans affichage : trajet payant d'Anchorville au
//! Repaire de l'éclaireur (정찰자의 터, 100 pièces), puis achat d'une page de banque chez Holly.
//!
//! ```text
//! cargo run -p archrust-netclient --example portal -- [compte mdp perso] [hôte[:port]]
//! ```
//! Par défaut : test/test et le premier personnage du compte, sur 127.0.0.1:11102 (instance de
//! test). Il faut 1 000 100 pièces sur le personnage (trajet et page) : s'il en manque, l'essai les
//! demande par la commande MJ `/money`, qui ne marche que pour un compte MJ (gmtest).
//!
//! Prix lus dans le client : `TeleportFee.txt` (앵커빌 마을 → 정찰자의 터 : 100) et
//! `bankslotprice.txt` (deuxième page : 1 000 000, les suivantes -1 : pas en vente contre de l'or).

use archrust_netclient::npc::{function, NpcCommand, NpcEvent, NpcFunction};
use archrust_netclient::teleport::{TeleportCommand, TeleportEvent};
use archrust_netclient::{Command, LoginClient, WorldClient, WorldEvent};
use glam::Vec3;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{timeout, Instant};

/// Portail d'Anchorville (objet 132646586 de `obj02024.ini`, point « 앵커빌 마을 »).
const ANCHORVILLE: (u32, Vec3) = (132_646_586, Vec3::new(-475_600.0, 4_516.0, -63_400.0));
/// Portail du Repaire de l'éclaireur (objet 126091730, point « 정찰자의 터 »).
const SCOUT: (u32, Vec3) = (126_091_730, Vec3::new(-531_600.0, 4_872.0, -33_600.0));
const HOLLY: Vec3 = Vec3::new(-480_400.0, 4_920.0, -69_100.0);
/// Codes AGPMSYSTEMMESSAGE de la banque.
const BANK_SLOT_IS_MAX: i32 = 29;
const BANK_ADD_SLOT_SUCCESS: i32 = 30;

struct Session {
    commands: mpsc::Sender<Command>,
    events: mpsc::Receiver<WorldEvent>,
    cid: i32,
    npcs: HashMap<i32, (String, Vec<NpcFunction>)>,
    money: Option<i64>,
    bank_pages: Option<i8>,
    position: Option<Vec3>,
}

impl Session {
    async fn send(&self, c: Command) {
        self.commands.send(c).await.expect("liaison fermée");
    }

    fn track(&mut self, ev: &WorldEvent) {
        match ev {
            WorldEvent::CharacterSeen(c) if !c.npc_functions.is_empty() => {
                self.npcs.insert(c.cid, (c.name.clone(), c.npc_functions.clone()));
            }
            WorldEvent::CharacterUpdated { stats: Some(s), .. } => {
                self.money = s.money.or(self.money);
                self.bank_pages = s.bank_pages.or(self.bank_pages);
            }
            WorldEvent::SelfCharacter { stats, .. } => {
                if let Some(s) = stats {
                    self.money = s.money.or(self.money);
                    self.bank_pages = s.bank_pages.or(self.bank_pages);
                }
            }
            WorldEvent::CharacterMoved { cid, position, .. } if *cid == self.cid => self.position = Some(*position),
            WorldEvent::SystemMessage(m) => println!("  (message système : {m})"),
            WorldEvent::Disconnected(why) => panic!("déconnecté : {why}"),
            _ => {}
        }
    }

    async fn next(&mut self, wait: Duration) -> Option<WorldEvent> {
        let ev = timeout(wait, self.events.recv()).await.ok()??;
        self.track(&ev);
        Some(ev)
    }

    async fn expect(&mut self, what: &str, secs: u64, mut want: impl FnMut(&WorldEvent) -> bool) -> anyhow::Result<WorldEvent> {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            if let Some(ev) = self.next(left).await {
                if want(&ev) {
                    return Ok(ev);
                }
            }
        }
        anyhow::bail!("{what} : rien reçu en {secs} s")
    }

    async fn drain(&mut self, ms: u64) {
        while self.next(Duration::from_millis(ms)).await.is_some() {}
    }

    async fn go(&mut self, to: Vec3) {
        self.send(Command::Stop { position: to }).await;
        self.position = Some(to);
        self.drain(1200).await;
    }

    /// Au moins `amount` pièces sur soi ; sinon `/money` (compte MJ seulement).
    async fn need_money(&mut self, amount: i64) -> anyhow::Result<i64> {
        if self.money.unwrap_or(0) < amount {
            self.send(Command::Say(format!("/money {amount}"))).await;
            self.expect(&format!("{amount} pièces (compte MJ requis pour /money)"), 4, |e| {
                matches!(e, WorldEvent::CharacterUpdated { stats: Some(s), .. } if s.money == Some(amount))
            })
            .await?;
        }
        Ok(self.money.unwrap_or(0))
    }
}

async fn enter(host: &str, port: u16, user: &str, pass: &str, name: Option<&str>) -> anyhow::Result<Session> {
    let mut login = LoginClient::connect(host, port).await?;
    login.authenticate(user, pass).await?;
    let world = login.worlds.first().map(|w| w.name.clone()).unwrap_or_else(|| "ArchCore".into());
    let names = login.select_world(&world).await?;
    let name = match name {
        Some(n) => names.iter().find(|x| x.eq_ignore_ascii_case(n)).cloned().ok_or_else(|| anyhow::anyhow!("{n} absent du compte {user}"))?,
        None => names.first().cloned().ok_or_else(|| anyhow::anyhow!("aucun personnage sur le compte {user}"))?,
    };
    let ticket = login.enter_world(&name, &world).await?;
    login.close().await;
    let mut wc = WorldClient::connect(ticket).await?;
    let early = wc.enter().await?;
    let cid = wc.cid;
    let (etx, erx) = mpsc::channel(512);
    let (ctx, crx) = mpsc::channel(64);
    tokio::spawn(wc.run(etx, crx));
    println!("{name} en jeu (cid {cid})");
    let mut s = Session { commands: ctx, events: erx, cid, npcs: HashMap::new(), money: None, bank_pages: None, position: None };
    for ev in &early {
        s.track(ev);
    }
    Ok(s)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let hostport = args.iter().find(|a| a.contains(':') || a.contains('.')).cloned().unwrap_or_else(|| "127.0.0.1:11102".into());
    let rest: Vec<&String> = args.iter().filter(|a| **a != hostport).collect();
    let (user, pass) = (rest.first().map_or("test", |s| s.as_str()), rest.get(1).map_or("test", |s| s.as_str()));
    let name = rest.get(2).map(|s| s.as_str());
    let (host, port) = hostport.split_once(':').map_or((hostport.as_str(), 11102), |(h, p)| (h, p.parse().unwrap_or(11102)));
    let mut s = enter(host, port, user, pass, name).await?;

    println!("\n▶ portail d'Anchorville, demandé à 2500 unités : le serveur fait courir le joueur");
    s.go(ANCHORVILLE.1 - Vec3::new(2_500.0, 0.0, 0.0)).await;
    let money0 = s.need_money(1_000_100).await?;
    println!("  {money0} pièces sur soi");
    s.send(Command::Teleport(TeleportCommand::Request(ANCHORVILLE.0))).await;
    let ev = s.expect("accord", 20, |e| matches!(e, WorldEvent::Teleport(TeleportEvent::Granted { .. }))).await?;
    anyhow::ensure!(matches!(ev, WorldEvent::Teleport(TeleportEvent::Granted { object }) if object == ANCHORVILLE.0), "accord : {ev:?}");
    println!("  ✓ accord (op 5) pour l'objet {}", ANCHORVILLE.0);

    println!("\n▶ trajet vers 정찰자의 터 (TeleportFee.txt : 100 pièces)");
    s.send(Command::Teleport(TeleportCommand::Go { object: ANCHORVILLE.0, point: "정찰자의 터".into() })).await;
    s.expect("paiement", 4, |e| matches!(e, WorldEvent::CharacterUpdated { stats: Some(st), .. } if st.money.is_some())).await?;
    anyhow::ensure!(s.money == Some(money0 - 100), "argent après le trajet : {:?}", s.money);
    let ev = s.expect("départ", 4, |e| matches!(e, WorldEvent::Teleport(TeleportEvent::Start { .. }))).await?;
    let WorldEvent::Teleport(TeleportEvent::Start { cid, position: Some(at) }) = ev else { anyhow::bail!("départ sans position : {ev:?}") };
    anyhow::ensure!(cid == s.cid, "départ pour le CID {cid}");
    let at = Vec3::from_array(at);
    let d = Vec3::new(at.x - SCOUT.1.x, 0.0, at.z - SCOUT.1.z).length();
    anyhow::ensure!(d <= 1_601.0, "arrivée à {d:.0} unités du portail d'arrivée");
    s.expect("chargement terminé", 4, |e| matches!(e, WorldEvent::Teleport(TeleportEvent::Loaded { .. }))).await?;
    println!("  ✓ départ (op 1) vers ({:.0}, {:.0}, {:.0}), à {d:.0} unités du portail ; chargement fini (op 8)", at.x, at.y, at.z);
    println!("  ✓ 100 pièces payées : {} restantes", money0 - 100);

    println!("\n▶ page de banque chez Holly");
    s.go(HOLLY).await;
    let holly = s
        .npcs
        .iter()
        .find(|(_, (n, fs))| n.ends_with(" Holly") && fs.iter().any(|f| f.function == function::BANK))
        .map(|(cid, _)| *cid)
        .ok_or_else(|| anyhow::anyhow!("Holly introuvable"))?;
    s.send(Command::Npc(NpcCommand::BankOpen(holly))).await;
    s.expect("banque ouverte", 4, |e| matches!(e, WorldEvent::Npc(NpcEvent::BankOpened { .. }))).await?;
    let pages0 = s.bank_pages.unwrap_or(0);
    println!("  ✓ banque ouverte ; {pages0} page(s) en plus");
    let money1 = s.need_money(1_000_000).await?;
    s.send(Command::Npc(NpcCommand::BuyBankSlot)).await;
    let code = s.expect("message système", 4, |e| matches!(e, WorldEvent::SystemCode(_))).await?;
    match code {
        WorldEvent::SystemCode(BANK_ADD_SLOT_SUCCESS) => {
            s.drain(400).await;
            anyhow::ensure!(s.bank_pages == Some(pages0 + 1) && s.money == Some(money1 - 1_000_000), "pages {:?}, argent {:?}", s.bank_pages, s.money);
            println!("  ✓ page achetée 1 000 000 pièces (code 30) : {} page(s) en plus, {} pièces restantes", pages0 + 1, money1 - 1_000_000);
        }
        WorldEvent::SystemCode(BANK_SLOT_IS_MAX) => println!("  · banque déjà au maximum (code 29)"),
        other => anyhow::bail!("réponse inattendue : {other:?}"),
    }
    // La page suivante (bankslotprice.txt : -1) ne s'achète pas contre de l'or : le client ne
    // propose pas l'achat (IsActiveBuyBankSlot) ; le serveur refuse aussi.
    s.send(Command::Npc(NpcCommand::BuyBankSlot)).await;
    let refusal = s.expect("refus", 4, |e| matches!(e, WorldEvent::SystemMessage(_) | WorldEvent::SystemCode(_) | WorldEvent::Chat { .. })).await?;
    println!("  ✓ page suivante refusée ({refusal:?})");
    s.drain(300).await;
    println!("\nportail et banque : OK");
    Ok(())
}
