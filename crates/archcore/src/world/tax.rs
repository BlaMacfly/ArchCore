//! Taxes des régions des châteaux (AgpmTax 0x53) ; intendants « Grand Advisor » (fonction de PNJ 34).
//!
//! ```text
//!   C op 0 (EVENT_BASE, fonction 34)       ← op 1 {CID, EVENT_BASE} (PNJ à portée) : le client n'ouvre UI_TaxSteward
//!                                            qu'au maître du château où il se trouve
//!   C op 3 {château}                       ← op 3 {CID, 0, château, régions} ; pas le propriétaire : résultat 1, rien
//!   C op 2 {château, régions {nom, taux}}  ← op 2 {CID, résultat, château, régions} : 0 fait, 1 pas le propriétaire,
//!                                            3 taux hors de 0 à 50, 2 taux déjà changés cette semaine ; fait : op 4 à tous
//!   entrée en jeu, changement des taux     ← op 4 {CID du destinataire, région {nom, recette, taux}}, une par région
//! ```
//! Régions taxées : celles du château et de ses abords (`siege_war`). Le nom d'une région dans les paquets est celui que
//! compose le client du joueur (RegionTemplate.ini traduit par le RegionGlossary.txt de sa langue, `locale.rs`) : il y
//! cherche le taux de la région où il se tient pour afficher ses prix.
//!
//! Règles du client appliquées telles quelles : taux de 0 à 50 % ; un changement par semaine, du lundi au dimanche (heure
//! de `World.TimeZone`) ; taxe = prix × taux / 100 (division entière) ajoutée au prix ; tribut de 20 % des recettes des
//! régions à l'ArchLord (ligne 5 de UI_TaxView), sauf à Rensphere.
//!
//! Choix d'ArchCore (le serveur d'origine n'est pas connu) :
//! * taxés : achats aux PNJ, réparation, coiffeur, portails, maître des compétences, production (compétence, recette),
//!   création de guilde ; ni la vente aux PNJ, ni les échanges et ventes entre joueurs. La taxe va à la recette de la
//!   région où se tient le joueur ;
//! * un château sans propriétaire ne taxe pas (taux 0) ; un nouveau propriétaire repart de taux à 0, libre de les changer ;
//! * versement chaque semaine (`Tax.PayoutDay`, `Tax.PayoutHour` : lundi 0 h) des recettes de la période dans la banque du
//!   compte du chef de la guilde propriétaire, moins le tribut versé à la banque de l'ArchLord (sans ArchLord, le
//!   propriétaire garde tout) ; recette totale du château cumulée, recettes des régions remises à 0, versements inscrits
//!   au journal de la banque (`taxe`, `tribut`) ; sans propriétaire, les recettes attendent. `/tax pay` verse aussitôt.

use super::npcs::event_function;
use super::siege::{self, CastleDef, RENSPHERE};
use super::{WorldCtx, WorldState};
use crate::character::unix_now;
use crate::db::{col, Databases};
use crate::{info, warn};
use archcore_net::schema::{self, mini, tax_op, tax_result};
use archcore_net::{build_mini, build_packet, decode_mini, Fields, Packet};
use chrono::{Datelike, NaiveDateTime};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// Taux maximal (`DAT_140e0beb4` d'Archonia).
pub const MAX_RATIO: i32 = 50;
/// Tribut versé à l'ArchLord, en pour cent des recettes des régions (`FUN_14026e250`).
pub const ARCHLORD_TRIBUTE: i64 = 20;
/// Cadence des versements et des enregistrements différés.
const STEP: Duration = Duration::from_secs(10);
/// Distance à l'intendant pour lui parler.
const NPC_RANGE: f64 = super::npc_events::MAX_NPC_DISTANCE;

/// Une région taxée.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RegionTax {
    pub castle: u8,
    pub ratio: i32,
    /// Recette depuis le dernier versement.
    pub income: i64,
}

/// Comptes des taxes d'un château (secondes Unix, 0 : jamais).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CastleTax {
    /// Recette déjà versée au propriétaire, cumulée.
    pub total: i64,
    pub paid_at: i64,
    pub modified_at: i64,
    /// Début de la période en cours (dernier versement).
    pub since: i64,
}

#[derive(Default)]
pub struct Taxes {
    pub regions: BTreeMap<u16, RegionTax>,
    pub castles: BTreeMap<u8, CastleTax>,
    /// Châteaux changés hors d'un paquet (changement de propriétaire), à enregistrer.
    dirty: Vec<u8>,
    next_step: Option<Instant>,
}

/// Régions taxées d'un château : celle du château, puis celle de ses abords.
pub fn regions_of(def: &CastleDef) -> Vec<u16> {
    let mut v: Vec<u16> = [def.castle_region, def.outskirts_region].into_iter().filter(|r| *r != 0).collect();
    v.dedup();
    v
}

impl Taxes {
    /// Régions des châteaux de `siege_war`, puis taux, recettes et comptes enregistrés.
    pub async fn load(db: &Databases, cat: &siege::SiegeCatalog) -> Self {
        let mut t = Taxes::default();
        let now = unix_now();
        for d in &cat.castles {
            t.castles.insert(d.id, CastleTax { since: now, ..Default::default() });
            for r in regions_of(d) {
                t.regions.insert(r, RegionTax { castle: d.id, ..Default::default() });
            }
        }
        let sql = "SELECT castle_id, total_income, UNIX_TIMESTAMP(last_paid_at) AS paid, UNIX_TIMESTAMP(modified_at) AS modified, \
                   UNIX_TIMESTAMP(period_start) AS since FROM castle_tax";
        match db.characters.rows(sql, ()).await {
            Ok(rows) => {
                for r in &rows {
                    let Some(c) = t.castles.get_mut(&(col::<u32>(r, "castle_id") as u8)) else { continue };
                    let ts = |k: &str| col::<Option<i64>>(r, k).unwrap_or(0);
                    (c.total, c.paid_at, c.modified_at) = (col(r, "total_income"), ts("paid"), ts("modified"));
                    c.since = ts("since").max(1);
                }
            }
            Err(e) => warn!("[taxe] comptes des châteaux non lus ({e:#}) : lancer `archcore db-init`"),
        }
        if let Ok(rows) = db.characters.rows("SELECT region_id, ratio, income FROM castle_tax_region", ()).await {
            for r in &rows {
                let Some(x) = t.regions.get_mut(&(col::<u32>(r, "region_id") as u16)) else { continue };
                x.ratio = col::<i32>(r, "ratio").clamp(0, MAX_RATIO);
                x.income = col(r, "income");
            }
        }
        info!("taxes : {} région(s) de {} château(x), {} au taux non nul", t.regions.len(), t.castles.len(), t.regions.values().filter(|r| r.ratio > 0).count());
        t
    }
}

// ---------------------------------------------------------------------------------------------
// Règles
// ---------------------------------------------------------------------------------------------

/// Taxe d'un prix : `prix × taux / 100`, division entière ; rien pour un taux nul ou négatif (formule du client).
pub fn tax_of(price: i64, ratio: i32) -> i64 {
    if ratio > 0 && price > 0 { price * ratio as i64 / 100 } else { 0 }
}

/// Résultat d'un changement de taux (op 2), dans l'ordre des contrôles : propriétaire, plage de 0 à 50, semaine.
pub fn modify_result(owner: bool, ratios: &[i64], may_modify: bool) -> i64 {
    if !owner {
        tax_result::NOT_OWNER
    } else if ratios.iter().any(|r| !(0..=MAX_RATIO as i64).contains(r)) {
        tax_result::OUT_OF_RANGE
    } else if !may_modify {
        tax_result::NOT_TIME
    } else {
        tax_result::SUCCESS
    }
}

/// Les taux peuvent-ils changer (`FUN_14026e450` d'Archonia, heures locales) : jamais changés, autre année, plus de six
/// jours d'écart, ou dernier changement avant le lundi de la semaine en cours.
pub fn may_modify(modified: Option<NaiveDateTime>, now: NaiveDateTime) -> bool {
    let Some(m) = modified else { return true };
    if m.year() != now.year() || now - m > chrono::Duration::days(6) {
        return true;
    }
    let monday = now.date() - chrono::Duration::days(now.weekday().num_days_from_monday() as i64);
    m.date() < monday
}

/// Dernier instant de versement au plus tard `now` : jour `day` (1 lundi à 7 dimanche) à `hour` h.
pub fn last_payout(now: NaiveDateTime, day: u32, hour: u32) -> NaiveDateTime {
    let back = (now.weekday().number_from_monday() + 7 - day.clamp(1, 7)) % 7;
    let at = (now.date() - chrono::Duration::days(back as i64)).and_hms_opt(hour.min(23), 0, 0).unwrap_or(now);
    if at > now { at - chrono::Duration::days(7) } else { at }
}

/// Partage d'une recette : (tribut à l'ArchLord, part du propriétaire).
pub fn split_income(sum: i64, tribute: bool) -> (i64, i64) {
    let t = if tribute { sum * ARCHLORD_TRIBUTE / 100 } else { 0 };
    (t, sum - t)
}

/// Taux effectif d'une région : 0 si elle n'est pas taxée ou si son château n'a pas de propriétaire.
pub fn ratio_at(st: &WorldState, region: u16) -> i32 {
    st.tax.regions.get(&region).filter(|r| st.siege.castles.get(&r.castle).is_some_and(|c| c.owner.is_some())).map(|r| r.ratio).unwrap_or(0)
}

/// Région taxée (taux positif) où se tient le joueur.
pub fn taxed_region(ctx: &WorldCtx, st: &WorldState, sid: u32) -> Option<u16> {
    siege::region_of(ctx, st, sid).filter(|r| ratio_at(st, *r) > 0)
}

/// Taux de taxe de la région du joueur.
pub fn ratio_of(ctx: &WorldCtx, st: &WorldState, sid: u32) -> i32 {
    taxed_region(ctx, st, sid).map(|r| ratio_at(st, r)).unwrap_or(0)
}

/// Prix taxé : total à payer, taxe, région qui la reçoit.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Taxed {
    pub total: i64,
    pub tax: i64,
    pub region: Option<u16>,
}

/// Prix `price` taxé dans la région du joueur.
pub fn quote(ctx: &WorldCtx, st: &WorldState, sid: u32, price: i64) -> Taxed {
    let region = taxed_region(ctx, st, sid);
    let tax = region.map(|r| tax_of(price, ratio_at(st, r))).unwrap_or(0);
    Taxed { total: price + tax, tax, region }
}

/// Taxe payée : ajoutée à la recette de sa région, enregistrée.
pub async fn collect(ctx: &WorldCtx, st: &mut WorldState, t: &Taxed) -> anyhow::Result<()> {
    let Some(rid) = t.region.filter(|_| t.tax > 0) else { return Ok(()) };
    let Some(r) = st.tax.regions.get_mut(&rid) else { return Ok(()) };
    r.income = r.income.saturating_add(t.tax);
    let r = r.clone();
    save_region(ctx, rid, &r).await
}

// ---------------------------------------------------------------------------------------------
// Enregistrement
// ---------------------------------------------------------------------------------------------

async fn save_region(ctx: &WorldCtx, rid: u16, r: &RegionTax) -> anyhow::Result<()> {
    ctx.db
        .characters
        .exec(
            "INSERT INTO castle_tax_region (region_id, castle_id, ratio, income) VALUES (?, ?, ?, ?) \
             ON DUPLICATE KEY UPDATE castle_id = VALUES(castle_id), ratio = VALUES(ratio), income = VALUES(income)",
            (rid, r.castle, r.ratio, r.income),
        )
        .await?;
    Ok(())
}

/// Enregistre les comptes d'un château et ses régions.
async fn save_castle(ctx: &WorldCtx, st: &WorldState, id: u8) -> anyhow::Result<()> {
    let Some(c) = st.tax.castles.get(&id).cloned() else { return Ok(()) };
    let t = |v: i64| (v > 0).then_some(v);
    ctx.db
        .characters
        .exec(
            "INSERT INTO castle_tax (castle_id, total_income, last_paid_at, modified_at, period_start) VALUES (?, ?, FROM_UNIXTIME(?), FROM_UNIXTIME(?), FROM_UNIXTIME(?)) \
             ON DUPLICATE KEY UPDATE total_income = VALUES(total_income), last_paid_at = VALUES(last_paid_at), modified_at = VALUES(modified_at), period_start = VALUES(period_start)",
            (id, c.total, t(c.paid_at), t(c.modified_at), c.since.max(1)),
        )
        .await?;
    let regions: Vec<(u16, RegionTax)> = st.tax.regions.iter().filter(|(_, r)| r.castle == id).map(|(k, r)| (*k, r.clone())).collect();
    for (rid, r) in regions {
        save_region(ctx, rid, &r).await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Paquets
// ---------------------------------------------------------------------------------------------

fn base(op: i64, cid: u32) -> Fields {
    Fields::new(&schema::TAX).with(0, op).with(1, cid)
}

/// Mini château.
pub fn castle_mini(name: &str, c: &CastleTax) -> Vec<u8> {
    let m = &mini::TAX_CASTLE;
    build_mini(m, &Fields::new(m).with(0, name).with(1, c.total).with(2, c.paid_at.max(0)).with(3, c.modified_at.max(0)))
}

/// Mini région : `name` traduit pour le destinataire, taux effectif.
pub fn region_mini(name: &str, income: i64, ratio: i32) -> Vec<u8> {
    let m = &mini::TAX_REGION;
    build_mini(m, &Fields::new(m).with(0, name).with(1, income).with(2, ratio))
}

/// Op 2 ou 3 au joueur `sid` : résultat, château et ses régions (noms dans sa langue).
fn castle_packet(ctx: &WorldCtx, st: &WorldState, sid: u32, op: i64, result: i64, def: &CastleDef) -> Option<Vec<u8>> {
    let p = st.players.get(&sid)?;
    let c = st.tax.castles.get(&def.id).cloned().unwrap_or_default();
    let mut f = base(op, p.ch.id).with(2, result).with(3, castle_mini(&def.name, &c));
    for (i, rid) in regions_of(def).into_iter().take(4).enumerate() {
        let income = st.tax.regions.get(&rid).map(|r| r.income).unwrap_or(0);
        f.set(4 + i, region_mini(&ctx.locale.region_name(rid, &p.language), income, ratio_at(st, rid)));
    }
    Some(build_packet(&schema::TAX, &f))
}

/// Op 4 d'une région au joueur `sid`.
fn ratio_packet(ctx: &WorldCtx, st: &WorldState, sid: u32, rid: u16) -> Option<Vec<u8>> {
    let p = st.players.get(&sid)?;
    let income = st.tax.regions.get(&rid).map(|r| r.income).unwrap_or(0);
    Some(build_packet(&schema::TAX, &base(tax_op::REGION_RATIO, p.ch.id).with(4, region_mini(&ctx.locale.region_name(rid, &p.language), income, ratio_at(st, rid)))))
}

/// Op 4 des régions d'un château à tous les joueurs en jeu.
pub fn broadcast_regions(ctx: &WorldCtx, st: &WorldState, id: u8) {
    let rids: Vec<u16> = st.tax.regions.iter().filter(|(_, r)| r.castle == id).map(|(k, _)| *k).collect();
    for sid in st.sids() {
        if !st.players[&sid].in_game {
            continue;
        }
        for rid in &rids {
            if let Some(pkt) = ratio_packet(ctx, st, sid, *rid) {
                st.players[&sid].send_raw(pkt);
            }
        }
    }
}

/// Entrée en jeu : le taux de chaque région taxée (op 4).
pub fn on_enter(ctx: &WorldCtx, st: &WorldState, sid: u32) {
    let Some(p) = st.players.get(&sid) else { return };
    for rid in st.tax.regions.keys() {
        if let Some(pkt) = ratio_packet(ctx, st, sid, *rid) {
            p.send_raw(pkt);
        }
    }
}

/// Le joueur est le chef de la guilde propriétaire du château.
fn is_owner(st: &WorldState, sid: u32, id: u8) -> bool {
    let Some(cid) = st.players.get(&sid).map(|p| p.ch.id) else { return false };
    siege::mastered_guild_of(st, cid).is_some_and(|g| st.siege.castles.get(&id).and_then(|c| c.owner) == Some(g))
}

/// Château désigné par le mini du champ 3 (nom de SiegeWar.txt).
fn castle_of(ctx: &WorldCtx, p: &Packet) -> Option<CastleDef> {
    let name = p.bytes(3).and_then(|b| decode_mini(&mini::TAX_CASTLE, b).ok()).and_then(|f| f.text(0))?;
    ctx.sieges.named(&name).cloned()
}

// ---------------------------------------------------------------------------------------------
// Paquets reçus
// ---------------------------------------------------------------------------------------------

/// AgpmTax reçu du client.
pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some(player) = st.players.get(&sid).filter(|pl| pl.in_game) else { return Ok(()) };
    if p.int(1).is_some_and(|c| c != 0 && c != player.ch.id as i64) {
        warn!("[taxe] #{sid} CID {:?} usurpé", p.int(1));
        return Ok(());
    }
    match p.int(0).unwrap_or(-1) {
        tax_op::REQUEST => open(ctx, st, sid, p),
        tax_op::VIEW => view(ctx, st, sid, p),
        tax_op::MODIFY => modify(ctx, st, sid, p).await?,
        other => warn!("[taxe] #{sid} op {other} refusée (réservée au serveur ou inconnue)"),
    }
    Ok(())
}

/// Op 0 : l'intendant (fonction 34, à portée) accorde l'ouverture.
fn open(ctx: &WorldCtx, st: &WorldState, sid: u32, p: &Packet) {
    let player = &st.players[&sid];
    let ev = p.bytes(3).and_then(|b| decode_mini(&mini::EVENT_BASE, b).ok()).unwrap_or_else(|| Fields::new(&mini::EVENT_BASE));
    let Some((npc, _)) = super::npc_events::resolve(&ctx.npcs, &ev) else {
        warn!("[taxe] #{sid} intendant inconnu");
        return;
    };
    if !npc.events.iter().any(|e| e.function == event_function::TAX) {
        warn!("[taxe] #{sid} {} n'est pas un intendant", npc.name);
        return;
    }
    if super::npc_events::npc_distance(npc, player.ch.pos_x, player.ch.pos_z) > NPC_RANGE {
        return;
    }
    let echo = if p.bytes(3).is_some() { build_mini(&mini::EVENT_BASE, &ev) } else { super::npcs::event_base(npc.cid, event_function::TAX) };
    player.send(&schema::TAX, base(tax_op::GRANT, player.ch.id).with(3, echo));
    let castle = ctx.map.region(npc.pos.x as f64, npc.pos.z as f64).and_then(|r| ctx.sieges.by_region(r)).map(|d| d.name.clone()).unwrap_or_else(|| "hors château".into());
    info!("[taxe] {} parle à l'intendant {} ({castle})", player.ch.name, npc.name);
}

/// Op 3 : taxes du château, à son propriétaire.
fn view(ctx: &WorldCtx, st: &WorldState, sid: u32, p: &Packet) {
    let Some(def) = castle_of(ctx, p) else {
        warn!("[taxe] #{sid} op 3 pour un château inconnu");
        return;
    };
    let result = if is_owner(st, sid, def.id) { tax_result::SUCCESS } else { tax_result::NOT_OWNER };
    if let Some(pkt) = castle_packet(ctx, st, sid, tax_op::VIEW, result, &def) {
        st.players[&sid].send_raw(pkt);
    }
}

/// Op 2 : nouveaux taux des régions du château.
async fn modify(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some(def) = castle_of(ctx, p) else {
        warn!("[taxe] #{sid} op 2 pour un château inconnu");
        return Ok(());
    };
    let (name, lang) = (st.players[&sid].ch.name.clone(), st.players[&sid].language.clone());
    let regions = regions_of(&def);
    // Région désignée par son nom traduit pour ce joueur, à défaut par sa place dans le paquet.
    let asked: Vec<(u16, i64)> = (4..8)
        .enumerate()
        .filter_map(|(i, f)| p.bytes(f).and_then(|b| decode_mini(&mini::TAX_REGION, b).ok()).map(|m| (i, m)))
        .filter_map(|(i, m)| {
            let text = m.text(0).unwrap_or_default();
            let by_name = regions.iter().copied().find(|r| ctx.locale.region_name(*r, &lang).trim().eq_ignore_ascii_case(text.trim()));
            Some((by_name.or_else(|| regions.get(i).copied())?, m.int(2).unwrap_or(0)))
        })
        .collect();
    let tz = ctx.config.world.time_zone;
    let now = unix_now();
    let modified = st.tax.castles.get(&def.id).map(|c| c.modified_at).filter(|t| *t > 0).map(|t| siege::to_local(tz, t));
    let ratios: Vec<i64> = asked.iter().map(|(_, r)| *r).collect();
    let mut result = modify_result(is_owner(st, sid, def.id), &ratios, may_modify(modified, siege::to_local(tz, now)));
    if result == tax_result::SUCCESS && asked.is_empty() {
        result = tax_result::OUT_OF_RANGE;
    }
    if result == tax_result::SUCCESS {
        for (rid, ratio) in &asked {
            if let Some(r) = st.tax.regions.get_mut(rid) {
                r.ratio = *ratio as i32;
            }
        }
        st.tax.castles.entry(def.id).or_default().modified_at = now;
        save_castle(ctx, st, def.id).await?;
    }
    if let Some(pkt) = castle_packet(ctx, st, sid, tax_op::MODIFY, result, &def) {
        st.players[&sid].send_raw(pkt);
    }
    if result == tax_result::SUCCESS {
        broadcast_regions(ctx, st, def.id);
        let list: Vec<String> = asked.iter().map(|(r, t)| format!("{} {t} %", ctx.locale.region_name(*r, "en"))).collect();
        info!("[taxe] {name} fixe les taxes de {} : {}", def.name, list.join(", "));
    } else {
        info!("[taxe] {name} : changement des taxes de {} refusé (code {result})", def.name);
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Propriétaire, versements
// ---------------------------------------------------------------------------------------------

/// Changement de propriétaire (siege/mod.rs) : taux du château remis à 0, changeables tout de suite ; op 4 à tous.
pub fn on_owner_change(ctx: &WorldCtx, st: &mut WorldState, id: u8) {
    for r in st.tax.regions.values_mut().filter(|r| r.castle == id) {
        r.ratio = 0;
    }
    st.tax.castles.entry(id).or_default().modified_at = 0;
    st.tax.dirty.push(id);
    broadcast_regions(ctx, st, id);
}

/// Crédite `amount` à la banque du compte du personnage `cid` (en jeu ou non) et l'inscrit au journal.
async fn credit(ctx: &WorldCtx, st: &mut WorldState, cid: u32, name: &str, amount: i64, action: &'static str, castle: u8) -> anyhow::Result<()> {
    if amount <= 0 {
        return Ok(());
    }
    let online = st.by_cid.get(&cid).and_then(|s| st.players.get(s)).map(|p| p.account.id);
    let account: u32 = match online {
        Some(a) => a,
        None => match ctx.db.characters.first("SELECT account_id FROM characters WHERE id = ?", (cid,)).await? {
            Some(r) => col(&r, "account_id"),
            None => return Ok(()),
        },
    };
    let max = super::bank::MAX_BANK_MONEY;
    ctx.db
        .characters
        .exec("INSERT IGNORE INTO account_bank (account_id, money) SELECT ?, COALESCE(SUM(bank_money), 0) FROM characters WHERE account_id = ?", (account, account))
        .await?;
    ctx.db.characters.exec("UPDATE account_bank SET money = LEAST(money + ?, ?) WHERE account_id = ?", (amount, max, account)).await?;
    // Joueur en jeu sur ce compte : banque en mémoire et au client (AGPMCHARACTER champ 9).
    if let Some(p) = st.players.values_mut().find(|p| p.account.id == account) {
        let bank = (p.bank.money as i64).saturating_add(amount).min(max);
        p.bank.money = bank as u64;
        p.ch.bank_money = bank as u64;
        let c = &schema::CHARACTER;
        p.send(c, Fields::new(c).with(0, 1).with(1, p.ch.id).with(9, bank));
    }
    crate::journal::bank_credit(&ctx.db, account, (cid, name), action, castle, amount);
    Ok(())
}

/// Versement des recettes d'un château (période close) ; rend le compte rendu.
pub async fn payout(ctx: &WorldCtx, st: &mut WorldState, id: u8) -> anyhow::Result<String> {
    let Some(def) = ctx.sieges.get(id).cloned() else { return Ok(String::new()) };
    let sum: i64 = st.tax.regions.values().filter(|r| r.castle == id).map(|r| r.income).sum();
    let master = st.siege.castles.get(&id).and_then(|c| c.owner).and_then(|g| st.guilds.by_id.get(&g)).and_then(|g| g.master()).map(|m| (m.cid, m.name.clone()));
    let lord = st.archlord.lord.clone().filter(|_| id != RENSPHERE);
    let (tribute, kept) = split_income(sum, lord.is_some());
    let now = unix_now();
    let report = match (&master, sum) {
        (_, 0) => format!("{} : aucune recette.", def.name),
        (None, _) => format!("{} : {sum} en attente (aucun propriétaire).", def.name),
        (Some((mcid, mname)), _) => {
            credit(ctx, st, *mcid, mname, kept, "taxe", id).await?;
            if let Some((lcid, lname)) = lord.as_ref().filter(|_| tribute > 0) {
                credit(ctx, st, *lcid, lname, tribute, "tribut", id).await?;
            }
            for r in st.tax.regions.values_mut().filter(|r| r.castle == id) {
                r.income = 0;
            }
            st.tax.castles.entry(id).or_default().total += kept;
            match lord.as_ref().filter(|_| tribute > 0) {
                Some((_, lname)) => format!("{} : {kept} versé(s) à {mname}, tribut de {tribute} à l'ArchLord {lname}.", def.name),
                None => format!("{} : {kept} versé(s) à {mname}.", def.name),
            }
        }
    };
    let c = st.tax.castles.entry(id).or_default();
    c.paid_at = now;
    c.since = now;
    save_castle(ctx, st, id).await?;
    broadcast_regions(ctx, st, id);
    info!("[taxe] versement : {report}");
    Ok(report)
}

/// Boucle des taxes (appelée par la boucle de combat) : enregistrements différés, versements de la semaine.
pub async fn step(ctx: &WorldCtx, st: &mut WorldState, now: Instant) -> anyhow::Result<()> {
    if st.tax.next_step.is_some_and(|t| now < t) {
        return Ok(());
    }
    st.tax.next_step = Some(now + STEP);
    for id in std::mem::take(&mut st.tax.dirty) {
        save_castle(ctx, st, id).await?;
    }
    let (w, unix) = (&ctx.config.world, unix_now());
    let mark = siege::from_local(w.time_zone, last_payout(siege::to_local(w.time_zone, unix), w.tax_payout_day, w.tax_payout_hour));
    let due: Vec<u8> = st.tax.castles.iter().filter(|(_, c)| c.since < mark).map(|(id, _)| *id).collect();
    for id in due {
        payout(ctx, st, id).await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Commandes MJ
// ---------------------------------------------------------------------------------------------

/// `/tax` : taux et recettes des châteaux (noms des régions dans la langue `lang`).
pub fn describe(ctx: &WorldCtx, st: &WorldState, lang: &str) -> String {
    let tz = ctx.config.world.time_zone;
    let mut lines = Vec::new();
    for d in &ctx.sieges.castles {
        let c = st.tax.castles.get(&d.id).cloned().unwrap_or_default();
        let regions: Vec<String> = regions_of(d)
            .into_iter()
            .map(|r| {
                let income = st.tax.regions.get(&r).map(|x| x.income).unwrap_or(0);
                format!("{} {} % ({income})", ctx.locale.region_name(r, lang), ratio_at(st, r))
            })
            .collect();
        let paid = if c.paid_at > 0 { siege::to_local(tz, c.paid_at).format("%Y-%m-%d %H:%M").to_string() } else { "-".into() };
        lines.push(format!("{} : {} ; total versé {}, dernier versement {paid}", d.name, regions.join(", "), c.total));
    }
    if lines.is_empty() { "Aucun château (table siege_war vide).".into() } else { lines.join("\n") }
}

/// `/tax pay [château]` : versement immédiat d'un château, ou de tous.
pub async fn force_payout(ctx: &WorldCtx, st: &mut WorldState, castle: Option<&str>) -> anyhow::Result<Result<String, String>> {
    let ids: Vec<u8> = match castle {
        Some(name) => match ctx.sieges.named(name) {
            Some(d) => vec![d.id],
            None => return Ok(Err(format!("château inconnu : {name}"))),
        },
        None => ctx.sieges.castles.iter().map(|d| d.id).collect(),
    };
    let mut out = Vec::new();
    for id in ids {
        out.push(payout(ctx, st, id).await?);
    }
    Ok(Ok(out.join("\n")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at(y: i32, m: u32, d: u32, h: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, 0, 0).unwrap()
    }

    #[test]
    fn formule_et_partage() {
        assert_eq!((tax_of(1000, 10), tax_of(199, 5), tax_of(99, 1), tax_of(1000, 0), tax_of(1000, -5)), (100, 9, 0, 0, 0));
        assert_eq!(tax_of(12_345, 50), 6172);
        assert_eq!(split_income(1001, true), (200, 801), "tribut de 20 %");
        assert_eq!(split_income(1001, false), (0, 1001), "sans ArchLord");
    }

    #[test]
    fn codes_du_changement() {
        assert_eq!(modify_result(false, &[10], true), tax_result::NOT_OWNER);
        assert_eq!(modify_result(true, &[10, 51], false), tax_result::OUT_OF_RANGE, "la plage avant la semaine");
        assert_eq!(modify_result(true, &[-1], true), tax_result::OUT_OF_RANGE);
        assert_eq!(modify_result(true, &[0, 50], false), tax_result::NOT_TIME);
        assert_eq!(modify_result(true, &[0, 50], true), tax_result::SUCCESS);
    }

    #[test]
    fn une_fois_par_semaine() {
        // 2026-10-28 est un mercredi.
        let wed = at(2026, 10, 28, 12);
        assert!(may_modify(None, wed), "jamais changés");
        assert!(!may_modify(Some(at(2026, 10, 26, 0)), wed), "déjà changés lundi");
        assert!(may_modify(Some(at(2026, 10, 25, 23)), wed), "dimanche dernier : semaine précédente");
        assert!(may_modify(Some(at(2025, 12, 31, 23)), at(2026, 1, 1, 1)), "autre année");
        assert!(may_modify(Some(at(2026, 10, 20, 12)), wed), "plus de six jours");
    }

    #[test]
    fn date_du_versement() {
        let wed = at(2026, 10, 28, 12);
        assert_eq!(last_payout(wed, 1, 0), at(2026, 10, 26, 0), "lundi 0 h de la semaine");
        assert_eq!(last_payout(at(2026, 10, 26, 0), 1, 0), at(2026, 10, 26, 0), "à l'instant même");
        assert_eq!(last_payout(at(2026, 10, 26, 3), 1, 6), at(2026, 10, 19, 6), "lundi 6 h pas encore passé");
        assert_eq!(last_payout(wed, 7, 20), at(2026, 10, 25, 20), "dimanche 20 h");
    }

    #[test]
    fn paquets() {
        use archcore_net::decode_packet;
        let c = CastleTax { total: 5000, paid_at: 1_761_000_000, modified_at: 1_760_000_000, since: 1 };
        let f = base(tax_op::VIEW, 7).with(2, 0).with(3, castle_mini("Inflame", &c)).with(4, region_mini("Inflame Castle", 300, 10));
        let p = decode_packet(&schema::TAX, &build_packet(&schema::TAX, &f)).unwrap();
        assert_eq!((p.int(0), p.int(1), p.int(2)), (Some(3), Some(7), Some(0)));
        let castle = decode_mini(&mini::TAX_CASTLE, p.bytes(3).unwrap()).unwrap();
        assert_eq!((castle.text(0).as_deref(), castle.int(1), castle.int(2), castle.int(3)), (Some("Inflame"), Some(5000), Some(1_761_000_000), Some(1_760_000_000)));
        let region = decode_mini(&mini::TAX_REGION, p.bytes(4).unwrap()).unwrap();
        assert_eq!((region.text(0).as_deref(), region.int(1), region.int(2)), (Some("Inflame Castle"), Some(300), Some(10)));
        assert!(p.bytes(5).is_none());
        // Demande du client : op 0 et EVENT_BASE de la fonction 34.
        let ev = crate::world::npcs::event_base(0x4000_0000 + 379, event_function::TAX);
        let req = decode_packet(&schema::TAX, &build_packet(&schema::TAX, &base(tax_op::REQUEST, 7).with(3, ev))).unwrap();
        let b = decode_mini(&mini::EVENT_BASE, req.bytes(3).unwrap()).unwrap();
        assert_eq!((b.int(1), b.int(3)), (Some(0x4000_0000 + 379), Some(34)));
    }
}
