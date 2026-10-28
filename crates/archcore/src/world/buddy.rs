//! Liste d'amis et liste d'ignorés (AGPMBUDDY 0x2B, schémas `BUDDY` et `BUDDY_LIST`).
//!
//! ```text
//!   Paquet (drapeaux 4) : 0 op, 1 CID du propriétaire de la liste (le destinataire, du serveur au client),
//!     2 nom de l'autre joueur (CHAR[33]), 3 drapeaux de l'entrée, 4 région (op 7), 5 relation « Buddy », 6 code (op 8).
//!   Entrée en jeu           ← op 1 par entrée (amis : 0x02000000, + 0x00100000 en ligne ; ignorés : blocages), op 9
//!                           → op 15 (schéma BUDDY_LIST) : ignorés de `Ini\IgnoreList.txt`, fusionnés en base
//!   Ami     A → op 2 (nom de B)          B ← op 2 (nom de A) ; A ← op 8 (1 déjà, 2 absent, 3 plein, 4 soi-même)
//!           B → op 1 (nom de A, 0x02000000) accepte : A ← op 1 (B), B ← op 1 (A)
//!           B → op 3 (nom de A) refuse : A ← op 3 (nom de B) ; option « refus d'ami » (0x80) de B : A ← op 3 aussitôt
//!   Ignoré  → op 1 (nom, drapeaux 0)     ← op 1 (personnage en base, connecté ou non)
//!           → op 5 (nom, cases 0x1 discussion, 0x2 échange, 0x4 invitations)   ← op 5
//!   Retrait → op 4 (nom)                 ← op 4, chez le demandeur seulement
//!   État    ← op 6 (nom, 0x00100000 ou non) aux joueurs qui ont X dans leurs amis, quand X entre en jeu ou en sort
//!   Détail  → op 7 (nom d'un ami)        ← op 7 (champ 4 : région, -1 hors ligne)
//! ```
//! Choix d'ArchCore (le serveur d'origine n'est pas connu) : acceptation réciproque, retrait d'un ami chez le
//! demandeur seulement, un même personnage dans une seule des deux listes, 30 entrées par liste, noms comparés sans
//! casse. Les blocages d'un ignoré s'appliquent au serveur : chuchotement non transmis (sans rien dire à l'auteur,
//! comme le filtre du client), demande d'échange, invitations de groupe et de guilde refusées avec le message des
//! options (`option_flags.rs`). La relation « Buddy » (mentor, ops 10 à 14) est reportée : l'op 10 reçoit le code 12
//! (« Request was Invalid »), les autres sont ignorées. En base : `character_buddy`.

use super::{find_character, option_flags, WorldCtx, WorldState};
use crate::db::{col, Databases};
use crate::{debug, info};
use archcore_net::schema::{self, buddy_flag, buddy_op, buddy_result};
use archcore_net::{decode_packet, Fields, Packet};
use std::collections::HashSet;

/// Entrées au plus dans chaque liste (celles du client).
pub const MAX_ENTRIES: usize = 30;
/// Longueur maximale d'un nom (CHAR[33] avec son zéro).
const MAX_NAME: usize = 32;
/// Taille d'un nom dans le paquet de l'op 15.
const LIST_NAME: usize = 33;

/// Une entrée de liste.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub cid: u32,
    pub name: String,
    /// Drapeaux enregistrés (ami ou blocages), jamais l'état en ligne.
    pub flags: u32,
    pub relation: u16,
}

impl Entry {
    pub fn is_friend(&self) -> bool {
        self.flags & buddy_flag::FRIEND != 0
    }
}

/// Listes d'un joueur en jeu.
#[derive(Clone, Debug, Default)]
pub struct Buddies {
    pub entries: Vec<Entry>,
    /// Demandes d'ami reçues sans réponse (CID des demandeurs).
    pub requests: HashSet<u32>,
    /// Liste du fichier local (op 15) déjà fusionnée pendant cette session.
    pub merged: bool,
}

impl Buddies {
    pub async fn load(db: &Databases, cid: u32) -> anyhow::Result<Self> {
        let rows = db
            .characters
            .rows(
                "SELECT b.other_id, c.name, b.flags, b.relation FROM character_buddy b JOIN characters c ON c.id = b.other_id \
                 WHERE b.character_id = ? AND c.deleted_at IS NULL ORDER BY b.created_at, b.other_id",
                (cid,),
            )
            .await?;
        let entries = rows
            .iter()
            .map(|r| Entry { cid: col(r, "other_id"), name: col(r, "name"), flags: col::<u32>(r, "flags") & !buddy_flag::ONLINE, relation: col(r, "relation") })
            .collect();
        Ok(Buddies { entries, ..Default::default() })
    }

    /// Entrée d'un nom (sans casse), dans l'une ou l'autre liste.
    pub fn find(&self, name: &str) -> Option<&Entry> {
        let key = name.to_lowercase();
        self.entries.iter().find(|e| e.name.to_lowercase() == key)
    }

    pub fn has(&self, cid: u32) -> bool {
        self.entries.iter().any(|e| e.cid == cid)
    }

    /// Entrées de la liste d'amis (`friends`) ou d'ignorés.
    pub fn count(&self, friends: bool) -> usize {
        self.entries.iter().filter(|e| e.is_friend() == friends).count()
    }

    /// `cid` est-il un ignoré dont l'entrée porte le blocage `bit` ?
    pub fn blocks(&self, cid: u32, bit: u32) -> bool {
        self.entries.iter().any(|e| e.cid == cid && !e.is_friend() && e.flags & bit != 0)
    }
}

/// Le joueur `target_sid` bloque-t-il `author_cid` pour ce que `bit` désigne (`buddy_flag::BLOCK_*`) ?
pub fn blocks(st: &WorldState, target_sid: u32, author_cid: u32, bit: u32) -> bool {
    st.players.get(&target_sid).is_some_and(|p| p.buddies.blocks(author_cid, bit))
}

fn base(op: i64, owner: u32) -> Fields {
    let s = &schema::BUDDY;
    Fields::new(s).with(0, op).with(1, owner)
}

/// Paquet d'une entrée, avec l'état en ligne.
pub fn entry_fields(op: i64, owner: u32, e: &Entry, online: bool) -> Fields {
    let flags = e.flags | if online { buddy_flag::ONLINE } else { 0 };
    base(op, owner).with(2, e.name.as_str()).with(3, flags as i64).with(4, -1).with(5, e.relation as i64)
}

/// Op 8 : code de résultat.
pub fn result_fields(owner: u32, code: i64) -> Fields {
    base(buddy_op::RESULT, owner).with(6, code)
}

/// Ignorés de l'op 15 : (nom, drapeaux), dans l'ordre, sans les noms vides.
pub fn list_entries(f: &Fields) -> Vec<(String, u32)> {
    let count = (f.int(2).unwrap_or(0).max(0) as usize).min(MAX_ENTRIES);
    let names = f.bytes(3).unwrap_or_default();
    let flags: Vec<u32> = f.list(4).map(|l| l.iter().map(|v| match v {
        archcore_net::Value::Int(n) => *n as u32,
        _ => 0,
    }).collect()).unwrap_or_default();
    (0..count)
        .filter_map(|i| {
            let raw = names.get(i * LIST_NAME..((i + 1) * LIST_NAME).min(names.len()))?;
            let end = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
            let name = archcore_net::text::decode(&raw[..end]).trim().to_string();
            (!name.is_empty()).then(|| (name, flags.get(i).copied().unwrap_or(0)))
        })
        .collect()
}

fn send(st: &WorldState, sid: u32, f: Fields) {
    if let Some(p) = st.players.get(&sid) {
        p.send(&schema::BUDDY, f);
    }
}

fn send_result(st: &WorldState, sid: u32, code: i64) {
    if let Some(p) = st.players.get(&sid) {
        p.send(&schema::BUDDY, result_fields(p.ch.id, code));
    }
}

fn online(st: &WorldState, cid: u32) -> bool {
    st.by_cid.get(&cid).and_then(|s| st.players.get(s)).is_some_and(|p| p.in_game)
}

/// Nom acceptable : 1 à 32 octets, et pas celui du joueur.
fn valid_name(name: &str, own: &str) -> bool {
    !name.is_empty() && archcore_net::text::encode(name).len() <= MAX_NAME && name.to_lowercase() != own.to_lowercase()
}

/// AGPMBUDDY reçu.
pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    if !st.players.get(&sid).is_some_and(|p| p.in_game) {
        return Ok(());
    }
    let name = p.text(2).unwrap_or_default().trim().to_string();
    let flags = p.int(3).unwrap_or(0) as u32;
    match p.int(0).unwrap_or(-1) {
        buddy_op::ADD if flags & buddy_flag::FRIEND != 0 => accept(ctx, st, sid, &name).await?,
        buddy_op::ADD => add_ignored(ctx, st, sid, &name, flags & buddy_flag::BLOCKS).await?,
        buddy_op::REQUEST => request(st, sid, &name),
        buddy_op::REJECT => reject(st, sid, &name),
        buddy_op::REMOVE => remove(ctx, st, sid, &name).await?,
        buddy_op::OPTIONS => options(ctx, st, sid, &name, flags).await?,
        buddy_op::REGION => region(ctx, st, sid, &name),
        buddy_op::LIST => {
            let list = decode_packet(&schema::BUDDY_LIST, &p.raw).map(|l| list_entries(&l.fields)).unwrap_or_default();
            merge_list(ctx, st, sid, &list).await?;
        }
        // Relation « Buddy » (mentor) : reportée.
        buddy_op::MENTOR_REQUEST => send_result(st, sid, buddy_result::MENTOR_INVALID),
        op => debug!("[amis] #{sid} op {op} ignorée"),
    }
    Ok(())
}

/// Op 2 : demande d'ami, relayée à la cible en ligne.
fn request(st: &mut WorldState, sid: u32, name: &str) {
    let me = &st.players[&sid];
    let (my_cid, my_name) = (me.ch.id, me.ch.name.clone());
    if !valid_name(name, &my_name) {
        return send_result(st, sid, buddy_result::INVALID);
    }
    if me.buddies.find(name).is_some() {
        return send_result(st, sid, buddy_result::ALREADY);
    }
    if me.buddies.count(true) >= MAX_ENTRIES {
        return send_result(st, sid, buddy_result::FULL);
    }
    let Some(tsid) = st.by_name(name).filter(|t| st.players[t].in_game) else {
        return send_result(st, sid, buddy_result::OFFLINE_OR_UNKNOWN);
    };
    let target = &st.players[&tsid];
    let (tcid, tname) = (target.ch.id, target.ch.name.clone());
    // Option « refuser les demandes d'ami » : refus aussitôt, comme si la cible avait dit non.
    if option_flags::refuses(st, tsid, option_flags::REFUSE_BUDDY) {
        info!("[amis] {tname} refuse les demandes d'ami ({my_name} demandait)");
        return send(st, sid, base(buddy_op::REJECT, my_cid).with(2, tname.as_str()));
    }
    st.players.get_mut(&tsid).unwrap().buddies.requests.insert(my_cid);
    send(st, tsid, base(buddy_op::REQUEST, tcid).with(2, my_name.as_str()).with(3, buddy_flag::FRIEND as i64));
    info!("[amis] {my_name} demande {tname} en ami");
}

/// Op 1 avec le bit « ami » : la cible accepte la demande de `name` ; chacun entre dans la liste de l'autre.
async fn accept(ctx: &WorldCtx, st: &mut WorldState, sid: u32, name: &str) -> anyhow::Result<()> {
    let Some(asid) = st.by_name(name).filter(|a| st.players[a].in_game) else {
        send_result(st, sid, buddy_result::OFFLINE_OR_UNKNOWN);
        return Ok(());
    };
    let (acid, aname) = (st.players[&asid].ch.id, st.players[&asid].ch.name.clone());
    let me = st.players.get_mut(&sid).unwrap();
    if !me.buddies.requests.remove(&acid) {
        debug!("[amis] {} accepte {aname} sans demande", me.ch.name);
        return Ok(());
    }
    let (bcid, bname) = (me.ch.id, me.ch.name.clone());
    // Chacun reçoit l'autre, sauf s'il l'a déjà (ami gardé après un retrait d'un seul côté).
    let mine = st.players[&sid].buddies.find(&aname).cloned();
    let theirs = st.players[&asid].buddies.find(&bname).cloned();
    if mine.as_ref().is_some_and(|e| !e.is_friend()) || theirs.as_ref().is_some_and(|e| !e.is_friend()) {
        send_result(st, sid, buddy_result::ALREADY);
        return Ok(());
    }
    if (mine.is_none() && st.players[&sid].buddies.count(true) >= MAX_ENTRIES) || (theirs.is_none() && st.players[&asid].buddies.count(true) >= MAX_ENTRIES) {
        send_result(st, sid, buddy_result::FULL);
        return Ok(());
    }
    for (owner_sid, owner, other, other_name, known) in [(sid, bcid, acid, aname.clone(), mine.is_some()), (asid, acid, bcid, bname.clone(), theirs.is_some())] {
        if known {
            continue;
        }
        ctx.db
            .characters
            .exec("INSERT IGNORE INTO character_buddy (character_id, other_id, flags) VALUES (?, ?, ?)", (owner, other, buddy_flag::FRIEND))
            .await?;
        let e = Entry { cid: other, name: other_name, flags: buddy_flag::FRIEND, relation: 0 };
        send(st, owner_sid, entry_fields(buddy_op::ADD, owner, &e, true));
        st.players.get_mut(&owner_sid).unwrap().buddies.entries.push(e);
    }
    info!("[amis] {bname} et {aname} sont amis");
    Ok(())
}

/// Op 3 : la cible refuse ; le demandeur l'apprend.
fn reject(st: &mut WorldState, sid: u32, name: &str) {
    let Some(asid) = st.by_name(name) else { return };
    let acid = st.players[&asid].ch.id;
    let me = st.players.get_mut(&sid).unwrap();
    if !me.buddies.requests.remove(&acid) {
        return;
    }
    let bname = me.ch.name.clone();
    send(st, asid, base(buddy_op::REJECT, acid).with(2, bname.as_str()));
    info!("[amis] {bname} refuse la demande de {name}");
}

/// Op 1 sans le bit « ami » : un ignoré de plus, connecté ou non.
async fn add_ignored(ctx: &WorldCtx, st: &mut WorldState, sid: u32, name: &str, blocks: u32) -> anyhow::Result<()> {
    let me = &st.players[&sid];
    let (my_cid, my_name) = (me.ch.id, me.ch.name.clone());
    if !valid_name(name, &my_name) {
        send_result(st, sid, buddy_result::INVALID);
        return Ok(());
    }
    if me.buddies.find(name).is_some() {
        send_result(st, sid, buddy_result::ALREADY);
        return Ok(());
    }
    if me.buddies.count(false) >= MAX_ENTRIES {
        send_result(st, sid, buddy_result::FULL);
        return Ok(());
    }
    let Some((cid, exact)) = find_character(ctx, st, name).await? else {
        send_result(st, sid, buddy_result::OFFLINE_OR_UNKNOWN);
        return Ok(());
    };
    if cid == my_cid || st.players[&sid].buddies.has(cid) {
        send_result(st, sid, if cid == my_cid { buddy_result::INVALID } else { buddy_result::ALREADY });
        return Ok(());
    }
    ctx.db.characters.exec("INSERT IGNORE INTO character_buddy (character_id, other_id, flags) VALUES (?, ?, ?)", (my_cid, cid, blocks)).await?;
    let e = Entry { cid, name: exact, flags: blocks, relation: 0 };
    send(st, sid, entry_fields(buddy_op::ADD, my_cid, &e, online(st, cid)));
    info!("[amis] {my_name} ignore {}", e.name);
    st.players.get_mut(&sid).unwrap().buddies.entries.push(e);
    Ok(())
}

/// Op 4 : retrait d'un ami ou d'un ignoré, chez le demandeur seulement.
async fn remove(ctx: &WorldCtx, st: &mut WorldState, sid: u32, name: &str) -> anyhow::Result<()> {
    let me = &st.players[&sid];
    let Some(e) = me.buddies.find(name).cloned() else { return Ok(()) };
    let my_cid = me.ch.id;
    ctx.db.characters.exec("DELETE FROM character_buddy WHERE character_id = ? AND other_id = ?", (my_cid, e.cid)).await?;
    st.players.get_mut(&sid).unwrap().buddies.entries.retain(|x| x.cid != e.cid);
    send(st, sid, entry_fields(buddy_op::REMOVE, my_cid, &e, online(st, e.cid)));
    info!("[amis] {} retire {} de sa liste {}", st.players[&sid].ch.name, e.name, if e.is_friend() { "d'amis" } else { "d'ignorés" });
    Ok(())
}

/// Op 5 : cases de blocage d'un ignoré.
async fn options(ctx: &WorldCtx, st: &mut WorldState, sid: u32, name: &str, flags: u32) -> anyhow::Result<()> {
    let me = st.players.get_mut(&sid).unwrap();
    let my_cid = me.ch.id;
    let Some(e) = me.buddies.entries.iter_mut().find(|e| !e.is_friend() && e.name.to_lowercase() == name.to_lowercase()) else { return Ok(()) };
    e.flags = (e.flags & !buddy_flag::BLOCKS) | (flags & buddy_flag::BLOCKS);
    let e = e.clone();
    ctx.db.characters.exec("UPDATE character_buddy SET flags = ? WHERE character_id = ? AND other_id = ?", (e.flags, my_cid, e.cid)).await?;
    send(st, sid, entry_fields(buddy_op::OPTIONS, my_cid, &e, online(st, e.cid)));
    info!("[amis] {} bloque {} : 0x{:x}", st.players[&sid].ch.name, e.name, e.flags);
    Ok(())
}

/// Op 7 : région où se trouve un ami (-1 hors ligne).
fn region(ctx: &WorldCtx, st: &WorldState, sid: u32, name: &str) {
    let me = &st.players[&sid];
    let Some(e) = me.buddies.find(name).filter(|e| e.is_friend()) else { return };
    let here = st.by_cid.get(&e.cid).and_then(|s| st.players.get(s)).filter(|p| p.in_game).map(|p| p.ch.pos());
    let region = here.and_then(|pos| ctx.map.region(pos.x as f64, pos.z as f64)).map(i64::from).unwrap_or(-1);
    me.send(&schema::BUDDY, entry_fields(buddy_op::REGION, me.ch.id, e, region >= 0).with(4, region));
}

/// Op 15 : ignorés du fichier local du client, ajoutés en base s'ils manquent (sans réponse : le client les a déjà).
async fn merge_list(ctx: &WorldCtx, st: &mut WorldState, sid: u32, list: &[(String, u32)]) -> anyhow::Result<()> {
    let me = st.players.get_mut(&sid).unwrap();
    if std::mem::replace(&mut me.buddies.merged, true) {
        return Ok(());
    }
    let (my_cid, my_name) = (me.ch.id, me.ch.name.clone());
    let mut added = Vec::new();
    for (name, flags) in list {
        let me = &st.players[&sid];
        if me.buddies.count(false) >= MAX_ENTRIES {
            break;
        }
        if !valid_name(name, &my_name) || me.buddies.find(name).is_some() {
            continue;
        }
        let Some((cid, exact)) = find_character(ctx, st, name).await? else { continue };
        if cid == my_cid || st.players[&sid].buddies.has(cid) {
            continue;
        }
        let flags = flags & buddy_flag::BLOCKS;
        ctx.db.characters.exec("INSERT IGNORE INTO character_buddy (character_id, other_id, flags) VALUES (?, ?, ?)", (my_cid, cid, flags)).await?;
        added.push(exact.clone());
        st.players.get_mut(&sid).unwrap().buddies.entries.push(Entry { cid, name: exact, flags, relation: 0 });
    }
    if !added.is_empty() {
        info!("[amis] {my_name} : {} ignoré(s) repris du fichier local ({})", added.len(), added.join(", "));
    }
    Ok(())
}

/// Entrée en jeu : les deux listes (op 1), la fin de liste (op 9), puis l'état en ligne chez ceux qui l'ont en ami.
pub fn on_enter(st: &WorldState, sid: u32) {
    let Some(p) = st.players.get(&sid) else { return };
    for e in &p.buddies.entries {
        p.send(&schema::BUDDY, entry_fields(buddy_op::ADD, p.ch.id, e, online(st, e.cid)));
    }
    p.send(&schema::BUDDY, base(buddy_op::END_OF_LIST, p.ch.id).with(4, 0));
    announce(st, sid, true);
}

/// Départ du monde : ceux qui l'ont en ami le voient hors ligne.
pub fn on_leave_world(st: &WorldState, sid: u32) {
    announce(st, sid, false);
}

/// Op 6 aux joueurs en jeu qui ont `sid` dans leur liste d'amis.
fn announce(st: &WorldState, sid: u32, is_online: bool) {
    let Some(me) = st.players.get(&sid) else { return };
    for (osid, other) in &st.players {
        if *osid == sid || !other.in_game {
            continue;
        }
        if let Some(e) = other.buddies.entries.iter().find(|e| e.cid == me.ch.id && e.is_friend()) {
            let e = Entry { name: me.ch.name.clone(), ..e.clone() };
            other.send(&schema::BUDDY, entry_fields(buddy_op::ONLINE, other.ch.id, &e, is_online));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archcore_net::{build_packet, Value};

    fn entry(cid: u32, name: &str, flags: u32) -> Entry {
        Entry { cid, name: name.into(), flags, relation: 0 }
    }

    #[test]
    fn listes_et_blocages() {
        let b = Buddies { entries: vec![entry(1, "Alice", buddy_flag::FRIEND), entry(2, "Bob", buddy_flag::BLOCK_TRADE)], ..Default::default() };
        assert_eq!(b.find("alice").map(|e| e.cid), Some(1));
        assert_eq!((b.count(true), b.count(false)), (1, 1));
        assert!(b.blocks(2, buddy_flag::BLOCK_TRADE));
        assert!(!b.blocks(2, buddy_flag::BLOCK_CHAT));
        assert!(!b.blocks(1, buddy_flag::BLOCK_TRADE), "un ami ne bloque rien");
        assert!(valid_name("Bob", "Alice") && !valid_name("alice", "Alice") && !valid_name("", "Alice"));
        assert!(!valid_name(&"x".repeat(33), "Alice"));
    }

    #[test]
    fn paquet_d_une_entree() {
        let e = entry(7, "Alice", buddy_flag::FRIEND);
        let raw = build_packet(&schema::BUDDY, &entry_fields(buddy_op::ADD, 42, &e, true));
        let p = decode_packet(&schema::BUDDY, &raw).unwrap();
        assert_eq!((p.int(0), p.int(1), p.text(2).as_deref()), (Some(1), Some(42), Some("Alice")));
        assert_eq!(p.int(3), Some((buddy_flag::FRIEND | buddy_flag::ONLINE) as i64));
        assert_eq!((p.int(4), p.int(5)), (Some(-1), Some(0)));
        // Champs fixes : 1 + 4 + 33 + 4 + 4 + 4 octets après les drapeaux (le client exige au moins 0x13 octets).
        assert_eq!(raw.len(), 19 + 4 + 50 + 1);
    }

    #[test]
    fn liste_du_fichier_local() {
        let s = &schema::BUDDY_LIST;
        let mut names = vec![0u8; 990];
        names[..5].copy_from_slice(b"Alice");
        names[33..36].copy_from_slice(b"Bob");
        let flags: Vec<Value> = (0..30).map(|i| Value::Int(if i == 1 { 3 } else { 0 })).collect();
        let raw = build_packet(s, &Fields::new(s).with(0, 15).with(1, 42).with(2, 3).with(3, names).with(4, flags));
        let p = decode_packet(s, &raw).unwrap();
        assert_eq!(list_entries(&p.fields), vec![("Alice".to_string(), 0), ("Bob".to_string(), 3)], "le troisième nom est vide");
        // Lu d'abord avec le schéma principal (même type) : ce décodage ne doit pas échouer.
        assert_eq!(decode_packet(&schema::BUDDY, &raw).unwrap().int(0), Some(15));
    }
}
