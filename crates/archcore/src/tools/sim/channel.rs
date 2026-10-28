//! `archcore sim channel` : salons de discussion (AgpmChannel 0x2C).
//!
//! GmTest crée le salon public « SalonSim » (op 3, liste « GmTest; ») ; un second « SalonSim » est refusé (code 1).
//! SimA y entre (op 3 avec les deux noms ; op 5 chez GmTest), ne peut y entrer deux fois (code 11), y parle (op 9 chez
//! les deux). Salon privé « Secret » (mot de passe « abc ») : SimA reçoit l'op 8, se trompe (code 8), puis entre ;
//! il en sort (op 4, op 6 chez GmTest) ; invité par GmTest (op 7), il y revient sans mot de passe. Recherche d'un
//! joueur (op 10, puis code 3 pour un inconnu), salon inconnu (code 2), départ de SimA du monde (op 6 chez GmTest).

use super::{enter_world, step, InWorld, Recv};
use archcore_net::schema::{self, channel_kind, channel_op, channel_result};
use archcore_net::Fields;

fn op(w: &InWorld, op: i64) -> Fields {
    Fields::new(&schema::CHANNEL).with(0, op).with(1, w.cid)
}

fn is(op: i64) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| q.is(&schema::CHANNEL, Some(op))
}

async fn expect_code(w: &mut InWorld, code: i64, what: &str) -> anyhow::Result<()> {
    let r = w.world.expect_where(is(channel_op::RESULT), 3000, what).await?;
    anyhow::ensure!(r.int(6) == Some(code), "{what} : code {:?}", r.int(6));
    println!("  ✓ {what} : code {code}");
    Ok(())
}

/// Op 3 reçue : nom du salon et liste des membres.
async fn expect_entered(w: &mut InWorld, what: &str) -> anyhow::Result<(String, String)> {
    let r = w.world.expect_where(is(channel_op::JOIN), 3000, what).await?;
    let list = r.fields().bytes(8).map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
    Ok((r.fields().text(2).unwrap_or_default(), list))
}

pub async fn run(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let c = &schema::CHANNEL;
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    let mut b = enter_world(&cfg, ub, pb, cb, "B-").await?;

    step(&format!("{ca} crée le salon public « SalonSim »"));
    a.world.send(c, op(&a, channel_op::CREATE).with(2, "SalonSim").with(5, channel_kind::PUBLIC)).await?;
    let (name, list) = expect_entered(&mut a, "salon créé").await?;
    anyhow::ensure!(name == "SalonSim" && list == format!("{ca};\0"), "op 3 : {name:?}, liste {list:?}");
    println!("  ✓ op 3 : liste {:?}", list.trim_end_matches('\0'));
    a.world.send(c, op(&a, channel_op::CREATE).with(2, "salonsim").with(5, channel_kind::PUBLIC)).await?;
    expect_code(&mut a, channel_result::ALREADY_EXIST, "nom déjà pris").await?;

    step(&format!("{cb} entre dans « SalonSim » et y parle"));
    b.world.send(c, op(&b, channel_op::JOIN).with(2, "SalonSim")).await?;
    let (_, list) = expect_entered(&mut b, "entrée de B").await?;
    anyhow::ensure!(list == format!("{ca};{cb};\0"), "liste {list:?}");
    let added = a.world.expect_where(is(channel_op::ADD_MEMBER), 3000, "op 5 chez A").await?;
    anyhow::ensure!(added.fields().text(4).as_deref() == Some(cb), "op 5 : {:?}", added.fields().text(4));
    println!("  ✓ op 3 chez {cb} (liste {:?}), op 5 chez {ca}", list.trim_end_matches('\0'));
    b.world.send(c, op(&b, channel_op::JOIN).with(2, "SalonSim")).await?;
    expect_code(&mut b, channel_result::ALREADY_JOINED, "déjà membre").await?;
    b.world.send(c, op(&b, channel_op::MESSAGE).with(2, "SalonSim").with(4, cb).with(7, "Bonjour le salon")).await?;
    for (w, who) in [(&mut a, ca), (&mut b, cb)] {
        let m = w.world.expect_where(is(channel_op::MESSAGE), 3000, "message").await?;
        anyhow::ensure!(m.fields().text(4).as_deref() == Some(cb) && m.fields().text(7).as_deref() == Some("Bonjour le salon"), "message chez {who}");
    }
    println!("  ✓ op 9 reçue par les deux (auteur {cb})");

    step(&format!("salon privé « Secret » : mot de passe demandé, refusé puis accepté"));
    a.world.send(c, op(&a, channel_op::CREATE).with(2, "Secret").with(3, "abc").with(5, channel_kind::PRIVATE)).await?;
    expect_entered(&mut a, "salon privé créé").await?;
    b.world.send(c, op(&b, channel_op::JOIN).with(2, "Secret")).await?;
    b.world.expect_where(is(channel_op::PASSWORD), 3000, "mot de passe demandé (op 8)").await?;
    println!("  ✓ op 8 : mot de passe demandé");
    b.world.send(c, op(&b, channel_op::PASSWORD).with(2, "Secret").with(3, "zzz")).await?;
    expect_code(&mut b, channel_result::INVALID_PASSWORD, "mauvais mot de passe").await?;
    b.world.send(c, op(&b, channel_op::PASSWORD).with(2, "Secret").with(3, "abc")).await?;
    expect_entered(&mut b, "entrée avec le mot de passe").await?;
    println!("  ✓ entrée avec le bon mot de passe");

    step(&format!("{cb} sort de « Secret », puis y revient sur invitation"));
    b.world.send(c, op(&b, channel_op::LEAVE).with(2, "Secret")).await?;
    b.world.expect_where(is(channel_op::LEAVE), 3000, "sortie (op 4)").await?;
    let gone = a.world.expect_where(|q| is(channel_op::REMOVE_MEMBER)(q) && q.fields().text(2).as_deref() == Some("Secret"), 3000, "op 6 chez A").await?;
    anyhow::ensure!(gone.fields().text(4).as_deref() == Some(cb), "op 6 : {:?}", gone.fields().text(4));
    println!("  ✓ op 4 chez {cb}, op 6 chez {ca}");
    a.world.send(c, op(&a, channel_op::INVITE).with(2, "Secret").with(4, cb).with(5, channel_kind::PRIVATE)).await?;
    let inv = b.world.expect_where(is(channel_op::INVITE), 3000, "invitation (op 7)").await?;
    anyhow::ensure!(inv.fields().text(4).as_deref() == Some(ca) && inv.int(5) == Some(channel_kind::PRIVATE), "op 7 : {:?}, genre {:?}", inv.fields().text(4), inv.int(5));
    b.world.send(c, op(&b, channel_op::JOIN).with(2, "Secret")).await?;
    let (name, _) = expect_entered(&mut b, "entrée sur invitation, sans mot de passe").await?;
    anyhow::ensure!(name == "Secret", "salon {name:?}");
    println!("  ✓ invité, {cb} entre sans mot de passe");

    step("recherche d'un joueur, salon inconnu");
    a.world.send(c, op(&a, channel_op::FIND_USER).with(4, cb)).await?;
    let found = a.world.expect_where(is(channel_op::FIND_USER), 3000, "joueur trouvé (op 10)").await?;
    anyhow::ensure!(found.fields().text(4).as_deref() == Some(cb), "op 10 : {:?}", found.fields().text(4));
    println!("  ✓ op 10 : {cb} trouvé");
    a.world.send(c, op(&a, channel_op::FIND_USER).with(4, "Personne")).await?;
    expect_code(&mut a, channel_result::NO_CHARACTER, "joueur inconnu").await?;
    a.world.send(c, op(&a, channel_op::JOIN).with(2, "Rien")).await?;
    expect_code(&mut a, channel_result::NOT_EXIST, "salon inconnu").await?;

    step("chacun sort de ses salons ; un salon vide disparaît");
    for name in ["SalonSim", "Secret"] {
        b.world.send(c, op(&b, channel_op::LEAVE).with(2, name)).await?;
        b.world.expect_where(is(channel_op::LEAVE), 3000, "sortie de B").await?;
        a.world.expect_where(|q| is(channel_op::REMOVE_MEMBER)(q) && q.fields().text(4).as_deref() == Some(cb), 3000, "op 6 chez A").await?;
        a.world.send(c, op(&a, channel_op::LEAVE).with(2, name)).await?;
        a.world.expect_where(is(channel_op::LEAVE), 3000, "sortie de A").await?;
    }
    b.world.send(c, op(&b, channel_op::JOIN).with(2, "SalonSim")).await?;
    expect_code(&mut b, channel_result::NOT_EXIST, "salon vide disparu").await?;
    println!("\nsalons de discussion : OK");
    Ok(())
}
