//! Avis de recherche (AgpmWantedCriminal, 0x46 ; tableau chez les chefs de ville, fonction de PNJ 32).
//!
//! Droit de mise à prix (choix d'ArchCore, le client ne fait que l'afficher) : un joueur tué par un autre reçoit l'op 6
//! avec le nom de son tueur quand celui-ci l'a frappé le premier alors qu'il était innocent, ou quand le tueur est
//! meurtrier (palier 1 au moins) ; jamais dans un Battleground ni pendant un combat de guildes (ArchCore n'a pas de
//! siège). Le bouton « Wanted » de sa fenêtre de mort ouvre alors la mise à prix ; le droit dure jusqu'à la
//! résurrection (op 6 sans nom, qui éteint le bouton) ou jusqu'à une mise à prix réussie.
//!
//! Mise à prix (op 2, mini {nom, prime}) : nom du tueur exigé (sinon code 3), prime au moins égale à la prime de base
//! du poseur (`base_bounty`, barème d'Archonia selon son niveau ; sinon code 1), argent porté plus banque suffisant
//! (code 2) ; débit de l'argent porté d'abord, puis de la banque. Un seul avis par criminel : les primes s'ajoutent, et
//! chaque mise à prix repousse l'échéance (`Wanted.ExpireDays`). 100 avis au plus (le tableau du client : 10 pages de
//! 10) ; au-delà, refus (code 3 et message système). Le criminel en ligne reçoit l'op 7 (icône « recherché ») et
//! l'op 5 (rang, prime cumulée) ; de même à son entrée en jeu, et à chaque op 5 que son client envoie toutes les 30 s.
//!
//! Tableau (op 0 → op 1 près d'un PNJ de fonction 32 à 1600 unités au plus ; op 3 page 0 à 9 → op 4 par ligne puis
//! op 4 de page -100) : avis classés par prime cumulée décroissante.
//!
//! Capture : le joueur qui tue un criminel recherché, hors du groupe et de la guilde de celui-ci, touche la prime
//! cumulée (op 8 genre 4) ; le criminel perd `Wanted.ExpLossPercent` de l'expérience de son niveau sans descendre de
//! niveau (op 8 genre 5) et son avis disparaît (op 7 drapeau 0). Un criminel recherché n'est pas innocent : le tuer ne
//! donne ni points de meurtrier ni statut de premier attaquant.

use super::npc_events::{npc_distance, resolve};
use super::npcs::event_function;
use super::{send_system, set_money, WorldCtx, WorldState};
use crate::character::{factors_mini, unix_now};
use crate::db::{col, Databases};
use crate::{debug, info, warn};
use archcore_net::schema::{self, mini, wanted_notice, wanted_op, wanted_result};
use archcore_net::{build_mini, build_packet, decode_mini, Fields, Packet};
use std::collections::HashMap;

/// Avis au plus, lignes par page et pages du tableau du client.
pub const MAX_NOTICES: usize = 100;
pub const PAGE_SIZE: usize = 10;
pub const PAGES: i64 = 10;
/// Distance au PNJ du tableau (code partagé d'Archonia : 1600).
pub const BOARD_DISTANCE: f64 = 1600.0;

/// Prime de base selon le niveau du poseur (table d'Archonia `FUN_14028b7a0`).
pub fn base_bounty(level: i32) -> u64 {
    match level {
        ..=20 => 50_000,
        21..=40 => 100_000,
        41..=60 => 200_000,
        61..=80 => 400_000,
        _ => 1_000_000,
    }
}

/// Paiement de `amount` : argent porté d'abord, puis banque. Rend (porté, banque) après paiement, `None` si la somme
/// ne suffit pas.
pub fn split_payment(carried: u64, bank: u64, amount: u64) -> Option<(u64, u64)> {
    if carried.saturating_add(bank) < amount {
        return None;
    }
    let from_carried = carried.min(amount);
    Some((carried - from_carried, bank - (amount - from_carried)))
}

/// Expérience perdue par un criminel capturé : `percent` de l'expérience du niveau, sans descendre sous le début du
/// niveau (l'expérience se compte par niveau).
pub fn exp_loss(exp: u64, level_exp: u64, percent: f64) -> u64 {
    ((level_exp as f64 * percent / 100.0) as u64).min(exp)
}

/// Un avis.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Notice {
    pub criminal: u32,
    pub name: String,
    /// Prime cumulée.
    pub bounty: u64,
    /// Échéance et première mise à prix (Unix, s).
    pub expires_at: i64,
    pub since: i64,
    /// Niveau et modèle connus (relus en jeu si le criminel est connecté).
    pub level: i32,
    pub tid: u32,
}

/// Avis en cours, chargés au démarrage.
#[derive(Default)]
pub struct Board {
    pub notices: HashMap<u32, Notice>,
}

impl Board {
    pub async fn load(db: &Databases) -> Self {
        let mut board = Board::default();
        if let Err(e) = db.characters.exec("DELETE FROM wanted_criminal WHERE expires_at <= NOW()", ()).await {
            warn!("avis de recherche non chargés ({e:#}) : lancer `archcore db-init`");
            return board;
        }
        let sql = "SELECT w.criminal_id, w.bounty, UNIX_TIMESTAMP(w.expires_at) AS expires, UNIX_TIMESTAMP(w.created_at) AS since, \
                   c.name, c.level, c.template_id FROM wanted_criminal w JOIN characters c ON c.id = w.criminal_id WHERE c.deleted_at IS NULL";
        match db.characters.rows(sql, ()).await {
            Ok(rows) => {
                for r in &rows {
                    let n = Notice {
                        criminal: col(r, "criminal_id"),
                        name: col(r, "name"),
                        bounty: col(r, "bounty"),
                        expires_at: col::<Option<i64>>(r, "expires").unwrap_or(0),
                        since: col::<Option<i64>>(r, "since").unwrap_or(0),
                        level: col(r, "level"),
                        tid: col(r, "template_id"),
                    };
                    board.notices.insert(n.criminal, n);
                }
                info!("[recherche] {} avis de recherche en cours", board.notices.len());
            }
            Err(e) => warn!("avis de recherche non chargés : {e:#}"),
        }
        board
    }

    /// Avis en cours (non échu) d'un personnage.
    pub fn active(&self, cid: u32, now: i64) -> Option<&Notice> {
        self.notices.get(&cid).filter(|n| n.expires_at > now)
    }

    pub fn is_wanted(&self, cid: u32) -> bool {
        self.active(cid, unix_now()).is_some()
    }

    /// Avis en cours classés : prime décroissante, puis le plus ancien d'abord.
    pub fn ranking(&self, now: i64) -> Vec<&Notice> {
        let mut v: Vec<&Notice> = self.notices.values().filter(|n| n.expires_at > now).collect();
        v.sort_by(|a, b| b.bounty.cmp(&a.bounty).then(a.since.cmp(&b.since)).then(a.criminal.cmp(&b.criminal)));
        v
    }

    /// Rang (1 et plus) d'un criminel.
    pub fn rank_of(&self, cid: u32, now: i64) -> Option<usize> {
        self.ranking(now).iter().position(|n| n.criminal == cid).map(|i| i + 1)
    }

    /// Oublie les avis échus.
    pub fn purge(&mut self, now: i64) {
        self.notices.retain(|_, n| n.expires_at > now);
    }
}

/// Mini-paquet d'un avis.
pub fn notice_mini(name: &str, amount: u64, level: i32, tid: u32, online: bool) -> Vec<u8> {
    let m = &mini::WANTED_CRIMINAL;
    build_mini(m, &Fields::new(m).with(0, name).with(1, amount as i64).with(2, level).with(3, tid).with(4, online as i64))
}

/// Paquet AgpmWantedCriminal : op, CID et champs remplis par `f`.
pub fn packet(op: i64, cid: u32, f: impl FnOnce(Fields) -> Fields) -> Vec<u8> {
    let s = &schema::WANTED_CRIMINAL;
    build_packet(s, &f(Fields::new(s).with(0, op).with(1, cid)))
}

/// Mini réduit au nom et au montant (ops 2, 5, 6, 8).
fn short_mini(name: &str, amount: u64) -> Vec<u8> {
    let m = &mini::WANTED_CRIMINAL;
    build_mini(m, &Fields::new(m).with(0, name).with(1, amount as i64))
}

/// AgpmWantedCriminal reçu du client (ops 0, 2, 3 et 5 ; les autres sont réservées au serveur).
pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some(player) = st.players.get(&sid) else { return Ok(()) };
    let cid = player.ch.id;
    if p.int(1).is_some_and(|c| c != 0 && c != cid as i64) {
        warn!("[recherche] #{sid} CID {:?} usurpé", p.int(1));
        return Ok(());
    }
    match p.int(0) {
        Some(wanted_op::REQUEST) => {
            let base = p.bytes(5).and_then(|b| decode_mini(&mini::EVENT_BASE, b).ok()).unwrap_or_else(|| Fields::new(&mini::EVENT_BASE));
            let near = resolve(&ctx.npcs, &base).map(|(n, _)| n).filter(|n| {
                n.events.iter().any(|e| e.function == event_function::WANTED_CRIMINAL) && npc_distance(n, player.ch.pos_x, player.ch.pos_z) <= BOARD_DISTANCE
            });
            let Some(npc) = near else {
                warn!("[recherche] #{sid} {} : pas de tableau des avis à portée", player.ch.name);
                return Ok(());
            };
            debug!("[recherche] {} ouvre le tableau chez {}", player.ch.name, npc.name);
            player.send_raw(packet(wanted_op::GRANT, cid, |f| f.with(5, build_mini(&mini::EVENT_BASE, &base))));
        }
        Some(wanted_op::ARRANGE) => {
            let m = p.bytes(5).and_then(|b| decode_mini(&mini::WANTED_CRIMINAL, b).ok()).unwrap_or_else(|| Fields::new(&mini::WANTED_CRIMINAL));
            arrange(ctx, st, sid, &m.text(0).unwrap_or_default(), m.int(1).unwrap_or(0)).await?;
        }
        Some(wanted_op::PAGE) => send_page(st, sid, p.int(2).unwrap_or(0)),
        Some(wanted_op::MY_RANK) => send_rank(st, sid),
        op => debug!("[recherche] #{sid} op {op:?} ignorée"),
    }
    Ok(())
}

fn reply(st: &WorldState, sid: u32, code: i64, name: &str, amount: u64) {
    if let Some(p) = st.players.get(&sid) {
        p.send_raw(packet(wanted_op::ARRANGE, p.ch.id, |f| f.with(4, code).with(5, short_mini(name, amount))));
    }
}

/// Mise à prix du tueur `target` pour `amount` pièces.
async fn arrange(ctx: &WorldCtx, st: &mut WorldState, sid: u32, target: &str, amount: i64) -> anyhow::Result<()> {
    let now = unix_now();
    st.wanted.purge(now);
    let p = &st.players[&sid];
    let amount = amount.max(0) as u64;
    let Some((kcid, kname)) = p.wanted_killer.clone().filter(|(_, n)| n.eq_ignore_ascii_case(target.trim())) else {
        info!("[recherche] {} : mise à prix de {target} sans droit", p.ch.name);
        return Ok(reply(st, sid, wanted_result::ALREADY_ARRANGED, target, amount));
    };
    if amount < base_bounty(p.ch.level) {
        return Ok(reply(st, sid, wanted_result::NOT_ENOUGH_BASE, &kname, amount));
    }
    let Some((carried, bank)) = split_payment(p.ch.money, p.bank.money, amount) else {
        return Ok(reply(st, sid, wanted_result::NOT_ENOUGH_MONEY, &kname, amount));
    };
    if st.wanted.active(kcid, now).is_none() && st.wanted.ranking(now).len() >= MAX_NOTICES {
        send_system(st, sid, "Le tableau des avis de recherche est plein.");
        return Ok(reply(st, sid, wanted_result::ALREADY_ARRANGED, &kname, amount));
    }
    // Niveau et modèle du criminel : en jeu, sinon en base.
    let (level, tid) = match st.by_cid.get(&kcid).and_then(|s| st.players.get(s)) {
        Some(k) => (k.ch.level, k.ch.template_id),
        None => match ctx.db.characters.first("SELECT level, template_id FROM characters WHERE id = ?", (kcid,)).await? {
            Some(r) => (col(&r, "level"), col(&r, "template_id")),
            None => return Ok(reply(st, sid, wanted_result::ALREADY_ARRANGED, &kname, amount)),
        },
    };
    let p = &st.players[&sid];
    let (cid, account) = (p.ch.id, p.bank.account_id);
    let expires = now + ctx.config.world.wanted_expire_days * 86_400;
    ctx.db
        .characters
        .atomic(vec![
            (super::SAVE_MONEY, (carried, cid).into()),
            ("UPDATE account_bank SET money = ? WHERE account_id = ?", (bank, account).into()),
            ("DELETE FROM wanted_criminal WHERE criminal_id = ? AND expires_at <= NOW()", (kcid,).into()),
            (
                "INSERT INTO wanted_criminal (criminal_id, bounty, expires_at) VALUES (?, ?, FROM_UNIXTIME(?)) \
                 ON DUPLICATE KEY UPDATE bounty = bounty + VALUES(bounty), updated_at = NOW(), expires_at = VALUES(expires_at)",
                (kcid, amount, expires).into(),
            ),
            ("INSERT INTO wanted_bounty (criminal_id, placed_by, amount) VALUES (?, ?, ?)", (kcid, cid, amount).into()),
        ])
        .await?;
    let p = st.players.get_mut(&sid).unwrap();
    p.ch.money = carried;
    p.bank.money = bank;
    p.ch.bank_money = bank;
    p.wanted_killer = None;
    let c = &schema::CHARACTER;
    p.send(c, Fields::new(c).with(0, 1).with(1, cid).with(8, carried).with(9, bank));
    let who = p.ch.name.clone();
    let n = st.wanted.notices.entry(kcid).or_insert_with(|| Notice { criminal: kcid, since: now, ..Default::default() });
    n.name = kname.clone();
    n.bounty += amount;
    n.expires_at = expires;
    (n.level, n.tid) = (level, tid);
    let total = n.bounty;
    info!("[recherche] {who} met {kname} à prix pour {amount} pièce(s) (prime cumulée {total})");
    reply(st, sid, wanted_result::SUCCESS, &kname, amount);
    if let Some(&ksid) = st.by_cid.get(&kcid) {
        on_enter(st, ksid);
    }
    Ok(())
}

/// Op 4 : les lignes d'une page du tableau, puis la fin de page.
fn send_page(st: &WorldState, sid: u32, page: i64) {
    let Some(p) = st.players.get(&sid) else { return };
    let page = page.clamp(0, PAGES - 1);
    let cid = p.ch.id;
    let ranking = st.wanted.ranking(unix_now());
    for (i, n) in ranking.iter().enumerate().skip(page as usize * PAGE_SIZE).take(PAGE_SIZE) {
        let online = st.by_cid.get(&n.criminal).and_then(|s| st.players.get(s));
        let (level, tid) = online.map(|k| (k.ch.level, k.ch.template_id)).unwrap_or((n.level, n.tid));
        let m = notice_mini(&n.name, n.bounty, level, tid, online.is_some());
        p.send_raw(packet(wanted_op::ENTRY, cid, |f| f.with(2, page).with(3, i as i64 + 1).with(5, m)));
    }
    p.send_raw(packet(wanted_op::ENTRY, cid, |f| f.with(2, wanted_op::END_OF_PAGE)));
}

/// Op 5 : rang et prime cumulée du joueur, ou op 7 drapeau 0 s'il n'est plus recherché.
fn send_rank(st: &WorldState, sid: u32) {
    let Some(p) = st.players.get(&sid) else { return };
    let now = unix_now();
    let cid = p.ch.id;
    match (st.wanted.active(cid, now), st.wanted.rank_of(cid, now)) {
        (Some(n), Some(rank)) => p.send_raw(packet(wanted_op::MY_RANK, cid, |f| f.with(3, rank as i64).with(5, short_mini(&n.name, n.bounty)))),
        _ => p.send_raw(packet(wanted_op::WANTED, cid, |f| f.with(4, 0))),
    }
}

/// Entrée en jeu (ou nouvelle mise à prix) : un criminel recherché reçoit l'op 7 drapeau 1 puis son rang (op 5).
pub fn on_enter(st: &WorldState, sid: u32) {
    let Some(p) = st.players.get(&sid) else { return };
    if st.wanted.is_wanted(p.ch.id) {
        p.send_raw(packet(wanted_op::WANTED, p.ch.id, |f| f.with(4, 1)));
        send_rank(st, sid);
    }
}

/// Résurrection : le droit de mettre son tueur à prix s'éteint (op 6 sans nom).
pub fn on_resurrect(st: &mut WorldState, sid: u32) {
    let Some(p) = st.players.get_mut(&sid) else { return };
    if p.wanted_killer.take().is_some() {
        p.send_raw(packet(wanted_op::KILLER, p.ch.id, |f| f.with(5, short_mini("", 0))));
    }
}

/// Le joueur `ksid` a tué le joueur `vsid` (hors combat de guildes). `guilty` : ce meurtre ouvre à la victime le droit
/// de mettre son tueur à prix. Rend vrai si une prime a été touchée.
pub async fn on_kill(ctx: &WorldCtx, st: &mut WorldState, ksid: u32, vsid: u32, guilty: bool) -> anyhow::Result<bool> {
    let (Some(k), Some(v)) = (st.players.get(&ksid), st.players.get(&vsid)) else { return Ok(false) };
    let (kcid, vcid, kname, vname) = (k.ch.id, v.ch.id, k.ch.name.clone(), v.ch.name.clone());
    let now = unix_now();
    if let Some(notice) = st.wanted.active(vcid, now).cloned() {
        let same_party = st.parties.party_of(vcid).is_some_and(|p| p.members.contains(&kcid));
        let same_guild = st.guilds.of_cid.get(&vcid).is_some_and(|g| st.guilds.of_cid.get(&kcid) == Some(g));
        if same_party || same_guild {
            debug!("[recherche] {kname} tue {vname}, de son groupe ou de sa guilde : pas de prime");
            return Ok(false);
        }
        claim(ctx, st, (ksid, kcid, &kname), (vsid, vcid, &vname), notice.bounty).await?;
        return Ok(true);
    }
    if guilty {
        let v = st.players.get_mut(&vsid).unwrap();
        v.wanted_killer = Some((kcid, kname.clone()));
        v.send_raw(packet(wanted_op::KILLER, vcid, |f| f.with(5, short_mini(&kname, 0))));
        info!("[recherche] {vname} peut mettre {kname} à prix");
    }
    Ok(false)
}

/// Capture : prime au chasseur, expérience perdue par le criminel, avis effacé.
async fn claim(ctx: &WorldCtx, st: &mut WorldState, hunter: (u32, u32, &str), criminal: (u32, u32, &str), bounty: u64) -> anyhow::Result<()> {
    let (ksid, kcid, kname) = hunter;
    let (vsid, vcid, vname) = criminal;
    let money = st.players[&ksid].ch.money as i64 + bounty as i64;
    set_money(ctx, st, ksid, money).await?;
    if let Some(k) = st.players.get(&ksid) {
        k.send_raw(packet(wanted_op::NOTIFY, kcid, |f| f.with(4, wanted_notice::KILL).with(5, short_mini(vname, bounty))));
    }
    let v = st.players.get_mut(&vsid).unwrap();
    let need = ctx.level_exp.get(v.ch.level.max(0) as usize).copied().unwrap_or(0);
    let loss = exp_loss(v.ch.exp, need, ctx.config.world.wanted_exp_loss_percent);
    if loss > 0 {
        v.ch.exp -= loss;
        v.factors.point[3] = (v.ch.exp % (1u64 << 32)) as u32 as i32;
        v.factors.point[4] = (v.ch.exp >> 32) as i32;
        let c = &schema::CHARACTER;
        v.send(c, Fields::new(c).with(0, 1).with(1, vcid).with(7, factors_mini(&v.factors)));
    }
    let exp = v.ch.exp;
    v.send_raw(packet(wanted_op::NOTIFY, vcid, |f| f.with(4, wanted_notice::DEAD).with(5, short_mini(kname, loss))));
    v.send_raw(packet(wanted_op::WANTED, vcid, |f| f.with(4, 0)));
    st.wanted.notices.remove(&vcid);
    ctx.db
        .characters
        .atomic(vec![
            ("UPDATE characters SET exp = ? WHERE id = ?", (exp, vcid).into()),
            ("DELETE FROM wanted_criminal WHERE criminal_id = ?", (vcid,).into()),
            ("UPDATE wanted_bounty SET claimed_by = ?, claimed_at = NOW() WHERE criminal_id = ? AND claimed_by IS NULL", (kcid, vcid).into()),
        ])
        .await?;
    info!("[recherche] {kname} capture {vname} : prime de {bounty} pièce(s), {loss} point(s) d'expérience perdus");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use archcore_net::decode_packet;

    #[test]
    fn prime_de_base() {
        assert_eq!([1, 20, 21, 40, 41, 60, 61, 80, 81, 99].map(base_bounty), [50_000, 50_000, 100_000, 100_000, 200_000, 200_000, 400_000, 400_000, 1_000_000, 1_000_000]);
    }

    #[test]
    fn paiement_porte_puis_banque() {
        assert_eq!(split_payment(80_000, 0, 50_000), Some((30_000, 0)));
        assert_eq!(split_payment(30_000, 100_000, 50_000), Some((0, 80_000)));
        assert_eq!(split_payment(30_000, 10_000, 50_000), None);
        assert_eq!(split_payment(0, 50_000, 50_000), Some((0, 0)));
    }

    #[test]
    fn perte_d_experience() {
        assert_eq!(exp_loss(10_000, 40_000, 5.0), 2_000);
        assert_eq!(exp_loss(500, 40_000, 5.0), 500, "jamais sous le début du niveau");
        assert_eq!(exp_loss(0, 40_000, 5.0), 0);
        assert_eq!(exp_loss(10_000, 40_000, 0.0), 0);
    }

    #[test]
    fn classement() {
        let mut b = Board::default();
        for (cid, bounty, since) in [(1, 50_000, 10), (2, 200_000, 20), (3, 50_000, 5), (4, 900_000, 1)] {
            b.notices.insert(cid, Notice { criminal: cid, bounty, since, expires_at: if cid == 4 { 50 } else { 1000 }, ..Default::default() });
        }
        assert_eq!(b.ranking(100).iter().map(|n| n.criminal).collect::<Vec<_>>(), vec![2, 3, 1], "échu exclu, égalité : le plus ancien");
        assert_eq!((b.rank_of(1, 100), b.rank_of(4, 100)), (Some(3), None));
        b.purge(100);
        assert!(!b.notices.contains_key(&4));
    }

    #[test]
    fn paquets() {
        let s = &schema::WANTED_CRIMINAL;
        let line = decode_packet(s, &packet(wanted_op::ENTRY, 7, |f| f.with(2, 0).with(3, 1).with(5, notice_mini("GmTest", 60_000, 20, 96, true)))).unwrap();
        assert_eq!((line.int(0), line.int(1), line.int(2), line.int(3)), (Some(4), Some(7), Some(0), Some(1)));
        let m = decode_mini(&mini::WANTED_CRIMINAL, line.bytes(5).unwrap()).unwrap();
        assert_eq!((m.text(0).as_deref(), m.int(1), m.int(2), m.int(3), m.int(4)), (Some("GmTest"), Some(60_000), Some(20), Some(96), Some(1)));
        let end = decode_packet(s, &packet(wanted_op::ENTRY, 7, |f| f.with(2, wanted_op::END_OF_PAGE))).unwrap();
        assert_eq!((end.int(2), end.bytes(5)), (Some(-100), None));
        let notify = decode_packet(s, &packet(wanted_op::NOTIFY, 7, |f| f.with(4, wanted_notice::KILL).with(5, short_mini("GmTest", 60_000)))).unwrap();
        assert_eq!(notify.int(4), Some(4));
    }
}
