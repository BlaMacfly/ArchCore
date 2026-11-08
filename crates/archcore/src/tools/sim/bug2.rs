//! `archcore sim bug2` : grille 5 × 5 × 5, saturation, destruction et réemploi d'une case.
//! À lancer sur un inventaire vide dans les bases de copie.
use super::*;

pub async fn run(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    cash(user, pass, char_name).await?;
    let cfg = crate::config::load()?;
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let (it, ch) = (&schema::ITEM, &schema::CHATTING);
    let system = |q: &Recv| q.ptype == ch.id && q.int(2) == Some(chat_type::WHOLE_WORLD);
    let mut cells = std::collections::BTreeSet::new();
    let mut last = 0;
    step("125 épées, une dans chaque case de la grille 5 × 5 × 5");
    for _ in 0..125 {
        let cmd = format!("/create \"{char_name}\" 11 1");
        w.world.send(ch, Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(&cmd))).await?;
        let p = w.world.expect_where(|q| (q.is(it, Some(0)) && q.int(3) == Some(11)) || system(q), 3000, "épée ajoutée").await?;
        anyhow::ensure!(p.is(it, Some(0)), "inventaire plein avant 125 cases : {}", cells.len());
        let g = decode_mini(&mini::ITEM_GRID, p.fields().bytes(7).ok_or_else(|| anyhow::anyhow!("grille absente"))?)?;
        let cell = [g.int(0).unwrap_or(-1), g.int(1).unwrap_or(-1), g.int(2).unwrap_or(-1)];
        anyhow::ensure!(cell.iter().all(|v| (0..5).contains(v)) && cells.insert(cell), "case invalide ou doublon : {cell:?}");
        last = p.int(2).unwrap_or(0);
        w.world.expect_where(system, 3000, "réponse MJ").await?;
    }
    anyhow::ensure!(cells.contains(&[4, 4, 4]), "dernière case absente");
    let cmd = format!("/create \"{char_name}\" 11 1");
    w.world.send(ch, Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(&cmd))).await?;
    let p = w.world.expect_where(|q| (q.is(it, Some(0)) && q.int(3) == Some(11)) || system(q), 3000, "refus du 126e objet").await?;
    anyhow::ensure!(system(&p), "126e objet accepté");
    step("destruction puis réemploi de la dernière case");
    w.world.send(it, Fields::new(it).with(0, 21).with(2, last)).await?;
    w.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(last), 3000, "objet détruit").await?;
    w.world.send(ch, Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(&cmd))).await?;
    let p = w.world.expect_where(|q| q.is(it, Some(0)) && q.int(3) == Some(11), 3000, "case réemployée").await?;
    let g = decode_mini(&mini::ITEM_GRID, p.fields().bytes(7).unwrap_or_default())?;
    anyhow::ensure!((0..3).all(|i| g.int(i) == Some(4)), "dernière case non réemployée");
    println!("\ninventaire bug2 5x5x5 et destruction : OK");
    Ok(())
}

/// Rune Cash 4571 sur collier 5018 : réponse, consommation et persistance après reconnexion.
pub async fn cash(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let (it, ch, ic, ev) = (&schema::ITEM, &schema::CHATTING, &schema::ITEM_CONVERT, &schema::EVENT_ITEM_CONVERT);
    let say = |cid: u32, text: &str| Fields::new(ch).with(0, 0).with(1, cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text));
    let system = |q: &Recv| q.ptype == ch.id && q.int(2) == Some(chat_type::WHOLE_WORLD);
    step("rune Cash 4571 sur collier 5018 chez Halo");
    w.world.send(ch, say(w.cid, "/move -479400,-69300")).await?;
    w.world.expect_where(system, 3000, "déplacement chez Halo").await?;
    let mut smith = w.npcs.iter().find(|n| n.1.ends_with(" Halo")).map(|n| n.0);
    while let Ok(p) = w.world.expect_where(|q| q.is(&schema::OPT_VIEW, Some(1)), 1000, "PNJ").await {
        if let Ok(v) = decode_mini(&mini::OPT_VIEW_CHAR, p.fields().bytes(1).unwrap_or_default()) {
            if v.text(2).is_some_and(|n| n.ends_with(" Halo")) { smith = v.int(0); }
        }
    }
    let smith = smith.ok_or_else(|| anyhow::anyhow!("Halo introuvable"))?;
    let eb = &mini::EVENT_BASE;
    let event = build_mini(eb, &Fields::new(eb).with(0, 3).with(1, smith).with(3, 22));
    w.world.send(ev, Fields::new(ev).with(0, 0).with(1, event).with(2, w.cid)).await?;
    w.world.expect_where(|q| q.is(ev, Some(1)), 3000, "conversion ouverte").await?;
    let mut made = Vec::new();
    for (tid, status) in [(5018, 2), (4571, 51)] {
        w.world.send(ch, say(w.cid, &format!("/additem {tid} 1"))).await?;
        let p = w.world.expect_where(|q| q.is(it, Some(0)) && q.int(3) == Some(tid), 3000, "objet de conversion").await?;
        anyhow::ensure!(p.int(1) == Some(status), "TID {tid} : statut {:?}, attendu {status}", p.int(1));
        made.push(p.int(2).unwrap_or(0));
        w.world.expect_where(system, 3000, "réponse MJ").await?;
    }
    let (collar, rune) = (made[0], made[1]);
    w.world.send(ic, Fields::new(ic).with(0, 3).with(1, w.cid).with(2, collar).with(3, rune)).await?;
    let r = w.world.expect_where(|q| q.is(ic, Some(8)) && q.int(2) == Some(collar), 3000, "résultat rune Cash").await?;
    anyhow::ensure!(r.int(4) == Some(1) && r.int(7) == Some(1) && r.int(8) == Some(4571), "rune Cash : résultat {:?}, remplis {:?}, TID {:?}", r.int(4), r.int(7), r.int(8));
    w.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(rune), 3000, "rune Cash consommée").await?;
    let sockets = r.int(6).unwrap_or(0);
    drop(w);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    anyhow::ensure!(w.converts.iter().any(|c| *c == (collar, 0, sockets, 1)), "conversion non persistée");
    anyhow::ensure!(!w.items.iter().any(|i| i.0 == rune), "rune Cash encore présente");
    w.world.send(it, Fields::new(it).with(0, 21).with(2, collar)).await?;
    w.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(collar), 3000, "collier détruit").await?;
    println!("\nrune Cash bug2, consommation et persistance : OK");
    Ok(())
}
