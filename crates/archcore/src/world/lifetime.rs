//! Objets à durée : objets de la boutique (« Cash ») à temps de jeu (« RemainTime ») ou à date d'expiration
//! (« ExpireTime ») d'itemdatatable, en minutes.
//!
//! ```text
//!   Objets : « Cash » 2 (temps de jeu) avec « RemainTime », ou 3 (temps réel) avec « ExpireTime », « CashItemUseType » 2
//!     (usage continu) : potions de vitesse 30 min, talismans <1/3/14 Days>, costumes…
//!   Première utilisation (AGPMITEM op 3) : l'objet passe « en cours d'utilisation » et n'est plus consommé à l'usage ;
//!     temps de jeu : reste = RemainTime × 60 000 ms s'il est vide ; temps réel : date = maintenant + ExpireTime × 60 s
//!     si elle est vide. Enregistrés (`character_items.remain_ms`, `expire_at`), envoyés par AGPMITEM op 26 (mini Cash au
//!     champ 23 : 0 en cours d'utilisation, 1 reste en ms, 2 date en secondes Unix) et avec l'objet à l'entrée en jeu.
//!     L'effet posé dure ce qui reste à l'objet (si sa compétence n'a pas de durée).
//!   Décompte (toutes les 10 s) : temps réel, l'objet expire quand l'horloge passe la date, même hors ligne (la date est
//!     absolue ; vu à l'entrée en jeu au premier décompte) ; temps de jeu, le reste ne diminue que si l'objet est en
//!     cours d'utilisation et le joueur en jeu, op 26 et enregistrement chaque minute.
//!   Expiration : « en cours d'utilisation » à 0, effets de la compétence de l'objet retirés, op 26, message système 5
//!     (« The effect of the %s item has disappeared », nom au champ 4), puis l'objet est consommé (op 2 ou op 1) ; un
//!     exemplaire restant de la pile repart de zéro.
//!   Restrictions : un objet en cours d'utilisation ou à date d'expiration ne s'échange pas.
//! ```
//! Règle proposée (le client affiche, sans minuterie : il ne retire rien lui-même) ; `Rules.ItemLifetime` (1). Sans objet :
//! la pause (op 25, message 8 ; le client affiche « Toggled Off ») : ni colonne ni texte ne dit quand un objet s'y met ; et
//! l'endurance du familier (op 29) : le client décode le paquet sans rien en ranger, sa jauge n'a pas de valeurs.

use super::items::{cash_mini_full, item_remove, Inventory};
use super::{WorldCtx, WorldState};
use crate::info;
use archcore_net::schema;
use archcore_net::{build_packet, Fields};
use std::time::{Duration, Instant};

/// Cadence du décompte.
pub const STEP: Duration = Duration::from_secs(10);
/// Message système (AgpmSystemMessage, code au champ 1) : effet de l'objet terminé.
pub const MSG_END_USING_ITEM: i64 = 5;

/// Durée de vie d'un modèle d'objet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifespan {
    /// Temps de jeu, en minutes.
    PlayTime(i64),
    /// Temps réel depuis la première utilisation, en minutes.
    RealTime(i64),
}

impl Lifespan {
    /// Lit une ligne d'itemdatatable : « Cash » 2 et « RemainTime », ou 3 et « ExpireTime », avec « CashItemUseType » 2.
    pub fn from_raw(raw: &serde_json::Value) -> Option<Self> {
        let num = |k: &str| raw.get(k).and_then(|v| v.as_str()).and_then(|t| t.trim().parse::<f64>().ok()).map(|v| v as i64).unwrap_or(0);
        let cash = num("Cash");
        let cash = if cash >= 100 { cash - 100 } else { cash };
        if num("CashItemUseType") != 2 {
            return None;
        }
        match cash {
            2 if num("RemainTime") > 0 => Some(Lifespan::PlayTime(num("RemainTime"))),
            3 if num("ExpireTime") > 0 => Some(Lifespan::RealTime(num("ExpireTime"))),
            _ => None,
        }
    }
}

/// Vie commencée d'un exemplaire : temps de jeu restant (ms) et date d'expiration (secondes Unix) ; 0 : pas commencé.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Life {
    pub remain_ms: i64,
    pub expire_at: i64,
}

impl Life {
    /// Vie commencée à la première utilisation, `now` en secondes Unix (ce qui l'est déjà reste).
    pub fn started(self, span: Lifespan, now: i64) -> Life {
        match span {
            Lifespan::PlayTime(min) if self.remain_ms == 0 => Life { remain_ms: min * 60_000, ..self },
            Lifespan::RealTime(min) if self.expire_at == 0 => Life { expire_at: now + min * 60, ..self },
            _ => self,
        }
    }

    /// Temps restant en ms (temps de jeu, ou jusqu'à la date), 0 au moins.
    pub fn left_ms(&self, span: Lifespan, now: i64) -> u64 {
        match span {
            Lifespan::PlayTime(_) => self.remain_ms.max(0) as u64,
            Lifespan::RealTime(_) => ((self.expire_at - now).max(0) * 1000) as u64,
        }
    }
}

/// Secondes Unix.
pub fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Mini Cash d'un objet de l'inventaire Cash : en cours d'utilisation, reste, date.
pub fn cash_mini_of(inv: &Inventory, iid: u32) -> Vec<u8> {
    let life = inv.life.get(&iid).copied().unwrap_or_default();
    cash_mini_full(i64::from(inv.in_use.contains(&iid)), life.remain_ms, life.expire_at)
}

/// AGPMITEM op 26 : mini Cash complet de l'objet `iid`.
pub fn use_time_packet(inv: &Inventory, iid: u32) -> Vec<u8> {
    let s = &schema::ITEM;
    build_packet(s, &Fields::new(s).with(0, super::items::item_op::UPDATE_USE_TIME).with(2, iid).with(4, inv.character_id).with(23, cash_mini_of(inv, iid)))
}

/// AgpmSystemMessage op 0 : code et texte (champ 4, le %s du message).
pub fn system_message(code: i64, text: &str) -> Vec<u8> {
    let s = &schema::SYSTEM_MESSAGE;
    build_packet(s, &Fields::new(s).with(0, 0).with(1, code).with(4, text))
}

/// Objet à durée utilisé (AGPMITEM op 3) : vie commencée, enregistrée et envoyée. Rend le temps qui lui reste (ms) : l'objet
/// n'est alors pas consommé ; `None` pour un objet ordinaire.
pub async fn on_use(ctx: &WorldCtx, st: &mut WorldState, sid: u32, iid: u32, tid: u32) -> anyhow::Result<Option<u64>> {
    let Some(span) = ctx.catalog.lifespans.get(&tid).copied().filter(|_| crate::config::rules().item_lifetime) else { return Ok(None) };
    let Some(p) = st.players.get_mut(&sid) else { return Ok(None) };
    let now = unix_now();
    let before = p.inventory.life.get(&iid).copied().unwrap_or_default();
    let life = before.started(span, now);
    p.inventory.life.insert(iid, life);
    p.inventory.in_use.insert(iid);
    if life != before {
        ctx.db.characters.exec("UPDATE character_items SET remain_ms = ?, expire_at = ? WHERE id = ?", (life.remain_ms, life.expire_at, iid)).await?;
        info!("[objet] {} commence la vie de l'objet {tid} ({iid}) : {:?}", p.ch.name, life);
    }
    p.send_raw(use_time_packet(&p.inventory, iid));
    Ok(Some(life.left_ms(span, now)))
}

/// Un objet à durée ne s'échange pas pendant qu'il sert ou une fois sa date fixée.
pub fn locked(inv: &Inventory, iid: u32) -> bool {
    inv.in_use.contains(&iid) && inv.life.contains_key(&iid) || inv.life.get(&iid).is_some_and(|l| l.expire_at != 0)
}

/// Décompte des objets à durée des joueurs en jeu (toutes les `STEP`).
pub async fn step(ctx: &WorldCtx, st: &mut WorldState, now: Instant) -> anyhow::Result<()> {
    if st.lifetime_at.is_some_and(|t| now < t) || !crate::config::rules().item_lifetime {
        return Ok(());
    }
    st.lifetime_at = Some(now + STEP);
    let unix = unix_now();
    for sid in st.sids() {
        let Some(p) = st.players.get_mut(&sid).filter(|p| p.in_game) else { continue };
        let mut expired = Vec::new();
        let mut saved = Vec::new();
        let iids: Vec<u32> = p.inventory.life.keys().copied().collect();
        for iid in iids {
            let Some(tid) = p.inventory.get_usable(iid).map(|i| i.tid) else { continue };
            let Some(span) = ctx.catalog.lifespans.get(&tid).copied() else { continue };
            let in_use = p.inventory.in_use.contains(&iid);
            let life = p.inventory.life.get_mut(&iid).unwrap();
            match span {
                Lifespan::RealTime(_) if life.expire_at > 0 && unix >= life.expire_at => expired.push(iid),
                Lifespan::PlayTime(_) if in_use && life.remain_ms > 0 => {
                    let before = life.remain_ms / 60_000;
                    life.remain_ms = (life.remain_ms - STEP.as_millis() as i64).max(0);
                    if life.remain_ms == 0 {
                        expired.push(iid);
                    } else if life.remain_ms / 60_000 != before {
                        saved.push((iid, life.remain_ms));
                    }
                }
                _ => {}
            }
        }
        for (iid, remain) in saved {
            p.send_raw(use_time_packet(&p.inventory, iid));
            ctx.db.characters.exec("UPDATE character_items SET remain_ms = ? WHERE id = ?", (remain, iid)).await?;
        }
        for iid in expired {
            expire(ctx, st, sid, iid).await?;
        }
    }
    Ok(())
}

/// Fin de vie d'un exemplaire : effets retirés, op 26, message système 5, un exemplaire consommé.
pub async fn expire(ctx: &WorldCtx, st: &mut WorldState, sid: u32, iid: u32) -> anyhow::Result<()> {
    // Costume : ses pièces tombent avec lui (avatar.rs).
    super::avatar::take_off(st, sid, iid);
    let Some(p) = st.players.get_mut(&sid) else { return Ok(()) };
    let Some(item) = p.inventory.get_usable(iid).cloned() else { return Ok(()) };
    let cid = p.ch.id;
    let name = ctx.catalog.get(item.tid).map(|d| d.name.clone()).unwrap_or_else(|| format!("objet {}", item.tid));
    // Effets de la compétence de l'objet (et objet allumé éteint).
    let tids: Vec<u32> = ctx.catalog.uses.get(&item.tid).map(|u| u.skills.iter().map(|(t, _)| *t).collect()).unwrap_or_default();
    p.uses.lit.remove(&iid);
    p.inventory.in_use.remove(&iid);
    p.inventory.life.insert(iid, Life::default());
    let gone: Vec<super::effects::Effect> = p.skills.effects.iter().filter(|e| e.ends_at.is_some() && tids.contains(&e.tid)).cloned().collect();
    p.skills.effects.retain(|e| !(e.ends_at.is_some() && tids.contains(&e.tid)));
    p.send_raw(use_time_packet(&p.inventory, iid));
    p.send_raw(system_message(MSG_END_USING_ITEM, &name));
    p.inventory.life.remove(&iid);
    match p.inventory.consume(&ctx.db, iid, 1).await? {
        Some(left) => {
            p.send_raw(p.inventory.count_packet(&left, cid));
            ctx.db.characters.exec("UPDATE character_items SET remain_ms = 0, expire_at = 0 WHERE id = ?", (iid,)).await?;
        }
        None => p.send_raw(item_remove(iid, Some(cid))),
    }
    for e in &gone {
        super::combat::send_around(st, sid, &super::effects::remove_packet(cid, e));
    }
    if !gone.is_empty() {
        super::equipment::refresh_factors(ctx, st, sid);
    }
    info!("[objet] {} : l'objet {name} ({iid}) arrive en fin de vie", st.players.get(&sid).map(|p| p.ch.name.as_str()).unwrap_or("?"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vie_des_objets() {
        let raw = |cash: &str, kind: &str, remain: &str, expire: &str| {
            serde_json::json!({ "Cash": cash, "CashItemUseType": kind, "RemainTime": remain, "ExpireTime": expire })
        };
        assert_eq!(Lifespan::from_raw(&raw("2", "2", "30", "")), Some(Lifespan::PlayTime(30)), "potion de vitesse 30 min");
        assert_eq!(Lifespan::from_raw(&raw("3", "2", "", "1440")), Some(Lifespan::RealTime(1440)), "talisman d'un jour");
        assert_eq!(Lifespan::from_raw(&raw("1", "1", "", "")), None, "consommé à l'usage");
        assert_eq!(Lifespan::from_raw(&raw("", "", "240000", "")), None, "potion de métamorphose : durée d'effet");
        assert_eq!(Lifespan::from_raw(&raw("103", "2", "", "1440")), Some(Lifespan::RealTime(1440)));
        // Première utilisation : reste en ms, date en secondes ; une vie commencée ne change pas.
        let play = Life::default().started(Lifespan::PlayTime(30), 1000);
        assert_eq!(play, Life { remain_ms: 1_800_000, expire_at: 0 });
        assert_eq!(Life { remain_ms: 5000, expire_at: 0 }.started(Lifespan::PlayTime(30), 1000).remain_ms, 5000);
        let real = Life::default().started(Lifespan::RealTime(1440), 1000);
        assert_eq!(real.expire_at, 1000 + 86_400);
        assert_eq!(real.left_ms(Lifespan::RealTime(1440), 1000 + 86_000), 400_000);
        assert_eq!(real.left_ms(Lifespan::RealTime(1440), 2_000_000), 0);
        // Mini Cash : en cours, reste, date.
        let mut inv = Inventory::default();
        inv.life.insert(5, real);
        inv.in_use.insert(5);
        let pkt = archcore_net::decode_packet(&schema::ITEM, &use_time_packet(&inv, 5)).unwrap();
        assert_eq!((pkt.int(0), pkt.int(2)), (Some(super::super::items::item_op::UPDATE_USE_TIME), Some(5)));
        let m = archcore_net::decode_mini(&archcore_net::schema::mini::ITEM_CASH, pkt.bytes(23).unwrap()).unwrap();
        assert_eq!((m.int(0), m.int(1), m.int(2)), (Some(1), Some(0), Some(87_400)));
        assert!(locked(&inv, 5));
        inv.life.insert(6, play);
        assert!(!locked(&inv, 6), "temps de jeu hors usage : échangeable");
    }
}
