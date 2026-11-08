//! `archcore sim gacha` : socles de tirage (« Fortune Gate », AgpmEventGacha 0x57, fonction d'objet 38).
//!
//! GmTest (compte MJ, inventaire vidé par `isole.sh`) passe niveau 25. Socle des armes d'Anchorville (obj02024.ini,
//! 132646683) : demande de loin (rien), objet inconnu (rien), ouverture à 400 unités (op 1 : Fortune Gate Pass
//! demandé, bande non vide), tirage sans le pass (code 1), avec trois pass (objet de la bande reçu, un pass pris),
//! inventaire rempli d'épées (code 4, rien de pris). Socle du charisme (obj02418.ini, 158467052) : 100 points
//! demandés (champ 7), tirage refusé (code 3) puis accepté après `/charisma`.

use super::{enter_world, step, InWorld, Recv};
use archcore_net::schema::{self, chat_type, gacha_op, gacha_result, mini};
use archcore_net::{build_mini, Fields};

/// Fortune Gate Pass : l'objet que demandent les tables des armes, armures et accessoires.
const PASS: i64 = 7436;
/// Socle des armes d'Anchorville (TID 9081) et socle du charisme (13173).
const WEAPON: (i64, f64, f64) = (132_646_683, -473_600.0, -60_200.0);
const CHARISMA: (i64, f64, f64) = (158_467_052, -60_200.0, -675_800.0);
const SWORD: i64 = 11;

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

fn base(object: i64) -> Vec<u8> {
    let eb = &mini::EVENT_BASE;
    build_mini(eb, &Fields::new(eb).with(0, 1).with(1, object).with(3, 38))
}

async fn send(w: &mut InWorld, op: i64, object: i64) -> anyhow::Result<()> {
    let g = &schema::EVENT_GACHA;
    w.world.send(g, Fields::new(g).with(0, op).with(1, base(object)).with(2, w.cid).with(3, 0)).await
}

/// TID de la bande (op 1, champ 8).
fn band(r: &Recv) -> Vec<i64> {
    r.fields().bytes(8).unwrap_or_default().chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]) as i64).collect()
}

pub async fn run(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let (g, it) = (&schema::EVENT_GACHA, &schema::ITEM);
    let system = |q: &Recv| q.ptype == schema::CHATTING.id && q.int(2) == Some(chat_type::WHOLE_WORLD);
    step(&format!("{char_name} entre en jeu, niveau 25"));
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    gm(&mut w, "/level 25", "").await?;

    step("refus : trop loin, objet inconnu");
    gm(&mut w, &format!("/move {},{}", WEAPON.1 - 3000.0, WEAPON.2), "porté en").await?;
    send(&mut w, gacha_op::REQUEST, WEAPON.0).await?;
    anyhow::ensure!(w.world.expect_where(|q| q.is(g, Some(gacha_op::OPEN)), 1000, "refus").await.is_err(), "fenêtre ouverte à 3000 unités");
    println!("  ✓ à 3000 unités : rien");
    gm(&mut w, &format!("/move {},{}", WEAPON.1, WEAPON.2 + 400.0), "porté en").await?;
    send(&mut w, gacha_op::REQUEST, 132_646_583).await?;
    anyhow::ensure!(w.world.expect_where(|q| q.is(g, Some(gacha_op::OPEN)), 1000, "refus").await.is_err(), "fenêtre ouverte sur une sphère d'alchimie");
    println!("  ✓ objet d'une autre fonction : rien");

    step("ouverture du socle des armes, tirage sans pass");
    send(&mut w, gacha_op::REQUEST, WEAPON.0).await?;
    let open = w.world.expect_where(|q| q.is(g, Some(gacha_op::OPEN)), 3000, "op 1").await?;
    let shown = band(&open);
    anyhow::ensure!(open.int(3) == Some(gacha_result::NOT_ENOUGH_ITEM), "code {:?} sans pass (1 attendu)", open.int(3));
    anyhow::ensure!(open.int(4) == Some(PASS) && open.int(5) == Some(1) && !shown.is_empty(), "coût {:?} × {:?}, bande {shown:?}", open.int(4), open.int(5));
    println!("  ✓ op 1 : code 1, 1 × Fortune Gate Pass, {} objet(s) dans la bande", shown.len());
    send(&mut w, gacha_op::ROLL, WEAPON.0).await?;
    let r = w.world.expect_where(|q| q.is(g, Some(gacha_op::RESULT)), 3000, "op 3").await?;
    anyhow::ensure!(r.int(4) == Some(gacha_result::NOT_ENOUGH_ITEM), "sans pass : code {:?}", r.int(4));
    println!("  ✓ op 3 : code 1 (sysstr 729)");

    step("tirage avec trois pass");
    w.world.send(&schema::CHATTING, say(&w, &format!("/additem {PASS} 3"))).await?;
    // Objet Cash : rangé dans l'inventaire Cash.
    let pile = w.world.expect_where(|q| q.is(it, Some(0)) && q.int(3) == Some(PASS), 3000, "pass").await?;
    let pile_iid = pile.int(2).unwrap_or(0);
    send(&mut w, gacha_op::REQUEST, WEAPON.0).await?;
    let open = w.world.expect_where(|q| q.is(g, Some(gacha_op::OPEN)), 3000, "op 1").await?;
    anyhow::ensure!(open.int(3) == Some(gacha_result::SUCCESS), "avec le pass : code {:?}", open.int(3));
    let shown = band(&open);
    send(&mut w, gacha_op::ROLL, WEAPON.0).await?;
    let mut won_iid = None;
    let r = loop {
        let q = w.world.expect_where(|q| q.is(g, Some(gacha_op::RESULT)) || q.is(it, Some(0)), 3000, "op 3").await?;
        if q.is(it, Some(0)) {
            won_iid = Some((q.int(2).unwrap_or(0), q.int(3).unwrap_or(0)));
            continue;
        }
        break q;
    };
    let tid = r.int(3).unwrap_or(0);
    anyhow::ensure!(r.int(4) == Some(gacha_result::SUCCESS) && shown.contains(&tid), "code {:?}, TID {tid} (bande {shown:?})", r.int(4));
    anyhow::ensure!(won_iid.is_some_and(|(_, t)| t == tid), "objet {tid} non reçu ({won_iid:?})");
    println!("  ✓ op 3 : TID {tid}, objet rangé (IID {:?})", won_iid.map(|w| w.0));

    step("inventaire plein : code 4, rien de pris");
    let mut swords = Vec::new();
    for _ in 0..126 {
        w.world.send(&schema::CHATTING, say(&w, &format!("/create \"{char_name}\" {SWORD} 1"))).await?;
        let r = w.world.expect_where(|q| (q.is(it, Some(0)) && q.int(3) == Some(SWORD)) || system(q), 3000, "épée").await?;
        if r.ptype == schema::CHATTING.id {
            break;
        }
        swords.push(r.int(2).unwrap_or(0));
        w.world.expect_where(system, 3000, "réponse MJ").await?;
    }
    send(&mut w, gacha_op::ROLL, WEAPON.0).await?;
    let r = w.world.expect_where(|q| q.is(g, Some(gacha_op::RESULT)), 3000, "op 3").await?;
    anyhow::ensure!(r.int(4) == Some(gacha_result::INVENTORY_FULL), "sac plein ({} épées) : code {:?}", swords.len(), r.int(4));
    println!("  ✓ {} épée(s) : code 4 (sysstr 731)", swords.len());
    // Pile de pass intacte : 2 restants (un pris au premier tirage).
    w.world.send(it, Fields::new(it).with(0, 21).with(2, pile_iid)).await?;
    w.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(pile_iid), 3000, "pass détruits").await?;
    let freed = swords.pop().unwrap_or(0);
    w.world.send(it, Fields::new(it).with(0, 21).with(2, freed)).await?;
    w.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(freed), 3000, "épée détruite").await?;

    step("socle du charisme : 100 points");
    gm(&mut w, &format!("/move {},{}", CHARISMA.1, CHARISMA.2 + 300.0), "porté en").await?;
    send(&mut w, gacha_op::REQUEST, CHARISMA.0).await?;
    let open = w.world.expect_where(|q| q.is(g, Some(gacha_op::OPEN)), 3000, "op 1").await?;
    anyhow::ensure!(open.int(7) == Some(100) && open.int(4).unwrap_or(0) == 0, "coût du charisme : {:?} point(s), objet {:?}", open.int(7), open.int(4));
    println!("  ✓ op 1 : code {:?}, 100 points de charisme, {} objet(s) dans la bande", open.int(3), band(&open).len());
    if open.int(3) == Some(gacha_result::NOT_ENOUGH_CHARISMA) {
        send(&mut w, gacha_op::ROLL, CHARISMA.0).await?;
        let r = w.world.expect_where(|q| q.is(g, Some(gacha_op::RESULT)), 3000, "op 3").await?;
        anyhow::ensure!(r.int(4) == Some(gacha_result::NOT_ENOUGH_CHARISMA), "sans charisme : code {:?}", r.int(4));
        println!("  ✓ op 3 : code 3 (sysstr 739)");
        gm(&mut w, "/charisma 100", "charisme").await?;
    }
    send(&mut w, gacha_op::ROLL, CHARISMA.0).await?;
    let r = w.world.expect_where(|q| q.is(g, Some(gacha_op::RESULT)), 3000, "op 3").await?;
    anyhow::ensure!(r.int(4) == Some(gacha_result::SUCCESS) && r.int(3).unwrap_or(0) > 0, "charisme : code {:?}, TID {:?}", r.int(4), r.int(3));
    println!("  ✓ op 3 : TID {:?} contre 100 points de charisme", r.int(3));
    println!("
gacha : OK");
    Ok(())
}
