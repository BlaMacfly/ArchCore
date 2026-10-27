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
//!   AGPMSKILL (0x05), AGPMEVENTSKILLMASTER (0x3B) : compétences (skills.rs)
//!   AGPMEVENTBANK (0x36), AGPMCHARACTER op 7 : banque (bank.rs) ; AGPMPRIVATETRADE (0x07) : échange (trade.rs)
//!   AGPMQUEST (0x13), AGPMEVENTQUEST (0x3E) : quêtes (quests.rs)
//! ```
//! L'état du monde est partagé derrière un verrou : les paquets de tous les joueurs sont traités un à
//! un, comme dans la boucle unique du serveur Node.

pub mod addon;
pub mod ai;
pub mod bank;
pub mod chat;
pub mod combat;
pub mod convert;
pub mod effects;
pub mod equipment;
pub mod gm;
pub mod guild;
pub mod items;
pub mod lang;
pub mod loot;
pub mod map;
pub mod monster_skills;
pub mod monsters;
pub mod npc_events;
pub mod npc_trade;
pub mod npcs;
pub mod option_flags;
pub mod options;
pub mod party;
pub mod portal;
pub mod presence;
pub mod pvp;
pub mod quests;
pub mod refinery;
pub mod reconnect;
pub mod recovery;
pub mod regions;
pub mod remission;
pub mod sector;
pub mod skills;
pub mod trade;
pub mod ui_status;
pub mod travel;

use crate::character::{character_packet, factors_mini, make_factors, move_mini, CharView, CharacterRow, Factors, MoveMini, Templates};
use crate::config::Config;
use crate::db::{col, Databases};
use crate::net::{Connection, Service, SessionHandle};
use crate::{debug, info, warn};
use archcore_net::schema::{self, char_mgr_op, chat_type, ext_op, ptype};
use archcore_net::{build_packet, Fields, Packet, Vec3};
use async_trait::async_trait;
use items::{item_status, Inventory, ItemCatalog};
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
    /// Tables de butin.
    pub loot: loot::LootTables,
    /// Régions (sûreté) et points de résurrection.
    pub regions: regions::Regions,
    /// Compétences (SKILL_SPEC, SKILL_CONST).
    pub skills: skills::SkillCatalog,
    /// Quêtes (questtemplate.ini, questgroup.ini).
    pub quests: quests::QuestCatalog,
    /// Langues proposées et textes localisés (tables `language`, `localized_text`).
    pub locale: crate::locale::Catalog,
    /// Portails de téléportation (points, objets, prix des trajets).
    pub portals: portal::Portals,
    /// Prix des pages de banque en plus (indice 0 : deuxième page) ; -1 : pas en vente.
    pub bank_slot_prices: Vec<i64>,
    /// Conversion des objets (ItemConvertTable.txt).
    pub convert: convert::ConvertTable,
    /// Options des objets (itemoptiontable.txt) et tables de nombre d'options et de sockets.
    pub options: options::OptionCatalog,
    /// Recettes de raffinage (refineryrecipetable.txt).
    pub recipes: refinery::Recipes,
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
    /// Objets au sol déjà montrés.
    pub seen_items: HashSet<u32>,
    pub seen_players: HashSet<u32>,
    pub in_game: bool,
    pub dest: Option<Vec3>,
    /// Réception du dernier déplacement, et position autour de laquelle la vue a été calculée.
    pub moved_at: Instant,
    pub view_pos: Vec3,
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
    /// Délais des potions, état de combat, prochaine régénération.
    pub timers: recovery::Timers,
    /// Endroit quitté par un parchemin d'aller-retour.
    pub return_point: Option<Vec3>,
    /// Région sûre de rattachement (résurrection, retour au village).
    pub bound_region: u16,
    /// Limite de débit des messages d'AddOn.
    pub addon_bucket: addon::TokenBucket,
    /// Compétences connues et lancement en cours.
    pub skills: skills::PlayerSkills,
    /// Banque du compte.
    pub bank: bank::Bank,
    /// Échange en cours avec un autre joueur.
    pub trade: Option<trade::Trade>,
    /// Journal des quêtes.
    pub quests: quests::PlayerQuests,
    /// Potions de la barre, seuils de leur prise automatique, casque (AgpmUIStatus).
    pub ui_status: ui_status::UiStatus,
    /// Langue appliquée aux textes envoyés à ce joueur (code de la table `language`, voir `locale.rs`).
    pub language: String,
    /// Demande faite à un portail de téléportation, en attendant d'être à portée.
    pub portal: Option<portal::Pending>,
    /// Point de départ du dernier trajet par portail (« Retour » depuis une zone de combat entre joueurs).
    pub trip_from: Option<String>,
    /// Clé de reconnexion et attente d'une reconnexion (0x63).
    pub link: reconnect::Link,
    /// Dernière recette raffinée avec succès (relancée par /last_refine).
    pub last_recipe: Option<u32>,
    /// Multiplicateur de la vitesse de déplacement réglé par /speed (1 : normal), oublié à la déconnexion.
    pub gm_speed: i32,
    /// Effets de combat de l'équipement porté (options à déclenchement, runes à compétence), recalculés avec
    /// les facteurs.
    pub gear: options::Gear,
    /// Combat entre joueurs : ennemis déclarés (pvp.rs).
    pub pvp: pvp::PvpState,
    /// Pardonneur auprès duquel la fenêtre de rémission est ouverte.
    pub remission_keeper: Option<u32>,
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
    /// Objets posés au sol.
    pub ground: loot::Ground,
    /// Dernier identifiant de compétence attribué.
    pub next_skill_id: u32,
    /// Groupes de joueurs.
    pub parties: party::Parties,
    /// Guildes et leurs membres.
    pub guilds: guild::Guilds,
    /// Sessions écartées par une nouvelle entrée du même compte, en attente de fermeture : leurs paquets
    /// sont ignorés.
    pub evicted: HashSet<u32>,
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
                | ptype::ITEM
                | ptype::EVENT_NPC_DIALOG
                | ptype::EVENT_NPC_TRADE
                | ptype::SKILL
                | ptype::EVENT_SKILL_MASTER
                | ptype::EVENT_BANK
                | ptype::ITEM_CONVERT
                | ptype::EVENT_ITEM_CONVERT
                | ptype::REFINERY
                | ptype::EVENT_REFINERY
                | ptype::EVENT_TELEPORT
                | ptype::GUILD
                | ptype::EVENT_GUILD
                | ptype::GUILD_WAREHOUSE
                | ptype::PRIVATE_TRADE
                | ptype::PARTY
                | ptype::PARTY_MANAGEMENT
                | ptype::QUEST
                | ptype::EVENT_QUEST
                | ptype::UI_STATUS
                | ptype::RECONNECT
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

/// Seuil (ms) au-delà duquel le traitement d'un paquet sous le verrou du monde est noté (`World.SlowPacketMs`,
/// 0 : jamais).
static SLOW_PACKET_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(100);

pub fn set_slow_packet_ms(ms: u64) {
    SLOW_PACKET_MS.store(ms, std::sync::atomic::Ordering::Relaxed);
}

/// Mesure d'un paquet : attente du verrou du monde, puis temps où il est tenu ; notée par `finish` à la fin du
/// traitement si elle dépasse le seuil. Pas de garde à `Drop` : dans le futur d'`on_packet`, il fait planter
/// la compilation release (rustc, 0xc0000005) ; les retours anticipés (erreur, session évincée) ne sont pas mesurés.
struct PacketTimer {
    ptype: u8,
    op: Option<i64>,
    waited: std::time::Duration,
    held_since: Instant,
}

impl PacketTimer {
    fn new(ptype: u8, op: Option<i64>, asked: Instant) -> Self {
        let held_since = Instant::now();
        PacketTimer { ptype, op, waited: held_since - asked, held_since }
    }
}

impl PacketTimer {
    fn finish(self) {
        let limit = SLOW_PACKET_MS.load(std::sync::atomic::Ordering::Relaxed);
        let held = self.held_since.elapsed().as_millis() as u64;
        if limit > 0 && held >= limit {
            warn!("[world] paquet lent 0x{:02x} op {:?} : verrou du monde tenu {held} ms (attente {} ms)", self.ptype, self.op, self.waited.as_millis());
        }
    }
}

#[async_trait]
impl Connection for WorldConn {
    async fn on_packet(&mut self, s: &SessionHandle, p: Packet) -> anyhow::Result<()> {
        let world = self.world.clone();
        let asked = Instant::now();
        let mut st = world.state.lock().await;
        let timer = PacketTimer::new(p.header.ptype, p.int(0), asked);
        if st.evicted.contains(&self.sid) {
            return Ok(());
        }
        // Connexion remplacée par une reconnexion (0x63) : ses derniers paquets sont ignorés.
        if st.players.get(&self.sid).is_some_and(|pl| pl.sess.id() != s.id()) {
            return Ok(());
        }
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
            ptype::CHARACTER if p.int(0) == Some(bank::MOVE_BANK_MONEY) => bank::move_money(ctx, &mut st, self.sid, p.int(9).unwrap_or(0)).await?,
            ptype::CHARACTER if p.int(0) == Some(option_flags::UPDATE_OPTION_FLAG) => option_flags::on_packet(ctx, &mut st, self.sid, &p).await?,
            ptype::CHARACTER => debug!("[world] #{} AGPMCHARACTER op {:?} (ignoré)", s.id(), p.int(0)),
            ptype::CHATTING => chat::on_chatting(&world, &mut st, self.sid, &p).await?,
            ptype::OPT_CHAR_MOVE => presence::on_move(ctx, &mut st, self.sid, &p),
            ptype::OPT_CHAR_ACTION => combat::on_action(ctx, &mut st, self.sid, &p),
            ptype::REMISSION => remission::on_packet(ctx, &mut st, self.sid, &p).await?,
            ptype::ITEM => loot::on_item_packet(ctx, &mut st, self.sid, &p).await?,
            ptype::EVENT_NPC_DIALOG => npc_events::on_dialog(ctx, &st, self.sid, &p),
            ptype::EVENT_NPC_TRADE => npc_trade::on_packet(ctx, &mut st, self.sid, &p).await?,
            ptype::ARCHCORE_EXT => match p.int(0) {
                Some(ext_op::SET_LANGUAGE | ext_op::TEXT_REQUEST) => lang::on_packet(ctx, &mut st, self.sid, &p).await?,
                _ => addon::on_packet(ctx, &mut st, self.sid, &p),
            },
            ptype::SKILL => skills::on_packet(ctx, &mut st, self.sid, &p),
            ptype::EVENT_SKILL_MASTER => skills::on_master_packet(ctx, &mut st, self.sid, &p).await?,
            ptype::EVENT_BANK => bank::on_packet(ctx, &mut st, self.sid, &p),
            ptype::ITEM_CONVERT => convert::on_packet(ctx, &mut st, self.sid, &p).await?,
            ptype::EVENT_ITEM_CONVERT => convert::on_event(ctx, &st, self.sid, &p),
            ptype::REFINERY => refinery::on_packet(ctx, &mut st, self.sid, &p).await?,
            ptype::EVENT_REFINERY => refinery::on_event(&st, self.sid, &p),
            ptype::EVENT_TELEPORT => portal::on_packet(ctx, &mut st, self.sid, &p).await?,
            ptype::GUILD => guild::on_packet(ctx, &mut st, self.sid, &p).await?,
            ptype::EVENT_GUILD => guild::on_event(ctx, &mut st, self.sid, &p),
            ptype::GUILD_WAREHOUSE => guild::warehouse::on_packet(ctx, &mut st, self.sid, &p).await?,
            ptype::PRIVATE_TRADE => trade::on_packet(ctx, &mut st, self.sid, &p).await?,
            ptype::PARTY => party::on_party(&mut st, self.sid, &p),
            ptype::PARTY_MANAGEMENT => party::on_management(&mut st, self.sid, &p),
            ptype::QUEST => quests::on_packet(ctx, &mut st, self.sid, &p).await?,
            ptype::EVENT_QUEST => quests::on_event(ctx, &mut st, self.sid, &p),
            ptype::UI_STATUS => ui_status::on_packet(ctx, &mut st, self.sid, &p).await?,
            ptype::RECONNECT => {
                if let Some(sid) = reconnect::on_packet(ctx, &mut st, s, &p).await? {
                    self.sid = sid;
                }
            }
            _ => {}
        }
        timer.finish();
        Ok(())
    }

    async fn on_close(&mut self, s: &SessionHandle) {
        let world = self.world.clone();
        let mut st = world.state.lock().await;
        st.evicted.remove(&self.sid);
        // Personnage repris par une autre connexion, ou gardé en attendant une reconnexion.
        if st.players.get(&self.sid).is_some_and(|p| p.sess.id() != s.id()) || reconnect::detach(&mut st, self.sid, Instant::now()) {
            return;
        }
        remove_player(&world.ctx, &mut st, self.sid).await;
    }
}

/// Retire un joueur du monde : échange annulé, groupe quitté, position enregistrée.
pub(super) async fn remove_player(ctx: &WorldCtx, st: &mut WorldState, sid: u32) {
    // Un échange en cours est annulé : les objets n'ont pas quitté les inventaires.
    trade::cancel(st, sid);
    // Le groupe perd ce membre (et se dissout s'il ne reste qu'un joueur).
    if let Some(cid) = st.players.get(&sid).map(|p| p.ch.id) {
        party::on_leave_world(st, cid);
        // Les membres de sa guilde le voient hors ligne.
        guild::on_leave_world(st, cid);
    }
    guild::warehouse::on_leave_world(st, sid);
    if let Err(e) = presence::leave(ctx, st, sid).await {
        warn!("[world] départ de #{sid} : {e:#}");
    }
    if let Some(p) = st.players.remove(&sid) {
        if st.by_cid.get(&p.ch.id) == Some(&sid) {
            st.by_cid.remove(&p.ch.id);
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
    // La clé d'entrée ne sert qu'une fois.
    ctx.db.auth.exec("UPDATE account SET auth_token = NULL WHERE id = ? AND auth_token = ?", (account.id, auth_key)).await?;
    // Une seule session en jeu par compte : la précédente quitte le monde (et sa position est enregistrée)
    // avant la lecture de la base. Sinon deux copies de la banque du compte, ou du même personnage,
    // s'écraseraient l'une l'autre.
    let previous: Vec<u32> = st.players.iter().filter(|(sid, p)| **sid != s.id() && p.account.id == account.id).map(|(sid, _)| *sid).collect();
    for old in previous {
        warn!("[world] #{} {} entre de nouveau : la session #{old} est fermée", s.id(), account.username);
        let sess = st.players[&old].sess.clone();
        remove_player(ctx, st, old).await;
        st.evicted.insert(old);
        sess.close();
    }
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
    let mut ch = CharacterRow::from_row(&row);
    let Some(tpl) = ctx.templates.get(ch.template_id) else {
        crate::error!("[world] template {} inconnu", ch.template_id);
        s.close();
        return Ok(());
    };
    let gm_level: i64 = col(&acc, "gm_level");
    let inventory = Inventory::load(&ctx.db, ch.id).await?;
    let bank = bank::Bank::load(&ctx.db, account.id).await?;
    let quests = quests::PlayerQuests::load(&ctx.db, ch.id).await?;
    let ui_status = ui_status::UiStatus::load(&ctx.db, ch.id).await?;
    // Banque du compte : son argent et ses pages remplacent ceux de la ligne du personnage.
    ch.bank_money = bank.money;
    ch.bank_size = bank.extra_slots as i32;
    let mut factors = equipment::with_equipment(make_factors(tpl, Some(&ch)), &equipment::worn(&inventory, &ctx.catalog, &ctx.convert));
    ctx.options.apply(&mut factors, &equipment::worn_options(&inventory, &ctx.catalog, &ctx.options, ch.level as i64));
    let gear = equipment::worn_gear(ctx, &inventory, ch.level as i64);
    let mut skills = skills::load(&ctx.db, &ctx.skills, ch.id, &mut st.next_skill_id).await?;
    skills::refresh_points(&ctx.skills, &skills, &mut factors);
    skills::sync_passives(&ctx.skills, &mut skills, ch.template_id);
    effects::apply(&mut factors, &effects::total(&skills.effects));
    // PV et PM enregistrés au-delà du maximum (maximum changé depuis) : ramenés au maximum.
    factors.point[crate::character::POINT_HP] = factors.point[crate::character::POINT_HP].min(factors.point_max[0]);
    factors.point[1] = factors.point[1].min(factors.point_max[1]);
    // Rattachement : la région où il se trouve si elle est sûre, sinon celle de sa race.
    let bound_region = ctx.map.region(ch.pos_x, ch.pos_z).filter(|r| ctx.regions.is_safe(*r)).unwrap_or_else(|| regions::default_region(factors.ftype[0]));
    // Langue : souhait du client ArchRust (HELLO), sinon dernier choix du compte, sinon World.Language.
    let wish = match s.language() {
        Some(l) => Some(l),
        None => crate::locale::account_language(&ctx.db, account.id).await,
    };
    let language = ctx.locale.pick(wish.as_deref());
    info!(
        "[world] #{} {} entre avec {} (cid {}{}, langue {language})",
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
    // AGPMTIMER champ 1 : le client en tire l'heure du ciel par (ms / 3 600 000) % 24 sans correction
    // de fuseau, d'où l'heure locale par défaut (config world.timeZone).
    let now_ms = ctx.config.world.time_zone.game_time_ms();
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
            moved_at: Instant::now(),
            view_pos: Vec3::default(),
            seen_items: HashSet::new(),
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
            timers: recovery::Timers::default(),
            return_point: None,
            bound_region,
            addon_bucket: addon::TokenBucket::default(),
            skills,
            bank,
            trade: None,
            quests,
            language,
            portal: None,
            trip_from: None,
            link: reconnect::Link::default(),
            last_recipe: None,
            gm_speed: 1,
            ui_status,
            gear,
            pvp: pvp::PvpState::default(),
            remission_keeper: None,
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
    // Client ArchRust en « automatique » (HELLO sans langue) : le HELLO_ACK annonçait la langue par défaut ;
    // la langue du compte s'applique finalement, on la lui dit (LANGUAGE_ACK, demande 0). Envoyé ici, juste
    // après le paquet personnage : avant, le client n'attend que des paquets précis (CONFIG, personnage…).
    if p.sess.has_capability(crate::ext::LANGUAGE) && p.sess.language().is_none() {
        p.send(&schema::ARCHCORE_EXT, crate::ext::language_ack(0, &p.language, &ctx.locale.summary.available_list()));
    }
    // Niveau MJ : le client l'accroche à un personnage déjà connu (CID), d'où l'envoi après le paquet personnage.
    if p.gm_level > 0 {
        p.send_raw(chat::admin_level_packet(p.ch.id, &p.ch.name, p.gm_level, &p.account.username));
    }
    // Quêtes : drapeaux des quêtes terminées, puis le journal.
    for pkt in quests::login_packets(p.ch.id, &p.quests) {
        p.send_raw(pkt);
    }
    // Inventaire : paquets AGPMITEM complets mis bout à bout dans le champ 6 (« tous les objets ») de la
    // vue optimisée op 2 ; le client rattache chaque objet au personnage déjà reçu (champ 4).
    let all_items: Vec<u8> = p.inventory.add_packets().into_iter().chain(p.bank.add_packets(p.ch.id)).collect::<Vec<_>>().concat();
    info!("[world] #{sid} {} objet(s) d'inventaire et {} compétence(s) envoyé(s)", p.inventory.items.len(), p.skills.list.len());
    let ov = &schema::OPT_VIEW;
    let empty = Vec::<u8>::new();
    p.send(ov, Fields::new(ov).with(0, 2).with(6, all_items).with(7, empty.clone()).with(8, skills::all_packets(p)));
    p.send(ov, Fields::new(ov).with(0, 2).with(6, empty.clone()).with(7, empty));
    let mv = &schema::OPT_CHAR_MOVE;
    p.send(mv, Fields::new(mv).with(0, p.ch.id).with(1, pos).with(2, pos).with(5, MOVE_STOP_SYNC).with(6, 0));
    let sent = send_nearby_npcs(ctx, st, sid, pos);
    info!("[world] #{sid} {sent} PNJ et {} monstre(s) envoyés (secteur {:?} et voisins)", st.players[&sid].seen_monsters.len(), sector::of(pos));
    let p = &st.players[&sid];
    let cm = &schema::CHAR_MANAGER;
    p.send(cm, Fields::new(cm).with(0, char_mgr_op::FINISH_ENTER).with(4, p.ch.id));
    presence::enter(ctx, st, sid);
    // Guilde : la guilde, ses membres et le lien du joueur ; les membres le voient en ligne.
    guild::on_enter(st, sid);
    // Potions de la barre et seuils de leur prise automatique.
    ui_status::on_enter(st, sid);
    // Victoires et défaites entre joueurs, mode de la zone (AgpmPvP op 0).
    if let Some(p) = st.players.get(&sid) {
        p.send_raw(pvp::info_packet(p.ch.id, p.ch.pvp_win, p.ch.pvp_lose, pvp::zone_mode(ctx, st, sid)));
    }
    let p = &st.players[&sid];
    let c = &schema::CHARACTER;
    p.send(c, Fields::new(c).with(0, 1).with(1, p.ch.id).with(4, 2).with(5, move_mini(MoveMini { pos, dest: None, flags: 0 })));
    p.send(c, Fields::new(c).with(0, 1).with(1, p.ch.id).with(4, 2).with(7, factors_mini(&p.factors)));
    let region = ctx.map.region(pos.x as f64, pos.z as f64).map(|r| r.to_string()).unwrap_or_else(|| "inconnue".into());
    info!("[world] #{sid} {} est en jeu (région {region})", p.ch.name);
    reconnect::on_enter(st, sid);
}

/// Vue d'un joueur placé en `pos` (secteurs voisins, voir `sector.rs`) : PNJ entrés (vue optimisée op 1,
/// un paquet par PNJ) ou sortis (op 2), puis monstres (`sync_monsters`) et objets au sol. Rend le nombre
/// de PNJ envoyés.
pub fn send_nearby_npcs(ctx: &WorldCtx, st: &mut WorldState, sid: u32, pos: Vec3) -> usize {
    let Some(p) = st.players.get_mut(&sid) else { return 0 };
    p.view_pos = pos;
    let mut sent = 0;
    let gone: Vec<u32> = p.seen_npcs.iter().copied().filter(|cid| ctx.npcs.get(*cid).is_none_or(|n| !sector::keeps(pos, n.pos))).collect();
    for cid in gone {
        p.seen_npcs.remove(&cid);
        p.sess.send_raw(character_remove_packet(cid));
    }
    for npc in ctx.npcs.near(pos, sector::REACH).filter(|n| sector::sees(pos, n.pos)) {
        if p.seen_npcs.insert(npc.cid) {
            // Nom dans la langue du joueur (texte localisé, sinon titre traduit, sinon nom du serveur).
            let name = ctx.locale.resolve(crate::locale::kind::NPC_NAME, npc.cid.wrapping_sub(npcs::NPC_CID_BASE), &p.language, &npc.name);
            p.sess.send_raw(NpcRegistry::view_packet_named(npc, &name));
            sent += 1;
        }
    }
    sync_monsters(ctx, st, sid, pos);
    loot::sync_ground(ctx, st, sid, pos);
    sent
}

/// Montre les monstres entrés dans les secteurs vus depuis `pos` et retire ceux qui en sont sortis. Rend
/// (ajoutés, retirés).
pub fn sync_monsters(ctx: &WorldCtx, st: &mut WorldState, sid: u32, pos: Vec3) -> (usize, usize) {
    let Some(p) = st.players.get_mut(&sid) else { return (0, 0) };
    let mut added = 0;
    for m in st.monsters.near(pos, sector::REACH).filter(|m| m.is_visible() && sector::sees(pos, m.pos)) {
        if p.seen_monsters.insert(m.cid) {
            let name = ctx.locale.resolve(crate::locale::kind::MONSTER_NAME, m.tid, &p.language, &m.name);
            p.sess.send_raw(st.monsters.view_packet_named(m, &name));
            added += 1;
        }
    }
    let gone: Vec<u32> = p.seen_monsters.iter().copied().filter(|cid| st.monsters.get(*cid).is_none_or(|m| !m.is_visible() || !sector::keeps(pos, m.pos))).collect();
    for cid in &gone {
        p.seen_monsters.remove(cid);
        p.sess.send_raw(character_remove_packet(*cid));
        if let Some(m) = st.monsters.get(*cid) {
            debug!(
                "[vue] #{sid} retire {} {cid:#x} en {:.0}, {:.0} (secteur {:?}, {}) ; vue depuis {:.0}, {:.0} (secteur {:?})",
                m.name,
                m.pos.x,
                m.pos.z,
                sector::of(m.pos),
                if m.is_visible() { "visible" } else { "disparu" },
                pos.x,
                pos.z,
                sector::of(pos)
            );
        }
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

/// Enregistre l'argent de l'inventaire d'un personnage : (argent, CID).
pub const SAVE_MONEY: &str = "UPDATE characters SET money = ? WHERE id = ?";

/// Argent de l'inventaire borné à ce que le client accepte.
pub fn clamp_money(money: i64) -> u64 {
    money.clamp(0, npc_trade::MAX_MONEY) as u64
}

/// Argent de l'inventaire d'un joueur en jeu : enregistré puis envoyé au client (AGPMCHARACTER champ 8).
pub async fn set_money(ctx: &WorldCtx, st: &mut WorldState, sid: u32, money: i64) -> anyhow::Result<()> {
    let Some(p) = st.players.get_mut(&sid) else { return Ok(()) };
    let money = clamp_money(money);
    ctx.db.characters.exec(SAVE_MONEY, (money, p.ch.id)).await?;
    show_money(p, money);
    Ok(())
}

/// Argent déjà enregistré : reporté en mémoire et envoyé au client.
pub fn show_money(p: &mut Player, money: u64) {
    p.ch.money = money;
    let c = &schema::CHARACTER;
    p.send(c, Fields::new(c).with(0, 1).with(1, p.ch.id).with(8, p.ch.money));
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
    // Catalogue des objets (ItemDataTable), sinon modèles du client (itemtemplateentry.ini).
    let item_name: String = match ctx.catalog.get(tid) {
        Some(d) => d.name.clone(),
        None => match ctx.db.world.first("SELECT tid, name FROM item_template WHERE tid = ?", (tid,)).await? {
            Some(item) => col(&item, "name"),
            None => return Ok(Err("item")),
        },
    };
    let note = if bound { " (non échangeable : non enregistré)" } else { "" };
    if let Some(&sid) = st.by_cid.get(&cid) {
        let stack = ctx.catalog.get(tid).map(|d| d.stack).unwrap_or(1);
        let p = st.players.get_mut(&sid).unwrap();
        let Some((it, created)) = p.inventory.add(&ctx.db, tid, count, stack, None).await? else { return Ok(Err("full")) };
        if created {
            // Comme un objet lâché par une source du niveau de l'objet (1 au moins).
            let level = ctx.catalog.get(tid).and_then(|d| d.equip.as_ref()).map(|e| e.level.max(1)).unwrap_or(1);
            let (options, sockets) = options::generate(&ctx.catalog, &ctx.options, tid, level, &mut rand::thread_rng());
            p.inventory.set_generated(&ctx.db, it.iid, &options, sockets).await?;
        }
        let it = p.inventory.get(it.iid).cloned().unwrap_or(it);
        p.send_raw(items::item_add_of(&it, cid, item_status::INVENTORY, Some(it.grid)));
        if let Some(c) = items::convert_add(&it, cid) {
            p.send_raw(c);
        }
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
    // Sans altitude donnée : la hauteur du sol (map_height), sinon l'altitude actuelle.
    let y = y.or_else(|| ctx.map.height(x, z).map(f64::from)).unwrap_or(p.ch.pos_y);
    let pos = Vec3::new(x as f32, y as f32, z as f32);
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

/// Paquet AGPMCHARACTER op 3 (retrait de la vue) : le personnage sort de la vue du client. Contrairement à
/// l'op 2, qui le supprime aussitôt, le client consulte d'abord ses propres fonctions de vue, qui peuvent
/// différer la suppression (un monstre encore à l'écran reste affiché).
pub fn character_remove_packet(cid: u32) -> Vec<u8> {
    let c = &schema::CHARACTER;
    build_packet(c, &Fields::new(c).with(0, 3).with(1, cid))
}
