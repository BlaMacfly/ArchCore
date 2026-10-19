//! Serveur monde.
//!
//! ```text
//!   AGSMCHARMANAGER (0x03) op 2 GAME_ENTER : nom + clé d'auth ; op 13 = keepalive
//!       → AGPMCONFIG, AGPMCHARACTER op 25, AGSMCHARMANAGER op 8 (position de chargement), AGPMTIMER
//!   AGSMCHARMANAGER op 9 ENTER_WORLD
//!       → paquet personnage complet, AGPMADMIN (niveau MJ), vue op 2 (inventaire), déplacement, PNJ,
//!         AGSMCHARMANAGER op 7, mises à jour
//!   AGPMCHATTING (0x0F) op 0 : message ; « /… » = commande MJ (gm.rs)
//!   AGPMOPTIMIZEDCHARMOVE (0x40) : déplacement (presence.rs)
//!   AGPMEVENTNPCDIALOG (0x37), AGCMEVENTNPCTRADE (0x32) : PNJ (npc_events.rs, npc_trade.rs)
//! ```
//! L'état du monde est partagé derrière un verrou : les paquets de tous les joueurs sont traités un à
//! un, comme dans la boucle unique du serveur Node.

pub mod addon;
pub mod ai;
pub mod chat;
pub mod combat;
pub mod gm;
pub mod items;
pub mod map;
pub mod monsters;
pub mod npc_events;
pub mod npc_trade;
pub mod npcs;
pub mod presence;

use crate::character::{character_packet, factors_mini, make_factors, move_mini, CharView, CharacterRow, Factors, MoveMini, Templates};
use crate::config::Config;
use crate::db::{col, Databases};
use crate::net::{Connection, Service, SessionHandle};
use crate::{debug, info, warn};
use archcore_net::schema::{self, char_mgr_op, chat_type, ptype};
use archcore_net::{build_packet, Fields, Packet, Vec3};
use async_trait::async_trait;
use items::{item_add, item_status, Inventory, ItemCatalog};
use npcs::NpcRegistry;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

/// Drapeaux de déplacement : 0x04 arrêt + 0x08 synchronisation.
pub const MOVE_STOP_SYNC: i64 = 0x0c;

pub struct WorldCtx {
    pub db: Databases,
    pub templates: Arc<Templates>,
    pub config: Arc<Config>,
    pub npcs: NpcRegistry,
    pub catalog: ItemCatalog,
    /// Carte serveur : sol infranchissable, régions.
    pub map: map::WorldMap,
    /// Expérience nécessaire pour passer chaque niveau (indice = niveau, LevelUpExp.txt).
    pub level_exp: Vec<u64>,
    pub view_radius: f64,
}

#[derive(Clone, Debug)]
pub struct Account {
    pub id: u32,
    pub username: String,
}

/// Boutique ouverte par un joueur.
#[derive(Clone, Debug)]
pub struct OpenShop {
    pub npc_cid: u32,
    pub items: Vec<ShopItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShopItem {
    pub iid: u32,
    pub tid: u32,
    pub count: i64,
    pub price: i64,
    pub grid: [i64; 3],
}

/// Un joueur connecté au serveur monde.
pub struct Player {
    pub sess: SessionHandle,
    pub account: Account,
    pub gm_level: i64,
    pub ch: CharacterRow,
    pub factors: Factors,
    pub seen_npcs: HashSet<u32>,
    pub seen_monsters: HashSet<u32>,
    pub seen_players: HashSet<u32>,
    pub in_game: bool,
    pub dest: Option<Vec3>,
    pub move_flags: i64,
    pub direction: i64,
    pub follow_target: i64,
    pub follow_distance: i64,
    pub dirty: bool,
    pub move_count: u32,
    pub inventory: Inventory,
    pub shop: Option<OpenShop>,
    /// Attaque en cours (monstre visé).
    pub attack: Option<combat::Attack>,
    /// Mort, en attente de résurrection.
    pub dead: bool,
    /// Invincible jusqu'à cet instant (après une résurrection).
    pub invincible_until: Option<Instant>,
    /// Limite de débit des messages d'AddOn.
    pub addon_bucket: addon::TokenBucket,
}

impl Player {
    pub fn view(&self) -> CharView {
        CharView::from(&self.ch)
    }
    pub fn send(&self, schema: &archcore_net::Schema, fields: Fields) {
        self.sess.send(schema, fields);
    }
    pub fn send_raw(&self, plain: Vec<u8>) {
        self.sess.send_raw(plain);
    }
}

pub struct RestartState {
    pub task: tokio::task::JoinHandle<()>,
}

#[derive(Default)]
pub struct WorldState {
    /// Rayon de vue (config.world.viewRadius).
    pub view_radius: f64,
    /// Joueurs en jeu, par identifiant de session.
    pub players: HashMap<u32, Player>,
    /// CID → identifiant de session.
    pub by_cid: HashMap<u32, u32>,
    pub rates: gm::ServerRates,
    /// Identifiant de personnage → fin de l'interdiction de discussion.
    pub chat_bans: HashMap<u32, Instant>,
    pub restart: Option<RestartState>,
    /// Monstres du monde.
    pub monsters: monsters::MonsterRegistry,
}

impl WorldState {
    pub fn sids(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.players.keys().copied().collect();
        v.sort_unstable();
        v
    }

    pub fn by_name(&self, name: &str) -> Option<u32> {
        let key = name.to_lowercase();
        self.players.iter().find(|(_, p)| p.ch.name.to_lowercase() == key).map(|(sid, _)| *sid)
    }
}

pub struct World {
    pub ctx: WorldCtx,
    pub state: Mutex<WorldState>,
}

#[derive(Clone)]
pub struct WorldService(pub Arc<World>);

impl Service for WorldService {
    fn handles(&self, t: u8) -> bool {
        matches!(
            t,
            ptype::CHAR_MANAGER
                | ptype::CHARACTER
                | ptype::CHATTING
                | ptype::OPT_CHAR_MOVE
                | ptype::OPT_CHAR_ACTION
                | ptype::EVENT_NPC_DIALOG
                | ptype::EVENT_NPC_TRADE
                | ptype::ARCHCORE_EXT
        )
    }
    fn connect(&self, session: &SessionHandle) -> Box<dyn Connection> {
        Box::new(WorldConn { world: self.0.clone(), sid: session.id() })
    }
}

struct WorldConn {
    world: Arc<World>,
    sid: u32,
}

#[async_trait]
impl Connection for WorldConn {
    async fn on_packet(&mut self, s: &SessionHandle, p: Packet) -> anyhow::Result<()> {
        let world = self.world.clone();
        let mut st = world.state.lock().await;
        let ctx = &world.ctx;
        match p.header.ptype {
            ptype::CHAR_MANAGER => {
                let v = &p;
                debug!(
                    "[world] #{} AGSMCHARMANAGER op={:?} f2={:?} f3={:?} f4={:?} f6={:?}",
                    s.id(), v.int(0), v.int(2), v.text(3), v.int(4), v.int(6)
                );
                match p.int(0).unwrap_or(-1) {
                    char_mgr_op::GAME_ENTER => game_enter(ctx, &mut st, s, &p).await?,
                    char_mgr_op::KEEPALIVE => {}
                    char_mgr_op::ENTER_WORLD => enter_world(ctx, &mut st, s),
                    op => warn!("[world] #{} AGSMCHARMANAGER op {op} non géré", s.id()),
                }
            }
            ptype::CHARACTER if p.int(0) == Some(combat::REQUEST_RESURRECTION_TOWN) => combat::resurrect(ctx, &mut st, self.sid).await?,
            ptype::CHARACTER => debug!("[world] #{} AGPMCHARACTER op {:?} (ignoré)", s.id(), p.int(0)),
            ptype::CHATTING => chat::on_chatting(&world, &mut st, self.sid, &p).await?,
            ptype::OPT_CHAR_MOVE => presence::on_move(ctx, &mut st, self.sid, &p),
            ptype::OPT_CHAR_ACTION => combat::on_action(&mut st, self.sid, &p),
            ptype::EVENT_NPC_DIALOG => npc_events::on_dialog(ctx, &st, self.sid, &p),
            ptype::EVENT_NPC_TRADE => npc_trade::on_packet(ctx, &mut st, self.sid, &p).await?,
            ptype::ARCHCORE_EXT => addon::on_packet(ctx, &mut st, self.sid, &p),
            _ => {}
        }
        Ok(())
    }

    async fn on_close(&mut self, _s: &SessionHandle) {
        let world = self.world.clone();
        let mut st = world.state.lock().await;
        if let Err(e) = presence::leave(&world.ctx, &mut st, self.sid).await {
            warn!("[world] départ de #{} : {e:#}", self.sid);
        }
        if let Some(p) = st.players.remove(&self.sid) {
            if st.by_cid.get(&p.ch.id) == Some(&self.sid) {
                st.by_cid.remove(&p.ch.id);
            }
        }
    }
}

async fn game_enter(ctx: &WorldCtx, st: &mut WorldState, s: &SessionHandle, p: &Packet) -> anyhow::Result<()> {
    let raw_name = p.text(3).unwrap_or_default();
    let name = raw_name.split('#').next().unwrap_or("").trim().to_string();
    let auth_key = p.int(6).unwrap_or(0);
    let Some(acc) = ctx.db.auth.first("SELECT id, username, gm_level FROM account WHERE auth_token = ?", (auth_key,)).await? else {
        warn!("[world] #{} clé d'auth {auth_key} inconnue pour {raw_name:?}", s.id());
        s.close();
        return Ok(());
    };
    let account = Account { id: col(&acc, "id"), username: col(&acc, "username") };
    let Some(row) = ctx
        .db
        .characters
        .first("SELECT * FROM characters WHERE account_id = ? AND name = ? AND deleted_at IS NULL", (account.id, name.clone()))
        .await?
    else {
        warn!("[world] #{} personnage {name:?} absent pour le compte {}", s.id(), account.username);
        s.close();
        return Ok(());
    };
    let ch = CharacterRow::from_row(&row);
    let Some(tpl) = ctx.templates.get(ch.template_id) else {
        crate::error!("[world] template {} inconnu", ch.template_id);
        s.close();
        return Ok(());
    };
    let gm_level: i64 = col(&acc, "gm_level");
    let factors = make_factors(tpl, Some(&ch));
    let inventory = Inventory::load(&ctx.db, ch.id).await?;
    info!(
        "[world] #{} {} entre avec {} (cid {}{})",
        s.id(),
        account.username,
        ch.name,
        ch.id,
        if gm_level > 0 { format!(", MJ {gm_level}") } else { String::new() }
    );
    let pos = ch.pos();
    s.send(&schema::CONFIG, Fields::new(&schema::CONFIG).with(0, 0).with(1, 0).with(2, 0));
    s.send(&schema::CHARACTER, Fields::new(&schema::CHARACTER).with(0, 25).with(1, ch.id).with(23, 0));
    let cm = &schema::CHAR_MANAGER;
    s.send(cm, Fields::new(cm).with(0, char_mgr_op::LOADING_POS).with(4, ch.id).with(5, pos));
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    s.send(&schema::TIMER, Fields::new(&schema::TIMER).with(0, 6).with(1, now_ms));
    ctx.db.characters.exec("UPDATE characters SET last_played = NOW() WHERE id = ?", (ch.id,)).await?;

    let cid = ch.id;
    st.players.insert(
        s.id(),
        Player {
            sess: s.clone(),
            account,
            gm_level,
            ch,
            factors,
            seen_npcs: HashSet::new(),
            seen_monsters: HashSet::new(),
            seen_players: HashSet::new(),
            in_game: false,
            dest: None,
            move_flags: 0,
            direction: 0,
            follow_target: 0,
            follow_distance: 0,
            dirty: false,
            move_count: 0,
            inventory,
            shop: None,
            attack: None,
            dead: false,
            invincible_until: None,
            addon_bucket: addon::TokenBucket::default(),
        },
    );
    st.by_cid.insert(cid, s.id());
    Ok(())
}

fn enter_world(ctx: &WorldCtx, st: &mut WorldState, s: &SessionHandle) {
    let sid = s.id();
    let Some(p) = st.players.get(&sid) else {
        warn!("[world] #{sid} ENTER_WORLD sans personnage");
        s.close();
        return;
    };
    let pos = p.ch.pos();
    p.send_raw(character_packet(&p.view(), &p.factors, 0));
    // Niveau MJ : le client l'accroche à un personnage déjà connu (CID), d'où l'envoi après le paquet personnage.
    if p.gm_level > 0 {
        p.send_raw(chat::admin_level_packet(p.ch.id, &p.ch.name, p.gm_level, &p.account.username));
    }
    // Inventaire : paquets AGPMITEM complets mis bout à bout dans le champ 6 (« tous les objets ») de la
    // vue optimisée op 2 ; le client rattache chaque objet au personnage déjà reçu (champ 4).
    let all_items: Vec<u8> = p.inventory.add_packets().concat();
    info!("[world] #{sid} {} objet(s) d'inventaire envoyé(s)", p.inventory.items.len());
    let ov = &schema::OPT_VIEW;
    let empty = Vec::<u8>::new();
    p.send(ov, Fields::new(ov).with(0, 2).with(6, all_items).with(7, empty.clone()).with(8, empty.clone()));
    p.send(ov, Fields::new(ov).with(0, 2).with(6, empty.clone()).with(7, empty));
    let mv = &schema::OPT_CHAR_MOVE;
    p.send(mv, Fields::new(mv).with(0, p.ch.id).with(1, pos).with(2, pos).with(5, MOVE_STOP_SYNC).with(6, 0));
    let sent = send_nearby_npcs(ctx, st, sid, pos);
    info!("[world] #{sid} {sent} PNJ et {} monstre(s) envoyés dans un rayon de {}", st.players[&sid].seen_monsters.len(), ctx.view_radius);
    let p = &st.players[&sid];
    let cm = &schema::CHAR_MANAGER;
    p.send(cm, Fields::new(cm).with(0, char_mgr_op::FINISH_ENTER).with(4, p.ch.id));
    presence::enter(ctx, st, sid);
    let p = &st.players[&sid];
    let c = &schema::CHARACTER;
    p.send(c, Fields::new(c).with(0, 1).with(1, p.ch.id).with(4, 2).with(5, move_mini(MoveMini { pos, dest: None, flags: 0 })));
    p.send(c, Fields::new(c).with(0, 1).with(1, p.ch.id).with(4, 2).with(7, factors_mini(&p.factors)));
    let region = ctx.map.region(pos.x as f64, pos.z as f64).map(|r| r.to_string()).unwrap_or_else(|| "inconnue".into());
    info!("[world] #{sid} {} est en jeu (région {region})", p.ch.name);
}

/// PNJ autour de `pos` pas encore envoyés à ce joueur (vue optimisée op 1, un paquet par PNJ) ; met aussi
/// à jour les monstres visibles (voir `sync_monsters`). Rend le nombre de PNJ envoyés.
pub fn send_nearby_npcs(ctx: &WorldCtx, st: &mut WorldState, sid: u32, pos: Vec3) -> usize {
    let Some(p) = st.players.get_mut(&sid) else { return 0 };
    let mut sent = 0;
    for npc in ctx.npcs.near(pos, ctx.view_radius) {
        if p.seen_npcs.insert(npc.cid) {
            p.sess.send_raw(NpcRegistry::view_packet(npc));
            sent += 1;
        }
    }
    sync_monsters(ctx, st, sid, pos);
    sent
}

/// Montre les monstres entrés dans le rayon de vue et retire ceux qui en sont sortis. Le retrait se fait
/// un peu au-delà du rayon, pour ne pas faire clignoter un monstre en bordure. Rend (ajoutés, retirés).
pub fn sync_monsters(ctx: &WorldCtx, st: &mut WorldState, sid: u32, pos: Vec3) -> (usize, usize) {
    let Some(p) = st.players.get_mut(&sid) else { return (0, 0) };
    let mut added = 0;
    for m in st.monsters.near(pos, ctx.view_radius).filter(|m| m.is_visible()) {
        if p.seen_monsters.insert(m.cid) {
            p.sess.send_raw(st.monsters.view_packet(m));
            added += 1;
        }
    }
    let keep = (ctx.view_radius * 1.2).powi(2);
    let gone: Vec<u32> = p
        .seen_monsters
        .iter()
        .copied()
        .filter(|cid| {
            st.monsters.get(*cid).is_none_or(|m| !m.is_visible() || {
                let (dx, dz) = ((m.pos.x - pos.x) as f64, (m.pos.z - pos.z) as f64);
                dx * dx + dz * dz > keep
            })
        })
        .collect();
    for cid in &gone {
        p.seen_monsters.remove(cid);
        p.sess.send_raw(character_remove_packet(*cid));
    }
    (added, gone.len())
}

// ---------------------------------------------------------------------------------------------
// Actions utilisées par les commandes MJ et les boutiques
// ---------------------------------------------------------------------------------------------

/// Message système (type 7) à un joueur, une ligne par paquet.
pub fn send_system(st: &WorldState, sid: u32, text: &str) {
    if let Some(p) = st.players.get(&sid) {
        for line in text.split('\n') {
            p.send_raw(chat::chat_packet(chat_type::SYSTEM, None, None, None, line));
        }
    }
}

/// Annonce (type 10) à tous les joueurs en jeu.
pub fn broadcast_notice(st: &WorldState, text: &str) {
    let pkt = chat::chat_packet(chat_type::NOTICE, None, None, None, text);
    for p in st.players.values() {
        p.send_raw(pkt.clone());
    }
    info!("[annonce] {text}");
}

/// Argent de l'inventaire d'un joueur en jeu : enregistré puis envoyé au client (AGPMCHARACTER champ 8).
pub async fn set_money(ctx: &WorldCtx, st: &mut WorldState, sid: u32, money: i64) -> anyhow::Result<()> {
    let Some(p) = st.players.get_mut(&sid) else { return Ok(()) };
    p.ch.money = money.clamp(0, npc_trade::MAX_MONEY) as u64;
    ctx.db.characters.exec("UPDATE characters SET money = ? WHERE id = ?", (p.ch.money, p.ch.id)).await?;
    let c = &schema::CHARACTER;
    p.send(c, Fields::new(c).with(0, 1).with(1, p.ch.id).with(8, p.ch.money));
    Ok(())
}

pub fn kill_character(st: &mut WorldState, sid: u32) {
    let Some(p) = st.players.get(&sid) else { return };
    info!("[mj] {} tué", p.ch.name);
    combat::player_death(st, sid);
}

/// Personnage par nom : en jeu d'abord, sinon en base. Rend (id, nom exact).
pub async fn find_character(ctx: &WorldCtx, st: &WorldState, name: &str) -> anyhow::Result<Option<(u32, String)>> {
    if let Some(sid) = st.by_name(name) {
        let p = &st.players[&sid];
        return Ok(Some((p.ch.id, p.ch.name.clone())));
    }
    let row = ctx.db.characters.first("SELECT id, name FROM characters WHERE name = ? AND deleted_at IS NULL", (name.to_string(),)).await?;
    Ok(row.map(|r| (col(&r, "id"), col(&r, "name"))))
}

/// Ajoute un objet à l'inventaire (joueur en jeu : il apparaît tout de suite).
pub async fn create_item(ctx: &WorldCtx, st: &mut WorldState, name: &str, tid: u32, count: i64, bound: bool) -> anyhow::Result<Result<(String, String), &'static str>> {
    let Some((cid, ch_name)) = find_character(ctx, st, name).await? else { return Ok(Err("character")) };
    let Some(item) = ctx.db.world.first("SELECT tid, name FROM item_template WHERE tid = ?", (tid,)).await? else { return Ok(Err("item")) };
    let item_name: String = col(&item, "name");
    let note = if bound { " (non échangeable : non enregistré)" } else { "" };
    if let Some(&sid) = st.by_cid.get(&cid) {
        let stack = ctx.catalog.get(tid).map(|d| d.stack).unwrap_or(1);
        let p = st.players.get_mut(&sid).unwrap();
        let Some((it, _)) = p.inventory.add(&ctx.db, tid, count, stack, None).await? else { return Ok(Err("full")) };
        p.send_raw(item_add(it.iid, it.tid, Some(cid), it.count, item_status::INVENTORY, Some(it.grid)));
        info!("[mj] {count} × objet {tid} ({item_name}) donné à {ch_name}{note}");
        return Ok(Ok((item_name, ch_name)));
    }
    let mut inv = Inventory::load(&ctx.db, cid).await?;
    if inv.add(&ctx.db, tid, count, 1, None).await?.is_none() {
        return Ok(Err("full"));
    }
    info!("[mj] {count} × objet {tid} ({item_name}) donné à {ch_name}{note}");
    Ok(Ok((item_name, ch_name)))
}

/// Renomme un personnage. Rend « missing », « taken », « online » ou « offline ».
pub async fn rename_character(ctx: &WorldCtx, st: &mut WorldState, old: &str, new: &str) -> anyhow::Result<&'static str> {
    let Some((cid, old_name)) = find_character(ctx, st, old).await? else { return Ok("missing") };
    if ctx.db.characters.first("SELECT id FROM characters WHERE name = ? AND id <> ?", (new.to_string(), cid)).await?.is_some() {
        return Ok("taken");
    }
    if let Err(e) = ctx.db.characters.exec("UPDATE characters SET name = ? WHERE id = ?", (new.to_string(), cid)).await {
        if format!("{e:#}").contains("Duplicate") {
            return Ok("taken");
        }
        return Err(e);
    }
    info!("[mj] {old_name} renommé en {new}");
    if let Some(&sid) = st.by_cid.get(&cid) {
        st.players.get_mut(&sid).unwrap().ch.name = new.to_string();
        send_system(st, sid, &format!("Votre personnage s'appelle désormais {new}."));
        return Ok("online");
    }
    Ok("offline")
}

/// Interdit la discussion pendant `minutes` (0 = lève l'interdiction). Rend le nom exact.
pub async fn chat_ban(ctx: &WorldCtx, st: &mut WorldState, name: &str, minutes: i64) -> anyhow::Result<Option<String>> {
    let Some((cid, ch_name)) = find_character(ctx, st, name).await? else { return Ok(None) };
    if minutes > 0 {
        st.chat_bans.insert(cid, Instant::now() + std::time::Duration::from_secs(minutes as u64 * 60));
    } else {
        st.chat_bans.remove(&cid);
    }
    if let Some(&sid) = st.by_cid.get(&cid) {
        let msg = if minutes > 0 { format!("Un MJ vous interdit de parler pendant {minutes} min.") } else { "Vous pouvez de nouveau parler.".into() };
        send_system(st, sid, &msg);
    }
    info!("[mj] interdiction de discussion {ch_name} : {minutes} min");
    Ok(Some(ch_name))
}

/// Téléporte un joueur en (x, z). Sans y, garde l'altitude actuelle.
pub async fn teleport(ctx: &WorldCtx, st: &mut WorldState, sid: u32, x: f64, y: Option<f64>, z: f64) -> anyhow::Result<Vec3> {
    let Some(p) = st.players.get_mut(&sid) else { anyhow::bail!("joueur absent") };
    let pos = Vec3::new(x as f32, y.unwrap_or(p.ch.pos_y) as f32, z as f32);
    p.ch.set_pos(pos);
    let mv = &schema::OPT_CHAR_MOVE;
    p.send(mv, Fields::new(mv).with(0, p.ch.id).with(1, pos).with(2, pos).with(5, MOVE_STOP_SYNC).with(6, 0));
    let c = &schema::CHARACTER;
    p.send(c, Fields::new(c).with(0, 1).with(1, p.ch.id).with(5, move_mini(MoveMini { pos, dest: None, flags: MOVE_STOP_SYNC })));
    p.dest = None;
    let (id, name) = (p.ch.id, p.ch.name.clone());
    let sent = send_nearby_npcs(ctx, st, sid, pos);
    presence::refresh(st, sid);
    ctx.db.characters.exec("UPDATE characters SET pos_x = ?, pos_y = ?, pos_z = ? WHERE id = ?", (pos.x as f64, pos.y as f64, pos.z as f64, id)).await?;
    info!("[mj] {name} téléporté en {}, {}, {} ({sent} PNJ envoyé(s))", pos.x, pos.y, pos.z);
    Ok(pos)
}

/// Paquet AGPMCHARACTER op 0 d'un autre joueur, pour le rendre visible.
pub fn character_add_packet(p: &Player) -> Vec<u8> {
    character_packet(&p.view(), &p.factors, 0)
}

/// Paquet AGPMCHARACTER op 2 : retire un personnage chez le client.
pub fn character_remove_packet(cid: u32) -> Vec<u8> {
    let c = &schema::CHARACTER;
    build_packet(c, &Fields::new(c).with(0, 2).with(1, cid))
}
