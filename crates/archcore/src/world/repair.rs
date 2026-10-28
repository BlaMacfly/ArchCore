//! Usure des objets et réparation chez les PNJ (fonction 17 : forgerons, éleveurs).
//!
//! La durabilité maximale d'un objet est la colonne « DUR » d'itemdatatable.txt ; la durabilité actuelle est
//! enregistrée dans `character_items.durability` (-1 : pleine) et envoyée au client dans le facteur « objet »
//! (AGPMITEM champ 10, sous-valeur 0), avec l'objet quand elle est entamée et à chaque changement (op 2 sans statut).
//! Le client n'affiche que le pourcentage (fenêtre `UI_EquipStatus`, allumée sous 21 %).
//!
//! Usure (règles d'ArchCore : le client n'en dit rien, réglages `Item.WeaponWearChance`, `Item.ArmorWearChance`) :
//! chaque coup qui blesse, compétences comprises, use d'un point l'arme qui le porte avec la première probabilité
//! (arme montée à cheval, sinon main droite, sinon arme de la main gauche) ; chaque coup reçu qui blesse use d'un
//! point, avec la seconde probabilité, une pièce d'armure portée tirée au hasard (corps, tête, mains, jambes, pieds,
//! bouclier). Les coups sont comptés au moment où ils portent (`hit_dealt`, `hit_taken`) puis tirés et enregistrés
//! au tour suivant de la boucle de combat (`step`). Pas d'usure à la mort ; un objet sans « DUR » ou inusable
//! (« @ ») ne s'use jamais. À 0, l'objet reste porté mais ne compte plus dans les facteurs (`equipment::counted`).
//!
//! Réparation, AgpmEventItemRepair (0x34) : op 3 demande d'ouverture (PNJ à portée, fonction 17, pas meurtrier de
//! palier 2) → op 4 ; op 0 demande de réparation des objets de la grille (inventaire seulement), avec le coût que
//! le client a calculé → op 1 et son code (`repair_result`). Le prix est celui du client (`repair_cost`), plus la taxe de
//! la région du joueur sur chaque objet (`tax.rs`, versée à la recette de la région) ; un écart avec le coût envoyé donne
//! le code 4.

use super::items::{item_durability, DURABILITY_FULL};
use super::npc_events::{npc_distance, resolve, MAX_NPC_DISTANCE};
use super::npcs::{event_function, Npc};
use super::{send_system, set_money, WorldCtx, WorldState};
use crate::{debug, info, warn};
use archcore_net::packet::Value;
use archcore_net::schema::{self, mini};
use archcore_net::{build_mini, build_packet, decode_mini, Fields, Packet};
use rand::Rng;

pub mod repair_op {
    pub const REPAIR: i64 = 0;
    pub const RESULT: i64 = 1;
    pub const OPEN: i64 = 3;
    pub const GRANT: i64 = 4;
}

/// Codes de résultat (op 1, champ 5).
pub mod repair_result {
    /// « Repairing succeeded. » (sysstr 317), fenêtre fermée.
    pub const SUCCESS: i64 = 1;
    /// « You do not have enough Gold ».
    pub const NOT_ENOUGH_MONEY: i64 = 2;
    /// « This item cannot be repaired any further ».
    pub const ALREADY_MAX: i64 = 3;
    /// « Repair Failed » (sysstr 318).
    pub const FAILED: i64 = 4;
    /// « This item cannot be repaired » (sysstr 319).
    pub const CANNOT_REPAIR: i64 = 5;
}

/// Parties dont l'armure s'use quand on est frappé : corps, tête, mains, jambes, pieds, main gauche (bouclier).
pub const ARMOR_PARTS: [i64; 6] = [2, 3, 5, 6, 7, 9];

/// Coups pas encore comptés dans l'usure.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Wear {
    pub dealt: u32,
    pub taken: u32,
}

/// Le joueur vient de blesser quelqu'un (coup ou compétence).
pub fn hit_dealt(st: &mut WorldState, sid: u32) {
    if let Some(p) = st.players.get_mut(&sid) {
        p.wear.dealt = p.wear.dealt.saturating_add(1);
    }
}

/// Le joueur vient d'être blessé.
pub fn hit_taken(st: &mut WorldState, sid: u32) {
    if let Some(p) = st.players.get_mut(&sid) {
        p.wear.taken = p.wear.taken.saturating_add(1);
    }
}

/// Prix de réparation d'un objet, comme le client (`FUN_1402b43a0`) : prix au PNJ divisé par la durabilité
/// maximale, multiplié par les points perdus et par 0,5, calcul en flottant puis tronqué, sans la taxe (ajoutée par `quote`).
pub fn repair_cost(npc_price: i64, max: i32, current: i32) -> i64 {
    if max <= 0 || current >= max {
        return 0;
    }
    ((npc_price as f64 / max as f64) * (max - current) as f64 * 0.5) as i64
}

/// Partie de l'arme qui porte les coups : arme montée à cheval, sinon main droite, sinon arme de la main gauche ;
/// seulement si elle s'use et n'est pas déjà à 0.
fn weapon_part(inv: &super::items::Inventory, catalog: &super::items::ItemCatalog) -> Option<i64> {
    use super::equipment::{part, KIND_WEAPON};
    let usable = |at: i64| {
        inv.equipment
            .get(&at)
            .and_then(|i| catalog.get(i.tid)?.equip.as_ref().map(|e| (i, e)))
            .is_some_and(|(i, e)| e.kind == KIND_WEAPON && e.wears() && e.current_durability(i.durability) > 0)
    };
    let order: &[i64] = if super::ride::is_mounted(inv) { &[part::LANCER] } else { &[part::HAND_RIGHT, part::HAND_LEFT] };
    order.iter().copied().find(|at| usable(*at))
}

/// Pièces d'armure portées qui peuvent encore s'user.
fn armor_parts(inv: &super::items::Inventory, catalog: &super::items::ItemCatalog) -> Vec<i64> {
    ARMOR_PARTS
        .iter()
        .copied()
        .filter(|at| {
            inv.equipment
                .get(at)
                .and_then(|i| catalog.get(i.tid)?.equip.as_ref().map(|e| (i, e)))
                .is_some_and(|(i, e)| e.kind != super::equipment::KIND_WEAPON && e.wears() && e.current_durability(i.durability) > 0)
        })
        .collect()
}

/// Parties usées d'un point pour ces coups : un tirage par coup (probabilités en pour cent).
pub fn roll_wear(rng: &mut impl Rng, wear: Wear, weapon_chance: f64, armor_chance: f64, weapon: Option<i64>, armor: &[i64]) -> Vec<i64> {
    let mut out = Vec::new();
    for _ in 0..wear.dealt {
        if let Some(at) = weapon.filter(|_| rng.gen_range(0.0..100.0) < weapon_chance) {
            out.push(at);
        }
    }
    for _ in 0..wear.taken {
        if !armor.is_empty() && rng.gen_range(0.0..100.0) < armor_chance {
            out.push(armor[rng.gen_range(0..armor.len())]);
        }
    }
    out
}

/// Enregistre une durabilité (`DURABILITY_FULL` pour pleine).
async fn save(ctx: &WorldCtx, iid: u32, stored: i32) -> anyhow::Result<()> {
    ctx.db.characters.exec("UPDATE character_items SET durability = ? WHERE id = ?", (stored, iid)).await?;
    Ok(())
}

/// Usure des coups comptés depuis le tour précédent : durabilités baissées, enregistrées et envoyées ; un objet
/// tombé à 0 ne compte plus dans les facteurs, recalculés.
pub async fn step(ctx: &WorldCtx, st: &mut WorldState) -> anyhow::Result<()> {
    let (weapon_chance, armor_chance) = (ctx.config.world.item_weapon_wear_chance, ctx.config.world.item_armor_wear_chance);
    for sid in st.sids() {
        let Some(p) = st.players.get_mut(&sid) else { continue };
        if p.wear == Wear::default() {
            continue;
        }
        let wear = std::mem::take(&mut p.wear);
        if !p.in_game || p.dead {
            continue;
        }
        let (weapon, armor) = (weapon_part(&p.inventory, &ctx.catalog), armor_parts(&p.inventory, &ctx.catalog));
        let parts = roll_wear(&mut rand::thread_rng(), wear, weapon_chance, armor_chance, weapon, &armor);
        let mut broken = Vec::new();
        for at in parts {
            let cid = p.ch.id;
            let Some(item) = p.inventory.equipment.get_mut(&at) else { continue };
            let Some(e) = ctx.catalog.get(item.tid).and_then(|d| d.equip.as_ref()) else { continue };
            let now = e.current_durability(item.durability);
            if now <= 0 {
                continue;
            }
            item.durability = now - 1;
            let (iid, tid, left) = (item.iid, item.tid, item.durability);
            save(ctx, iid, left).await?;
            p.send_raw(item_durability(iid, tid, cid, left));
            debug!("[usure] {} : objet {tid} à {left}/{}", p.ch.name, e.durability);
            if left == 0 {
                broken.push(ctx.catalog.get(tid).map(|d| d.name.clone()).unwrap_or_default());
            }
        }
        if !broken.is_empty() {
            for name in &broken {
                info!("[usure] {} : {name} hors d'usage", st.players[&sid].ch.name);
                send_system(st, sid, &format!("{name} est hors d'usage : faites-le réparer chez un forgeron."));
            }
            super::equipment::refresh_factors(ctx, st, sid);
        }
    }
    Ok(())
}

/// Commande MJ : fixe la durabilité des objets portés qui s'usent (`value` négatif : pleine). Rend le nombre
/// d'objets changés.
pub async fn set_worn_durability(ctx: &WorldCtx, st: &mut WorldState, sid: u32, value: i32) -> anyhow::Result<usize> {
    let Some(p) = st.players.get_mut(&sid) else { return Ok(0) };
    let (cid, sess) = (p.ch.id, p.sess.clone());
    let mut changed = 0;
    for item in p.inventory.equipment.values_mut() {
        let Some(e) = ctx.catalog.get(item.tid).and_then(|d| d.equip.as_ref()).filter(|e| e.wears()) else { continue };
        let stored = if value < 0 || value >= e.durability { DURABILITY_FULL } else { value };
        item.durability = stored;
        save(ctx, item.iid, stored).await?;
        sess.send_raw(item_durability(item.iid, item.tid, cid, e.current_durability(stored)));
        changed += 1;
    }
    super::equipment::refresh_factors(ctx, st, sid);
    Ok(changed)
}

/// Paquet AgpmEventItemRepair.
fn packet(op: i64, base: Option<Vec<u8>>, cid: u32, code: Option<i64>) -> Vec<u8> {
    let s = &schema::EVENT_ITEM_REPAIR;
    build_packet(s, &Fields::new(s).with(0, op).with_opt(1, base).with(2, cid).with_opt(5, code))
}

/// PNJ réparateur visé par l'événement du paquet (champ 1), à portée ; sans événement, le plus proche à portée.
fn repairer<'a>(ctx: &'a WorldCtx, p: &super::Player, base: Option<&Fields>) -> Option<&'a Npc> {
    let repairs = |n: &Npc| n.events.iter().any(|e| e.function == event_function::ITEM_REPAIR);
    let near = |n: &Npc| npc_distance(n, p.ch.pos_x, p.ch.pos_z) <= MAX_NPC_DISTANCE;
    match base {
        Some(b) => resolve(&ctx.npcs, b).map(|(n, _)| n).filter(|n| repairs(n) && near(n)),
        None => ctx.npcs.near(p.ch.pos(), MAX_NPC_DISTANCE).find(|n| repairs(n)),
    }
}

/// AgpmEventItemRepair reçu du client.
pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some(player) = st.players.get(&sid) else { return Ok(()) };
    if !player.in_game || player.dead {
        return Ok(());
    }
    let base = p.bytes(1).and_then(|b| decode_mini(&mini::EVENT_BASE, b).ok());
    let echo = base.as_ref().map(|b| build_mini(&mini::EVENT_BASE, b));
    let cid = player.ch.id;
    let Some(npc) = repairer(ctx, player, base.as_ref()) else {
        warn!("[réparation] #{sid} {} : pas de réparateur à portée", player.ch.name);
        if p.int(0) == Some(repair_op::REPAIR) {
            player.send_raw(packet(repair_op::RESULT, echo, cid, Some(repair_result::FAILED)));
        }
        return Ok(());
    };
    let npc_name = npc.name.clone();
    // Meurtrier de palier 2 : « CharacterPenalty_CantConvert » (pénalité 3) ; refusé avant toute fenêtre.
    if super::pvp::murderer_level(player.ch.murderer) >= 2 {
        info!("[réparation] {} : refusée par {npc_name} (meurtrier)", player.ch.name);
        send_system(st, sid, "Un meurtrier ne peut pas faire réparer ses objets.");
        if p.int(0) == Some(repair_op::REPAIR) {
            st.players[&sid].send_raw(packet(repair_op::RESULT, echo, cid, Some(repair_result::FAILED)));
        }
        return Ok(());
    }
    match p.int(0) {
        Some(repair_op::OPEN) => {
            let base = echo.unwrap_or_else(|| super::npcs::event_base(npc.cid, event_function::ITEM_REPAIR));
            player.send_raw(packet(repair_op::GRANT, Some(base), cid, None));
            debug!("[réparation] {} ouvre la réparation chez {npc_name}", player.ch.name);
        }
        Some(repair_op::REPAIR) => {
            let iids: Vec<u32> = p.list(3).unwrap_or_default().iter().filter_map(|v| match v {
                Value::Int(i) if *i > 0 => Some(*i as u32),
                _ => None,
            }).collect();
            let code = repair(ctx, st, sid, &iids, p.int(4).unwrap_or(-1), &npc_name).await?;
            st.players[&sid].send_raw(packet(repair_op::RESULT, echo, cid, Some(code)));
        }
        op => debug!("[réparation] #{sid} op {op:?} ignorée"),
    }
    Ok(())
}

/// Ce que vaut une demande au taux de taxe `ratio` : objets à réparer (IID, durabilité maximale, coût taxe comprise, taxe)
/// ou code de refus. Comme le client, la taxe se calcule objet par objet.
pub fn quote(items: &[(u32, Option<(i64, i32, i32, bool)>)], ratio: i32) -> Result<Vec<(u32, i32, i64, i64)>, i64> {
    use repair_result::*;
    if items.is_empty() {
        return Err(FAILED);
    }
    let mut out = Vec::new();
    let mut full = 0;
    for (iid, data) in items {
        // (prix au PNJ, durabilité maximale, actuelle, s'use)
        let Some((price, max, current, wears)) = *data else { return Err(FAILED) };
        if !wears {
            return Err(CANNOT_REPAIR);
        }
        if current >= max {
            full += 1;
            continue;
        }
        let cost = repair_cost(price, max, current);
        let tax = super::tax::tax_of(cost, ratio);
        out.push((*iid, max, cost + tax, tax));
    }
    if out.is_empty() && full > 0 {
        return Err(ALREADY_MAX);
    }
    Ok(out)
}

/// Répare les objets `iids` de l'inventaire : rend le code de résultat.
async fn repair(ctx: &WorldCtx, st: &mut WorldState, sid: u32, iids: &[u32], asked: i64, npc_name: &str) -> anyhow::Result<i64> {
    let p = &st.players[&sid];
    let mut unique = iids.to_vec();
    unique.sort_unstable();
    unique.dedup();
    let items: Vec<(u32, Option<(i64, i32, i32, bool)>)> = unique
        .iter()
        .map(|iid| {
            let found = p.inventory.get(*iid).and_then(|i| {
                let d = ctx.catalog.get(i.tid)?;
                let e = d.equip.as_ref()?;
                Some((d.npc_price, e.durability, e.current_durability(i.durability), e.wears()))
            });
            // Objet connu mais sans caractéristiques d'équipement : il ne se répare pas.
            let found = found.or_else(|| p.inventory.get(*iid).map(|_| (0, 0, 0, false)));
            (*iid, found)
        })
        .collect();
    let ratio = super::tax::ratio_of(ctx, st, sid);
    let plan = match quote(&items, ratio) {
        Ok(plan) => plan,
        Err(code) => {
            debug!("[réparation] {} : refus {code} ({} objet(s))", p.ch.name, items.len());
            return Ok(code);
        }
    };
    let cost: i64 = plan.iter().map(|(_, _, c, _)| c).sum();
    let tax: i64 = plan.iter().map(|(_, _, _, t)| t).sum();
    if cost != asked {
        warn!("[réparation] {} : coût annoncé {asked}, calculé {cost}", p.ch.name);
        return Ok(repair_result::FAILED);
    }
    if (p.ch.money as i64) < cost {
        return Ok(repair_result::NOT_ENOUGH_MONEY);
    }
    let money = p.ch.money as i64 - cost;
    set_money(ctx, st, sid, money).await?;
    let p = st.players.get_mut(&sid).unwrap();
    let (cid, name) = (p.ch.id, p.ch.name.clone());
    for (iid, max, price, _) in &plan {
        let Some(item) = p.inventory.items.get_mut(iid) else { continue };
        item.durability = DURABILITY_FULL;
        let tid = item.tid;
        save(ctx, *iid, DURABILITY_FULL).await?;
        p.send_raw(item_durability(*iid, tid, cid, *max));
        crate::journal::shop(&ctx.db, (cid, &name), npc_name, "repair", tid, *iid, 1, *price);
    }
    info!("[réparation] {name} fait réparer {} objet(s) chez {npc_name} pour {cost} pièce(s) (taxe {tax})", plan.len());
    let taxed = super::tax::Taxed { total: cost, tax, region: super::tax::taxed_region(ctx, st, sid) };
    super::tax::collect(ctx, st, &taxed).await?;
    Ok(repair_result::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn prix_comme_le_client() {
        // Bastard Sword : 69 au plus, 21 000 au PNJ, à 30 → 21000 / 69 × 39 × 0,5 = 5934,78.
        assert_eq!(repair_cost(21000, 69, 30), 5934);
        assert_eq!(repair_cost(21000, 69, 69), 0, "pleine");
        assert_eq!(repair_cost(100, 88, 80), 4, "Short Sword : 100 / 88 × 8 × 0,5 = 4,54");
        assert_eq!(repair_cost(100, 88, 0), 50);
        assert_eq!(repair_cost(100, 0, 0), 0, "sans durabilité");
    }

    #[test]
    fn devis_et_codes() {
        use repair_result::*;
        let worn = (1u32, Some((100i64, 88, 80, true)));
        let full = (2u32, Some((30i64, 63, 63, true)));
        let potion = (3u32, Some((10i64, 0, 0, false)));
        assert_eq!(quote(&[worn, full], 0), Ok(vec![(1, 88, 4, 0)]), "l'objet plein est ignoré");
        // Taxe de 10 % objet par objet : 5934 + 593.
        assert_eq!(quote(&[(5, Some((21000, 69, 30, true))), worn], 10), Ok(vec![(5, 69, 6527, 593), (1, 88, 4, 0)]));
        assert_eq!(quote(&[full], 0), Err(ALREADY_MAX));
        assert_eq!(quote(&[worn, potion], 0), Err(CANNOT_REPAIR));
        assert_eq!(quote(&[(4, None)], 0), Err(FAILED), "objet absent de l'inventaire");
        assert_eq!(quote(&[], 0), Err(FAILED));
    }

    #[test]
    fn tirage_de_l_usure() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(5);
        let wear = Wear { dealt: 3, taken: 4 };
        let all = roll_wear(&mut rng, wear, 100.0, 100.0, Some(10), &[2, 3]);
        assert_eq!(all.iter().filter(|p| **p == 10).count(), 3, "chaque coup porté use l'arme");
        assert_eq!(all.iter().filter(|p| **p == 2 || **p == 3).count(), 4, "chaque coup reçu use une armure");
        assert!(roll_wear(&mut rng, wear, 0.0, 0.0, Some(10), &[2]).is_empty(), "probabilités nulles");
        assert!(roll_wear(&mut rng, wear, 100.0, 100.0, None, &[]).is_empty(), "rien qui s'use");
        let n = roll_wear(&mut rng, Wear { dealt: 1000, taken: 0 }, 10.0, 0.0, Some(10), &[]).len();
        assert!((60..=140).contains(&n), "10 % de 1000 coups : {n}");
    }

    #[test]
    fn durabilite_d_un_exemplaire() {
        use crate::world::items::EquipData;
        let sword = EquipData::from_raw(&serde_json::json!({"Part":"10","Kind":"2","DUR":"88"})).unwrap();
        assert!(sword.wears());
        assert_eq!((sword.current_durability(DURABILITY_FULL), sword.current_durability(12), sword.current_durability(500)), (88, 12, 88));
        assert!(sword.is_broken(0) && !sword.is_broken(DURABILITY_FULL));
        let unbreakable = EquipData::from_raw(&serde_json::json!({"Part":"10","Kind":"2","DUR":"@50000"})).unwrap();
        assert!(unbreakable.unbreakable && !unbreakable.wears() && !unbreakable.is_broken(0));
        let ring = EquipData::from_raw(&serde_json::json!({"Part":"11","Kind":"8"})).unwrap();
        assert!(!ring.wears() && !ring.is_broken(0), "sans DUR : jamais cassé");
        let horse = EquipData::from_raw(&serde_json::json!({"Part":"14","Kind":"32","DUR":"252000","Run_Buff":"10500","SubType":"567"})).unwrap();
        assert_eq!((horse.run_buff, horse.ride_tid, horse.durability), (10500, 567, 252000));
    }

    #[test]
    fn paquets() {
        use archcore_net::decode_packet;
        let iid_list = Value::List((0..20).map(|i| Value::Int(if i == 0 { 42 } else { 0 })).collect());
        let s = &schema::EVENT_ITEM_REPAIR;
        let plain = build_packet(s, &Fields::new(s).with(0, 0).with(1, super::super::npcs::event_base(0x4000_000b, 17)).with(2, 3).with(3, iid_list).with(4, 4i64).with(5, 0));
        let p = decode_packet(s, &plain).unwrap();
        assert_eq!((p.int(0), p.int(2), p.int(4), p.list(3).map(|l| l.len())), (Some(0), Some(3), Some(4), Some(20)));
        let item = decode_packet(&schema::ITEM, &item_durability(42, 11, 3, 87)).unwrap();
        assert_eq!((item.int(0), item.int(1), item.int(2)), (Some(2), None, Some(42)), "op 2 sans statut");
        let f = decode_mini(&mini::CHAR_FACTORS, item.bytes(10).unwrap()).unwrap();
        assert_eq!(decode_mini(&mini::FACTOR_ITEM, f.bytes(10).unwrap()).unwrap().int(0), Some(87));
    }
}
