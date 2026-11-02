//! Consommables à compétence, résurrection par objet, oubli des compétences et perte d'expérience à la mort.
//!
//! ```text
//!   Consommables à compétence (AGPMITEM op 3, objets d'« ItemType » 1 qui ont un « Use skill id ») : ragoûts, potions
//!             de vitesse, pierres, ailes, charmes (usage 12), costumes (31), talismans de Janus (37)… Chaque compétence
//!             de la liste (« Use skill id » au niveau « Use skill level », listes « ; ») pose son effet sur le joueur
//!             (AGPMSKILL op 4), sans incantation. Durée : `duration` du niveau, sinon « Apply Effect Count » ×
//!             « Apply Effect Time », sinon la durée de l'objet (« RemainTime » ou « ExpireTime », en minutes), sinon
//!             24 h (objet « allumé » sans durée : Sharon Stone).
//!             Recharge : « Use Interval » + « Apply Effect Count » × « Apply Effect Time » par TID d'objet (ce qu'affiche
//!             la barre d'Archonia) et `recast_time` du niveau par TID de compétence (420 s aux ragoûts) ; refus : op 5.
//!             Familles (`skill_classify_id`, `skill_classify_level` de SKILL_CONST) : un effet de la même famille est
//!             remplacé si le nouveau est de rang égal ou supérieur, sinon l'objet est refusé.
//!             Un exemplaire est consommé, sauf objet « CanStopUsingItem » (familiers à part, `summons.rs`) : il reste,
//!             et le réutiliser arrête son effet. Confirmation : op 4 (TID, CID, 18 = recharge en ms).
//!             Bonus d'expérience et de butin (« bonus_exp », « bonus_drop_rate » des effets portés) : en pour cent,
//!             valeur / `Skill.GameBonusDivisor` (10), sur l'expérience des monstres et le taux de butin du tueur.
//!   Orbe de résurrection (usage 14), par le mort seulement (vivant : op 5, « Regeneration has failed ») ; refus par
//!             op 27 code 3 (« You cannot use the resurrection item. ») pour l'ArchLord, en zone de siège en guerre,
//!             dans une région sans le bit 0x100 du Type (Battle Square, prison…), en région de combat (sûreté 2) avec
//!             un orbe qui n'est pas Cash, hors des niveaux de l'orbe (« Level » 1;30) ou pendant sa recharge (op 18
//!             d'abord). Sinon : un exemplaire consommé, statut normal sur place, PV et PM pleins, invincible 10 s
//!             (comme au village), AGPMCHARACTER op 1 autour, op 4 (3 TID, 4 CID, 18 recharge) puis op 18 (4 CID,
//!             18 recharge restante) ; recharge = « Use Interval » (90 s, 30 min), sinon 15 min. Aucune perte d'expérience.
//!   Oubli des compétences : Gem of origin (usage 27, op 3 ; un exemplaire consommé, autant de fois qu'on veut) ou
//!             maître des compétences (AgpmEventSkillMaster op 8, champ 2 = CID, sans événement → op 9, résultat au
//!             champ 5 : 0 réussi, 1 échec, 2 pas de place pour le remboursement), une seule fois : la chaîne
//!             « skill init » (AGPMCHARACTER champ 19, `characters.skill_init`) passe à « 1 » (op 23 au joueur).
//!             Oubliées : toutes les compétences apprises sauf l'attaque normale, celles de l'ArchLord et celles de
//!             production ; AGPMSKILL op 1 pour chacune, points de compétence et héroïques recalculés. Le maître rend
//!             aussi l'or des niveaux appris (`skill_cost` puis `skill_upgrade_cost` de SKILL_CONST, sans les taxes) :
//!             code 2 si l'argent dépasserait le plafond.
//!   Perte d'expérience : à la résurrection au village (AGPMCHARACTER op 9), `Death.ExpPenaltyPercent` de l'expérience du
//!             niveau (`Death.ExpPenaltyRoguePercent` pour un meurtrier de palier 3), jamais au point de perdre un
//!             niveau, rien sous le niveau 6 ni après une mort en combat entre joueurs. Désactivée par défaut (0) :
//!             Archonia masque ces textes (`IsExpPenalty` rend 0) ; l'épisode 8 annonçait 3 % et 10 %.
//! ```
//!   Return Orb (usage 13) : AGPMSKILL op 13 (champ 1 = identifiant de la compétence, champ 2 = propriétaire ; pas d'IID
//!             d'orbe, le serveur prend le premier objet d'usage 13 de l'inventaire, puis de l'inventaire Cash). Refus (message
//!             système) : mort, évolution du Scion, compétence qui ne s'oublie pas (comme ci-dessous), prérequis d'une
//!             autre compétence apprise (condition de son arbre de maîtrise remplie avant, plus après), aucune orbe. Sinon
//!             la compétence (tous ses niveaux) est retirée (op 1), les points (et points héroïques) recalculés, une orbe
//!             consommée (op 4) ; pas d'or rendu (le texte de l'objet ne parle que des points). L'op 3 sur l'orbe ne fait
//!             rien : le bouton « Reset Selected » de la fenêtre des compétences envoie l'op 13.
//!   Objets allumés de l'inventaire Cash sans compétence (Fortune Manual 25, Talisman of Protection 19) : l'op 3 les
//!             allume (un seul par usage) ou les éteint, l'op 23 les éteint ; AGPMITEM op 26 (mini Cash « en cours
//!             d'utilisation »), état non enregistré. Le Fortune Manual sert aux conversions (`convert.rs`). Le talisman
//!             reste sans effet : ArchCore ne fait rien perdre à la mort, comme les données d'origine (pvpitemdroptable.txt
//!             à 0 partout, réglage « bPCDropItemOnDeath » à 0, texte « Holding items will not be dropped even in case of
//!             PK death » à l'entrée des zones de combat) ; les messages PvP 8 et 23 ne partent donc jamais.
//!   Feux d'artifice (usage 22, `FIREWORKS`) : un exemplaire consommé, recharge « Use Interval », op 4 (TID, CID, 18
//!             recharge) au joueur et à ceux qui le voient (le client joue l'effet de l'objet). Les autres objets de
//!             l'usage 22 sont des paquets de la boutique : sans effet.
//!   Commandes de discussion (`chat_command`), seulement avec l'objet (inventaire ou inventaire Cash), hors recharge
//!             (« Use Interval », 30 min, par TID) : `/go Nom` (Gate of Phantom, Jumper's Illusion : téléporté près d'un
//!             joueur en jeu), `/safekeeper` (Safe Keeper 4923 : banque ouverte par le gardien le plus proche, à distance)
//!             et `/merchant` (Merchant 4924 : boutique du marchand le plus proche, à distance) ; refus du mort, sous
//!             métamorphose et là où le rappel de groupe est refusé (`party::recall_refusal`, pour la cible de `/go`
//!             aussi). `/nick texte` (Signature of Glory) : signature de 16 octets au plus (AGPMCHARACTER champ 27,
//!             `characters.signature`), envoyée au joueur et à ceux qui le voient (op 1), puis avec le personnage ;
//!             `/nick` seul l'efface. Un exemplaire consommé, op 4 et « Objet … utilisé » (texte `JumpItemUsed`).
//! Choix d'ArchCore (règles du serveur d'origine inconnues) : l'or rendu, les compétences gardées, l'unité des bonus,
//! les effets d'objet perdus à la mort et à la déconnexion comme les autres effets, l'objet à durée qui ne s'use pas
//! (son temps de vie, « RemainTime » / « ExpireTime » de l'exemplaire, n'est pas décompté).

use super::items::{item_remove, InventoryItem};
use super::skills::{self, Owned, SkillCatalog};
use super::{send_system, Player, WorldCtx, WorldState};
use crate::{debug, info};
use archcore_net::schema;
use archcore_net::{build_packet, Fields};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Types d'usage (« Type » d'un objet d'« ItemType » 1) traités ici.
pub const USABLE_REVERSE_ORB: i64 = 14;
pub const USABLE_INIT_SKILL: i64 = super::morph::USABLE_INIT_SKILL;
/// Return Orb, Bead of Returning (« SKILLROLLBACK_SCROLL ») : une compétence oubliée (AGPMSKILL op 13).
pub const USABLE_SKILL_ROLLBACK: i64 = 13;
/// Talisman of Protection (« ANTI_DROP ») : objet Cash allumé, sans effet (voir l'en-tête).
pub const USABLE_ANTI_DROP: i64 = 19;
/// Secret Safe (« ADD_BANK_SLOT ») : page de banque en plus (`bank.rs`).
pub const USABLE_ADD_BANK_SLOT: i64 = 21;
/// Feux d'artifice et paquets de la boutique (« USE_EFFECT »).
pub const USABLE_USE_EFFECT: i64 = 22;
/// Gate of Phantom, Jumper's Illusion, Safe Keeper, Merchant (« JUMP ») : commandes `/go`, `/safekeeper`, `/merchant`.
pub const USABLE_JUMP: i64 = 24;
/// Fortune Manual (« LUCKY_SCROLL ») : objet Cash allumé, bonus aux conversions (`convert.rs`).
pub const USABLE_LUCKY_SCROLL: i64 = 25;
/// Signature of Glory (« NICK ») : commande `/nick`.
pub const USABLE_NICK: i64 = 26;
/// Release of Agreement (« SOCKET_INIT ») : pierres serties retirées (AGPMITEMCONVERT op 12, `convert.rs`).
pub const USABLE_SOCKET_INIT: i64 = 28;
/// Marvel Scroll of Release (« PRIVATE_TRADE_OPTION ») : grille Ex de l'échange (`trade.rs`).
pub const USABLE_PRIVATE_TRADE_OPTION: i64 = 35;
/// Types d'usage sans compétence qu'`ItemUse` retient quand même.
const USABLE_WITHOUT_SKILL: [i64; 12] = [
    USABLE_REVERSE_ORB,
    USABLE_INIT_SKILL,
    super::party::USABLE_RECALL_PARTY,
    USABLE_SKILL_ROLLBACK,
    USABLE_ANTI_DROP,
    USABLE_ADD_BANK_SLOT,
    USABLE_USE_EFFECT,
    USABLE_JUMP,
    USABLE_LUCKY_SCROLL,
    USABLE_NICK,
    USABLE_SOCKET_INIT,
    USABLE_PRIVATE_TRADE_OPTION,
];
/// Feux d'artifice de l'usage 22 (Small firecracker, Middle Fireworks Shine 1-3, Unique Fireworks Heart, Bead of Love) ;
/// les autres objets de l'usage 22 sont des paquets de la boutique, laissés sans effet (14720 et 65462 sont d'Archonia).
pub const FIREWORKS: [u32; 11] = [4606, 6261, 6262, 6263, 6264, 6478, 6479, 6480, 6481, 9024, 11004];
/// Objets de l'usage 24 propres à `/safekeeper` et `/merchant` ; les autres servent à `/go`.
pub const SAFE_KEEPER_ITEM: u32 = 4923;
pub const MERCHANT_ITEM: u32 = 4924;
/// Signature (AGPMCHARACTER champ 27, CHAR[16]) : octets au plus.
pub const SIGNATURE_MAX_BYTES: usize = 16;

/// Opérations AGPMITEM propres à ce module.
pub mod item_op {
    pub const USE_ITEM_FAILED_BY_TID: i64 = 5;
    pub const UPDATE_REUSE_TIME_FOR_REVERSE_ORB: i64 = 18;
}

/// Code d'AGPMITEM op 27 (champ 1) : « ReverseOrbError ».
pub const REVERSE_ORB_ERROR: i64 = 3;
/// Recharge d'un orbe sans « Use Interval » : celle du client (15 min).
pub const DEFAULT_ORB_REUSE: Duration = Duration::from_millis(900_000);
/// Bit « résurrection permise » du Type d'une région (RegionTemplate.ini).
pub const REGION_RESURRECTION: u32 = 0x100;
/// Sûreté d'une région de combat, où seul un orbe Cash sert.
pub const SAFETY_DANGER: i64 = 2;
/// Durée de l'effet d'un objet « allumé » qui n'en donne aucune.
pub const LIT_EFFECT_MS: u64 = 24 * 3600 * 1000;

/// AGPMCHARACTER op 23 : chaîne « skill init » mise à jour (champ 19).
pub const UPDATE_SKILLINIT_STRING: i64 = 23;
/// Chaîne « skill init » après l'oubli chez le maître.
pub const SKILL_INIT_DONE: &str = "1";
/// AGPMSKILL op 1 : compétence retirée.
pub const SKILL_REMOVE: i64 = 1;

/// Opérations d'AgpmEventSkillMaster de l'oubli des compétences.
pub mod master_op {
    pub const SKILL_INITIALIZE: i64 = 8;
    pub const SKILL_INITIALIZE_RESULT: i64 = 9;
}

/// Résultats de l'op 9 (champ 5).
pub mod init_result {
    pub const SUCCESS: i64 = 0;
    pub const FAIL: i64 = 1;
    pub const NO_ROOM: i64 = 2;
}

/// Pas de perte d'expérience sous ce niveau (le client ne demande pas de confirmation en dessous, déduit).
pub const EXP_PENALTY_MIN_LEVEL: i32 = 6;

/// Ce qu'un consommable fait à l'utilisation (colonnes d'itemdatatable.txt).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemUse {
    /// « Type » (12 compétence, 14 orbe, 27 oubli des compétences, 31 costume, 37 talisman…).
    pub usable_type: i64,
    /// Compétences lancées : (TID, niveau), listes « Use skill id » et « Use skill level ».
    pub skills: Vec<(u32, u32)>,
    /// « CanStopUsingItem » : l'objet reste, le réutiliser arrête son effet.
    pub can_stop: bool,
    /// Recharge (ms) : « Use Interval » + « Apply Effect Count » × « Apply Effect Time ».
    pub reuse_ms: u64,
    /// « Apply Effect Count » × « Apply Effect Time » (ms).
    pub effect_ms: u64,
    /// Durée de l'objet (« RemainTime », sinon « ExpireTime », minutes), en ms.
    pub lifetime_ms: u64,
    /// « Level » : niveaux admis (minimum, maximum ; 0 : sans borne).
    pub level: (i32, i32),
    /// Objet de la boutique (« Cash » non nul, moins 100 au-delà de 100).
    pub cash: bool,
}

impl ItemUse {
    /// Lit une ligne d'itemdatatable.txt : consommable (« ItemType » 1) à compétence, orbe ou objet d'oubli.
    pub fn from_raw(raw: &serde_json::Value) -> Option<Self> {
        let text = |k: &str| raw.get(k).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let num = |k: &str| text(k).split(';').next().and_then(|t| t.trim().parse::<f64>().ok()).unwrap_or(0.0);
        let list = |k: &str| text(k).split(';').map(|t| t.trim().parse::<f64>().map(|v| v as i64).unwrap_or(0)).collect::<Vec<_>>();
        if num("ItemType") as i64 != 1 {
            return None;
        }
        let usable_type = num("Type") as i64;
        let levels = list("Use skill level");
        let skills: Vec<(u32, u32)> = list("Use skill id")
            .iter()
            .enumerate()
            .filter(|(_, t)| **t > 0)
            .map(|(i, t)| (*t as u32, levels.get(i).or(levels.first()).copied().unwrap_or(1).max(1) as u32))
            .collect();
        if skills.is_empty() && !USABLE_WITHOUT_SKILL.contains(&usable_type) {
            return None;
        }
        let ms = |k: &str| num(k).max(0.0) as u64;
        let effect_ms = ms("Apply Effect Count") * ms("Apply Effect Time");
        let minutes = if ms("RemainTime") > 0 { ms("RemainTime") } else { ms("ExpireTime") };
        let cash = num("Cash") as i64;
        // Niveaux admis : « Level », sinon (orbes de rappel) « SuitableLevelMin » et « SuitableLevelMax ».
        let bounds = match list("Level") {
            l if l.iter().any(|v| *v > 0) => l,
            _ => vec![num("SuitableLevelMin") as i64, num("SuitableLevelMax") as i64],
        };
        Some(ItemUse {
            usable_type,
            skills,
            can_stop: num("CanStopUsingItem") != 0.0,
            reuse_ms: ms("Use Interval") + effect_ms,
            effect_ms,
            lifetime_ms: minutes * 60_000,
            level: (bounds.first().copied().unwrap_or(0) as i32, bounds.get(1).copied().unwrap_or(0) as i32),
            cash: (if cash >= 100 { cash - 100 } else { cash }) != 0,
        })
    }

    /// Niveau admis (« 1;30 » : 1 à 30 ; « 20 » : 20 et plus ; vide : tous).
    pub fn level_ok(&self, level: i32) -> bool {
        let (min, max) = self.level;
        level >= min && (max == 0 || level <= max)
    }
}

/// Recharges des consommables, objets allumés et nature de la dernière mort, pour un joueur.
#[derive(Clone, Debug, Default)]
pub struct Uses {
    /// Fin de la recharge de chaque objet (TID) et de chaque compétence d'objet (TID).
    pub item_ready: HashMap<u32, Instant>,
    pub skill_ready: HashMap<u32, Instant>,
    /// Fin de la recharge des orbes de résurrection (commune, comme dans le client).
    pub orb_ready: Option<Instant>,
    /// Objets « CanStopUsingItem » allumés : IID → TID des compétences posées.
    pub lit: HashMap<u32, Vec<u32>>,
    /// Dernière mort en combat entre joueurs (pas de perte d'expérience au village).
    pub pk_death: bool,
}

impl Uses {
    /// Recharge restante de l'objet `tid` ou de l'une de ses compétences.
    pub fn left(&self, tid: u32, skills: &[(u32, u32)], now: Instant) -> Option<Duration> {
        let item = self.item_ready.get(&tid).copied();
        let skill = skills.iter().filter_map(|(s, _)| self.skill_ready.get(s).copied()).max();
        item.into_iter().chain(skill).max().filter(|t| *t > now).map(|t| t - now)
    }

    /// Commande MJ /recast : plus aucune recharge.
    pub fn clear(&mut self) {
        self.item_ready.clear();
        self.skill_ready.clear();
        self.orb_ready = None;
    }
}

/// Familles : TID des effets portés à remplacer par un effet de famille `new` (id, rang), ou `Err` si l'un est plus
/// fort. Un effet sans famille (id 0) ne remplace que lui-même (`effects::push`).
pub fn family_clash(worn: &[(u32, (i32, i32))], new_tid: u32, new: (i32, i32)) -> Result<Vec<u32>, u32> {
    if new.0 == 0 {
        return Ok(Vec::new());
    }
    let same: Vec<(u32, i32)> = worn.iter().filter(|(t, c)| *t != new_tid && c.0 == new.0).map(|(t, c)| (*t, c.1)).collect();
    match same.iter().find(|(_, rank)| *rank > new.1) {
        Some((t, _)) => Err(*t),
        None => Ok(same.into_iter().map(|(t, _)| t).collect()),
    }
}

/// Durée de l'effet posé par un objet : celle du niveau, sinon celle de l'objet, sinon `LIT_EFFECT_MS`.
pub fn effect_duration_ms(level_ms: u64, u: &ItemUse) -> u64 {
    [level_ms, u.effect_ms, u.lifetime_ms].into_iter().find(|ms| *ms > 0).unwrap_or(LIT_EFFECT_MS)
}

/// Bonus en pour cent d'une valeur brute de SKILL_CONST.
pub fn bonus_pct(raw: f64, divisor: f64) -> f64 {
    (raw / divisor.max(0.01)).max(0.0)
}

/// Bonus d'expérience (pour cent) des effets d'un joueur.
pub fn exp_bonus_pct(ctx: &WorldCtx, p: &Player) -> f64 {
    bonus_pct(super::effects::total(&p.skills.effects).exp_bonus, ctx.config.world.skill_game_bonus_divisor)
}

/// Bonus de butin (pour cent) des effets d'un joueur.
pub fn drop_bonus_pct(ctx: &WorldCtx, p: &Player) -> f64 {
    bonus_pct(super::effects::total(&p.skills.effects).drop_bonus, ctx.config.world.skill_game_bonus_divisor)
}

/// Expérience d'un monstre majorée du bonus des effets du joueur.
pub fn with_exp_bonus(ctx: &WorldCtx, st: &WorldState, sid: u32, gain: u64) -> u64 {
    let pct = st.players.get(&sid).map(|p| exp_bonus_pct(ctx, p)).unwrap_or(0.0);
    (gain as f64 * (100.0 + pct) / 100.0).round() as u64
}

// ---------------------------------------------------------------------------------------------
// Paquets
// ---------------------------------------------------------------------------------------------

/// AGPMITEM op 5 : utilisation refusée (orbe : « Regeneration has failed »).
pub fn failed_packet(tid: u32, cid: u32) -> Vec<u8> {
    let s = &schema::ITEM;
    build_packet(s, &Fields::new(s).with(0, item_op::USE_ITEM_FAILED_BY_TID).with(3, tid).with(4, cid))
}

/// AGPMITEM op 18 : recharge restante des orbes de résurrection (ms).
pub fn orb_reuse_packet(cid: u32, left: Duration) -> Vec<u8> {
    let s = &schema::ITEM;
    build_packet(s, &Fields::new(s).with(0, item_op::UPDATE_REUSE_TIME_FOR_REVERSE_ORB).with(4, cid).with(18, left.as_millis() as u64))
}

/// AGPMCHARACTER op 23 : chaîne « skill init » du personnage.
pub fn skill_init_packet(cid: u32, value: &str) -> Vec<u8> {
    let c = &schema::CHARACTER;
    build_packet(c, &Fields::new(c).with(0, UPDATE_SKILLINIT_STRING).with(1, cid).with(19, value))
}

/// AgpmEventSkillMaster op 9 : résultat de l'oubli des compétences (champ 5).
pub fn init_result_packet(cid: u32, result: i64) -> Vec<u8> {
    let s = &schema::EVENT_SKILL_MASTER;
    build_packet(s, &Fields::new(s).with(0, master_op::SKILL_INITIALIZE_RESULT).with(2, cid).with(5, result))
}

/// Un exemplaire de l'objet consommé : quantité (op 2) ou retrait (op 1) envoyés.
async fn consume_one(ctx: &WorldCtx, p: &mut Player, iid: u32) -> anyhow::Result<()> {
    let cid = p.ch.id;
    match p.inventory.consume(&ctx.db, iid, 1).await? {
        Some(left) => p.send_raw(p.inventory.count_packet(&left, cid)),
        None => p.send_raw(item_remove(iid, Some(cid))),
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Consommables à compétence
// ---------------------------------------------------------------------------------------------

/// Objet à compétence utilisé (AGPMITEM op 3) : effets posés, recharge, op 4.
pub async fn use_skill_item(ctx: &WorldCtx, st: &mut WorldState, sid: u32, item: &InventoryItem, u: &ItemUse, now: Instant) -> anyhow::Result<()> {
    let Some(p) = st.players.get_mut(&sid) else { return Ok(()) };
    let (cid, name, caster_tid) = (p.ch.id, p.ch.name.clone(), p.ch.template_id);
    let label = ctx.catalog.get(item.tid).map(|d| d.name.clone()).unwrap_or_else(|| format!("objet {}", item.tid));
    // Objet allumé : le réutiliser éteint ses effets.
    if u.can_stop {
        if let Some(tids) = p.uses.lit.remove(&item.iid) {
            let gone: Vec<super::effects::Effect> = p.skills.effects.iter().filter(|e| e.ends_at.is_some() && tids.contains(&e.tid)).cloned().collect();
            if !gone.is_empty() {
                p.skills.effects.retain(|e| !(e.ends_at.is_some() && tids.contains(&e.tid)));
                for e in &gone {
                    super::combat::send_around(st, sid, &super::effects::remove_packet(cid, e));
                }
                super::equipment::refresh_factors(ctx, st, sid);
                info!("[objet] {name} éteint {label}");
                return Ok(());
            }
        }
    }
    let p = st.players.get_mut(&sid).unwrap();
    if let Some(left) = p.uses.left(item.tid, &u.skills, now) {
        debug!("[objet] {name} : {label} en recharge ({} ms)", left.as_millis());
        p.send_raw(failed_packet(item.tid, cid));
        send_system(st, sid, &format!("{label} n'est pas encore prêt ({} s).", left.as_secs().max(1)));
        return Ok(());
    }
    // Niveaux des compétences, et familles des effets déjà portés.
    let levels: Vec<(&skills::SkillTemplate, skills::SkillLevel, u32)> = u
        .skills
        .iter()
        .filter_map(|(tid, level)| {
            let t = ctx.skills.get(*tid)?;
            let n = if t.level(*level).is_some() { *level } else { 1 };
            Some((t, t.level(n)?.clone(), n))
        })
        .collect();
    if levels.is_empty() {
        debug!("[objet] {name} utilise {label} : compétence inconnue");
        return Ok(());
    }
    let worn: Vec<(u32, (i32, i32))> = p.skills.effects.iter().filter(|e| e.ends_at.is_some()).filter_map(|e| Some((e.tid, ctx.skills.get(e.tid)?.level(e.level)?.classify))).collect();
    let mut replaced = Vec::new();
    for (t, lv, _) in &levels {
        match family_clash(&worn, t.tid, lv.classify) {
            Ok(tids) => replaced.extend(tids),
            Err(stronger) => {
                let other = ctx.skills.get(stronger).map(|t| t.name.as_str()).unwrap_or("?");
                info!("[objet] {name} : {label} refusé, effet plus fort de la même famille ({other})");
                p.send_raw(failed_packet(item.tid, cid));
                send_system(st, sid, &format!("Un effet plus fort de la même famille est déjà actif ({other})."));
                return Ok(());
            }
        }
    }
    // Effets remplacés de la même famille.
    let gone: Vec<super::effects::Effect> = p.skills.effects.iter().filter(|e| e.ends_at.is_some() && replaced.contains(&e.tid)).cloned().collect();
    p.skills.effects.retain(|e| !(e.ends_at.is_some() && replaced.contains(&e.tid)));
    for e in &gone {
        super::combat::send_around(st, sid, &super::effects::remove_packet(cid, e));
    }
    let mut recast = 0u64;
    let mut posed = Vec::new();
    for (t, lv, n) in levels {
        st.next_skill_id += 1;
        let owned = Owned { id: st.next_skill_id, tid: t.tid, level: n, ready_at: None, exp: 0 };
        let lasting = skills::SkillLevel { duration_ms: effect_duration_ms(lv.duration_ms, u), ..lv.clone() };
        let e = skills::new_effect(&owned, &lasting, caster_tid, lv.status, None, now);
        if !lv.heal.is_empty() {
            if let Some(p) = st.players.get_mut(&sid) {
                super::effects::heal(&mut p.factors, &lv.heal);
            }
        }
        skills::add_player_effect(ctx, st, sid, e);
        recast = recast.max(lv.recast_ms);
        let p = st.players.get_mut(&sid).unwrap();
        if lv.recast_ms > 0 {
            p.uses.skill_ready.insert(t.tid, now + Duration::from_millis(lv.recast_ms));
        }
        posed.push(t.tid);
    }
    skills::sync_player_status(st, sid, now);
    let p = st.players.get_mut(&sid).unwrap();
    if u.reuse_ms > 0 {
        p.uses.item_ready.insert(item.tid, now + Duration::from_millis(u.reuse_ms));
    }
    if u.can_stop {
        p.uses.lit.insert(item.iid, posed.clone());
    } else {
        consume_one(ctx, p, item.iid).await?;
    }
    p.send_raw(super::recovery::used_packet(item.tid, cid, Duration::from_millis(u.reuse_ms.max(recast))));
    info!("[objet] {name} utilise {label} : compétence(s) {posed:?}{}", if replaced.is_empty() { String::new() } else { format!(", remplace {replaced:?}") });
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Orbe de résurrection
// ---------------------------------------------------------------------------------------------

/// Pourquoi l'orbe ne sert pas ici et maintenant (op 27 code 3), sinon `None`.
fn orb_refusal(ctx: &WorldCtx, st: &WorldState, sid: u32, u: &ItemUse, now: Instant) -> Option<String> {
    let p = st.players.get(&sid)?;
    if super::archlord::is_lord(st, sid) {
        return Some("l'ArchLord".into());
    }
    if super::siege::war::war_zone(ctx, st, sid).is_some() {
        return Some("zone de siège en guerre".into());
    }
    if let Some(region) = ctx.map.region(p.ch.pos_x, p.ch.pos_z) {
        if ctx.regions.type_bits(region).is_some_and(|bits| bits & REGION_RESURRECTION == 0) {
            return Some(format!("région {region} sans résurrection"));
        }
        if ctx.regions.safety(region) == SAFETY_DANGER && !u.cash {
            return Some(format!("région de combat {region}, orbe non Cash"));
        }
    }
    if !u.level_ok(p.ch.level) {
        return Some(format!("niveau {} hors de {:?}", p.ch.level, u.level));
    }
    if p.uses.orb_ready.is_some_and(|t| t > now) {
        return Some("recharge".into());
    }
    None
}

/// Orbe de résurrection utilisé (AGPMITEM op 3) : résurrection sur place.
pub async fn use_reverse_orb(ctx: &WorldCtx, st: &mut WorldState, sid: u32, item: &InventoryItem, u: &ItemUse, now: Instant) -> anyhow::Result<()> {
    let Some(p) = st.players.get(&sid) else { return Ok(()) };
    let (cid, name) = (p.ch.id, p.ch.name.clone());
    if !p.dead {
        debug!("[objet] {name} : orbe de résurrection refusé (vivant)");
        p.send_raw(failed_packet(item.tid, cid));
        return Ok(());
    }
    if let Some(why) = orb_refusal(ctx, st, sid, u, now) {
        info!("[objet] {name} : orbe de résurrection refusé ({why})");
        if let Some(left) = p.uses.orb_ready.filter(|t| *t > now).map(|t| t - now) {
            p.send_raw(orb_reuse_packet(cid, left));
        }
        p.send_raw(super::morph::result_packet(cid, REVERSE_ORB_ERROR));
        return Ok(());
    }
    let reuse = if u.reuse_ms > 0 { Duration::from_millis(u.reuse_ms) } else { DEFAULT_ORB_REUSE };
    let p = st.players.get_mut(&sid).unwrap();
    consume_one(ctx, p, item.iid).await?;
    p.uses.orb_ready = Some(now + reuse);
    super::combat::revive(ctx, st, sid);
    let p = &st.players[&sid];
    p.send_raw(super::recovery::used_packet(item.tid, cid, reuse));
    p.send_raw(orb_reuse_packet(cid, reuse));
    info!("[objet] {name} ressuscite sur place par un orbe ({} s de recharge)", reuse.as_secs());
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Oubli des compétences
// ---------------------------------------------------------------------------------------------

/// Compétence que l'oubli retire : ni l'attaque normale, ni une compétence de l'ArchLord, ni une de production.
pub fn forgettable(catalog: &SkillCatalog, s: &Owned) -> bool {
    s.tid != skills::NORMAL_ATTACK && catalog.get(s.tid).is_some_and(|t| !t.auto_attack && !t.archlord_only() && !t.is_product())
}

/// Or rendu pour des compétences : prix d'apprentissage du niveau 1 et d'amélioration de chaque niveau suivant.
pub fn refund(catalog: &SkillCatalog, list: &[Owned]) -> i64 {
    list.iter()
        .filter_map(|s| {
            let t = catalog.get(s.tid)?;
            Some((1..=s.level).filter_map(|n| t.level(n)).enumerate().map(|(i, lv)| if i == 0 { lv.learn_cost } else { lv.upgrade_cost }).sum::<i64>())
        })
        .sum()
}

/// AGPMSKILL op 1 : compétence `s` retirée à `owner`.
pub fn skill_remove_packet(owner: u32, s: &Owned) -> Vec<u8> {
    let k = &schema::SKILL;
    let b = &schema::mini::SKILL_BASE;
    let base = archcore_net::build_mini(b, &Fields::new(b).with(0, 3).with(1, owner));
    build_packet(k, &Fields::new(k).with(0, SKILL_REMOVE).with(1, s.id).with(2, base))
}

/// Retire les compétences oubliables du joueur : en base, au client (op 1), points et passives recalculés. Rend les
/// compétences retirées.
async fn forget_all(ctx: &WorldCtx, st: &mut WorldState, sid: u32) -> anyhow::Result<Vec<Owned>> {
    let Some(p) = st.players.get(&sid) else { return Ok(Vec::new()) };
    let gone: Vec<Owned> = p.skills.list.iter().copied().filter(|s| forgettable(&ctx.skills, s)).collect();
    remove_skills(ctx, st, sid, &gone).await?;
    Ok(gone)
}

/// Retire des compétences au joueur : en base, au client (op 1), points et passives recalculés.
async fn remove_skills(ctx: &WorldCtx, st: &mut WorldState, sid: u32, gone: &[Owned]) -> anyhow::Result<()> {
    let Some(p) = st.players.get_mut(&sid) else { return Ok(()) };
    if gone.is_empty() {
        return Ok(());
    }
    let cid = p.ch.id;
    let stmts: Vec<(&str, mysql_async::Params)> =
        gone.iter().map(|s| ("DELETE FROM character_skills WHERE character_id = ? AND skill_tid = ?", (cid, s.tid).into())).collect();
    ctx.db.characters.atomic(stmts).await?;
    p.skills.list.retain(|s| !gone.iter().any(|g| g.id == s.id));
    p.skills.cast = None;
    for s in gone {
        p.send_raw(skill_remove_packet(cid, s));
    }
    skills::refresh_points(&ctx.skills, &p.skills, &mut p.factors);
    let (caster_tid, form) = (p.ch.template_id, skills::form_class(&p.factors));
    if skills::sync_passives(&ctx.skills, &mut p.skills, caster_tid, form) {
        super::equipment::refresh_factors(ctx, st, sid);
    } else {
        skills::send_points(p);
    }
    Ok(())
}

/// Gem of origin utilisée (AGPMITEM op 3, usage 27) : toutes les compétences oubliées, un exemplaire consommé.
pub async fn use_skill_init_item(ctx: &WorldCtx, st: &mut WorldState, sid: u32, item: &InventoryItem) -> anyhow::Result<()> {
    let Some(p) = st.players.get(&sid) else { return Ok(()) };
    let name = p.ch.name.clone();
    if !p.skills.list.iter().any(|s| forgettable(&ctx.skills, s)) {
        send_system(st, sid, "Vous ne pouvez pas oublier vos compétences sans en avoir appris.");
        return Ok(());
    }
    let gone = forget_all(ctx, st, sid).await?;
    let p = st.players.get_mut(&sid).unwrap();
    let cid = p.ch.id;
    consume_one(ctx, p, item.iid).await?;
    p.send_raw(super::recovery::used_packet(item.tid, cid, Duration::ZERO));
    let points = p.factors.dirt[skills::DIRT_SKILL_POINT];
    info!("[objet] {name} oublie {} compétence(s) par une Gem of origin ({points} point(s) de compétence)", gone.len());
    send_system(st, sid, &format!("Vous avez oublié {} compétence(s) : {points} point(s) de compétence à répartir.", gone.len()));
    Ok(())
}

/// AgpmEventSkillMaster op 8 : oubli des compétences chez le maître, une seule fois, or rendu. Rend le résultat envoyé.
pub async fn master_initialize(ctx: &WorldCtx, st: &mut WorldState, sid: u32) -> anyhow::Result<i64> {
    let Some(p) = st.players.get(&sid) else { return Ok(init_result::FAIL) };
    let (cid, name) = (p.ch.id, p.ch.name.clone());
    let refusal = if !p.in_game || p.dead {
        Some("mort")
    } else if p.form.evolved.is_some() {
        Some("pendant l'évolution")
    } else if !p.ch.skill_init.is_empty() {
        Some("déjà fait")
    } else if !p.skills.list.iter().any(|s| forgettable(&ctx.skills, s)) {
        Some("aucune compétence apprise")
    } else {
        None
    };
    if let Some(why) = refusal {
        info!("[maître] {name} : oubli des compétences refusé ({why})");
        p.send_raw(init_result_packet(cid, init_result::FAIL));
        return Ok(init_result::FAIL);
    }
    let gone: Vec<Owned> = p.skills.list.iter().copied().filter(|s| forgettable(&ctx.skills, s)).collect();
    let gold = refund(&ctx.skills, &gone);
    if p.ch.money as i64 + gold > super::npc_trade::MAX_MONEY {
        info!("[maître] {name} : oubli des compétences refusé ({gold} or ne tiennent pas)");
        p.send_raw(init_result_packet(cid, init_result::NO_ROOM));
        return Ok(init_result::NO_ROOM);
    }
    let money = p.ch.money as i64 + gold;
    let gone = forget_all(ctx, st, sid).await?;
    ctx.db.characters.exec("UPDATE characters SET skill_init = ? WHERE id = ?", (SKILL_INIT_DONE, cid)).await?;
    super::set_money(ctx, st, sid, money).await?;
    let p = st.players.get_mut(&sid).unwrap();
    p.ch.skill_init = SKILL_INIT_DONE.to_string();
    p.send_raw(skill_init_packet(cid, SKILL_INIT_DONE));
    p.send_raw(init_result_packet(cid, init_result::SUCCESS));
    info!("[maître] {name} oublie {} compétence(s), {gold} or rendu(s)", gone.len());
    Ok(init_result::SUCCESS)
}

// ---------------------------------------------------------------------------------------------
// Perte d'expérience à la mort
// ---------------------------------------------------------------------------------------------

/// Part de l'expérience du niveau perdue au village (pour cent) : 0 sous le niveau 6 et après une mort en combat entre
/// joueurs ; le taux du meurtrier de palier 3 remplace le taux ordinaire quand celui-ci est actif.
pub fn town_penalty_pct(base: f64, rogue: f64, level: i32, murderer_level: usize, pk_death: bool) -> f64 {
    if base <= 0.0 || pk_death || level < EXP_PENALTY_MIN_LEVEL {
        return 0.0;
    }
    if murderer_level >= 3 && rogue > 0.0 { rogue } else { base }
}

/// Résurrection au village (AGPMCHARACTER op 9) d'un mort : expérience perdue selon la configuration, sans changer de
/// niveau (le plancher est le début du niveau), enregistrée.
pub async fn town_penalty(ctx: &WorldCtx, st: &mut WorldState, sid: u32) -> anyhow::Result<()> {
    let w = &ctx.config.world;
    let Some(p) = st.players.get_mut(&sid).filter(|p| p.dead) else { return Ok(()) };
    let pct = town_penalty_pct(w.death_exp_penalty_percent, w.death_exp_penalty_rogue_percent, p.ch.level, super::pvp::murderer_level(p.ch.murderer), p.uses.pk_death);
    let need = ctx.level_exp.get(p.ch.level.max(0) as usize).copied().unwrap_or(0);
    let loss = super::wanted::exp_loss(p.ch.exp, need, pct);
    if loss == 0 {
        return Ok(());
    }
    p.ch.exp -= loss;
    p.factors.point[3] = (p.ch.exp % (1u64 << 32)) as u32 as i32;
    p.factors.point[4] = (p.ch.exp >> 32) as i32;
    let (cid, name, exp) = (p.ch.id, p.ch.name.clone(), p.ch.exp);
    ctx.db.characters.exec("UPDATE characters SET exp = ? WHERE id = ?", (exp, cid)).await?;
    info!("[combat] {name} perd {loss} point(s) d'expérience en ressuscitant au village ({pct} %)");
    send_system(st, sid, &format!("Vous perdez {loss} point(s) d'expérience."));
    Ok(())
}


// ---------------------------------------------------------------------------------------------
// Return Orb : une compétence oubliée
// ---------------------------------------------------------------------------------------------

/// AGPMSKILL op 13 (« REQUEST_SKILL_ROLLBACK ») : champ 1 = identifiant de la compétence, champ 2 = propriétaire.
pub const SKILL_ROLLBACK: i64 = 13;

/// Autre compétence apprise qui dépend de `target` : celle dont la condition de son arbre de maîtrise (compétences
/// exigées, niveaux de la classe, total) est remplie avec `target` et ne le serait plus sans elle. Rend son TID.
pub fn rollback_dependent(catalog: &SkillCatalog, skills: &skills::PlayerSkills, class_col: Option<usize>, target: &Owned) -> Option<u32> {
    let without = skills::PlayerSkills { list: skills.list.iter().copied().filter(|s| s.id != target.id).collect(), ..Default::default() };
    without.list.iter().find_map(|s| {
        let m = catalog.mastery(s.tid, class_col)?;
        (skills::mastery_unmet(catalog, skills, m).is_none() && skills::mastery_unmet(catalog, &without, m).is_some()).then_some(s.tid)
    })
}

/// Première Return Orb du joueur (usage 13) : inventaire, puis inventaire Cash, comme le client la cherche.
fn rollback_orb(ctx: &WorldCtx, p: &Player) -> Option<InventoryItem> {
    let orb = |i: &&InventoryItem| ctx.catalog.get(i.tid).is_some_and(|d| d.usable_type == USABLE_SKILL_ROLLBACK);
    p.inventory.items.values().find(orb).or_else(|| p.inventory.cash.values().find(orb)).cloned()
}

/// AGPMSKILL op 13 : la compétence `skill_id` oubliée contre une Return Orb.
pub async fn skill_rollback(ctx: &WorldCtx, st: &mut WorldState, sid: u32, skill_id: u32) -> anyhow::Result<()> {
    let Some(p) = st.players.get(&sid) else { return Ok(()) };
    let name = p.ch.name.clone();
    let class_col = skills::class_column(p.factors.ftype[0], p.factors.ftype[2]);
    let target = p.skills.by_id(skill_id).copied();
    let orb = rollback_orb(ctx, p);
    let skill_name = |tid: u32| ctx.skills.get(tid).map(|t| t.name.clone()).unwrap_or_else(|| format!("compétence {tid}"));
    let refusal = if !p.in_game || p.dead {
        Some("mort".to_string())
    } else if p.form.evolved.is_some() {
        Some("pendant l'évolution".into())
    } else {
        match target {
            None => Some(format!("compétence {skill_id} inconnue")),
            Some(s) if !forgettable(&ctx.skills, &s) => Some(format!("{} ne s'oublie pas", skill_name(s.tid))),
            Some(s) => match rollback_dependent(&ctx.skills, &p.skills, class_col, &s) {
                Some(other) => Some(format!("{} est exigée par {}", skill_name(s.tid), skill_name(other))),
                None if orb.is_none() => Some("il faut une Return Orb".into()),
                None => None,
            },
        }
    };
    if let Some(why) = refusal {
        info!("[compétence] {name} : Return Orb refusée ({why})");
        send_system(st, sid, &format!("Impossible d'oublier cette compétence : {why}."));
        return Ok(());
    }
    let (Some(target), Some(orb)) = (target, orb) else { return Ok(()) };
    remove_skills(ctx, st, sid, &[target]).await?;
    let p = st.players.get_mut(&sid).unwrap();
    let cid = p.ch.id;
    consume_one(ctx, p, orb.iid).await?;
    p.send_raw(super::recovery::used_packet(orb.tid, cid, Duration::ZERO));
    let (points, heroic) = (p.factors.dirt[skills::DIRT_SKILL_POINT], p.factors.dirt[skills::DIRT_HEROIC_POINT]);
    let label = skill_name(target.tid);
    info!("[compétence] {name} oublie {label} (niveau {}) par une Return Orb : {points} point(s), {heroic} point(s) héroïque(s)", target.level);
    send_system(st, sid, &format!("{label} oubliée : {points} point(s) de compétence à répartir."));
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Objets allumés de l'inventaire Cash (Fortune Manual, Talisman of Protection)
// ---------------------------------------------------------------------------------------------

/// Usages des objets allumés sans compétence.
pub fn lit_usage(usable: i64) -> bool {
    matches!(usable, USABLE_LUCKY_SCROLL | USABLE_ANTI_DROP)
}

/// Objet allumé de l'usage `usable` dans l'inventaire Cash du joueur.
pub fn lit_item(ctx: &WorldCtx, p: &Player, usable: i64) -> Option<InventoryItem> {
    p.inventory.in_use.iter().filter_map(|iid| p.inventory.cash.get(iid)).find(|i| ctx.catalog.get(i.tid).is_some_and(|d| d.usable_type == usable)).cloned()
}

/// AGPMITEM op 3 sur un Fortune Manual ou un Talisman of Protection de l'inventaire Cash : allumé (l'autre objet allumé
/// du même usage s'éteint), ou éteint s'il l'était.
pub fn toggle_lit(ctx: &WorldCtx, st: &mut WorldState, sid: u32, item: &InventoryItem, usable: i64) {
    let Some(p) = st.players.get(&sid) else { return };
    if !p.inventory.cash.contains_key(&item.iid) {
        debug!("[objet] {} : objet {} hors de l'inventaire Cash, pas allumé", p.ch.name, item.tid);
        return;
    }
    let other = lit_item(ctx, p, usable).filter(|o| o.iid != item.iid);
    let p = st.players.get_mut(&sid).unwrap();
    let cid = p.ch.id;
    let label = ctx.catalog.get(item.tid).map(|d| d.name.clone()).unwrap_or_default();
    if p.inventory.in_use.remove(&item.iid) {
        p.send_raw(super::items::use_time_packet(item.iid, cid, false));
        info!("[objet] {} éteint {label}", p.ch.name);
        return;
    }
    if let Some(o) = other {
        p.inventory.in_use.remove(&o.iid);
        p.send_raw(super::items::use_time_packet(o.iid, cid, false));
    }
    p.inventory.in_use.insert(item.iid);
    p.send_raw(super::items::use_time_packet(item.iid, cid, true));
    info!("[objet] {} allume {label}", p.ch.name);
}

/// AGPMITEM op 23 : éteint un Fortune Manual ou un Talisman of Protection allumé. Rend vrai si l'objet en était un.
pub fn stop_lit(ctx: &WorldCtx, st: &mut WorldState, sid: u32, iid: u32) -> bool {
    let Some(p) = st.players.get_mut(&sid) else { return false };
    let lit = p.inventory.cash.get(&iid).is_some_and(|i| ctx.catalog.get(i.tid).is_some_and(|d| lit_usage(d.usable_type)));
    if !lit || !p.inventory.in_use.remove(&iid) {
        return lit;
    }
    p.send_raw(super::items::use_time_packet(iid, p.ch.id, false));
    info!("[objet] {} éteint l'objet {iid}", p.ch.name);
    true
}

// ---------------------------------------------------------------------------------------------
// Feux d'artifice
// ---------------------------------------------------------------------------------------------

/// Feu d'artifice utilisé (AGPMITEM op 3, usage 22) : un exemplaire consommé, op 4 au joueur et à ceux qui le voient.
pub async fn use_firework(ctx: &WorldCtx, st: &mut WorldState, sid: u32, item: &InventoryItem, u: &ItemUse, now: Instant) -> anyhow::Result<()> {
    let Some(p) = st.players.get_mut(&sid) else { return Ok(()) };
    let (cid, name) = (p.ch.id, p.ch.name.clone());
    let label = ctx.catalog.get(item.tid).map(|d| d.name.clone()).unwrap_or_default();
    if let Some(left) = p.uses.left(item.tid, &[], now) {
        p.send_raw(failed_packet(item.tid, cid));
        send_system(st, sid, &format!("{label} n'est pas encore prêt ({} s).", left.as_secs().max(1)));
        return Ok(());
    }
    if u.reuse_ms > 0 {
        p.uses.item_ready.insert(item.tid, now + Duration::from_millis(u.reuse_ms));
    }
    consume_one(ctx, p, item.iid).await?;
    super::combat::send_around(st, sid, &super::recovery::used_packet(item.tid, cid, Duration::from_millis(u.reuse_ms)));
    info!("[objet] {name} lance {label}");
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Commandes des objets : /go, /safekeeper, /merchant, /nick
// ---------------------------------------------------------------------------------------------

/// Commande de discussion propre à un objet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemCommand {
    Go,
    SafeKeeper,
    Merchant,
    Nick,
}

impl ItemCommand {
    pub fn parse(cmd: &str) -> Option<Self> {
        match cmd.to_lowercase().as_str() {
            "go" => Some(ItemCommand::Go),
            "safekeeper" => Some(ItemCommand::SafeKeeper),
            "merchant" => Some(ItemCommand::Merchant),
            "nick" => Some(ItemCommand::Nick),
            _ => None,
        }
    }

    /// L'objet d'usage `usable` et de TID `tid` permet cette commande.
    pub fn accepts(self, usable: i64, tid: u32) -> bool {
        match self {
            ItemCommand::Go => usable == USABLE_JUMP && tid != SAFE_KEEPER_ITEM && tid != MERCHANT_ITEM,
            ItemCommand::SafeKeeper => usable == USABLE_JUMP && tid == SAFE_KEEPER_ITEM,
            ItemCommand::Merchant => usable == USABLE_JUMP && tid == MERCHANT_ITEM,
            ItemCommand::Nick => usable == USABLE_NICK,
        }
    }

    fn needs(self) -> &'static str {
        match self {
            ItemCommand::Go => "un Gate of Phantom",
            ItemCommand::SafeKeeper => "un Safe Keeper",
            ItemCommand::Merchant => "un Merchant",
            ItemCommand::Nick => "une Signature of Glory",
        }
    }
}

/// Signature acceptée pour `/nick` : 16 octets au plus une fois codée, sans caractère de contrôle ni « " # \ ». Vide :
/// la signature est effacée.
pub fn valid_signature(text: &str) -> bool {
    archcore_net::text::encode(text).len() <= SIGNATURE_MAX_BYTES && !text.chars().any(|c| c.is_control() || matches!(c, '"' | '#' | '\\'))
}

/// AGPMCHARACTER op 1 : signature (champ 27) du personnage.
pub fn signature_packet(cid: u32, text: &str) -> Vec<u8> {
    let c = &schema::CHARACTER;
    build_packet(c, &Fields::new(c).with(0, 1).with(1, cid).with(27, text))
}

/// Objet du joueur qui permet la commande (inventaire, puis inventaire Cash) ; celui qui n'est pas en recharge d'abord.
fn command_item(ctx: &WorldCtx, p: &Player, cmd: ItemCommand, now: Instant) -> Option<InventoryItem> {
    let all: Vec<&InventoryItem> = p.inventory.items.values().chain(p.inventory.cash.values()).filter(|i| ctx.catalog.get(i.tid).is_some_and(|d| cmd.accepts(d.usable_type, i.tid))).collect();
    all.iter().find(|i| p.uses.left(i.tid, &[], now).is_none()).or(all.first()).map(|i| (*i).clone())
}

/// Commandes `/go`, `/safekeeper`, `/merchant`, `/nick` tapées dans la discussion. Rend vrai si le texte en était une.
pub async fn chat_command(ctx: &WorldCtx, st: &mut WorldState, sid: u32, text: &str) -> anyhow::Result<bool> {
    let body = text.trim().trim_start_matches('/');
    let (word, rest) = body.split_once(char::is_whitespace).map(|(c, r)| (c, r.trim())).unwrap_or((body, ""));
    let Some(cmd) = ItemCommand::parse(word) else { return Ok(false) };
    let now = Instant::now();
    let Some(p) = st.players.get(&sid) else { return Ok(false) };
    let name = p.ch.name.clone();
    let Some(item) = command_item(ctx, p, cmd, now) else {
        send_system(st, sid, &format!("Il faut {} pour la commande /{}.", cmd.needs(), word.to_lowercase()));
        return Ok(true);
    };
    let label = ctx.catalog.get(item.tid).map(|d| d.name.clone()).unwrap_or_default();
    let target = rest.trim_matches('"').trim();
    let mut refusal = if !p.in_game || p.dead {
        Some("mort".to_string())
    } else if let Some(left) = p.uses.left(item.tid, &[], now) {
        Some(format!("{label} n'est pas encore prêt ({} s)", left.as_secs().max(1)))
    } else if cmd != ItemCommand::Nick && super::morph::restricted(st, sid) {
        Some("sous métamorphose".into())
    } else if cmd != ItemCommand::Nick {
        super::party::recall_refusal(ctx, st, sid, now)
    } else if !valid_signature(target) {
        Some(format!("signature de {SIGNATURE_MAX_BYTES} caractères au plus, sans \" # \\"))
    } else {
        None
    };
    // `/go` : un autre joueur en jeu, là où l'on peut le rejoindre.
    let mut dest = None;
    if refusal.is_none() && cmd == ItemCommand::Go {
        refusal = match st.by_name(target).filter(|t| *t != sid) {
            None => Some(format!("{target} n'est pas en jeu")),
            Some(tsid) => match super::party::recall_refusal(ctx, st, tsid, now) {
                Some(why) => Some(format!("{target} est hors d'atteinte ({why})")),
                None => {
                    let t = &st.players[&tsid];
                    dest = Some(super::presence::position_now(t, now));
                    (!t.in_game).then(|| format!("{target} n'est pas en jeu"))
                }
            },
        };
    }
    let npc = match cmd {
        ItemCommand::SafeKeeper => super::npc_events::nearest_with(&ctx.npcs, p, super::npcs::event_function::BANK),
        ItemCommand::Merchant => super::npc_events::nearest_with(&ctx.npcs, p, super::npcs::event_function::NPC_TRADE),
        _ => None,
    };
    if refusal.is_none() && matches!(cmd, ItemCommand::SafeKeeper | ItemCommand::Merchant) && npc.is_none() {
        refusal = Some("aucun PNJ ne la propose".into());
    }
    if let Some(why) = refusal {
        info!("[objet] {name} : /{} refusé ({why})", word.to_lowercase());
        send_system(st, sid, &format!("Commande /{} impossible : {why}.", word.to_lowercase()));
        return Ok(true);
    }
    let reuse = ctx.catalog.uses.get(&item.tid).map(|u| u.reuse_ms).unwrap_or(0);
    let p = st.players.get_mut(&sid).unwrap();
    let cid = p.ch.id;
    if cmd == ItemCommand::Nick {
        ctx.db.characters.exec("UPDATE characters SET signature = ? WHERE id = ?", (target.to_string(), cid)).await?;
        p.ch.signature = target.to_string();
    }
    if reuse > 0 {
        p.uses.item_ready.insert(item.tid, now + Duration::from_millis(reuse));
    }
    consume_one(ctx, p, item.iid).await?;
    p.send_raw(super::recovery::used_packet(item.tid, cid, Duration::from_millis(reuse)));
    send_system(st, sid, &format!("Objet {label} utilisé."));
    match cmd {
        ItemCommand::Go => {
            if st.players.get(&sid).is_some_and(|p| p.trade.is_some()) {
                super::trade::cancel(st, sid);
            }
            let to = dest.unwrap_or_default();
            info!("[objet] {name} rejoint {target} par {label}");
            super::teleport(ctx, st, sid, to.x as f64, Some(to.y as f64), to.z as f64).await?;
        }
        ItemCommand::SafeKeeper => {
            if let Some(npc) = npc {
                super::bank::open_remote(st, sid, npc);
            }
        }
        ItemCommand::Merchant => {
            if let Some(npc) = npc {
                info!("[objet] {name} ouvre à distance la boutique de {}", npc.name);
                super::npc_trade::open_remote(ctx, st, sid, npc);
            }
        }
        ItemCommand::Nick => {
            super::combat::send_around(st, sid, &signature_packet(cid, target));
            info!("[objet] {name} prend la signature « {target} »");
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lecture_des_consommables() {
        let stew = serde_json::json!({"ItemType":"1","Type":"12","Use skill id":"459","Use skill level":"1"});
        let u = ItemUse::from_raw(&stew).unwrap();
        assert_eq!((u.usable_type, u.skills.clone(), u.reuse_ms, u.can_stop), (12, vec![(459, 1)], 0, false));
        // Liste « ; », niveau unique répété ; recharge = intervalle + nombre × durée ; durée de l'objet en minutes.
        let cake = serde_json::json!({"ItemType":"1","Type":"12","Use skill id":"10;11","Use skill level":"3","Use Interval":"900000",
            "Apply Effect Count":"1","Apply Effect Time":"600000","ExpireTime":"30","CanStopUsingItem":"1"});
        let u = ItemUse::from_raw(&cake).unwrap();
        assert_eq!(u.skills, vec![(10, 3), (11, 3)]);
        assert_eq!((u.reuse_ms, u.effect_ms, u.lifetime_ms, u.can_stop), (1_500_000, 600_000, 1_800_000, true));
        let orb = serde_json::json!({"ItemType":"1","Type":"14","Use Interval":"90000","Cash":"1","Level":"1;30"});
        let u = ItemUse::from_raw(&orb).unwrap();
        assert_eq!((u.usable_type, u.cash, u.level, u.reuse_ms), (14, true, (1, 30), 90_000));
        assert!(u.level_ok(30) && !u.level_ok(31));
        assert!(ItemUse::from_raw(&serde_json::json!({"ItemType":"1","Type":"14","Cash":"100"})).is_some_and(|u| !u.cash && u.level_ok(99)));
        assert!(ItemUse::from_raw(&serde_json::json!({"ItemType":"1","Type":"27"})).is_some(), "Gem of origin sans compétence");
        // Orbe de rappel (Bead of Summoning) : sans compétence, niveaux de « SuitableLevelMin/Max ».
        let bead = ItemUse::from_raw(&serde_json::json!({"ItemType":"1","Type":"15","SuitableLevelMin":"1","SuitableLevelMax":"99","Use Interval":"300000"})).unwrap();
        assert_eq!((bead.usable_type, bead.level, bead.reuse_ms), (15, (1, 99), 300_000));
        assert!(bead.level_ok(99) && !bead.level_ok(100));
        assert!(ItemUse::from_raw(&serde_json::json!({"ItemType":"1","Type":"12"})).is_none(), "sans compétence");
        assert!(ItemUse::from_raw(&serde_json::json!({"Type":"31","Use skill id":"857"})).is_none(), "ItemType vide");
    }

    #[test]
    fn familles_des_effets() {
        // Ragoûts de feu (famille 17) : le rang égal ou supérieur remplace, un rang inférieur est refusé.
        let worn = [(459, (17, 3)), (463, (18, 3)), (222, (0, 0))];
        assert_eq!(family_clash(&worn, 460, (17, 3)), Ok(vec![459]));
        assert_eq!(family_clash(&worn, 460, (17, 2)), Err(459));
        assert_eq!(family_clash(&worn, 459, (17, 1)), Ok(vec![]), "même compétence : remplacée par effects::push");
        assert_eq!(family_clash(&worn, 1, (0, 0)), Ok(vec![]), "sans famille");
        assert_eq!(family_clash(&worn, 2, (50, 1)), Ok(vec![]));
    }

    #[test]
    fn recharges_et_durees() {
        let now = Instant::now();
        let mut u = Uses::default();
        u.item_ready.insert(111, now + Duration::from_secs(5));
        u.skill_ready.insert(459, now + Duration::from_secs(420));
        assert_eq!(u.left(111, &[(459, 1)], now), Some(Duration::from_secs(420)));
        assert_eq!(u.left(112, &[(463, 1)], now), None);
        assert_eq!(u.left(111, &[], now + Duration::from_secs(6)), None);
        u.clear();
        assert_eq!(u.left(111, &[(459, 1)], now), None);
        let talisman = ItemUse { lifetime_ms: 1_800_000, ..Default::default() };
        assert_eq!(effect_duration_ms(300_000, &talisman), 300_000, "durée du niveau d'abord");
        assert_eq!(effect_duration_ms(0, &talisman), 1_800_000);
        assert_eq!(effect_duration_ms(0, &ItemUse::default()), LIT_EFFECT_MS);
        assert_eq!(bonus_pct(100.0, 10.0), 10.0);
        assert_eq!(bonus_pct(1000.0, 1.0), 1000.0);
    }

    #[test]
    fn colonnes_des_effets_d_objet() {
        use super::super::effects;
        // Scorching Stew (459) et Janus Talisman of Treasure LV1 (1811), colonnes de SKILL_CONST.
        let stew = serde_json::json!({"point.def_point_fire":"8","recast_time":"420000","duration":"300000","skill_classify_id":"17","skill_classify_level":"3"});
        let lv = skills::SkillLevel::from_consts(&stew);
        assert_eq!((lv.classify, lv.duration_ms, lv.recast_ms, lv.mods.spirit_defense[2]), ((17, 3), 300_000, 420_000, 8.0));
        let mut f = crate::character::Factors::default();
        f.defense_point[3] = 5;
        effects::apply(&mut f, &lv.mods);
        assert_eq!(f.defense_point[3], 13, "résistance au feu (attribut 3)");
        let treasure = effects::Mods::from_consts(&serde_json::json!({"bonus_drop_rate":"100","bonus_exp":"0"}));
        assert_eq!((treasure.drop_bonus, treasure.exp_bonus, bonus_pct(treasure.drop_bonus, 10.0)), (100.0, 0.0, 10.0));
        assert!(!treasure.is_empty());
    }

    #[test]
    fn oubli_et_remboursement() {
        let mut catalog = SkillCatalog::default();
        let lv = |learn: i64, upgrade: i64| skills::SkillLevel { learn_cost: learn, upgrade_cost: upgrade, ..Default::default() };
        // Dual Slash : 80 au niveau 1, 480 puis 1440 aux améliorations.
        catalog.insert(skills::SkillTemplate { tid: 1, levels: vec![lv(80, 0), lv(0, 480), lv(0, 1440)], ..Default::default() });
        catalog.insert(skills::SkillTemplate { tid: skills::NORMAL_ATTACK, auto_attack: true, levels: vec![lv(0, 0)], ..Default::default() });
        // Production (unités 8 d'`attribute`) et ArchLord (`skill_type` 9) : gardées.
        catalog.insert(skills::SkillTemplate { tid: 500, attribute: 8, levels: vec![lv(100, 0)], ..Default::default() });
        let lord = skills::SkillLevel { skill_type: skills::ARCHLORD_SKILL_TYPE, ..Default::default() };
        catalog.insert(skills::SkillTemplate { tid: 600, levels: vec![lord], ..Default::default() });
        let owned = |tid: u32, level: u32| Owned { id: tid, tid, level, ready_at: None, exp: 0 };
        let list = [owned(1, 2), owned(skills::NORMAL_ATTACK, 1), owned(500, 3), owned(600, 1), owned(9999, 1)];
        let gone: Vec<Owned> = list.iter().copied().filter(|s| forgettable(&catalog, s)).collect();
        assert_eq!(gone.iter().map(|s| s.tid).collect::<Vec<_>>(), vec![1], "compétence inconnue du catalogue gardée");
        assert_eq!(refund(&catalog, &gone), 560);
        assert_eq!(refund(&catalog, &[owned(1, 3)]), 2000);
    }

    #[test]
    fn perte_au_village() {
        assert_eq!(town_penalty_pct(0.0, 10.0, 50, 3, false), 0.0, "désactivée par défaut");
        assert_eq!(town_penalty_pct(3.0, 10.0, 50, 0, false), 3.0);
        assert_eq!(town_penalty_pct(3.0, 10.0, 50, 3, false), 10.0, "meurtrier de palier 3");
        assert_eq!(town_penalty_pct(3.0, 0.0, 50, 3, false), 3.0);
        assert_eq!(town_penalty_pct(3.0, 10.0, 50, 0, true), 0.0, "mort en combat entre joueurs");
        assert_eq!(town_penalty_pct(3.0, 10.0, 5, 0, false), 0.0, "sous le niveau 6");
    }

    #[test]
    fn return_orb_et_prerequis() {
        let mut catalog = SkillCatalog::default();
        let lv = skills::SkillLevel::default();
        for tid in [1, 2, 3] {
            catalog.insert(skills::SkillTemplate { tid, levels: vec![lv.clone(), lv.clone(), lv.clone()], ..Default::default() });
        }
        // 2 exige 1 au niveau 2 ; 3 n'exige rien.
        catalog.insert_mastery(2, skills::Mastery { requires: vec![(1, 2)], ..Default::default() });
        let owned = |id: u32, tid: u32, level: u32| Owned { id, tid, level, ready_at: None, exp: 0 };
        let learned = skills::PlayerSkills { list: vec![owned(10, 1, 2), owned(11, 2, 1), owned(12, 3, 1)], ..Default::default() };
        assert_eq!(rollback_dependent(&catalog, &learned, None, &learned.list[0]), Some(2), "prérequis d'une compétence apprise");
        assert_eq!(rollback_dependent(&catalog, &learned, None, &learned.list[1]), None);
        assert_eq!(rollback_dependent(&catalog, &learned, None, &learned.list[2]), None);
        // Condition déjà non remplie avant (base importée) : le retrait ne la casse pas.
        let broken = skills::PlayerSkills { list: vec![owned(10, 1, 1), owned(11, 2, 1)], ..Default::default() };
        assert_eq!(rollback_dependent(&catalog, &broken, None, &broken.list[0]), None);
    }

    #[test]
    fn commandes_des_objets() {
        assert_eq!(ItemCommand::parse("GO"), Some(ItemCommand::Go));
        assert_eq!(ItemCommand::parse("safekeeper"), Some(ItemCommand::SafeKeeper));
        assert_eq!(ItemCommand::parse("goto"), None);
        assert!(ItemCommand::Go.accepts(USABLE_JUMP, 4568) && ItemCommand::Go.accepts(USABLE_JUMP, 6493));
        assert!(!ItemCommand::Go.accepts(USABLE_JUMP, SAFE_KEEPER_ITEM) && !ItemCommand::Go.accepts(USABLE_NICK, 4875));
        assert!(ItemCommand::SafeKeeper.accepts(USABLE_JUMP, 4923) && !ItemCommand::SafeKeeper.accepts(USABLE_JUMP, 4924));
        assert!(ItemCommand::Merchant.accepts(USABLE_JUMP, 4924));
        assert!(ItemCommand::Nick.accepts(USABLE_NICK, 4875));
        assert!(valid_signature("Héros du Nord") && valid_signature("") && valid_signature("abcdefghijklmnop"));
        assert!(!valid_signature("abcdefghijklmnopq"), "17 octets");
        assert!(!valid_signature("a#b") && !valid_signature("a\"b") && !valid_signature("a\tb"));
        let p = archcore_net::decode_packet(&schema::CHARACTER, &signature_packet(42, "Héros")).unwrap();
        assert_eq!((p.int(0), p.int(1), p.text(27).as_deref()), (Some(1), Some(42), Some("Héros")));
        // Usages sans compétence retenus : Return Orb, Secret Safe, feux, objets de commande, Marvel Scroll.
        for t in ["13", "19", "21", "22", "24", "25", "26", "28", "35"] {
            assert!(ItemUse::from_raw(&serde_json::json!({"ItemType":"1","Type":t})).is_some(), "usage {t}");
        }
        assert!(FIREWORKS.contains(&4606) && !FIREWORKS.contains(&14720) && !FIREWORKS.contains(&65462));
        assert!(lit_usage(USABLE_LUCKY_SCROLL) && lit_usage(USABLE_ANTI_DROP) && !lit_usage(USABLE_NICK));
    }

    #[test]
    fn paquets() {
        let p = archcore_net::decode_packet(&schema::ITEM, &orb_reuse_packet(42, Duration::from_secs(90))).unwrap();
        assert_eq!((p.int(0), p.int(4), p.int(18)), (Some(18), Some(42), Some(90_000)));
        let p = archcore_net::decode_packet(&schema::ITEM, &failed_packet(842, 42)).unwrap();
        assert_eq!((p.int(0), p.int(3), p.int(4)), (Some(5), Some(842), Some(42)));
        let p = archcore_net::decode_packet(&schema::CHARACTER, &skill_init_packet(42, "1")).unwrap();
        assert_eq!((p.int(0), p.int(1), p.text(19).as_deref()), (Some(23), Some(42), Some("1")));
        let p = archcore_net::decode_packet(&schema::EVENT_SKILL_MASTER, &init_result_packet(42, init_result::NO_ROOM)).unwrap();
        assert_eq!((p.int(0), p.int(2), p.int(5)), (Some(9), Some(42), Some(2)));
        let s = Owned { id: 7, tid: 1, level: 1, ready_at: None, exp: 0 };
        let p = archcore_net::decode_packet(&schema::SKILL, &skill_remove_packet(42, &s)).unwrap();
        assert_eq!((p.int(0), p.int(1)), (Some(1), Some(7)));
    }
}
