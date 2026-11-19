//! `archcore sim cashmall` : boutique Cash (AgpmCashMall 0x2E) et recrutement (AgcmRecruit 0x0A).
//!
//! GmTest (compte MJ) : ouverture comme les clients (op 7 puis op 1 → op 2 onglets, op 3 produits de l'onglet 0),
//! produit le moins cher de l'onglet ; achat avec une version périmée (code 4), sans assez de Cash (code 2), puis
//! avec `/cash` prix + 10 (objet reçu, Cash à 10 au champ 10 d'AGPMCHARACTER, code 1) ; op 6 : solde renvoyé.
//! Recrutement : ops 4 et 6 d'AgcmRecruit acceptées sans réponse, la session reste ouverte.

use super::{enter_world, step, InWorld, Recv};
use archcore_net::schema::{self, cash_mall_op as op, cash_mall_result as result, chat_type, mini, recruit_op};
use archcore_net::{build_mini, Fields};

fn say(w: &InWorld, text: &str) -> Fields {
    let ch = &schema::CHATTING;
    Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text))
}

/// Commande MJ et sa réponse (message système).
async fn gm(w: &mut InWorld, text: &str, expect: &str) -> anyhow::Result<()> {
    w.world.send(&schema::CHATTING, say(w, text)).await?;
    let ch = &schema::CHATTING;
    w.world.expect_where(|q| q.ptype == ch.id && q.fields().text(6).is_some_and(|t| t.contains(expect)), 5000, text).await?;
    Ok(())
}

async fn send(w: &mut InWorld, f: Fields) -> anyhow::Result<()> {
    w.world.send(&schema::CASH_MALL, f.with(3, w.cid)).await
}

fn mall(o: i64) -> Fields {
    Fields::new(&schema::CASH_MALL).with(0, o)
}

/// Produits d'un texte d'op 3 : (produit, TID, nombre, prix).
fn products(text: &[u8]) -> Vec<(i64, i64, i64, i64)> {
    String::from_utf8_lossy(text)
        .split(':')
        .filter_map(|e| {
            let n: Vec<i64> = e.split(',').take(4).filter_map(|v| v.trim().parse().ok()).collect();
            (n.len() == 4).then(|| (n[0], n[1], n[2], n[3]))
        })
        .collect()
}

/// Achat du produit `id` avec la version `version` : code de l'op 5.
async fn buy(w: &mut InWorld, id: i64, version: i64) -> anyhow::Result<i64> {
    send(w, mall(op::BUY).with(2, version).with(5, id).with(8, 0)).await?;
    let c = &schema::CASH_MALL;
    let r = w.world.expect_where(|q| q.is(c, Some(op::BUY_RESULT)), 3000, "op 5").await?;
    Ok(r.int(1).unwrap_or(-1))
}

pub async fn run(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let (c, it, ch) = (&schema::CASH_MALL, &schema::ITEM, &schema::CHARACTER);
    step(&format!("{char_name} entre en jeu, ouvre la boutique"));
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    send(&mut w, mall(op::CHECK_VERSION).with(2, 0)).await?;
    send(&mut w, mall(op::REQUEST_PRODUCTS).with(4, 0)).await?;
    let tabs = w.world.expect_where(|q| q.is(c, Some(op::MALL_LIST)), 3000, "op 2").await?;
    let text = String::from_utf8_lossy(tabs.fields().bytes(6).unwrap_or_default()).to_string();
    let version = tabs.int(2).unwrap_or(0);
    anyhow::ensure!(text.contains(';') && text.ends_with(",:") && version != 0, "onglets : {text:?}, version {version}");
    println!("  ✓ op 2 : {} onglet(s), version {version} ({})", text.matches(':').count(), text.split(';').next().unwrap_or(""));
    let list = w.world.expect_where(|q| q.is(c, Some(op::PRODUCT_LIST)), 3000, "op 3").await?;
    let shown = products(list.fields().bytes(7).unwrap_or_default());
    anyhow::ensure!(list.int(4) == Some(0) && !shown.is_empty(), "onglet {:?}, {} produit(s)", list.int(4), shown.len());
    let &(id, tid, count, price) = shown.iter().min_by_key(|p| p.3).unwrap();
    println!("  ✓ op 3 : {} produit(s) ; le moins cher : produit {id}, {count} × TID {tid}, {price} Cash", shown.len());

    step("achats refusés : version périmée, Cash insuffisant");
    gm(&mut w, &format!("/cash {}", price - 1), "Cash").await?;
    let code = buy(&mut w, id, version + 1).await?;
    anyhow::ensure!(code == result::NEED_NEW_ITEM_LIST, "version périmée : code {code}");
    println!("  ✓ version périmée : code 4");
    let code = buy(&mut w, id, version).await?;
    anyhow::ensure!(code == result::NOT_ENOUGH_CASH, "{} Cash pour {price} : code {code}", price - 1);
    println!("  ✓ {} Cash pour {price} : code 2", price - 1);

    step("achat réussi");
    gm(&mut w, &format!("/cash {}", price + 10), "Cash").await?;
    send(&mut w, mall(op::BUY).with(2, version).with(5, id).with(8, 0)).await?;
    let (mut got, mut cash) = (0, None);
    let r = loop {
        let q = w.world.expect_where(|q| q.is(c, Some(op::BUY_RESULT)) || (q.is(it, None) && q.int(3) == Some(tid)) || (q.is(ch, Some(1)) && q.int(10).is_some()), 3000, "achat").await?;
        if q.is(it, None) {
            got += 1;
            continue;
        }
        if q.is(ch, Some(1)) {
            cash = q.int(10);
            continue;
        }
        break q;
    };
    anyhow::ensure!(r.int(1) == Some(result::SUCCESS), "achat : code {:?}", r.int(1));
    anyhow::ensure!(got > 0 && cash == Some(10), "objets reçus {got}, Cash {cash:?} (10 attendu)");
    println!("  ✓ code 1 : {got} paquet(s) d'objet TID {tid}, Cash restant 10");
    send(&mut w, mall(op::REFRESH_CASH)).await?;
    let q = w.world.expect_where(|q: &Recv| q.is(ch, Some(1)) && q.int(10).is_some(), 3000, "solde").await?;
    anyhow::ensure!(q.int(10) == Some(10), "op 6 : Cash {:?}", q.int(10));
    println!("  ✓ op 6 : Cash 10 renvoyé");

    step("recrutement : ops 4 et 6 acceptées sans réponse");
    let (rc, rp, rs) = (&schema::RECRUIT, &mini::RECRUIT_PARTY, &mini::RECRUIT_SEARCH);
    let ad = build_mini(rp, &Fields::new(rp).with(0, 0).with(1, 0).with(2, "Chasse").with(3, 3).with(4, 20).with(5, 30).with(6, 0).with(7, 15).with(8, char_name).with(9, w.cid));
    w.world.send(rc, Fields::new(rc).with(0, recruit_op::REGISTER_PARTY).with(1, 0).with(2, w.cid).with(4, ad).with(6, 0)).await?;
    let search = build_mini(rs, &Fields::new(rs).with(0, 0).with(1, 0).with(2, 1).with(3, 15));
    w.world.send(rc, Fields::new(rc).with(0, recruit_op::SEARCH_PARTY).with(1, 0).with(2, w.cid).with(5, search)).await?;
    anyhow::ensure!(w.world.expect_where(|q| q.is(rc, None), 1000, "réponse").await.is_err(), "réponse d'AgcmRecruit inattendue");
    send(&mut w, mall(op::CHECK_VERSION).with(2, version)).await?;
    w.world.expect_where(|q| q.is(c, Some(op::MALL_LIST)), 3000, "op 2 après le recrutement").await?;
    println!("  ✓ aucune réponse, session ouverte (op 2 reçue ensuite)");
    println!("\nboutique Cash et recrutement : OK");
    Ok(())
}
