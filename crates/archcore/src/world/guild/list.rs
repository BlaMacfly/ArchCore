//! Liste des guildes : AgpmGuild op 28 (FUN_140203630, FUN_140203e90, FUN_140204010).
//!
//! ```text
//!   → CID (1), 21 = GUILD_LIST { 1 : page }       page demandée (0 à l'ouverture, puis ← →)
//!   → CID (1), 2 = nom                           recherche : la page qui contient cette guilde
//!   ← CID (1), 2 = nom cherché, 21 = GUILD_LIST { nombre total, page, nombre de la page, 19 entrées }
//! ```
//! Entrée (GUILD_LIST_ITEM) : nom, chef, second, 0, 0, membres, taille, 0, emblème (TID, couleur),
//! points de combat ; les deux zéros sont ceux qu'écrit Archonia. Les guildes sont prises dans l'ordre de
//! leur création, 19 par page ; une page vide n'a pas de réponse. Recherche sans résultat : code 47.

use super::{code, op, rank, send_to, system_packet, Guild};
use crate::world::WorldState;
use archcore_net::schema::{self, mini};
use archcore_net::{build_mini, build_packet, decode_mini, Fields, Packet};

/// Guildes par page (`Guild_List_Index`, 19 lignes).
pub const PAGE: usize = 19;

/// Entrée GUILD_LIST_ITEM d'une guilde (aussi pour les détails des alliées et des hostiles).
pub(super) fn item(g: &Guild) -> Vec<u8> {
    let s = &mini::GUILD_LIST_ITEM;
    let name_of = |r: i64| g.members.values().find(|m| m.rank == r).map(|m| m.name.clone()).unwrap_or_default();
    let members = g.members.values().filter(|m| m.active()).count() as i64;
    let f = Fields::new(s)
        .with(0, g.name.as_str())
        .with(1, name_of(rank::MASTER).as_str())
        .with(2, name_of(rank::SUBMASTER).as_str())
        .with(3, 0)
        .with(4, 0)
        .with(5, members)
        .with(6, g.max_members)
        .with(7, 0)
        .with(8, g.mark_tid)
        .with(9, g.mark_color)
        .with(10, g.record.points);
    build_mini(s, &f)
}

/// Guildes dans l'ordre de la liste.
fn ordered(st: &WorldState) -> Vec<&Guild> {
    let mut v: Vec<&Guild> = st.guilds.by_id.values().collect();
    v.sort_by_key(|g| g.id);
    v
}

/// Page `page` de la liste, pour le CID `cid` ; `searched` : nom recherché, rendu au champ 2.
pub fn page_packet(st: &WorldState, cid: u32, page: usize, searched: Option<&str>) -> Option<Vec<u8>> {
    let all = ordered(st);
    let start = page.checked_mul(PAGE)?;
    if start >= all.len() {
        return None;
    }
    let shown = &all[start..(start + PAGE).min(all.len())];
    let s = &mini::GUILD_LIST;
    let mut f = Fields::new(s).with(0, all.len() as i64).with(1, page as i64).with(2, shown.len() as i64);
    for (i, g) in shown.iter().enumerate() {
        f = f.with(3 + i, item(g));
    }
    let main = Fields::new(&schema::GUILD).with(0, op::GUILD_LIST).with(1, cid).with_opt(2, searched).with(21, build_mini(s, &f));
    Some(build_packet(&schema::GUILD, &main))
}

pub fn on_packet(st: &WorldState, sid: u32, p: &Packet) {
    let Some(player) = st.players.get(&sid) else { return };
    let cid = player.ch.id;
    let searched = p.text(2).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    if let Some(name) = searched {
        let key = name.to_lowercase();
        let Some(i) = ordered(st).iter().position(|g| g.name.to_lowercase() == key) else {
            send_to(st, sid, system_packet(code::NO_EXIST_SEARCH_GUILD, None, None));
            return;
        };
        if let Some(pkt) = page_packet(st, cid, i / PAGE, Some(&name)) {
            send_to(st, sid, pkt);
        }
        return;
    }
    let page = p.bytes(21).and_then(|b| decode_mini(&mini::GUILD_LIST, b).ok()).and_then(|l| l.int(1)).unwrap_or(0).max(0) as usize;
    if let Some(pkt) = page_packet(st, cid, page, None) {
        send_to(st, sid, pkt);
    }
}
