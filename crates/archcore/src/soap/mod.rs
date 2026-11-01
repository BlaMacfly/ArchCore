//! Interface SOAP d'administration, compatible avec celle d'AzerothCore et de TrinityCore (`SOAP.*` dans world.conf).
//!
//! ```text
//!   POST http://127.0.0.1:7878/   Authorization: Basic <compte:mot de passe>
//!   <SOAP-ENV:Envelope …><SOAP-ENV:Body><ns1:executeCommand><command>server info</command></ns1:executeCommand>…
//!     → 200 <ns1:executeCommandResponse><result>texte de la commande</result>
//!     → 500 <SOAP-ENV:Fault> faultcode SOAP-ENV:Client, faultstring = message (commande refusée ou en échec)
//!     → 401 compte inconnu ou banni, mauvais mot de passe ; 403 niveau MJ sous SOAP.MinGmLevel
//! ```
//! Comme gSOAP chez eux : une requête par connexion, réponse puis fermeture ; `ns1` reprend l'espace de noms de la
//! requête (`urn:AC`, `urn:TC`, `urn:ArchCore`…) ; chaque ligne du résultat finit par « \r\n » (`&#xD;` dans le XML),
//! comme la sortie de leur console. Le texte de la commande est exécuté par un `Backend` : dans le serveur,
//! `world/console.rs` (même interpréteur que les commandes MJ du jeu, sous le verrou du monde).
//!
//! Sécurité : les mots de passe passent en clair dans l'en-tête Basic. N'exposer qu'en local (défaut 127.0.0.1) ou
//! derrière un pare-feu ou un tunnel.

pub mod http;
pub mod xml;

use crate::{info, warn};
use async_trait::async_trait;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};

/// Compte authentifié qui envoie une commande.
#[derive(Clone, Debug, PartialEq)]
pub struct Caller {
    pub account_id: u32,
    pub username: String,
    pub gm_level: i64,
    /// Adresse du client SOAP.
    pub ip: String,
}

/// Résultat de la vérification des identifiants.
#[derive(Clone, Debug, PartialEq)]
pub enum Auth {
    Unknown,
    BadPassword,
    Banned,
    Granted { account_id: u32, username: String, gm_level: i64 },
}

/// Ce que l'interface SOAP demande au serveur : vérifier un compte, exécuter une commande.
#[async_trait]
pub trait Backend: Send + Sync + 'static {
    async fn authenticate(&self, username: &str, password: &str) -> anyhow::Result<Auth>;
    /// Texte de la commande réussie, ou message d'échec (rendu en `SOAP-ENV:Fault`).
    async fn execute(&self, caller: &Caller, command: &str) -> Result<String, String>;
}

/// Bornes d'une connexion.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Taille maximale du corps de la requête.
    pub max_body: usize,
    /// Délai pour recevoir la requête entière (en-têtes et corps).
    pub read_timeout: Duration,
    /// Délai pour envoyer la réponse.
    pub write_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_body: 64 * 1024, read_timeout: Duration::from_secs(10), write_timeout: Duration::from_secs(10) }
    }
}

/// Commande sans ses mots de passe, pour les journaux : `account create <nom> ***`, `account set password <nom> *** ***`,
/// `account password *** *** ***`. Les autres sont rendues telles quelles.
pub fn mask_secrets(command: &str) -> String {
    let words: Vec<&str> = command.split_whitespace().collect();
    let lower: Vec<String> = words.iter().map(|w| w.trim_start_matches(['.', '/']).to_ascii_lowercase()).collect();
    let l: Vec<&str> = lower.iter().map(String::as_str).collect();
    let from = match l.as_slice() {
        ["account", "create", ..] => 3,
        ["account", "set", "password", ..] => 4,
        ["account", "password", ..] => 2,
        _ => return command.trim().to_string(),
    };
    let to = if from == 3 { 4 } else { words.len() };
    words.iter().enumerate().map(|(i, w)| if i >= from && i < to { "***" } else { w }).collect::<Vec<_>>().join(" ")
}

/// Sortie d'une commande au format de la console d'AzerothCore : chaque ligne suivie de « \r\n ».
pub fn cli_text(text: &str) -> String {
    text.lines().map(|l| format!("{l}\r\n")).collect()
}

fn summary(text: &str) -> String {
    let first = text.lines().next().unwrap_or("");
    let more = text.lines().count().saturating_sub(1);
    let mut s: String = first.chars().take(160).collect();
    if first.chars().count() > 160 {
        s.push('…');
    }
    if more > 0 {
        s.push_str(&format!(" (+{more} ligne(s))"));
    }
    s
}

const XML: &str = "text/xml; charset=utf-8";

fn fault_response(status: u16, ns: &xml::Namespaces, code: &str, message: &str, detail: Option<&str>) -> Vec<u8> {
    let extra: &[(&str, &str)] = if status == 401 { &[("WWW-Authenticate", "Basic realm=\"gSOAP Web Service\"")] } else { &[] };
    http::response(status, extra, XML, xml::fault(ns, code, message, detail).as_bytes())
}

/// Erreur HTTP sans enveloppe exploitable (statut d'AzerothCore / gSOAP, message dans un Fault).
fn http_error(status: u16, ns: &xml::Namespaces) -> Vec<u8> {
    fault_response(status, ns, "SOAP-ENV:Client", &format!("HTTP Error: {status} {}", http::reason(status)), None)
}

/// Traite une requête reçue : authentification, enveloppe, exécution ; rend la réponse HTTP complète.
pub async fn process(req: &http::Request, ip: &str, backend: Arc<dyn Backend>, min_level: i64) -> Vec<u8> {
    let default_ns = xml::Namespaces::default();
    crate::debug!("[soap] {ip} : {} {} ({} octet(s))", req.method, req.target, req.body.len());
    if !req.method.eq_ignore_ascii_case("POST") {
        return fault_response(405, &default_ns, "SOAP-ENV:Client", "HTTP Error: 405 Method Not Allowed", None);
    }
    let charset_1252 = req.header("Content-Type").is_some_and(|t| {
        let t = t.to_ascii_lowercase();
        t.contains("iso-8859-1") || t.contains("windows-1252") || t.contains("latin1")
    });
    let text = if charset_1252 { encoding_rs::WINDOWS_1252.decode(&req.body).0.into_owned() } else { http::text_of(&req.body) };
    // Lu avant l'authentification, comme gSOAP, pour répondre dans l'espace de noms du client ; rien n'est exécuté avant.
    let parsed = xml::parse_request(&text);
    let ns = parsed.as_ref().map(xml::Namespaces::of).unwrap_or_default();

    let Some((user, password)) = http::basic_auth(req.header("Authorization")) else {
        info!("[soap] {ip} : requête sans identifiants");
        return http_error(401, &ns);
    };
    let auth = match backend.authenticate(&user, &password).await {
        Ok(a) => a,
        Err(e) => {
            warn!("[soap] vérification du compte {user:?} : {e:#}");
            return fault_response(500, &ns, "SOAP-ENV:Server", "Erreur du serveur", None);
        }
    };
    let caller = match auth {
        Auth::Unknown => {
            info!("[soap] {ip} : compte inconnu {user:?}");
            return http_error(401, &ns);
        }
        Auth::BadPassword => {
            info!("[soap] {ip} : mauvais mot de passe pour {user:?}");
            return http_error(401, &ns);
        }
        Auth::Banned => {
            info!("[soap] {ip} : compte banni {user:?}");
            return http_error(401, &ns);
        }
        Auth::Granted { gm_level, ref username, .. } if gm_level < min_level => {
            info!("[soap] {ip} : niveau MJ de {username} trop bas ({gm_level}, {min_level} requis)");
            return http_error(403, &ns);
        }
        Auth::Granted { account_id, username, gm_level } => Caller { account_id, username, gm_level, ip: ip.to_string() },
    };
    let call = match parsed {
        Ok(c) => c,
        Err(e) => {
            info!("[soap] {} ({ip}) : {}", caller.username, e.message());
            return fault_response(500, &ns, "SOAP-ENV:Client", &e.message(), None);
        }
    };
    let command = call.command.as_deref().map(str::trim).unwrap_or("");
    if command.is_empty() {
        return fault_response(500, &ns, "SOAP-ENV:Client", "Command can not be empty", Some("The supplied command was an empty string"));
    }
    let shown = mask_secrets(command);
    info!("[soap] {} ({ip}, MJ {}) : {shown}", caller.username, caller.gm_level);
    // Tâche à part : une commande commencée va à son terme même si le client se déconnecte, et une panique devient un
    // Fault au lieu de couper la connexion sans réponse.
    let (who, cmd) = (caller.clone(), command.to_string());
    let outcome = match tokio::spawn(async move { backend.execute(&who, &cmd).await }).await {
        Ok(r) => r,
        Err(e) => {
            warn!("[soap] {} : « {shown} » interrompue : {e}", caller.username);
            return fault_response(500, &ns, "SOAP-ENV:Server", "Erreur interne du serveur", None);
        }
    };
    match outcome {
        Ok(out) => {
            info!("[soap] {} : « {shown} » réussie : {}", caller.username, summary(&out));
            http::response(200, &[], XML, xml::response(&ns, &cli_text(&out)).as_bytes())
        }
        Err(msg) => {
            info!("[soap] {} : « {shown} » en échec : {}", caller.username, summary(&msg));
            let text = cli_text(&msg);
            fault_response(500, &ns, "SOAP-ENV:Client", &text, Some(&text))
        }
    }
}

async fn handle(mut socket: TcpStream, ip: String, backend: Arc<dyn Backend>, min_level: i64, limits: Limits) {
    let reply = match tokio::time::timeout(limits.read_timeout, http::read_request(&mut socket, limits.max_body)).await {
        Err(_) => {
            crate::debug!("[soap] {ip} : délai de lecture dépassé");
            Some(http_error(408, &xml::Namespaces::default()))
        }
        Ok(Err(e)) => {
            crate::debug!("[soap] {ip} : {}", e.message());
            e.status().map(|s| http_error(s, &xml::Namespaces::default()))
        }
        Ok(Ok(req)) => Some(process(&req, &ip, backend, min_level).await),
    };
    if let Some(bytes) = reply {
        let _ = tokio::time::timeout(limits.write_timeout, async {
            socket.write_all(&bytes).await?;
            socket.shutdown().await
        })
        .await;
    }
}

/// Démarre l'écoute SOAP de la configuration ; rend l'adresse effective.
pub async fn serve(cfg: &crate::config::SoapConfig, backend: Arc<dyn Backend>) -> anyhow::Result<SocketAddr> {
    serve_with(&cfg.ip, cfg.port, cfg.min_gm_level, Limits::default(), backend).await
}

/// Démarre l'écoute sur `ip:port` (port 0 : port libre, pour les essais).
pub async fn serve_with(ip: &str, port: u16, min_level: i64, limits: Limits, backend: Arc<dyn Backend>) -> anyhow::Result<SocketAddr> {
    let listener = TcpListener::bind((ip, port)).await.map_err(|e| anyhow::anyhow!("[soap] écoute sur {ip}:{port} impossible : {e}"))?;
    let addr = listener.local_addr()?;
    info!("[soap] en écoute sur http://{addr} (comptes de niveau MJ {min_level} au moins)");
    if !addr.ip().is_loopback() {
        warn!("[soap] écoute sur {} : les mots de passe passent en clair, n'exposez SOAP que derrière un pare-feu", addr.ip());
    }
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((socket, peer)) => {
                    let _ = socket.set_nodelay(true);
                    tokio::spawn(handle(socket, peer.ip().to_string(), backend.clone(), min_level, limits));
                }
                Err(e) => {
                    warn!("[soap] accept : {e}");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    });
    Ok(addr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    /// Comptes et commandes factices : `admin`/`secret` (MJ 3), `joueur`/`pw` (MJ 0), `banni`/`x`.
    struct Fake;

    #[async_trait]
    impl Backend for Fake {
        async fn authenticate(&self, username: &str, password: &str) -> anyhow::Result<Auth> {
            Ok(match (username, password) {
                ("admin", "secret") => Auth::Granted { account_id: 1, username: "admin".into(), gm_level: 3 },
                ("joueur", "pw") => Auth::Granted { account_id: 2, username: "joueur".into(), gm_level: 0 },
                ("banni", _) => Auth::Banned,
                ("admin" | "joueur", _) => Auth::BadPassword,
                ("panne", _) => anyhow::bail!("base injoignable"),
                _ => Auth::Unknown,
            })
        }
        async fn execute(&self, caller: &Caller, command: &str) -> Result<String, String> {
            match command {
                ".server info" | "server info" => Ok("ArchCore rev. test\nConnected players: 0. Characters in world: 0.".into()),
                "panique" => panic!("essai"),
                c if c.starts_with("echo ") => Ok(format!("{} ({}, MJ {}) : {}", &c[5..], caller.username, caller.gm_level, caller.ip)),
                c => Err(format!("Commande inconnue : {c}")),
            }
        }
    }

    fn envelope(ns: &str, command: &str) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<SOAP-ENV:Envelope xmlns:SOAP-ENV=\"http://schemas.xmlsoap.org/soap/envelope/\" xmlns:SOAP-ENC=\"http://schemas.xmlsoap.org/soap/encoding/\" xmlns:xsi=\"http://www.w3.org/1999/XMLSchema-instance\" xmlns:xsd=\"http://www.w3.org/1999/XMLSchema\" xmlns:ns1=\"{ns}\"><SOAP-ENV:Body><ns1:executeCommand><command>{command}</command></ns1:executeCommand></SOAP-ENV:Body></SOAP-ENV:Envelope>"
        )
    }

    fn post(auth: Option<&str>, body: &str) -> String {
        let auth = auth.map(|a| format!("Authorization: Basic {a}\r\n")).unwrap_or_default();
        format!("POST / HTTP/1.1\r\nHost: localhost:7878\r\nContent-Type: text/xml; charset=utf-8\r\nSOAPAction: \"\"\r\n{auth}Content-Length: {}\r\n\r\n{body}", body.len())
    }

    async fn call(addr: SocketAddr, raw: &[u8]) -> (u16, String) {
        let mut s = TcpStream::connect(addr).await.unwrap();
        s.write_all(raw).await.unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).await.unwrap();
        let text = String::from_utf8(out).unwrap();
        let status = text.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        (status, text)
    }

    const ADMIN: &str = "YWRtaW46c2VjcmV0"; // admin:secret

    #[test]
    fn mots_de_passe_masques() {
        assert_eq!(mask_secrets("account create Bob s3cret bob@x.fr"), "account create Bob *** bob@x.fr");
        assert_eq!(mask_secrets(".Account Set Password Bob a b"), ".Account Set Password Bob *** ***");
        assert_eq!(mask_secrets("account password vieux neuf neuf"), "account password *** *** ***");
        assert_eq!(mask_secrets("  server info "), "server info");
        assert_eq!(cli_text("a\nb"), "a\r\nb\r\n");
        assert_eq!(cli_text(""), "");
    }

    /// L'écouteur sur un port libre, appelé en HTTP comme le ferait un panneau AzerothCore ou TrinityCore.
    #[tokio::test]
    async fn ecouteur_soap_de_bout_en_bout() {
        let limits = Limits { read_timeout: Duration::from_millis(300), ..Limits::default() };
        let addr = serve_with("127.0.0.1", 0, 3, limits, Arc::new(Fake)).await.unwrap();

        // Réussite, client AzerothCore : ns1 urn:AC, lignes en « &#xD;\n ».
        let (status, r) = call(addr, post(Some(ADMIN), &envelope("urn:AC", ".server info")).as_bytes()).await;
        assert_eq!(status, 200, "{r}");
        assert!(r.contains("Content-Type: text/xml; charset=utf-8\r\n") && r.contains("Connection: close\r\n"));
        assert!(r.contains("xmlns:ns1=\"urn:AC\""));
        assert!(r.contains("<ns1:executeCommandResponse><result>ArchCore rev. test&#xD;\nConnected players: 0. Characters in world: 0.&#xD;\n</result></ns1:executeCommandResponse>"), "{r}");
        let body = &r[r.find("\r\n\r\n").unwrap() + 4..];
        assert!(r.contains(&format!("Content-Length: {}\r\n", body.len())));

        // Client TrinityCore (PHP SoapClient) : ns1 urn:TC repris, entités décodées puis échappées, compte et IP transmis.
        let (status, r) = call(addr, post(Some(ADMIN), &envelope("urn:TC", "echo a &lt;b&gt; &amp; c")).as_bytes()).await;
        assert_eq!(status, 200);
        assert!(r.contains("xmlns:ns1=\"urn:TC\"") && r.contains("<result>a &lt;b&gt; &amp; c (admin, MJ 3) : 127.0.0.1&#xD;\n</result>"), "{r}");

        // Commande en échec : Fault SOAP-ENV:Client, HTTP 500.
        let (status, r) = call(addr, post(Some(ADMIN), &envelope("urn:AC", "inconnue")).as_bytes()).await;
        assert_eq!(status, 500);
        assert!(r.contains("<SOAP-ENV:Fault><faultcode>SOAP-ENV:Client</faultcode><faultstring>Commande inconnue : inconnue&#xD;\n</faultstring><detail>Commande inconnue : inconnue&#xD;\n</detail></SOAP-ENV:Fault>"), "{r}");

        // Commande vide, méthode inconnue, XML invalide.
        let (status, r) = call(addr, post(Some(ADMIN), &envelope("urn:AC", "  ")).as_bytes()).await;
        assert!(status == 500 && r.contains("<faultstring>Command can not be empty</faultstring><detail>The supplied command was an empty string</detail>"), "{r}");
        let other = envelope("urn:AC", "x").replace("executeCommand", "getVersion");
        let (status, r) = call(addr, post(Some(ADMIN), &other).as_bytes()).await;
        assert!(status == 500 && r.contains("Method 'ns1:getVersion' not implemented"), "{r}");
        let (status, _) = call(addr, post(Some(ADMIN), "<pas du xml").as_bytes()).await;
        assert_eq!(status, 500);

        // Identifiants : absents, mauvais, inconnus, banni → 401 ; niveau MJ trop bas → 403 ; base en panne → Fault Server.
        let soap = envelope("urn:AC", "server info");
        let (status, r) = call(addr, post(None, &soap).as_bytes()).await;
        assert!(status == 401 && r.contains("WWW-Authenticate: Basic realm=\"gSOAP Web Service\"\r\n") && r.contains("<faultstring>HTTP Error: 401 Unauthorized</faultstring>"), "{r}");
        for creds in ["admin:faux", "personne:x", "banni:x"] {
            let b64 = base64(creds);
            assert_eq!(call(addr, post(Some(&b64), &soap).as_bytes()).await.0, 401, "{creds}");
        }
        let (status, r) = call(addr, post(Some(&base64("joueur:pw")), &soap).as_bytes()).await;
        assert!(status == 403 && !r.contains("WWW-Authenticate"), "{r}");
        let (status, r) = call(addr, post(Some(&base64("panne:x")), &soap).as_bytes()).await;
        assert!(status == 500 && r.contains("<faultcode>SOAP-ENV:Server</faultcode>"), "{r}");

        // Panique de l'exécuteur : Fault Server, l'écouteur continue.
        let (status, r) = call(addr, post(Some(ADMIN), &envelope("urn:AC", "panique")).as_bytes()).await;
        assert!(status == 500 && r.contains("SOAP-ENV:Server"), "{r}");

        // Corps en chunked, avec Expect: 100-continue (curl au-delà de 1 Ko).
        let body = envelope("urn:ArchCore", "echo bloc");
        let (a, b) = body.split_at(body.len() / 2);
        let raw = format!("POST /soap HTTP/1.1\r\nAuthorization: Basic {ADMIN}\r\nExpect: 100-continue\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{a}\r\n{:x}\r\n{b}\r\n0\r\n\r\n", a.len(), b.len());
        let (status, r) = call(addr, raw.as_bytes()).await;
        assert!(r.starts_with("HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 OK"), "{r}");
        assert!(status == 100 && r.contains("xmlns:ns1=\"urn:ArchCore\"") && r.contains("<result>bloc (admin"), "{r}");

        // Bornes : corps trop long (413), autre méthode (405), requête incomplète (408 après le délai).
        let big = format!("POST / HTTP/1.1\r\nAuthorization: Basic {ADMIN}\r\nContent-Length: {}\r\n\r\n", 64 * 1024 + 1);
        assert_eq!(call(addr, big.as_bytes()).await.0, 413);
        assert_eq!(call(addr, b"GET / HTTP/1.1\r\nHost: x\r\n\r\n").await.0, 405);
        let started = std::time::Instant::now();
        let (status, _) = call(addr, b"POST / HTTP/1.1\r\nContent-Length: 10\r\n\r\nabc").await;
        assert_eq!(status, 408);
        assert!(started.elapsed() >= Duration::from_millis(250));
    }

    fn base64(s: &str) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let b = s.as_bytes();
        let mut out = String::new();
        for c in b.chunks(3) {
            let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
            for i in 0..4 {
                out.push(if i <= c.len() { T[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
            }
        }
        out
    }
}
