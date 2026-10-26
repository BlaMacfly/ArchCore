//! Second et passation de chef : AgpmGuild op 48, dont la suite est brute (sous-opération en +0x18).
//!
//! ```text
//!   2 → (0x80)  +0x19 émetteur, +0x4a membre, +0x7c rang : 11 nommer second, 1 lui retirer ce rang
//!   3 ← (0xa0)  +0x19 guilde, +0x4a chef, +0x6b membre, +0x9c rang : le membre confirme sa nomination
//!   4 → (0x84)  +0x19 guilde, +0x4a soi, +0x7c rang, +0x80 réponse (1 accepte)
//!   5 → (0x7b)  +0x19 émetteur, +0x4a second : passation de chef
//!   6 ← (0x9c)  +0x19 guilde, +0x4a chef, +0x6b second : le second confirme
//!   7 → (0x80)  +0x19 guilde, +0x4a soi, +0x7c réponse
//!   8 ← (0x70)  +0x19 guilde, +0x3a membre, +0x6c nouveau rang (1, 10 chef, 11 second), à toute la guilde
//! ```
//! Les tailles sont celles des structures d'Archonia, sans le pied 0x6B ajouté à l'envoi (FUN_1404a66a0).
//! Comme les boutons d'Archonia (FUN_1408c10f0) : seul le chef nomme ; le membre a le niveau 20, est en
//! ligne et la guilde n'a pas déjà de second ; la passation ne va qu'au second, en ligne.

use super::{rank, send_to, send_to_guild, Guild};
use crate::world::{WorldCtx, WorldState};
use crate::{info, warn};
use archcore_net::Packet;

pub const OP: i64 = 48;

pub mod sub {
    pub const APPOINT: u8 = 2;
    pub const APPOINT_ASK: u8 = 3;
    pub const APPOINT_ANSWER: u8 = 4;
    pub const SUCCESSION: u8 = 5;
    pub const SUCCESSION_ASK: u8 = 6;
    pub const SUCCESSION_ANSWER: u8 = 7;
    pub const RANK_UPDATE: u8 = 8;
}

/// Niveau exigé pour être nommé second (FUN_1408c10f0).
pub const APPOINT_LEVEL: i64 = 20;

const SUB: usize = 0x18;

/// Paquet AgpmGuild de `size` octets (pied exclu) : en-tête, drapeaux 1 (seule l'opération est un
/// champ), op 48, sous-opération, puis ce que `fill` écrit ; le pied 0x6B est ajouté.
pub fn raw_packet(sub_op: u8, size: usize, fill: impl FnOnce(&mut [u8])) -> Vec<u8> {
    let mut out = vec![0u8; size];
    out[0] = archcore_net::packet::GUARD_HEAD;
    out[5] = archcore_net::schema::ptype::GUILD;
    out[0x13..0x17].copy_from_slice(&1u32.to_le_bytes());
    out[0x17] = OP as u8;
    out[SUB] = sub_op;
    fill(&mut out);
    out.push(archcore_net::packet::GUARD_TAIL);
    let len = out.len() as u32;
    out[1..5].copy_from_slice(&len.to_le_bytes());
    out
}

/// Écrit `s` (CP1252) à `off`, sur `max` octets zéro final compris.
pub fn put_text(buf: &mut [u8], off: usize, max: usize, s: &str) {
    let b = archcore_net::text::encode(s);
    let n = b.len().min(max - 1);
    buf[off..off + n].copy_from_slice(&b[..n]);
}

/// Texte zéro-terminé lu à `off` sur `max` octets au plus.
pub fn get_text(buf: &[u8], off: usize, max: usize) -> String {
    let Some(s) = buf.get(off..(off + max).min(buf.len())) else { return String::new() };
    let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
    archcore_net::text::decode(&s[..end]).trim().to_string()
}

fn get_u32(buf: &[u8], off: usize) -> Option<u32> {
    buf.get(off..off + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

fn ask_appoint(g: &Guild, master: &str, member: &str, r: i64) -> Vec<u8> {
    raw_packet(sub::APPOINT_ASK, 0xa0, |b| {
        put_text(b, 0x19, 49, &g.name);
        put_text(b, 0x4a, 33, master);
        put_text(b, 0x6b, 49, member);
        b[0x9c..0xa0].copy_from_slice(&(r as u32).to_le_bytes());
    })
}

fn ask_succession(g: &Guild, master: &str, member: &str) -> Vec<u8> {
    raw_packet(sub::SUCCESSION_ASK, 0x9c, |b| {
        put_text(b, 0x19, 49, &g.name);
        put_text(b, 0x4a, 33, master);
        put_text(b, 0x6b, 49, member);
    })
}

pub fn rank_update(g: &Guild, member: &str, r: i64) -> Vec<u8> {
    raw_packet(sub::RANK_UPDATE, 0x70, |b| {
        put_text(b, 0x19, 33, &g.name);
        put_text(b, 0x3a, 49, member);
        b[0x6c..0x70].copy_from_slice(&(r as u32).to_le_bytes());
    })
}

pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let raw = &p.raw;
    let Some(&sub_op) = raw.get(SUB) else { return Ok(()) };
    let Some((cid, gid, my_rank)) = super::mine(st, sid) else { return Ok(()) };
    let me = st.players[&sid].ch.name.clone();
    match sub_op {
        sub::APPOINT if raw.len() >= 0x80 => {
            if !get_text(raw, 0x19, 49).eq_ignore_ascii_case(&me) || my_rank != rank::MASTER {
                return Ok(());
            }
            let target = get_text(raw, 0x4a, 49);
            let wanted = get_u32(raw, 0x7c).unwrap_or(0) as i64;
            appoint(ctx, st, gid, &me, &target, wanted).await
        }
        sub::APPOINT_ANSWER if raw.len() >= 0x84 => {
            let yes = get_u32(raw, 0x80) == Some(1);
            let g = st.guilds.by_id.get_mut(&gid).unwrap();
            if g.appoint != Some(cid) {
                return Ok(());
            }
            g.appoint = None;
            if !yes || g.members.values().any(|m| m.rank == rank::SUBMASTER) || my_rank != rank::NORMAL {
                info!("[guilde] {me} refuse d'être second de {}", g.name);
                return Ok(());
            }
            set_rank(ctx, st, gid, cid, rank::SUBMASTER).await
        }
        sub::SUCCESSION if raw.len() >= 0x7b => {
            if !get_text(raw, 0x19, 49).eq_ignore_ascii_case(&me) || my_rank != rank::MASTER {
                return Ok(());
            }
            let target = get_text(raw, 0x4a, 49);
            let g = &st.guilds.by_id[&gid];
            let Some(m) = g.member_named(&target).filter(|m| m.rank == rank::SUBMASTER).cloned() else { return Ok(()) };
            let Some(tsid) = st.by_cid.get(&m.cid).copied() else { return Ok(()) };
            if g.in_battle() {
                return Ok(());
            }
            send_to(st, tsid, ask_succession(g, &me, &m.name));
            st.guilds.by_id.get_mut(&gid).unwrap().succession = Some(m.cid);
            info!("[guilde] {me} propose à {} de diriger {}", m.name, st.guilds.by_id[&gid].name);
            Ok(())
        }
        sub::SUCCESSION_ANSWER if raw.len() >= 0x80 => {
            let yes = get_u32(raw, 0x7c) == Some(1);
            let g = st.guilds.by_id.get_mut(&gid).unwrap();
            if g.succession != Some(cid) {
                return Ok(());
            }
            g.succession = None;
            let Some(old) = g.master().map(|m| m.cid) else { return Ok(()) };
            if !yes || my_rank != rank::SUBMASTER {
                info!("[guilde] {me} refuse de diriger {}", g.name);
                return Ok(());
            }
            set_rank(ctx, st, gid, old, rank::NORMAL).await?;
            set_rank(ctx, st, gid, cid, rank::MASTER).await
        }
        other => {
            warn!("[guilde] #{sid} op 48 sous-op {other} ({} o) ignorée", raw.len());
            Ok(())
        }
    }
}

/// Sous-op 2 : nomination (demande au membre) ou retrait du rang de second (aussitôt).
async fn appoint(ctx: &WorldCtx, st: &mut WorldState, gid: u32, master: &str, target: &str, wanted: i64) -> anyhow::Result<()> {
    let g = &st.guilds.by_id[&gid];
    let Some(m) = g.member_named(target).cloned() else { return Ok(()) };
    match wanted {
        rank::NORMAL if m.rank == rank::SUBMASTER => set_rank(ctx, st, gid, m.cid, rank::NORMAL).await,
        rank::SUBMASTER => {
            let Some(tsid) = st.by_cid.get(&m.cid).copied() else { return Ok(()) };
            let level = st.players.get(&tsid).map(|p| p.ch.level as i64).unwrap_or(m.level);
            if m.rank != rank::NORMAL || level < APPOINT_LEVEL || g.members.values().any(|x| x.rank == rank::SUBMASTER) {
                info!("[guilde] {} ne peut pas être nommé second de {} (rang {}, niveau {level})", m.name, g.name, m.rank);
                return Ok(());
            }
            send_to(st, tsid, ask_appoint(g, master, &m.name, rank::SUBMASTER));
            st.guilds.by_id.get_mut(&gid).unwrap().appoint = Some(m.cid);
            info!("[guilde] {master} propose à {} d'être second de {}", m.name, st.guilds.by_id[&gid].name);
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Change le rang d'un membre (base, mémoire) et l'annonce à la guilde (sous-op 8).
pub async fn set_rank(ctx: &WorldCtx, st: &mut WorldState, gid: u32, cid: u32, r: i64) -> anyhow::Result<()> {
    ctx.db.characters.exec("UPDATE guild_member SET `rank` = ? WHERE character_id = ?", (r, cid)).await?;
    let g = st.guilds.by_id.get_mut(&gid).unwrap();
    let Some(m) = g.members.get_mut(&cid) else { return Ok(()) };
    m.rank = r;
    let name = m.name.clone();
    let g = &st.guilds.by_id[&gid];
    send_to_guild(st, g, None, &rank_update(g, &name, r));
    info!("[guilde] {name} passe au rang {r} dans {}", g.name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paquets_bruts() {
        let g = Guild { id: 1, name: "Sylvania".into(), ..Default::default() };
        let p = rank_update(&g, "SimA", rank::SUBMASTER);
        assert_eq!((p.len(), p[0], p[5], p[0x17], p[SUB], *p.last().unwrap()), (0x71, 0xd6, 0x15, 48, sub::RANK_UPDATE, 0x6b));
        assert_eq!(u32::from_le_bytes(p[1..5].try_into().unwrap()), 0x71);
        assert_eq!((get_text(&p, 0x19, 33).as_str(), get_text(&p, 0x3a, 49).as_str(), get_u32(&p, 0x6c)), ("Sylvania", "SimA", Some(11)));
        let decoded = archcore_net::decode_packet(&archcore_net::schema::GUILD, &p).unwrap();
        assert_eq!(decoded.int(0), Some(OP));
        assert_eq!(ask_appoint(&g, "GmTest", "SimA", 11).len(), 0xa1);
        assert_eq!(ask_succession(&g, "GmTest", "SimA").len(), 0x9d);
    }
}
