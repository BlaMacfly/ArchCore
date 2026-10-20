//! Négociation des extensions ArchRust contre un faux serveur local : avec un serveur qui les gère
//! (`HELLO_ACK` intercepté, sans gêner les autres paquets) et avec un serveur d'avant les extensions
//! (le `HELLO` est ignoré et le client continue sans attendre).

use archcore_net::schema::{ext_op, ARCHCORE_EXT, CONFIG};
use archcore_net::{build_packet, decode_packet, parse_header, Fields};
use archrust_netclient::Connection;
use archrust_protocol::framing::{CryptoSession, Direction, FrameSplitter};
use archrust_protocol::handshake::ServerHandshake;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Faux serveur : poignée de main, puis rend les paquets du client en clair et envoie ce qu'on
/// lui donne (chiffré).
struct FakeServer {
    stream: TcpStream,
    crypto: CryptoSession,
    splitter: FrameSplitter,
    pending: Vec<Vec<u8>>,
}

impl FakeServer {
    async fn accept(listener: &TcpListener) -> Self {
        let (stream, _) = listener.accept().await.unwrap();
        let mut s = FakeServer {
            stream,
            crypto: CryptoSession::new(),
            splitter: FrameSplitter::new(),
            pending: Vec::new(),
        };
        let mut hs = ServerHandshake::default();
        while !hs.done {
            let p = s.recv().await;
            let out = hs.on_packet(&mut s.crypto, &p).unwrap();
            for plain in out.plain {
                s.stream.write_all(&plain).await.unwrap();
            }
            for enc in out.encrypted {
                s.stream.write_all(&enc).await.unwrap();
            }
        }
        s
    }

    async fn recv(&mut self) -> Vec<u8> {
        loop {
            if !self.pending.is_empty() {
                return self.pending.remove(0);
            }
            let mut buf = vec![0u8; 4096];
            let n = self.stream.read(&mut buf).await.unwrap();
            assert!(n > 0, "client parti");
            for f in self.splitter.push(&buf[..n]).unwrap() {
                let plain = if f.encrypted {
                    self.crypto.decode_frame(&f.data).unwrap()
                } else {
                    f.data
                };
                self.pending.push(plain);
            }
        }
    }

    async fn send(&mut self, plain: &[u8]) {
        let f = self
            .crypto
            .encode_frame(plain, Direction::ServerToClient)
            .unwrap();
        self.stream.write_all(&f).await.unwrap();
    }
}

fn game_packet() -> Vec<u8> {
    build_packet(&CONFIG, &Fields::new(&CONFIG).with(0, 1).with(1, 0).with(2, 0))
}

fn hello_ack() -> Vec<u8> {
    let s = &ARCHCORE_EXT;
    build_packet(
        s,
        &Fields::new(s)
            .with(0, ext_op::HELLO_ACK)
            .with(2, 1)
            .with(3, 0)
            .with(4, b"ArchCore test".to_vec())
            .with(5, b"addon-msg/1".to_vec())
            .with(6, 255)
            .with(7, 20)
            .with(8, 10),
    )
}

async fn client_and_server() -> (Connection, FakeServer) {
    client_and_server_lang(None).await
}

async fn client_and_server_lang(language: Option<&str>) -> (Connection, FakeServer) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let language = language.map(str::to_string);
    let (conn, server) = tokio::join!(
        async move {
            let mut c = Connection::connect("test", "127.0.0.1", port).await.unwrap();
            c.ext_enabled = true;
            c.language = language;
            c.handshake().await.unwrap();
            c
        },
        FakeServer::accept(&listener)
    );
    (conn, server)
}

#[tokio::test]
async fn langue_annoncee_et_appliquee() {
    let (mut conn, mut server) = client_and_server_lang(Some("fr")).await;
    let hello = server.recv().await;
    let p = decode_packet(&ARCHCORE_EXT, &hello).unwrap();
    assert_eq!(p.int(0), Some(ext_op::HELLO));
    assert_eq!(p.text(5).as_deref(), Some("addon-msg/1;language/1"));
    assert_eq!(p.text(16).as_deref(), Some("fr"), "langue souhaitée au HELLO");
    let s = &ARCHCORE_EXT;
    let ack = build_packet(
        s,
        &Fields::new(s)
            .with(0, ext_op::HELLO_ACK)
            .with(2, 1)
            .with(3, 1)
            .with(4, b"ArchCore test".to_vec())
            .with(5, b"addon-msg/1;language/1".to_vec())
            .with(6, 255)
            .with(7, 20)
            .with(8, 10)
            .with(16, b"fr".to_vec())
            .with(17, b"en;fr;de".to_vec()),
    );
    server.send(&ack).await;
    server.send(&game_packet()).await;
    conn.next().await.unwrap();
    let e = conn.take_new_extensions().expect("accord reçu");
    assert!(e.has("language"));
    assert_eq!(e.language.as_deref(), Some("fr"));
    assert_eq!(e.languages, vec!["en", "fr", "de"]);
}

#[tokio::test]
async fn serveur_avec_extensions() {
    let (mut conn, mut server) = client_and_server().await;
    // Le client envoie HELLO juste après la poignée de main.
    let hello = server.recv().await;
    assert_eq!(parse_header(&hello).unwrap().ptype, 0xac);
    let p = decode_packet(&ARCHCORE_EXT, &hello).unwrap();
    assert_eq!(p.int(0), Some(ext_op::HELLO));
    assert_eq!(p.text(5).as_deref(), Some("addon-msg/1;language/1"));
    assert!(p.get(16).is_none(), "pas de langue choisie : champ 16 absent");
    // Un paquet de jeu avant l'accord, puis l'accord, puis un autre paquet de jeu.
    server.send(&game_packet()).await;
    server.send(&hello_ack()).await;
    server.send(&game_packet()).await;
    let first = conn.next().await.unwrap();
    assert_eq!(first.packet_type, CONFIG.id);
    let second = conn.next().await.unwrap();
    assert_eq!(second.packet_type, CONFIG.id, "HELLO_ACK est intercepté, pas remis");
    let e = conn.take_new_extensions().expect("accord reçu");
    assert_eq!(e.software, "ArchCore test");
    assert!(e.has("addon-msg"));
    assert_eq!((e.max_payload, e.rate_burst, e.rate_per_sec), (255, 20, 10));
    assert!(conn.take_new_extensions().is_none(), "signalé une seule fois");
    assert!(conn.extensions_settled());
}

#[tokio::test]
async fn serveur_sans_extensions() {
    let (mut conn, mut server) = client_and_server().await;
    let hello = server.recv().await;
    assert_eq!(parse_header(&hello).unwrap().ptype, 0xac);
    // Un serveur d'avant les extensions ignore 0xAC et continue.
    server.send(&game_packet()).await;
    let started = std::time::Instant::now();
    let p = conn.next().await.unwrap();
    assert_eq!(p.packet_type, CONFIG.id);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(1),
        "le client n'attend pas HELLO_ACK"
    );
    assert!(conn.extensions.is_none());
    assert!(!conn.extensions_settled(), "encore dans le délai de 3 s");
    // Un HELLO_ACK hors délai est refusé.
    tokio::time::sleep(std::time::Duration::from_millis(3100)).await;
    assert!(conn.extensions_settled());
    server.send(&hello_ack()).await;
    server.send(&game_packet()).await;
    conn.next().await.unwrap();
    assert!(conn.extensions.is_none(), "HELLO_ACK tardif ignoré");
}
