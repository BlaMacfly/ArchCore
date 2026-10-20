//! Compétences contre ArchCore, sans affichage : liste à l'entrée en jeu, instructeur Black
//! (fenêtre, apprentissage, amélioration), lancement d'un renforcement sur soi.
//!
//! ```text
//! cargo run -p archrust-netclient --example skills -- [compte mdp perso] [hôte[:port]]
//! ```
//! Par défaut : gmtest/gmtest/GmTest (compte MJ des bases de test) sur 127.0.0.1:11102. Le personnage
//! passe au niveau 25 le temps de l'essai.

use archrust_netclient::npc::function;
use archrust_netclient::skills::{learn_result, SkillCommand, SkillEvent};
use archrust_netclient::{Command, LoginClient, WorldClient, WorldEvent};
use glam::Vec3;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{timeout, Instant};

const TRAINER: Vec3 = Vec3::new(-480_700.0, 5_070.0, -62_100.0);
/// Speed Attack : renforcement du guerrier humain, niveau 5.
const SPEED_ATTACK: i32 = 4;

struct Session {
    cid: i32,
    commands: mpsc::Sender<Command>,
    events: mpsc::Receiver<WorldEvent>,
    /// Compétences : identifiant → (TID, niveau).
    skills: HashMap<i32, (i32, i32)>,
    points: Option<i32>,
    trainers: HashMap<i32, (String, (i32, i32))>,
}

impl Session {
    async fn send(&self, c: Command) {
        self.commands.send(c).await.expect("liaison fermée");
    }

    fn track(&mut self, ev: &WorldEvent) {
        match ev {
            WorldEvent::Skill(SkillEvent::Known { owner, id, tid, level }) if *owner == self.cid || *owner == 0 => {
                self.skills.insert(*id, (*tid, *level));
            }
            WorldEvent::Skill(SkillEvent::Level { id, level }) => {
                if let Some(s) = self.skills.get_mut(id) {
                    s.1 = *level;
                }
            }
            WorldEvent::CharacterSeen(c) => {
                if let Some(t) = c.npc_functions.iter().find(|f| f.function == function::SKILL_MASTER).and_then(|f| f.teach) {
                    self.trainers.insert(c.cid, (c.name.clone(), t));
                }
            }
            WorldEvent::CharacterUpdated { stats: Some(s), .. } | WorldEvent::SelfCharacter { stats: Some(s), .. } => {
                self.points = s.skill_points.or(self.points);
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let hostport = args.iter().find(|a| a.contains(':') || a.contains('.')).cloned().unwrap_or_else(|| "127.0.0.1:11102".into());
    let rest: Vec<&String> = args.iter().filter(|a| **a != hostport).collect();
    let arg = |i: usize, d: &str| rest.get(i).map(|s| s.to_string()).unwrap_or_else(|| d.into());
    let (user, pass, name) = (arg(0, "gmtest"), arg(1, "gmtest"), arg(2, "GmTest"));
    let (host, port) = hostport.split_once(':').map_or((hostport.as_str(), 11102), |(h, p)| (h, p.parse().unwrap_or(11102)));

    let mut login = LoginClient::connect(host, port).await?;
    login.authenticate(&user, &pass).await?;
    let world = login.worlds.first().map(|w| w.name.clone()).unwrap_or_else(|| "ArchCore".into());
    login.select_world(&world).await?;
    let ticket = login.enter_world(&name, &world).await?;
    login.close().await;
    let mut wc = WorldClient::connect(ticket).await?;
    let early = wc.enter().await?;
    let cid = wc.cid;
    let (etx, erx) = mpsc::channel(512);
    let (ctx, crx) = mpsc::channel(64);
    tokio::spawn(wc.run(etx, crx));
    let mut s = Session { cid, commands: ctx, events: erx, skills: HashMap::new(), points: None, trainers: HashMap::new() };
    for ev in &early {
        s.track(ev);
    }
    println!("{name} en jeu (cid {cid}) : {} compétence(s) connue(s) {:?}, {:?} point(s)", s.skills.len(), s.skills.values().collect::<Vec<_>>(), s.points);
    anyhow::ensure!(s.skills.values().any(|(t, _)| *t == 222), "« Normal Attack » (TID 222) absente de la liste");

    println!("\n▶ niveau 25, argent, chez Black");
    s.send(Command::Say("/level 25".into())).await;
    s.send(Command::Say("/money 100000".into())).await;
    s.send(Command::Stop { position: TRAINER }).await;
    s.drain(1500).await;
    println!("  {:?} point(s) de compétence", s.points);
    let (&black, (tname, teach)) = s.trainers.iter().find(|(_, (n, _))| n.ends_with(" Black")).ok_or_else(|| anyhow::anyhow!("Black introuvable : {:?}", s.trainers))?;
    println!("  {tname} enseigne race/classe {teach:?}");
    s.send(Command::Skill(SkillCommand::MasterOpen(black))).await;
    s.expect("fenêtre de l'instructeur", 4, |e| matches!(e, WorldEvent::Skill(SkillEvent::MasterOpened { .. }))).await?;
    println!("  ✓ fenêtre ouverte");

    println!("\n▶ apprendre puis améliorer Speed Attack");
    let id = match s.skills.iter().find(|(_, (t, _))| *t == SPEED_ATTACK).map(|(id, _)| *id) {
        Some(id) => {
            println!("  déjà connue (identifiant {id})");
            id
        }
        None => {
            s.send(Command::Skill(SkillCommand::Learn { npc: black, tid: SPEED_ATTACK })).await;
            let ev = s.expect("résultat d'apprentissage", 4, |e| matches!(e, WorldEvent::Skill(SkillEvent::Learned { .. }))).await?;
            anyhow::ensure!(matches!(ev, WorldEvent::Skill(SkillEvent::Learned { result: learn_result::SUCCESS, .. })), "apprentissage : {ev:?}");
            s.drain(400).await;
            let id = s.skills.iter().find(|(_, (t, _))| *t == SPEED_ATTACK).map(|(id, _)| *id).ok_or_else(|| anyhow::anyhow!("compétence apprise absente de la liste"))?;
            println!("  ✓ apprise (identifiant {id}), {:?} point(s)", s.points);
            id
        }
    };
    let before = s.skills[&id].1;
    s.send(Command::Skill(SkillCommand::Upgrade { npc: black, id })).await;
    match s.expect("amélioration", 4, |e| matches!(e, WorldEvent::Skill(SkillEvent::Upgraded { .. }) | WorldEvent::SystemMessage(_))).await? {
        WorldEvent::Skill(_) => {
            s.drain(300).await;
            println!("  ✓ niveau {before} → {}", s.skills[&id].1);
        }
        _ => println!("  ✓ refusée par le serveur (message ci-dessus)"),
    }

    println!("\n▶ lancer Speed Attack sur soi");
    s.send(Command::Skill(SkillCommand::Cast { id, target: cid })).await;
    let ev = s.expect("lancement", 6, |e| matches!(e, WorldEvent::Skill(SkillEvent::Cast { .. }))).await?;
    println!("  ✓ lancement : {ev:?}");
    s.expect("effet sur soi", 6, |e| matches!(e, WorldEvent::BuffAdded(b) if b.owner == cid)).await?;
    println!("  ✓ effet posé");

    s.send(Command::Say("/level 3".into())).await;
    s.drain(500).await;
    println!("\ncompétences : OK");
    Ok(())
}
