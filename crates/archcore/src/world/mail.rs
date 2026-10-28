//! Courrier entre joueurs (AGPMMAILBOX 0x2D, mini-paquet `mini::MAIL`).
//!
//! ```text
//!   Paquet (drapeaux 2) : 0 op, 1 CID du joueur, 2 courrier (mini MAIL), 3 IID de la pièce jointe (op 3), 4 résultat.
//!   Courrier (drapeaux 2) : 0 identifiant, 1 expéditeur, 2 destinataire (nom exact), 3 objet, 4 date (secondes Unix),
//!     5 drapeaux (0x01 lu, 0x10 pièce récupérée), 6 TID et 7 quantité de la pièce, 8 texte (zéro final compris).
//!   Entrée en jeu   ← op 0 par courrier, du plus ancien au plus récent (le client insère en tête)
//!   Écrire          → op 3 (destinataire, objet, texte ; champ 3 : IID joint ou 0)
//!                   ← AGPMITEM : papier à lettre décompté, pièce jointe retirée ; op 3 (résultat)
//!                   destinataire en jeu ← op 0 (« YouHaveMail »)
//!   Lire            → op 2 (identifiant) : marqué lu, sans réponse
//!   Supprimer       → op 1 (identifiant)    ← op 1 (résultat ; refus tant que la pièce n'est pas récupérée)
//!   Récupérer       → op 4 (identifiant)    ← AGPMITEM op 0 dans l'inventaire, op 4 (résultat, identifiant)
//! ```
//! Choix d'ArchCore (le serveur d'origine n'est pas connu) : une feuille « Ordinary Letter Paper » (TID 3601)
//! consommée par lettre, pas d'or joint, une seule pièce jointe (la pile entière) qui reste une ligne de
//! `character_items` au statut 4 (options et conversion gardées) rattachée au destinataire ; destinataire cherché
//! en base, hors ligne compris ; 30 courriers au plus par boîte ; pas d'expiration. Refus : destinataire inconnu
//! ou soi-même, boîte pleine, pas de papier, pièce interdite (papiers 3601-3602, TID 4609 à 4623 comme le client),
//! liée dès l'obtention (`BoundType` 1), de la boutique payante, hors de l'inventaire ; récupération refusée si
//! l'inventaire est plein. Le client ne distingue que le succès (0) : un message système précise le refus.
//! En base : `mail`.

use super::items::{convert_add, convert_of, item_add_of, item_count, item_remove, item_status, options_of, stored_status, InventoryItem, CONVERT_COLUMNS};
use super::{find_character, send_system, WorldCtx, WorldState};
use crate::db::{col, Databases};
use crate::{debug, info};
use archcore_net::schema::{self, mail_op, mail_result, mini};
use archcore_net::{build_mini, decode_mini, Fields, Packet};

/// Courriers au plus dans une boîte (ceux du client).
pub const MAX_MAILS: usize = 30;
/// Papier à lettre consommé par lettre envoyée.
pub const LETTER_PAPER: u32 = 3601;
pub const FINE_LETTER_PAPER: u32 = 3602;
/// Objets que le client refuse de joindre (bagues et colliers 4609-4619, potions de transmutation 4620-4623).
const FORBIDDEN: std::ops::RangeInclusive<u32> = 4609..=4623;
/// Drapeaux d'un courrier.
pub const READ: u32 = 0x01;
pub const ITEM_SAVED: u32 = 0x10;
/// Longueurs : nom 32 octets, objet 50, texte 999 (sans le zéro final).
const MAX_NAME: usize = 32;
const MAX_SUBJECT: usize = 50;
const MAX_BODY: usize = 999;

/// Pièce jointe en attente.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attachment {
    /// Ligne de `character_items`.
    pub iid: u32,
    pub tid: u32,
    pub count: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mail {
    pub id: u32,
    pub sender: String,
    pub subject: String,
    pub body: String,
    pub sent_at: u32,
    pub flags: u32,
    pub item: Option<Attachment>,
}

/// Boîte d'un joueur en jeu, du plus ancien au plus récent.
#[derive(Clone, Debug, Default)]
pub struct MailBox {
    pub mails: Vec<Mail>,
}

impl MailBox {
    pub async fn load(db: &Databases, cid: u32) -> anyhow::Result<Self> {
        let rows = db
            .characters
            .rows(
                "SELECT m.id, m.sender_name, m.subject, m.body, m.sent_at, m.flags, i.id AS iid, i.template_id, i.count \
                 FROM mail m LEFT JOIN character_items i ON i.id = m.item_id AND i.status = ? WHERE m.recipient_id = ? ORDER BY m.id",
                (stored_status::MAILBOX, cid),
            )
            .await?;
        let mails = rows
            .iter()
            .map(|r| {
                let iid: Option<u32> = col(r, "iid");
                Mail {
                    id: col(r, "id"),
                    sender: col(r, "sender_name"),
                    subject: col(r, "subject"),
                    body: col(r, "body"),
                    sent_at: col(r, "sent_at"),
                    flags: col(r, "flags"),
                    item: iid.map(|iid| Attachment { iid, tid: col(r, "template_id"), count: col(r, "count") }),
                }
            })
            .collect();
        Ok(MailBox { mails })
    }

    pub fn get(&self, id: u32) -> Option<&Mail> {
        self.mails.iter().find(|m| m.id == id)
    }
}

/// Mini-paquet d'un courrier pour son destinataire (`to` : son nom exact).
pub fn mail_mini(m: &Mail, to: &str) -> Vec<u8> {
    let s = &mini::MAIL;
    let mut body = archcore_net::text::encode(&m.body);
    body.push(0);
    let (tid, count) = m.item.as_ref().map(|i| (i.tid, i.count)).unwrap_or((0, 0));
    let f = Fields::new(s)
        .with(0, m.id)
        .with(1, m.sender.as_str())
        .with(2, to)
        .with(3, m.subject.as_str())
        .with(4, m.sent_at)
        .with(5, m.flags)
        .with(6, tid)
        .with(7, count)
        .with(8, body);
    build_mini(s, &f)
}

/// Mini-paquet réduit à l'identifiant (réponses aux ops 1 et 4).
fn id_mini(id: u32) -> Vec<u8> {
    let s = &mini::MAIL;
    build_mini(s, &Fields::new(s).with(0, id))
}

fn fields(op: i64, cid: u32) -> Fields {
    let s = &schema::MAILBOX;
    Fields::new(s).with(0, op).with(1, cid)
}

/// Courrier écrit par le client (op 3) : destinataire, objet, texte, contrôlés.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Draft {
    pub to: String,
    pub subject: String,
    pub body: String,
}

/// Lit le courrier d'une op 3 ; `None` s'il est mal formé (longueurs).
pub fn draft_of(mini_bytes: &[u8]) -> Option<Draft> {
    let f = decode_mini(&mini::MAIL, mini_bytes).ok()?;
    let to = f.text(2)?.trim().to_string();
    let subject = f.text(3)?.trim_end().to_string();
    let body = f.text(8).unwrap_or_default();
    let len = |s: &str| archcore_net::text::encode(s).len();
    let ok = (1..=MAX_NAME).contains(&len(&to)) && (1..=MAX_SUBJECT).contains(&len(&subject)) && len(&body) <= MAX_BODY;
    ok.then_some(Draft { to, subject, body })
}

/// Peut-on joindre cet objet ? Rend le code de refus.
pub fn attach_refusal(ctx: &WorldCtx, tid: u32) -> Option<i64> {
    if tid == LETTER_PAPER || tid == FINE_LETTER_PAPER || FORBIDDEN.contains(&tid) {
        return Some(mail_result::FORBIDDEN_ITEM);
    }
    let d = ctx.catalog.get(tid)?;
    if d.bound_type == 1 {
        return Some(mail_result::BOUND_ITEM);
    }
    d.cash.then_some(mail_result::CASH_ITEM)
}

/// Message système qui accompagne un refus (le client n'affiche qu'un échec général, ou rien).
fn refusal_text(code: i64) -> &'static str {
    match code {
        mail_result::NO_RECIPIENT => "Ce destinataire n'existe pas.",
        mail_result::BOX_FULL => "La boîte aux lettres du destinataire est pleine.",
        mail_result::NO_PAPER => "Il faut une feuille de papier à lettre (Ordinary Letter Paper).",
        mail_result::FORBIDDEN_ITEM => "Cet objet ne peut pas être envoyé par courrier.",
        mail_result::BOUND_ITEM => "Un objet lié ne peut pas être envoyé par courrier.",
        mail_result::CASH_ITEM => "Un objet de la boutique ne peut pas être envoyé par courrier.",
        mail_result::NOT_IN_INVENTORY => "L'objet joint doit être dans l'inventaire.",
        mail_result::INVENTORY_FULL => "Votre inventaire est plein.",
        mail_result::ITEM_PENDING => "Récupérez d'abord l'objet joint à ce courrier.",
        _ => "Le courrier n'a pas pu être envoyé.",
    }
}

fn refuse(st: &WorldState, sid: u32, op: i64, id: Option<u32>, code: i64) {
    let Some(p) = st.players.get(&sid) else { return };
    p.send(&schema::MAILBOX, fields(op, p.ch.id).with_opt(2, id.map(id_mini)).with(4, code));
    send_system(st, sid, refusal_text(code));
}

/// AGPMMAILBOX reçu.
pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    if !st.players.get(&sid).is_some_and(|p| p.in_game) {
        return Ok(());
    }
    let mini_bytes = p.bytes(2).unwrap_or_default();
    let id = || decode_mini(&mini::MAIL, mini_bytes).ok().and_then(|f| f.int(0)).unwrap_or(0) as u32;
    match p.int(0).unwrap_or(-1) {
        mail_op::WRITE => write(ctx, st, sid, mini_bytes, p.int(3).unwrap_or(0) as u32).await?,
        mail_op::READ => read(ctx, st, sid, id()).await?,
        mail_op::REMOVE => remove(ctx, st, sid, id()).await?,
        mail_op::ITEM_SAVE => save_item(ctx, st, sid, id()).await?,
        op => debug!("[courrier] #{sid} op {op} ignorée"),
    }
    Ok(())
}

/// Op 3 : envoi d'une lettre.
async fn write(ctx: &WorldCtx, st: &mut WorldState, sid: u32, mini_bytes: &[u8], iid: u32) -> anyhow::Result<()> {
    let refused = |st: &WorldState, code: i64| refuse(st, sid, mail_op::WRITE, None, code);
    let Some(draft) = draft_of(mini_bytes) else {
        refused(st, mail_result::FAILED);
        return Ok(());
    };
    let (my_cid, my_name) = (st.players[&sid].ch.id, st.players[&sid].ch.name.clone());
    let Some((to_cid, to_name)) = find_character(ctx, st, &draft.to).await? else {
        refused(st, mail_result::NO_RECIPIENT);
        return Ok(());
    };
    if to_cid == my_cid {
        refused(st, mail_result::FAILED);
        return Ok(());
    }
    let me = &st.players[&sid];
    let Some(paper) = me.inventory.items.values().find(|i| i.tid == LETTER_PAPER).cloned() else {
        refused(st, mail_result::NO_PAPER);
        return Ok(());
    };
    let attached = if iid == 0 {
        None
    } else {
        let in_trade = me.trade.as_ref().is_some_and(|t| t.items.contains_key(&iid));
        let Some(item) = me.inventory.get(iid).filter(|_| !in_trade).cloned() else {
            refused(st, mail_result::NOT_IN_INVENTORY);
            return Ok(());
        };
        if let Some(code) = attach_refusal(ctx, item.tid) {
            refused(st, code);
            return Ok(());
        }
        Some(item)
    };
    let to_sid = st.by_cid.get(&to_cid).copied().filter(|s| st.players.contains_key(s));
    let in_box = match to_sid {
        Some(s) => st.players[&s].mailbox.mails.len(),
        None => ctx.db.characters.first("SELECT COUNT(*) AS n FROM mail WHERE recipient_id = ?", (to_cid,)).await?.map(|r| col::<u64>(&r, "n")).unwrap_or(0) as usize,
    };
    if in_box >= MAX_MAILS {
        refused(st, mail_result::BOX_FULL);
        return Ok(());
    }
    // Papier décompté, pièce jointe passée au destinataire (statut 4), courrier inséré : d'un seul tenant.
    let sent_at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as u32).unwrap_or(0);
    let mut stmts: Vec<(&str, mysql_async::Params)> = Vec::new();
    if paper.count > 1 {
        stmts.push(("UPDATE character_items SET count = ? WHERE id = ?", (paper.count - 1, paper.iid).into()));
    } else {
        stmts.push(("DELETE FROM character_items WHERE id = ? AND character_id = ?", (paper.iid, my_cid).into()));
    }
    if let Some(item) = &attached {
        stmts.push((
            "UPDATE character_items SET character_id = ?, status = ?, layer = 0, row_idx = 0, col_idx = 0 WHERE id = ? AND character_id = ?",
            (to_cid, stored_status::MAILBOX, item.iid, my_cid).into(),
        ));
    }
    stmts.push((
        "INSERT INTO mail (recipient_id, sender_id, sender_name, subject, body, sent_at, flags, item_id) VALUES (?, ?, ?, ?, ?, ?, 0, ?)",
        (to_cid, my_cid, my_name.clone(), draft.subject.clone(), draft.body.clone(), sent_at, attached.as_ref().map(|i| i.iid)).into(),
    ));
    let id = ctx.db.characters.atomic(stmts).await? as u32;
    let me = st.players.get_mut(&sid).unwrap();
    if paper.count > 1 {
        let left = me.inventory.items.get_mut(&paper.iid).unwrap();
        left.count -= 1;
        let left = left.clone();
        me.send_raw(item_count(&left, my_cid));
    } else {
        me.inventory.items.remove(&paper.iid);
        me.send_raw(item_remove(paper.iid, Some(my_cid)));
    }
    if let Some(item) = &attached {
        me.inventory.items.remove(&item.iid);
        me.send_raw(item_remove(item.iid, Some(my_cid)));
    }
    me.send(&schema::MAILBOX, fields(mail_op::WRITE, my_cid).with(4, mail_result::OK));
    let mail = Mail {
        id,
        sender: my_name.clone(),
        subject: draft.subject,
        body: draft.body,
        sent_at,
        flags: 0,
        item: attached.as_ref().map(|i| Attachment { iid: i.iid, tid: i.tid, count: i.count }),
    };
    let what = attached.as_ref().map(|i| format!(" avec {} × {}", i.count, ctx.catalog.get(i.tid).map(|d| d.name.as_str()).unwrap_or("?"))).unwrap_or_default();
    info!("[courrier] {my_name} écrit à {to_name}{what} (courrier {id}{})", if to_sid.is_some() { "" } else { ", hors ligne" });
    // Destinataire en jeu : délivré tout de suite ; encore en chargement, il le recevra à l'entrée.
    if let Some(p) = to_sid.and_then(|s| st.players.get_mut(&s)) {
        if p.in_game {
            p.send(&schema::MAILBOX, fields(mail_op::ADD, p.ch.id).with(2, mail_mini(&mail, &p.ch.name)));
        }
        p.mailbox.mails.push(mail);
    }
    Ok(())
}

/// Op 2 : courrier ouvert, marqué lu.
async fn read(ctx: &WorldCtx, st: &mut WorldState, sid: u32, id: u32) -> anyhow::Result<()> {
    let p = st.players.get_mut(&sid).unwrap();
    let cid = p.ch.id;
    let Some(m) = p.mailbox.mails.iter_mut().find(|m| m.id == id) else { return Ok(()) };
    if m.flags & READ == 0 {
        m.flags |= READ;
        ctx.db.characters.exec("UPDATE mail SET flags = flags | ? WHERE id = ? AND recipient_id = ?", (READ, id, cid)).await?;
    }
    Ok(())
}

/// Op 1 : suppression, refusée tant que la pièce jointe attend.
async fn remove(ctx: &WorldCtx, st: &mut WorldState, sid: u32, id: u32) -> anyhow::Result<()> {
    let p = &st.players[&sid];
    let cid = p.ch.id;
    let Some(m) = p.mailbox.get(id) else {
        refuse(st, sid, mail_op::REMOVE, Some(id), mail_result::FAILED);
        return Ok(());
    };
    if m.item.is_some() {
        refuse(st, sid, mail_op::REMOVE, Some(id), mail_result::ITEM_PENDING);
        return Ok(());
    }
    ctx.db.characters.exec("DELETE FROM mail WHERE id = ? AND recipient_id = ? AND item_id IS NULL", (id, cid)).await?;
    let p = st.players.get_mut(&sid).unwrap();
    p.mailbox.mails.retain(|m| m.id != id);
    p.send(&schema::MAILBOX, fields(mail_op::REMOVE, cid).with(2, id_mini(id)).with(4, mail_result::OK));
    info!("[courrier] {} supprime le courrier {id}", p.ch.name);
    Ok(())
}

/// Op 4 : pièce jointe rangée dans l'inventaire (première case libre).
async fn save_item(ctx: &WorldCtx, st: &mut WorldState, sid: u32, id: u32) -> anyhow::Result<()> {
    let p = &st.players[&sid];
    let cid = p.ch.id;
    let Some(item) = p.mailbox.get(id).and_then(|m| m.item.clone()) else {
        refuse(st, sid, mail_op::ITEM_SAVE, Some(id), mail_result::FAILED);
        return Ok(());
    };
    let Some(grid) = p.inventory.free_slot() else {
        refuse(st, sid, mail_op::ITEM_SAVE, Some(id), mail_result::INVENTORY_FULL);
        return Ok(());
    };
    let Some(row) = ctx
        .db
        .characters
        .first(&format!("SELECT template_id, count, {CONVERT_COLUMNS} FROM character_items WHERE id = ? AND character_id = ? AND status = ?"), (item.iid, cid, stored_status::MAILBOX))
        .await?
    else {
        refuse(st, sid, mail_op::ITEM_SAVE, Some(id), mail_result::FAILED);
        return Ok(());
    };
    ctx.db
        .characters
        .atomic(vec![
            (
                "UPDATE character_items SET status = ?, layer = ?, row_idx = ?, col_idx = ? WHERE id = ? AND character_id = ?",
                (stored_status::INVENTORY, grid[0], grid[1], grid[2], item.iid, cid).into(),
            ),
            ("UPDATE mail SET item_id = NULL, flags = flags | ? WHERE id = ? AND recipient_id = ?", (ITEM_SAVED, id, cid).into()),
        ])
        .await?;
    let taken = InventoryItem { iid: item.iid, tid: col(&row, "template_id"), count: col(&row, "count"), grid, conv: convert_of(&row), options: options_of(&row), durability: super::items::durability_of(&row) };
    let p = st.players.get_mut(&sid).unwrap();
    if let Some(m) = p.mailbox.mails.iter_mut().find(|m| m.id == id) {
        m.item = None;
        m.flags |= ITEM_SAVED;
    }
    p.inventory.items.insert(taken.iid, taken.clone());
    p.send_raw(item_add_of(&taken, cid, item_status::INVENTORY, Some(grid)));
    if let Some(c) = convert_add(&taken, cid) {
        p.send_raw(c);
    }
    p.send(&schema::MAILBOX, fields(mail_op::ITEM_SAVE, cid).with(2, id_mini(id)).with(4, mail_result::OK));
    info!("[courrier] {} récupère {} × {} (courrier {id})", p.ch.name, taken.count, ctx.catalog.get(taken.tid).map(|d| d.name.as_str()).unwrap_or("?"));
    Ok(())
}

/// Entrée en jeu : chaque courrier (op 0), du plus ancien au plus récent.
pub fn on_enter(st: &WorldState, sid: u32) {
    let Some(p) = st.players.get(&sid) else { return };
    for m in &p.mailbox.mails {
        p.send(&schema::MAILBOX, fields(mail_op::ADD, p.ch.id).with(2, mail_mini(m, &p.ch.name)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archcore_net::{build_packet, decode_packet};

    fn letter() -> Mail {
        Mail { id: 12, sender: "GmTest".into(), subject: "Bonjour".into(), body: "Une potion pour toi.".into(), sent_at: 1_800_000_000, flags: 0, item: Some(Attachment { iid: 99, tid: 190, count: 3 }) }
    }

    #[test]
    fn mini_paquet_d_un_courrier() {
        let raw = build_packet(&schema::MAILBOX, &fields(mail_op::ADD, 7).with(2, mail_mini(&letter(), "SimA")));
        let p = decode_packet(&schema::MAILBOX, &raw).unwrap();
        assert_eq!((p.int(0), p.int(1)), (Some(0), Some(7)));
        let m = decode_mini(&mini::MAIL, p.bytes(2).unwrap()).unwrap();
        assert_eq!((m.int(0), m.text(1).as_deref(), m.text(2).as_deref(), m.text(3).as_deref()), (Some(12), Some("GmTest"), Some("SimA"), Some("Bonjour")));
        assert_eq!((m.int(4), m.int(5), m.int(6), m.int(7)), (Some(1_800_000_000), Some(0), Some(190), Some(3)));
        let body = m.bytes(8).unwrap();
        assert_eq!(body.last(), Some(&0), "texte envoyé avec son zéro final");
        assert_eq!(m.text(8).as_deref(), Some("Une potion pour toi."));
    }

    #[test]
    fn courrier_ecrit_par_le_client() {
        let s = &mini::MAIL;
        let write = |to: &str, subject: &str, body: &[u8]| build_mini(s, &Fields::new(s).with(0, 0).with(1, "GmTest").with(2, to).with(3, subject).with(4, 0).with(5, 0).with(6, 0).with(7, 0).with(8, body.to_vec()));
        let d = draft_of(archcore_net::packet::mini_payload(&write("SimA  ", "Objet", b"Texte\0"))).unwrap();
        assert_eq!(d, Draft { to: "SimA".into(), subject: "Objet".into(), body: "Texte".into() });
        assert!(draft_of(archcore_net::packet::mini_payload(&write("", "Objet", b"x\0"))).is_none(), "destinataire vide");
        assert!(draft_of(archcore_net::packet::mini_payload(&write("SimA", "", b"x\0"))).is_none(), "objet vide");
        assert!(draft_of(archcore_net::packet::mini_payload(&write("SimA", "Objet", &[b'a'; 1200]))).is_none(), "texte trop long");
    }

    #[test]
    fn reponse_reduite_a_l_identifiant() {
        let m = decode_mini(&mini::MAIL, archcore_net::packet::mini_payload(&id_mini(5))).unwrap();
        assert_eq!((m.int(0), m.bytes(1)), (Some(5), None));
    }
}
