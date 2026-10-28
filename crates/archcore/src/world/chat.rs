//! Discussion (AGPMCHATTING, 0x0F), cris de zone (AgpmAreaChatting, 0x4C) et données MJ (AGPMADMIN, 0x12).
//!
//! Le client envoie un message sans zéro final. Si le joueur a choisi une couleur de discussion, il
//! ajoute « \x01RRGGBB » à la fin du message : ce suffixe est retiré avant d'interpréter le texte, puis
//! remis sur le message relayé.
//!
//! Archonia envoie tout en type 0, le mode de la barre de discussion ajoutant son préfixe au texte : « /p » groupe,
//! « /g » guilde, « /s » cri local ; le joueur tape aussi « /race », « /scream » et « /warn » (`split_prefix`). Ces
//! préfixes sont lus avant les commandes MJ ; une commande inconnue d'un joueur sans droits est ignorée sans réponse.
//! Seul le chuchotement arrive dans son type (5, nom de la cible).
//!
//! Cris (choix d'ArchCore, le client n'en dit que les objets et l'affichage) :
//! * `/s` : AGPMCHATTING type 1 « [Shout] » aux joueurs à moins de `Chat.ShoutRange` (0 : deux fois la vue) ;
//! * `/race`, `/scream`, `/warn` : un objet consommé par message (Race Trumpet 3905 ; Shout of Elemental 3906, 6182,
//!   11518 ; Warning of Ashtal 3911), refus s'il manque ; AgpmAreaChatting op 0 de type 19 (même race), 20 ou 7
//!   (tous les joueurs : un seul monde), texte « [Objet] [Nom] message » composé par le serveur (ces onglets n'ont
//!   ni en-tête ni nom).
//!
//! Cadence (valeurs du code partagé d'Archonia) : un message toutes les 100 ms au plus (le surplus est ignoré), un
//! cri de zone par seconde (refus avec message). Les commandes MJ n'y sont pas soumises.

use super::{gm, send_system, World, WorldCtx, WorldState};
use crate::{debug, info};
use archcore_net::schema::{self, chat_type, mini};
use archcore_net::{build_mini, build_packet, Fields, Packet};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Taille maximale d'un message accepté par le client à la réception.
pub const CHAT_MAX_BYTES: usize = 512;
/// Délai minimal entre deux messages, et entre deux cris de zone.
pub const CHAT_INTERVAL: Duration = Duration::from_millis(100);
pub const SHOUT_INTERVAL: Duration = Duration::from_millis(1000);

/// Dernier message et dernier cri de zone d'un joueur.
#[derive(Clone, Copy, Debug, Default)]
pub struct ChatClock {
    pub said: Option<Instant>,
    pub shouted: Option<Instant>,
}

impl ChatClock {
    /// Message permis à `now` (noté s'il l'est).
    pub fn allow_say(&mut self, now: Instant) -> bool {
        if self.said.is_some_and(|t| now < t + CHAT_INTERVAL) {
            return false;
        }
        self.said = Some(now);
        true
    }
    pub fn may_shout(&self, now: Instant) -> bool {
        !self.shouted.is_some_and(|t| now < t + SHOUT_INTERVAL)
    }
}

/// Cri de zone : `/race`, `/scream`, `/warn`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Area {
    Race,
    Scream,
    Warn,
}

impl Area {
    /// Objets « usable » de genre 20 dont le SubType est ce cri (itemdatatable.txt) ; un est consommé par message.
    pub fn items(self) -> &'static [u32] {
        match self {
            Area::Race => &[3905],
            Area::Scream => &[3906, 6182, 11518],
            Area::Warn => &[3911],
        }
    }
    /// Genre du cri (SubType des objets), champ 2 d'AgpmAreaChatting.
    pub fn kind(self) -> i64 {
        match self {
            Area::Race => 1,
            Area::Scream => 2,
            Area::Warn => 3,
        }
    }
    /// Type AGPMCHATTING du message, qui choisit l'onglet du client.
    pub fn chat_type(self) -> i64 {
        match self {
            Area::Race => chat_type::RACE,
            Area::Scream => chat_type::SCREAM,
            Area::Warn => chat_type::WHOLE_WORLD,
        }
    }
    /// Nom de l'objet exigé, si le catalogue ne le donne pas.
    pub fn item_label(self) -> &'static str {
        match self {
            Area::Race => "Race Trumpet",
            Area::Scream => "Shout of Elemental",
            Area::Warn => "Warning of Ashtal",
        }
    }
}

/// Ce que demande le préfixe d'un message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prefix {
    Party,
    Guild,
    Shout,
    Area(Area),
}

/// Préfixe d'un message (« /p », « /g », « /s », « /race » ou « /racetrumpet », « /scream », « /warn », casse
/// ignorée) et le message qui suit ; `None` sans préfixe connu (commande MJ ou texte ordinaire).
pub fn split_prefix(text: &str) -> Option<(Prefix, &str)> {
    let rest = text.strip_prefix('/')?;
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let prefix = match rest[..end].to_lowercase().as_str() {
        "p" => Prefix::Party,
        "g" => Prefix::Guild,
        "s" => Prefix::Shout,
        "race" | "racetrumpet" => Prefix::Area(Area::Race),
        "scream" => Prefix::Area(Area::Scream),
        "warn" => Prefix::Area(Area::Warn),
        _ => return None,
    };
    Some((prefix, rest[end..].trim_start()))
}

/// Portée du cri local `/s` : `Chat.ShoutRange`, ou deux fois le rayon de vue s'il vaut 0.
pub fn shout_range(configured: f64, view_radius: f64) -> f64 {
    if configured > 0.0 { configured } else { view_radius * 2.0 }
}

fn strip_nuls(raw: &[u8]) -> &[u8] {
    let end = raw.iter().rposition(|&b| b != 0).map(|i| i + 1).unwrap_or(0);
    &raw[..end]
}

fn color_suffix(raw: &[u8]) -> Option<usize> {
    if raw.len() >= 7 && raw[raw.len() - 7] == 1 && raw[raw.len() - 6..].iter().all(|b| b.is_ascii_hexdigit()) {
        Some(raw.len() - 7)
    } else {
        None
    }
}

/// Texte d'un message reçu : sans zéro final ni suffixe de couleur.
pub fn chat_text(block: &[u8]) -> String {
    let raw = strip_nuls(block);
    let raw = color_suffix(raw).map(|i| &raw[..i]).unwrap_or(raw);
    archcore_net::text::decode(raw)
}

/// Couleur de discussion choisie par le joueur (suffixe « \x01RRGGBB »), ou `None`.
pub fn chat_color(block: &[u8]) -> Option<Vec<u8>> {
    let raw = strip_nuls(block);
    color_suffix(raw).map(|i| raw[i..].to_vec())
}

/// Paquet AGPMCHATTING op 0. Émetteur facultatif pour les messages système.
pub fn chat_packet(kind: i64, sender_cid: Option<u32>, sender_name: Option<&str>, target_name: Option<&str>, text: &str) -> Vec<u8> {
    chat_packet_bytes(kind, sender_cid, sender_name, target_name, archcore_net::text::encode(text))
}

pub fn chat_packet_bytes(kind: i64, sender_cid: Option<u32>, sender_name: Option<&str>, target_name: Option<&str>, mut data: Vec<u8>) -> Vec<u8> {
    data.truncate(CHAT_MAX_BYTES);
    let s = &schema::CHATTING;
    build_packet(
        s,
        &Fields::new(s).with(0, 0).with_opt(1, sender_cid).with(2, kind).with_opt(4, sender_name).with_opt(5, target_name).with(6, data),
    )
}

/// Paquet AGPMADMIN op 0 qui donne au client le niveau MJ d'un personnage (mini ADMIN_CHAR op 1).
pub fn admin_level_packet(cid: u32, name: &str, level: i64, account: &str) -> Vec<u8> {
    let m = &mini::ADMIN_CHAR;
    let info = build_mini(m, &Fields::new(m).with(0, 1).with(1, cid).with(2, name).with(3, level).with(5, account));
    let s = &schema::ADMIN;
    build_packet(s, &Fields::new(s).with(0, 0).with(1, cid).with(2, info))
}

pub async fn on_chatting(world: &Arc<World>, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some(player) = st.players.get(&sid) else { return Ok(()) };
    if p.int(0) != Some(0) {
        crate::debug!("[world] #{sid} AGPMCHATTING op {:?} ignoré", p.int(0));
        return Ok(());
    }
    let mut kind = p.int(2).unwrap_or(chat_type::NORMAL);
    let block = p.bytes(6).unwrap_or_default().to_vec();
    let full = chat_text(&block);
    if full.is_empty() {
        return Ok(());
    }
    let (cid, name) = (player.ch.id, player.ch.name.clone());
    info!("[chat] {name} (type {kind}) : {full}");

    // Préfixes d'Archonia (groupe, guilde, cris) avant les commandes MJ ; jamais dans un chuchotement.
    let prefix = if kind == chat_type::WHISPER { None } else { split_prefix(&full) };
    let text = prefix.map(|(_, rest)| rest.to_string()).unwrap_or_else(|| full.clone());
    // Commandes de l'ArchLord (/archlord, /guardactivate…) avant les commandes MJ (archlord.rs).
    if prefix.is_none() && text.starts_with('/') && super::archlord::chat_command(&world.ctx, st, sid, &text).await? {
        return Ok(());
    }
    if prefix.is_none() && text.starts_with('/') && gm::execute(world, st, sid, &text).await? {
        return Ok(());
    }
    if text.is_empty() {
        return Ok(());
    }
    if let Some(until) = st.chat_bans.get(&cid).copied() {
        let now = Instant::now();
        if until > now {
            let left = (until - now).as_secs_f64().ceil() as i64;
            send_system(st, sid, &format!("Vous ne pouvez pas parler encore {}.", gm::format_duration(left)));
            return Ok(());
        }
    }
    match prefix.map(|(p, _)| p) {
        Some(Prefix::Party) => kind = chat_type::PARTY,
        Some(Prefix::Guild) => kind = chat_type::GUILD,
        Some(Prefix::Shout) => kind = chat_type::WORD_BALLOON,
        Some(Prefix::Area(area)) => return area_chat(&world.ctx, st, sid, area, &text, chat_color(&block).unwrap_or_default()).await,
        None => {}
    }
    if !st.players.get_mut(&sid).is_some_and(|p| p.chat.allow_say(Instant::now())) {
        debug!("[chat] {name} : message trop rapproché, ignoré");
        return Ok(());
    }
    let mut colored = archcore_net::text::encode(&text);
    colored.extend(chat_color(&block).unwrap_or_default());
    if kind == chat_type::WHISPER {
        let target_name = p.text(5).unwrap_or_default();
        let Some(tsid) = st.by_name(&target_name) else {
            send_system(st, sid, &format!("{target_name} n'est pas connecté."));
            return Ok(());
        };
        let target = &st.players[&tsid];
        let pkt = chat_packet_bytes(kind, Some(cid), Some(&name), Some(&target.ch.name), colored);
        // Auteur ignoré avec « Block Chat » (buddy.rs) : rien n'est transmis, et l'auteur n'en sait rien (il voit
        // son message comme d'habitude), comme avec le filtre du client.
        if super::buddy::blocks(st, tsid, cid, archcore_net::schema::buddy_flag::BLOCK_CHAT) {
            info!("[chat] chuchotement de {name} à {} bloqué (ignoré)", target.ch.name);
        } else {
            target.send_raw(pkt.clone());
        }
        if tsid != sid {
            st.players[&sid].send_raw(pkt);
        }
        return Ok(());
    }
    // Discussion de guilde : aux membres en ligne ; de groupe : aux membres du groupe.
    if kind == chat_type::GUILD {
        let pkt = chat_packet_bytes(kind, Some(cid), Some(&name), None, colored);
        if !super::guild::chat(st, cid, &pkt) {
            send_system(st, sid, "Vous n'appartenez à aucune guilde.");
        }
        return Ok(());
    }
    if kind == chat_type::PARTY {
        let Some(members) = st.parties.party_of(cid).map(|p| p.members.clone()) else {
            send_system(st, sid, "Vous n'appartenez à aucun groupe.");
            return Ok(());
        };
        let pkt = chat_packet_bytes(kind, Some(cid), Some(&name), None, colored);
        for m in members {
            if let Some(p) = st.by_cid.get(&m).and_then(|s| st.players.get(s)) {
                p.send_raw(pkt.clone());
            }
        }
        return Ok(());
    }
    // Discussion normale : tous les joueurs à portée de vue, émetteur compris (le client n'affiche son
    // propre message qu'au retour du serveur) ; cri local : même chose, plus loin.
    let (kind, radius) = match kind {
        chat_type::WORD_BALLOON => (kind, shout_range(world.ctx.config.world.chat_shout_range, st.view_radius)),
        _ => (chat_type::NORMAL, st.view_radius),
    };
    let pkt = chat_packet_bytes(kind, Some(cid), Some(&name), None, colored);
    let me = st.players[&sid].ch.pos();
    let r2 = radius * radius;
    for other in st.players.values() {
        let o = other.ch.pos();
        let (dx, dz) = ((o.x - me.x) as f64, (o.z - me.z) as f64);
        if dx * dx + dz * dz <= r2 {
            other.send_raw(pkt.clone());
        }
    }
    Ok(())
}

/// Paquet AgpmAreaChatting op 0.
pub fn area_packet(area: Area, cid: u32, name: &str, text: Vec<u8>) -> Vec<u8> {
    let s = &schema::AREA_CHATTING;
    let mut text = text;
    text.truncate(CHAT_MAX_BYTES);
    build_packet(s, &Fields::new(s).with(0, 0).with(1, cid).with(2, area.kind()).with(3, 1).with(4, area.chat_type()).with(5, name).with(6, text))
}

/// Cri de zone : cadence, objet consommé, puis AgpmAreaChatting aux joueurs visés (émetteur compris).
async fn area_chat(ctx: &WorldCtx, st: &mut WorldState, sid: u32, area: Area, text: &str, color: Vec<u8>) -> anyhow::Result<()> {
    let now = Instant::now();
    let Some(p) = st.players.get(&sid) else { return Ok(()) };
    if !p.chat.may_shout(now) {
        send_system(st, sid, "Attendez un instant avant de crier de nouveau.");
        return Ok(());
    }
    let found = p.inventory.items.values().find(|i| area.items().contains(&i.tid)).map(|i| (i.iid, i.tid));
    let Some((iid, tid)) = found else {
        let wanted = ctx.catalog.get(area.items()[0]).map(|d| d.name.clone()).unwrap_or_else(|| area.item_label().to_string());
        send_system(st, sid, &format!("Il vous faut l'objet « {wanted} » pour ce cri."));
        return Ok(());
    };
    let p = st.players.get_mut(&sid).unwrap();
    let (cid, name, race) = (p.ch.id, p.ch.name.clone(), p.factors.ftype[0]);
    match p.inventory.consume(&ctx.db, iid, 1).await? {
        Some(left) => p.send_raw(super::items::item_count(&left, cid)),
        None => p.send_raw(super::items::item_remove(iid, Some(cid))),
    }
    p.chat.shouted = Some(now);
    let title = ctx.catalog.get(tid).map(|d| d.name.clone()).unwrap_or_else(|| area.item_label().to_string());
    let mut bytes = archcore_net::text::encode(&format!("[{title}] [{name}] {text}"));
    bytes.extend(color);
    let pkt = area_packet(area, cid, &name, bytes);
    let mut heard = 0;
    for other in st.players.values().filter(|o| o.in_game && (area != Area::Race || o.factors.ftype[0] == race)) {
        other.send_raw(pkt.clone());
        heard += 1;
    }
    info!("[chat] {name} crie ({area:?}, objet {tid}) à {heard} joueur(s) : {text}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suffixe_de_couleur() {
        let raw = b"bonjour\x01FF8800\0".to_vec();
        assert_eq!(chat_text(&raw), "bonjour");
        assert_eq!(chat_color(&raw), Some(b"\x01FF8800".to_vec()));
        assert_eq!(chat_text(b"/SetExp 2"), "/SetExp 2");
        assert_eq!(chat_color(b"salut"), None);
    }

    #[test]
    fn prefixes_d_archonia() {
        assert_eq!(split_prefix("/p En route"), Some((Prefix::Party, "En route")));
        assert_eq!(split_prefix("/G salut"), Some((Prefix::Guild, "salut")));
        assert_eq!(split_prefix("/s Ohé"), Some((Prefix::Shout, "Ohé")));
        assert_eq!(split_prefix("/race Pour la race"), Some((Prefix::Area(Area::Race), "Pour la race")));
        assert_eq!(split_prefix("/RaceTrumpet x"), Some((Prefix::Area(Area::Race), "x")));
        assert_eq!(split_prefix("/scream a"), Some((Prefix::Area(Area::Scream), "a")));
        assert_eq!(split_prefix("/warn"), Some((Prefix::Area(Area::Warn), "")));
        assert_eq!(split_prefix("/party"), None, "commande, pas préfixe");
        assert_eq!(split_prefix("/level 40"), None);
        assert_eq!(split_prefix("bonjour /p"), None);
    }

    #[test]
    fn cris_de_zone() {
        assert_eq!((Area::Race.kind(), Area::Race.chat_type()), (1, chat_type::RACE));
        assert_eq!((Area::Scream.kind(), Area::Scream.chat_type()), (2, chat_type::SCREAM));
        assert_eq!((Area::Warn.kind(), Area::Warn.chat_type()), (3, chat_type::WHOLE_WORLD));
        assert_eq!(Area::Scream.items(), &[3906, 6182, 11518]);
        let p = archcore_net::decode_packet(&schema::AREA_CHATTING, &area_packet(Area::Race, 7, "SimA", b"[Race Trumpet] [SimA] salut".to_vec())).unwrap();
        assert_eq!((p.int(0), p.int(1), p.int(2), p.int(3), p.int(4)), (Some(0), Some(7), Some(1), Some(1), Some(19)));
        assert_eq!((p.text(5).as_deref(), p.text(6).as_deref()), (Some("SimA"), Some("[Race Trumpet] [SimA] salut")));
        assert_eq!(shout_range(0.0, 8000.0), 16000.0);
        assert_eq!(shout_range(5000.0, 8000.0), 5000.0);
    }

    #[test]
    fn cadence() {
        let t = Instant::now();
        let mut c = ChatClock::default();
        assert!(c.allow_say(t));
        assert!(!c.allow_say(t + Duration::from_millis(50)), "moins de 100 ms");
        assert!(c.allow_say(t + Duration::from_millis(120)));
        assert!(c.may_shout(t));
        c.shouted = Some(t);
        assert!(!c.may_shout(t + Duration::from_millis(900)));
        assert!(c.may_shout(t + Duration::from_millis(1000)));
    }
}
