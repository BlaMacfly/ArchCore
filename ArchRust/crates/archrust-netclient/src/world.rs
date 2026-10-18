//! Serveur monde (port 11008) : entrée en jeu, PNJ, keepalive, événements pour le moteur.
//!
//! Séquence relevée dans le client (fonctions d'`AgcmCharacter`) et validée avec ArchCore :
//!
//! ```text
//! CHAR_MANAGER op 2  (champ 2 = 17, "Nom#Monde", clé d'auth, CHAR[248])   client → monde
//! CHAR_MANAGER op 13 (keepalive, champ 2 = 0), puis toutes les 15 s
//! CONFIG, CHARACTER op 25 (le joueur), CHAR_MANAGER op 8 (position, champ 5), TIMER
//! CHAR_MANAGER op 9  (CID champ 4)                                         client → monde
//! CHARACTER op 0, OPT_VIEW op 1 par PNJ/personnage en vue (mini vue champ 1)
//! CHAR_MANAGER op 7  : le joueur est en jeu
//! ```

use crate::connection::{Connection, Incoming, NetError};
use crate::login::WorldTicket;
use archrust_protocol::packet::{build_mini, decode_mini, sparse, Value};
use archrust_protocol::schema::{char_mgr_op, main, mini, packet_type, view_op};
use glam::Vec3;
use std::time::Duration;
use tokio::sync::mpsc;

pub const KEEPALIVE: Duration = Duration::from_secs(15);

/// Entité vue par le client (PNJ, monstre, joueur).
#[derive(Debug, Clone, PartialEq)]
pub struct SeenCharacter {
    pub cid: i32,
    pub tid: i32,
    pub name: String,
    pub position: Vec3,
    pub destination: Vec3,
}

/// Événements remontés au moteur.
#[derive(Debug, Clone)]
pub enum WorldEvent {
    Connected,
    /// Paquet CHARACTER du joueur (op 25 puis 0) : CID, TID, nom.
    SelfCharacter { cid: i32, tid: i32, name: String },
    LoadingPosition(Vec3),
    GameTime(u64),
    CharacterSeen(SeenCharacter),
    CharacterMoved { cid: i32, position: Vec3, destination: Vec3 },
    Chat { from_cid: i32, channel: i8, from: String, message: String },
    SystemMessage(String),
    /// Entrée en jeu confirmée (op 7).
    EnteredWorld,
    /// Paquet non interprété (type, opération).
    Unhandled { packet_type: u8, op: Option<i64> },
    Disconnected(String),
}

/// Commandes du moteur vers le serveur.
#[derive(Debug, Clone)]
pub enum Command {
    MoveTo { position: Vec3, destination: Vec3 },
    Say(String),
    Quit,
}

fn vec3(v: Option<&Value>) -> Option<Vec3> {
    v.and_then(Value::as_vec3).map(Vec3::from_array)
}
fn text(v: Option<&Value>) -> String {
    v.and_then(Value::as_text).unwrap_or_default()
}

/// Décode une vue de personnage (mini OPT_VIEW_CHAR).
pub fn decode_view(mini_bytes: &[u8]) -> Option<SeenCharacter> {
    let (_, v) = decode_mini(mini::OPT_VIEW_CHAR, mini_bytes).ok()?;
    let g = |i: usize| v.get(i).and_then(|x| x.as_ref());
    Some(SeenCharacter {
        cid: g(0)?.as_i64()? as i32,
        tid: g(1).and_then(Value::as_i64).unwrap_or(0) as i32,
        name: text(g(2)),
        position: vec3(g(8)).unwrap_or(Vec3::ZERO),
        destination: vec3(g(9)).unwrap_or(Vec3::ZERO),
    })
}

/// Traduit un paquet reçu en événement.
pub fn translate(p: &Incoming) -> WorldEvent {
    match p.packet_type {
        packet_type::CHARACTER => WorldEvent::SelfCharacter {
            cid: p.get(1).and_then(Value::as_i64).unwrap_or(0) as i32,
            tid: p.get(2).and_then(Value::as_i64).unwrap_or(0) as i32,
            name: text(p.get(3)),
        },
        packet_type::CHAR_MANAGER if p.op() == Some(char_mgr_op::LOADING_POS as i64) => {
            WorldEvent::LoadingPosition(vec3(p.get(5)).unwrap_or(Vec3::ZERO))
        }
        packet_type::CHAR_MANAGER if p.op() == Some(char_mgr_op::FINISH_ENTER as i64) => WorldEvent::EnteredWorld,
        packet_type::TIMER => WorldEvent::GameTime(p.get(1).and_then(Value::as_i64).unwrap_or(0) as u64),
        packet_type::OPT_VIEW if p.op() == Some(view_op::ADD_CHAR as i64) => match p.get(1).and_then(Value::as_bytes).and_then(decode_view) {
            Some(c) => WorldEvent::CharacterSeen(c),
            None => WorldEvent::Unhandled { packet_type: p.packet_type, op: p.op() },
        },
        packet_type::OPT_CHAR_MOVE => WorldEvent::CharacterMoved {
            cid: p.get(0).and_then(Value::as_i64).unwrap_or(0) as i32,
            position: vec3(p.get(1)).unwrap_or(Vec3::ZERO),
            destination: vec3(p.get(2)).unwrap_or(Vec3::ZERO),
        },
        packet_type::CHATTING => WorldEvent::Chat {
            from_cid: p.get(1).and_then(Value::as_i64).unwrap_or(0) as i32,
            channel: p.get(2).and_then(Value::as_i64).unwrap_or(0) as i8,
            from: text(p.get(4)),
            message: text(p.get(6)),
        },
        packet_type::SYSTEM_MESSAGE => WorldEvent::SystemMessage(text(p.get(4))),
        _ => WorldEvent::Unhandled { packet_type: p.packet_type, op: p.op() },
    }
}

pub struct WorldClient {
    pub conn: Connection,
    pub ticket: WorldTicket,
    pub cid: i32,
    pub position: Vec3,
    pub seen: Vec<SeenCharacter>,
}

impl WorldClient {
    /// Se connecte au monde, s'annonce (op 2 + keepalive) et attend la position de chargement.
    pub async fn connect(ticket: WorldTicket) -> Result<Self, NetError> {
        let mut conn = Connection::connect("world", &ticket.host, ticket.port).await?;
        conn.handshake().await?;
        let who = format!("{}#{}", ticket.character, ticket.world);
        conn.send(
            main::CHAR_MANAGER,
            &sparse(
                main::CHAR_MANAGER,
                &[
                    (0, Value::I8(char_mgr_op::GAME_ENTER)),
                    (2, Value::I32(17)),
                    (3, Value::str(&who)),
                    (6, Value::I32(ticket.auth_key)),
                    (7, Value::Str(Vec::new())),
                ],
            ),
        )
        .await?;
        conn.send(main::CHAR_MANAGER, &sparse(main::CHAR_MANAGER, &[(0, Value::I8(char_mgr_op::KEEPALIVE)), (2, Value::I32(0))])).await?;
        Ok(Self { conn, ticket, cid: 0, position: Vec3::ZERO, seen: Vec::new() })
    }

    /// Déroule l'entrée en jeu jusqu'à l'op 7 ; rend les événements dans l'ordre.
    pub async fn enter(&mut self) -> Result<Vec<WorldEvent>, NetError> {
        let mut events = vec![WorldEvent::Connected];
        self.conn.expect(packet_type::CONFIG, None).await?;
        let me = self.conn.expect(packet_type::CHARACTER, Some(25)).await?;
        if let WorldEvent::SelfCharacter { cid, .. } = translate(&me) {
            self.cid = cid;
        }
        events.push(translate(&me));
        let loading = self.conn.expect(packet_type::CHAR_MANAGER, Some(char_mgr_op::LOADING_POS as i64)).await?;
        if let WorldEvent::LoadingPosition(p) = translate(&loading) {
            self.position = p;
        }
        events.push(translate(&loading));
        let timer = self.conn.expect(packet_type::TIMER, None).await?;
        events.push(translate(&timer));
        self.conn
            .send(main::CHAR_MANAGER, &sparse(main::CHAR_MANAGER, &[(0, Value::I8(char_mgr_op::ENTER_WORLD)), (4, Value::I32(self.cid))]))
            .await?;
        loop {
            let p = self.conn.expect_where("entrée en jeu", |_| true).await?;
            let ev = translate(&p);
            match &ev {
                WorldEvent::CharacterSeen(c) => self.seen.push(c.clone()),
                WorldEvent::EnteredWorld => {
                    events.push(ev);
                    break;
                }
                _ => {}
            }
            events.push(ev);
        }
        log::info!("[world] en jeu : cid {} à {:?}, {} personnages en vue", self.cid, self.position, self.seen.len());
        Ok(events)
    }

    /// Envoie un déplacement (type 0x40, format du client).
    pub async fn send_move(&mut self, position: Vec3, destination: Vec3) -> Result<(), NetError> {
        let values = sparse(
            main::OPT_CHAR_MOVE,
            &[
                (0, Value::I32(self.cid)),
                (1, Value::Vec3(position.to_array())),
                (2, Value::Vec3(destination.to_array())),
                (5, Value::I8(0)),
                (6, Value::I8(0)),
            ],
        );
        self.conn.send(main::OPT_CHAR_MOVE, &values).await
    }

    /// Envoie un message sur le canal « dire » (type 0x0F).
    pub async fn send_say(&mut self, message: &str) -> Result<(), NetError> {
        let values = sparse(
            main::CHATTING,
            &[(0, Value::I8(0)), (1, Value::I32(self.cid)), (2, Value::I8(0)), (6, Value::Block(message.as_bytes().to_vec()))],
        );
        self.conn.send(main::CHATTING, &values).await
    }

    async fn keepalive(&mut self) -> Result<(), NetError> {
        self.conn.send(main::CHAR_MANAGER, &sparse(main::CHAR_MANAGER, &[(0, Value::I8(char_mgr_op::KEEPALIVE)), (2, Value::I32(0))])).await
    }

    /// Boucle de jeu réseau : relaie les paquets en événements et exécute les commandes,
    /// avec le keepalive toutes les 15 s. Se termine sur `Command::Quit` ou déconnexion.
    pub async fn run(mut self, events: mpsc::Sender<WorldEvent>, mut commands: mpsc::Receiver<Command>) {
        let mut tick = tokio::time::interval(KEEPALIVE);
        tick.tick().await; // le premier keepalive a déjà été envoyé
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    if let Err(e) = self.keepalive().await {
                        let _ = events.send(WorldEvent::Disconnected(e.to_string())).await;
                        return;
                    }
                }
                cmd = commands.recv() => {
                    let r = match cmd {
                        Some(Command::MoveTo { position, destination }) => self.send_move(position, destination).await,
                        Some(Command::Say(m)) => self.send_say(&m).await,
                        Some(Command::Quit) | None => { self.conn.close().await; return; }
                    };
                    if let Err(e) = r {
                        let _ = events.send(WorldEvent::Disconnected(e.to_string())).await;
                        return;
                    }
                }
                p = self.conn.next() => {
                    match p {
                        Ok(p) => {
                            let ev = translate(&p);
                            if let WorldEvent::CharacterSeen(c) = &ev { self.seen.push(c.clone()); }
                            if events.send(ev).await.is_err() { return; }
                        }
                        Err(e) => {
                            let _ = events.send(WorldEvent::Disconnected(e.to_string())).await;
                            return;
                        }
                    }
                }
            }
        }
    }
}

/// Mini-paquet de vue (pour les tests et les outils serveur).
pub fn build_view(c: &SeenCharacter) -> Vec<u8> {
    build_mini(
        mini::OPT_VIEW_CHAR,
        &sparse(
            mini::OPT_VIEW_CHAR,
            &[
                (0, Value::I32(c.cid)),
                (1, Value::I32(c.tid)),
                (2, Value::Block(c.name.as_bytes().to_vec())),
                (8, Value::Vec3(c.position.to_array())),
                (9, Value::Vec3(c.destination.to_array())),
            ],
        ),
    )
    .expect("schéma statique")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_roundtrip() {
        let c = SeenCharacter { cid: 0x4000_0001, tid: 584, name: "<Merchant> Darryl".into(), position: Vec3::new(-479097.9, 4753.4, -66021.9), destination: Vec3::ZERO };
        let bytes = build_view(&c);
        let back = decode_view(&bytes).unwrap();
        assert_eq!(back, c);
    }
}
