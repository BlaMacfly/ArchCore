//! Raffinage : recettes de `RefineryItem2.txt`, la table que lit `Archonia.exe` (à défaut,
//! `refineryrecipetable.txt` des tables du serveur d'origine, sans les colonnes « Keep »).
//!
//! ```text
//!   Recette : identifiant, ingrédients (« Resource TID » et « Resource Count »), prix en or (« Combine Use
//!        Price »), ce que l'objet produit garde de l'équipement fourni (« Keep Reinforcement Level », « Keep
//!        Item Socket Count », « Keep Socketed Items », « Keep Item Options »). Ses « MakeItem Total » lignes,
//!        de même identifiant, sont autant d'issues équiprobables : objet produit et quantité, objet bonus,
//!        nombre d'options (« Item Option Min/Max ») et de sockets (« Socket Inchant Min/Max ») de l'objet
//!        produit. Une recette se reconnaît à ses ingrédients, dans n'importe quel ordre.
//!   Ouverture : AGPMEVENTREFINERY op 0 sur un objet du monde de fonction 26 (sphère d'alchimie), op 1 accord.
//!   AGPMREFINERY : op 5 devis (20 IID de la grille → op 5, prix au champ 6) ; op 2 raffinage de la grille ;
//!        op 6 raffinage d'une recette (identifiant au champ 2, ingrédients pris dans l'inventaire). Réponse
//!        op 1 : résultat 0 réussite (TID produit au champ 5), 1 ingrédients manquants, 2 objets sans recette,
//!        4 or insuffisant, 5 inventaire plein.
//! ```

use rand::Rng;
use std::collections::HashMap;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    pub tid: u32,
    pub count: i64,
    pub fail: bool,
    pub bonus: Option<u32>,
    pub options: (i64, i64),
    pub sockets: (i64, i64),
}

/// Ce que l'objet produit garde de l'équipement fourni.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Keep {
    pub level: bool,
    pub sockets: bool,
    pub filled: bool,
    pub options: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Recipe {
    pub id: u32,
    pub keep: Keep,
    /// Ingrédients triés par TID : (TID, quantité).
    pub resources: Vec<(u32, i64)>,
    pub price: i64,
    pub timer_ms: i64,
    pub outcomes: Vec<Outcome>,
}

#[derive(Clone, Debug, Default)]
pub struct Recipes {
    pub by_id: HashMap<u32, Recipe>,
    by_resources: HashMap<Vec<(u32, i64)>, u32>,
}

/// Ingrédients triés et regroupés par TID.
pub fn normalize(items: impl IntoIterator<Item = (u32, i64)>) -> Vec<(u32, i64)> {
    let mut m: Vec<(u32, i64)> = Vec::new();
    for (tid, n) in items {
        match m.iter_mut().find(|(t, _)| *t == tid) {
            Some(e) => e.1 += n,
            None => m.push((tid, n)),
        }
    }
    m.sort_unstable();
    m
}

impl Recipes {
    /// Table importée dans `client_table` (`archcore import --server`) ; vide si elle manque.
    pub async fn load(db: &crate::db::Databases) -> anyhow::Result<Self> {
        for name in ["RefineryItem2.txt", "refineryrecipetable.txt"] {
            let row = db.world.first("SELECT data FROM client_table WHERE name = ? AND row_idx = 0", (name,)).await?;
            let Some(text) = row.and_then(|r| serde_json::from_str::<String>(&crate::db::col::<String>(&r, "data")).ok()) else { continue };
            let r = Self::parse(&text);
            crate::info!("raffinage : {} recette(s) ({name})", r.by_id.len());
            return Ok(r);
        }
        crate::warn!("raffinage : aucune table de recettes dans client_table (lancer archcore import)");
        Ok(Self::default())
    }

    pub fn parse(text: &str) -> Self {
        let mut lines = text.lines().map(|l| l.trim_end_matches('\r').split('\t').map(|c| c.trim().to_string()).collect::<Vec<_>>());
        let Some(header) = lines.next() else { return Self::default() };
        let col = |name: &str| header.iter().position(|h| h == name);
        let first_resource = col("Resource TID").unwrap_or(header.len());
        let mut out = Recipes::default();
        for r in lines {
            let n = |i: Option<usize>| i.and_then(|i| r.get(i)).and_then(|c| c.parse::<f64>().ok()).unwrap_or(0.0) as i64;
            let Ok(id) = r.first().map(String::as_str).unwrap_or("").parse::<u32>() else { continue };
            let mut resources = Vec::new();
            let mut i = first_resource;
            while i + 2 < header.len() {
                let tid = n(Some(i)) as u32;
                let count = n(Some(i + 2));
                if tid > 0 && count > 0 {
                    resources.push((tid, count));
                }
                i += 3;
            }
            let outcome = Outcome {
                tid: n(col("MakeItem TID")) as u32,
                count: n(col("MakeItem Count")).max(1),
                fail: n(col("MakeItem IsFail")) != 0,
                bonus: Some(n(col("BonusItem TID")) as u32).filter(|t| *t > 0),
                options: (n(col("Item Option Min")), n(col("Item Option Max"))),
                sockets: (n(col("Socket Inchant Min")), n(col("Socket Inchant Max"))),
            };
            let flag = |name: &str| n(col(name)) != 0;
            let e = out.by_id.entry(id).or_insert_with(|| Recipe {
                id,
                keep: Keep { level: flag("Keep Reinforcement Level"), sockets: flag("Keep Item Socket Count"), filled: flag("Keep Socketed Items"), options: flag("Keep Item Options") },
                resources: normalize(resources),
                price: n(col("Combine Use Price")),
                timer_ms: n(col("timer(msec)")),
                outcomes: Vec::new(),
            });
            e.outcomes.push(outcome);
        }
        for (id, r) in &out.by_id {
            out.by_resources.entry(r.resources.clone()).or_insert(*id);
        }
        out
    }

    /// Recette dont les ingrédients sont exactement ceux-ci (dans n'importe quel ordre).
    pub fn find(&self, items: impl IntoIterator<Item = (u32, i64)>) -> Option<&Recipe> {
        self.by_resources.get(&normalize(items)).and_then(|id| self.by_id.get(id))
    }
}

impl Recipe {
    /// Une issue tirée au hasard, toutes équiprobables.
    pub fn roll(&self, rng: &mut impl Rng) -> Option<&Outcome> {
        (!self.outcomes.is_empty()).then(|| &self.outcomes[rng.gen_range(0..self.outcomes.len())])
    }
}

/// Opérations d'AGPMREFINERY.
pub mod op {
    pub const RESULT: i64 = 1;
    pub const REFINE: i64 = 2;
    pub const QUOTE: i64 = 5;
    pub const REFINE_RECIPE: i64 = 6;
}

/// Résultats (op 1).
pub mod result {
    pub const SUCCESS: i64 = 0;
    pub const MISSING_ITEMS: i64 = 1;
    pub const NO_RECIPE: i64 = 2;
    pub const NO_MONEY: i64 = 4;
    pub const FULL_INVENTORY: i64 = 5;
}

/// Fonction d'événement de la sphère d'alchimie (objets du monde).
pub const EVENT_FUNCTION: i64 = 26;

fn packet(op: i64, cid: u32, result: Option<i64>, tid: Option<u32>, price: Option<i64>) -> Vec<u8> {
    use archcore_net::{build_packet, schema, Fields};
    let s = &schema::REFINERY;
    let f = Fields::new(s).with(0, op).with(1, cid).with_opt(4, result).with_opt(5, tid).with_opt(6, price);
    build_packet(s, &f)
}

/// AGPMEVENTREFINERY : la demande d'ouverture (op 0) d'une sphère d'alchimie est accordée (op 1).
pub fn on_event(st: &super::WorldState, sid: u32, p: &archcore_net::Packet) {
    use archcore_net::schema::{self, mini};
    use archcore_net::{build_mini, decode_mini, Fields};
    if p.int(0) != Some(0) {
        return;
    }
    let Some(player) = st.players.get(&sid) else { return };
    let base = p.bytes(1).and_then(|b| decode_mini(&mini::EVENT_BASE, b).ok()).unwrap_or_else(|| Fields::new(&mini::EVENT_BASE));
    if base.int(3) != Some(EVENT_FUNCTION) {
        crate::warn!("[raffinage] #{sid} événement {:?} : pas une sphère d'alchimie", base.int(3));
        return;
    }
    let s = &schema::EVENT_REFINERY;
    player.send(s, Fields::new(s).with(0, 1).with(1, build_mini(&mini::EVENT_BASE, &base)).with(2, player.ch.id));
}

/// AGPMREFINERY reçu du client.
pub async fn on_packet(ctx: &super::WorldCtx, st: &mut super::WorldState, sid: u32, p: &archcore_net::Packet) -> anyhow::Result<()> {
    let Some(player) = st.players.get(&sid) else { return Ok(()) };
    let cid = player.ch.id;
    match p.int(0) {
        Some(op::QUOTE) | Some(op::REFINE) => {
            let iids: Vec<u32> = p.list(3).unwrap_or_default().iter().filter_map(|v| match v {
                archcore_net::packet::Value::Int(i) if *i > 0 => Some(*i as u32),
                _ => None,
            }).collect();
            let picked: Vec<(u32, u32, i64)> = iids.iter().filter_map(|i| player.inventory.get(*i).map(|it| (it.iid, it.tid, it.count))).collect();
            let recipe = (picked.len() == iids.len()).then(|| ctx.recipes.find(picked.iter().map(|(_, t, n)| (*t, *n)))).flatten();
            let Some(recipe) = recipe else {
                player.send_raw(packet(op::RESULT, cid, Some(result::NO_RECIPE), None, None));
                return Ok(());
            };
            if p.int(0) == Some(op::QUOTE) {
                player.send_raw(packet(op::QUOTE, cid, None, None, Some(recipe.price)));
                return Ok(());
            }
            let take = picked.iter().map(|(iid, _, n)| (*iid, *n)).collect();
            refine(ctx, st, sid, recipe.id, take).await
        }
        Some(op::REFINE_RECIPE) => refine_recipe(ctx, st, sid, p.int(2).unwrap_or(0) as u32).await,
        other => {
            crate::debug!("[raffinage] #{sid} AGPMREFINERY op {other:?} (ignoré)");
            Ok(())
        }
    }
}

/// Raffinage d'une recette (op 6, et /last_refine) : ingrédients pris dans les piles de l'inventaire, autant
/// qu'il en faut.
pub async fn refine_recipe(ctx: &super::WorldCtx, st: &mut super::WorldState, sid: u32, id: u32) -> anyhow::Result<()> {
    let Some(player) = st.players.get(&sid) else { return Ok(()) };
    let cid = player.ch.id;
    let Some(recipe) = ctx.recipes.by_id.get(&id) else {
        player.send_raw(packet(op::RESULT, cid, Some(result::NO_RECIPE), None, None));
        return Ok(());
    };
    let mut take = Vec::new();
    for (tid, need) in &recipe.resources {
        let mut left = *need;
        for it in player.inventory.items.values().filter(|i| i.tid == *tid) {
            if left == 0 {
                break;
            }
            let n = it.count.min(left);
            take.push((it.iid, n));
            left -= n;
        }
        if left > 0 {
            player.send_raw(packet(op::RESULT, cid, Some(result::MISSING_ITEMS), None, None));
            return Ok(());
        }
    }
    refine(ctx, st, sid, recipe.id, take).await
}

/// Raffine selon la recette `id` avec les ingrédients `take` (IID, quantité) : or et place vérifiés,
/// ingrédients consommés, issue tirée, objet produit (et objet bonus) rangé dans l'inventaire.
async fn refine(ctx: &super::WorldCtx, st: &mut super::WorldState, sid: u32, id: u32, take: Vec<(u32, i64)>) -> anyhow::Result<()> {
    use super::items::{item_count, item_remove, item_status};
    let Some(recipe) = ctx.recipes.by_id.get(&id) else { return Ok(()) };
    let Some(p) = st.players.get_mut(&sid) else { return Ok(()) };
    let cid = p.ch.id;
    let money = p.ch.money as i64;
    if money < recipe.price {
        p.send_raw(packet(op::RESULT, cid, Some(result::NO_MONEY), None, None));
        return Ok(());
    }
    // Place : les piles entièrement consommées libèrent leur case.
    let freed = take.iter().filter(|(iid, n)| p.inventory.get(*iid).is_some_and(|i| i.count <= *n)).count();
    let needed = recipe.outcomes.iter().map(|o| {
        let stack = ctx.catalog.get(o.tid).map(|d| d.stack).unwrap_or(1).max(1);
        ((o.count + stack - 1) / stack) as usize + o.bonus.is_some() as usize
    }).max().unwrap_or(1);
    if p.inventory.free_count() + freed < needed {
        p.send_raw(packet(op::RESULT, cid, Some(result::FULL_INVENTORY), None, None));
        return Ok(());
    }
    // Équipement fourni dont l'objet produit garde les caractéristiques (« Keep ») : celui du même modèle,
    // sinon le premier équipement des ingrédients.
    let source = {
        let ingredients: Vec<super::items::InventoryItem> = take.iter().filter_map(|(iid, _)| p.inventory.get(*iid).cloned()).collect();
        let equip = |i: &&super::items::InventoryItem| ctx.catalog.get(i.tid).is_some_and(|d| d.equip.is_some());
        let first = recipe.outcomes.first().map(|o| o.tid).unwrap_or(0);
        ingredients.iter().find(|i| i.tid == first && equip(i)).or_else(|| ingredients.iter().find(equip)).cloned()
    };
    for (iid, n) in &take {
        match p.inventory.consume(&ctx.db, *iid, *n).await? {
            Some(left) => p.send_raw(item_count(&left, cid)),
            None => p.send_raw(item_remove(*iid, Some(cid))),
        }
    }
    let outcome = {
        let mut rng = rand::thread_rng();
        recipe.roll(&mut rng).cloned()
    };
    let Some(outcome) = outcome else { return Ok(()) };
    let mut made = vec![(outcome.tid, outcome.count, true)];
    if let Some(b) = outcome.bonus {
        made.push((b, 1, false));
    }
    for (tid, count, main) in made {
        let stack = ctx.catalog.get(tid).map(|d| d.stack).unwrap_or(1).max(1);
        let mut left = count;
        while left > 0 {
            let n = left.min(stack);
            left -= n;
            let Some((it, created)) = p.inventory.add(&ctx.db, tid, n, stack, None).await? else { break };
            if created {
                let (options, conv) = {
                    let mut rng = rand::thread_rng();
                    produced(ctx, tid, main.then_some(&outcome), main.then_some(recipe.keep), source.as_ref(), &mut rng)
                };
                p.inventory.set_generated(&ctx.db, it.iid, &options, conv.sockets).await?;
                if conv.level > 0 || !conv.filled.is_empty() {
                    ctx.db.characters.exec("UPDATE character_items SET convert_level = ?, socket_tids = ? WHERE id = ?", (conv.level, conv.filled_column(), it.iid)).await?;
                    if let Some(i) = p.inventory.items.get_mut(&it.iid) {
                        i.conv = conv.clone();
                    }
                }
            }
            let it = p.inventory.get(it.iid).cloned().unwrap_or(it);
            p.send_raw(if created { super::items::item_add_of(&it, cid, item_status::INVENTORY, Some(it.grid)) } else { item_count(&it, cid) });
            if let Some(c) = super::items::convert_add(&it, cid) {
                p.send_raw(c);
            }
        }
    }
    let name = p.ch.name.clone();
    if recipe.price > 0 {
        super::set_money(ctx, st, sid, money - recipe.price).await?;
    }
    if let Some(p) = st.players.get_mut(&sid) {
        p.send_raw(packet(op::RESULT, cid, Some(result::SUCCESS), Some(outcome.tid), None));
        // Recette que /last_refine relance.
        p.last_recipe = Some(id);
    }
    crate::info!("[raffinage] {name} : recette {id} → {} × {}{}", outcome.count, outcome.tid, if outcome.fail { " (issue d'échec)" } else { "" });
    Ok(())
}

/// Options et conversion d'un objet produit : repris de l'équipement fourni selon « Keep », sinon tirés
/// (options entre « Item Option Min » et « Max », sockets entre « Socket Inchant Min » et « Max », comme
/// pour un objet neuf ; à défaut, ceux d'un objet neuf).
fn produced(ctx: &super::WorldCtx, tid: u32, outcome: Option<&Outcome>, keep: Option<Keep>, source: Option<&super::items::InventoryItem>, rng: &mut impl Rng) -> (Vec<u16>, super::convert::ItemConvert) {
    let Some(e) = ctx.catalog.get(tid).and_then(|d| d.equip.as_ref()) else { return (Vec::new(), Default::default()) };
    let level = e.level.max(1);
    let (mut options, sockets) = super::options::generate(&ctx.catalog, &ctx.options, tid, level, rng);
    let mut conv = super::convert::ItemConvert { sockets, ..Default::default() };
    if let Some(o) = outcome {
        if o.options.1 > 0 {
            let n = super::options::roll_count(&ctx.options.option_rates, o.options.0.max(1), o.options.1, rng);
            options = ctx.options.pick_options(e, level, n, rng);
        }
        if o.sockets.1 > 0 {
            conv.sockets = super::options::roll_count(&ctx.options.socket_rates, o.sockets.0.max(1), o.sockets.1, rng).clamp(0, super::convert::MAX_SOCKETS as i64) as u8;
        }
    }
    if let (Some(k), Some(src)) = (keep, source) {
        if k.options {
            options = src.options.clone();
        }
        if k.level {
            conv.level = src.conv.level;
        }
        if k.sockets {
            conv.sockets = conv.sockets.max(src.conv.sockets);
        }
        if k.filled {
            conv.filled = src.conv.filled.clone();
            conv.sockets = conv.sockets.max(conv.filled.len() as u8);
        }
    }
    (options, conv)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    const TABLE: &str = "Recipe ID\tMakeItem Total\tMakeItem TID\tMakeItem Name\tMakeItem Count\tMakeItem IsFail\tBonusItem TID\tBonusItem Name\tItem Option Min\tItem Option Max\tSocket Inchant Min\tSocket Inchant Max\tCombine Use Price\ttimer(msec)\tResource TotalCount\tResource TID\tResource Name\tResource Count\tResource TID\tResource Name\tResource Count\r\n\
4\t2\t10305\tRank 5\t1\t0\t\t\t0\t0\t0\t0\t0\t\t2\t9001\tManual\t1\t10304\tRank 4\t1\r\n\
4\t2\t10303\tRank 3\t1\t0\t\t\t0\t0\t0\t0\t0\t\t2\t9001\tManual\t1\t10304\tRank 4\t1\r\n\
944\t1\t3299\tComet Wand\t1\t0\t\t\t1\t3\t1\t4\t5000\t\t2\t257\tStone\t1\t3299\tWand\t1\r\n";

    #[test]
    fn recettes() {
        let r = Recipes::parse(TABLE);
        assert_eq!(r.by_id.len(), 2);
        let pet = r.find([(10304, 1), (9001, 1)]).expect("ordre indifférent");
        assert_eq!((pet.id, pet.outcomes.len()), (4, 2));
        assert!(r.find([(10304, 1)]).is_none());
        assert!(r.find([(10304, 1), (9001, 2)]).is_none(), "quantités exactes");
        let wand = &r.by_id[&944];
        assert_eq!((wand.price, wand.outcomes[0].options, wand.outcomes[0].sockets), (5000, (1, 3), (1, 4)));
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        let mut seen = [0; 2];
        for _ in 0..200 {
            seen[(pet.roll(&mut rng).unwrap().tid == 10305) as usize] += 1;
        }
        assert!(seen[0] > 60 && seen[1] > 60, "{seen:?}");
        assert_eq!(normalize([(5, 1), (3, 2), (5, 1)]), vec![(3, 2), (5, 2)]);
    }

    #[test]
    fn format_du_client() {
        let t = "Recipe ID\tMakeItem Total\tKeep Skill Bonus\tKeep Reinforcement Level\tKeep Item Socket Count\tKeep Socketed Items\tKeep Item Options\tMakeItem TID\tMakeItem Name\tMakeItem Count\tMakeItem IsFail\tBonusItem TID\tBonusItem Name\tItem Option Min\tItem Option Max\tSocket Inchant Min\tSocket Inchant Max\tCombine Use Price\ttimer(msec)\tResource TotalCount\tResource TID\tResource Name\tResource Count\tResource TID\tResource Name\tResource Count\r\n\
183\t1\t\t1\t\t1\t\t5429\tDagger\t1\t0\t\t\t1\t1\t1\t1\t5000\t\t2\t258\tStone\t1\t5429\tDagger\t1\r\n";
        let r = Recipes::parse(t);
        let d = &r.by_id[&183];
        assert_eq!((d.keep, d.price, d.resources.clone()), (Keep { level: true, sockets: false, filled: true, options: false }, 5000, vec![(258, 1), (5429, 1)]));
        assert_eq!(d.outcomes[0].options, (1, 1));
    }
}
