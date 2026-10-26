//! Emblèmes de guilde : AgpmGuild op 32 (achat) et op 33 (achat malgré une guilde au même emblème).
//!
//! ```text
//!   → CID (1), guilde (2), TID (22), couleur (23)      FUN_140202210 / FUN_1402022d0
//!   ← la même op à toute la guilde ; le lien de chaque membre en ligne (op 8, 22-23) aux joueurs qui le voient
//! ```
//! TID = fond | motif << 8 | symbole << 16 (numéros de guildmark.txt), couleur = indice de couleur par couche
//! (mêmes décalages ; FUN_140205be0, FUN_140205c50). Comme Archonia : le chef seul, 10 membres au moins
//! (`Guild_GuildMark_Not_Enought_Member`), un fond obligatoire (code 58), prix en or et en argent liquide
//! de chaque couche choisie (FUN_14055b200) ; une autre guilde au même emblème et à la même couleur donne
//! le code 59, auquel le client répond par l'op 33 s'il confirme (`Guild_GuildMark_Duplicate`).

use super::{code, mine, rank, send_to, send_to_guild};
use crate::db::col;
use crate::world::{WorldCtx, WorldState};
use crate::{info, warn};
use archcore_net::{build_packet, schema, Fields, Packet};
use std::collections::HashMap;

pub const BUY: i64 = 32;
pub const BUY_FORCE: i64 = 33;
/// Membres exigés pour choisir un emblème (FUN_14055b920).
pub const MIN_MEMBERS: usize = 10;

/// Un élément de guildmark.txt.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Template {
    pub tid: u32,
    pub layer: u8,
    pub name: String,
    pub gold: i64,
    pub skull_tid: i64,
    pub skull_count: i64,
    pub cash: i64,
}

/// Éléments d'emblème par TID (base du monde, table `guild_mark`).
#[derive(Default)]
pub struct Templates(pub HashMap<u32, Template>);

impl Templates {
    pub async fn load(db: &crate::db::Databases) -> Self {
        match db.world.rows("SELECT tid, layer, name, gold, skull_tid, skull_count, cash FROM guild_mark", ()).await {
            Ok(rows) => Templates(
                rows.iter()
                    .map(|r| {
                        let t = Template {
                            tid: col(r, "tid"),
                            layer: col(r, "layer"),
                            name: col(r, "name"),
                            gold: col(r, "gold"),
                            skull_tid: col(r, "skull_tid"),
                            skull_count: col(r, "skull_count"),
                            cash: col(r, "cash"),
                        };
                        (t.tid, t)
                    })
                    .collect(),
            ),
            Err(e) => {
                warn!("emblèmes de guilde non chargés ({e:#}) : lancer `archcore db-init` puis `archcore import`");
                Templates::default()
            }
        }
    }

    /// Éléments choisis d'un emblème (fond, motif, symbole ; 0 = aucun) : `None` si l'un est inconnu.
    pub fn parts(&self, tid: u32) -> Option<Vec<&Template>> {
        let mut out = Vec::new();
        for (layer, mask) in [(1u8, 0xffu32), (2, 0xff00), (3, 0xff_0000)] {
            let part = tid & mask;
            if part == 0 {
                continue;
            }
            out.push(self.0.get(&part).filter(|t| t.layer == layer)?);
        }
        Some(out)
    }
}

/// Enregistre l'argent liquide d'un personnage : (montant, CID).
const SAVE_CASH: &str = "UPDATE characters SET cash = ? WHERE id = ?";

pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet, force: bool) -> anyhow::Result<()> {
    let Some((cid, gid, my_rank)) = mine(st, sid) else { return Ok(()) };
    let g = &st.guilds.by_id[&gid];
    if my_rank != rank::MASTER || g.active_members() < MIN_MEMBERS {
        return Ok(());
    }
    let tid = p.int(22).unwrap_or(0) as u32 & 0xff_ffff;
    let color = p.int(23).unwrap_or(0);
    if tid & 0xff == 0 {
        send_to(st, sid, super::system_packet(code::BUY_GUILDMARK_NO_EXIST_BOTTOM, None, None));
        return Ok(());
    }
    let Some(parts) = st.guilds.marks.parts(tid) else {
        warn!("[guilde] #{sid} emblème {tid:#x} inconnu de guildmark.txt");
        return Ok(());
    };
    // Aucun élément de guildmark.txt ne demande de crânes ; ArchCore ne sait pas encore les prendre.
    if parts.iter().any(|t| t.skull_tid != 0 && t.skull_count > 0) {
        send_to(st, sid, super::system_packet(code::BUY_GUILDMARK_NOT_ENOUGHT_SKULL, None, None));
        return Ok(());
    }
    let gold: i64 = parts.iter().map(|t| t.gold).sum();
    let cash: i64 = parts.iter().map(|t| t.cash).sum();
    let player = &st.players[&sid];
    let (money, my_cash) = (player.ch.money as i64, player.ch.cash as i64);
    if money < gold || my_cash < cash {
        send_to(st, sid, super::system_packet(code::BUY_GUILDMARK_NOT_ENOUGHT_GHELD, None, None));
        return Ok(());
    }
    if !force && st.guilds.by_id.values().any(|o| o.id != gid && o.mark_tid == tid as i64 && o.mark_color == color) {
        send_to(st, sid, super::system_packet(code::BUY_GUILDMARK_DUPLICATE, None, None));
        return Ok(());
    }
    ctx.db
        .characters
        .atomic(vec![
            (crate::world::SAVE_MONEY, (money - gold, cid).into()),
            (SAVE_CASH, (my_cash - cash, cid).into()),
            ("UPDATE guild SET mark_tid = ?, mark_color = ? WHERE id = ?", (tid as i64, color, gid).into()),
        ])
        .await?;
    let player = st.players.get_mut(&sid).unwrap();
    crate::world::show_money(player, (money - gold) as u64);
    if cash > 0 {
        player.ch.cash = (my_cash - cash) as u64;
        let c = &schema::CHARACTER;
        player.send(c, Fields::new(c).with(0, 1).with(1, cid).with(10, player.ch.cash));
    }
    let g = st.guilds.by_id.get_mut(&gid).unwrap();
    g.mark_tid = tid as i64;
    g.mark_color = color;
    let g = &st.guilds.by_id[&gid];
    let pkt = build_packet(&schema::GUILD, &super::guild(BUY).with(1, cid).with(2, g.name.as_str()).with(22, g.mark_tid).with(23, g.mark_color));
    send_to_guild(st, g, None, &pkt);
    let online: Vec<u32> = g.members.values().filter(|m| m.online).map(|m| m.cid).collect();
    for m in online {
        super::announce_link(st, m);
    }
    info!("[guilde] {} choisit l'emblème {tid:#x} (couleur {color:#x}) pour {} : {gold} pièce(s), {cash} en argent liquide", st.players[&sid].ch.name, g.name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn couches() {
        let mut t = Templates::default();
        for (tid, layer) in [(3u32, 1u8), (2 << 8, 2), (65 << 16, 3)] {
            t.0.insert(tid, Template { tid, layer, gold: 10, ..Default::default() });
        }
        assert_eq!(t.parts(3 | 65 << 16).map(|p| p.len()), Some(2));
        assert_eq!(t.parts(3 | 2 << 8 | 65 << 16).map(|p| p.iter().map(|x| x.gold).sum::<i64>()), Some(30));
        assert!(t.parts(4).is_none());
        assert!(t.parts(2 << 8 << 8).is_none(), "motif à la place d'un symbole");
    }
}
