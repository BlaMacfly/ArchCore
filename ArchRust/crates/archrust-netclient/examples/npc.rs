//! PNJ contre ArchCore, sans affichage : dialogue et boutique chez Darryl (achat, vente), banque chez
//! Holly (objet et argent déposés puis retirés), quête prise puis abandonnée.
//!
//! ```text
//! cargo run -p archrust-netclient --example npc -- [compte mdp perso] [hôte[:port]]
//! ```
//! Par défaut : gmtest/gmtest/GmTest (compte MJ des bases de test) sur 127.0.0.1:11102.

use archrust_netclient::npc::{function, trade_result, NpcCommand, NpcEvent, NpcFunction, STATUS_BANK};
use archrust_netclient::world::ItemRecord;
use archrust_netclient::{Command, LoginClient, WorldClient, WorldEvent};
use glam::Vec3;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{timeout, Instant};

const DARRYL: Vec3 = Vec3::new(-478_900.0, 4_753.0, -66_022.0);
const HOLLY: Vec3 = Vec3::new(-480_400.0, 4_920.0, -69_100.0);
const POTION: i32 = 190;
/// Gladiator's Demise : niveau 99 à 120, prise depuis le journal.
const QUEST: i32 = 4611;

struct Session {
    commands: mpsc::Sender<Command>,
    events: mpsc::Receiver<WorldEvent>,
    /// PNJ en vue : nom et fonctions.
    npcs: HashMap<i32, (String, Vec<NpcFunction>)>,
    items: HashMap<i32, ItemRecord>,
    money: Option<i64>,
    bank_money: Option<i64>,
}

impl Session {
    async fn send(&self, c: Command) {
        self.commands.send(c).await.expect("liaison fermée");
    }

    async fn npc(&self, c: NpcCommand) {
        self.send(Command::Npc(c)).await;
    }

    fn track(&mut self, ev: &WorldEvent) {
        match ev {
            WorldEvent::CharacterSeen(c) if !c.npc_functions.is_empty() => {
                self.npcs.insert(c.cid, (c.name.clone(), c.npc_functions.clone()));
            }
            WorldEvent::ItemsSnapshot(items) => {
                for i in items {
                    self.items.insert(i.id, i.clone());
                }
            }
            WorldEvent::ItemChanged(i) if i.operation == 1 => {
                self.items.remove(&i.id);
            }
            WorldEvent::ItemChanged(i) => {
                let cur = self.items.entry(i.id).or_insert_with(|| i.clone());
                if i.template_id.is_some() {
                    cur.template_id = i.template_id;
                }
                if i.status.is_some() {
                    cur.status = i.status;
                }
                if i.grid.is_some() {
                    cur.grid = i.grid;
                }
                if i.count.is_some() {
                    cur.count = i.count;
                }
            }
            WorldEvent::CharacterUpdated { stats: Some(s), .. } => {
                self.money = s.money.or(self.money);
                self.bank_money = s.bank_money.or(self.bank_money);
            }
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

    /// PNJ en vue dont le nom finit par `name` et qui a la fonction `f`.
    fn find_npc(&self, name: &str, f: i32) -> anyhow::Result<i32> {
        self.npcs
            .iter()
            .find(|(_, (n, fs))| n.ends_with(name) && fs.iter().any(|x| x.function == f))
            .map(|(cid, _)| *cid)
            .ok_or_else(|| anyhow::anyhow!("{name} (fonction {f}) introuvable parmi {:?}", self.npcs.values().map(|v| &v.0).collect::<Vec<_>>()))
    }

    async fn go(&mut self, to: Vec3) {
        self.send(Command::Stop { position: to }).await;
        self.drain(1200).await;
    }
}

async fn enter(host: &str, port: u16, user: &str, pass: &str, name: &str) -> anyhow::Result<Session> {
    let mut login = LoginClient::connect(host, port).await?;
    login.authenticate(user, pass).await?;
    let world = login.worlds.first().map(|w| w.name.clone()).unwrap_or_else(|| "ArchCore".into());
    let names = login.select_world(&world).await?;
    anyhow::ensure!(names.iter().any(|n| n.eq_ignore_ascii_case(name)), "{name} absent du compte {user}");
    let ticket = login.enter_world(name, &world).await?;
    login.close().await;
    let mut wc = WorldClient::connect(ticket).await?;
    let early = wc.enter().await?;
    let cid = wc.cid;
    let (etx, erx) = mpsc::channel(512);
    let (ctx, crx) = mpsc::channel(64);
    tokio::spawn(wc.run(etx, crx));
    println!("{name} en jeu (cid {cid})");
    let mut s = Session { commands: ctx, events: erx, npcs: HashMap::new(), items: HashMap::new(), money: None, bank_money: None };
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
    let arg = |i: usize, d: &str| rest.get(i).map(|s| s.to_string()).unwrap_or_else(|| d.into());
    let (user, pass, name) = (arg(0, "gmtest"), arg(1, "gmtest"), arg(2, "GmTest"));
    let (host, port) = hostport.split_once(':').map_or((hostport.as_str(), 11102), |(h, p)| (h, p.parse().unwrap_or(11102)));
    let mut s = enter(host, port, &user, &pass, &name).await?;
    s.send(Command::Say("/money 5000".into())).await;

    println!("\n▶ Darryl : fonctions, dialogue");
    s.go(DARRYL).await;
    let darryl = s.find_npc(" Darryl", function::TRADE)?;
    println!("  fonctions : {:?}", s.npcs[&darryl].1);
    s.npc(NpcCommand::Dialog(darryl)).await;
    s.expect("dialogue accordé", 4, |e| matches!(e, WorldEvent::Npc(NpcEvent::DialogGranted { .. }))).await?;
    println!("  ✓ dialogue accordé");

    println!("\n▶ boutique : liste, achat de 3 potions, vente");
    s.npc(NpcCommand::ShopList(darryl)).await;
    let mut shop = Vec::new();
    loop {
        match s.expect("liste de la boutique", 5, |e| matches!(e, WorldEvent::Npc(NpcEvent::ShopItem { .. } | NpcEvent::ShopEnd { .. }))).await? {
            WorldEvent::Npc(NpcEvent::ShopItem { iid, tid, price, .. }) => shop.push((iid, tid, price)),
            _ => break,
        }
    }
    let (iid, _, price) = *shop.iter().find(|(_, t, _)| *t == POTION).ok_or_else(|| anyhow::anyhow!("potion absente"))?;
    println!("  ✓ {} objet(s), potion à {price} pièce(s)", shop.len());
    let before: Vec<i32> = s.items.keys().copied().collect();
    s.npc(NpcCommand::Buy { npc: darryl, iid, count: 3 }).await;
    let ev = s.expect("résultat d'achat", 5, |e| matches!(e, WorldEvent::Npc(NpcEvent::Bought { .. }))).await?;
    anyhow::ensure!(matches!(ev, WorldEvent::Npc(NpcEvent::Bought { result: trade_result::BUY_OK })), "achat : {ev:?}");
    s.drain(400).await;
    let bought = s.items.values().filter(|i| i.template_id == Some(POTION) && i.status == Some(2)).max_by_key(|i| (!before.contains(&i.id), i.id)).cloned().ok_or_else(|| anyhow::anyhow!("potions absentes"))?;
    println!("  ✓ achat réussi (pile {} × {:?}), argent {:?}", bought.id, bought.count, s.money);
    s.npc(NpcCommand::Sell { npc: darryl, iid: bought.id }).await;
    let ev = s.expect("résultat de vente", 5, |e| matches!(e, WorldEvent::Npc(NpcEvent::Sold { .. }))).await?;
    anyhow::ensure!(matches!(ev, WorldEvent::Npc(NpcEvent::Sold { result: trade_result::SELL_OK })), "vente : {ev:?}");
    s.drain(400).await;
    println!("  ✓ vente réussie, argent {:?}", s.money);

    println!("\n▶ banque chez Holly");
    s.go(HOLLY).await;
    let holly = s.find_npc(" Holly", function::BANK)?;
    s.npc(NpcCommand::BankOpen(holly)).await;
    s.expect("banque ouverte", 4, |e| matches!(e, WorldEvent::Npc(NpcEvent::BankOpened { .. }))).await?;
    println!("  ✓ ouverte ; {:?} pièce(s) en banque", s.bank_money);
    s.send(Command::Say(format!("/create \"{name}\" {POTION} 2"))).await;
    s.expect("potions créées", 5, |e| matches!(e, WorldEvent::ItemChanged(i) if i.template_id == Some(POTION))).await?;
    s.drain(400).await;
    let pile = s.items.values().filter(|i| i.template_id == Some(POTION) && i.status == Some(2)).max_by_key(|i| i.id).cloned().unwrap();
    let taken: Vec<[i16; 3]> = s.items.values().filter(|i| i.status == Some(STATUS_BANK)).filter_map(|i| i.grid).collect();
    let cell = (0..7i16).flat_map(|r| (0..4i16).map(move |c| [0, r, c])).find(|g| !taken.contains(g)).ok_or_else(|| anyhow::anyhow!("banque pleine"))?;
    s.npc(NpcCommand::BankStore { iid: pile.id, grid: cell }).await;
    s.expect("objet en banque", 4, |e| matches!(e, WorldEvent::ItemChanged(i) if i.id == pile.id && i.status == Some(STATUS_BANK))).await?;
    println!("  ✓ pile {} déposée en {cell:?}", pile.id);
    let bank0 = s.bank_money.unwrap_or(0);
    s.npc(NpcCommand::BankMoney(1000)).await;
    s.expect("argent déposé", 4, |e| matches!(e, WorldEvent::CharacterUpdated { stats: Some(st), .. } if st.bank_money.is_some())).await?;
    anyhow::ensure!(s.bank_money == Some(bank0 + 1000), "banque {:?}", s.bank_money);
    println!("  ✓ 1000 pièces déposées : {:?} en banque, {:?} sur soi", s.bank_money, s.money);
    s.npc(NpcCommand::BankMoney(-1000)).await;
    s.expect("argent retiré", 4, |e| matches!(e, WorldEvent::CharacterUpdated { stats: Some(st), .. } if st.bank_money.is_some())).await?;
    s.npc(NpcCommand::BankTake { iid: pile.id, grid: None }).await;
    s.expect("objet retiré", 4, |e| matches!(e, WorldEvent::ItemChanged(i) if i.id == pile.id && i.status == Some(2))).await?;
    println!("  ✓ argent et pile retirés ({:?} en banque)", s.bank_money);
    s.send(Command::Item(archrust_netclient::items::ItemCommand::Destroy(pile.id))).await;

    println!("\n▶ quête {QUEST} : prise depuis le journal puis abandonnée (niveau 120 le temps de l'essai)");
    s.send(Command::Say(format!("/questreset {QUEST}"))).await;
    s.send(Command::Say("/level 120".into())).await;
    s.drain(800).await;
    s.npc(NpcCommand::QuestAccept { npc: None, tid: QUEST }).await;
    let ev = s.expect("prise", 4, |e| matches!(e, WorldEvent::Npc(NpcEvent::QuestResult { op: 1, .. }))).await?;
    anyhow::ensure!(matches!(ev, WorldEvent::Npc(NpcEvent::QuestResult { success: true, .. })), "prise refusée : {ev:?}");
    println!("  ✓ prise");
    s.npc(NpcCommand::QuestAbandon(QUEST)).await;
    let ev = s.expect("abandon", 4, |e| matches!(e, WorldEvent::Npc(NpcEvent::QuestResult { op: 7, .. }))).await?;
    anyhow::ensure!(matches!(ev, WorldEvent::Npc(NpcEvent::QuestResult { success: true, .. })), "abandon refusé : {ev:?}");
    println!("  ✓ abandonnée");
    s.send(Command::Say("/level 3".into())).await;
    s.drain(500).await;
    println!("\nPNJ : OK");
    Ok(())
}
