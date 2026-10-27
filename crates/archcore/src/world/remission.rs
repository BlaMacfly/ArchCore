//! Rémission des points de meurtrier auprès des pardonneurs (AgpmRemission, 0x2A ; fonction de PNJ 31).
//!
//! Op 0 : le client demande la fenêtre au pardonneur (mini EVENT_BASE) ; op 1 : accord, qui l'ouvre
//! (`UI_Remission`). Op 2 : le client demande la rémission (mini {type 4 = lettres de pardon}) ; le serveur répond
//! par l'op 2 et le mini {type, résultat}. Le client (`FUN_1402772f0`) exige des points et 10 lettres de pardon
//! (« Letter of Pardon » TID 3311, « Letter of Great Pardon » TID 1836, objets de genre « OtherType 4 ») ; il annonce
//! 2 points retirés (`PvP.RemissionPoints`). Pas de rémission avant la fin du délai de 2 heures qui suit le dernier
//! meurtre (« REMIT_MSG_NOT_TP » : réponse de type 1, résultat 0).
use super::items::{item_count, item_remove};
use super::npc_events::{npc_distance, resolve, MAX_NPC_DISTANCE};
use super::{WorldCtx, WorldState};
use crate::character::unix_now;
use crate::{info, warn};
use archcore_net::schema::{self, mini};
use archcore_net::{build_mini, decode_mini, Fields, Packet};

pub mod op {
    pub const REQUEST: i64 = 0;
    pub const GRANT: i64 = 1;
    pub const REMIT: i64 = 2;
}

/// Pardonneur (AgpmEventRemission).
pub const FUNCTION: i64 = 31;
/// Rémission par lettres de pardon, la seule que propose la fenêtre.
pub const TYPE_LETTERS: i64 = 4;
/// Réponse « délai pas écoulé » (type 1).
const TYPE_TIME: i64 = 1;
const RESULT_SUCCESS: i64 = 1;
const RESULT_NO_POINT: i64 = 3;
const RESULT_NOT_ENOUGH_LETTERS: i64 = 7;
/// Lettres exigées, et leurs modèles.
pub const LETTERS: i64 = 10;
pub const LETTER_TIDS: [u32; 2] = [3311, 1836];

fn reply(st: &WorldState, sid: u32, kind: i64, result: i64) {
    let Some(p) = st.players.get(&sid) else { return };
    let m = &mini::REMISSION;
    let s = &schema::REMISSION;
    p.send(s, Fields::new(s).with(0, op::REMIT).with(1, p.ch.id).with(2, build_mini(m, &Fields::new(m).with(0, kind).with(1, result))));
}

pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    match p.int(0) {
        Some(op::REQUEST) => {
            let base = p.bytes(2).and_then(|b| decode_mini(&mini::EVENT_BASE, b).ok()).unwrap_or_else(|| Fields::new(&mini::EVENT_BASE));
            let Some(player) = st.players.get_mut(&sid) else { return Ok(()) };
            let Some((npc, _)) = resolve(&ctx.npcs, &base) else {
                warn!("[rémission] #{sid} pardonneur inconnu");
                return Ok(());
            };
            if !npc.events.iter().any(|e| e.function == FUNCTION) || npc_distance(npc, player.ch.pos_x, player.ch.pos_z) > MAX_NPC_DISTANCE {
                warn!("[rémission] #{sid} {} n'est pas un pardonneur à portée", npc.name);
                return Ok(());
            }
            player.remission_keeper = Some(npc.cid);
            let s = &schema::REMISSION;
            player.send(s, Fields::new(s).with(0, op::GRANT).with(1, player.ch.id).with(2, build_mini(&mini::EVENT_BASE, &base)));
        }
        Some(op::REMIT) => remit(ctx, st, sid).await?,
        _ => {}
    }
    Ok(())
}

async fn remit(ctx: &WorldCtx, st: &mut WorldState, sid: u32) -> anyhow::Result<()> {
    let Some(p) = st.players.get(&sid) else { return Ok(()) };
    let near = p.remission_keeper.and_then(|cid| ctx.npcs.get(cid)).is_some_and(|n| npc_distance(n, p.ch.pos_x, p.ch.pos_z) <= MAX_NPC_DISTANCE);
    if !near {
        warn!("[rémission] #{sid} sans pardonneur à portée");
        return Ok(());
    }
    if p.ch.murderer == 0 {
        return Ok(reply(st, sid, TYPE_LETTERS, RESULT_NO_POINT));
    }
    if p.ch.murderer_until > unix_now() {
        info!("[rémission] {} : délai de 2 heures pas écoulé", p.ch.name);
        return Ok(reply(st, sid, TYPE_TIME, 0));
    }
    let letters: Vec<(u32, i64)> = p.inventory.items.values().filter(|i| LETTER_TIDS.contains(&i.tid)).map(|i| (i.iid, i.count)).collect();
    if letters.iter().map(|(_, n)| n).sum::<i64>() < LETTERS {
        return Ok(reply(st, sid, TYPE_LETTERS, RESULT_NOT_ENOUGH_LETTERS));
    }
    let cid = p.ch.id;
    let mut need = LETTERS;
    for (iid, count) in letters {
        if need <= 0 {
            break;
        }
        let n = count.min(need);
        need -= n;
        let p = st.players.get_mut(&sid).unwrap();
        match p.inventory.consume(&ctx.db, iid, n).await? {
            Some(left) => p.send_raw(item_count(&left, cid)),
            None => p.send_raw(item_remove(iid, Some(cid))),
        }
    }
    let p = st.players.get_mut(&sid).unwrap();
    let before = p.ch.murderer;
    p.ch.murderer = before.saturating_sub(ctx.config.world.pvp_remission_points);
    p.factors.status[11] = p.ch.murderer as i32;
    info!("[rémission] {} : {before} → {} point(s) de meurtrier", p.ch.name, p.ch.murderer);
    ctx.db.characters.exec("UPDATE characters SET murderer_points = ? WHERE id = ?", (p.ch.murderer, cid)).await?;
    super::skills::send_points(p);
    reply(st, sid, TYPE_LETTERS, RESULT_SUCCESS);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reponse_de_remission() {
        let m = &mini::REMISSION;
        let f = decode_mini(m, archcore_net::packet::mini_payload(&build_mini(m, &Fields::new(m).with(0, TYPE_LETTERS).with(1, RESULT_SUCCESS)))).unwrap();
        assert_eq!((f.int(0), f.int(1)), (Some(4), Some(1)));
    }
}
