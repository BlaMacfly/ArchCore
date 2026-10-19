//! Client simulé : rejoue le flux d'un client ArchLord contre le login puis le monde, pour valider le
//! serveur sans lancer le jeu.
//!
//! ```text
//!   archcore sim client [compte] [mdp] [perso] [commandes…]   connexion complète (+ commandes de discussion)
//!   archcore sim move [compteA mdpA persoA compteB mdpB persoB] deux joueurs : visibilité et déplacement
//!   archcore sim shop [compte] [mdp] [perso]                  boutique de Darryl : liste, achat, revente
//!   archcore sim addon [A… B… C…]                             extensions ArchRust : messages d'AddOn (A, B : ArchRust ; C : client d'origine, MJ)
//!   archcore sim lang [compte] [mdp] [perso]                  langues : HELLO avec langue, noms de PNJ localisés, SET_LANGUAGE, TEXT_REQUEST, client d'origine (lang.rs)
//!   archcore sim combat [compte] [mdp] [perso] [monstre]      Vers verts au nord d'Anchorville : riposte, poursuite, mort et résurrection,
//!                                                             puis (invincible) victoire, expérience, butin ramassé, réapparition ; compte MJ requis
//!   archcore sim equip [compte] [mdp] [perso]                 équiper épée et armure (dégâts, défense), refus de classe, retrait ; SimA voit l'arme
//!   archcore sim potion [compte] [mdp] [perso]                blessé par un Ver vert : régénération hors combat, potion de vie, refus ; compte MJ
//!   archcore sim scroll [compte] [mdp] [perso]                parchemins : destination fixe, aller-retour, retour au village ; compte MJ
//!   archcore sim patrol [compte] [mdp] [perso]                immobile chez les Vers verts : des monstres se promènent
//!   archcore sim bag [compte] [mdp] [perso]                   séparer, réunir et détruire une pile ; ressusciter au point de la région ; compte MJ
//!   archcore sim skill [compte] [mdp] [perso]                 maître des compétences : apprendre, refus, amélioration ; Dual Slash sur un Ver vert ; compte MJ
//!   archcore sim bank [compte] [mdp] [perso]                  banque chez Holly : dépôt d'objet et d'argent, reconnexion, retrait ; compte MJ
//!   archcore sim trade [A… B…]                                échange GmTest ↔ SimA : objet et argent, verrou, échange ; refus ; A compte MJ
//!   archcore sim quest [compte] [mdp] [perso]                 quêtes : prendre, refus, terminer (objets rapportés, récompense), abandonner ; compte MJ
//!   archcore sim status [compte] [mdp] [perso]                état spécial : Shield Bash étourdit un Ver vert (champ 17), puis l'état retombe ; compte MJ, personnage GmEffets
//!   archcore sim effects [compte] [mdp] [perso]               passive (défense), renforcement (vitesse d'attaque), zone, affaiblissement d'un Ver vert ; compte MJ, personnage GmEffets
//! ```
//! À lancer contre un serveur de test (`ARCHCORE_CONF_DIR`) : les personnages sont créés au besoin,
//! déplacés, et `sim shop` leur donne 1000 pièces d'or.

use crate::config::Config;
use archcore_net::blowfish::Blowfish;
use archcore_net::framing::{FrameSplitter, FRAME_CLIENT, FRAME_HEADER, FRAME_OVERHEAD, FRAME_TAIL_CLIENT};
use archcore_net::schema::{self, char_mgr_op, chat_type, crypt_op, ext_channel, ext_op, ext_result, login_op, mini, world_op};
use archcore_net::{build_mini, build_packet, decode_mini, decode_packet, parse_header, Fields, Packet, Schema, Vec3};
use rand::RngCore;
use std::collections::VecDeque;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

mod lang;

/// Valeur facultative affichée sans « Some(…) ».
fn o<T: std::fmt::Display>(v: Option<T>) -> String {
    v.map(|x| x.to_string()).unwrap_or_else(|| "-".into())
}

pub async fn run(args: &[String]) -> anyhow::Result<()> {
    let a = |i: usize, d: &str| args.get(i).cloned().unwrap_or_else(|| d.to_string());
    match args.first().map(String::as_str) {
        Some("client") => client(&a(1, "test"), &a(2, "test"), &a(3, "Testeur"), args.get(4..).unwrap_or_default()).await,
        Some("move") => movement(&a(1, "sima"), &a(2, "sima"), &a(3, "SimA"), &a(4, "simb"), &a(5, "simb"), &a(6, "SimB")).await,
        Some("shop") => shop(&a(1, "sima"), &a(2, "sima"), &a(3, "SimA")).await,
        Some("addon") => {
            let acc = [
                (a(1, "sima"), a(2, "sima"), a(3, "SimA")),
                (a(4, "simb"), a(5, "simb"), a(6, "SimB")),
                (a(7, "gmtest"), a(8, "gmtest"), a(9, "GmTest")),
            ];
            Box::pin(addon(&acc)).await
        }
        Some("lang") => Box::pin(lang::run(&a(1, "sima"), &a(2, "sima"), &a(3, "SimA"))).await,
        Some("bank") => Box::pin(bank(&a(1, "gmtest"), &a(2, "gmtest"), &a(3, "GmTest"))).await,
        Some("trade") => Box::pin(private_trade(&a(1, "gmtest"), &a(2, "gmtest"), &a(3, "GmTest"), &a(4, "sima"), &a(5, "sima"), &a(6, "SimA"))).await,
        Some("quest") => Box::pin(quest(&a(1, "gmtest"), &a(2, "gmtest"), &a(3, "GmTest"))).await,
        Some("status") => Box::pin(skill_status(&a(1, "gmtest"), &a(2, "gmtest"), &a(3, "GmEffets"))).await,
        Some("effects") => Box::pin(skill_effects(&a(1, "gmtest"), &a(2, "gmtest"), &a(3, "GmEffets"))).await,
        Some("skill") => Box::pin(skill(&a(1, "gmtest"), &a(2, "gmtest"), &a(3, "GmTest"))).await,
        Some("bag") => Box::pin(bag(&a(1, "gmtest"), &a(2, "gmtest"), &a(3, "GmTest"))).await,
        Some("scroll") => Box::pin(scroll(&a(1, "gmtest"), &a(2, "gmtest"), &a(3, "GmTest"))).await,
        Some("patrol") => Box::pin(patrol(&a(1, "sima"), &a(2, "sima"), &a(3, "SimA"))).await,
        Some("potion") => Box::pin(potion(&a(1, "gmtest"), &a(2, "gmtest"), &a(3, "GmTest"))).await,
        Some("equip") => Box::pin(equip(&a(1, "gmtest"), &a(2, "gmtest"), &a(3, "GmTest"))).await,
        Some("combat") => Box::pin(combat(&a(1, "gmtest"), &a(2, "gmtest"), &a(3, "GmTest"), &a(4, "Green Worm"))).await,
        _ => anyhow::bail!("usage : archcore sim client|move|shop|addon|combat|equip|potion|scroll|patrol|bag|skill|effects|status|bank|trade|quest [arguments]"),
    }
}

/// Paquet reçu : décodé si le type est connu.
#[derive(Debug, Clone)]
pub struct Recv {
    pub ptype: u8,
    pub packet: Option<Packet>,
}

impl Recv {
    fn int(&self, i: usize) -> Option<i64> {
        self.packet.as_ref().and_then(|p| p.int(i))
    }
    fn is(&self, s: &Schema, op: Option<i64>) -> bool {
        self.ptype == s.id && (op.is_none() || self.int(0) == op)
    }
    fn fields(&self) -> &Fields {
        &self.packet.as_ref().expect("paquet décodé").fields
    }
}

pub struct TestClient {
    label: String,
    stream: TcpStream,
    splitter: FrameSplitter,
    queue: VecDeque<Recv>,
    server_cipher: Option<Blowfish>,
    client_cipher: Option<Blowfish>,
    counter: u32,
    encrypted: bool,
    /// Paquets d'extension (0xAC) reçus sur cette connexion.
    pub ext_seen: usize,
    /// Dernier `HELLO_ACK` reçu.
    pub hello_ack: Option<Fields>,
}

impl TestClient {
    pub async fn connect(label: &str, addr: &str) -> anyhow::Result<Self> {
        let stream = TcpStream::connect(addr).await.map_err(|e| anyhow::anyhow!("[{label}] connexion à {addr} : {e}"))?;
        println!("[{label}] connecté à {addr}");
        Ok(TestClient { label: label.into(), stream, splitter: FrameSplitter::default(), queue: VecDeque::new(), server_cipher: None, client_cipher: None, counter: 0, encrypted: false, ext_seen: 0, hello_ack: None })
    }

    pub async fn send(&mut self, schema: &Schema, fields: Fields) -> anyhow::Result<()> {
        let plain = build_packet(schema, &fields);
        println!("[{}] → {} op={} ({} o)", self.label, schema.name, o(fields.int(0)), plain.len());
        if !self.encrypted {
            self.stream.write_all(&plain).await?;
            return Ok(());
        }
        let padded = plain.len().div_ceil(8) * 8;
        let mut frame = vec![0u8; FRAME_OVERHEAD + padded];
        frame[FRAME_HEADER..FRAME_HEADER + plain.len()].copy_from_slice(&plain);
        self.client_cipher.as_ref().unwrap().encrypt_le(&mut frame[FRAME_HEADER..FRAME_HEADER + padded])?;
        self.counter += 1;
        let len = frame.len() as u32;
        frame[0] = FRAME_CLIENT;
        frame[1..5].copy_from_slice(&len.to_le_bytes());
        frame[5..9].copy_from_slice(&self.counter.to_le_bytes());
        *frame.last_mut().unwrap() = FRAME_TAIL_CLIENT;
        self.stream.write_all(&frame).await?;
        Ok(())
    }

    async fn read_more(&mut self) -> anyhow::Result<()> {
        let mut buf = vec![0u8; 64 * 1024];
        let n = self.stream.read(&mut buf).await?;
        if n == 0 {
            anyhow::bail!("[{}] connexion fermée par le serveur", self.label);
        }
        for f in self.splitter.push(&buf[..n])? {
            let plain = if f.encrypted {
                let mut body = f.data[FRAME_HEADER..f.data.len() - 1].to_vec();
                self.server_cipher.as_ref().unwrap().decrypt_le(&mut body)?;
                let size = u32::from_le_bytes(body[1..5].try_into().unwrap()) as usize;
                body.truncate(size);
                body
            } else {
                f.data
            };
            let h = parse_header(&plain).ok_or_else(|| anyhow::anyhow!("en-tête invalide"))?;
            let schema = schema::main_by_id(h.ptype);
            let packet = schema.and_then(|s| decode_packet(s, &plain).ok());
            println!(
                "[{}] ← {} op={} ({} o)",
                self.label,
                schema.map(|s| s.name.to_string()).unwrap_or(format!("type 0x{:x}", h.ptype)),
                o(packet.as_ref().and_then(|p| p.int(0))),
                plain.len()
            );
            if h.ptype == schema::ptype::ARCHCORE_EXT {
                self.ext_seen += 1;
            }
            self.queue.push_back(Recv { ptype: h.ptype, packet });
        }
        Ok(())
    }

    /// Attend le premier paquet qui satisfait `pred` (les autres sont écartés), sous `timeout_ms`.
    pub async fn expect_where(&mut self, pred: impl Fn(&Recv) -> bool, timeout_ms: u64, what: &str) -> anyhow::Result<Recv> {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);
        loop {
            while let Some(p) = self.queue.pop_front() {
                if pred(&p) {
                    return Ok(p);
                }
            }
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            if left.is_zero() || tokio::time::timeout(left, self.read_more()).await.is_err() {
                anyhow::bail!("[{}] attente dépassée : {what}", self.label);
            }
        }
    }

    pub async fn expect(&mut self, s: &Schema, op: Option<i64>) -> anyhow::Result<Recv> {
        let what = format!("{} op {:?}", s.name, op);
        self.expect_where(|p| p.is(s, op), 5000, &what).await
    }

    pub async fn handshake(&mut self) -> anyhow::Result<()> {
        let se = &schema::STARTUP_ENCRYPTION;
        self.send(se, Fields::new(se).with(0, crypt_op::REQUEST)).await?;
        self.expect(se, Some(crypt_op::SET_ALGO)).await?;
        let public = self.expect(se, Some(crypt_op::PUBLIC_KEY)).await?;
        let server_key = public.fields().bytes(1).unwrap_or_default().to_vec();
        anyhow::ensure!(server_key.len() == 32, "clé serveur invalide");
        let server = Blowfish::new(&server_key)?;
        let mut client_key = vec![0u8; 32];
        rand::thread_rng().fill_bytes(&mut client_key);
        self.client_cipher = Some(Blowfish::new(&client_key)?);
        let mut enc = client_key.clone();
        server.encrypt_le(&mut enc)?;
        self.server_cipher = Some(server);
        self.send(se, Fields::new(se).with(0, crypt_op::PRIVATE_KEY).with(1, enc)).await?;
        self.encrypted = true;
        self.expect(se, Some(crypt_op::COMPLETE)).await?;
        println!("[{}] chiffrement établi", self.label);
        Ok(())
    }

    /// Négociation des extensions ArchRust (HELLO → HELLO_ACK), avec la langue souhaitée (champ 16,
    /// capacité `language/1`) ; rend le `HELLO_ACK` (capacités accordées au champ 5).
    pub async fn hello_lang(&mut self, language: Option<&str>) -> anyhow::Result<Fields> {
        let s = &schema::ARCHCORE_EXT;
        let hello = Fields::new(s)
            .with(0, ext_op::HELLO)
            .with(2, 1)
            .with(3, 1)
            .with(4, b"ArchCore sim".to_vec())
            .with(5, b"addon-msg/1;language/1;saved-vars/1".to_vec())
            .with_opt(16, language.map(|l| l.as_bytes().to_vec()));
        self.send(s, hello).await?;
        let ack = self.expect(s, Some(ext_op::HELLO_ACK)).await?;
        let f = ack.fields().clone();
        println!(
            "[{}] extensions : {} v{}.{} accorde « {} » (charge ≤ {} o, {} messages puis {}/s){}",
            self.label,
            f.text(4).unwrap_or_default(),
            o(f.int(2)),
            o(f.int(3)),
            f.text(5).unwrap_or_default(),
            o(f.int(6)),
            o(f.int(7)),
            o(f.int(8)),
            f.text(16).map(|l| format!(" ; langue {l} parmi {}", f.text(17).unwrap_or_default())).unwrap_or_default()
        );
        self.hello_ack = Some(f.clone());
        Ok(f)
    }
}

pub struct InWorld {
    pub world: TestClient,
    pub cid: u32,
    pub start: Vec3,
    /// Monstres reçus à l'entrée en jeu.
    pub monsters: Vec<SeenMonster>,
    /// PNJ reçus à l'entrée : (CID, nom).
    pub npcs: Vec<(i64, String)>,
    /// Niveau du personnage à l'entrée.
    pub level: i64,
    /// Argent en banque (champ 9 du personnage).
    pub bank_money: i64,
    /// Objets reçus à l'entrée (vue op 2, champ 6) : (IID, TID, statut, quantité).
    pub items: Vec<(i64, i64, i64, i64)>,
    /// Quêtes à l'entrée : terminées (drapeaux de l'op 5) et en cours (op 4).
    pub quests_done: Vec<u32>,
    pub quests_active: Vec<i64>,
    /// Compétences reçues à l'entrée (vue op 2, champ 8) : (identifiant, TID, niveau).
    pub skills: Vec<(i64, i64, i64)>,
    /// Langue appliquée annoncée par un `LANGUAGE_ACK` reçu pendant l'entrée (langue du compte).
    pub language_ack: Option<String>,
}

/// Paquets complets mis bout à bout (champs « tous les objets », « compétences » de la vue op 2).
fn split_packets(buf: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut off = 0;
    while let Some(h) = parse_header(&buf[off..]) {
        let end = off + h.size as usize;
        if h.size == 0 || end > buf.len() {
            break;
        }
        out.push(&buf[off..end]);
        off = end;
    }
    out
}

/// AGPMSKILL op 0 ou 2 : (identifiant, TID, niveau).
fn skill_info(p: &Packet) -> (i64, i64, i64) {
    let level = p
        .bytes(3)
        .and_then(|b| decode_mini(&mini::CHAR_FACTORS, b).ok())
        .and_then(|f| f.bytes(11).and_then(|d| decode_mini(&mini::FACTOR_DIRT, d).ok()))
        .and_then(|d| d.int(4))
        .unwrap_or(0);
    (p.int(1).unwrap_or(0), p.int(4).unwrap_or(0), level)
}

/// Monstre vu à l'entrée en jeu.
#[derive(Clone, Debug)]
pub struct SeenMonster {
    pub cid: u32,
    pub tid: i64,
    pub name: String,
    pub pos: Vec3,
    pub hp: i64,
}

impl SeenMonster {
    /// Lit la vue (mini OPT_VIEW_CHAR) d'un monstre.
    fn from_view(view: &Fields) -> Self {
        SeenMonster {
            cid: view.int(0).unwrap_or(0) as u32,
            tid: view.int(1).unwrap_or(0),
            name: view.text(2).unwrap_or_default(),
            pos: view.vec3(8).unwrap_or_default(),
            hp: view.bytes(5).and_then(factors_hp).unwrap_or(0),
        }
    }
}

/// Dégâts physiques (min, max) et défense physique d'un mini-paquet de facteurs complet.
fn factors_combat(bytes: &[u8]) -> Option<((i64, i64), i64)> {
    let f = decode_mini(&mini::CHAR_FACTORS, bytes).ok()?;
    let first = |b: &[u8]| decode_mini(&mini::FACTOR_ATTRIBUTE, b).ok().and_then(|a| a.int(0)).unwrap_or(0);
    let dmg = decode_mini(&mini::FACTOR_DMG, f.bytes(7)?).ok()?;
    let def = decode_mini(&mini::FACTOR_DEFENSE, f.bytes(8)?).ok()?;
    Some(((first(dmg.bytes(0)?), first(dmg.bytes(1)?)), first(def.bytes(0)?)))
}

/// PV d'un mini-paquet de facteurs complet (sous-paquet 3, valeur 0).
fn factors_hp(bytes: &[u8]) -> Option<i64> {
    let f = decode_mini(&mini::CHAR_FACTORS, bytes).ok()?;
    decode_mini(&mini::FACTOR_POINT, f.bytes(3)?).ok()?.int(0)
}

/// Se connecte au login, entre dans le monde avec `char_name` (créé s'il n'existe pas).
pub async fn enter_world(cfg: &Config, user: &str, pass: &str, char_name: &str, label: &str) -> anyhow::Result<InWorld> {
    Box::pin(enter_world_with(cfg, user, pass, char_name, label, false)).await
}

/// Comme `enter_world` ; avec `ext`, négocie les extensions ArchRust sur chaque connexion, comme ArchRust.
pub async fn enter_world_with(cfg: &Config, user: &str, pass: &str, char_name: &str, label: &str, ext: bool) -> anyhow::Result<InWorld> {
    Box::pin(enter_world_opts(cfg, user, pass, char_name, label, ext, None)).await
}

/// Comme `enter_world_with`, en annonçant une langue au `HELLO` (capacité `language/1`).
pub async fn enter_world_opts(cfg: &Config, user: &str, pass: &str, char_name: &str, label: &str, ext: bool, language: Option<&str>) -> anyhow::Result<InWorld> {
    let lg = &schema::LOGIN;
    let mut login = TestClient::connect(&format!("{label}login"), &format!("127.0.0.1:{}", cfg.login.port)).await?;
    login.handshake().await?;
    if ext {
        login.hello_lang(language).await?;
    }

    let ws = &schema::WORLD;
    login.send(ws, Fields::new(ws).with(0, world_op::SERVER_LIST_REQ)).await?;
    let worlds = login.expect(ws, Some(world_op::SERVER_LIST_ANS)).await?;
    let world_str = worlds.fields().text(3).unwrap_or_default();
    println!("  mondes : {world_str}");
    let world_name = world_str.split('=').next().unwrap_or("").to_string();

    login.send(lg, Fields::new(lg).with(0, login_op::HASHKEY)).await?;
    let hash_key = login.expect(lg, Some(login_op::HASHKEY)).await?.fields().text(1).unwrap_or_default();
    println!("  clé de hachage : {hash_key}");
    let enc_user = archcore_net::rc4::capi_crypt(hash_key.as_bytes(), &crate::tools::to_cp1252(user));
    let enc_pass = archcore_net::rc4::capi_crypt(hash_key.as_bytes(), &crate::tools::to_cp1252(pass));
    let (ul, pl) = (enc_user.len(), enc_pass.len());
    login.send(lg, Fields::new(lg).with(0, login_op::USER_LOGIN).with(2, enc_user).with(3, ul).with(4, enc_pass).with(5, pl)).await?;
    let ans = login.expect(lg, None).await?;
    if ans.int(0) == Some(login_op::LOGIN_RESULT) {
        anyhow::bail!("login refusé, résultat {:?}", ans.int(10));
    }
    println!("  authentifié (op {})", o(ans.int(0)));

    login.send(lg, Fields::new(lg).with(0, login_op::UNION_INFO).with(7, world_name.as_str())).await?;
    login.expect(lg, Some(login_op::UNION_INFO)).await?;
    let mut names = Vec::new();
    loop {
        let p = login.expect(lg, None).await?;
        match p.int(0) {
            Some(login_op::CHAR_NAME_FINISH) => break,
            Some(login_op::CHAR_NAME) => {
                let info = decode_mini(&mini::LOGIN_CHAR_INFO, p.fields().bytes(8).unwrap_or_default())?;
                names.push(info.text(1).unwrap_or_default());
            }
            _ => {}
        }
    }
    println!("  personnages : {}", if names.is_empty() { "(aucun)".into() } else { names.join(", ") });

    login.send(lg, Fields::new(lg).with(0, login_op::CHAR_LIST)).await?;
    loop {
        let p = login
            .expect_where(|p| p.is(&schema::CHARACTER, None) || p.is(lg, Some(login_op::CHAR_LIST_FINISH)), 5000, "liste des personnages")
            .await?;
        if p.ptype == lg.id {
            break;
        }
        let f = p.fields();
        let factors = decode_mini(&mini::CHAR_FACTORS, f.bytes(7).unwrap_or_default())?;
        let status = decode_mini(&mini::FACTOR_STATUS, factors.bytes(1).unwrap_or_default())?;
        println!("  personnage cid={} tid={} nom={} niveau={}", o(f.int(1)), o(f.int(2)), f.text(3).unwrap_or_default(), o(status.int(7)));
    }

    if !names.iter().any(|n| n == char_name) {
        login.send(lg, Fields::new(lg).with(0, login_op::CHAR_CREATION_ENTER)).await?;
        let mut bases = 0;
        loop {
            let p = login
                .expect_where(|p| p.is(&schema::CHARACTER, None) || p.is(lg, Some(login_op::CHAR_CREATION_ENTER)), 5000, "écran de création")
                .await?;
            if p.ptype == lg.id {
                break;
            }
            bases += 1;
        }
        println!("  {bases} personnage(s) de base reçus");
        let m = &mini::LOGIN_CHAR_INFO;
        let info = build_mini(m, &Fields::new(m).with(0, 96).with(1, char_name).with(6, 1).with(7, 2));
        login.send(lg, Fields::new(lg).with(0, login_op::CREATE_CHAR).with(8, info)).await?;
        let created = login.expect(lg, None).await?;
        if created.int(0) == Some(login_op::LOGIN_RESULT) {
            anyhow::bail!("création refusée : {:?}", created.int(10));
        }
        login.expect(&schema::CHARACTER, None).await?;
        let fin = login.expect(lg, Some(login_op::CREATE_CHAR_FINISH)).await?;
        println!("  personnage créé, cid={}", o(fin.int(6)));
    }

    let m = &mini::LOGIN_CHAR_INFO;
    let enter_info = build_mini(m, &Fields::new(m).with(1, char_name));
    login.send(lg, Fields::new(lg).with(0, login_op::WORLD_ENTER).with(8, enter_info)).await?;
    let cm = &schema::CHAR_MANAGER;
    let auth_key = login.expect(cm, Some(char_mgr_op::AUTH_KEY)).await?.int(6).unwrap_or(0);
    let enter = login.expect(lg, Some(login_op::WORLD_ENTER)).await?;
    let address = decode_mini(&mini::LOGIN_SERVER_INFO, enter.fields().bytes(9).unwrap_or_default())?.text(0).unwrap_or_default();
    println!("  clé d'auth {auth_key}, monde {address}");
    drop(login);

    let mut world = TestClient::connect(&format!("{label}world"), &address).await?;
    world.handshake().await?;
    if ext {
        let caps = world.hello_lang(language).await?.text(5).unwrap_or_default();
        anyhow::ensure!(caps.split(';').any(|c| c == "addon-msg/1"), "capacité addon-msg/1 refusée : « {caps} »");
    }
    // Comme le client : op 2, champ 2 = 17, nom, clé d'auth, bloc de 248 octets ; puis un keepalive op 13.
    world.send(cm, Fields::new(cm).with(0, char_mgr_op::GAME_ENTER).with(2, 17).with(3, format!("{char_name}#{world_name}").as_str()).with(6, auth_key).with(7, "")).await?;
    world.send(cm, Fields::new(cm).with(0, char_mgr_op::KEEPALIVE).with(2, 0)).await?;
    world.expect(&schema::CONFIG, None).await?;
    world.expect(&schema::CHARACTER, Some(25)).await?;
    let loading = world.expect(cm, Some(char_mgr_op::LOADING_POS)).await?;
    let start = loading.fields().vec3(5).unwrap_or_default();
    let cid = loading.int(4).unwrap_or(0) as u32;
    println!("  position de chargement : {:.0}, {:.0}, {:.0}", start.x, start.y, start.z);
    let timer = world.expect(&schema::TIMER, None).await?;
    let game_ms = timer.int(1).unwrap_or(0);
    println!("  horloge du serveur : {game_ms} ms, soit {} h en jeu", (game_ms / 3_600_000) % 24);
    world.send(cm, Fields::new(cm).with(0, char_mgr_op::ENTER_WORLD)).await?;
    let me = world.expect(&schema::CHARACTER, Some(0)).await?;
    let bank_money = me.int(9).unwrap_or(0);
    let level = me
        .fields()
        .bytes(7)
        .and_then(|b| decode_mini(&mini::CHAR_FACTORS, b).ok())
        .and_then(|f| f.bytes(1).and_then(|s| decode_mini(&mini::FACTOR_STATUS, s).ok()))
        .and_then(|s| s.int(7))
        .unwrap_or(0);
    let mut npc_names = Vec::new();
    let mut monsters = Vec::new();
    let mut skills = Vec::new();
    let mut npcs = Vec::new();
    let mut items = Vec::new();
    let (mut quests_done, mut quests_active) = (Vec::new(), Vec::new());
    let mut language_ack = None;
    loop {
        let p = world
            .expect_where(
                |q| {
                    q.is(&schema::ADMIN, None)
                        || q.is(&schema::OPT_VIEW, Some(1))
                        || q.is(&schema::OPT_VIEW, Some(2))
                        || q.is(&schema::QUEST, None)
                        || q.is(&schema::ARCHCORE_EXT, Some(ext_op::LANGUAGE_ACK))
                        || q.is(cm, Some(char_mgr_op::FINISH_ENTER))
                },
                5000,
                "entrée en jeu",
            )
            .await?;
        if p.ptype == cm.id {
            break;
        }
        if p.ptype == schema::ARCHCORE_EXT.id {
            language_ack = p.fields().text(16);
            continue;
        }
        if p.is(&schema::QUEST, Some(5)) {
            for (block, field) in [8usize, 9, 10].iter().enumerate() {
                for (byte, v) in p.fields().bytes(*field).unwrap_or_default().iter().enumerate() {
                    for bit in 0..8 {
                        if v & (1 << bit) != 0 {
                            quests_done.push((block * 0x4000 + byte * 8 + bit) as u32);
                        }
                    }
                }
            }
            continue;
        }
        if p.is(&schema::QUEST, Some(4)) {
            quests_active.push(p.int(4).unwrap_or(0));
            continue;
        }
        if p.ptype == schema::QUEST.id {
            continue;
        }
        if p.is(&schema::OPT_VIEW, Some(2)) {
            for raw in split_packets(p.fields().bytes(6).unwrap_or_default()) {
                if let Ok(i) = decode_packet(&schema::ITEM, raw) {
                    items.push((i.int(2).unwrap_or(0), i.int(3).unwrap_or(0), i.int(1).unwrap_or(0), i.int(5).unwrap_or(0)));
                }
            }
            for raw in split_packets(p.fields().bytes(8).unwrap_or_default()) {
                if let Ok(k) = decode_packet(&schema::SKILL, raw) {
                    skills.push(skill_info(&k));
                }
            }
            continue;
        }
        if p.ptype == schema::ADMIN.id {
            let a = decode_mini(&mini::ADMIN_CHAR, p.fields().bytes(2).unwrap_or_default())?;
            println!("  niveau MJ reçu : {}", o(a.int(3)));
            continue;
        }
        let view = decode_mini(&mini::OPT_VIEW_CHAR, p.fields().bytes(1).unwrap_or_default())?;
        if view.int(0).unwrap_or(0) >= crate::world::monsters::MONSTER_CID_BASE as i64 {
            monsters.push(SeenMonster::from_view(&view));
        } else {
            npc_names.push(view.text(2).unwrap_or_default());
            npcs.push((view.int(0).unwrap_or(0), view.text(2).unwrap_or_default()));
        }
    }
    let shown: Vec<&str> = npc_names.iter().take(6).map(String::as_str).collect();
    println!("  {} PNJ reçu(s) : {}{}", npc_names.len(), shown.join(", "), if npc_names.len() > 6 { ", …" } else { "" });
    println!("  {} monstre(s) reçu(s)", monsters.len());
    println!("  niveau {level}, {} compétence(s) : {skills:?}", skills.len());
    println!("  entrée en jeu confirmée");
    println!("  quêtes : terminées {quests_done:?}, en cours {quests_active:?}");
    Ok(InWorld { world, cid, start, monsters, npcs, level, bank_money, items, quests_done, quests_active, skills, language_ack })
}

async fn client(user: &str, pass: &str, char_name: &str, commands: &[String]) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let commands: Vec<String> = if commands.is_empty() { vec!["/GetExp".into()] } else { commands.to_vec() };
    let ch = &schema::CHATTING;
    for command in &commands {
        w.world
            .send(ch, Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(command)))
            .await?;
        let mut replies = vec![w.world.expect_where(|q| q.ptype == ch.id, 5000, &format!("réponse à {command}")).await?];
        while let Ok(more) = w.world.expect_where(|q| q.ptype == ch.id, 300, "suite").await {
            replies.push(more);
        }
        println!("  {command}");
        for r in replies {
            println!("    → [type {}] {}", o(r.int(2)), r.fields().text(6).unwrap_or_default());
        }
    }
    println!("  flux complet OK");
    Ok(())
}

fn step(t: &str) {
    println!("\n=== {t}");
}

async fn movement(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    const STOP: i64 = 0x04;
    let cfg = crate::config::load()?;
    let c = &schema::CHARACTER;
    let mv = &schema::OPT_CHAR_MOVE;
    let is_char = |op: i64, cid: u32| move |p: &Recv| p.is(c, Some(op)) && p.int(1) == Some(cid as i64);
    step(&format!("{ca} entre en jeu"));
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    step(&format!("{cb} entre en jeu"));
    let mut b = enter_world(&cfg, ub, pb, cb, "B-").await?;
    let start = b.start;

    b.world.expect_where(is_char(0, a.cid), 3000, &format!("{cb} reçoit {ca}")).await?;
    a.world.expect_where(is_char(0, b.cid), 3000, &format!("{ca} reçoit {cb}")).await?;
    println!("  ✓ {ca} et {cb} se voient");

    let dest = Vec3::new(start.x + 600.0, start.y, start.z + 300.0);
    a.world.send(mv, Fields::new(mv).with(0, a.cid).with(1, start).with(2, dest).with(5, 0).with(6, 0)).await?;
    let acid = a.cid as i64;
    let m = b.world.expect_where(|p| p.is(mv, None) && p.int(0) == Some(acid), 3000, "déplacement relayé").await?;
    let d = m.fields().vec3(2).unwrap_or_default();
    anyhow::ensure!((d.x - dest.x).abs() <= 0.5 && (d.z - dest.z).abs() <= 0.5, "destination relayée inattendue : {d:?}");
    println!("  ✓ {cb} voit {ca} partir vers {:.0}, {:.0}", d.x, d.z);

    let far = Vec3::new(start.x + 50_000.0, start.y, start.z);
    a.world.send(mv, Fields::new(mv).with(0, a.cid).with(1, far).with(2, far).with(5, STOP).with(6, 0)).await?;
    b.world.expect_where(is_char(2, a.cid), 3000, &format!("{cb} retire {ca}")).await?;
    a.world.expect_where(is_char(2, b.cid), 3000, &format!("{ca} retire {cb}")).await?;
    println!("  ✓ hors de portée : chacun retire l'autre");

    a.world.send(mv, Fields::new(mv).with(0, a.cid).with(1, start).with(2, start).with(5, STOP).with(6, 0)).await?;
    b.world.expect_where(is_char(0, a.cid), 3000, &format!("{cb} revoit {ca}")).await?;
    println!("  ✓ de retour à portée : ils se revoient");

    let cm = &schema::CHAR_MANAGER;
    a.world.send(cm, Fields::new(cm).with(0, char_mgr_op::KEEPALIVE).with(2, 0)).await?;
    drop(a);
    b.world.expect_where(is_char(2, acid as u32), 3000, &format!("{cb} retire {ca} déconnecté")).await?;
    println!("  ✓ déconnexion : le personnage disparaît chez l'autre joueur");
    println!("\ndéplacement à deux joueurs : OK");
    Ok(())
}

/// Paquet `ADDON_SEND`.
fn addon_send(request_id: i64, prefix: &[u8], channel: i64, target: Option<&str>, payload: &[u8]) -> Fields {
    let s = &schema::ARCHCORE_EXT;
    let f = Fields::new(s).with(0, ext_op::ADDON_SEND).with(1, request_id).with(9, prefix.to_vec()).with(10, channel).with_opt(11, target);
    if payload.is_empty() { f } else { f.with(14, payload.to_vec()) }
}

/// Attend le `RESULT` de la demande `request_id` et vérifie son code.
async fn expect_result(w: &mut TestClient, request_id: i64, code: i64, what: &str) -> anyhow::Result<()> {
    let s = &schema::ARCHCORE_EXT;
    let r = w.expect_where(|p| p.is(s, Some(ext_op::RESULT)) && p.int(1) == Some(request_id), 3000, what).await?;
    anyhow::ensure!(r.int(15) == Some(code), "{what} : résultat {} au lieu de {code}", o(r.int(15)));
    println!("  ✓ {what} : résultat {code}");
    Ok(())
}

/// Vérifie qu'aucun paquet d'extension n'arrive pendant `ms`.
async fn expect_no_ext(w: &mut TestClient, ms: u64, what: &str) -> anyhow::Result<()> {
    let s = &schema::ARCHCORE_EXT;
    if let Ok(p) = w.expect_where(|p| p.ptype == s.id, ms, what).await {
        anyhow::bail!("{what} : paquet d'extension inattendu (op {})", o(p.int(0)));
    }
    Ok(())
}

async fn addon(acc: &[(String, String, String); 3]) -> anyhow::Result<()> {
    const STOP: i64 = 0x04;
    let cfg = crate::config::load()?;
    let s = &schema::ARCHCORE_EXT;
    let [(ua, pa, ca), (ub, pb, cb), (uc, pc, cc)] = acc;
    step(&format!("{ca} et {cb} entrent en jeu avec les extensions, {cc} avec le client d'origine"));
    let mut a = Box::pin(enter_world_with(&cfg, ua, pa, ca, "A-", true)).await?;
    let mut b = Box::pin(enter_world_with(&cfg, ub, pb, cb, "B-", true)).await?;
    let mut c = Box::pin(enter_world_with(&cfg, uc, pc, cc, "C-", false)).await?;
    // Tous au même endroit, à portée de discussion.
    let mv = &schema::OPT_CHAR_MOVE;
    let here = b.start;
    for w in [&mut a, &mut c] {
        w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, here).with(2, here).with(5, STOP).with(6, 0)).await?;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let is_recv = |prefix: &'static str, channel: i64| {
        move |p: &Recv| p.is(s, Some(ext_op::ADDON_RECV)) && p.fields().bytes(9) == Some(prefix.as_bytes()) && p.int(10) == Some(channel)
    };

    step("chuchotement d'AddOn");
    a.world.send(s, addon_send(1, b"SimTest", ext_channel::WHISPER, Some(cb.as_str()), b"bonjour B")).await?;
    let r = b.world.expect_where(is_recv("SimTest", ext_channel::WHISPER), 3000, "chuchotement reçu").await?;
    let f = r.fields();
    anyhow::ensure!(f.int(12) == Some(a.cid as i64), "émetteur {} au lieu de {}", o(f.int(12)), a.cid);
    anyhow::ensure!(f.text(13).as_deref() == Some(ca.as_str()), "nom d'émetteur {:?}", f.text(13));
    anyhow::ensure!(f.text(11).as_deref() == Some(cb.as_str()), "destinataire {:?}", f.text(11));
    anyhow::ensure!(f.bytes(14) == Some(&b"bonjour B"[..]), "charge altérée");
    println!("  ✓ {cb} reçoit le message de {ca} (cid {}), charge intacte", a.cid);
    expect_no_ext(&mut a.world, 400, "pas d'écho ni d'erreur chez l'émetteur").await?;
    println!("  ✓ pas d'écho ni de RESULT chez {ca}");

    step("message d'AddOn à portée (SAY)");
    let acid = a.cid as i64;
    a.world.send(s, addon_send(2, b"SimTest", ext_channel::SAY, None, b"tout le monde")).await?;
    for (w, n) in [(&mut a, ca), (&mut b, cb)] {
        let r = w.world.expect_where(is_recv("SimTest", ext_channel::SAY), 3000, "SAY reçu").await?;
        anyhow::ensure!(r.fields().text(11).is_none() && r.int(12) == Some(acid), "SAY mal formé");
        println!("  ✓ {n} reçoit le SAY");
    }

    step("refus");
    a.world.send(s, addon_send(3, b"SimTest", ext_channel::WHISPER, Some(cc.as_str()), b"x")).await?;
    expect_result(&mut a.world, 3, ext_result::TARGET_NO_EXTENSION, &format!("chuchotement à {cc} (client d'origine)")).await?;
    a.world.send(s, addon_send(4, b"SimTest", ext_channel::WHISPER, Some("PersonneIci"), b"x")).await?;
    expect_result(&mut a.world, 4, ext_result::TARGET_OFFLINE, "chuchotement à un absent").await?;
    a.world.send(s, addon_send(5, b"AC.Test", ext_channel::SAY, None, b"x")).await?;
    expect_result(&mut a.world, 5, ext_result::NOT_ALLOWED, "préfixe réservé AC.").await?;
    a.world.send(s, addon_send(6, b"a b", ext_channel::SAY, None, b"x")).await?;
    expect_result(&mut a.world, 6, ext_result::INVALID_PREFIX, "préfixe avec espace").await?;
    a.world.send(s, addon_send(7, b"SimTest", ext_channel::SAY, None, &[b'z'; 256])).await?;
    expect_result(&mut a.world, 7, ext_result::MESSAGE_TOO_LONG, "charge de 256 octets").await?;
    a.world.send(s, addon_send(8, b"SimTest", ext_channel::PARTY, None, b"x")).await?;
    expect_result(&mut a.world, 8, ext_result::NOT_AVAILABLE, "canal de groupe").await?;

    step(&format!("{cc} (sans HELLO) tente un message d'AddOn"));
    c.world.send(s, addon_send(1, b"SimTest", ext_channel::SAY, None, b"intrus")).await?;
    expect_no_ext(&mut b.world, 500, "message d'un client sans extensions").await?;
    println!("  ✓ ignoré par le serveur");

    step("interdiction de discussion");
    let ch = &schema::CHATTING;
    let say = |cid: u32, text: &str| Fields::new(ch).with(0, 0).with(1, cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text));
    c.world.send(ch, say(c.cid, &format!("/chatban {ca} 1"))).await?;
    c.world.expect_where(|q| q.ptype == ch.id, 3000, "réponse à /chatban").await?;
    a.world.send(s, addon_send(9, b"SimTest", ext_channel::SAY, None, b"x")).await?;
    expect_result(&mut a.world, 9, ext_result::NOT_ALLOWED, &format!("{ca} sous /chatban")).await?;
    c.world.send(ch, say(c.cid, &format!("/chatban {ca} 0"))).await?;
    c.world.expect_where(|q| q.ptype == ch.id, 3000, "réponse à /chatban 0").await?;

    step("rafale de 25 messages (limite : 20 puis 10/s)");
    tokio::time::sleep(Duration::from_millis(2100)).await;
    for i in 0..25 {
        a.world.send(s, addon_send(100 + i, b"Rafale", ext_channel::WHISPER, Some(cb.as_str()), b"r")).await?;
    }
    let mut throttled = 0;
    while let Ok(r) = a.world.expect_where(|p| p.is(s, Some(ext_op::RESULT)), 800, "refus").await {
        anyhow::ensure!(r.int(15) == Some(ext_result::THROTTLED), "résultat {} inattendu", o(r.int(15)));
        throttled += 1;
    }
    let mut delivered = 0;
    while b.world.expect_where(is_recv("Rafale", ext_channel::WHISPER), 800, "rafale").await.is_ok() {
        delivered += 1;
    }
    println!("  {delivered} remis, {throttled} refusés (THROTTLED)");
    anyhow::ensure!(throttled >= 1 && delivered >= 20 && delivered + throttled == 25, "limite de débit inattendue");
    println!("  ✓ limite de débit appliquée");

    step(&format!("{cc} n'a reçu aucun paquet d'extension"));
    let _ = c.world.expect_where(|_| false, 500, "vidage").await;
    anyhow::ensure!(c.world.ext_seen == 0, "{} paquet(s) 0xAC envoyés au client d'origine", c.world.ext_seen);
    println!("  ✓ aucun paquet 0xAC chez {cc}");
    println!("\nextensions et messages d'AddOn : OK");
    Ok(())
}

async fn combat(user: &str, pass: &str, char_name: &str, prey: &str) -> anyhow::Result<()> {
    /// Point de chasse : Vers verts au nord d'Anchorville (Limelight Nord).
    const HUNT: Vec3 = Vec3 { x: -466_468.0, y: 4_700.0, z: -50_010.0 };
    const STOP: i64 = 0x04;
    let cfg = crate::config::load()?;
    let (c, act, mv, ch, ov) = (&schema::CHARACTER, &schema::OPT_CHAR_ACTION, &schema::OPT_CHAR_MOVE, &schema::CHATTING, &schema::OPT_VIEW);
    let village = cfg.world.start_positions.get("1").map(|p| Vec3::new(p.x as f32, p.y as f32, p.z as f32));
    step(&format!("{char_name} entre en jeu"));
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let me = w.cid as i64;
    let village = village.unwrap_or(w.start);

    // Se rend au point de chasse et choisit le monstre demandé le plus proche.
    async fn hunt(w: &mut InWorld, prey: &str) -> anyhow::Result<SeenMonster> {
        let (mv, ov) = (&schema::OPT_CHAR_MOVE, &schema::OPT_VIEW);
        w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, HUNT).with(2, HUNT).with(5, STOP).with(6, 0)).await?;
        let mut seen = w.monsters.clone();
        while let Ok(p) = w.world.expect_where(|q| q.is(ov, Some(1)), 1500, "monstres du lieu de chasse").await {
            if let Ok(view) = decode_mini(&mini::OPT_VIEW_CHAR, p.fields().bytes(1).unwrap_or_default()) {
                if view.int(0).unwrap_or(0) >= crate::world::monsters::MONSTER_CID_BASE as i64 {
                    seen.push(SeenMonster::from_view(&view));
                }
            }
        }
        let dist = |p: Vec3| (((p.x - HUNT.x) as f64).powi(2) + ((p.z - HUNT.z) as f64).powi(2)).sqrt();
        let target = seen
            .iter()
            .filter(|m| m.hp > 0 && m.name == prey)
            .min_by_key(|m| dist(m.pos) as i64)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("aucun « {prey} » vivant en vue"))?;
        println!("  cible : {} (TID {}, CID {:#x}), {} PV, à {:.0} unités", target.name, target.tid, target.cid, target.hp, dist(target.pos));
        Ok(target)
    }
    let say = |cid: u32, text: &str| Fields::new(ch).with(0, 0).with(1, cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text));

    step(&format!("riposte : {char_name} attaque un {prey}"));
    let target = hunt(&mut w, prey).await?;
    let tcid = target.cid as i64;
    w.world.send(act, Fields::new(act).with(0, w.cid).with(1, target.cid).with(6, 1)).await?;
    let first = w.world.expect_where(|q| q.is(act, None) && q.int(0) == Some(tcid) && q.int(1) == Some(me), 15_000, "riposte du monstre").await?;
    let my_hp = first.fields().bytes(3).and_then(factors_hp);
    println!("  ✓ le {prey} riposte (résultat {}, PV de {char_name} : {})", o(first.int(2)), o(my_hp));

    step("poursuite");
    let away = Vec3::new(target.pos.x + 1500.0, target.pos.y, target.pos.z);
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, away).with(2, away).with(5, STOP).with(6, 0)).await?;
    let chase = w.world.expect_where(|q| q.is(mv, None) && q.int(0) == Some(tcid), 10_000, "poursuite du monstre").await?;
    let d = chase.fields().vec3(2).unwrap_or_default();
    println!("  ✓ {char_name} s'éloigne de 1500 unités : le {prey} le suit jusqu'à {:.0}, {:.0}", d.x, d.z);

    step(&format!("mort de {char_name}"));
    let started = std::time::Instant::now();
    w.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(me) && q.int(11) == Some(2), 90_000, "mort du joueur").await?;
    println!("  ✓ {char_name} meurt après {:.0} s", started.elapsed().as_secs_f64());

    step("résurrection au village");
    w.world.send(c, Fields::new(c).with(0, 9).with(1, w.cid)).await?;
    let back = w.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(me) && q.int(11) == Some(0), 5000, "résurrection").await?;
    let hp = back.fields().bytes(7).and_then(factors_hp).unwrap_or(0);
    anyhow::ensure!(hp > 0, "ressuscité sans PV");
    let tp = w.world.expect_where(|q| q.is(mv, None) && q.int(0) == Some(me), 5000, "retour au village").await?;
    let at = tp.fields().vec3(2).unwrap_or_default();
    // Point de résurrection de la région de rattachement : à quelques milliers d'unités du point de départ.
    let from_village = (((at.x - village.x) as f64).powi(2) + ((at.z - village.z) as f64).powi(2)).sqrt();
    anyhow::ensure!(from_village < 10_000.0, "ressuscité en {at:?}, à {from_village:.0} du village");
    println!("  ✓ {char_name} ressuscite au village avec {hp} PV");

    step(&format!("victoire : {char_name} invincible contre un {prey}"));
    w.world.send(ch, say(w.cid, "/invincible 1")).await?;
    let reply = w.world.expect_where(|q| q.ptype == ch.id, 3000, "réponse à /invincible").await?;
    println!("  {}", reply.fields().text(6).unwrap_or_default());
    // Le monstre précédent garde son agresseur 30 s : on en prend un autre au repos si possible.
    let target = hunt(&mut w, prey).await?;
    let tcid = target.cid as i64;
    w.world.send(act, Fields::new(act).with(0, w.cid).with(1, target.cid).with(6, 1)).await?;
    let (mut hits, mut misses, mut wounds, mut hp) = (0, 0, 0, target.hp);
    let started = std::time::Instant::now();
    loop {
        let p = w
            .world
            .expect_where(
                |q| (q.is(act, None) && (q.int(1) == Some(tcid) || q.int(1) == Some(me))) || (q.is(c, Some(1)) && q.int(1) == Some(tcid) && q.int(11) == Some(2)),
                10_000,
                "coups portés",
            )
            .await?;
        if p.ptype == c.id {
            break;
        }
        if p.int(1) == Some(me) {
            if p.int(2) == Some(5) {
                wounds += 1;
            }
            continue;
        }
        match p.int(2) {
            Some(5) => {
                hits += 1;
                hp = p.fields().bytes(3).and_then(factors_hp).unwrap_or(hp);
            }
            Some(1) => misses += 1,
            other => anyhow::bail!("résultat inattendu {other:?}"),
        }
    }
    anyhow::ensure!(wounds == 0, "{wounds} coup(s) reçu(s) malgré l'invincibilité");
    println!("  ✓ le {prey} meurt après {hits} coup(s) portés et {misses} manqué(s), en {:.0} s, sans blesser {char_name}", started.elapsed().as_secs_f64());
    let gain = w
        .world
        .expect_where(|q| q.ptype == ch.id && q.fields().text(6).is_some_and(|t| t.contains("expérience")), 3000, "gain d'expérience")
        .await?;
    println!("  ✓ {}", gain.fields().text(6).unwrap_or_default());

    step("butin");
    let it = &schema::ITEM;
    let mut ground = Vec::new();
    while let Ok(p) = w.world.expect_where(|q| q.is(it, Some(0)) && q.int(1) == Some(1) && q.int(4) == Some(me), 1500, "butin au sol").await {
        ground.push((p.int(2).unwrap_or(0), p.int(3).unwrap_or(0), p.int(5).unwrap_or(0)));
    }
    anyhow::ensure!(ground.iter().any(|g| g.1 == crate::world::loot::GOLD_TID as i64), "pas d'or au sol : {ground:?}");
    println!("  ✓ {} objet(s) au sol : {}", ground.len(), ground.iter().map(|g| format!("TID {} × {}", g.1, g.2)).collect::<Vec<_>>().join(", "));
    // Ordre du serveur : objet rangé (AGPMITEM statut 2) puis résultat (op 12) ; pour l'or, résultat puis somme.
    for (iid, tid, count) in &ground {
        w.world.send(it, Fields::new(it).with(0, 11).with(2, *iid)).await?;
        let gold = *tid == crate::world::loot::GOLD_TID as i64;
        if !gold {
            w.world.expect_where(|q| q.is(it, None) && q.int(3) == Some(*tid) && q.int(1) == Some(2), 3000, "objet rangé").await?;
        }
        let r = w.world.expect_where(|q| q.is(it, Some(12)) && q.int(2) == Some(*iid), 3000, "résultat du ramassage").await?;
        anyhow::ensure!(r.int(1) == Some(0), "ramassage de {iid:#x} refusé");
        if gold {
            let m = w.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(me) && q.int(8).is_some(), 3000, "or crédité").await?;
            println!("  ✓ {count} pièce(s) d'or ramassée(s) : {} au total", o(m.int(8)));
        } else {
            println!("  ✓ objet TID {tid} × {count} rangé dans l'inventaire");
        }
    }

    step("cadavre et réapparition");
    w.world.expect_where(|q| q.is(c, Some(2)) && q.int(1) == Some(tcid), 30_000, "retrait du cadavre").await?;
    println!("  ✓ cadavre retiré");
    let back = w
        .world
        .expect_where(
            |q| q.is(ov, Some(1)) && q.fields().bytes(1).and_then(|b| decode_mini(&mini::OPT_VIEW_CHAR, b).ok()).is_some_and(|v| v.int(0) == Some(tcid)),
            120_000,
            "réapparition",
        )
        .await?;
    let view = decode_mini(&mini::OPT_VIEW_CHAR, back.fields().bytes(1).unwrap_or_default())?;
    let full = view.bytes(5).and_then(factors_hp).unwrap_or(0);
    // La cible a pu être entamée avant ce combat : elle revient au moins avec les PV vus au départ.
    anyhow::ensure!(full >= target.hp && full > 0, "réapparu avec {full} PV (vus au départ : {})", target.hp);
    println!("  ✓ {} réapparaît avec {full} PV", target.name);

    // Remise en état : plus d'invincibilité, retour au village.
    w.world.send(ch, say(w.cid, "/invincible 0")).await?;
    w.world.expect_where(|q| q.ptype == ch.id, 3000, "réponse à /invincible 0").await?;
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, village).with(2, village).with(5, STOP).with(6, 0)).await?;
    tokio::time::sleep(Duration::from_millis(300)).await;
    println!("\ncombat : OK");
    Ok(())
}

async fn equip(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    const SWORD: i64 = 11; // Short Sword : guerrier, 11 à 13
    const ARMOR: i64 = 324; // Halfsteel Armor : guerrier humain, niveau 2, défense 4
    const WAND: i64 = 9; // Glass Wand : mage
    let cfg = crate::config::load()?;
    let (c, it, ch) = (&schema::CHARACTER, &schema::ITEM, &schema::CHATTING);
    step("SimA, témoin, entre en jeu");
    let mut witness = enter_world(&cfg, "sima", "sima", "SimA", "B-").await?;
    step(&format!("{char_name} entre en jeu"));
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let me = w.cid as i64;
    let say = |cid: u32, text: &str| Fields::new(ch).with(0, 0).with(1, cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text));

    step("objets créés par commande MJ");
    let mut iids = std::collections::HashMap::new();
    for tid in [SWORD, ARMOR, WAND] {
        w.world.send(ch, say(w.cid, &format!("/create \"{char_name}\" {tid} 1"))).await?;
        let p = w.world.expect_where(|q| q.is(it, Some(0)) && q.int(3) == Some(tid) && q.int(1) == Some(2), 3000, "objet créé").await?;
        iids.insert(tid, p.int(2).unwrap_or(0));
    }
    println!("  ✓ épée, armure et baguette dans l'inventaire");

    // Équipe et rend les facteurs reçus ensuite.
    async fn wear(w: &mut InWorld, iid: i64) -> anyhow::Result<((i64, i64), i64)> {
        let (c, it, me) = (&schema::CHARACTER, &schema::ITEM, w.cid as i64);
        w.world.send(it, Fields::new(it).with(0, 2).with(1, 3).with(2, iid)).await?;
        w.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(iid) && q.int(1) == Some(3), 3000, "objet équipé").await?;
        let f = w.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(me) && q.fields().bytes(7).is_some(), 3000, "facteurs").await?;
        factors_combat(f.fields().bytes(7).unwrap_or_default()).ok_or_else(|| anyhow::anyhow!("facteurs illisibles"))
    }

    step("épée");
    let (dmg, _) = wear(&mut w, iids[&SWORD]).await?;
    anyhow::ensure!(dmg.0 >= 11 && dmg.1 >= 13, "dégâts {dmg:?} sans l'épée");
    println!("  ✓ épée portée : dégâts physiques {} à {}", dmg.0, dmg.1);
    let sword_iid = iids[&SWORD];
    let seen = witness.world.expect_where(|q| q.is(it, Some(0)) && q.int(2) == Some(sword_iid) && q.int(1) == Some(3), 3000, "épée vue par SimA").await?;
    println!("  ✓ SimA voit l'épée sur {char_name} (propriétaire {})", o(seen.int(4)));

    step("armure");
    let (_, def) = wear(&mut w, iids[&ARMOR]).await?;
    anyhow::ensure!(def >= 4, "défense {def} sans l'armure");
    println!("  ✓ armure portée : défense physique {def}");

    step("baguette de mage");
    w.world.send(it, Fields::new(it).with(0, 2).with(1, 3).with(2, iids[&WAND])).await?;
    let no = w.world.expect_where(|q| q.ptype == ch.id && q.fields().text(6).is_some_and(|t| t.contains("porter")), 3000, "refus").await?;
    println!("  ✓ refusée : {}", no.fields().text(6).unwrap_or_default());

    step("retrait de l'épée et de l'armure");
    for tid in [SWORD, ARMOR] {
        let iid = iids[&tid];
        let any = &mini::ITEM_GRID;
        let grid = build_mini(any, &Fields::new(any).with(0, -1).with(1, -1).with(2, -1));
        w.world.send(it, Fields::new(it).with(0, 2).with(1, 2).with(2, iid).with(7, grid)).await?;
        w.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(iid) && q.int(1) == Some(2), 3000, "objet rangé").await?;
        let f = w.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(me) && q.fields().bytes(7).is_some(), 3000, "facteurs").await?;
        let (dmg, def) = factors_combat(f.fields().bytes(7).unwrap_or_default()).unwrap_or_default();
        println!("  ✓ objet {tid} rangé : dégâts {} à {}, défense {def}", dmg.0, dmg.1);
    }
    witness.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(sword_iid), 3000, "épée retirée chez SimA").await?;
    println!("  ✓ SimA ne voit plus l'épée");
    println!("\néquipement : OK");
    Ok(())
}

async fn potion(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    const HUNT: Vec3 = Vec3 { x: -466_468.0, y: 4_700.0, z: -50_010.0 };
    const LIFE: i64 = 190; // Small Life Potion : 75 PV, 2 s
    const STOP: i64 = 0x04;
    let cfg = crate::config::load()?;
    let (c, it, ch, act, mv, ov) = (&schema::CHARACTER, &schema::ITEM, &schema::CHATTING, &schema::OPT_CHAR_ACTION, &schema::OPT_CHAR_MOVE, &schema::OPT_VIEW);
    step(&format!("{char_name} entre en jeu"));
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let me = w.cid as i64;
    let village = cfg.world.start_positions.get("1").map(|p| Vec3::new(p.x as f32, p.y as f32, p.z as f32)).unwrap_or(w.start);
    let say = |cid: u32, text: &str| Fields::new(ch).with(0, 0).with(1, cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text));

    step("potions créées");
    w.world.send(ch, say(w.cid, "/invincible 0")).await?;
    w.world.send(ch, say(w.cid, &format!("/create \"{char_name}\" {LIFE} 3"))).await?;
    let made = w.world.expect_where(|q| q.is(it, None) && q.int(3) == Some(LIFE) && q.int(1) == Some(2), 3000, "potions créées").await?;
    let (iid, count) = (made.int(2).unwrap_or(0), made.int(5).unwrap_or(0));
    println!("  ✓ {count} Small Life Potion(s), IID {iid}");

    step("blessé par un Green Worm");
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, HUNT).with(2, HUNT).with(5, STOP).with(6, 0)).await?;
    // Déjà sur place à l'entrée : les vers ont été envoyés avec l'entrée en jeu.
    let near_hunt = |p: Vec3| (((p.x - HUNT.x) as f64).powi(2) + ((p.z - HUNT.z) as f64).powi(2)).sqrt() < 6000.0;
    let mut worm = w.monsters.iter().find(|m| m.name == "Green Worm" && m.hp > 0 && near_hunt(m.pos)).map(|m| m.cid as i64);
    while let Ok(p) = w.world.expect_where(|q| q.is(ov, Some(1)), 1500, "monstres").await {
        if let Ok(view) = decode_mini(&mini::OPT_VIEW_CHAR, p.fields().bytes(1).unwrap_or_default()) {
            if view.text(2).as_deref() == Some("Green Worm") && view.bytes(5).and_then(factors_hp).unwrap_or(0) > 0 {
                worm.get_or_insert(view.int(0).unwrap_or(0));
            }
        }
    }
    let worm = worm.ok_or_else(|| anyhow::anyhow!("aucun Green Worm en vue"))?;
    w.world.send(act, Fields::new(act).with(0, w.cid).with(1, worm).with(6, 1)).await?;
    let hit = w
        .world
        .expect_where(|q| q.is(act, None) && q.int(0) == Some(worm) && q.int(1) == Some(me) && q.int(2) == Some(5), 20_000, "coup du ver")
        .await?;
    let mut hp = hit.fields().bytes(3).and_then(factors_hp).unwrap_or(0);
    w.world.send(ch, say(w.cid, "/invincible 1")).await?;
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, village).with(2, village).with(5, STOP).with(6, 0)).await?;
    println!("  ✓ blessé : {hp} PV ; invincible, retour au village");

    step("régénération hors combat");
    let regen = w
        .world
        .expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(me) && q.fields().bytes(7).and_then(factors_hp).is_some_and(|v| v > hp), 30_000, "régénération")
        .await?;
    let after = regen.fields().bytes(7).and_then(factors_hp).unwrap_or(0);
    println!("  ✓ {hp} → {after} PV (10 s après le dernier coup)");
    hp = after;

    step("potion de vie");
    w.world.send(it, Fields::new(it).with(0, 3).with(2, iid)).await?;
    let left = w.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(iid), 3000, "pile diminuée").await?;
    w.world.expect_where(|q| q.is(it, Some(4)) && q.int(3) == Some(LIFE) && q.int(4) == Some(me), 3000, "potion utilisée").await?;
    let f = w.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(me) && q.fields().bytes(7).is_some(), 3000, "PV après la potion").await?;
    let healed = f.fields().bytes(7).and_then(factors_hp).unwrap_or(0);
    anyhow::ensure!(healed > hp, "la potion n'a pas soigné ({hp} → {healed})");
    println!("  ✓ {hp} → {healed} PV, {} potion(s) restante(s)", o(left.int(5)));

    step("seconde potion aussitôt");
    w.world.send(it, Fields::new(it).with(0, 3).with(2, iid)).await?;
    anyhow::ensure!(w.world.expect_where(|q| q.is(it, Some(4)), 1000, "refus").await.is_err(), "seconde potion acceptée");
    println!("  ✓ refusée (PV pleins ou délai de 2 s)");

    w.world.send(ch, say(w.cid, "/invincible 0")).await?;
    w.world.expect_where(|q| q.ptype == ch.id, 3000, "réponse à /invincible 0").await?;
    println!("\npotions et régénération : OK");
    Ok(())
}

async fn scroll(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    const GO_TOWN: i64 = 198; // Passport
    const ROUNDTRIP: i64 = 213; // Roundtrip Passport
    const TULLAN: i64 = 1830; // Tullan Passport : -353400 ; 81200
    let cfg = crate::config::load()?;
    let (it, ch, mv) = (&schema::ITEM, &schema::CHATTING, &schema::OPT_CHAR_MOVE);
    step(&format!("{char_name} entre en jeu"));
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let me = w.cid as i64;
    let village = cfg.world.start_positions.get("1").map(|p| Vec3::new(p.x as f32, p.y as f32, p.z as f32)).unwrap_or(w.start);
    let say = |cid: u32, text: &str| Fields::new(ch).with(0, 0).with(1, cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text));
    let mut iids = std::collections::HashMap::new();
    for tid in [GO_TOWN, ROUNDTRIP, TULLAN] {
        w.world.send(ch, say(w.cid, &format!("/create \"{char_name}\" {tid} 1"))).await?;
        let p = w.world.expect_where(|q| q.is(it, None) && q.int(3) == Some(tid) && q.int(1) == Some(2), 3000, "parchemin créé").await?;
        iids.insert(tid, p.int(2).unwrap_or(0));
    }
    println!("  ✓ trois parchemins dans l'inventaire");
    // Utilise un objet et rend la destination de la téléportation qui suit.
    async fn use_and_land(w: &mut InWorld, iid: i64, tid: i64) -> anyhow::Result<Vec3> {
        let (it, mv, me) = (&schema::ITEM, &schema::OPT_CHAR_MOVE, w.cid as i64);
        w.world.send(it, Fields::new(it).with(0, 3).with(2, iid)).await?;
        w.world.expect_where(|q| q.is(it, Some(4)) && q.int(3) == Some(tid), 3000, "parchemin utilisé").await?;
        let m = w.world.expect_where(|q| q.is(mv, None) && q.int(0) == Some(me), 3000, "téléportation").await?;
        Ok(m.fields().vec3(2).unwrap_or_default())
    }
    let near = |a: Vec3, b: Vec3| (a.x - b.x).abs() < 1.0 && (a.z - b.z).abs() < 1.0;
    // Retour « au village » : au point de résurrection de la région, à quelques milliers d'unités du départ.
    let town = |a: Vec3| (((a.x - village.x) as f64).powi(2) + ((a.z - village.z) as f64).powi(2)).sqrt() < 10_000.0;
    let tullan = Vec3::new(-353_400.0, 2125.0, 81_200.0);

    step("passeport de Tullan");
    let at = use_and_land(&mut w, iids[&TULLAN], TULLAN).await?;
    anyhow::ensure!(near(at, tullan), "arrivé en {at:?}");
    println!("  ✓ à Tullan : {:.0}, {:.0}", at.x, at.z);

    step("aller-retour");
    let at = use_and_land(&mut w, iids[&ROUNDTRIP], ROUNDTRIP).await?;
    anyhow::ensure!(town(at), "aller : {at:?}");
    println!("  ✓ aller : au village");
    w.world.send(it, Fields::new(it).with(0, 6).with(4, w.cid)).await?;
    w.world.expect_where(|q| q.is(it, Some(10)) && q.int(4) == Some(me), 3000, "retour désactivé").await?;
    let back = w.world.expect_where(|q| q.is(mv, None) && q.int(0) == Some(me), 3000, "retour").await?.fields().vec3(2).unwrap_or_default();
    anyhow::ensure!(near(back, tullan), "retour en {back:?}");
    println!("  ✓ retour : de nouveau à Tullan");

    step("passeport (retour au village)");
    let at = use_and_land(&mut w, iids[&GO_TOWN], GO_TOWN).await?;
    anyhow::ensure!(town(at), "arrivé en {at:?}");
    println!("  ✓ au village");
    println!("\nparchemins : OK");
    Ok(())
}

async fn patrol(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    const HUNT: Vec3 = Vec3 { x: -466_468.0, y: 4_700.0, z: -50_010.0 };
    let cfg = crate::config::load()?;
    let mv = &schema::OPT_CHAR_MOVE;
    step(&format!("{char_name} entre en jeu"));
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let home = cfg.world.start_positions.get("1").map(|p| Vec3::new(p.x as f32, p.y as f32, p.z as f32)).unwrap_or(w.start);
    step("immobile chez les Vers verts (50 s au plus)");
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, HUNT).with(2, HUNT).with(5, 0x04).with(6, 0)).await?;
    let base = crate::world::monsters::MONSTER_CID_BASE as i64;
    let mut walkers = std::collections::BTreeSet::new();
    let started = std::time::Instant::now();
    while walkers.len() < 3 && started.elapsed() < Duration::from_secs(50) {
        if let Ok(p) = w.world.expect_where(|q| q.is(mv, None) && q.int(0).is_some_and(|c| c >= base), 5000, "promenade").await {
            anyhow::ensure!(p.int(5) == Some(0), "promenade en courant (drapeaux {})", o(p.int(5)));
            let d = p.fields().vec3(2).unwrap_or_default();
            if walkers.insert(p.int(0).unwrap_or(0)) {
                println!("  ✓ monstre {:#x} marche vers {:.0}, {:.0} ({:.0} s)", p.int(0).unwrap_or(0), d.x, d.z, started.elapsed().as_secs_f64());
            }
        }
    }
    anyhow::ensure!(!walkers.is_empty(), "aucun monstre ne s'est promené");
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, home).with(2, home).with(5, 0x04).with(6, 0)).await?;
    tokio::time::sleep(Duration::from_millis(300)).await;
    println!("\npatrouille : OK ({} monstre(s) en promenade)", walkers.len());
    Ok(())
}

async fn bag(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    const MANA: i64 = 193; // Small Mana Potion : une pile neuve, détruite à la fin
    const FREE: [i64; 3] = [3, 3, 3]; // dernière case du dernier sac, libre en pratique
    let cfg = crate::config::load()?;
    let (c, it, ch, mv) = (&schema::CHARACTER, &schema::ITEM, &schema::CHATTING, &schema::OPT_CHAR_MOVE);
    step(&format!("{char_name} entre en jeu"));
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let me = w.cid as i64;
    let say = |cid: u32, text: &str| Fields::new(ch).with(0, 0).with(1, cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text));
    let grid = |g: [i64; 3]| {
        let s = &mini::ITEM_GRID;
        build_mini(s, &Fields::new(s).with(0, g[0]).with(1, g[1]).with(2, g[2]))
    };
    let grid_of = |r: &Recv| r.fields().bytes(7).and_then(|b| decode_mini(&mini::ITEM_GRID, b).ok()).map(|g| [g.int(0).unwrap_or(0), g.int(1).unwrap_or(0), g.int(2).unwrap_or(0)]);

    step("pile neuve de potions de mana");
    let mut made = None;
    for _ in 0..2 {
        w.world.send(ch, say(w.cid, &format!("/create \"{char_name}\" {MANA} 4"))).await?;
        let p = w.world.expect_where(|q| q.is(it, None) && q.int(3) == Some(MANA) && q.int(1) == Some(2), 3000, "potions de mana").await?;
        match grid_of(&p) {
            Some(g) => {
                made = Some((p.int(2).unwrap_or(0), p.int(5).unwrap_or(0), g));
                break;
            }
            // Pile laissée par un essai précédent : on la détruit et on recommence.
            None => {
                let old = p.int(2).unwrap_or(0);
                w.world.send(it, Fields::new(it).with(0, 21).with(2, old)).await?;
                w.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(old), 3000, "ancienne pile détruite").await?;
            }
        }
    }
    let (iid, count, origin_grid) = made.ok_or_else(|| anyhow::anyhow!("pas de pile neuve"))?;
    println!("  ✓ {count} potions en {origin_grid:?}");

    step("séparer");
    w.world.send(it, Fields::new(it).with(0, 17).with(2, iid).with(5, 2).with(7, grid(FREE))).await?;
    let part = w.world.expect_where(|q| q.is(it, Some(0)) && q.int(3) == Some(MANA) && q.int(5) == Some(2), 3000, "nouvelle pile").await?;
    let left = w.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(iid), 3000, "pile d'origine").await?;
    anyhow::ensure!(left.int(5) == Some(count - 2), "pile d'origine à {}", o(left.int(5)));
    let part_iid = part.int(2).unwrap_or(0);
    println!("  ✓ {count} → {} + 2 en {FREE:?}", count - 2);

    step("réunir");
    w.world.send(it, Fields::new(it).with(0, 2).with(1, 2).with(2, part_iid).with(7, grid(origin_grid))).await?;
    w.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(part_iid), 3000, "pile versée").await?;
    let merged = w.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(iid), 3000, "pile réunie").await?;
    anyhow::ensure!(merged.int(5) == Some(count), "pile réunie à {}", o(merged.int(5)));
    println!("  ✓ réunies : {count} potions");

    step("détruire");
    w.world.send(it, Fields::new(it).with(0, 21).with(2, iid)).await?;
    w.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(iid), 3000, "pile détruite").await?;
    println!("  ✓ pile détruite");

    step("résurrection au point de la région");
    w.world.send(ch, say(w.cid, "/kill")).await?;
    w.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(me) && q.int(11) == Some(2), 3000, "mort").await?;
    w.world.send(c, Fields::new(c).with(0, 9).with(1, w.cid)).await?;
    let tp = w.world.expect_where(|q| q.is(mv, None) && q.int(0) == Some(me), 5000, "résurrection").await?;
    let d = tp.fields().vec3(2).unwrap_or_default();
    anyhow::ensure!((d.x + 476_000.0).abs() < 1.0 && (d.z + 61_000.0).abs() < 1.0, "ressuscité en {d:?}");
    println!("  ✓ ressuscité au point de résurrection d'Anchorville : {:.0}, {:.0}", d.x, d.z);
    println!("\ninventaire et résurrection : OK");
    Ok(())
}

/// Quêtes « personnelles » (prises depuis le journal) : Shattered Roots (rapporter 50 Corrupted Root Cores,
/// récompense Tempering Gem), Gladiator's Demise (prise puis abandonnée), Winged Menace (niveau 25 à 40).
async fn quest(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    const ROOTS: i64 = 4632; // niveau 120, 50 × objet 15513
    const CORES: i64 = 15513;
    const GEM: i64 = 65461;
    const GLADIATOR: i64 = 4611; // niveau 99 à 120
    const WINGED: i64 = 4599; // niveau 25 à 40
    let cfg = crate::config::load()?;
    let (qs, ch, it) = (&schema::QUEST, &schema::CHATTING, &schema::ITEM);
    let say = |cid: u32, text: &str| Fields::new(ch).with(0, 0).with(1, cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text));
    let system = |q: &Recv| q.ptype == ch.id && q.int(2) == Some(chat_type::SYSTEM);
    let result = |op: i64, tid: i64| move |q: &Recv| q.is(&schema::QUEST, Some(op)) && q.int(4) == Some(tid);

    step(&format!("{char_name} entre en jeu, niveau 120"));
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let me = w.cid as i64;
    let level = w.level.max(1);
    for cmd in [format!("/questreset {ROOTS}"), format!("/questreset {GLADIATOR}"), "/level 120".to_string()] {
        w.world.send(ch, say(w.cid, &cmd)).await?;
        w.world.expect_where(system, 3000, "réponse MJ").await?;
    }
    let ask = |op: i64, tid: i64| Fields::new(qs).with(0, op).with(1, me).with(4, tid);

    step("prendre Shattered Roots");
    w.world.send(qs, ask(0, ROOTS)).await?;
    let r = w.world.expect_where(result(1, ROOTS), 3000, "acceptation").await?;
    anyhow::ensure!(r.int(6) == Some(1), "refusée : {:?}", r.int(6));
    println!("  ✓ acceptée");

    step("terminer sans les objets : refus");
    w.world.send(qs, ask(2, ROOTS)).await?;
    let r = w.world.expect_where(result(3, ROOTS), 3000, "refus").await?;
    anyhow::ensure!(r.int(6) == Some(0), "terminée sans les objets");
    println!("  ✓ refusée (objectifs non remplis)");

    step("50 Corrupted Root Cores, puis terminer");
    w.world.send(ch, say(w.cid, &format!("/create \"{char_name}\" {CORES} 50"))).await?;
    let made = w.world.expect_where(|q| q.is(it, None) && q.int(3) == Some(CORES), 3000, "objets de quête").await?;
    let cores = made.int(2).unwrap_or(0);
    w.world.send(qs, ask(2, ROOTS)).await?;
    w.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(cores), 3000, "objets rapportés").await?;
    let r = w.world.expect_where(result(3, ROOTS), 3000, "fin").await?;
    anyhow::ensure!(r.int(6) == Some(1), "fin refusée");
    let gem = w.world.expect_where(|q| q.is(it, None) && q.int(3) == Some(GEM), 3000, "récompense").await?;
    let gem_iid = gem.int(2).unwrap_or(0);
    println!("  ✓ terminée : 50 objets repris, Tempering Gem reçue");

    step("refus : déjà terminée, niveau");
    w.world.send(qs, ask(0, ROOTS)).await?;
    anyhow::ensure!(w.world.expect_where(result(1, ROOTS), 3000, "refus").await?.int(6) == Some(0), "reprise acceptée");
    w.world.send(qs, ask(0, WINGED)).await?;
    anyhow::ensure!(w.world.expect_where(result(1, WINGED), 3000, "refus").await?.int(6) == Some(0), "niveau ignoré");
    println!("  ✓ refusées (déjà terminée ; niveau 25 à 40)");

    step("prendre puis abandonner Gladiator's Demise");
    w.world.send(qs, ask(0, GLADIATOR)).await?;
    anyhow::ensure!(w.world.expect_where(result(1, GLADIATOR), 3000, "acceptation").await?.int(6) == Some(1), "refusée");
    w.world.send(qs, ask(6, GLADIATOR)).await?;
    anyhow::ensure!(w.world.expect_where(result(7, GLADIATOR), 3000, "abandon").await?.int(6) == Some(1), "abandon refusé");
    println!("  ✓ prise puis abandonnée");

    step("reconnexion : journal");
    drop(w);
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    anyhow::ensure!(w.quests_done.contains(&(ROOTS as u32)), "Shattered Roots absente des quêtes terminées");
    anyhow::ensure!(!w.quests_active.contains(&GLADIATOR), "quête abandonnée encore au journal");
    println!("  ✓ Shattered Roots terminée, Gladiator's Demise absente du journal");

    // Remise en état : niveau, quête, récompense.
    for cmd in [format!("/questreset {ROOTS}"), format!("/level {level}")] {
        w.world.send(ch, say(w.cid, &cmd)).await?;
        w.world.expect_where(system, 3000, "réponse MJ").await?;
    }
    w.world.send(it, Fields::new(it).with(0, 21).with(2, gem_iid)).await?;
    w.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(gem_iid), 3000, "récompense détruite").await?;
    println!("\nquêtes : OK");
    Ok(())
}

/// Banque chez Holly (gardienne au sud d'Anchorville) : dépôt d'une pile et d'argent, reconnexion, retrait.
async fn bank(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    const KEEPER: Vec3 = Vec3 { x: -480_400.0, y: 4_920.0, z: -69_100.0 };
    const POTION: i64 = 190; // Small Life Potion
    const BANK: i64 = 100;
    const STOP: i64 = 0x04;
    let cfg = crate::config::load()?;
    let (it, ch, c, mv, eb, bk) = (&schema::ITEM, &schema::CHATTING, &schema::CHARACTER, &schema::OPT_CHAR_MOVE, &mini::EVENT_BASE, &schema::EVENT_BANK);
    let say = |cid: u32, text: &str| Fields::new(ch).with(0, 0).with(1, cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text));
    let grid = |g: [i64; 3]| build_mini(&mini::ITEM_GRID, &Fields::new(&mini::ITEM_GRID).with(0, g[0]).with(1, g[1]).with(2, g[2]));

    step(&format!("{char_name} entre en jeu, chez Holly"));
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let me = w.cid as i64;
    let bank0 = w.bank_money;
    let banked0 = w.items.iter().filter(|i| i.2 == BANK).count();
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, KEEPER).with(2, KEEPER).with(5, STOP).with(6, 0)).await?;
    let mut keeper = w.npcs.iter().find(|n| n.1.ends_with(" Holly")).map(|n| n.0);
    while let Ok(p) = w.world.expect_where(|q| q.is(&schema::OPT_VIEW, Some(1)), 1500, "PNJ").await {
        if let Ok(view) = decode_mini(&mini::OPT_VIEW_CHAR, p.fields().bytes(1).unwrap_or_default()) {
            if view.text(2).is_some_and(|n| n.ends_with(" Holly")) {
                keeper = view.int(0);
            }
        }
    }
    let keeper = keeper.ok_or_else(|| anyhow::anyhow!("Holly introuvable"))?;
    println!("  {} objet(s) déjà en banque, {bank0} pièce(s)", banked0);

    step("dépôt refusé sans gardien");
    w.world.send(ch, say(w.cid, &format!("/create \"{char_name}\" {POTION} 3"))).await?;
    let made = w.world.expect_where(|q| q.is(it, None) && q.int(3) == Some(POTION) && q.int(1) == Some(2), 3000, "potions créées").await?;
    let iid = made.int(2).unwrap_or(0);
    w.world.send(it, Fields::new(it).with(0, 2).with(1, BANK).with(2, iid).with(8, grid([0, 6, 3]))).await?;
    w.world.expect_where(|q| q.ptype == ch.id && q.int(2) == Some(chat_type::SYSTEM), 3000, "refus").await?;
    println!("  ✓ refusé (fenêtre fermée)");

    step("ouverture chez Holly");
    let event = build_mini(eb, &Fields::new(eb).with(0, 3).with(1, keeper).with(3, 20));
    w.world.send(bk, Fields::new(bk).with(0, 0).with(1, event.clone()).with(2, w.cid)).await?;
    w.world.expect_where(|q| q.is(bk, Some(1)), 3000, "accord").await?;
    println!("  ✓ banque ouverte ({keeper:#x})");

    step("dépôt d'une pile et de 1000 pièces");
    w.world.send(ch, say(w.cid, "/money 5000")).await?;
    w.world.expect_where(|q| q.is(c, Some(1)) && q.int(8) == Some(5000), 3000, "argent").await?;
    w.world.send(it, Fields::new(it).with(0, 2).with(1, BANK).with(2, iid).with(8, grid([0, 6, 3]))).await?;
    let put = w.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(iid), 3000, "objet en banque").await?;
    anyhow::ensure!(put.int(1) == Some(BANK), "statut {:?}", put.int(1));
    println!("  ✓ pile {iid} en banque");
    w.world.send(c, Fields::new(c).with(0, 7).with(1, w.cid).with(9, 1000)).await?;
    let m = w.world.expect_where(|q| q.is(c, Some(1)) && q.int(9).is_some(), 3000, "argent en banque").await?;
    anyhow::ensure!(m.int(8) == Some(4000) && m.int(9) == Some(bank0 + 1000), "argent {:?}, banque {:?}", m.int(8), m.int(9));
    println!("  ✓ 4000 sur soi, {} en banque", bank0 + 1000);

    step("reconnexion");
    drop(w);
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    anyhow::ensure!(w.items.iter().any(|i| i.0 == iid && i.2 == BANK), "pile absente de la banque à l'entrée");
    anyhow::ensure!(w.bank_money == bank0 + 1000, "banque {} à l'entrée", w.bank_money);
    println!("  ✓ pile et {} pièce(s) retrouvées en banque", w.bank_money);

    step("retrait");
    w.world.send(bk, Fields::new(bk).with(0, 0).with(1, event).with(2, w.cid)).await?;
    w.world.expect_where(|q| q.is(bk, Some(1)), 3000, "accord").await?;
    w.world.send(it, Fields::new(it).with(0, 2).with(1, 2).with(2, iid)).await?;
    let back = w.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(iid), 3000, "objet retiré").await?;
    anyhow::ensure!(back.int(1) == Some(2), "statut {:?}", back.int(1));
    w.world.send(c, Fields::new(c).with(0, 7).with(1, w.cid).with(9, -1000)).await?;
    let m = w.world.expect_where(|q| q.is(c, Some(1)) && q.int(9).is_some(), 3000, "argent retiré").await?;
    anyhow::ensure!(m.int(9) == Some(bank0), "banque {:?}", m.int(9));
    println!("  ✓ pile dans l'inventaire, banque revenue à {bank0} pièce(s) ({me:#x})");
    // La pile créée est détruite.
    w.world.send(it, Fields::new(it).with(0, 21).with(2, iid)).await?;
    w.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(iid), 3000, "pile détruite").await?;
    println!("\nbanque : OK");
    Ok(())
}

/// Échange entre deux joueurs : A (compte MJ) propose une pile et de l'argent, B de l'argent.
async fn private_trade(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    const SPOT: Vec3 = Vec3 { x: -476_000.0, y: 4_422.0, z: -61_000.0 };
    const POTION: i64 = 190;
    const STOP: i64 = 0x04;
    let cfg = crate::config::load()?;
    let (it, ch, c, mv, tr) = (&schema::ITEM, &schema::CHATTING, &schema::CHARACTER, &schema::OPT_CHAR_MOVE, &schema::PRIVATE_TRADE);
    let say = |cid: u32, text: &str| Fields::new(ch).with(0, 0).with(1, cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text));
    let t = |op: i64| Fields::new(tr).with(0, op);
    let is_t = |op: i64| move |q: &Recv| q.is(tr, Some(op));

    step(&format!("{ca} et {cb} entrent en jeu au même endroit"));
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    let mut b = enter_world(&cfg, ub, pb, cb, "B-").await?;
    for w in [&mut a, &mut b] {
        let cid = w.cid;
        w.world.send(mv, Fields::new(mv).with(0, cid).with(1, SPOT).with(2, SPOT).with(5, STOP).with(6, 0)).await?;
    }
    a.world.send(ch, say(a.cid, &format!("/create \"{ca}\" {POTION} 2"))).await?;
    let made = a.world.expect_where(|q| q.is(it, None) && q.int(3) == Some(POTION) && q.int(1) == Some(2), 3000, "potions créées").await?;
    let iid = made.int(2).unwrap_or(0);
    a.world.send(ch, say(a.cid, "/money 10000")).await?;
    a.world.expect_where(|q| q.is(c, Some(1)) && q.int(8) == Some(10_000), 3000, "argent de A").await?;
    a.world.send(ch, say(a.cid, &format!("/money 300 \"{cb}\""))).await?;
    b.world.expect_where(|q| q.is(c, Some(1)) && q.int(8) == Some(300), 3000, "argent de B").await?;
    let (acid, bcid) = (a.cid as i64, b.cid as i64);

    step("demande et acceptation");
    a.world.send(tr, t(1).with(1, acid).with(2, bcid)).await?;
    let ask = b.world.expect_where(is_t(3), 3000, "demande reçue").await?;
    anyhow::ensure!(ask.int(1) == Some(acid), "demandeur {:?}", ask.int(1));
    b.world.send(tr, t(4).with(1, bcid)).await?;
    a.world.expect_where(is_t(6), 3000, "début chez A").await?;
    b.world.expect_where(is_t(6), 3000, "début chez B").await?;
    println!("  ✓ échange ouvert");

    step("A propose une pile et 500 pièces, B 100 pièces");
    a.world.send(tr, t(9).with(1, acid).with(3, iid)).await?;
    a.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(iid) && q.int(1) == Some(6), 3000, "pile dans la grille de A").await?;
    b.world.expect_where(|q| q.is(it, Some(0)) && q.int(2) == Some(iid) && q.int(1) == Some(6) && q.int(4) == Some(acid), 3000, "pile vue par B").await?;
    a.world.send(tr, t(23).with(1, acid).with(5, 500)).await?;
    b.world.expect_where(|q| q.is(tr, Some(24)) && q.int(5) == Some(500), 3000, "argent de A vu par B").await?;
    b.world.send(tr, t(23).with(1, bcid).with(5, 100)).await?;
    a.world.expect_where(|q| q.is(tr, Some(24)) && q.int(5) == Some(100), 3000, "argent de B vu par A").await?;
    // Un objet proposé ne se détruit pas.
    a.world.send(it, Fields::new(it).with(0, 21).with(2, iid)).await?;
    anyhow::ensure!(a.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(iid), 800, "").await.is_err(), "objet proposé détruit");
    println!("  ✓ propositions vues des deux côtés ; l'objet proposé reste intouchable");

    step("verrous, puis échange");
    a.world.send(tr, t(16).with(1, acid)).await?;
    b.world.expect_where(is_t(17), 3000, "A verrouillé").await?;
    b.world.send(tr, t(16).with(1, bcid)).await?;
    a.world.expect_where(is_t(20), 3000, "prêt à échanger (A)").await?;
    b.world.expect_where(is_t(20), 3000, "prêt à échanger (B)").await?;
    a.world.send(tr, t(21).with(1, acid)).await?;
    b.world.expect_where(is_t(22), 3000, "A prêt").await?;
    b.world.send(tr, t(21).with(1, bcid)).await?;
    let got = b.world.expect_where(|q| q.is(it, Some(0)) && q.int(2) == Some(iid) && q.int(4) == Some(bcid) && q.int(1) == Some(2), 3000, "pile reçue par B").await?;
    let bm = b.world.expect_where(|q| q.is(c, Some(1)) && q.int(8).is_some(), 3000, "argent de B après").await?;
    b.world.expect_where(|q| q.is(tr, Some(0)) && q.int(8) == Some(8), 3000, "résultat chez B").await?;
    a.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(iid), 3000, "pile partie de chez A").await?;
    let am = a.world.expect_where(|q| q.is(c, Some(1)) && q.int(8).is_some(), 3000, "argent de A après").await?;
    a.world.expect_where(|q| q.is(tr, Some(0)), 3000, "résultat chez A").await?;
    anyhow::ensure!(am.int(8) == Some(9600) && bm.int(8) == Some(700), "argent A {:?}, B {:?}", am.int(8), bm.int(8));
    println!("  ✓ B a reçu {} potion(s) ; A : 9600 pièces, B : 700", o(got.int(5)));

    step("refus d'une seconde demande");
    a.world.send(tr, t(1).with(1, acid).with(2, bcid)).await?;
    b.world.expect_where(is_t(3), 3000, "seconde demande").await?;
    b.world.send(tr, t(25).with(1, bcid)).await?;
    a.world.expect_where(is_t(5), 3000, "annulation chez A").await?;
    b.world.expect_where(is_t(5), 3000, "annulation chez B").await?;
    println!("  ✓ refusée, échange annulé des deux côtés");

    // B jette la pile reçue.
    b.world.send(it, Fields::new(it).with(0, 21).with(2, iid)).await?;
    b.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(iid), 3000, "pile détruite").await?;
    println!("\néchange : OK");
    Ok(())
}

/// Vitesse d'attaque (facteurs « attaque », valeur 2) d'un mini-paquet de facteurs complet.
fn factors_attack_speed(bytes: &[u8]) -> Option<i64> {
    let f = decode_mini(&mini::CHAR_FACTORS, bytes).ok()?;
    decode_mini(&mini::FACTOR_ATTACK, f.bytes(9)?).ok()?.int(2)
}

/// PV et PM max d'un mini-paquet de facteurs complet (sous-paquet 4).
fn factors_max_points(bytes: &[u8]) -> Option<(i64, i64)> {
    let f = decode_mini(&mini::CHAR_FACTORS, bytes).ok()?;
    let max = decode_mini(&mini::FACTOR_POINTMAX, f.bytes(4)?).ok()?;
    Some((max.int(0)?, max.int(1)?))
}

/// Niveau d'un mini-paquet de facteurs complet (sous-paquet 1, valeur 7).
fn factors_level(bytes: &[u8]) -> Option<i64> {
    let f = decode_mini(&mini::CHAR_FACTORS, bytes).ok()?;
    decode_mini(&mini::FACTOR_STATUS, f.bytes(1)?).ok()?.int(7)
}

/// Effets des compétences d'un guerrier humain : passive, renforcement, zone, affaiblissement.
async fn skill_effects(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    const TRAINER: Vec3 = Vec3 { x: -480_700.0, y: 5_070.0, z: -62_100.0 };
    const HUNT: Vec3 = Vec3 { x: -466_468.0, y: 4_700.0, z: -50_010.0 };
    const SPEED_ATTACK: i64 = 4; // renforcement : vitesse d'attaque +18 %
    const DEFENSE_HOLD: i64 = 58; // passive : défense +20
    const LINEAR_BLOW: i64 = 57; // zone de 1000 autour de la cible
    const BLADE_STING: i64 = 223; // affaiblissement de 6 s (esquive -6)
    const STOP: i64 = 0x04;
    let cfg = crate::config::load()?;
    let (k, sm, ch, mv, ov, c) = (&schema::SKILL, &schema::EVENT_SKILL_MASTER, &schema::CHATTING, &schema::OPT_CHAR_MOVE, &schema::OPT_VIEW, &schema::CHARACTER);
    step(&format!("{char_name} entre en jeu"));
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let me = w.cid as i64;
    let say = |cid: u32, text: &str| Fields::new(ch).with(0, 0).with(1, cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text));
    let system = |q: &Recv| q.ptype == ch.id && q.int(2) == Some(chat_type::SYSTEM);
    let mine = |q: &Recv| q.is(c, Some(1)) && q.int(1) == Some(me) && q.fields().bytes(7).is_some();

    step("niveau 25, argent");
    let level = w.level.max(1);
    w.world.send(ch, say(w.cid, "/level 25")).await?;
    let at_25 = |q: &Recv| mine(q) && q.fields().bytes(7).and_then(factors_level) == Some(25);
    let f = w.world.expect_where(at_25, 3000, "facteurs au niveau 25").await?;
    let bytes = f.fields().bytes(7).unwrap_or_default().to_vec();
    let defense0 = factors_combat(&bytes).map(|x| x.1).unwrap_or(0);
    let speed0 = factors_attack_speed(&bytes).unwrap_or(0);
    let (hp_max, mp_max) = factors_max_points(&bytes).unwrap_or((0, 0));
    // Guerrier humain au niveau 25 (GrowUpFactor) : 456 PV et 108 PM, plus l'équipement éventuel.
    anyhow::ensure!(hp_max >= 456 && mp_max >= 108, "PV/PM max {hp_max}/{mp_max} au niveau 25 (attendu au moins 456/108)");
    println!("  PV/PM max {hp_max}/{mp_max}, défense {defense0}, vitesse d'attaque {speed0}");
    w.world.send(ch, say(w.cid, "/money 100000")).await?;
    w.world.expect_where(|q| q.is(c, Some(1)) && q.int(8) == Some(100_000), 3000, "argent").await?;

    step("apprendre chez Black");
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, TRAINER).with(2, TRAINER).with(5, STOP).with(6, 0)).await?;
    let mut trainer = w.npcs.iter().find(|n| n.1.ends_with(" Black")).map(|n| n.0);
    while let Ok(p) = w.world.expect_where(|q| q.is(ov, Some(1)), 1500, "PNJ").await {
        if let Ok(view) = decode_mini(&mini::OPT_VIEW_CHAR, p.fields().bytes(1).unwrap_or_default()) {
            if view.text(2).is_some_and(|n| n.ends_with(" Black")) {
                trainer = view.int(0);
            }
        }
    }
    let trainer = trainer.ok_or_else(|| anyhow::anyhow!("maître Black introuvable"))?;
    let eb = &mini::EVENT_BASE;
    let event = build_mini(eb, &Fields::new(eb).with(0, 3).with(1, trainer).with(3, 25));
    let mut ids = std::collections::HashMap::new();
    for tid in [SPEED_ATTACK, DEFENSE_HOLD, LINEAR_BLOW, BLADE_STING] {
        if let Some(s) = w.skills.iter().find(|s| s.1 == tid) {
            ids.insert(tid, s.0);
            println!("  TID {tid} déjà connue");
            continue;
        }
        w.world.send(sm, Fields::new(sm).with(0, 1).with(1, event.clone()).with(2, w.cid).with(4, tid)).await?;
        let add = w.world.expect_where(|q| q.is(k, Some(0)) && q.int(4) == Some(tid), 3000, "compétence apprise").await?;
        ids.insert(tid, add.int(1).unwrap_or(0));
        if tid == DEFENSE_HOLD {
            let f = w.world.expect_where(mine, 3000, "facteurs après la passive").await?;
            let defense = f.fields().bytes(7).and_then(factors_combat).map(|x| x.1).unwrap_or(0);
            anyhow::ensure!(defense == defense0 + 20, "défense {defense0} → {defense} (attendu +20)");
            println!("  ✓ Defense Hold : défense {defense0} → {defense}");
        } else {
            println!("  ✓ TID {tid} apprise");
        }
    }
    if w.skills.iter().any(|s| s.1 == DEFENSE_HOLD) {
        // Passive déjà connue : comptée dès l'entrée en jeu.
        println!("  ✓ Defense Hold déjà comptée (défense {defense0})");
    }
    let sb = &mini::SKILL_BASE;
    let sa = &mini::SKILL_ACTION;
    let cast = |id: i64, target: i64| {
        let act = build_mini(sa, &Fields::new(sa).with(0, 0).with(1, build_mini(sb, &Fields::new(sb).with(0, 3).with(1, target))));
        Fields::new(k).with(0, 7).with(1, id).with(6, act)
    };

    step("renforcement : Speed Attack");
    w.world.send(k, cast(ids[&SPEED_ATTACK], me)).await?;
    let buff = w.world.expect_where(|q| q.is(k, Some(4)) && q.int(1) == Some(ids[&SPEED_ATTACK]), 5000, "effet sur soi").await?;
    let owner = decode_mini(sb, buff.fields().bytes(2).unwrap_or_default())?.int(1);
    anyhow::ensure!(owner == Some(me), "effet posé sur {owner:?}");
    let f = w.world.expect_where(mine, 3000, "facteurs renforcés").await?;
    let speed = f.fields().bytes(7).and_then(factors_attack_speed).unwrap_or(0);
    anyhow::ensure!(speed > speed0, "vitesse d'attaque {speed0} → {speed}");
    println!("  ✓ effet affiché ; vitesse d'attaque {speed0} → {speed}");

    step("zone : Linear Blow sur les Vers verts");
    w.world.send(ch, say(w.cid, "/invincible 1")).await?;
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, HUNT).with(2, HUNT).with(5, STOP).with(6, 0)).await?;
    let mut worms: Vec<(i64, Vec3)> = Vec::new();
    while let Ok(p) = w.world.expect_where(|q| q.is(ov, Some(1)), 1500, "monstres").await {
        if let Ok(view) = decode_mini(&mini::OPT_VIEW_CHAR, p.fields().bytes(1).unwrap_or_default()) {
            if view.text(2).as_deref() == Some("Green Worm") && view.bytes(5).and_then(factors_hp).unwrap_or(0) > 0 {
                worms.push((view.int(0).unwrap_or(0), view.vec3(8).unwrap_or_default()));
            }
        }
    }
    anyhow::ensure!(worms.len() >= 2, "{} Ver(s) vert(s) en vue", worms.len());
    // Le ver qui a le plus de voisins à moins de 1000 unités.
    let neighbours = |p: Vec3| worms.iter().filter(|(_, q)| (((q.x - p.x) as f64).powi(2) + ((q.z - p.z) as f64).powi(2)).sqrt() <= 1000.0).count();
    let (center, _) = *worms.iter().max_by_key(|(_, p)| neighbours(*p)).unwrap();
    // /level rend les PM dépensés par les lancements précédents.
    w.world.send(ch, say(w.cid, "/level 25")).await?;
    w.world.expect_where(system, 3000, "PM rendus").await?;
    w.world.send(k, cast(ids[&LINEAR_BLOW], center)).await?;
    w.world.expect_where(|q| q.is(k, Some(7)) && q.int(1) == Some(ids[&LINEAR_BLOW]), 10_000, "lancement").await?;
    let mut hit = std::collections::BTreeSet::new();
    while let Ok(r) = w.world.expect_where(|q| q.is(k, Some(8)) && q.int(1) == Some(ids[&LINEAR_BLOW]), if hit.is_empty() { 5000 } else { 500 }, "résultats").await {
        let act = decode_mini(sa, r.fields().bytes(6).unwrap_or_default())?;
        if let Some(t) = decode_mini(sb, act.bytes(1).unwrap_or_default())?.int(1) {
            hit.insert(t);
        }
    }
    anyhow::ensure!(hit.contains(&center), "la cible désignée n'est pas touchée : {hit:?}");
    println!("  ✓ {} ver(s) touché(s) (cible et {} voisin(s) à moins de 1000)", hit.len(), neighbours(worms.iter().find(|w| w.0 == center).unwrap().1) - 1);

    step("affaiblissement : Blade Sting");
    // Un déplacement arrête l'attaque automatique ; un ver encore intact.
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, HUNT).with(2, HUNT).with(5, STOP).with(6, 0)).await?;
    let (victim, _) = *worms.iter().find(|(c, _)| !hit.contains(c)).unwrap_or(&worms[0]);
    w.world.send(ch, say(w.cid, "/level 25")).await?;
    w.world.expect_where(system, 3000, "PM rendus").await?;
    w.world.send(k, cast(ids[&BLADE_STING], victim)).await?;
    w.world.expect_where(|q| q.is(k, Some(8)) && q.int(1) == Some(ids[&BLADE_STING]), 10_000, "coup").await?;
    let on = w.world.expect_where(|q| q.is(k, Some(4)) && q.int(1) == Some(ids[&BLADE_STING]), 2000, "affaiblissement posé").await?;
    let owner = decode_mini(sb, on.fields().bytes(2).unwrap_or_default())?.int(1);
    anyhow::ensure!(owner == Some(victim), "affaiblissement posé sur {owner:?}, pas sur {victim}");
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, HUNT).with(2, HUNT).with(5, STOP).with(6, 0)).await?;
    let t0 = std::time::Instant::now();
    let off = w
        .world
        .expect_where(|q| (q.is(k, Some(5)) && q.int(1) == Some(ids[&BLADE_STING])) || (q.is(c, Some(1)) && q.int(1) == Some(victim) && q.int(11) == Some(2)), 10_000, "fin de l'affaiblissement")
        .await?;
    if off.ptype == k.id {
        println!("  ✓ posé sur le ver, retiré au bout de {:.1} s", t0.elapsed().as_secs_f64());
    } else {
        println!("  ✓ posé sur le ver, mort avant la fin");
    }

    w.world.send(ch, say(w.cid, "/invincible 0")).await?;
    w.world.send(ch, say(w.cid, &format!("/level {level}"))).await?;
    w.world.expect_where(|q| system(q) && q.fields().text(6).is_some_and(|t| t.contains(&format!("niveau {level}"))), 3000, "niveau rétabli").await?;
    println!("  ✓ niveau {level} rétabli");
    println!("\neffets des compétences : OK");
    Ok(())
}

/// Shield Bash (30 % de chances d'étourdir 3 s) relancé sur des Vers verts jusqu'à un étourdissement : le ver passe
/// à l'état 0x1 (AGPMCHARACTER op 1, champ 17), puis y revient à 0 au bout de 3 s.
async fn skill_status(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    const TRAINER: Vec3 = Vec3 { x: -480_700.0, y: 5_070.0, z: -62_100.0 };
    const HUNT: Vec3 = Vec3 { x: -466_468.0, y: 4_700.0, z: -50_010.0 };
    const SHIELD_BASH: i64 = 7;
    const STUN: i64 = 0x1;
    const STOP: i64 = 0x04;
    const TRIES: usize = 25;
    let cfg = crate::config::load()?;
    let (k, sm, ch, mv, ov, c) = (&schema::SKILL, &schema::EVENT_SKILL_MASTER, &schema::CHATTING, &schema::OPT_CHAR_MOVE, &schema::OPT_VIEW, &schema::CHARACTER);
    step(&format!("{char_name} entre en jeu"));
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let say = |cid: u32, text: &str| Fields::new(ch).with(0, 0).with(1, cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text));
    let system = |q: &Recv| q.ptype == ch.id && q.int(2) == Some(chat_type::SYSTEM);
    let level = w.level.max(1);
    w.world.send(ch, say(w.cid, "/level 25")).await?;
    w.world.expect_where(system, 3000, "niveau 25").await?;
    w.world.send(ch, say(w.cid, "/money 100000")).await?;
    w.world.expect_where(|q| q.is(c, Some(1)) && q.int(8) == Some(100_000), 3000, "argent").await?;

    step("Shield Bash chez Black");
    let id = match w.skills.iter().find(|s| s.1 == SHIELD_BASH) {
        Some(s) => s.0,
        None => {
            w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, TRAINER).with(2, TRAINER).with(5, STOP).with(6, 0)).await?;
            let mut trainer = w.npcs.iter().find(|n| n.1.ends_with(" Black")).map(|n| n.0);
            while let Ok(p) = w.world.expect_where(|q| q.is(ov, Some(1)), 1500, "PNJ").await {
                if let Ok(view) = decode_mini(&mini::OPT_VIEW_CHAR, p.fields().bytes(1).unwrap_or_default()) {
                    if view.text(2).is_some_and(|n| n.ends_with(" Black")) {
                        trainer = view.int(0);
                    }
                }
            }
            let trainer = trainer.ok_or_else(|| anyhow::anyhow!("maître Black introuvable"))?;
            let eb = &mini::EVENT_BASE;
            let event = build_mini(eb, &Fields::new(eb).with(0, 3).with(1, trainer).with(3, 25));
            w.world.send(sm, Fields::new(sm).with(0, 1).with(1, event).with(2, w.cid).with(4, SHIELD_BASH)).await?;
            let add = w.world.expect_where(|q| q.is(k, Some(0)) && q.int(4) == Some(SHIELD_BASH), 3000, "compétence apprise").await?;
            println!("  ✓ apprise");
            add.int(1).unwrap_or(0)
        }
    };

    step("étourdir un Ver vert");
    w.world.send(ch, say(w.cid, "/invincible 1")).await?;
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, HUNT).with(2, HUNT).with(5, STOP).with(6, 0)).await?;
    // Vers vus à l'entrée (déjà sur place) ou en arrivant ; sinon, ceux qui réapparaissent.
    let near_hunt = |p: Vec3| (((p.x - HUNT.x) as f64).powi(2) + ((p.z - HUNT.z) as f64).powi(2)).sqrt() <= 3000.0;
    let mut worms: Vec<i64> = w.monsters.iter().filter(|m| m.name == "Green Worm" && m.hp > 0 && near_hunt(m.pos)).map(|m| m.cid as i64).collect();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let wait = if worms.is_empty() && std::time::Instant::now() < deadline { 5000 } else { 1500 };
        let Ok(p) = w.world.expect_where(|q| q.is(ov, Some(1)), wait, "monstres").await else {
            if worms.is_empty() && std::time::Instant::now() < deadline {
                continue;
            }
            break;
        };
        if let Ok(view) = decode_mini(&mini::OPT_VIEW_CHAR, p.fields().bytes(1).unwrap_or_default()) {
            let cid = view.int(0).unwrap_or(0);
            if view.text(2).as_deref() == Some("Green Worm") && view.bytes(5).and_then(factors_hp).unwrap_or(0) > 0 && !worms.contains(&cid) {
                worms.push(cid);
            }
        }
    }
    anyhow::ensure!(!worms.is_empty(), "aucun Ver vert en vue");
    let sb = &mini::SKILL_BASE;
    let sa = &mini::SKILL_ACTION;
    let cast = |target: i64| {
        let act = build_mini(sa, &Fields::new(sa).with(0, 0).with(1, build_mini(sb, &Fields::new(sb).with(0, 3).with(1, target))));
        Fields::new(k).with(0, 7).with(1, id).with(6, act)
    };
    let dead = |q: &Recv, t: i64| q.is(c, Some(1)) && q.int(1) == Some(t) && q.int(11) == Some(2);
    let mut stunned = None;
    let mut killed = std::collections::BTreeSet::new();
    for attempt in 1..=TRIES {
        let Some(&worm) = worms.iter().find(|t| !killed.contains(*t)) else { break };
        w.world.send(ch, say(w.cid, "/recast")).await?;
        w.world.expect_where(system, 3000, "relance").await?;
        w.world.send(ch, say(w.cid, "/level 25")).await?;
        w.world.expect_where(system, 3000, "PM rendus").await?;
        w.world.send(k, cast(worm)).await?;
        w.world.expect_where(|q| q.is(k, Some(8)) && q.int(1) == Some(id), 10_000, "coup").await?;
        // Un déplacement arrête l'attaque automatique qui suit la compétence.
        w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, HUNT).with(2, HUNT).with(5, STOP).with(6, 0)).await?;
        let state = |q: &Recv| q.is(c, Some(1)) && q.int(1) == Some(worm) && (q.int(17).is_some() || q.int(11) == Some(2));
        match w.world.expect_where(state, 600, "état").await {
            Ok(q) if dead(&q, worm) => {
                killed.insert(worm);
            }
            Ok(q) if q.int(17).unwrap_or(0) & STUN != 0 => {
                println!("  ✓ essai {attempt} : ver {worm:#x} étourdi (état {:#x})", q.int(17).unwrap_or(0));
                stunned = Some(worm);
                break;
            }
            _ => {}
        }
    }
    let worm = stunned.ok_or_else(|| anyhow::anyhow!("aucun étourdissement en {TRIES} essais (chance 30 %)"))?;
    let t0 = std::time::Instant::now();
    let back = w.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(worm) && (q.int(17) == Some(0) || q.int(11) == Some(2)), 6000, "fin de l'étourdissement").await?;
    if dead(&back, worm) {
        println!("  ✓ ver mort avant la fin de l'étourdissement");
    } else {
        let s = t0.elapsed().as_secs_f64();
        anyhow::ensure!((2.0..=4.0).contains(&s), "étourdissement levé au bout de {s:.1} s (attendu 3)");
        println!("  ✓ état revenu à 0 au bout de {s:.1} s");
    }

    w.world.send(ch, say(w.cid, "/invincible 0")).await?;
    w.world.send(ch, say(w.cid, &format!("/level {level}"))).await?;
    w.world.expect_where(|q| system(q) && q.fields().text(6).is_some_and(|t| t.contains(&format!("niveau {level}"))), 3000, "niveau rétabli").await?;
    println!("  ✓ niveau {level} rétabli");
    println!("
états spéciaux : OK");
    Ok(())
}

/// Maître des compétences des guerriers humains (Black, au sud d'Anchorville), puis Dual Slash sur un Ver vert.
async fn skill(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    const TRAINER: Vec3 = Vec3 { x: -480_700.0, y: 5_070.0, z: -62_100.0 };
    const HUNT: Vec3 = Vec3 { x: -466_468.0, y: 4_700.0, z: -50_010.0 };
    const NORMAL_ATTACK: i64 = 222;
    const DUAL_SLASH: i64 = 1;
    const FLAME_CORE: i64 = 11; // mage
    const STOP: i64 = 0x04;
    let cfg = crate::config::load()?;
    let (k, sm, ch, mv, ov, c) = (&schema::SKILL, &schema::EVENT_SKILL_MASTER, &schema::CHATTING, &schema::OPT_CHAR_MOVE, &schema::OPT_VIEW, &schema::CHARACTER);
    step(&format!("{char_name} entre en jeu"));
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let me = w.cid as i64;
    let say = |cid: u32, text: &str| Fields::new(ch).with(0, 0).with(1, cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text));
    let system = |q: &Recv| q.ptype == ch.id && q.int(2) == Some(chat_type::SYSTEM);
    anyhow::ensure!(w.skills.iter().any(|s| s.1 == NORMAL_ATTACK), "Normal Attack absente : {:?}", w.skills);
    println!("  ✓ Normal Attack connue");

    step("niveau 10, argent");
    let level = w.level.max(1);
    w.world.send(ch, say(w.cid, "/level 10")).await?;
    let r = w.world.expect_where(system, 3000, "réponse à /level").await?;
    println!("  {}", r.fields().text(6).unwrap_or_default());
    w.world.send(ch, say(w.cid, "/money 100000")).await?;
    w.world.expect_where(|q| q.is(c, Some(1)) && q.int(8) == Some(100_000), 3000, "argent").await?;

    step("maître des compétences");
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, TRAINER).with(2, TRAINER).with(5, STOP).with(6, 0)).await?;
    let mut trainer = w.npcs.iter().find(|n| n.1.ends_with(" Black")).map(|n| n.0);
    while let Ok(p) = w.world.expect_where(|q| q.is(ov, Some(1)), 1500, "PNJ").await {
        if let Ok(view) = decode_mini(&mini::OPT_VIEW_CHAR, p.fields().bytes(1).unwrap_or_default()) {
            if view.text(2).is_some_and(|n| n.ends_with(" Black")) {
                trainer = view.int(0);
            }
        }
    }
    let trainer = trainer.ok_or_else(|| anyhow::anyhow!("maître Black introuvable"))?;
    let eb = &mini::EVENT_BASE;
    let event = build_mini(eb, &Fields::new(eb).with(0, 3).with(1, trainer).with(3, 25));
    let master = |op: i64| Fields::new(sm).with(0, op).with(1, event.clone()).with(2, w.cid);
    w.world.send(sm, master(4)).await?;
    w.world.expect_where(|q| q.is(sm, Some(5)) && q.int(5) == Some(0), 3000, "fenêtre du maître").await?;
    println!("  ✓ fenêtre ouverte ({trainer:#x})");

    step("apprendre Dual Slash");
    w.world.send(sm, master(1).with(4, DUAL_SLASH)).await?;
    let id = match w.skills.iter().find(|s| s.1 == DUAL_SLASH) {
        Some(s) => {
            w.world.expect_where(|q| q.is(sm, Some(6)) && q.int(5) == Some(2), 3000, "déjà apprise").await?;
            println!("  ✓ déjà apprise (niveau {}) : refus « déjà apprise »", s.2);
            s.0
        }
        None => {
            let add = w.world.expect_where(|q| q.is(k, Some(0)) && q.int(4) == Some(DUAL_SLASH), 3000, "compétence ajoutée").await?;
            w.world.expect_where(|q| q.is(sm, Some(6)) && q.int(5) == Some(0), 3000, "apprise").await?;
            let info = skill_info(add.packet.as_ref().unwrap());
            println!("  ✓ apprise : identifiant {}, niveau {}", info.0, info.2);
            info.0
        }
    };

    step("refus : compétence de mage");
    w.world.send(sm, master(1).with(4, FLAME_CORE)).await?;
    w.world.expect_where(|q| q.is(sm, Some(6)) && q.int(5) == Some(5), 3000, "refus de classe").await?;
    println!("  ✓ Flame Core refusée (classe)");

    step("améliorer Dual Slash");
    w.world.send(sm, master(2).with(3, id)).await?;
    let r = w.world.expect_where(|q| q.is(k, Some(2)) && q.int(1) == Some(id) || system(q), 3000, "amélioration").await?;
    if r.ptype == k.id {
        w.world.expect_where(|q| q.is(sm, Some(3)) && q.int(3) == Some(id), 3000, "réponse d'amélioration").await?;
        println!("  ✓ niveau {}", skill_info(r.packet.as_ref().unwrap()).2);
    } else {
        let text = r.fields().text(6).unwrap_or_default();
        anyhow::ensure!(text.starts_with("Impossible d'améliorer") || text.contains("niveau maximal"), "réponse inattendue : {text}");
        println!("  ✓ {text}");
    }

    step("Dual Slash sur un Ver vert");
    w.world.send(ch, say(w.cid, "/invincible 1")).await?;
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, HUNT).with(2, HUNT).with(5, STOP).with(6, 0)).await?;
    let mut worm = None;
    while let Ok(p) = w.world.expect_where(|q| q.is(ov, Some(1)), 1500, "monstres").await {
        if let Ok(view) = decode_mini(&mini::OPT_VIEW_CHAR, p.fields().bytes(1).unwrap_or_default()) {
            if view.text(2).as_deref() == Some("Green Worm") && view.bytes(5).and_then(factors_hp).unwrap_or(0) > 0 {
                worm.get_or_insert(view.int(0).unwrap_or(0));
            }
        }
    }
    let worm = worm.ok_or_else(|| anyhow::anyhow!("aucun Ver vert en vue"))?;
    let sb = &mini::SKILL_BASE;
    let sa = &mini::SKILL_ACTION;
    let cast = |target: i64| {
        let act = build_mini(sa, &Fields::new(sa).with(0, 0).with(1, build_mini(sb, &Fields::new(sb).with(0, 3).with(1, target))));
        Fields::new(k).with(0, 7).with(1, id).with(6, act)
    };
    w.world.send(k, cast(worm)).await?;
    let started = w.world.expect_where(|q| q.is(k, Some(7)) && q.int(1) == Some(id), 10_000, "lancement").await?;
    let act = decode_mini(sa, started.fields().bytes(6).unwrap_or_default())?;
    println!("  ✓ lancement : incantation {} ms, relance {} ms, niveau {}", o(act.int(5)), o(act.int(7)), o(act.int(8)));
    let hit = w.world.expect_where(|q| q.is(k, Some(8)) && q.int(1) == Some(id), 5000, "résultat").await?;
    let act = decode_mini(sa, hit.fields().bytes(6).unwrap_or_default())?;
    let target = decode_mini(sb, act.bytes(1).unwrap_or_default())?.int(1);
    anyhow::ensure!(target == Some(worm), "cible {target:?} au lieu de {worm}");
    let result = decode_mini(&mini::CHAR_FACTORS, act.bytes(2).unwrap_or_default())?;
    let shown = decode_mini(&mini::CHAR_FACTORS, result.bytes(0).unwrap_or_default())?;
    let points = decode_mini(&mini::FACTOR_POINT, shown.bytes(3).unwrap_or_default())?;
    let damage = -points.int(9).unwrap_or(0);
    anyhow::ensure!(damage > 0, "pas de dégâts");
    println!("  ✓ {damage} dégât(s), {} PV restants au ver", o(points.int(0)));

    step("relance refusée pendant le délai");
    w.world.send(k, cast(worm)).await?;
    anyhow::ensure!(w.world.expect_where(|q| q.is(k, Some(7)) && q.int(1) == Some(id), 1500, "relance").await.is_err(), "relance acceptée trop tôt");
    println!("  ✓ refusée");

    // Un déplacement arrête l'attaque automatique reprise après la compétence.
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, HUNT).with(2, HUNT).with(5, STOP).with(6, 0)).await?;
    w.world.send(ch, say(w.cid, "/invincible 0")).await?;
    w.world.send(ch, say(w.cid, &format!("/level {level}"))).await?;
    w.world.expect_where(|q| system(q) && q.fields().text(6).is_some_and(|t| t.contains(&format!("niveau {level}"))), 3000, "niveau rétabli").await?;
    println!("  ✓ niveau {level} rétabli ({me:#x})");
    println!("\ncompétences : OK");
    Ok(())
}

async fn shop(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    const DARRYL: Vec3 = Vec3 { x: -479_091.0, y: 4753.0, z: -66_202.0 };
    const DARRYL_CID: u32 = 0x4000_0000 + 3;
    const POTION_TID: i64 = 190;
    let cfg = crate::config::load()?;
    step("préparation : 1000 pièces d'or");
    let db = crate::db::Databases::new(&cfg.database);
    db.characters
        .exec("UPDATE characters SET money = 1000, pos_x = ?, pos_y = ?, pos_z = ? WHERE name = ?", (DARRYL.x as f64 + 200.0, DARRYL.y as f64, DARRYL.z as f64, char_name.to_string()))
        .await?;
    db.close().await;

    step(&format!("{char_name} entre en jeu près de Darryl"));
    let mut a = enter_world(&cfg, user, pass, char_name, "A-").await?;
    let near = Vec3::new(DARRYL.x + 200.0, DARRYL.y, DARRYL.z);
    let mv = &schema::OPT_CHAR_MOVE;
    a.world.send(mv, Fields::new(mv).with(0, a.cid).with(1, near).with(2, near).with(5, 0x04).with(6, 0)).await?;

    step("ouverture de la boutique");
    let eb = &mini::EVENT_BASE;
    let base = build_mini(eb, &Fields::new(eb).with(0, 3).with(1, DARRYL_CID).with(3, 9));
    let tr = &schema::EVENT_NPC_TRADE;
    a.world.send(tr, Fields::new(tr).with(0, base.clone()).with(1, 0).with(2, a.cid)).await?;
    let mut items: Vec<(i64, Fields)> = Vec::new();
    loop {
        let p = a.world.expect_where(|p| p.is_trade(), 5000, "fin de liste (op 1)").await?;
        match p.fields().int(1) {
            Some(0) => items.push((p.fields().int(3).unwrap_or(0), decode_mini(&mini::NPC_TRADE_DETAIL, p.fields().bytes(4).unwrap_or_default())?)),
            Some(1) => break,
            _ => {}
        }
    }
    println!("  ✓ {} objet(s) reçus", items.len());
    let (iid, detail) = items.iter().find(|(_, d)| d.int(0) == Some(POTION_TID)).ok_or_else(|| anyhow::anyhow!("potion absente de la boutique"))?;
    println!("  ✓ potion TID {POTION_TID} : IID {iid}, prix {}, case {}/{}/{}", o(detail.int(1)), o(detail.int(4)), o(detail.int(5)), o(detail.int(6)));

    step("achat de 3 potions");
    let d = &mini::NPC_TRADE_DETAIL;
    a.world.send(tr, Fields::new(tr).with(0, base.clone()).with(1, 2).with(2, a.cid).with(3, *iid).with(4, build_mini(d, &Fields::new(d).with(2, 3)))).await?;
    let mut bought: Option<Fields> = None;
    loop {
        let p = a.world.expect_where(|p| p.is(&schema::ITEM, Some(0)) || (p.is_trade() && p.fields().int(1) == Some(2)), 5000, "résultat d'achat").await?;
        if p.ptype == schema::ITEM.id {
            if p.fields().int(3) == Some(POTION_TID) && p.fields().int(1) == Some(2) {
                bought = Some(p.fields().clone());
            }
            continue;
        }
        anyhow::ensure!(p.fields().int(6) == Some(2), "achat refusé : résultat {:?}", p.fields().int(6));
        break;
    }
    let bought = bought.ok_or_else(|| anyhow::anyhow!("objet acheté non reçu"))?;
    let grid = decode_mini(&mini::ITEM_GRID, bought.bytes(7).unwrap_or_default())?;
    println!("  ✓ achat réussi : IID {}, quantité {}, case {}/{}/{}", o(bought.int(2)), o(bought.int(5)), o(grid.int(0)), o(grid.int(1)), o(grid.int(2)));

    step("revente");
    a.world.send(tr, Fields::new(tr).with(0, base).with(1, 3).with(2, a.cid).with(3, bought.int(2).unwrap_or(0))).await?;
    let sell = a.world.expect_where(|p| p.is_trade() && p.fields().int(1) == Some(3), 5000, "résultat de vente").await?;
    anyhow::ensure!(sell.fields().int(6) == Some(0), "vente refusée : résultat {:?}", sell.fields().int(6));
    println!("  ✓ vente réussie");
    println!("\nboutique : OK");
    Ok(())
}

impl Recv {
    fn is_trade(&self) -> bool {
        self.ptype == schema::EVENT_NPC_TRADE.id && self.packet.is_some()
    }
}
