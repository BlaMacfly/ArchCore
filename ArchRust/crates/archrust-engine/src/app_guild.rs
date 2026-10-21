//! Guildes et discussion de guilde et de groupe côté moteur. Protocole : `archrust_netclient::guild` ;
//! fenêtres : `hud_guild.rs`.
//!
//! Comportement relevé dans Archonia (AgcmUIGuild, sha256 e9c7609b… ; fonctions d'interface
//! enregistrées par FUN_1406a13a0, événements par FUN_1406a09e0) :
//! * gestionnaire de guilde (PNJ à fonction 23, bouton `sysstr` 328 « Guilde » du dialogue,
//!   FUN_1406ca6a0) : AGPMEVENTGUILD op 0, l'accord (op 1, FUN_1406a3890) ouvre `Guild_Main` ;
//!   texte `Guild_Main_Text` (niveau 20, prix de la ligne 50 de `guildmaxmember.txt`, FUN_1408c6ad0) ;
//!   « Créer » actif sans guilde et avec l'or ; « Dissoudre » actif pour un chef seul dans sa guilde ;
//! * création (FUN_1408c67f0, FUN_1408c6e50, FUN_1408c7330, FUN_1408c7290) : déjà membre → message 5,
//!   niveau < 20 → `Guild_Msg_NeedMoreLevel` ; nom 1 à 32 caractères, mot de passe 1 à 12, confirmé
//!   (`Guild_Msg_CheckPassword`) ; `Guild_Create_Request` (`Guild_Guild_Info`, `Guild_Create_Confirm_Text`)
//!   puis op 0 ; la guilde reçue avec le joueur pour chef ouvre `Guild_Create_Complete` (FUN_1406a36b0) ;
//! * dissolution (FUN_1408c6920, FUN_1408c70c0, FUN_1408c7670, FUN_1408c75a0) : `Guild_Destroy`, puis
//!   `Guild_Destroy_Confirm`, puis op 6 avec le mot de passe ; `Guild_Destroy_Complete` à la fin ;
//! * touche G (`Hotkey=0:34`, « Guild ») ou bouton du menu : `Guild_Notice_Open_Request`
//!   (FUN_1408c7f10) ouvre `Guild_Notice` ou ferme les fenêtres ouvertes ; `Guild_NoticeUI_Init`
//!   (FUN_1408c81d0) : annonce (89 caractères au plus, modifiable par le chef), `Guild_Notice_Info`
//!   (guilde, chef, membres / taille, en ligne), `Guild_Notice_Detail_NoGuildMember` sans guilde ;
//!   « Envoyer un avis » : op 9 par le chef (FUN_1408c7fe0) ; « Quitter la guilde » (FUN_1408c7d00) :
//!   le chef reçoit `Guild_Leave_Error_Master`, un membre confirme puis op 4 ;
//! * liste des membres (FUN_1408c14c0, FUN_1408c10f0) : 19 par page ; « Expulser » (chef seulement,
//!   FUN_1408c0c20) confirme puis op 5 ; « Inviter » et « Chuchotement » pour un membre en ligne ;
//!   « Approbation » (`Guild_Join_Allow_Button_Text`, `Guild_Leave_Allow_Button_Text` pour une demande
//!   de départ, FUN_1408bf620 / FUN_1408bf480) confirme puis op 3 ou op 29 ; détail d'un membre
//!   `Guild_GuildMemberInfo` ou `Guild_GuildLeadMemberInfo` (FUN_1408bfa80) ;
//! * invitation reçue (FUN_1406a3d10) : boîte OK / Annuler `Guild_Invite_ConfirmUI_Open` ; OK = op 3,
//!   Annuler = op 2 (FUN_1406a39f0) ;
//! * messages système (op 10) : code → clé de `uimessage.txt` (FUN_1406a44e0), couleur
//!   `Guild_System_Message_Color` pour 47, 48 et 50 ;
//! * discussion : type 3 « [Guilde][Nom] texte » en `GUILD_CHAT`, type 4 « [Groupe][Nom] texte » en
//!   `PARTY_CHAT` (FUN_1407a2b30, couleurs de `ini/color.ini`) ; envoi par `/G texte` et `/P texte`
//!   (modes `sysstr` 818 « Guilde (/G) » et 817 « Groupe (/P) »), `/N texte` pour la discussion normale.
//!
//! Non repris (ArchCore ne les gère pas ou ressource absente) : emblèmes, agrandissement, changement de
//! nom, classement, missions, second et passation (boutons inactifs) ; position exacte de la boîte
//! OK / Annuler ; nom de guilde au-dessus des têtes (affichage d'Archonia non établi) ; durée restante
//! d'une demande de départ (ArchCore n'envoie pas sa date).

use super::App;
use crate::assets::Assets;
use crate::hud::{GuildEdit, GuildRow, GuildUi, GuildView, HudState, PageColors};
use crate::net::NetLink;
use archrust_netclient::guild::{self, GuildCommand, GuildEvent, GuildMember, GuildNotice, GuildState};
use archrust_netclient::party::PartyCommand;
use archrust_netclient::Command;
use std::collections::{HashMap, HashSet};

/// Membres par page de la liste (`Guild_Member_Index`, 0x13).
const PAGE: usize = 19;
/// Niveau requis pour créer une guilde (FUN_1408c67f0).
const CREATE_LEVEL: i64 = 20;
/// Taille de la guilde créée : ligne de `guildmaxmember.txt` dont le prix est demandé (FUN_1408c6ad0).
const CREATE_SIZE: i64 = 50;
/// Masques des onglets de discussion (FUN_140511e20) : guilde, groupe, chuchotement.
pub(super) const MASK_GUILD: u16 = 0x2;
pub(super) const MASK_PARTY: u16 = 0x4;
pub(super) const MASK_WHISPER: u16 = 0x8;

/// Textes du client utiles aux guildes.
#[derive(Default)]
struct GuildData {
    messages: HashMap<String, String>,
    events: HashMap<String, String>,
    /// Prix de création (`guildmaxmember.txt`, ligne de 50 places), 100 000 sans la table.
    create_price: i64,
}

impl GuildData {
    fn msg(&self, key: &str) -> String {
        self.messages.get(key).cloned().unwrap_or_default()
    }
    fn color(&self, key: &str, default: u32) -> u32 {
        self.messages.get(key).and_then(|v| v.trim().parse::<u64>().ok()).map_or(default, |v| v as u32)
    }
}

fn load_data(assets: &Assets) -> GuildData {
    let messages: HashMap<String, String> = super::npc_glue::client_text(assets, "uimessage.txt")
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    let mut create_price = 100_000;
    for line in super::npc_glue::client_text_in(assets, "ini", "guildmaxmember.txt").lines() {
        let mut cols = line.split('\t').map(str::trim);
        if let (Some(Ok(size)), Some(Ok(price))) = (cols.next().map(str::parse::<i64>), cols.next().map(str::parse::<i64>)) {
            if size == CREATE_SIZE {
                create_price = price;
            }
        }
    }
    let events = super::portal_glue::ui_events(assets);
    GuildData { messages, events, create_price }
}

/// Une fenêtre de guilde ouverte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Win {
    Main,
    Create,
    Destroy,
    CreateRequest,
    CreateComplete,
    DestroyConfirm,
    DestroyComplete,
    Notice,
    Member,
}

impl Win {
    fn name(self) -> &'static str {
        match self {
            Win::Main => "Guild_Main",
            Win::Create => "Guild_Create",
            Win::Destroy => "Guild_Destroy",
            Win::CreateRequest => "Guild_Create_Request",
            Win::CreateComplete => "Guild_Create_Complete",
            Win::DestroyConfirm => "Guild_Destroy_Confirm",
            Win::DestroyComplete => "Guild_Destroy_Complete",
            Win::Notice => "Guild_Notice",
            Win::Member => "Guild_Member",
        }
    }
}

/// Question posée dans la boîte OK / Annuler.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Ask {
    Invite { guild: String },
    Leave,
    Kick(String),
    AllowJoin(String),
    AllowLeave(String),
}

#[derive(Default)]
pub struct GuildGlue {
    pub state: GuildState,
    data: Option<GuildData>,
    open: HashSet<Win>,
    // Saisies.
    name: String,
    password: String,
    password2: String,
    destroy_password: String,
    search: String,
    notice: String,
    /// Création demandée (nom, mot de passe) ; nom de la guilde dissoute pour `Guild_Destroy_Complete`.
    pending_create: Option<(String, String)>,
    pending_destroy: Option<String>,
    complete_name: String,
    selected: Option<usize>,
    page: usize,
    ask: Option<(Ask, String)>,
    /// Clics de la dernière image, traités par `guild_frame`.
    clicks: Vec<GuildUi>,
}

impl GuildGlue {
    fn data(&mut self, assets: &Assets) -> &GuildData {
        self.data.get_or_insert_with(|| load_data(assets))
    }
}

fn send(net: Option<&NetLink>, cmd: GuildCommand) {
    if let Some(net) = net {
        net.send(Command::Guild(cmd));
    }
}

/// Nom affiché d'un membre : la suite d'un `@` devient « ... » (FUN_1408bfe60).
fn shown_name(name: &str) -> String {
    match name.split_once('@') {
        Some((head, _)) => format!("{head}..."),
        None => name.to_string(),
    }
}

/// Remplace les `{{…}}` d'un message d'événement par `value`.
fn fill_braces(text: &str, value: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find("{{") {
        out.push_str(&rest[..i]);
        match rest[i..].find("}}") {
            Some(j) => {
                out.push_str(value);
                rest = &rest[i + j + 2..];
            }
            None => {
                rest = &rest[i..];
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Remplace dans l'ordre les `%s` et `%d` d'un texte d'Archonia.
fn printf(fmt: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut out = String::new();
    let mut it = args.iter();
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' && matches!(chars.peek(), Some('s' | 'd')) {
            chars.next();
            if let Some(a) = it.next() {
                out.push_str(&a.to_string());
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Message d'un code de guilde (op 10) : texte et couleur ARGB (FUN_1406a44e0) ; `None` = rien.
fn system_text(d: &GuildData, code: i32, text: &str, number: i64) -> Option<(String, u32)> {
    let white = 0xFFFF_FFFF;
    let guild_color = d.color("Guild_System_Message_Color", 0xFFFF_FF33);
    let key = match code {
        0 => "Guild_Msg_ExistGuildID",
        1 => "Guild_Msg_NeedMoreLevel",
        2 => return Some((printf(&d.msg("Guild_Msg_NeedMoreMoney"), &[&number]), white)),
        4 => "Guild_Msg_UseSpecialChar",
        5 => "Guild_Msg_AlreadyMember",
        6 => "Guild_Msg_MaxMember",
        7 => "Guild_Msg_JoinFail",
        10 => "Guild_Msg_InvalidPassword",
        11 => "Guild_Msg_CreateComplete",
        12 => "Guild_Msg_Destroy",
        13 => return Some((printf(&d.msg("Guild_Msg_Join"), &[&text]), white)),
        14 => "Guild_Msg_JoinReject",
        15 => return Some((printf(&d.msg("Guild_Msg_Leave"), &[&text]), white)),
        16 => return Some((printf(&d.msg("Guild_Msg_ForcedLeave"), &[&text]), white)),
        17 => "Guild_Msg_AlreadyMember2",
        18 => "Guild_Msg_ForcedLeave2",
        47 => return Some((d.msg("Guild_No_Exist_Search_Guild"), guild_color)),
        48 => return Some((printf(&d.msg("Guild_Join_Request_Self"), &[&text]), guild_color)),
        50 => return Some((printf(&d.msg("Guild_Leave_Request_Self"), &[&text]), guild_color)),
        _ => return None,
    };
    Some((d.msg(key), white))
}

/// Nom de race et de classe d'un modèle (clés de `uimessage.txt`).
fn race_class(assets: &Assets, d: &GuildData, tid: i32) -> (String, String) {
    let (race, class) = crate::login_settings::template_identity(assets, tid);
    let tr = |k: &str| d.messages.get(k).cloned().filter(|v| !v.is_empty()).unwrap_or_else(|| k.to_string());
    (tr(race), if class.is_empty() { String::new() } else { tr(&class) })
}

/// Ligne colorée pour la discussion (codes `|cffRRGGBB…|r`).
fn colored(text: &str, argb: u32) -> String {
    format!("|cff{:06x}{text}|r", argb & 0xFF_FFFF)
}

/// Fenêtres et boîte OK / Annuler (dans la passe egui) ; les clics sont traités par `guild_frame`.
pub fn panel(ctx: &egui::Context, glue: &mut GuildGlue, hud: &mut HudState, assets: &Assets, me: &str) {
    glue.data(assets);
    let money = hud.stats.money.unwrap_or(0);
    let mine = glue.state.member(me).cloned();
    let master = mine.as_ref().is_some_and(|m| m.rank == guild::rank::MASTER);
    let leads = mine.as_ref().is_some_and(GuildMember::leads);
    let info = glue.state.info.clone();
    let guild_name = info.as_ref().map(|i| i.name.clone()).unwrap_or_default();
    let rows = glue.state.members.clone();
    let selected = glue.selected.and_then(|i| rows.get(glue.page * PAGE + i).cloned());
    let (online, total) = glue.state.counts();
    let d = glue.data.take().expect("données lues");
    let colors = PageColors { selected: d.color("Guild_SelectPageColor", 0xFFFF_F0C8), other: d.color("Guild_DeselectPageColor", 0xFF80_8080) };
    let mut out = Vec::new();
    let open: Vec<Win> = [Win::Main, Win::Create, Win::Destroy, Win::Notice, Win::Member, Win::CreateRequest, Win::CreateComplete, Win::DestroyConfirm, Win::DestroyComplete]
        .into_iter()
        .filter(|w| glue.open.contains(w))
        .collect();
    for win in open {
        let mut v = GuildView::default();
        match win {
            Win::Main => {
                v.texts.insert("GD_Main_Edit_Info", printf(&d.msg("Guild_Main_Text"), &[&CREATE_LEVEL, &d.create_price]));
                if info.is_some() || money < d.create_price {
                    v.disabled.insert("GD_Main_Button_1");
                }
                if !(master && rows.len() == 1) {
                    v.disabled.insert("GD_Main_Button_2");
                }
                // Emblème, agrandissement, changement de nom : non gérés par ArchCore.
                v.disabled.extend(["GD_Main_Button_3", "CONTROL_29434", "CONTROL_10138"]);
            }
            Win::Create => {
                v.texts.insert("Guild_Create_Edit_Info", d.msg("Guild_Create_Text"));
                v.edits.push(GuildEdit { control: "GD_Create_Edit_Name", text: &mut glue.name, password: false, max: 12, multiline: false });
                v.edits.push(GuildEdit { control: "GD_Create_Edit_PW", text: &mut glue.password, password: true, max: 12, multiline: false });
                v.edits.push(GuildEdit { control: "GD_Create_Edit_PW2", text: &mut glue.password2, password: true, max: 12, multiline: false });
            }
            Win::Destroy => {
                v.texts.insert("GD_Destroy_Edit_Info", d.msg("Guild_Destroy_Text"));
                v.texts.insert("GD_Destroy_Edit_GuildName", guild_name.clone());
                v.edits.push(GuildEdit { control: "GD_Destroy_Edit_PW", text: &mut glue.destroy_password, password: true, max: 12, multiline: false });
            }
            Win::CreateRequest => {
                let name = glue.pending_create.as_ref().map(|p| p.0.clone()).unwrap_or_default();
                v.texts.insert("GD_Create_Request_Edit", printf(&d.msg("Guild_Guild_Info"), &[&name, &me]));
                v.texts.insert("GD_Create_Request_Edit2", d.msg("Guild_Create_Confirm_Text"));
            }
            Win::CreateComplete => {
                let n = &glue.complete_name;
                v.texts.insert("GD_Create_Complete_Edit", printf(&d.msg("Guild_Create_Complete_Text"), &[n, n, &me]));
            }
            Win::DestroyConfirm => {
                v.texts.insert("GD_Destroy_Confirm_Edit", printf(&d.msg("Guild_Guild_Info"), &[&guild_name, &me]));
                v.texts.insert("GD_Destroy_Confirm_Edit2", d.msg("Guild_Destroy_Confirm_Text"));
            }
            Win::DestroyComplete => {
                v.texts.insert("GD_Destroy_Complete_Edit", printf(&d.msg("Guild_Destroy_Complete_Text"), &[&glue.complete_name, &me]));
            }
            Win::Notice => {
                v.active.insert("GD_Notice_Button_Tab1");
                v.disabled.extend(["GD_Notice_Button_Tab3", "GD_Notice_Button_GvGTournament"]);
                if info.is_none() {
                    v.disabled.insert("GD_Notice_Button_Tab2");
                }
                match &info {
                    Some(i) => {
                        v.texts.insert("GD_Notice_Edit_Guild", String::new());
                        v.texts.insert("GD_Notice_Edit_Member", printf(&d.msg("Guild_Notice_Info"), &[&i.name, &i.master, &total, &i.max_members, &online]));
                        if master {
                            v.edits.push(GuildEdit { control: "GD_Notice_Notice", text: &mut glue.notice, password: false, max: 84, multiline: true });
                        } else {
                            let n = if i.notice.chars().count() > 89 { String::new() } else { i.notice.clone() };
                            v.texts.insert("GD_Notice_Notice", n);
                        }
                    }
                    None => {
                        v.texts.insert("GD_Notice_Edit_Guild", d.msg("Guild_Notice_Detail_NoGuildMember"));
                        v.texts.insert("GD_Notice_Edit_Member", String::new());
                        v.texts.insert("GD_Notice_Notice", String::new());
                    }
                }
                if !master {
                    v.disabled.insert("GD_Notice_Button_Send");
                }
                if mine.as_ref().is_none_or(|m| m.rank == guild::rank::LEAVE_REQUEST) {
                    v.disabled.insert("CONTROL_18715");
                }
                v.texts.insert("GD_Notice_Edit_Member_UD", String::new());
            }
            Win::Member => {
                v.active.insert("GD_Member_Button_Tab2");
                v.disabled.extend(["GD_Member_Button_Tab3", "GD_Member_Button_GvGTournament", "CONTROL_31321", "CONTROL_3394"]);
                let pages = rows.len().div_ceil(PAGE).max(1);
                v.pages = pages;
                v.page = glue.page.min(pages - 1);
                for m in rows.iter().skip(v.page * PAGE).take(PAGE) {
                    let (race, class) = race_class(assets, &d, m.tid);
                    let level = match m.rank {
                        guild::rank::JOIN_REQUEST => d.msg("Guild_Member_Join_Request"),
                        guild::rank::LEAVE_REQUEST => d.msg("Guild_Member_Leave_Request"),
                        _ => m.level.to_string(),
                    };
                    v.rows.push(GuildRow { name: shown_name(&m.name), level, class, race, online: m.online });
                }
                v.selected = glue.selected;
                v.texts.insert("CONTROL_2357", pages.to_string());
                // Détail et boutons selon le membre choisi (FUN_1408c10f0, FUN_1408bfa80).
                let mut detail = String::new();
                let mut allow = d.msg("Guild_Join_Allow_Button_Text");
                let (mut kick, mut invite, mut allow_on) = (false, false, false);
                if let Some(s) = &selected {
                    let (race, class) = race_class(assets, &d, s.tid);
                    detail = match s.rank {
                        guild::rank::MASTER | guild::rank::SUBMASTER => {
                            let title = d.msg(if s.rank == guild::rank::MASTER { "Guild_Master" } else { "Guild_SubMaster" });
                            printf(&d.msg("Guild_GuildLeadMemberInfo"), &[&shown_name(&s.name), &s.level, &title, &race, &class])
                        }
                        _ => printf(&d.msg("Guild_GuildMemberInfo"), &[&shown_name(&s.name), &s.level, &race, &class]),
                    };
                    kick = master;
                    if leads && matches!(s.rank, guild::rank::JOIN_REQUEST | guild::rank::LEAVE_REQUEST) {
                        allow_on = !(s.rank == guild::rank::LEAVE_REQUEST && !master);
                    }
                    if s.rank == guild::rank::LEAVE_REQUEST {
                        allow = d.msg("Guild_Leave_Allow_Button_Text");
                    }
                    invite = s.online && !s.name.eq_ignore_ascii_case(&me);
                }
                v.texts.insert("CONTROL_5979", detail);
                v.texts.insert("CONTROL_23938", allow);
                for (on, c) in [(kick, "GD_Member_Button_Leave"), (invite, "CONTROL_14314"), (allow_on, "CONTROL_23938")] {
                    if !on {
                        v.disabled.insert(c);
                    }
                }
                // Chuchotement : la discussion privée n'est pas encore dans ArchRust.
                v.disabled.insert("CONTROL_24708");
                v.edits.push(GuildEdit { control: "CONTROL_12329", text: &mut glue.search, password: false, max: 48, multiline: false });
            }
        }
        crate::hud::draw_guild_window(ctx, hud, assets, win.name(), v, &colors, &mut out);
    }
    if let Some((_, text)) = &glue.ask {
        crate::hud::draw_message_box(ctx, hud, assets, text, &d.msg("UI_Name_OK"), &d.msg("UI_Name_Cancel"), &mut out);
    }
    glue.data = Some(d);
    glue.clicks.extend(out);
}


impl App {
    fn my_name(&self) -> String {
        self.player.as_ref().map(|p| p.name.clone()).unwrap_or_default()
    }

    fn my_member(&self) -> Option<&GuildMember> {
        let me = self.my_name();
        self.guild.state.member(&me)
    }

    fn guild_message(&mut self, text: &str, argb: u32) {
        if text.is_empty() {
            return;
        }
        self.hud.push_system(&text.replace("___", " "));
        self.ui.push_chat_in(MASK_GUILD, colored(&text.replace("___", " "), argb));
    }

    /// Paquets de guilde reçus.
    pub(super) fn on_guild_event(&mut self, ev: GuildEvent) {
        let assets = self.assets.clone();
        self.guild.data(&assets);
        let me = self.my_name();
        let my_cid = self.player.as_ref().map_or(0, |p| p.cid);
        match &ev {
            GuildEvent::ManagerGranted { .. } => {
                self.guild.open.insert(Win::Main);
                return;
            }
            GuildEvent::System { code, text, number, .. } => {
                let d = self.guild.data.as_ref().expect("données lues");
                if let Some((t, color)) = system_text(d, *code, text, *number) {
                    self.guild_message(&t, color);
                }
                return;
            }
            GuildEvent::Guild(info) => {
                // Création confirmée : la guilde demandée, avec le joueur pour chef (FUN_1406a36b0).
                if self.guild.pending_create.as_ref().is_some_and(|(n, _)| n.eq_ignore_ascii_case(&info.name)) && info.master.eq_ignore_ascii_case(&me) {
                    self.guild.pending_create = None;
                    self.guild.complete_name = info.name.clone();
                    self.guild.open.insert(Win::CreateComplete);
                }
            }
            GuildEvent::Destroyed { guild } => {
                if self.guild.pending_destroy.take().is_some() {
                    self.guild.complete_name = guild.clone();
                    self.guild.open.insert(Win::DestroyComplete);
                }
            }
            _ => {}
        }
        let notice = self.guild.state.apply(&ev, &me, my_cid);
        match notice.clone() {
            Some(GuildNotice::Invited(i)) => {
                let d = self.guild.data.as_ref().expect("données lues");
                let text = fill_braces(d.events.get("Guild_Invite_ConfirmUI_Open").map_or("", String::as_str), &i.guild);
                self.guild.ask = Some((Ask::Invite { guild: i.guild }, text));
            }
            Some(GuildNotice::Left) => {
                self.guild.selected = None;
                self.guild.open.remove(&Win::Member);
            }
            Some(GuildNotice::Joined | GuildNotice::NoticeChanged) => {
                self.guild.notice = self.guild.state.info.as_ref().map(|i| i.notice.clone()).unwrap_or_default();
            }
            _ => {}
        }
        // Les AddOns voient la liste comme sous WoW.
        if notice.is_some() {
            self.addons.fire_event_str("GUILD_ROSTER_UPDATE", &[]);
        }
    }

    /// Touche G et `/guilde` : `Guild_Notice_Open_Request` (FUN_1408c7f10).
    pub(super) fn guild_toggle(&mut self) {
        if self.guild.open.remove(&Win::Notice) | self.guild.open.remove(&Win::Member) {
            return;
        }
        self.guild.notice = self.guild.state.info.as_ref().map(|i| i.notice.clone()).unwrap_or_default();
        self.guild.open.insert(Win::Notice);
    }

    /// Discussion reçue sur un canal de guilde ou de groupe : ligne d'Archonia et masque d'onglet.
    pub(super) fn guild_chat_line(&mut self, channel: i8, from: &str, message: &str) -> Option<(String, u16)> {
        let assets = self.assets.clone();
        let d = self.guild.data(&assets);
        let (header, color, mask) = match channel {
            3 => (d.msg("Chat_Guild_Header"), 0xFFA500, MASK_GUILD),
            4 => (d.msg("Chat_Party_Header"), 0x7CFC00, MASK_PARTY),
            _ => return None,
        };
        Some((colored(&format!("{header}[{from}] {message}"), color), mask))
    }

    /// `/G texte`, `/P texte`, `/N texte` (modes de discussion d'Archonia) et commandes de guilde
    /// d'ArchRust : `/guilde`, `/ginviter [nom]`, `/gquitter`, `/gexclure nom`, `/gannonce texte`,
    /// `/gaccepter`, `/grefuser`.
    pub(super) fn guild_slash(&mut self, cmd: &str, arg: &str) -> bool {
        let arg = arg.trim();
        let lower = cmd.to_ascii_lowercase();
        let guild = self.guild.state.name().to_string();
        match lower.as_str() {
            "/g" | "/p" | "/n" => {
                if arg.is_empty() {
                    return true;
                }
                let kind = match lower.as_str() {
                    "/g" => 3,
                    "/p" => 4,
                    _ => 0,
                };
                if let Some(net) = &self.net {
                    net.send(Command::Chat { kind, message: arg.to_string() });
                }
            }
            "/guilde" | "/guild" => self.guild_toggle(),
            "/ginviter" | "/ginvite" => {
                let name = if arg.is_empty() {
                    self.addons.target_cid().and_then(|c| self.entities.iter().find(|e| e.cid == c)).map(|e| e.name.clone()).unwrap_or_default()
                } else {
                    arg.to_string()
                };
                if name.is_empty() || guild.is_empty() {
                    self.ui.push_chat("[guilde] Usage : /ginviter nom (ou un joueur ciblé), dans une guilde.");
                } else {
                    send(self.net.as_ref(), GuildCommand::Invite { guild, name });
                }
            }
            "/gquitter" | "/gquit" => self.guild_leave_request(),
            "/gexclure" | "/gkick" => {
                if !arg.is_empty() {
                    self.guild_kick_request(arg.to_string());
                }
            }
            "/gannonce" | "/gnotice" => {
                self.guild.notice = arg.to_string();
                self.guild_send_notice();
            }
            "/gaccepter" | "/gaccept" => self.guild_answer(true),
            "/grefuser" | "/gdecline" => self.guild_answer(false),
            _ => return false,
        }
        true
    }

    /// Réponse à la boîte OK / Annuler.
    fn guild_answer(&mut self, ok: bool) {
        let Some((ask, _)) = self.guild.ask.take() else { return };
        let guild = self.guild.state.name().to_string();
        let net = self.net.as_ref();
        match ask {
            Ask::Invite { guild } => {
                self.guild.state.invitation = None;
                send(net, if ok { GuildCommand::Join { guild } } else { GuildCommand::Reject { guild } });
            }
            _ if !ok => {}
            Ask::Leave => send(net, GuildCommand::Leave { guild, name: self.my_name() }),
            Ask::Kick(name) => send(net, GuildCommand::Kick { guild, name }),
            Ask::AllowJoin(name) => send(net, GuildCommand::AllowJoin { guild, name }),
            Ask::AllowLeave(name) => send(net, GuildCommand::AllowLeave { guild, name }),
        }
    }

    /// « Quitter la guilde » (FUN_1408c7d00) : le chef ne peut pas ; une demande en cours ne se répète pas.
    fn guild_leave_request(&mut self) {
        let Some(m) = self.my_member().cloned() else { return };
        if m.rank == guild::rank::LEAVE_REQUEST {
            return;
        }
        let assets = self.assets.clone();
        let d = self.guild.data(&assets);
        if m.rank == guild::rank::MASTER {
            let (t, c) = (d.msg("Guild_Leave_Error_Master"), d.color("Guild_System_Message_Color", 0xFFFF_FF33));
            self.guild_message(&t, c);
            return;
        }
        let text = d.events.get("Guild_Leave_ConfirmUI_Open").cloned().unwrap_or_default();
        self.guild.ask = Some((Ask::Leave, text));
    }

    /// « Expulser » (FUN_1408c0c20) : le chef confirme ; un autre membre se voit proposer de partir.
    fn guild_kick_request(&mut self, name: String) {
        let Some(m) = self.my_member().cloned() else { return };
        if m.rank != guild::rank::MASTER {
            self.guild_leave_request();
            return;
        }
        let assets = self.assets.clone();
        let d = self.guild.data(&assets);
        if name.eq_ignore_ascii_case(&m.name) {
            let (t, c) = (d.msg("Guild_Leave_Error_Master"), d.color("Guild_System_Message_Color", 0xFFFF_FF33));
            self.guild_message(&t, c);
            return;
        }
        let text = fill_braces(d.events.get("Guild_ForcedLeave_ConfirmUI_Open").map_or("", String::as_str), &name);
        self.guild.ask = Some((Ask::Kick(name), text));
    }

    /// « Envoyer un avis » : op 9 par le chef (FUN_1408c7fe0).
    fn guild_send_notice(&mut self) {
        if self.my_member().is_some_and(|m| m.rank == guild::rank::MASTER) && !self.guild.notice.trim().is_empty() {
            let guild = self.guild.state.name().to_string();
            send(self.net.as_ref(), GuildCommand::Notice { guild, text: self.guild.notice.trim().to_string() });
        }
    }

    /// Membres à part entière et demandes, dans l'ordre reçu.
    fn guild_rows(&self) -> Vec<GuildMember> {
        self.guild.state.members.clone()
    }

    fn guild_selected(&self) -> Option<GuildMember> {
        let rows = self.guild_rows();
        self.guild.selected.and_then(|i| rows.get(self.guild.page * PAGE + i).cloned())
    }

    fn guild_click(&mut self, r: GuildUi, level: i64) {
        let (window, control) = match r {
            GuildUi::MessageBox(ok) => return self.guild_answer(ok),
            GuildUi::Row(i) => {
                self.guild.selected = (self.guild.page * PAGE + i < self.guild.state.members.len()).then_some(i);
                return;
            }
            GuildUi::Click { window, control } => (window, control),
        };
        let close = |g: &mut GuildGlue, w: Win| {
            g.open.remove(&w);
        };
        let msg = |g: &GuildGlue, k: &str| g.data.as_ref().map(|d| d.msg(k)).unwrap_or_default();
        match (window.as_str(), control.as_str()) {
            ("Guild_Main", "GD_Main_Button_X") => close(&mut self.guild, Win::Main),
            ("Guild_Main", "GD_Main_Button_1") => {
                if self.guild.state.in_guild() {
                    let t = msg(&self.guild, "Guild_Msg_AlreadyMember");
                    self.guild_message(&t, 0xFFFF_FFFF);
                } else if level < CREATE_LEVEL {
                    let t = msg(&self.guild, "Guild_Msg_NeedMoreLevel");
                    self.guild_message(&t, 0xFFFF_FFFF);
                } else {
                    close(&mut self.guild, Win::Main);
                    self.guild.open.insert(Win::Create);
                }
            }
            ("Guild_Main", "GD_Main_Button_2") => {
                close(&mut self.guild, Win::Main);
                self.guild.open.insert(Win::Destroy);
            }
            ("Guild_Create", "GD_Create_Button_OK") => {
                let (n, p, p2) = (self.guild.name.trim().to_string(), self.guild.password.clone(), self.guild.password2.clone());
                if n.is_empty() || n.len() > 32 || p.is_empty() || p.len() > 12 || p2.is_empty() {
                    return;
                }
                if p != p2 {
                    let t = msg(&self.guild, "Guild_Msg_CheckPassword");
                    self.guild_message(&t, 0xFFFF_FFFF);
                    return;
                }
                self.guild.pending_create = Some((n, p));
                self.guild.name.clear();
                self.guild.password.clear();
                self.guild.password2.clear();
                close(&mut self.guild, Win::Create);
                self.guild.open.insert(Win::CreateRequest);
            }
            ("Guild_Create", _) => close(&mut self.guild, Win::Create),
            ("Guild_Create_Request", "GD_Request_Button_Ok") => {
                close(&mut self.guild, Win::CreateRequest);
                if let Some((name, password)) = self.guild.pending_create.clone() {
                    send(self.net.as_ref(), GuildCommand::Create { name, password });
                }
            }
            ("Guild_Create_Request", _) => {
                self.guild.pending_create = None;
                close(&mut self.guild, Win::CreateRequest);
            }
            ("Guild_Create_Complete", _) => close(&mut self.guild, Win::CreateComplete),
            ("Guild_Destroy", "GD_Destroy_Button_Ok") => {
                if self.guild.destroy_password.is_empty() {
                    return;
                }
                self.guild.pending_destroy = Some(std::mem::take(&mut self.guild.destroy_password));
                close(&mut self.guild, Win::Destroy);
                self.guild.open.insert(Win::DestroyConfirm);
            }
            ("Guild_Destroy", _) => close(&mut self.guild, Win::Destroy),
            ("Guild_Destroy_Confirm", "GD_Destroy_Confirm_Button_Ok") => {
                close(&mut self.guild, Win::DestroyConfirm);
                if let Some(password) = self.guild.pending_destroy.clone() {
                    let guild = self.guild.state.name().to_string();
                    send(self.net.as_ref(), GuildCommand::Destroy { guild, password });
                }
            }
            ("Guild_Destroy_Confirm", _) => {
                self.guild.pending_destroy = None;
                close(&mut self.guild, Win::DestroyConfirm);
            }
            ("Guild_Destroy_Complete", _) => close(&mut self.guild, Win::DestroyComplete),
            ("Guild_Notice", "GD_Notice_Button_Send") => self.guild_send_notice(),
            ("Guild_Notice", "GD_Notice_Button_Tab2") => {
                close(&mut self.guild, Win::Notice);
                self.guild.open.insert(Win::Member);
            }
            ("Guild_Notice", "CONTROL_18715") => self.guild_leave_request(),
            ("Guild_Notice", "GD_Notice_Button_X" | "GD_Notice_Button_Tab4") => close(&mut self.guild, Win::Notice),
            ("Guild_Member", "GD_Member_Button_Tab1") => {
                close(&mut self.guild, Win::Member);
                self.guild.notice = self.guild.state.info.as_ref().map(|i| i.notice.clone()).unwrap_or_default();
                self.guild.open.insert(Win::Notice);
            }
            ("Guild_Member", "GD_Member_Button_X" | "GD_Member_Button_Tab4") => close(&mut self.guild, Win::Member),
            ("Guild_Member", "GD_Member_Button_Left") => {
                if self.guild.page > 0 {
                    self.guild.page -= 1;
                    self.guild.selected = None;
                }
            }
            ("Guild_Member", "GD_Member_Button_Right") => {
                if (self.guild.page + 1) * PAGE < self.guild.state.members.len() {
                    self.guild.page += 1;
                    self.guild.selected = None;
                }
            }
            ("Guild_Member", "CONTROL_23904") => {
                let wanted = self.guild.search.trim().to_string();
                match self.guild.state.members.iter().position(|m| m.name.eq_ignore_ascii_case(&wanted)) {
                    Some(i) => {
                        self.guild.page = i / PAGE;
                        self.guild.selected = Some(i % PAGE);
                    }
                    None if !wanted.is_empty() => {
                        let t = printf(&msg(&self.guild, "Guild_No_Exist_Member"), &[&wanted]);
                        self.guild_message(&t, 0xFFFF_FFFF);
                    }
                    None => {}
                }
            }
            ("Guild_Member", "GD_Member_Button_Leave") => {
                if let Some(s) = self.guild_selected() {
                    self.guild_kick_request(s.name);
                }
            }
            ("Guild_Member", "CONTROL_14314") => {
                if let (Some(s), Some(net)) = (self.guild_selected(), &self.net) {
                    net.send(Command::Party(PartyCommand::Invite { cid: None, name: Some(s.name) }));
                }
            }
            ("Guild_Member", "CONTROL_23938") => {
                if let Some(s) = self.guild_selected() {
                    let assets = self.assets.clone();
                    let d = self.guild.data(&assets);
                    let (ask, event) = if s.rank == guild::rank::LEAVE_REQUEST { (Ask::AllowLeave(s.name), "Guild_Confirm_Allow_Leave") } else { (Ask::AllowJoin(s.name), "Guild_Confirm_Allow_Join") };
                    let text = d.events.get(event).cloned().unwrap_or_default();
                    self.guild.ask = Some((ask, text));
                }
            }
            _ => {}
        }
    }

    /// Clics de la dernière image ; onglets de la discussion (`Chat_Tab_*` de uimessage).
    pub(super) fn guild_frame(&mut self) {
        let assets = self.assets.clone();
        if self.ui.chat_tabs.is_empty() {
            let d = self.guild.data(&assets);
            self.ui.chat_tabs = vec![(d.msg("Chat_Tab_Normal"), u16::MAX), (d.msg("Chat_Tab_Party"), MASK_PARTY), (d.msg("Chat_Tab_Guild"), MASK_GUILD), (d.msg("Chat_Tab_Whisper"), MASK_WHISPER)];
        }
        let level = self.hud.stats.level.map_or(0, |l| l as i64);
        for r in std::mem::take(&mut self.guild.clicks) {
            self.guild_click(r, level);
        }
    }

    /// Déconnexion : plus de guilde ni de fenêtre.
    pub(super) fn guild_reset(&mut self) {
        let data = self.guild.data.take();
        self.guild = GuildGlue { data, ..Default::default() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn textes_d_archonia() {
        assert_eq!(printf("Il vous faut %d Or pour creer une guilde.", &[&10_000]), "Il vous faut 10000 Or pour creer une guilde.");
        assert_eq!(fill_braces("An invite has been sent from the guild {{Guild_Join_GuildID.Guild_Join_GuildID}}.  Do you wish to accept?", "Rust"), "An invite has been sent from the guild Rust.  Do you wish to accept?");
        assert_eq!(shown_name("Bob@serveur"), "Bob...");
        assert_eq!(colored("x", 0xFFFF_A500), "|cffffa500x|r");
    }
}
