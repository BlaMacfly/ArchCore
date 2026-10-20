//! Inventaire et équipement contre ArchCore, sans affichage : objets créés par commande MJ, épée
//! portée puis rangée, déplacée, pile de potions séparée puis réunie, potion utilisée, objets détruits.
//!
//! ```text
//! cargo run -p archrust-netclient --example items -- [compte mdp perso] [hôte[:port]]
//! ```
//! Par défaut : gmtest/gmtest/GmTest (compte MJ des bases de test) sur 127.0.0.1:11102.

use archrust_netclient::items::{status, Grid, ItemCommand};
use archrust_netclient::world::ItemRecord;
use archrust_netclient::{Command, LoginClient, WorldClient, WorldEvent};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{timeout, Instant};

/// Épée courte (guerrier) et petite potion de vie.
const SWORD: i32 = 11;
const POTION: i32 = 190;

struct Session {
    commands: mpsc::Sender<Command>,
    events: mpsc::Receiver<WorldEvent>,
    items: HashMap<i32, ItemRecord>,
}

impl Session {
    async fn send(&self, c: Command) {
        self.commands.send(c).await.expect("liaison fermée");
    }

    async fn item(&self, c: ItemCommand) {
        self.send(Command::Item(c)).await;
    }

    /// Prochain événement, avec l'inventaire tenu à jour comme le moteur.
    async fn next(&mut self, wait: Duration) -> Option<WorldEvent> {
        let ev = timeout(wait, self.events.recv()).await.ok()??;
        self.track(&ev);
        Some(ev)
    }

    fn track(&mut self, ev: &WorldEvent) {
        match ev {
            WorldEvent::ItemsSnapshot(items) => {
                for i in items.iter().filter(|i| matches!(i.status, Some(2 | 3))) {
                    self.items.insert(i.id, i.clone());
                }
            }
            WorldEvent::ItemChanged(i) if i.operation == 1 || i.status.is_some_and(|s| !matches!(s, 2 | 3)) => {
                self.items.remove(&i.id);
            }
            WorldEvent::ItemChanged(i) => {
                let cur = self.items.entry(i.id).or_insert_with(|| i.clone());
                if i.template_id.is_some() {
                    cur.template_id = i.template_id;
                }
                if i.count.is_some() {
                    cur.count = i.count;
                }
                if i.status.is_some() {
                    cur.status = i.status;
                }
                if i.grid.is_some() || i.status == Some(status::EQUIP) {
                    cur.grid = i.grid;
                }
            }
            WorldEvent::SystemMessage(m) => println!("  (message système : {m})"),
            WorldEvent::Disconnected(why) => panic!("déconnecté : {why}"),
            _ => {}
        }
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

    fn of_tid(&self, tid: i32) -> Vec<ItemRecord> {
        let mut v: Vec<ItemRecord> = self.items.values().filter(|i| i.template_id == Some(tid)).cloned().collect();
        v.sort_by_key(|i| i.id);
        v
    }

    fn free_cell(&self) -> Grid {
        let taken: Vec<Grid> = self.items.values().filter(|i| i.status == Some(status::INVENTORY)).filter_map(|i| i.grid).collect();
        // 4 sacs de 4 × 4 cases côté serveur.
        (0..4i16)
            .flat_map(|b| (0..4i16).flat_map(move |r| (0..4i16).map(move |c| [b, r, c])))
            .find(|g| !taken.contains(g))
            .expect("inventaire plein")
    }
}

async fn enter(host: &str, port: u16, user: &str, pass: &str, name: &str) -> anyhow::Result<(Session, i32)> {
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
    let mut s = Session { commands: ctx, events: erx, items: HashMap::new() };
    // L'inventaire arrive pendant l'entrée en jeu (OPT_VIEW op 2).
    for ev in &early {
        s.track(ev);
    }
    Ok((s, cid))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let hostport = args.iter().find(|a| a.contains(':') || a.contains('.')).cloned().unwrap_or_else(|| "127.0.0.1:11102".into());
    let rest: Vec<&String> = args.iter().filter(|a| **a != hostport).collect();
    let arg = |i: usize, d: &str| rest.get(i).map(|s| s.to_string()).unwrap_or_else(|| d.into());
    let (user, pass, name) = (arg(0, "gmtest"), arg(1, "gmtest"), arg(2, "GmTest"));
    let (host, port) = hostport.split_once(':').map_or((hostport.as_str(), 11102), |(h, p)| (h, p.parse().unwrap_or(11102)));
    let (mut s, _me) = enter(host, port, &user, &pass, &name).await?;
    s.drain(1500).await;
    println!("  {} objet(s) dans l'inventaire et l'équipement", s.items.len());

    println!("\n▶ objets créés par commande MJ");
    let before: Vec<i32> = s.of_tid(SWORD).iter().map(|i| i.id).collect();
    s.send(Command::Say(format!("/create \"{name}\" {SWORD} 1"))).await;
    s.expect("épée créée", 5, |e| matches!(e, WorldEvent::ItemChanged(i) if i.template_id == Some(SWORD))).await?;
    s.send(Command::Say(format!("/create \"{name}\" {POTION} 6"))).await;
    s.expect("potions créées", 5, |e| matches!(e, WorldEvent::ItemChanged(i) if i.template_id == Some(POTION))).await?;
    s.drain(500).await;
    let sword = s.of_tid(SWORD).into_iter().find(|i| !before.contains(&i.id)).ok_or_else(|| anyhow::anyhow!("épée absente"))?;
    let potions = s.of_tid(POTION).into_iter().max_by_key(|i| i.count).ok_or_else(|| anyhow::anyhow!("potions absentes"))?;
    println!("  ✓ épée IID {} en {:?} ; potions IID {} × {:?}", sword.id, sword.grid, potions.id, potions.count);

    println!("\n▶ porter l'épée (op 2, statut 3)");
    s.item(ItemCommand::Equip(sword.id)).await;
    s.expect("épée portée", 5, |e| matches!(e, WorldEvent::ItemChanged(i) if i.id == sword.id && i.status == Some(status::EQUIP))).await?;
    println!("  ✓ portée");

    println!("\n▶ la ranger, place au choix du serveur (op 2, statut 2 sans case)");
    s.item(ItemCommand::Unequip { iid: sword.id, grid: None }).await;
    let ev = s.expect("épée rangée", 5, |e| matches!(e, WorldEvent::ItemChanged(i) if i.id == sword.id && i.status == Some(status::INVENTORY))).await?;
    let WorldEvent::ItemChanged(back) = ev else { unreachable!() };
    println!("  ✓ rangée en {:?}", back.grid);

    println!("\n▶ la déplacer dans le sac (op 2, statut 2, case)");
    let to = s.free_cell();
    s.item(ItemCommand::Move { iid: sword.id, grid: to }).await;
    s.expect("épée déplacée", 5, |e| matches!(e, WorldEvent::ItemChanged(i) if i.id == sword.id && i.grid == Some(to))).await?;
    println!("  ✓ déplacée en {to:?}");

    println!("\n▶ séparer 2 potions (op 17)");
    let to = s.free_cell();
    s.item(ItemCommand::Split { iid: potions.id, count: 2, grid: to }).await;
    let ev = s.expect("nouvelle pile", 5, |e| matches!(e, WorldEvent::ItemChanged(i) if i.operation == 0 && i.template_id == Some(POTION) && i.grid == Some(to))).await?;
    let WorldEvent::ItemChanged(part) = ev else { unreachable!() };
    s.drain(400).await;
    let left = s.items.get(&potions.id).and_then(|i| i.count);
    anyhow::ensure!(part.count == Some(2) && left == Some(potions.count.unwrap_or(0) - 2), "piles {:?} et {left:?}", part.count);
    println!("  ✓ pile de {:?} en {to:?}, reste {left:?}", part.count);

    println!("\n▶ les réunir (op 2 sur la case de l'autre pile)");
    let grid = s.items[&potions.id].grid.expect("case de la pile");
    s.item(ItemCommand::Move { iid: part.id, grid }).await;
    s.expect("pile absorbée", 5, |e| matches!(e, WorldEvent::ItemChanged(i) if i.id == part.id && i.operation == 1)).await?;
    s.drain(400).await;
    let total = s.items.get(&potions.id).and_then(|i| i.count);
    anyhow::ensure!(total == potions.count, "pile réunie : {total:?}");
    println!("  ✓ pile réunie : {total:?}");

    println!("\n▶ utiliser une potion (op 3)");
    s.item(ItemCommand::Use(potions.id)).await;
    match s.expect("potion utilisée ou refusée", 4, |e| matches!(e, WorldEvent::ItemUsed { .. } | WorldEvent::SystemMessage(_))).await? {
        WorldEvent::ItemUsed { tid, cooldown_ms, .. } => {
            s.drain(400).await;
            println!("  ✓ op 4 : TID {tid:?}, délai {cooldown_ms:?} ms ; reste {:?}", s.items.get(&potions.id).and_then(|i| i.count));
        }
        _ => println!("  ✓ refusée par le serveur (message ci-dessus : PV pleins ou délai)"),
    }

    println!("\n▶ détruire l'épée et les potions (op 21)");
    for iid in [sword.id, potions.id] {
        s.item(ItemCommand::Destroy(iid)).await;
        s.expect("objet détruit", 5, |e| matches!(e, WorldEvent::ItemChanged(i) if i.id == iid && i.operation == 1)).await?;
    }
    println!("  ✓ détruits");
    println!("\ninventaire et équipement : OK");
    Ok(())
}
