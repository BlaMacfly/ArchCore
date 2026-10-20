//! Connexion TCP à un serveur ArchLord (login ou monde) : découpage des trames, chiffrement,
//! poignée de main et file d'attente de paquets décodés.

use archrust_protocol::framing::{CryptoSession, Direction, FrameSplitter};
use archrust_protocol::handshake::{ClientHandshake, ClientState};
use archrust_protocol::packet::{
    build_packet, decode_packet, hex_dump, parse_header, Packet, PacketError, Value,
};
use archrust_protocol::schema::{main, packet_type, Schema};
use crate::ext::{self, ExtMessage, Extensions};
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("E/S : {0}")]
    Io(#[from] std::io::Error),
    #[error("trame : {0}")]
    Frame(#[from] archrust_protocol::framing::FrameError),
    #[error("paquet : {0}")]
    Packet(#[from] PacketError),
    #[error("poignée de main : {0}")]
    Handshake(#[from] archrust_protocol::handshake::HandshakeError),
    #[error("connexion fermée par le serveur")]
    Closed,
    #[error("attente dépassée : {0}")]
    Timeout(String),
    #[error("serveur : {0}")]
    Server(String),
}

/// Paquet reçu : décodé si son type est connu, sinon brut.
#[derive(Debug, Clone)]
pub struct Incoming {
    pub packet_type: u8,
    pub packet: Option<Packet>,
    pub raw: Vec<u8>,
}

impl Incoming {
    pub fn op(&self) -> Option<i64> {
        self.packet.as_ref().and_then(Packet::op)
    }
    pub fn get(&self, i: usize) -> Option<&Value> {
        self.packet.as_ref().and_then(|p| p.get(i))
    }
    pub fn is(&self, packet_type: u8, op: Option<i64>) -> bool {
        self.packet_type == packet_type && op.map(|o| self.op() == Some(o)).unwrap_or(true)
    }
}

pub struct Connection {
    pub label: &'static str,
    stream: TcpStream,
    splitter: FrameSplitter,
    pub crypto: CryptoSession,
    queue: VecDeque<Incoming>,
    pub timeout: Duration,
    read_buf: Vec<u8>,
    /// Négocier les extensions ArchRust (`HELLO` après la poignée de main) ; désactivable par la
    /// variable d'environnement `ARCHRUST_NO_EXTENSIONS=1`.
    pub ext_enabled: bool,
    /// Langue souhaitée, annoncée au `HELLO` (capacité `language/1`) ; à régler avant `handshake`.
    pub language: Option<String>,
    /// Accord conclu (`HELLO_ACK`), `None` tant qu'il n'est pas reçu ou si le serveur n'en a pas.
    pub extensions: Option<Extensions>,
    ext_deadline: Option<Instant>,
    ext_fresh: bool,
}

impl Connection {
    pub async fn connect(label: &'static str, host: &str, port: u16) -> Result<Self, NetError> {
        let stream = TcpStream::connect((host, port)).await?;
        stream.set_nodelay(true)?;
        log::info!("[{label}] connecté à {host}:{port}");
        Ok(Self {
            label,
            stream,
            splitter: FrameSplitter::new(),
            crypto: CryptoSession::new(),
            queue: VecDeque::new(),
            timeout: Duration::from_secs(8),
            read_buf: vec![0u8; 64 * 1024],
            ext_enabled: std::env::var_os("ARCHRUST_NO_EXTENSIONS").is_none(),
            language: None,
            extensions: None,
            ext_deadline: None,
            ext_fresh: false,
        })
    }

    /// Envoie un paquet principal déjà construit, chiffré si la session est prête.
    pub async fn send_raw(&mut self, plain: &[u8]) -> Result<(), NetError> {
        let out = if self.crypto.ready {
            self.crypto.encode_frame(plain, Direction::ClientToServer)?
        } else {
            plain.to_vec()
        };
        if log::log_enabled!(log::Level::Trace) {
            if let Some(h) = parse_header(plain) {
                log::trace!(
                    "[{}] → type 0x{:02x} {} o{}",
                    self.label,
                    h.packet_type,
                    plain.len(),
                    if self.crypto.ready { " (chiffré)" } else { "" }
                );
            }
        }
        self.stream.write_all(&out).await?;
        Ok(())
    }

    /// Construit et envoie un paquet selon un schéma.
    pub async fn send(
        &mut self,
        schema: &Schema,
        values: &[Option<Value>],
    ) -> Result<(), NetError> {
        let plain = build_packet(schema, values)?;
        log::debug!(
            "[{}] → {} op={:?}",
            self.label,
            schema.name,
            values
                .first()
                .and_then(|v| v.as_ref())
                .and_then(Value::as_i64)
        );
        self.send_raw(&plain).await
    }

    async fn fill(&mut self) -> Result<(), NetError> {
        let n = self.stream.read(&mut self.read_buf).await?;
        if n == 0 {
            return Err(NetError::Closed);
        }
        let chunk = self.read_buf[..n].to_vec();
        for frame in self.splitter.push(&chunk)? {
            let plain = if frame.encrypted {
                self.crypto.decode_frame(&frame.data)?
            } else {
                frame.data
            };
            let Some(h) = parse_header(&plain) else {
                log::warn!(
                    "[{}] paquet sans en-tête : {}",
                    self.label,
                    hex_dump(&plain, 48)
                );
                continue;
            };
            if h.packet_type == ext::PACKET_TYPE {
                if let Some(ExtMessage::HelloAck(e)) = ext::decode(&plain) {
                    self.on_hello_ack(e);
                    continue;
                }
            }
            let packet = match main::by_id(h.packet_type) {
                Some(schema) => match decode_packet(schema, &plain) {
                    Ok(p) => Some(p),
                    Err(e) => {
                        log::warn!(
                            "[{}] décodage {} : {e} — {}",
                            self.label,
                            schema.name,
                            hex_dump(&plain, 64)
                        );
                        None
                    }
                },
                None => None,
            };
            log::debug!(
                "[{}] ← type 0x{:02x} op={:?} ({} o)",
                self.label,
                h.packet_type,
                packet.as_ref().and_then(Packet::op),
                plain.len()
            );
            self.queue.push_back(Incoming {
                packet_type: h.packet_type,
                packet,
                raw: plain,
            });
        }
        Ok(())
    }

    /// Prochain paquet, quel qu'il soit.
    pub async fn next(&mut self) -> Result<Incoming, NetError> {
        loop {
            if let Some(p) = self.queue.pop_front() {
                return Ok(p);
            }
            self.fill().await?;
        }
    }

    /// Prochain paquet sans bloquer au-delà de `wait`.
    pub async fn next_timeout(&mut self, wait: Duration) -> Result<Option<Incoming>, NetError> {
        match tokio::time::timeout(wait, self.next()).await {
            Ok(r) => r.map(Some),
            Err(_) => Ok(None),
        }
    }

    /// Attend le premier paquet satisfaisant `pred` ; les autres sont consommés et journalisés.
    pub async fn expect_where<F: Fn(&Incoming) -> bool>(
        &mut self,
        what: &str,
        pred: F,
    ) -> Result<Incoming, NetError> {
        let deadline = tokio::time::Instant::now() + self.timeout;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(NetError::Timeout(what.to_string()));
            }
            let p = match tokio::time::timeout(remaining, self.next()).await {
                Ok(r) => r?,
                Err(_) => return Err(NetError::Timeout(what.to_string())),
            };
            if pred(&p) {
                return Ok(p);
            }
            if p.packet_type == packet_type::RETURN_TO_LOGIN {
                return Err(NetError::Server(
                    "retour à l'écran de connexion demandé par le serveur".into(),
                ));
            }
            log::debug!(
                "[{}] ignoré en attendant {what} : type 0x{:02x} op={:?}",
                self.label,
                p.packet_type,
                p.op()
            );
        }
    }

    /// Attend un paquet d'un type (et d'une opération) donnés.
    pub async fn expect(&mut self, packet_type: u8, op: Option<i64>) -> Result<Incoming, NetError> {
        let what = format!("type 0x{packet_type:02x} op {op:?}");
        self.expect_where(&what, |p| p.is(packet_type, op)).await
    }

    /// Poignée de main Blowfish (type 0x48).
    pub async fn handshake(&mut self) -> Result<(), NetError> {
        let mut hs = ClientHandshake::new();
        let req = hs.start();
        self.send_raw(&req).await?;
        while hs.state != ClientState::Done {
            let p = self.expect(packet_type::STARTUP_ENCRYPTION, None).await?;
            let out = hs.on_packet(&mut self.crypto, &p.raw)?;
            for plain in out.plain {
                self.send_raw(&plain).await?;
            }
        }
        log::info!("[{}] chiffrement Blowfish établi", self.label);
        if self.ext_enabled {
            // Extensions ArchRust : HELLO sur chaque connexion, juste après 0x48. On n'attend pas la
            // réponse : elle est interceptée à la réception (3 s au plus).
            self.send_raw(&ext::hello_packet(self.language.as_deref())).await?;
            self.ext_deadline = Some(Instant::now() + ext::HELLO_TIMEOUT);
            log::debug!(
                "[{}] extensions : HELLO {} ({}){}",
                self.label,
                ext::software(),
                ext::REQUESTED,
                self.language.as_deref().map(|l| format!(", langue {l}")).unwrap_or_default()
            );
        }
        Ok(())
    }

    fn on_hello_ack(&mut self, e: Extensions) {
        let in_time = self.ext_deadline.is_some_and(|d| Instant::now() <= d);
        if self.extensions.is_some() || !in_time {
            log::warn!("[{}] HELLO_ACK inattendu ou tardif ignoré ({})", self.label, e.software);
            return;
        }
        log::info!(
            "[{}] extensions : {} {}.{} → {:?} (charge {} o, {} d'un coup puis {}/s){}",
            self.label,
            e.software,
            e.major,
            e.minor,
            e.capabilities,
            e.max_payload,
            e.rate_burst,
            e.rate_per_sec,
            e.language
                .as_deref()
                .map(|l| format!(" ; langue {l} parmi {}", e.languages.join(", ")))
                .unwrap_or_default()
        );
        self.extensions = Some(e);
        self.ext_fresh = true;
    }

    /// Accord d'extensions reçu depuis le dernier appel (une seule fois par connexion).
    pub fn take_new_extensions(&mut self) -> Option<Extensions> {
        if std::mem::take(&mut self.ext_fresh) {
            self.extensions.clone()
        } else {
            None
        }
    }

    /// La négociation est terminée : accord reçu, ou délai de 3 s écoulé.
    pub fn extensions_settled(&self) -> bool {
        self.extensions.is_some() || self.ext_deadline.is_none_or(|d| Instant::now() > d)
    }

    pub async fn close(mut self) {
        let _ = self.stream.shutdown().await;
    }
}
