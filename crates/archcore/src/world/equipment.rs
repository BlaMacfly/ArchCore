//! Équipement : porter armes et armures, et en tenir compte dans les facteurs.
//!
//! Le client demande un changement de place par AGPMITEM op 2 : statut 3 pour équiper un objet de
//! l'inventaire, statut 2 (et la case voulue au champ 7) pour ranger un objet porté ou déplacer un objet
//! dans l'inventaire. Avant d'équiper, le serveur vérifie le niveau, la race, la classe et le sexe exigés
//! (itemdatatable.txt) ; l'objet déjà porté sur la même partie du corps retourne dans l'inventaire, et une
//! arme à deux mains libère aussi l'autre main. Le porteur reçoit les mises à jour (op 2), ceux qui le voient
//! l'ajout (op 0, statut 3) ou le retrait (op 1) ; ses facteurs sont recalculés : dégâts et défense
//! physiques, précision, PV, vitesse et portée d'attaque de l'arme.
//!
//! Dans l'inventaire, poser une pile sur une pile du même objet les réunit si le total tient dans une pile
//! (la source disparaît, op 1 ; la cible est mise à jour, op 2). AGPMITEM op 17 sépare une pile (quantité au
//! champ 5, case libre au champ 7 ; nouvelle pile par op 0, l'ancienne mise à jour par op 2) ; op 21 détruit
//! un objet de l'inventaire (op 1 en réponse).

use super::items::{item_add, item_count, item_remove, item_status, item_update, EquipData, Grid, Inventory, InventoryItem, ItemCatalog};
use super::{send_system, Player, WorldCtx, WorldState};
use crate::character::{factors_mini, make_factors, Factors, POINT_HP};
use crate::{debug, info};
use archcore_net::schema::{self, mini};
use archcore_net::{decode_mini, Fields, Packet};

/// Parties du corps qui comptent pour les règles de remplacement.
pub mod part {
    pub const HAND_LEFT: i64 = 9;
    pub const HAND_RIGHT: i64 = 10;
    pub const RING1: i64 = 11;
    pub const RING2: i64 = 12;
    /// Monture (Kind 32) et arme montée (lance, étendard).
    pub const RIDE: i64 = 14;
    pub const LANCER: i64 = 15;
}
/// « Kind » d'une arme.
pub const KIND_WEAPON: i64 = 2;

/// Raison d'un refus, ou rien si le personnage peut porter l'objet.
pub fn check(e: &EquipData, level: i64, race: i64, class: i64, gender: i64) -> Result<(), &'static str> {
    if level < e.level {
        return Err("niveau insuffisant");
    }
    if !e.races.is_empty() && !e.races.contains(&race) {
        return Err("race non admise");
    }
    if !e.classes.is_empty() && !e.classes.contains(&class) {
        return Err("classe non admise");
    }
    if e.gender != 0 && e.gender != gender {
        return Err("réservé à l'autre sexe");
    }
    Ok(())
}

/// Partie où poser l'objet : la sienne, ou le second anneau si le premier est pris et l'autre libre.
pub fn target_part(inv: &Inventory, e: &EquipData) -> i64 {
    if e.part == part::RING1 && inv.equipment.contains_key(&part::RING1) && !inv.equipment.contains_key(&part::RING2) {
        return part::RING2;
    }
    e.part
}

/// Parties à libérer avant d'équiper \`e\` sur \`at\` : l'occupant de la partie, et l'autre main si l'un des deux
/// objets se tient à deux mains.
pub fn displaced(inv: &Inventory, catalog: &ItemCatalog, e: &EquipData, at: i64) -> Vec<i64> {
    let mut out = Vec::new();
    if inv.equipment.contains_key(&at) {
        out.push(at);
    }
    let other = match at {
        part::HAND_LEFT => Some(part::HAND_RIGHT),
        part::HAND_RIGHT => Some(part::HAND_LEFT),
        _ => None,
    };
    if let Some(o) = other {
        if let Some(held) = inv.equipment.get(&o) {
            let held_two = catalog.get(held.tid).and_then(|d| d.equip.as_ref()).is_some_and(|h| h.hand == 2);
            if e.hand == 2 || held_two {
                out.push(o);
            }
        }
    }
    out
}

/// Objets portés qui comptent dans les facteurs : pas ceux dont la durabilité est tombée à 0 (ils restent portés
/// jusqu'à leur réparation, `repair.rs`) ; à cheval, l'arme montée (partie 15) remplace les armes des mains, et à
/// pied elle ne compte pas (`ride.rs`).
pub fn counted<'a>(inv: &'a Inventory, catalog: &'a ItemCatalog) -> impl Iterator<Item = (&'a InventoryItem, &'a EquipData)> + 'a {
    let mounted = super::ride::is_mounted(inv);
    inv.equipment.iter().filter_map(move |(at, i)| {
        let e = catalog.get(i.tid).and_then(|d| d.equip.as_ref())?;
        let hand_weapon = matches!(*at, part::HAND_LEFT | part::HAND_RIGHT) && e.kind == KIND_WEAPON;
        let skipped = e.is_broken(i.durability) || (mounted && hand_weapon) || (!mounted && *at == part::LANCER);
        (!skipped).then_some((i, e))
    })
}

/// Caractéristiques des objets portés (ceux qui comptent, voir `counted`).
pub fn worn(inv: &Inventory, catalog: &ItemCatalog, table: &super::convert::ConvertTable) -> Vec<EquipData> {
    let stone = |tid: u32| catalog.get(tid).and_then(|d| d.stone);
    counted(inv, catalog).map(|(i, e)| super::convert::effective(table, e, &i.conv, stone)).collect()
}

/// TID des options de l'équipement porté par un personnage de niveau `level` : les siennes, celles des runes
/// serties (« OptionTID » ; une pierre d'esprit n'en a pas) et les options liées de l'objet à ce niveau.
pub fn worn_options(inv: &Inventory, catalog: &ItemCatalog, options: &super::options::OptionCatalog, level: i64) -> Vec<u16> {
    let mut out = Vec::new();
    for (i, _) in counted(inv, catalog) {
        out.extend(i.options.iter().copied());
        out.extend(i.conv.filled.iter().filter_map(|t| catalog.get(*t)).flat_map(|d| d.option_tids.iter().copied()));
        if let Some(e) = catalog.get(i.tid).and_then(|d| d.equip.as_ref()) {
            out.extend(e.link_ids.iter().filter_map(|l| options.linked(*l, level)));
        }
    }
    out
}

/// Effets de combat de l'équipement porté : options à déclenchement (les mêmes que `worn_options`) et
/// compétences des runes serties, retrouvées par leur nom dans le catalogue des compétences.
pub fn worn_gear(ctx: &WorldCtx, inv: &Inventory, level: i64) -> super::options::Gear {
    let mut g = ctx.options.gear(&worn_options(inv, &ctx.catalog, &ctx.options, level));
    for (i, _) in counted(inv, &ctx.catalog) {
        for rune in i.conv.filled.iter().filter_map(|t| ctx.convert.runes.get(t)) {
            let Some((name, skill_level, rate)) = &rune.skill else { continue };
            match ctx.skills.by_name(name) {
                Some(tid) => g.rune_skills.push((tid, *skill_level, *rate)),
                None => debug!("[équipement] compétence de rune inconnue : {name}"),
            }
        }
    }
    g
}

/// Facteurs de base auxquels s'ajoute l'équipement porté.
pub fn with_equipment(mut f: Factors, worn: &[EquipData]) -> Factors {
    for e in worn {
        f.damage_min[0] += e.min_damage;
        f.damage_max[0] += e.max_damage;
        f.defense_point[0] += e.defense;
        f.point_max[8] += e.ar;
        f.point_max[0] += e.hp;
        for i in 0..8 {
            f.damage_min[i + 1] += e.spirit_damage[i].0;
            f.damage_max[i + 1] += e.spirit_damage[i].1;
            f.defense_point[i + 1] += e.spirit_defense[i];
        }
        if e.kind == KIND_WEAPON {
            if e.atk_speed > 0 {
                f.attack[2] = e.atk_speed;
            }
            if e.atk_range > 0 {
                f.attack[0] = e.atk_range;
            }
        }
    }
    f
}

/// Recalcule les facteurs d'un joueur (modèle + équipement + effets des compétences) en gardant ses PV, PM, PE et son expérience,
/// et les lui envoie.
pub fn refresh_factors(ctx: &WorldCtx, st: &mut WorldState, sid: u32) {
    let Some(p) = st.players.get_mut(&sid) else { return };
    recompute_factors(ctx, p);
    let c = &schema::CHARACTER;
    p.send(c, Fields::new(c).with(0, 1).with(1, p.ch.id).with(7, factors_mini(&p.factors)));
}

/// Comme `refresh_factors`, sans rien envoyer : modèle et croissance au niveau de `p.ch`, équipement, effets.
pub fn recompute_factors(ctx: &WorldCtx, p: &mut Player) {
    let Some(tpl) = ctx.templates.get(p.ch.template_id) else { return };
    let old = p.factors.clone();
    let mut f = with_equipment(make_factors(tpl, Some(&p.ch)), &worn(&p.inventory, &ctx.catalog, &ctx.convert));
    ctx.options.apply(&mut f, &worn_options(&p.inventory, &ctx.catalog, &ctx.options, p.ch.level as i64));
    p.gear = worn_gear(ctx, &p.inventory, p.ch.level as i64);
    f.point[..5].copy_from_slice(&old.point[..5]);
    f.status[7] = old.status[7];
    f.dirt = old.dirt;
    super::effects::apply(&mut f, &super::effects::total(&p.skills.effects));
    // À cheval : la vitesse de la monture (« Run_Buff ») remplace marche et course.
    if let Some(speed) = super::ride::mount_speed(&p.inventory, &ctx.catalog) {
        f.status[8] = speed;
        f.status[9] = speed;
    }
    // Vitesse de déplacement multipliée par /speed (MJ), jusqu'à la déconnexion.
    if p.gm_speed > 1 {
        f.status[8] = f.status[8].saturating_mul(p.gm_speed);
        f.status[9] = f.status[9].saturating_mul(p.gm_speed);
    }
    f.point[POINT_HP] = f.point[POINT_HP].min(f.point_max[0]);
    f.point[1] = f.point[1].min(f.point_max[1]);
    p.factors = f;
}

/// Envoie \`plain\` aux joueurs qui voient \`sid\` (pas à lui).
fn to_viewers(st: &WorldState, sid: u32, plain: &[u8]) {
    let Some(me) = st.players.get(&sid) else { return };
    let cid = me.ch.id;
    for (_, o) in st.players.iter().filter(|(other, o)| **other != sid && o.seen_players.contains(&cid)) {
        o.send_raw(plain.to_vec());
    }
}

/// Paquets de l'équipement porté par \`p\`, pour un joueur qui le voit.
pub fn view_packets(p: &Player) -> Vec<Vec<u8>> {
    p.inventory.equip_packets()
}

/// Case demandée au champ 7 ; une coordonnée hors grille (0xFFFF, -1) laisse le serveur choisir.
fn wanted_grid(p: &Packet) -> Option<Grid> {
    let g = decode_mini(&mini::ITEM_GRID, p.bytes(7)?).ok()?;
    let v = [g.int(0)?, g.int(1)?, g.int(2)?];
    v.iter().all(|x| *x >= 0).then_some(v)
}

/// AGPMITEM op 2 reçu d'un joueur.
pub async fn on_item_update(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let iid = p.int(2).unwrap_or(0) as u32;
    let status = p.int(1).unwrap_or(-1);
    let Some(player) = st.players.get(&sid) else { return Ok(()) };
    if !player.in_game || player.dead {
        return Ok(());
    }
    if status == item_status::BANK || player.bank.items.contains_key(&iid) {
        return super::bank::on_item_update(ctx, st, sid, p).await;
    }
    let worn_part = player.inventory.equipment.iter().find(|(_, i)| i.iid == iid).map(|(part, _)| *part);
    match (status, worn_part) {
        (item_status::EQUIP, None) => equip(ctx, st, sid, iid, false).await.map(|_| ()),
        (item_status::INVENTORY, Some(at)) => unequip(ctx, st, sid, at, wanted_grid(p), false).await.map(|_| ()),
        (item_status::INVENTORY, None) => move_in_bag(ctx, st, sid, iid, wanted_grid(p)).await,
        _ => {
            debug!("[équipement] #{sid} statut {status} pour l'objet {iid} : ignoré");
            Ok(())
        }
    }
}

/// Renvoie au client la place réelle d'un objet de l'inventaire (après un refus).
fn resync(st: &WorldState, sid: u32, iid: u32) {
    let p = &st.players[&sid];
    if let Some(item) = p.inventory.get(iid) {
        p.send_raw(item_update(item, p.ch.id, item_status::INVENTORY));
    }
}

/// Porte un objet de l'inventaire, avec les contrôles du client (niveau, race, classe, sexe, monture, place pour ranger
/// ce qui est remplacé). `quiet` : refus sans message ni renvoi de la place (commutateur d'équipement, `itemset.rs`).
/// Rend vrai si l'objet est porté.
pub async fn equip(ctx: &WorldCtx, st: &mut WorldState, sid: u32, iid: u32, quiet: bool) -> anyhow::Result<bool> {
    let refused = |st: &WorldState, text: &str| {
        if !quiet {
            send_system(st, sid, text);
            resync(st, sid, iid);
        }
    };
    let p = &st.players[&sid];
    let Some(item) = p.inventory.get(iid).cloned() else { return Ok(false) };
    let Some(data) = ctx.catalog.get(item.tid) else { return Ok(false) };
    let Some(e) = data.equip.clone() else {
        refused(st, "Cet objet ne se porte pas.");
        return Ok(false);
    };
    let f = &p.factors;
    let (level, race, class, gender) = (f.status[7] as i64, f.ftype[0] as i64, f.ftype[2] as i64, f.ftype[1] as i64);
    if let Err(why) = check(&e, level, race, class, gender) {
        info!("[équipement] {} : {} refusé ({why})", p.ch.name, data.name);
        refused(st, &format!("Vous ne pouvez pas porter {} : {why}.", data.name));
        return Ok(false);
    }
    let at = target_part(&p.inventory, &e);
    // Monture : monter est refusé en combat, à un meurtrier, dans une région sans montures (ride.rs).
    if at == part::RIDE {
        if let Some(why) = super::ride::refusal(ctx, st, sid) {
            if !quiet {
                super::ride::refuse(st, sid, why);
                resync(st, sid, iid);
            }
            return Ok(false);
        }
    }
    let p = &st.players[&sid];
    let free = displaced(&p.inventory, &ctx.catalog, &e, at);
    let (cid, name, label) = (p.ch.id, p.ch.name.clone(), data.name.clone());
    let player = st.players.get_mut(&sid).unwrap();
    let Some((worn_item, stored)) = player.inventory.equip(&ctx.db, iid, at, &free).await? else {
        refused(st, "Pas assez de place dans l'inventaire pour changer d'équipement.");
        return Ok(false);
    };
    for s in &stored {
        player.send_raw(item_update(s, cid, item_status::INVENTORY));
    }
    player.send_raw(item_update(&worn_item, cid, item_status::EQUIP));
    for s in &stored {
        to_viewers(st, sid, &item_remove(s.iid, Some(cid)));
    }
    to_viewers(st, sid, &super::items::item_add_of(&worn_item, cid, item_status::EQUIP, None));
    refresh_factors(ctx, st, sid);
    if at == part::RIDE {
        super::ride::on_mount(st, sid, &worn_item);
    }
    info!("[équipement] {name} porte {label} (partie {at}{})", if stored.is_empty() { String::new() } else { format!(", {} objet(s) rangé(s)", stored.len()) });
    Ok(true)
}

/// Range dans l'inventaire l'objet porté sur `at` ; `quiet` : inventaire plein sans message. Rend vrai si l'objet est
/// rangé.
pub async fn unequip(ctx: &WorldCtx, st: &mut WorldState, sid: u32, at: i64, wanted: Option<Grid>, quiet: bool) -> anyhow::Result<bool> {
    let player = st.players.get_mut(&sid).unwrap();
    let cid = player.ch.id;
    let Some(item) = player.inventory.unequip(&ctx.db, at, wanted).await? else {
        if !quiet {
            send_system(st, sid, "Pas de place dans l'inventaire.");
        }
        return Ok(false);
    };
    player.send_raw(item_update(&item, cid, item_status::INVENTORY));
    let name = player.ch.name.clone();
    to_viewers(st, sid, &item_remove(item.iid, Some(cid)));
    refresh_factors(ctx, st, sid);
    if at == part::RIDE {
        super::ride::on_dismount(st, sid);
    }
    info!("[équipement] {name} retire l'objet {} (partie {at})", item.tid);
    Ok(true)
}

/// Range l'objet porté sur `at` à l'initiative du serveur (descente forcée de monture) : comme un retrait demandé
/// par le joueur. Rend faux si l'inventaire est plein.
pub async fn force_unequip(ctx: &WorldCtx, st: &mut WorldState, sid: u32, at: i64) -> anyhow::Result<bool> {
    let Some(p) = st.players.get(&sid) else { return Ok(false) };
    if !p.inventory.equipment.contains_key(&at) || p.inventory.free_slot().is_none() {
        return Ok(false);
    }
    unequip(ctx, st, sid, at, None, false).await
}

async fn move_in_bag(ctx: &WorldCtx, st: &mut WorldState, sid: u32, iid: u32, wanted: Option<Grid>) -> anyhow::Result<()> {
    let player = st.players.get_mut(&sid).unwrap();
    let cid = player.ch.id;
    // Case prise par une pile du même objet : fusion.
    let occupant = wanted.and_then(|g| player.inventory.at(g)).filter(|o| o.iid != iid).cloned();
    if let Some(target) = occupant {
        let max = ctx.catalog.get(target.tid).map(|d| d.stack).unwrap_or(1);
        match player.inventory.merge(&ctx.db, iid, target.iid, max).await? {
            Some(merged) => {
                player.send_raw(item_remove(iid, Some(cid)));
                player.send_raw(item_count(&merged, cid));
                debug!("[inventaire] {} réunit deux piles de l'objet {} : {}", player.ch.name, merged.tid, merged.count);
            }
            None => resync(st, sid, iid),
        }
        return Ok(());
    }
    match wanted {
        Some(g) => match player.inventory.move_to(&ctx.db, iid, g).await? {
            Some(item) => player.send_raw(item_update(&item, cid, item_status::INVENTORY)),
            None => resync(st, sid, iid),
        },
        None => resync(st, sid, iid),
    }
    Ok(())
}

/// Opérations AGPMITEM « séparer une pile » et « détruire un objet ».
pub const SPLIT_ITEM: i64 = 17;
pub const REQUEST_DESTROY_ITEM: i64 = 21;

/// AGPMITEM op 17 : sépare `count` exemplaires d'une pile vers une case libre.
pub async fn split(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let (iid, count) = (p.int(2).unwrap_or(0) as u32, p.int(5).unwrap_or(0));
    let Some(grid) = wanted_grid(p) else { return Ok(()) };
    let Some(player) = st.players.get_mut(&sid) else { return Ok(()) };
    let cid = player.ch.id;
    match player.inventory.split(&ctx.db, iid, count, grid).await? {
        Some((origin, part)) => {
            player.send_raw(item_add(part.iid, part.tid, Some(cid), part.count, item_status::INVENTORY, Some(part.grid)));
            player.send_raw(item_count(&origin, cid));
            debug!("[inventaire] {} sépare {count} exemplaire(s) de l'objet {}", player.ch.name, origin.tid);
        }
        None => resync(st, sid, iid),
    }
    Ok(())
}

/// AGPMITEM op 21 : détruit un objet de l'inventaire (pas un objet porté).
pub async fn destroy(ctx: &WorldCtx, st: &mut WorldState, sid: u32, iid: u32) -> anyhow::Result<()> {
    let Some(player) = st.players.get_mut(&sid) else { return Ok(()) };
    let Some(item) = player.inventory.get(iid).cloned() else { return Ok(()) };
    player.inventory.remove(&ctx.db, iid).await?;
    let cid = player.ch.id;
    player.send_raw(item_remove(iid, Some(cid)));
    let label = ctx.catalog.get(item.tid).map(|d| d.name.clone()).unwrap_or_default();
    info!("[inventaire] {} détruit {} × {label}", player.ch.name, item.count);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::items::{InventoryItem, ItemData};

    fn weapon(hand: i64) -> EquipData {
        EquipData { part: part::HAND_RIGHT, kind: KIND_WEAPON, hand, level: 1, classes: vec![1], min_damage: 11, max_damage: 13, atk_speed: 71, atk_range: 180, ..Default::default() }
    }

    fn catalog(entries: &[(u32, EquipData)]) -> ItemCatalog {
        let mut c = ItemCatalog::default();
        for (tid, e) in entries {
            c.items.insert(*tid, ItemData { tid: *tid, name: format!("objet {tid}"), npc_price: 0, pc_price: 0, stack: 1, equip: Some(e.clone()), potion: None, scroll: None, stone: None, usable_type: 0, option_tids: Vec::new(), kind: 0, bound_type: 0, cash: false, enable_on_ride: false, auction_category: (0, 0), event_item: false });
        }
        c
    }

    #[test]
    fn restrictions() {
        let mut e = weapon(1);
        e.races = vec![1, 2];
        e.level = 3;
        assert_eq!(check(&e, 2, 1, 1, 1), Err("niveau insuffisant"));
        assert_eq!(check(&e, 3, 3, 1, 1), Err("race non admise"));
        assert_eq!(check(&e, 3, 1, 4, 1), Err("classe non admise"));
        assert_eq!(check(&EquipData { gender: 2, ..e.clone() }, 3, 1, 1, 1), Err("réservé à l'autre sexe"));
        assert_eq!(check(&e, 3, 2, 1, 2), Ok(()));
    }

    #[test]
    fn remplacement_et_deux_mains() {
        let shield = EquipData { part: part::HAND_LEFT, kind: 4, hand: 1, ..Default::default() };
        let cat = catalog(&[(1, weapon(1)), (2, shield.clone()), (3, weapon(2))]);
        let mut inv = Inventory::default();
        inv.equipment.insert(part::HAND_RIGHT, InventoryItem { iid: 10, tid: 1, count: 1, grid: [10, 0, 0], conv: Default::default(), options: Vec::new(), durability: crate::world::items::DURABILITY_FULL });
        inv.equipment.insert(part::HAND_LEFT, InventoryItem { iid: 11, tid: 2, count: 1, grid: [9, 0, 0], conv: Default::default(), options: Vec::new(), durability: crate::world::items::DURABILITY_FULL });
        // Arme à une main : seule l'arme en main droite est rangée.
        assert_eq!(displaced(&inv, &cat, &weapon(1), part::HAND_RIGHT), vec![part::HAND_RIGHT]);
        // Arme à deux mains : le bouclier aussi.
        assert_eq!(displaced(&inv, &cat, &weapon(2), part::HAND_RIGHT), vec![part::HAND_RIGHT, part::HAND_LEFT]);
        // Bouclier alors qu'une arme à deux mains est tenue : elle est rangée.
        inv.equipment.insert(part::HAND_RIGHT, InventoryItem { iid: 12, tid: 3, count: 1, grid: [10, 0, 0], conv: Default::default(), options: Vec::new(), durability: crate::world::items::DURABILITY_FULL });
        inv.equipment.remove(&part::HAND_LEFT);
        assert_eq!(displaced(&inv, &cat, &shield, part::HAND_LEFT), vec![part::HAND_RIGHT]);
        // Second anneau.
        let ring = EquipData { part: part::RING1, kind: 8, ..Default::default() };
        inv.equipment.insert(part::RING1, InventoryItem { iid: 13, tid: 4, count: 1, grid: [11, 0, 0], conv: Default::default(), options: Vec::new(), durability: crate::world::items::DURABILITY_FULL });
        assert_eq!(target_part(&inv, &ring), part::RING2);
    }

    #[test]
    fn objets_qui_comptent() {
        let sword = EquipData { durability: 88, ..weapon(1) };
        let armor = EquipData { part: 2, kind: 1, defense: 4, durability: 63, ..Default::default() };
        let lance = EquipData { part: part::LANCER, kind: KIND_WEAPON, min_damage: 50, durability: 100, ..Default::default() };
        let horse = EquipData { part: part::RIDE, kind: 32, run_buff: 10500, ..Default::default() };
        let cat = catalog(&[(1, sword), (2, armor), (3, lance), (4, horse)]);
        let item = |iid: u32, tid: u32, at: i64, durability: i32| InventoryItem { iid, tid, count: 1, grid: [at, 0, 0], conv: Default::default(), options: Vec::new(), durability };
        let mut inv = Inventory::default();
        inv.equipment.insert(part::HAND_RIGHT, item(10, 1, part::HAND_RIGHT, -1));
        inv.equipment.insert(2, item(11, 2, 2, 0));
        inv.equipment.insert(part::LANCER, item(12, 3, part::LANCER, 5));
        let tids = |inv: &Inventory| counted(inv, &cat).map(|(i, _)| i.tid).collect::<Vec<_>>();
        assert_eq!(tids(&inv), vec![1], "armure cassée (0) et lance à pied ne comptent pas");
        inv.equipment.insert(part::RIDE, item(13, 4, part::RIDE, -1));
        assert_eq!(tids(&inv), vec![4, 3], "à cheval : la lance remplace l'épée");
    }

    #[test]
    fn facteurs_avec_equipement() {
        let armor = EquipData { part: 2, kind: 1, defense: 4, hp: 10, ..Default::default() };
        let mut base = Factors::default();
        base.attack = [150, 50, 60, 0, 0, 0, 0, 0];
        let f = with_equipment(base, &[weapon(1), armor]);
        assert_eq!((f.damage_min[0], f.damage_max[0], f.defense_point[0], f.point_max[0]), (11, 13, 4, 10));
        assert_eq!((f.attack[0], f.attack[2]), (180, 71), "portée et vitesse de l'arme");
    }
}
