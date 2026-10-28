//! `archcore sim buddy` : liste d'amis et d'ignorés (AgpmBuddy 0x2B).
//!
//! Listes vidées en base. Les deux joueurs entrent au même endroit : listes vides (op 9), le MJ envoie le fichier
//! local d'ignorés (op 15, SimB). Refus : soi-même (code 4), nom inconnu (code 2), option « refus d'ami » de l'autre
//! (op 3) ; relation Buddy reportée (code 12). Demande acceptée : op 1 des deux côtés, doublon (code 1), région de
//! l'ami (op 7). L'ami se reconnecte : op 6 hors ligne, puis en ligne. Il retire le MJ (chez lui
//! seulement) et l'ignore avec « Block Chat » et « Block Trade » : chuchotement non transmis, échange refusé
//! (op 25) ; invitation de groupe acceptée tant que « Block Invites » n'est pas coché, refusée ensuite (op 14). Cases
//! décochées : le chuchotement passe. Le MJ revient : son ami et l'ignoré repris du fichier local. Listes vidées à la fin.

use super::{enter_world, step, InWorld, Recv, TestClient};
use archcore_net::schema::{self, buddy_flag, buddy_op, buddy_result, chat_type};
use archcore_net::{build_packet, Fields, Value, Vec3};

const SPOT: Vec3 = Vec3 { x: -476_000.0, y: 4_422.0, z: -61_000.0 };
const STOP: i64 = 0x04;

/// Paquets reçus jusqu'au premier qui satisfait `last` (compris).
async fn until(w: &mut TestClient, last: impl Fn(&Recv) -> bool, ms: u64, what: &str) -> anyhow::Result<Vec<Recv>> {
    let mut got = Vec::new();
    loop {
        let q = w.expect_where(|_| true, ms, what).await?;
        let end = last(&q);
        got.push(q);
        if end {
            return Ok(got);
        }
    }
}

fn buddy(op: i64, cid: u32, name: &str, flags: u32) -> Fields {
    let s = &schema::BUDDY;
    Fields::new(s).with(0, op).with(1, cid).with(2, name).with(3, flags as i64).with(4, -1).with(5, 0).with(6, 0)
}

fn is_buddy(op: i64) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| q.is(&schema::BUDDY, Some(op))
}

/// Op 8 attendue avec ce code.
async fn expect_code(w: &mut InWorld, code: i64, what: &str) -> anyhow::Result<()> {
    let r = w.world.expect_where(is_buddy(buddy_op::RESULT), 3000, what).await?;
    anyhow::ensure!(r.int(6) == Some(code), "{what} : code {:?}", r.int(6));
    println!("  ✓ {what} : op 8, code {code}");
    Ok(())
}

/// Entrées reçues (op 1) jusqu'à la fin de liste (op 9) : (nom, drapeaux).
async fn entries(w: &mut InWorld) -> anyhow::Result<Vec<(String, u32)>> {
    let got = until(&mut w.world, is_buddy(buddy_op::END_OF_LIST), 5000, "fin de la liste (op 9)").await?;
    Ok(got.iter().filter(|q| q.is(&schema::BUDDY, Some(buddy_op::ADD))).map(|q| (q.fields().text(2).unwrap_or_default(), q.int(3).unwrap_or(0) as u32)).collect())
}

fn whisper(cid: u32, to: &str, text: &str) -> Fields {
    let ch = &schema::CHATTING;
    Fields::new(ch).with(0, 0).with(1, cid).with(2, chat_type::WHISPER).with(5, to).with(6, crate::tools::to_cp1252(text))
}

fn got_whisper(text: &'static str) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| q.is(&schema::CHATTING, None) && q.int(2) == Some(chat_type::WHISPER) && q.fields().text(6).is_some_and(|t| t.contains(text))
}

pub async fn run(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let (mv, c, tr, pm, bd) = (&schema::OPT_CHAR_MOVE, &schema::CHARACTER, &schema::PRIVATE_TRADE, &schema::PARTY_MANAGEMENT, &schema::BUDDY);
    let db = crate::db::Databases::new(&cfg.database);
    let (ida, idb) = (super::character_id(&db, ca).await?, super::character_id(&db, cb).await?);
    let wipe = || async {
        db.characters.exec("DELETE FROM character_buddy WHERE character_id IN (?, ?) OR other_id IN (?, ?)", (ida, idb, ida, idb)).await?;
        db.characters.exec("UPDATE characters SET option_flag = 0 WHERE id IN (?, ?)", (ida, idb)).await
    };
    wipe().await?;
    let place = |w: &InWorld| Fields::new(mv).with(0, w.cid).with(1, SPOT).with(2, SPOT).with(5, STOP).with(6, 0);

    step(&format!("{ca} et {cb} entrent en jeu : listes vides"));
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    let list = entries(&mut a).await?;
    anyhow::ensure!(list.is_empty(), "liste de {ca} : {list:?}");
    let mut b = enter_world(&cfg, ub, pb, cb, "B-").await?;
    let list = entries(&mut b).await?;
    anyhow::ensure!(list.is_empty(), "liste de {cb} : {list:?}");
    for w in [&mut a, &mut b] {
        let f = place(w);
        w.world.send(mv, f).await?;
    }
    let (acid, bcid) = (a.cid, b.cid);
    println!("  ✓ op 9 sans entrée des deux côtés");

    step(&format!("{ca} envoie son fichier local d'ignorés (op 15 : SimB)"));
    let s = &schema::BUDDY_LIST;
    let mut names = vec![0u8; 990];
    names[..4].copy_from_slice(b"SimB");
    let flags: Vec<Value> = (0..30).map(|_| Value::Int(0)).collect();
    a.world.send_plain(build_packet(s, &Fields::new(s).with(0, buddy_op::LIST).with(1, acid).with(2, 1).with(3, names).with(4, flags))).await?;
    anyhow::ensure!(a.world.expect_where(|q| q.ptype == bd.id, 800, "réponse à l'op 15").await.is_err(), "réponse inattendue à l'op 15");
    println!("  ✓ fusionné sans réponse (relu à la fin)");

    step("demandes refusées par le serveur");
    a.world.send(bd, buddy(buddy_op::REQUEST, acid, ca, buddy_flag::FRIEND)).await?;
    expect_code(&mut a, buddy_result::INVALID, "soi-même").await?;
    a.world.send(bd, buddy(buddy_op::REQUEST, acid, "Personne", buddy_flag::FRIEND)).await?;
    expect_code(&mut a, buddy_result::OFFLINE_OR_UNKNOWN, "nom inconnu").await?;
    a.world.send(bd, buddy(buddy_op::MENTOR_REQUEST, acid, cb, buddy_flag::FRIEND)).await?;
    expect_code(&mut a, buddy_result::MENTOR_INVALID, "relation Buddy (reportée)").await?;
    b.world.send(c, Fields::new(c).with(0, 0x11).with(1, bcid).with(22, 0x80)).await?;
    b.world.expect_where(|q| q.is(c, Some(0x11)), 3000, "option « refus d'ami »").await?;
    a.world.send(bd, buddy(buddy_op::REQUEST, acid, cb, buddy_flag::FRIEND)).await?;
    let r = a.world.expect_where(is_buddy(buddy_op::REJECT), 3000, "refus (op 3)").await?;
    anyhow::ensure!(r.fields().text(2).as_deref() == Some(cb), "refus de {:?}", r.fields().text(2));
    anyhow::ensure!(b.world.expect_where(is_buddy(buddy_op::REQUEST), 800, "demande").await.is_err(), "{cb} a reçu la demande malgré son option");
    b.world.send(c, Fields::new(c).with(0, 0x11).with(1, bcid).with(22, 0)).await?;
    b.world.expect_where(|q| q.is(c, Some(0x11)), 3000, "option levée").await?;
    println!("  ✓ option 0x80 : op 3 aussitôt, rien chez {cb}");

    step(&format!("{ca} demande {cb} en ami, {cb} accepte"));
    a.world.send(bd, buddy(buddy_op::REQUEST, acid, cb, buddy_flag::FRIEND)).await?;
    let ask = b.world.expect_where(is_buddy(buddy_op::REQUEST), 3000, "demande reçue (op 2)").await?;
    anyhow::ensure!(ask.int(1) == Some(bcid as i64) && ask.fields().text(2).as_deref() == Some(ca), "demande : cid {:?}, nom {:?}", ask.int(1), ask.fields().text(2));
    b.world.send(bd, buddy(buddy_op::ADD, bcid, ca, buddy_flag::FRIEND)).await?;
    let both = buddy_flag::FRIEND | buddy_flag::ONLINE;
    for (w, other, me) in [(&mut a, cb, acid), (&mut b, ca, bcid)] {
        let add = w.world.expect_where(is_buddy(buddy_op::ADD), 3000, "ami ajouté (op 1)").await?;
        anyhow::ensure!(add.int(1) == Some(me as i64) && add.fields().text(2).as_deref() == Some(other) && add.int(3) == Some(both as i64), "op 1 : {:?} {:?} 0x{:x}", add.int(1), add.fields().text(2), add.int(3).unwrap_or(0));
    }
    println!("  ✓ op 1 chez les deux (0x{both:x})");
    a.world.send(bd, buddy(buddy_op::REQUEST, acid, cb, buddy_flag::FRIEND)).await?;
    expect_code(&mut a, buddy_result::ALREADY, "déjà ami").await?;
    a.world.send(bd, buddy(buddy_op::REGION, acid, cb, buddy_flag::FRIEND).with(4, 0)).await?;
    let detail = a.world.expect_where(is_buddy(buddy_op::REGION), 3000, "région de l'ami (op 7)").await?;
    anyhow::ensure!(detail.int(4).is_some_and(|r| r >= 0), "région {:?}", detail.int(4));
    println!("  ✓ op 7 : région {}", detail.int(4).unwrap_or(-1));

    // Une connexion coupée garde le personnage 60 s (reconnexion) : la nouvelle entrée du compte le fait sortir.
    step(&format!("{cb} se reconnecte : il sort du monde puis y revient"));
    drop(b);
    let mut b = enter_world(&cfg, ub, pb, cb, "B-").await?;
    let list = entries(&mut b).await?;
    anyhow::ensure!(list == vec![(ca.to_string(), both)], "liste relue : {list:?}");
    let off = a.world.expect_where(is_buddy(buddy_op::ONLINE), 5000, "hors ligne (op 6)").await?;
    anyhow::ensure!(off.fields().text(2).as_deref() == Some(cb) && off.int(3) == Some(buddy_flag::FRIEND as i64), "op 6 : {:?} 0x{:x}", off.fields().text(2), off.int(3).unwrap_or(0));
    println!("  ✓ op 6 sans le bit en ligne");
    let on = a.world.expect_where(is_buddy(buddy_op::ONLINE), 5000, "en ligne (op 6)").await?;
    anyhow::ensure!(on.int(3) == Some(both as i64), "op 6 : 0x{:x}", on.int(3).unwrap_or(0));
    let f = place(&b);
    b.world.send(mv, f).await?;
    println!("  ✓ liste relue de la base, op 6 en ligne chez {ca}");

    step(&format!("{cb} retire {ca} et l'ignore (discussion et échange bloqués)"));
    b.world.send(bd, buddy(buddy_op::REMOVE, bcid, ca, buddy_flag::FRIEND)).await?;
    b.world.expect_where(is_buddy(buddy_op::REMOVE), 3000, "retrait (op 4)").await?;
    anyhow::ensure!(a.world.expect_where(is_buddy(buddy_op::REMOVE), 800, "retrait").await.is_err(), "{ca} a perdu son ami");
    b.world.send(bd, buddy(buddy_op::ADD, bcid, ca, 0)).await?;
    let add = b.world.expect_where(is_buddy(buddy_op::ADD), 3000, "ignoré ajouté (op 1)").await?;
    anyhow::ensure!(add.int(3).is_some_and(|f| f as u32 & buddy_flag::FRIEND == 0), "drapeaux 0x{:x}", add.int(3).unwrap_or(0));
    let blocks = buddy_flag::BLOCK_CHAT | buddy_flag::BLOCK_TRADE;
    b.world.send(bd, buddy(buddy_op::OPTIONS, bcid, ca, blocks)).await?;
    let opt = b.world.expect_where(is_buddy(buddy_op::OPTIONS), 3000, "cases (op 5)").await?;
    anyhow::ensure!(opt.int(3).is_some_and(|f| f as u32 & buddy_flag::BLOCKS == blocks), "drapeaux 0x{:x}", opt.int(3).unwrap_or(0));
    println!("  ✓ op 4 chez {cb} seulement, op 1 sans le bit ami, op 5 (0x{blocks:x})");
    a.world.send(&schema::CHATTING, whisper(acid, cb, "tu m'entends ?")).await?;
    a.world.expect_where(got_whisper("tu m'entends"), 3000, "écho du chuchotement").await?;
    anyhow::ensure!(b.world.expect_where(got_whisper("tu m'entends"), 1000, "chuchotement").await.is_err(), "chuchotement transmis malgré le blocage");
    println!("  ✓ chuchotement non transmis (l'auteur voit son écho)");
    a.world.send(tr, Fields::new(tr).with(0, 1).with(1, acid).with(2, bcid)).await?;
    let r = a.world.expect_where(|q| q.is(tr, Some(25)), 3000, "refus d'échange (op 25)").await?;
    anyhow::ensure!(r.int(1) == Some(bcid as i64), "personnage annoncé {:?}", r.int(1));
    anyhow::ensure!(b.world.expect_where(|q| q.is(tr, Some(3)), 800, "demande d'échange").await.is_err(), "{cb} a reçu la demande d'échange");
    println!("  ✓ échange refusé (op 25), rien chez {cb}");
    a.world.send(pm, Fields::new(pm).with(0, 0).with(1, acid).with(2, bcid)).await?;
    b.world.expect_where(|q| q.is(pm, Some(0)), 3000, "invitation de groupe (non bloquée)").await?;
    b.world.send(pm, Fields::new(pm).with(0, 1).with(1, bcid).with(2, acid)).await?;
    b.world.send(bd, buddy(buddy_op::OPTIONS, bcid, ca, blocks | buddy_flag::BLOCK_INVITE)).await?;
    b.world.expect_where(is_buddy(buddy_op::OPTIONS), 3000, "cases (op 5)").await?;
    a.world.send(pm, Fields::new(pm).with(0, 0).with(1, acid).with(2, bcid)).await?;
    a.world.expect_where(|q| q.is(pm, Some(14)), 3000, "refus d'invitation (op 14)").await?;
    anyhow::ensure!(b.world.expect_where(|q| q.is(pm, Some(0)), 800, "invitation").await.is_err(), "{cb} a reçu l'invitation bloquée");
    println!("  ✓ invitation de groupe : reçue sans « Block Invites », refusée (op 14) avec la case");

    step(&format!("{cb} décoche les cases : le chuchotement passe"));
    b.world.send(bd, buddy(buddy_op::OPTIONS, bcid, ca, 0)).await?;
    b.world.expect_where(is_buddy(buddy_op::OPTIONS), 3000, "cases (op 5)").await?;
    a.world.send(&schema::CHATTING, whisper(acid, cb, "et maintenant ?")).await?;
    b.world.expect_where(got_whisper("et maintenant"), 3000, "chuchotement reçu").await?;
    println!("  ✓ chuchotement reçu");

    step(&format!("{ca} revient : ami et ignoré du fichier local relus"));
    drop(a);
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    let list = entries(&mut a).await?;
    anyhow::ensure!(list.iter().any(|e| e == &(cb.to_string(), both)), "ami absent : {list:?}");
    anyhow::ensure!(list.iter().any(|(n, f)| n == "SimB" && f & buddy_flag::FRIEND == 0), "ignoré du fichier local absent : {list:?}");
    println!("  ✓ {list:?}");
    a.world.send(bd, buddy(buddy_op::REMOVE, a.cid, "SimB", 0)).await?;
    a.world.expect_where(is_buddy(buddy_op::REMOVE), 3000, "retrait de l'ignoré").await?;

    wipe().await?;
    println!("\namis et ignorés : OK");
    Ok(())
}
