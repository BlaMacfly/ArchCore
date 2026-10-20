//! Combat contre ArchCore, sans affichage : victoire sur un Ver vert (MJ invincible), butin ramassé,
//! puis défaite au niveau 1 et résurrection au village.
//!
//! ```text
//! cargo run -p archrust-netclient --example combat -- [compte mdp perso] [hôte[:port]]
//! ```
//! Par défaut : gmtest/gmtest/GmTest (compte MJ des bases de test) sur 127.0.0.1:11102. Le personnage
//! est remis au niveau 1 par `/level 1` pour la défaite.

use archrust_netclient::combat::{outcome, CombatCommand, GroundItem};
use archrust_netclient::{Command, LoginClient, WorldClient, WorldEvent};
use glam::Vec3;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{timeout, Instant};

/// Vers verts au nord d'Anchorville (Limelight Nord).
const HUNT: Vec3 = Vec3::new(-466_468.0, 4_700.0, -50_010.0);

struct Session {
    cid: i32,
    commands: mpsc::Sender<Command>,
    events: mpsc::Receiver<WorldEvent>,
    /// Monstres en vue : nom, position, PV.
    seen: HashMap<i32, (String, Vec3, Option<i32>)>,
}

impl Session {
    async fn send(&self, c: Command) {
        self.commands.send(c).await.expect("liaison fermée");
    }

    async fn say(&self, text: &str) {
        self.send(Command::Say(text.into())).await;
    }

    /// Prochain événement (avec suivi des monstres en vue), `None` après `wait`.
    async fn next(&mut self, wait: Duration) -> Option<WorldEvent> {
        let ev = timeout(wait, self.events.recv()).await.ok()??;
        match &ev {
            WorldEvent::CharacterSeen(c) => {
                let hp = c.stats.as_ref().and_then(|s| s.hp);
                let hp = if c.action_status == Some(2) { Some(0) } else { hp };
                self.seen.insert(c.cid, (format!("{} [{}]", c.name, c.tid), c.position, hp));
            }
            WorldEvent::CharacterRemoved { cid, .. } => {
                self.seen.remove(cid);
            }
            WorldEvent::CharacterMoved { cid, position, .. } => {
                if let Some(m) = self.seen.get_mut(cid) {
                    m.1 = *position;
                }
            }
            WorldEvent::CharacterUpdated { cid, action_status: Some(2), .. } => {
                if let Some(m) = self.seen.get_mut(cid) {
                    m.2 = Some(0);
                }
            }
            WorldEvent::CharacterAction { target_cid, factors, .. } => {
                if let (Some(m), Some(hp)) = (self.seen.get_mut(target_cid), factors.as_ref().and_then(|f| f.hp)) {
                    m.2 = Some(hp);
                }
            }
            WorldEvent::Disconnected(why) => panic!("déconnecté : {why}"),
            _ => {}
        }
        Some(ev)
    }

    /// Attend un événement qui satisfait `want` (au plus `secs` secondes).
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

    /// Ver vert vivant le plus proche du point de chasse.
    fn prey(&self) -> Option<(i32, String, Option<i32>)> {
        self.seen
            .iter()
            .filter(|(cid, (name, _, hp))| **cid >= 0x4000_0000 && name.ends_with("[450]") && hp.is_none_or(|h| h > 0))
            .min_by_key(|(_, (_, p, _))| p.distance(HUNT) as i64)
            .map(|(cid, (n, _, hp))| (*cid, n.clone(), *hp))
    }

    /// Attaque jusqu'à la mort de la cible ; rend les coups (portés, manqués).
    async fn fight(&mut self, target: i32) -> anyhow::Result<(u32, u32)> {
        self.send(Command::Combat(CombatCommand::Attack { target, forced: true })).await;
        let (mut hits, mut misses) = (0, 0);
        let me = self.cid;
        loop {
            let ev = self
                .expect("coups", 40, |e| matches!(e, WorldEvent::CharacterAction { .. } | WorldEvent::CharacterUpdated { action_status: Some(2), .. }))
                .await?;
            match ev {
                WorldEvent::CharacterUpdated { cid, .. } if cid == target => return Ok((hits, misses)),
                WorldEvent::CharacterUpdated { cid, .. } if cid == me => anyhow::bail!("mort pendant la victoire"),
                WorldEvent::CharacterAction { cid, target_cid, result, damage, factors, .. } if cid == me && target_cid == target => {
                    let hp = factors.and_then(|f| f.hp);
                    match result {
                        Some(outcome::SUCCESS | outcome::CRITICAL) => hits += 1,
                        _ => misses += 1,
                    }
                    println!("  coup : résultat {result:?}, {damage:?} dégât(s), {hp:?} PV restants");
                }
                _ => {}
            }
        }
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
    wc.enter().await?;
    let cid = wc.cid;
    // Monstres déjà en vue à l'entrée en jeu.
    let seen = wc
        .seen
        .iter()
        .map(|c| {
            let hp = if c.action_status == Some(2) { Some(0) } else { c.stats.as_ref().and_then(|s| s.hp) };
            (c.cid, (format!("{} [{}]", c.name, c.tid), c.position, hp))
        })
        .collect();
    let (etx, erx) = mpsc::channel(512);
    let (ctx, crx) = mpsc::channel(64);
    tokio::spawn(wc.run(etx, crx));
    println!("{name} en jeu (cid {cid})");
    Ok(Session { cid, commands: ctx, events: erx, seen })
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
    let me = s.cid;

    println!("\n▶ point de chasse");
    s.send(Command::Stop { position: HUNT }).await;
    while let Some(ev) = s.next(Duration::from_millis(1500)).await {
        if std::env::var("COMBAT_DEBUG").is_ok() {
            println!("  {:?}", std::mem::discriminant(&ev));
            if let WorldEvent::Unhandled { packet_type, op } = ev {
                println!("    non géré 0x{packet_type:02x} op {op:?}");
            }
        }
    }
    println!("  {} monstre(s) en vue", s.seen.len());
    let (target, prey, hp) = s.prey().ok_or_else(|| anyhow::anyhow!("aucun Ver vert en vue : {:?}", s.seen.values().map(|v| &v.0).take(20).collect::<Vec<_>>()))?;
    println!("  cible {prey} ({target:#x}), {hp:?} PV");

    println!("\n▶ victoire (invincible)");
    s.say("/invincible 1").await;
    let (hits, misses) = s.fight(target).await?;
    println!("  ✓ {prey} tué : {hits} coup(s) portés, {misses} manqué(s)");

    println!("\n▶ butin");
    let mut ground: Vec<GroundItem> = Vec::new();
    while let Some(ev) = s.next(Duration::from_millis(1500)).await {
        if let WorldEvent::GroundItem(g) = ev {
            if g.owner_cid == Some(me) {
                ground.push(g);
            }
        }
    }
    anyhow::ensure!(!ground.is_empty(), "aucun objet au sol");
    for g in &ground {
        s.send(Command::Combat(CombatCommand::PickUp(g.iid))).await;
        let r = s.expect("résultat du ramassage", 3, |e| matches!(e, WorldEvent::PickupResult { iid, .. } if *iid == g.iid)).await?;
        let WorldEvent::PickupResult { success, .. } = r else { unreachable!() };
        anyhow::ensure!(success, "ramassage de {:#x} refusé", g.iid);
        println!("  ✓ TID {} × {} ramassé{}", g.tid, g.count, if g.is_gold() { " (or)" } else { "" });
    }

    println!("\n▶ défaite au niveau 1");
    s.say("/invincible 0").await;
    s.say("/level 1").await;
    while s.next(Duration::from_millis(800)).await.is_some() {}
    let (target, prey, _) = s.prey().ok_or_else(|| anyhow::anyhow!("aucun autre Ver vert en vue"))?;
    s.send(Command::Combat(CombatCommand::Attack { target, forced: true })).await;
    let started = Instant::now();
    s.expect("mort du joueur", 180, |e| matches!(e, WorldEvent::CharacterUpdated { cid, action_status: Some(2), .. } if *cid == me)).await?;
    println!("  ✓ tué par {prey} en {:.0} s", started.elapsed().as_secs_f32());

    println!("\n▶ résurrection");
    s.send(Command::Combat(CombatCommand::ResurrectTown)).await;
    let back = s.expect("résurrection", 5, |e| matches!(e, WorldEvent::CharacterUpdated { cid, action_status: Some(0), .. } if *cid == me)).await?;
    let hp = match back {
        WorldEvent::CharacterUpdated { stats, .. } => stats.and_then(|s| s.hp),
        _ => None,
    };
    let at = s.expect("téléportation", 5, |e| matches!(e, WorldEvent::CharacterMoved { cid, .. } if *cid == me)).await?;
    if let WorldEvent::CharacterMoved { position, .. } = at {
        println!("  ✓ ressuscité avec {hp:?} PV en {:.0}, {:.0}, {:.0}", position.x, position.y, position.z);
    }
    s.send(Command::Quit).await;
    println!("\ncombat : OK");
    Ok(())
}
