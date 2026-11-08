//! Costumes par ensembles (usage 31, `avatarset.ini`).
//!
//! ```text
//!   Table : sections [n] : tid0 = l'objet de l'inventaire Cash, tid1… = les pièces (« Part » 16 à 21, costume de la tête, du
//!     torse, des mains, des jambes, des pieds), 2 163 ensembles dans la table de l'épisode 8. `avatarsetsummoner.ini` n'existe
//!     que dans le client et ses objets sont absents de l'épisode 8 : pas lu.
//!   Objet utilisé (AGPMITEM op 3) : sa compétence s'il en a une et sa vie (`consumables`, `lifetime.rs`), puis chaque pièce
//!     portée : AGPMITEM op 0, statut 3 (équipé), au joueur et à ceux qui le voient, avec l'équipement de sa vue ensuite. Le
//!     client les dessine par-dessus l'armure. Les pièces ne comptent pas dans les facteurs, n'existent pas en base (ID de
//!     `IID_BASE` en montant) et ne durent que la session, comme l'effet de l'objet.
//!   Refus (AGPMITEM op 5) : un autre costume porté, une partie de l'ensemble déjà occupée par un objet porté.
//!   Retrait : l'objet réutilisé, AGPMITEM op 23, ou sa fin de vie : pièces retirées (op 1), effets retirés, op 26. Sous
//!     métamorphose, refusé, comme le client (`CannotUnEquipAvatarOnTransform`).
//! ```

use super::consumables::{failed_packet, use_skill_item};
use super::items::{item_add_of, item_remove, item_status, InventoryItem, DURABILITY_FULL};
use super::{combat, WorldCtx, WorldState};
use crate::{debug, info};
use std::collections::HashMap;
use std::time::Instant;

/// Identifiants des pièces, hors de ceux de `character_items`.
pub const IID_BASE: u32 = 0x7E00_0000;

/// Lit avatarset.ini : objet (`tid0`) → pièces (`tid1`…), dans l'ordre.
pub fn parse(text: &str) -> HashMap<u32, Vec<u32>> {
    let mut out: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut current: Vec<(u32, u32)> = Vec::new();
    let mut flush = |list: &mut Vec<(u32, u32)>| {
        list.sort_unstable();
        if let Some((0, head)) = list.first().copied() {
            if list.len() > 1 {
                out.insert(head, list[1..].iter().map(|(_, t)| *t).collect());
            }
        }
        list.clear();
    };
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            flush(&mut current);
        } else if let Some((key, value)) = line.split_once('=') {
            let (Some(i), Ok(tid)) = (key.trim().strip_prefix("tid").and_then(|n| n.parse::<u32>().ok()), value.trim().parse::<u32>()) else { continue };
            if tid != 0 {
                current.push((i, tid));
            }
        }
    }
    flush(&mut current);
    out
}

/// Parties des pièces d'un ensemble ; `None` si l'une n'est pas un objet qui se porte.
fn parts(ctx: &WorldCtx, set: &[u32]) -> Option<Vec<(u32, i64)>> {
    set.iter().map(|t| Some((*t, ctx.catalog.get(*t)?.equip.as_ref()?.part))).collect()
}

/// Objet de costume utilisé (AGPMITEM op 3).
pub async fn use_item(ctx: &WorldCtx, st: &mut WorldState, sid: u32, item: &InventoryItem, now: Instant) -> anyhow::Result<()> {
    let Some(p) = st.players.get(&sid) else { return Ok(()) };
    let cid = p.ch.id;
    if p.inventory.avatar_item == Some(item.iid) {
        stop(ctx, st, sid, item.iid);
        return Ok(());
    }
    let Some(pieces) = ctx.catalog.avatars.get(&item.tid).and_then(|set| parts(ctx, set)) else { return Ok(()) };
    let taken = pieces.iter().any(|(_, part)| p.inventory.equipment.contains_key(part));
    if !p.inventory.cash.contains_key(&item.iid) || p.inventory.avatar_item.is_some() || taken {
        debug!("[objet] {} : costume {} refusé (autre costume ou partie occupée)", p.ch.name, item.tid);
        p.send_raw(failed_packet(item.tid, cid));
        return Ok(());
    }
    // Compétence de l'objet et sa vie ; refusée (recharge, effet plus fort), pas de pièces.
    match ctx.catalog.uses.get(&item.tid).filter(|u| !u.skills.is_empty()) {
        Some(u) => {
            use_skill_item(ctx, st, sid, item, u, now).await?;
            let Some(p) = st.players.get(&sid) else { return Ok(()) };
            if !p.uses.lit.contains_key(&item.iid) && !p.inventory.in_use.contains(&item.iid) {
                return Ok(());
            }
        }
        None => {
            super::lifetime::on_use(ctx, st, sid, item.iid, item.tid).await?;
        }
    }
    put_on(st, sid, item.iid, &pieces);
    Ok(())
}

/// Pièces portées, annoncées au joueur et à ceux qui le voient.
fn put_on(st: &mut WorldState, sid: u32, iid: u32, pieces: &[(u32, i64)]) {
    let mut worn = Vec::new();
    for (tid, part) in pieces {
        st.next_avatar_iid += 1;
        worn.push(InventoryItem { iid: IID_BASE + st.next_avatar_iid, tid: *tid, count: 1, grid: [*part, 0, 0], conv: Default::default(), options: Vec::new(), durability: DURABILITY_FULL });
    }
    let Some(p) = st.players.get_mut(&sid) else { return };
    let cid = p.ch.id;
    p.inventory.avatar_item = Some(iid);
    p.inventory.avatar = worn.clone();
    info!("[objet] {} porte un costume ({} pièce(s))", p.ch.name, worn.len());
    for piece in &worn {
        combat::send_around(st, sid, &item_add_of(piece, cid, item_status::EQUIP, None));
    }
}

/// Pièces de l'objet `iid` retirées, s'il est le costume porté.
pub fn take_off(st: &mut WorldState, sid: u32, iid: u32) {
    let Some(p) = st.players.get_mut(&sid).filter(|p| p.inventory.avatar_item == Some(iid)) else { return };
    let cid = p.ch.id;
    p.inventory.avatar_item = None;
    let worn = std::mem::take(&mut p.inventory.avatar);
    for piece in &worn {
        combat::send_around(st, sid, &item_remove(piece.iid, Some(cid)));
    }
}

/// Costume retiré à la demande (objet réutilisé, AGPMITEM op 23). Rend vrai si `iid` est le costume porté.
pub fn stop(ctx: &WorldCtx, st: &mut WorldState, sid: u32, iid: u32) -> bool {
    let Some(p) = st.players.get(&sid).filter(|p| p.inventory.avatar_item == Some(iid)) else { return false };
    let cid = p.ch.id;
    // Sous métamorphose : refusé, comme le client.
    if p.form.morph.is_some() {
        let tid = p.inventory.cash.get(&iid).map(|i| i.tid).unwrap_or(0);
        p.send_raw(failed_packet(tid, cid));
        return true;
    }
    take_off(st, sid, iid);
    let Some(p) = st.players.get_mut(&sid) else { return true };
    let tids = p.uses.lit.remove(&iid).unwrap_or_default();
    let gone: Vec<super::effects::Effect> = p.skills.effects.iter().filter(|e| e.ends_at.is_some() && tids.contains(&e.tid)).cloned().collect();
    p.skills.effects.retain(|e| !(e.ends_at.is_some() && tids.contains(&e.tid)));
    p.inventory.in_use.remove(&iid);
    p.send_raw(super::lifetime::use_time_packet(&p.inventory, iid));
    info!("[objet] {} retire son costume", p.ch.name);
    for e in &gone {
        combat::send_around(st, sid, &super::effects::remove_packet(cid, e));
    }
    if !gone.is_empty() {
        super::equipment::refresh_factors(ctx, st, sid);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_des_ensembles() {
        let t = parse("[1]\r\nname=robe\r\ntid0=6446\r\ntid1=5451\r\ntid2=5455\r\n[2]\r\nname=x\r\ntid0=6445\r\ntid2=7\r\ntid1=5\r\n[3]\r\ntid0=9\r\n[4]\r\nname=sans objet\r\ntid1=4\r\n");
        assert_eq!(t[&6446], vec![5451, 5455]);
        assert_eq!(t[&6445], vec![5, 7], "dans l'ordre des numéros");
        assert_eq!(t.len(), 2, "ensemble sans pièce ou sans objet : écarté");
    }
}
