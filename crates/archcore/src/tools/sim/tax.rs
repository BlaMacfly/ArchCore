//! `archcore sim tax` : taxes des régions du château d'Inflame (AgpmTax 0x53 ; `world/tax.rs`).
//!
//! GmTest (MJ) crée la guilde SimTaxA ; `/siege owner Inflame SimTaxA` le fait maître du château. Chez l'intendant
//! Alfredo (fonction 34) : op 0 → op 1 ; op 3 → op 3 (château, deux régions à 0 %) ; op 2 avec 60 % → code 3, avec
//! 0 % et 10 % (région des abords) → code 0, op 4 de la région reçu par GmTest et par SimA ; nouveau changement la même
//! semaine → code 2. Chez le marchand de siège Damian (abords d'Inflame), un Catapult bomb (10 000) coûte 11 000 ;
//! `/tax` montre la recette ; `/tax pay Inflame` la verse à la banque de GmTest (champ 9) et l'op 3 montre la recette
//! totale, la date du versement et la recette de la région remise à 0. Fin : château rendu (taux remis à 0, op 4).

use super::{enter_world, step, InWorld, Recv};
use archcore_net::schema::{self, chat_type, mini, tax_op, tax_result};
use archcore_net::{build_mini, decode_mini, Fields};
use std::time::Duration;

const CASTLE: &str = "Inflame";
const GUILD: &str = "SimTaxA";
const PASSWORD: &str = "simpass";
/// Intendant Alfredo (PNJ 379), au château d'Inflame.
const ALFREDO: u32 = 0x4000_0000 + 379;
const ALFREDO_AT: (f32, f32) = (-353_558.0, -403.0);
/// Marchand de siège Damian (PNJ 376), dans les abords d'Inflame : Catapult bomb à 10 000.
const DAMIAN: u32 = 0x4000_0000 + 376;
const DAMIAN_AT: (f32, f32) = (-352_786.0, -52_159.0);
const BOMB: i64 = 4553;
const MONEY: i64 = 1_000_000;

fn say(w: &InWorld, text: &str) -> Fields {
    let ch = &schema::CHATTING;
    Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text))
}

async fn gm(w: &mut InWorld, text: &str) -> anyhow::Result<String> {
    w.world.send(&schema::CHATTING, say(w, text)).await?;
    let r = w.world.expect_where(|q| q.ptype == schema::CHATTING.id, 5000, text).await?;
    let reply = r.fields().text(6).unwrap_or_default();
    println!("  {text} → {reply}");
    Ok(reply)
}

fn tax(op: i64) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| q.is(&schema::TAX, Some(op))
}

fn castle_mini(name: &str) -> Vec<u8> {
    let m = &mini::TAX_CASTLE;
    build_mini(m, &Fields::new(m).with(0, name))
}

fn region_mini(name: &str, ratio: i64) -> Vec<u8> {
    let m = &mini::TAX_REGION;
    build_mini(m, &Fields::new(m).with(0, name).with(2, ratio))
}

/// Régions d'une réponse (champs 4 à 7) : (nom, recette, taux).
fn regions(q: &Recv) -> Vec<(String, i64, i64)> {
    (4..8)
        .filter_map(|i| q.fields().bytes(i).and_then(|b| decode_mini(&mini::TAX_REGION, b).ok()))
        .map(|r| (r.text(0).unwrap_or_default(), r.int(1).unwrap_or(0), r.int(2).unwrap_or(0)))
        .collect()
}

fn castle(q: &Recv) -> Option<Fields> {
    decode_mini(&mini::TAX_CASTLE, q.fields().bytes(3)?).ok()
}

async fn place(w: &mut InWorld, at: (f32, f32)) -> anyhow::Result<()> {
    w.world.send(&schema::CHATTING, say(w, &format!("/move {},{}", at.0, at.1))).await?;
    tokio::time::sleep(Duration::from_millis(800)).await;
    Ok(())
}

/// Op 2 avec ces taux (dans l'ordre des régions reçues) ; rend la réponse.
async fn modify(a: &mut InWorld, names: &[String], ratios: &[i64]) -> anyhow::Result<Recv> {
    let s = &schema::TAX;
    let mut f = Fields::new(s).with(0, tax_op::MODIFY).with(1, a.cid).with(3, castle_mini(CASTLE));
    for (i, (n, r)) in names.iter().zip(ratios).enumerate() {
        f.set(4 + i, region_mini(n, *r));
    }
    a.world.send(s, f).await?;
    a.world.expect_where(tax(tax_op::MODIFY), 3000, "op 2").await
}

pub async fn run(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let (s, gd, c) = (&schema::TAX, &schema::GUILD, &schema::CHARACTER);

    step(&format!("{ca} (MJ) et {cb} entrent ; taux des régions reçus (op 4)"));
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    let mut b = enter_world(&cfg, ub, pb, cb, "B-").await?;
    let (level_a, bank_a) = (a.level, a.bank_money);
    let first = a.world.expect_where(tax(tax_op::REGION_RATIO), 3000, "op 4 à l'entrée").await?;
    anyhow::ensure!(first.int(1) == Some(a.cid as i64), "op 4 pour le CID {:?}", first.int(1));
    println!("  ✓ op 4 à l'entrée (CID du destinataire)");

    step(&format!("guilde {GUILD} ; {ca} maître d'{CASTLE}"));
    gm(&mut a, "/level 60").await?;
    gm(&mut a, &format!("/money {MONEY}")).await?;
    a.world.send(gd, Fields::new(gd).with(0, 0).with(1, a.cid).with(2, GUILD).with(9, PASSWORD)).await?;
    a.world.expect_where(|q| q.is(gd, Some(10)) && q.int(1) == Some(11), 3000, "création de la guilde").await?;
    let r = gm(&mut a, &format!("/siege owner {CASTLE} {GUILD}")).await?;
    anyhow::ensure!(r.contains(GUILD), "/siege owner : « {r} »");

    step("intendant Alfredo : op 0 → op 1, op 3 → château et régions");
    place(&mut a, ALFREDO_AT).await?;
    let ev = crate::world::npcs::event_base(ALFREDO, crate::world::npcs::event_function::TAX);
    a.world.send(s, Fields::new(s).with(0, tax_op::REQUEST).with(1, a.cid).with(3, ev)).await?;
    let grant = a.world.expect_where(tax(tax_op::GRANT), 3000, "op 1").await?;
    let base = decode_mini(&mini::EVENT_BASE, grant.fields().bytes(3).unwrap_or_default())?;
    anyhow::ensure!(base.int(1) == Some(ALFREDO as i64) && base.int(3) == Some(34), "EVENT_BASE {:?} {:?}", base.int(1), base.int(3));
    a.world.send(s, Fields::new(s).with(0, tax_op::VIEW).with(1, a.cid).with(3, castle_mini(CASTLE))).await?;
    let view = a.world.expect_where(tax(tax_op::VIEW), 3000, "op 3").await?;
    let list = regions(&view);
    anyhow::ensure!(view.int(2) == Some(0) && castle(&view).and_then(|c| c.text(0)).as_deref() == Some(CASTLE), "op 3 : résultat {:?}", view.int(2));
    anyhow::ensure!(list.len() == 2 && list.iter().all(|r| r.2 == 0), "régions {list:?}");
    let names: Vec<String> = list.iter().map(|r| r.0.clone()).collect();
    println!("  ✓ op 1 (Alfredo, fonction 34) ; op 3 : {CASTLE}, régions {names:?} à 0 %");

    step("op 2 : 60 % refusé (3), 0 % et 10 % acceptés (0, op 4 à tous), nouveau changement refusé (2)");
    let r = modify(&mut a, &names, &[0, 60]).await?;
    anyhow::ensure!(r.int(2) == Some(tax_result::OUT_OF_RANGE), "60 % : code {:?}", r.int(2));
    let r = modify(&mut a, &names, &[0, 10]).await?;
    anyhow::ensure!(r.int(2) == Some(tax_result::SUCCESS), "10 % : code {:?}", r.int(2));
    anyhow::ensure!(regions(&r).get(1).map(|x| x.2) == Some(10), "régions renvoyées {:?}", regions(&r));
    anyhow::ensure!(castle(&r).and_then(|c| c.int(3)).is_some_and(|t| t > 0), "date de modification absente");
    let outskirts = names[1].clone();
    let is_ten = |q: &Recv| tax(tax_op::REGION_RATIO)(q) && regions_one(q).is_some_and(|(n, _, t)| n == outskirts && t == 10);
    b.world.expect_where(is_ten, 3000, "op 4 à SimA").await?;
    a.world.expect_where(|q| tax(tax_op::REGION_RATIO)(q) && regions_one(q).is_some_and(|(_, _, t)| t == 10), 3000, "op 4 à GmTest").await?;
    let r = modify(&mut a, &names, &[0, 5]).await?;
    anyhow::ensure!(r.int(2) == Some(tax_result::NOT_TIME), "même semaine : code {:?}", r.int(2));
    println!("  ✓ codes 3, 0 puis 2 ; op 4 « {outskirts} » 10 % reçu par {ca} et {cb}");

    step("achat chez Damian : prix + 10 %");
    place(&mut a, DAMIAN_AT).await?;
    // Argent remis au compte rond (la création de la guilde en a pris).
    gm(&mut a, &format!("/money {MONEY}")).await?;
    let eb = crate::world::npcs::event_base(DAMIAN, crate::world::npcs::event_function::NPC_TRADE);
    let tr = &schema::EVENT_NPC_TRADE;
    a.world.send(tr, Fields::new(tr).with(0, eb.clone()).with(1, 0).with(2, a.cid)).await?;
    let mut bomb: Option<(i64, i64)> = None;
    loop {
        let p = a.world.expect_where(|p| p.ptype == tr.id && p.packet.is_some(), 5000, "boutique de Damian").await?;
        match p.int(1) {
            Some(0) => {
                let d = decode_mini(&mini::NPC_TRADE_DETAIL, p.fields().bytes(4).unwrap_or_default())?;
                if d.int(0) == Some(BOMB) {
                    bomb = Some((p.int(3).unwrap_or(0), d.int(1).unwrap_or(0)));
                }
            }
            Some(1) => break,
            _ => {}
        }
    }
    let (iid, price) = bomb.ok_or_else(|| anyhow::anyhow!("Catapult bomb absente de la boutique"))?;
    let d = &mini::NPC_TRADE_DETAIL;
    a.world.send(tr, Fields::new(tr).with(0, eb).with(1, 2).with(2, a.cid).with(3, iid).with(4, build_mini(d, &Fields::new(d).with(2, 1)))).await?;
    let paid = a.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(a.cid as i64) && q.int(8).is_some(), 5000, "argent après l'achat").await?;
    let left = paid.int(8).unwrap_or(0);
    let expected = price + price * 10 / 100;
    anyhow::ensure!(MONEY - left == expected, "payé {} au lieu de {expected} (prix {price})", MONEY - left);
    println!("  ✓ Catapult bomb à {price} payée {expected} (taxe {})", expected - price);

    step("recette, versement forcé, registre");
    // Une ligne par château : celle d'Inflame porte la recette de la région des abords.
    let income = format!("10 % ({})", expected - price);
    a.world.send(&schema::CHATTING, say(&a, "/tax")).await?;
    let line = a.world.expect_where(|q| q.ptype == schema::CHATTING.id && q.fields().text(6).is_some_and(|t| t.contains(&income)), 3000, "/tax : recette de la région").await?;
    println!("  /tax → {}", line.fields().text(6).unwrap_or_default());
    a.world.send(&schema::CHATTING, say(&a, &format!("/tax pay {CASTLE}"))).await?;
    let bank = a.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(a.cid as i64) && q.int(9).is_some(), 5000, "banque après le versement").await?;
    anyhow::ensure!(bank.int(9) == Some(bank_a + expected - price), "banque {:?} au lieu de {}", bank.int(9), bank_a + expected - price);
    a.world.expect_where(|q| q.ptype == schema::CHATTING.id && q.fields().text(6).is_some_and(|t| t.contains("versé")), 3000, "compte rendu du versement").await?;
    place(&mut a, ALFREDO_AT).await?;
    a.world.send(s, Fields::new(s).with(0, tax_op::VIEW).with(1, a.cid).with(3, castle_mini(CASTLE))).await?;
    let view = a.world.expect_where(tax(tax_op::VIEW), 3000, "op 3 après le versement").await?;
    let ct = castle(&view).ok_or_else(|| anyhow::anyhow!("château absent"))?;
    anyhow::ensure!(ct.int(1) == Some(expected - price) && ct.int(2).is_some_and(|t| t > 0), "recette totale {:?}, versement {:?}", ct.int(1), ct.int(2));
    anyhow::ensure!(regions(&view).iter().all(|r| r.1 == 0), "recettes des régions {:?}", regions(&view));
    println!("  ✓ banque +{} ; recette totale {}, versement daté, régions remises à 0", expected - price, expected - price);

    step("remise en état");
    gm(&mut a, &format!("/siege owner {CASTLE} -")).await?;
    b.world.expect_where(|q| tax(tax_op::REGION_RATIO)(q) && regions_one(q).is_some_and(|(n, _, t)| n == outskirts && t == 0), 3000, "taux remis à 0").await?;
    gm(&mut a, &format!("/money {}", left.max(0))).await?;
    gm(&mut a, &format!("/level {level_a}")).await?;
    println!("  ✓ château rendu : taux à 0 pour tous");
    println!("\ntaxes : OK");
    Ok(())
}

/// Région d'un op 4 (champ 4).
fn regions_one(q: &Recv) -> Option<(String, i64, i64)> {
    let r = decode_mini(&mini::TAX_REGION, q.fields().bytes(4)?).ok()?;
    Some((r.text(0).unwrap_or_default(), r.int(1).unwrap_or(0), r.int(2).unwrap_or(0)))
}
