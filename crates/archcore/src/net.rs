//! Serveur TCP générique : une tâche par connexion, découpage des trames, chiffrement, handshake
//! AGPMSTARTUPENCRYPTION (0x48), puis aiguillage des paquets décodés vers la connexion du service.
//!
//! Handshake, commun au login et au monde :
//! ```text
//!   client → CRYPT_REQUEST (0)              en clair
//!   serveur → CRYPT_SET_ALGO (6)            en clair (bloc mémoire vide)
//!   serveur → CRYPT_PUBLIC_KEY (1) + clé    en clair, 32 octets aléatoires ; clé Blowfish « serveur »
//!   client → CRYPT_PRIVATE_KEY (2) + clé    32 octets chiffrés Blowfish avec la clé serveur
//!   serveur → CRYPT_COMPLETE (3)            chiffré ; tout le reste de la session est chiffré
//! ```

use crate::config::Listen;
use crate::{debug, info, warn};
use archcore_net::framing::{CryptoSession, FrameSplitter};
use archcore_net::schema::{self, crypt_op, ext_op};
use archcore_net::{build_packet, decode_packet, hex_dump, parse_header, Fields, Packet, Schema};
use async_trait::async_trait;
use rand::RngCore;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

static NEXT_SESSION_ID: AtomicU32 = AtomicU32::new(1);

/// Paquets permis par seconde et par connexion, et rafale tolérée (`AntiCheat.MaxPacketsPerSecond`,
/// `AntiCheat.PacketBurst` ; 0 : sans limite). Voir `world/anticheat.rs`.
static PACKETS_PER_SEC: AtomicU32 = AtomicU32::new(0);
static PACKET_BURST: AtomicU32 = AtomicU32::new(0);

/// Une inondation se termine après cette pause sans paquet ignoré.
const FLOOD_PAUSE: Duration = Duration::from_secs(2);

pub fn set_packet_limit(per_sec: u32, burst: u32) {
    PACKETS_PER_SEC.store(per_sec, Ordering::Relaxed);
    PACKET_BURST.store(burst.max(per_sec), Ordering::Relaxed);
}

/// Sort d'un paquet reçu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admit {
    Pass,
    /// Au-delà du débit permis : ignoré.
    Drop,
    /// Inondation qui dure (une seconde de débit permis ignorée sans pause) : la connexion est fermée.
    Flood,
}

/// Débit des paquets d'une connexion : seau de jetons (`per_sec` par seconde, `burst` au plus), et pic mesuré sur une
/// seconde, noté à la déconnexion (pour régler la limite d'après le trafic réel).
#[derive(Clone, Debug)]
pub struct PacketBudget {
    tokens: f64,
    at: Instant,
    /// Paquets ignorés depuis le début de l'inondation en cours, et instant du dernier.
    dropped: u32,
    last_drop: Option<Instant>,
    /// Seconde en cours : début et paquets reçus.
    second: (Instant, u32),
    pub peak: u32,
    pub total: u64,
    pub ignored: u64,
}

impl PacketBudget {
    pub fn new(burst: u32, now: Instant) -> Self {
        PacketBudget { tokens: burst as f64, at: now, dropped: 0, last_drop: None, second: (now, 0), peak: 0, total: 0, ignored: 0 }
    }

    pub fn admit(&mut self, now: Instant, per_sec: u32, burst: u32) -> Admit {
        self.total += 1;
        if now.saturating_duration_since(self.second.0) >= Duration::from_secs(1) {
            self.second = (now, 0);
        }
        self.second.1 += 1;
        self.peak = self.peak.max(self.second.1);
        if per_sec == 0 {
            return Admit::Pass;
        }
        let dt = now.saturating_duration_since(self.at).as_secs_f64();
        self.tokens = (self.tokens + dt * per_sec as f64).min(burst.max(per_sec) as f64);
        self.at = now;
        if self.last_drop.is_some_and(|t| now.saturating_duration_since(t) >= FLOOD_PAUSE) {
            self.dropped = 0;
        }
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            return Admit::Pass;
        }
        self.dropped += 1;
        self.ignored += 1;
        self.last_drop = Some(now);
        if self.dropped >= per_sec { Admit::Flood } else { Admit::Drop }
    }

    /// Premier paquet ignoré d'une inondation (pour ne l'annoncer qu'une fois).
    pub fn flood_started(&self) -> bool {
        self.dropped == 1
    }
}

enum Out {
    Bytes(Vec<u8>),
    Close,
}

struct SessionInner {
    id: u32,
    server: &'static str,
    remote: String,
    ip: String,
    crypto: Mutex<CryptoSession>,
    tx: mpsc::UnboundedSender<Out>,
    closed: AtomicBool,
    /// Extensions ArchRust négociées sur cette connexion (`None` : client d'origine).
    ext: Mutex<Option<crate::ext::Agreement>>,
}

/// Poignée vers une connexion : envoi de paquets (chiffrés une fois le handshake terminé), fermeture.
#[derive(Clone)]
pub struct SessionHandle(Arc<SessionInner>);

impl SessionHandle {
    pub fn id(&self) -> u32 {
        self.0.id
    }
    pub fn ip(&self) -> &str {
        &self.0.ip
    }
    /// Accord d'extensions conclu sur cette connexion, s'il y en a un.
    pub fn extension(&self) -> Option<crate::ext::Agreement> {
        self.0.ext.lock().unwrap().clone()
    }

    /// Vrai si la connexion a obtenu la capacité `name`.
    pub fn has_capability(&self, name: &str) -> bool {
        self.0.ext.lock().unwrap().as_ref().is_some_and(|a| a.has(name))
    }

    /// Langue souhaitée par le client (capacité `language`), normalisée ; `None` sans extensions ou sans choix.
    pub fn language(&self) -> Option<String> {
        self.0.ext.lock().unwrap().as_ref().and_then(|a| a.language.clone())
    }

    /// Enregistre le souhait de langue d'un `SET_LANGUAGE`. Rend `false` si la connexion n'a pas d'extensions.
    pub fn set_language(&self, language: Option<String>) -> bool {
        match self.0.ext.lock().unwrap().as_mut() {
            Some(a) => {
                a.language = language;
                true
            }
            None => false,
        }
    }

    pub fn is_closed(&self) -> bool {
        self.0.closed.load(Ordering::Relaxed)
    }

    /// Envoie un paquet principal déjà construit (chiffré si la session est prête).
    pub fn send_raw(&self, plain: Vec<u8>) {
        let ready = self.0.crypto.lock().unwrap().ready;
        self.send_plain(plain, ready);
    }

    /// Construit et envoie un paquet principal selon un schéma.
    pub fn send(&self, schema: &Schema, fields: Fields) {
        self.send_raw(build_packet(schema, &fields));
    }

    fn send_plain(&self, plain: Vec<u8>, encrypt: bool) {
        if self.is_closed() {
            return;
        }
        if self.0.server == "world" {
            crate::probe::outgoing(self.0.id, &plain);
        }
        if crate::log::trace_enabled() {
            if let Some(h) = parse_header(&plain) {
                debug!("[{}] #{} → type=0x{:x} {} o{}", self.0.server, self.0.id, h.ptype, plain.len(), if encrypt { " (chiffré)" } else { "" });
            }
        }
        // Le compteur de trames et l'ordre d'envoi sont protégés par le même verrou.
        let mut crypto = self.0.crypto.lock().unwrap();
        let out = if encrypt {
            match crypto.encode_frame(&plain) {
                Ok(f) => f,
                Err(e) => {
                    warn!("[{}] #{} chiffrement : {e}", self.0.server, self.0.id);
                    return;
                }
            }
        } else {
            plain
        };
        let _ = self.0.tx.send(Out::Bytes(out));
    }

    pub fn close(&self) {
        let _ = self.0.tx.send(Out::Close);
    }
}

/// État propre à une connexion ; les paquets sont traités un à un, dans l'ordre d'arrivée.
#[async_trait]
pub trait Connection: Send {
    async fn on_packet(&mut self, session: &SessionHandle, packet: Packet) -> anyhow::Result<()>;
    async fn on_close(&mut self, _session: &SessionHandle) {}
    /// Inondation de paquets : la connexion va être fermée (`ignored` paquets ignorés).
    async fn on_flood(&mut self, _session: &SessionHandle, _ignored: u64) {}
}

/// Service : crée l'état de chaque connexion et déclare les types de paquets qu'il traite.
pub trait Service: Send + Sync + 'static {
    fn handles(&self, ptype: u8) -> bool;
    fn connect(&self, session: &SessionHandle) -> Box<dyn Connection>;
}

/// Ouvre le port et accepte les connexions en tâche de fond.
pub async fn serve(name: &'static str, listen: &Listen, service: Arc<dyn Service>) -> anyhow::Result<()> {
    let listener = TcpListener::bind((listen.bind.as_str(), listen.port))
        .await
        .map_err(|e| anyhow::anyhow!("[{name}] écoute sur {}:{} impossible : {e}", listen.bind, listen.port))?;
    info!("[{name}] en écoute sur {}:{}", listen.bind, listen.port);
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((socket, addr)) => {
                    let _ = socket.set_nodelay(true);
                    tokio::spawn(run_session(name, socket, addr.to_string(), addr.ip().to_string(), service.clone()));
                }
                Err(e) => warn!("[{name}] accept : {e}"),
            }
        }
    });
    Ok(())
}

async fn run_session(name: &'static str, socket: TcpStream, remote: String, ip: String, service: Arc<dyn Service>) {
    let (mut reader, mut writer) = socket.into_split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Out>();
    let session = SessionHandle(Arc::new(SessionInner {
        id: NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed),
        server: name,
        remote,
        ip,
        crypto: Mutex::new(CryptoSession::default()),
        tx,
        closed: AtomicBool::new(false),
        ext: Mutex::new(None),
    }));
    let id = session.id();
    info!("[{name}] #{id} connecté depuis {}", session.0.remote);

    let writer_session = session.clone();
    let writer_task = tokio::spawn(async move {
        while let Some(out) = rx.recv().await {
            match out {
                Out::Bytes(b) => {
                    if let Err(e) = writer.write_all(&b).await {
                        warn!("[{}] #{} erreur socket : {e}", writer_session.0.server, writer_session.0.id);
                        break;
                    }
                }
                Out::Close => break,
            }
        }
        writer_session.0.closed.store(true, Ordering::Relaxed);
        let _ = writer.shutdown().await;
    });

    let mut conn = service.connect(&session);
    let mut splitter = FrameSplitter::default();
    let mut buf = vec![0u8; 64 * 1024];
    let mut budget = PacketBudget::new(PACKET_BURST.load(Ordering::Relaxed), Instant::now());
    'read: loop {
        let n = tokio::select! {
            r = reader.read(&mut buf) => match r {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => { warn!("[{name}] #{id} erreur socket : {e}"); break; }
            },
            _ = wait_closed(&session) => break,
        };
        let frames = match splitter.push(&buf[..n]) {
            Ok(f) => f,
            Err(e) => {
                warn!("[{name}] #{id} flux invalide : {e} — {}", hex_dump(&buf[..n], 96));
                break;
            }
        };
        for frame in frames {
            let plain = if frame.encrypted {
                match session.0.crypto.lock().unwrap().decode_frame(&frame.data) {
                    Ok(p) => p,
                    Err(e) => {
                        warn!("[{name}] #{id} déchiffrement : {e} — {}", hex_dump(&frame.data, 96));
                        continue;
                    }
                }
            } else {
                frame.data
            };
            let Some(header) = parse_header(&plain) else {
                warn!("[{name}] #{id} paquet sans en-tête valide — {}", hex_dump(&plain, 96));
                continue;
            };
            // Débit permis par connexion : au-delà, le paquet est ignoré ; une inondation qui dure ferme la connexion.
            match budget.admit(Instant::now(), PACKETS_PER_SEC.load(Ordering::Relaxed), PACKET_BURST.load(Ordering::Relaxed)) {
                Admit::Pass => {}
                Admit::Drop => {
                    if budget.flood_started() {
                        warn!("[{name}] #{id} plus de {} paquets par seconde : le surplus est ignoré", PACKETS_PER_SEC.load(Ordering::Relaxed));
                    }
                    continue;
                }
                Admit::Flood => {
                    warn!("[{name}] #{id} inondation de paquets ({} ignorés) : connexion fermée", budget.ignored);
                    conn.on_flood(&session, budget.ignored).await;
                    break 'read;
                }
            }
            if header.ptype == schema::ptype::STARTUP_ENCRYPTION {
                if !startup_encryption(&session, &plain) {
                    break 'read;
                }
                continue;
            }
            if header.ptype == schema::ptype::ARCHCORE_EXT {
                match decode_packet(&schema::ARCHCORE_EXT, &plain) {
                    Ok(p) if p.int(0) == Some(ext_op::HELLO) => {
                        extension_hello(&session, &p);
                        continue;
                    }
                    Ok(p) if session.extension().is_none() => {
                        warn!("[{name}] #{id} paquet d'extension op {} sans négociation, ignoré", p.int(0).unwrap_or(-1));
                        continue;
                    }
                    _ => {}
                }
            }
            let schema = schema::main_by_id(header.ptype);
            let (Some(schema), true) = (schema, service.handles(header.ptype)) else {
                warn!("[{name}] #{id} type 0x{:x} non géré ({} o) — {}", header.ptype, plain.len(), hex_dump(&plain, 96));
                continue;
            };
            let packet = match decode_packet(schema, &plain) {
                Ok(p) => p,
                Err(e) => {
                    warn!("[{name}] #{id} décodage {} : {e} — {}", schema.name, hex_dump(&plain, 96));
                    continue;
                }
            };
            debug!("[{name}] #{id} ← {} op={:?} flags=0x{:x}", schema.name, packet.int(0), packet.flags);
            if name == "world" {
                crate::probe::incoming(id, header.ptype, &packet);
            }
            if let Err(e) = conn.on_packet(&session, packet).await {
                crate::error!("[{name}] #{id} gestionnaire : {e:#}");
            }
            if session.is_closed() {
                break 'read;
            }
        }
    }
    session.0.closed.store(true, Ordering::Relaxed);
    session.close();
    info!(
        "[{name}] #{id} déconnecté ({}) : {} paquet(s), pic {}/s{}",
        session.0.remote,
        budget.total,
        budget.peak,
        if budget.ignored > 0 { format!(", {} ignoré(s)", budget.ignored) } else { String::new() }
    );
    conn.on_close(&session).await;
    let _ = writer_task.await;
}

async fn wait_closed(session: &SessionHandle) {
    while !session.is_closed() {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

/// HELLO d'extension : enregistre l'accord et répond HELLO_ACK (une seule fois par connexion).
fn extension_hello(session: &SessionHandle, packet: &Packet) {
    let (name, id) = (session.0.server, session.0.id);
    let mut slot = session.0.ext.lock().unwrap();
    if slot.is_some() {
        warn!("[{name}] #{id} second HELLO d'extension ignoré");
        return;
    }
    let (agreement, ack) = crate::ext::hello(packet);
    info!(
        "[{name}] #{id} extensions : {} → {}{}",
        if agreement.software.is_empty() { "client inconnu" } else { agreement.software.as_str() },
        if agreement.capabilities.is_empty() { "aucune capacité".to_string() } else { crate::ext::format_capabilities(&agreement.capabilities) },
        agreement.language.as_deref().map(|l| format!(", langue {l}")).unwrap_or_default()
    );
    *slot = Some(agreement);
    drop(slot);
    session.send(&schema::ARCHCORE_EXT, ack);
}

/// Traite un paquet AGPMSTARTUPENCRYPTION ; rend `false` si la connexion doit être fermée.
fn startup_encryption(session: &SessionHandle, plain: &[u8]) -> bool {
    let (name, id) = (session.0.server, session.0.id);
    let packet = match decode_packet(&schema::STARTUP_ENCRYPTION, plain) {
        Ok(p) => p,
        Err(e) => {
            warn!("[{name}] #{id} décodage AGPMSTARTUPENCRYPTION : {e}");
            return true;
        }
    };
    let s = &schema::STARTUP_ENCRYPTION;
    match packet.int(0).unwrap_or(-1) {
        crypt_op::REQUEST => {
            session.send_plain(build_packet(s, &Fields::new(s).with(0, crypt_op::SET_ALGO).with(1, Vec::<u8>::new())), false);
            let mut key = vec![0u8; 32];
            rand::thread_rng().fill_bytes(&mut key);
            session.0.crypto.lock().unwrap().set_server_key(&key).expect("clé de 32 octets");
            session.send_plain(build_packet(s, &Fields::new(s).with(0, crypt_op::PUBLIC_KEY).with(1, key)), false);
            true
        }
        crypt_op::PRIVATE_KEY => {
            let block = packet.bytes(1).unwrap_or_default().to_vec();
            if block.len() != 32 {
                warn!("[{name}] #{id} clé client invalide ({} octets)", block.len());
                return false;
            }
            let mut crypto = session.0.crypto.lock().unwrap();
            let Some(server) = crypto.server_cipher.clone() else {
                warn!("[{name}] #{id} clé client reçue avant la clé serveur");
                return false;
            };
            let mut client_key = block;
            server.decrypt_le(&mut client_key).expect("32 octets");
            crypto.set_client_key(&client_key).expect("clé de 32 octets");
            crypto.ready = true;
            drop(crypto);
            session.send_plain(build_packet(s, &Fields::new(s).with(0, crypt_op::COMPLETE)), true);
            info!("[{name}] #{id} chiffrement Blowfish établi");
            true
        }
        op => {
            warn!("[{name}] #{id} opération de chiffrement inconnue {op}");
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debit_des_paquets() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        // Sans limite : tout passe, le pic est mesuré.
        let mut b = PacketBudget::new(0, t0);
        for _ in 0..500 {
            assert_eq!(b.admit(t0, 0, 0), Admit::Pass);
        }
        assert_eq!((b.peak, b.ignored), (500, 0));
        // 100 par seconde, rafale de 300 : la rafale passe, puis le surplus est ignoré.
        let mut b = PacketBudget::new(300, t0);
        assert!((0..300).all(|_| b.admit(t0, 100, 300) == Admit::Pass));
        assert_eq!(b.admit(t0, 100, 300), Admit::Drop);
        assert!(b.flood_started());
        // Au débit permis, tout passe de nouveau et l'inondation s'oublie après la pause.
        assert!((1..=100).all(|i| b.admit(at(2000 + i * 10), 100, 300) == Admit::Pass));
        assert_eq!(b.admit(at(3010), 100, 300), Admit::Pass);
        // Inondation qui dure : une seconde de débit ignorée ferme la connexion.
        let mut b = PacketBudget::new(300, t0);
        let verdicts: Vec<Admit> = (0..1000).map(|i| b.admit(at(i), 100, 300)).collect();
        let flood = verdicts.iter().position(|v| *v == Admit::Flood).expect("inondation");
        assert!((390..=520).contains(&flood), "fermée au paquet {flood}");
    }
}
