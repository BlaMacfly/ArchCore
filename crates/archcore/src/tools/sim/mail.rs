//! `archcore sim mail` : courrier entre joueurs (AgpmMailBox 0x2D).
//!
//! Courriers des deux personnages effacés. Le MJ, seul en jeu, reçoit 3 feuilles de papier à lettre (TID 3601) et 5
//! potions. Refus : destinataire inconnu (code 2), lui-même (code 1), papier joint (code 5). Il écrit à SimA, hors
//! ligne, avec les potions : potions retirées, une feuille décomptée, op 3 code 0. SimA entre : courrier reçu (op 0)
//! avec sa pièce ; lu (op 2, noté en base) ; suppression refusée tant que la pièce attend (code 11) ; pièce
//! récupérée (op 4, potions dans l'inventaire) ; supprimé (op 1). Une seconde lettre, SimA en jeu, arrive aussitôt.

use super::{enter_world, step, InWorld, Recv};
use archcore_net::schema::{self, chat_type, mail_op, mail_result, mini};
use archcore_net::{build_mini, decode_mini, Fields};

const PAPER: i64 = 3601;
const POTION: i64 = 190;

fn say(w: &InWorld, text: &str) -> Fields {
    let ch = &schema::CHATTING;
    Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text))
}

/// Courrier écrit comme le client : identifiant, date, drapeaux, TID et quantité à 0, texte avec son zéro.
pub(super) fn letter(from: &str, to: &str, subject: &str, body: &str) -> Vec<u8> {
    let s = &mini::MAIL;
    let mut text = crate::tools::to_cp1252(body);
    text.push(0);
    build_mini(s, &Fields::new(s).with(0, 0).with(1, from).with(2, to).with(3, subject).with(4, 0).with(5, 0).with(6, 0).with(7, 0).with(8, text))
}

pub(super) fn mail(op: i64, cid: u32) -> Fields {
    let s = &schema::MAILBOX;
    Fields::new(s).with(0, op).with(1, cid)
}

pub(super) fn id_only(id: i64) -> Vec<u8> {
    let s = &mini::MAIL;
    build_mini(s, &Fields::new(s).with(0, id))
}

fn is_mail(op: i64) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| q.is(&schema::MAILBOX, Some(op))
}

/// Réponse attendue à une op, avec ce code.
pub(super) async fn expect_code(w: &mut InWorld, op: i64, code: i64, what: &str) -> anyhow::Result<Recv> {
    let r = w.world.expect_where(is_mail(op), 3000, what).await?;
    anyhow::ensure!(r.int(4) == Some(code) && r.int(1) == Some(w.cid as i64), "{what} : code {:?}, CID {:?}", r.int(4), r.int(1));
    println!("  ✓ {what} : op {op}, code {code}");
    Ok(r)
}

/// Objet reçu par une commande MJ : IID.
pub(super) async fn give(w: &mut InWorld, tid: i64, n: i64) -> anyhow::Result<i64> {
    let f = say(w, &format!("/additem {tid} {n}"));
    w.world.send(&schema::CHATTING, f).await?;
    let it = &schema::ITEM;
    let got = w.world.expect_where(|q| q.is(it, None) && q.int(3) == Some(tid), 3000, &format!("objet {tid}")).await?;
    Ok(got.int(2).unwrap_or(0))
}

pub async fn run(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let (mb, it) = (&schema::MAILBOX, &schema::ITEM);
    let db = crate::db::Databases::new(&cfg.database);
    let (ida, idb) = (super::character_id(&db, ca).await?, super::character_id(&db, cb).await?);
    let wipe = || async {
        db.characters.exec("DELETE FROM mail WHERE recipient_id IN (?, ?) OR sender_id IN (?, ?)", (ida, idb, ida, idb)).await?;
        db.characters.exec("DELETE FROM character_items WHERE character_id IN (?, ?) AND status = 4", (ida, idb)).await
    };
    wipe().await?;

    step(&format!("{ca} entre seul, avec du papier à lettre et des potions"));
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    anyhow::ensure!(a.world.expect_where(is_mail(mail_op::ADD), 800, "courrier").await.is_err(), "boîte de {ca} non vide");
    let paper = give(&mut a, PAPER, 3).await?;
    let potions = give(&mut a, POTION, 5).await?;
    let acid = a.cid;
    println!("  ✓ papier {paper}, potions {potions}");

    step("envois refusés");
    a.world.send(mb, mail(mail_op::WRITE, acid).with(2, letter(ca, "Personne", "Bonjour", "Rien.")).with(3, 0)).await?;
    expect_code(&mut a, mail_op::WRITE, mail_result::NO_RECIPIENT, "destinataire inconnu").await?;
    a.world.send(mb, mail(mail_op::WRITE, acid).with(2, letter(ca, ca, "Bonjour", "Moi.")).with(3, 0)).await?;
    expect_code(&mut a, mail_op::WRITE, mail_result::FAILED, "à soi-même").await?;
    a.world.send(mb, mail(mail_op::WRITE, acid).with(2, letter(ca, cb, "Papier", "Tiens.")).with(3, paper)).await?;
    expect_code(&mut a, mail_op::WRITE, mail_result::FORBIDDEN_ITEM, "papier à lettre joint").await?;

    step(&format!("{ca} écrit à {cb} (hors ligne) en joignant les potions"));
    a.world.send(mb, mail(mail_op::WRITE, acid).with(2, letter(ca, cb, "Des potions", "Cinq potions pour la route.")).with(3, potions)).await?;
    let left = a.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(paper), 3000, "papier décompté").await?;
    anyhow::ensure!(left.int(5) == Some(2), "papier restant {:?}", left.int(5));
    a.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(potions), 3000, "potions retirées").await?;
    expect_code(&mut a, mail_op::WRITE, mail_result::OK, "lettre envoyée").await?;
    println!("  ✓ potions retirées (op 1), 2 feuilles restantes");

    step(&format!("{cb} entre : le courrier l'attend"));
    let mut b = enter_world(&cfg, ub, pb, cb, "B-").await?;
    let bcid = b.cid;
    let got = b.world.expect_where(is_mail(mail_op::ADD), 5000, "courrier à l'entrée (op 0)").await?;
    anyhow::ensure!(got.int(1) == Some(bcid as i64), "CID {:?}", got.int(1));
    let m = decode_mini(&mini::MAIL, got.fields().bytes(2).unwrap_or_default())?;
    let id = m.int(0).unwrap_or(0);
    anyhow::ensure!(id > 0 && m.text(1).as_deref() == Some(ca) && m.text(2).as_deref() == Some(cb) && m.text(3).as_deref() == Some("Des potions"), "courrier {m:?}");
    anyhow::ensure!(m.int(6) == Some(POTION) && m.int(7) == Some(5) && m.int(5) == Some(0) && m.int(4).is_some_and(|t| t > 0), "pièce jointe {m:?}");
    anyhow::ensure!(m.text(8).as_deref() == Some("Cinq potions pour la route."), "texte {:?}", m.text(8));
    println!("  ✓ courrier {id} de {ca} : 5 × potion, non lu");

    step(&format!("{cb} lit le courrier, ne peut pas le supprimer avant de récupérer la pièce"));
    b.world.send(mb, mail(mail_op::READ, bcid).with(2, id_only(id))).await?;
    b.world.send(mb, mail(mail_op::REMOVE, bcid).with(2, id_only(id))).await?;
    expect_code(&mut b, mail_op::REMOVE, mail_result::ITEM_PENDING, "suppression refusée").await?;
    let flags: u32 = db.characters.first("SELECT flags FROM mail WHERE id = ?", (id,)).await?.map(|r| crate::db::col(&r, "flags")).unwrap_or(0);
    anyhow::ensure!(flags & 1 == 1, "drapeaux en base {flags:#x}");
    println!("  ✓ lu en base (0x{flags:x})");

    step(&format!("{cb} récupère les potions, puis supprime le courrier"));
    b.world.send(mb, mail(mail_op::ITEM_SAVE, bcid).with(2, id_only(id))).await?;
    let item = b.world.expect_where(|q| q.is(it, Some(0)) && q.int(3) == Some(POTION), 3000, "potions dans l'inventaire").await?;
    anyhow::ensure!(item.int(5) == Some(5) && item.int(4) == Some(bcid as i64) && item.int(1) == Some(2) && item.int(2) == Some(potions), "objet {:?} × {:?}", item.int(2), item.int(5));
    let saved = expect_code(&mut b, mail_op::ITEM_SAVE, mail_result::OK, "pièce récupérée").await?;
    let sm = decode_mini(&mini::MAIL, saved.fields().bytes(2).unwrap_or_default())?;
    anyhow::ensure!(sm.int(0) == Some(id), "identifiant de la réponse {:?}", sm.int(0));
    b.world.send(mb, mail(mail_op::ITEM_SAVE, bcid).with(2, id_only(id))).await?;
    expect_code(&mut b, mail_op::ITEM_SAVE, mail_result::FAILED, "seconde récupération").await?;
    b.world.send(mb, mail(mail_op::REMOVE, bcid).with(2, id_only(id))).await?;
    expect_code(&mut b, mail_op::REMOVE, mail_result::OK, "courrier supprimé").await?;
    println!("  ✓ même objet (IID {potions}) chez {cb}, courrier supprimé");

    step(&format!("seconde lettre, {cb} en jeu : délivrée tout de suite"));
    a.world.send(mb, mail(mail_op::WRITE, acid).with(2, letter(ca, cb, "Encore", "Sans pièce jointe.")).with(3, 0)).await?;
    expect_code(&mut a, mail_op::WRITE, mail_result::OK, "lettre envoyée").await?;
    let got = b.world.expect_where(is_mail(mail_op::ADD), 3000, "courrier en direct (op 0)").await?;
    let m = decode_mini(&mini::MAIL, got.fields().bytes(2).unwrap_or_default())?;
    anyhow::ensure!(m.text(3).as_deref() == Some("Encore") && m.int(6) == Some(0), "courrier {m:?}");
    let id2 = m.int(0).unwrap_or(0);
    b.world.send(mb, mail(mail_op::REMOVE, bcid).with(2, id_only(id2))).await?;
    expect_code(&mut b, mail_op::REMOVE, mail_result::OK, "supprimé").await?;
    println!("  ✓ courrier {id2} reçu sans attendre, supprimé");

    wipe().await?;
    println!("\ncourrier : OK");
    Ok(())
}
