//! `archcore sim guildlead` : direction de la guilde (liste des guildes, agrandissement, second, passation).

use super::{enter_world, o, step, InWorld, Recv};
use crate::config::Config;
use archcore_net::schema::{self, mini};
use archcore_net::{build_mini, decode_mini, Fields};
use std::time::Duration;

/// Comptes d'appoint `simg01`… (mot de passe identique au nom), créés au besoin dans la base de test.
async fn helper_accounts(cfg: &Config, n: usize) -> anyhow::Result<Vec<String>> {
    let db = crate::db::Databases::new(&cfg.database);
    let mut out = Vec::new();
    for i in 1..=n {
        let user = format!("simg{i:02}");
        let hash = crate::login::sha256_hex(&crate::tools::to_cp1252(&user));
        db.auth
            .exec("INSERT INTO account (username, password_hash, gm_level) VALUES (?, ?, 0) ON DUPLICATE KEY UPDATE password_hash = VALUES(password_hash)", (user.as_str(), hash.as_str()))
            .await?;
        out.push(user);
    }
    Ok(out)
}

//// Direction de la guilde : A (compte MJ) crée SimLead ; B et 18 comptes d'appoint la rejoignent (20 membres).
/// Liste des guildes (page, recherche, nom inconnu : code 47) ; agrandissement refusé faute d'or (code 44)
/// puis fait (op 7, 60 places) ; nomination de B refusée sous le niveau 20, puis acceptée (op 48 : 2 → 3 → 4
/// → 8) ; retrait du rang (rang 1) et nouvelle nomination ; passation (5 → 6 → 7 → 8 : A rang 1, B chef) ;
/// rangs relus à la reconnexion ; B, nouveau chef, dissout la guilde.
pub async fn guild_lead(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let gd = &schema::GUILD;
    step(&format!("{ca}, {cb} et {LEAD_HELPERS} comptes d'appoint entrent en jeu"));
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    let mut b = enter_world(&cfg, ub, pb, cb, "B-").await?;
    Box::pin(leave_old_guild(&mut a)).await?;
    Box::pin(leave_old_guild(&mut b)).await?;
    let mut helpers = Vec::new();
    for (i, user) in helper_accounts(&cfg, LEAD_HELPERS).await?.iter().enumerate() {
        let mut h = enter_world(&cfg, user, user, &format!("SimG{:02}", i + 1), &format!("G{:02}-", i + 1)).await?;
        Box::pin(leave_old_guild(&mut h)).await?;
        helpers.push(h);
    }
    Box::pin(lead_create(&mut a, &mut b, &mut helpers, ca, cb)).await?;
    Box::pin(lead_list(&mut a, ca)).await?;
    Box::pin(lead_increase(&mut a, &mut b, cb)).await?;
    Box::pin(lead_appoint(&mut a, &mut b, ca, cb)).await?;
    Box::pin(lead_succession(&mut a, &mut b, ca, cb)).await?;

    step(&format!("{ca} se reconnecte : rangs relus"));
    drop(a);
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    let back = a.world.expect_where(gop(0), 3000, "guilde à l'entrée").await?;
    anyhow::ensure!(back.fields().text(3).as_deref() == Some(cb) && back.int(7) == Some(60), "chef {:?}, taille {:?}", back.fields().text(3), back.int(7));
    println!("  ✓ chef {cb}, 60 places");

    step(&format!("{cb}, nouveau chef, dissout {LEAD_NAME}"));
    b.world.send(gd, Fields::new(gd).with(0, 6).with(1, b.cid).with(2, LEAD_NAME).with(9, LEAD_PASSWORD)).await?;
    a.world.expect_where(gop(6), 3000, "dissolution").await?;
    println!("  ✓ {LEAD_NAME} dissoute");
    drop(helpers);
    println!("\ndirection de la guilde : OK");
    Ok(())
}

const LEAD_NAME: &str = "SimLead";
const LEAD_PASSWORD: &str = "simpass";
const LEAD_HELPERS: usize = 18;
/// Guilde de SimA pour les captures des combats.
const WAR_GUILD: &str = "SimWarB";

fn gop(n: i64) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| q.is(&schema::GUILD, Some(n))
}

fn gsys(code: i64) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| q.is(&schema::GUILD, Some(10)) && q.int(1) == Some(code)
}

fn chat_line(cid: u32, text: &str) -> Fields {
    let ch = &schema::CHATTING;
    Fields::new(ch).with(0, 0).with(1, cid).with(2, 0).with(6, crate::tools::to_cp1252(text))
}

fn guild_member(name: &str) -> Vec<u8> {
    build_mini(&mini::GUILD_MEMBER, &Fields::new(&mini::GUILD_MEMBER).with(0, name))
}

fn raw_of(q: &Recv) -> Vec<u8> {
    q.packet.as_ref().map(|p| p.raw.clone()).unwrap_or_default()
}

/// Paquet de l'op 48 (second, passation) de sous-opération `n`.
fn society_sub(n: u8) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| q.is(&schema::GUILD, Some(crate::world::guild::society::OP)) && q.packet.as_ref().is_some_and(|p| p.raw.get(0x18) == Some(&n))
}

/// Membre et rang annoncés par la sous-op 8.
fn society_rank(q: &Recv) -> (String, u32) {
    let r = raw_of(q);
    (crate::world::guild::society::get_text(&r, 0x3a, 49), r.get(0x6c..0x70).map(|b| u32::from_le_bytes(b.try_into().unwrap())).unwrap_or(0))
}

fn appoint_request(me: &str, target: &str, r: u32) -> Vec<u8> {
    use crate::world::guild::society::{put_text, raw_packet, sub};
    raw_packet(sub::APPOINT, 0x80, |b| {
        put_text(b, 0x19, 49, me);
        put_text(b, 0x4a, 49, target);
        b[0x7c..0x80].copy_from_slice(&r.to_le_bytes());
    })
}

fn appoint_answer(guild: &str, me: &str, yes: bool) -> Vec<u8> {
    use crate::world::guild::society::{put_text, raw_packet, sub};
    raw_packet(sub::APPOINT_ANSWER, 0x84, |x| {
        put_text(x, 0x19, 49, guild);
        put_text(x, 0x4a, 49, me);
        x[0x7c..0x80].copy_from_slice(&11u32.to_le_bytes());
        x[0x80..0x84].copy_from_slice(&(yes as u32).to_le_bytes());
    })
}

/// Reste d'un essai interrompu : le personnage quitte sa guilde (ou la dissout s'il en est le chef).
async fn leave_old_guild(w: &mut InWorld) -> anyhow::Result<()> {
    let gd = &schema::GUILD;
    let Ok(old) = w.world.expect_where(gop(0), 400, "guilde existante").await else { return Ok(()) };
    let old_name = old.fields().text(2).unwrap_or_default();
    w.world.send(gd, Fields::new(gd).with(0, 6).with(1, w.cid).with(2, old_name.as_str()).with(9, LEAD_PASSWORD)).await?;
    w.world.send(gd, Fields::new(gd).with(0, 4).with(1, w.cid).with(2, old_name.as_str())).await?;
    let _ = w.world.expect_where(|q| q.is(gd, Some(6)) || q.is(gd, Some(4)), 3000, "ancienne guilde quittée").await;
    println!("  · ancienne guilde {old_name} quittée ou dissoute");
    Ok(())
}

async fn lead_create(a: &mut InWorld, b: &mut InWorld, helpers: &mut [InWorld], ca: &str, cb: &str) -> anyhow::Result<()> {
    let (gd, c) = (&schema::GUILD, &schema::CHARACTER);
    step(&format!("{ca} crée {LEAD_NAME} ; {cb} et les comptes d'appoint la rejoignent"));
    a.world.send(&schema::CHATTING, chat_line(a.cid, "/level 20")).await?;
    a.world.send(&schema::CHATTING, chat_line(a.cid, "/money 20000")).await?;
    a.world.expect_where(|q| q.is(c, Some(1)) && q.int(8) == Some(20_000), 3000, "argent").await?;
    a.world.send(gd, Fields::new(gd).with(0, 0).with(1, a.cid).with(2, LEAD_NAME).with(9, LEAD_PASSWORD)).await?;
    a.world.expect_where(gsys(11), 3000, "création confirmée (code 11)").await?;
    let mut joiners: Vec<(&mut InWorld, String)> = vec![(b, cb.to_string())];
    for (i, h) in helpers.iter_mut().enumerate() {
        joiners.push((h, format!("SimG{:02}", i + 1)));
    }
    let n = joiners.len();
    for (w, name) in joiners {
        a.world.send(gd, Fields::new(gd).with(0, 1).with(1, a.cid).with(2, LEAD_NAME).with(16, guild_member(&name))).await?;
        w.world.expect_where(gop(1), 3000, "invitation").await?;
        w.world.send(gd, Fields::new(gd).with(0, 3).with(1, w.cid).with(2, LEAD_NAME)).await?;
        a.world.expect_where(|q| gsys(13)(q) && q.fields().text(2).as_deref() == Some(name.as_str()), 3000, "adhésion").await?;
    }
    println!("  ✓ {LEAD_NAME} : {} membres", n + 1);
    Ok(())
}

async fn lead_list(a: &mut InWorld, ca: &str) -> anyhow::Result<()> {
    let gd = &schema::GUILD;
    step("liste des guildes : page, recherche, nom inconnu");
    let l = &mini::GUILD_LIST;
    a.world.send(gd, Fields::new(gd).with(0, 28).with(1, a.cid).with(21, build_mini(l, &Fields::new(l).with(1, 0)))).await?;
    let page = a.world.expect_where(gop(28), 3000, "liste des guildes").await?;
    let list = decode_mini(l, page.fields().bytes(21).unwrap_or_default())?;
    let count = list.int(2).unwrap_or(0) as usize;
    let mut found = None;
    for i in 0..count {
        let e = decode_mini(&mini::GUILD_LIST_ITEM, list.bytes(3 + i).unwrap_or_default())?;
        if e.text(0).as_deref() == Some(LEAD_NAME) {
            found = Some((e.text(1).unwrap_or_default(), e.int(5), e.int(6)));
        }
    }
    let (master, members, max) = found.ok_or_else(|| anyhow::anyhow!("{LEAD_NAME} absente de la page 0 ({count} guilde(s))"))?;
    anyhow::ensure!(master == ca && members == Some(20) && max == Some(50), "entrée : chef {master}, {members:?}/{max:?}");
    println!("  ✓ page 0 : {count} guilde(s) sur {}, {LEAD_NAME} (chef {master}, 20/50)", o(list.int(0)));
    a.world.send(gd, Fields::new(gd).with(0, 28).with(1, a.cid).with(2, LEAD_NAME)).await?;
    let searched = a.world.expect_where(gop(28), 3000, "recherche").await?;
    anyhow::ensure!(searched.fields().text(2).as_deref() == Some(LEAD_NAME), "recherche : {:?}", searched.fields().text(2));
    a.world.send(gd, Fields::new(gd).with(0, 28).with(1, a.cid).with(2, "Inexistante")).await?;
    a.world.expect_where(gsys(47), 3000, "guilde introuvable (code 47)").await?;
    println!("  ✓ recherche de {LEAD_NAME} (page rendue), « Inexistante » : code 47");
    Ok(())
}

async fn lead_increase(a: &mut InWorld, b: &mut InWorld, cb: &str) -> anyhow::Result<()> {
    let (gd, c) = (&schema::GUILD, &schema::CHARACTER);
    step("agrandissement : or insuffisant, puis 60 places");
    a.world.send(gd, Fields::new(gd).with(0, 7).with(1, a.cid).with(2, LEAD_NAME).with(7, 60)).await?;
    a.world.expect_where(gsys(44), 3000, "or insuffisant (code 44)").await?;
    a.world.send(&schema::CHATTING, chat_line(a.cid, "/money 1500000")).await?;
    a.world.expect_where(|q| q.is(c, Some(1)) && q.int(8) == Some(1_500_000), 3000, "argent").await?;
    a.world.send(gd, Fields::new(gd).with(0, 7).with(1, a.cid).with(2, LEAD_NAME).with(7, 60)).await?;
    let grown = b.world.expect_where(gop(7), 3000, "nouvelle taille").await?;
    anyhow::ensure!(grown.int(7) == Some(60) && grown.fields().text(2).as_deref() == Some(LEAD_NAME), "taille {:?}", grown.int(7));
    let paid = a.world.expect_where(|q| q.is(c, Some(1)) && q.int(8).is_some(), 3000, "paiement").await?;
    anyhow::ensure!(paid.int(8) == Some(500_000), "argent restant {:?}", paid.int(8));
    println!("  ✓ code 44, puis 60 places (1 000 000 pièces) annoncées à {cb}");
    Ok(())
}

async fn lead_appoint(a: &mut InWorld, b: &mut InWorld, ca: &str, cb: &str) -> anyhow::Result<()> {
    use crate::world::guild::society::{get_text, sub};
    let ch = &schema::CHATTING;
    step(&format!("nomination de {cb} : refusée sous le niveau 20, puis acceptée"));
    a.world.send(ch, chat_line(a.cid, &format!("/level 1 \"{cb}\""))).await?;
    tokio::time::sleep(Duration::from_millis(300)).await;
    a.world.send_plain(appoint_request(ca, cb, 11)).await?;
    anyhow::ensure!(b.world.expect_where(society_sub(sub::APPOINT_ASK), 800, "demande").await.is_err(), "{cb} sollicité au niveau 1");
    a.world.send(ch, chat_line(a.cid, &format!("/level 20 \"{cb}\""))).await?;
    tokio::time::sleep(Duration::from_millis(300)).await;
    a.world.send_plain(appoint_request(ca, cb, 11)).await?;
    let ask = b.world.expect_where(society_sub(sub::APPOINT_ASK), 3000, "demande de nomination").await?;
    let r = raw_of(&ask);
    let seen = (get_text(&r, 0x19, 49), get_text(&r, 0x4a, 33), get_text(&r, 0x6b, 49));
    anyhow::ensure!(seen == (LEAD_NAME.to_string(), ca.to_string(), cb.to_string()), "demande {seen:?}");
    b.world.send_plain(appoint_answer(LEAD_NAME, cb, true)).await?;
    a.world.expect_where(|q| society_sub(sub::RANK_UPDATE)(q) && society_rank(q) == (cb.to_string(), 11), 3000, "second au rang 11").await?;
    b.world.expect_where(society_sub(sub::RANK_UPDATE), 3000, "rang vu de B").await?;
    println!("  ✓ {cb} second (rang 11), annoncé à la guilde");

    step("retrait du rang de second, puis nouvelle nomination");
    a.world.send_plain(appoint_request(ca, cb, 1)).await?;
    b.world.expect_where(|q| society_sub(sub::RANK_UPDATE)(q) && society_rank(q) == (cb.to_string(), 1), 3000, "rang retiré").await?;
    a.world.send_plain(appoint_request(ca, cb, 11)).await?;
    b.world.expect_where(society_sub(sub::APPOINT_ASK), 3000, "demande de nomination").await?;
    b.world.send_plain(appoint_answer(LEAD_NAME, cb, true)).await?;
    b.world.expect_where(|q| society_sub(sub::RANK_UPDATE)(q) && society_rank(q).1 == 11, 3000, "second à nouveau").await?;
    println!("  ✓ rang 1, puis second à nouveau");
    Ok(())
}

async fn lead_succession(a: &mut InWorld, b: &mut InWorld, ca: &str, cb: &str) -> anyhow::Result<()> {
    use crate::world::guild::society::{get_text, put_text, raw_packet, sub};
    step(&format!("passation : {ca} propose, {cb} accepte"));
    a.world
        .send_plain(raw_packet(sub::SUCCESSION, 0x7b, |x| {
            put_text(x, 0x19, 49, ca);
            put_text(x, 0x4a, 49, cb);
        }))
        .await?;
    let ask = b.world.expect_where(society_sub(sub::SUCCESSION_ASK), 3000, "demande de passation").await?;
    anyhow::ensure!(get_text(&raw_of(&ask), 0x4a, 33) == ca, "passation proposée par {:?}", get_text(&raw_of(&ask), 0x4a, 33));
    b.world
        .send_plain(raw_packet(sub::SUCCESSION_ANSWER, 0x80, |x| {
            put_text(x, 0x19, 49, LEAD_NAME);
            put_text(x, 0x4a, 49, cb);
            x[0x7c..0x80].copy_from_slice(&1u32.to_le_bytes());
        }))
        .await?;
    let (old, new) = ((ca.to_string(), 1), (cb.to_string(), 10));
    a.world.expect_where(|q| society_sub(sub::RANK_UPDATE)(q) && society_rank(q) == old, 3000, "ancien chef au rang 1").await?;
    a.world.expect_where(|q| society_sub(sub::RANK_UPDATE)(q) && society_rank(q) == new, 3000, "nouveau chef au rang 10").await?;
    println!("  ✓ {ca} rang 1, {cb} chef (rang 10)");
    Ok(())
}

/// Mise en scène pour les captures d'ArchRust (`sim guildstage <mode> [secondes]`) :
/// * `setup` : GmTest crée SimLead, SimA (niveau 20) et les 18 comptes d'appoint la rejoignent, puis tous
///   se déconnectent (la guilde reste) ;
/// * `present` : SimA reste en jeu et accepte nominations et passations ;
/// * `chef` : GmTest entre en jeu, propose à SimA d'être second, puis de diriger la guilde ;
/// * `dissoudre` : le chef de SimLead (GmTest ou SimA) la dissout, comme SimA SimWarB ;
/// * `guerre-prep` : GmTest crée SimLead avec 9 comptes d'appoint, SimA (niveau 20) SimWarB avec 9 autres,
///   tous au niveau 10, puis tous se déconnectent ;
/// * `guerre-defi` : GmTest défie SimWarB (aux points, 10 minutes) après `secondes`, attend le début, puis
///   abandonne `secondes` plus tard ;
/// * `guerre-refus` : GmTest refuse pendant `secondes` les défis reçus ;
/// * `relation-prep` : comme `guerre-prep`, 20 membres dans chaque guilde (38 comptes d'appoint) ;
/// * `relation-repond` : GmTest accepte pendant `secondes` alliances, hostilités et trêves demandées ;
/// * `relation-demande` : GmTest déclare SimWarB hostile après `secondes`, puis accepte la trêve ;
/// * `entrepot-prep` (après `guerre-prep`) : SimA va chez Holly, dépose une pile et 1000 pièces dans l'entrepôt de
///   SimWarB et garde une autre pile dans son sac.
pub async fn guild_stage(mode: &str, secs: u64) -> anyhow::Result<()> {
    use crate::world::guild::society::{get_text, put_text, raw_packet, sub};
    let cfg = crate::config::load()?;
    let gd = &schema::GUILD;
    match mode {
        "setup" => {
            let mut a = enter_world(&cfg, "gmtest", "gmtest", "GmTest", "A-").await?;
            let mut b = enter_world(&cfg, "sima", "sima", "SimA", "B-").await?;
            Box::pin(leave_old_guild(&mut a)).await?;
            Box::pin(leave_old_guild(&mut b)).await?;
            let mut helpers = Vec::new();
            for (i, user) in helper_accounts(&cfg, LEAD_HELPERS).await?.iter().enumerate() {
                let mut h = enter_world(&cfg, user, user, &format!("SimG{:02}", i + 1), &format!("G{:02}-", i + 1)).await?;
                Box::pin(leave_old_guild(&mut h)).await?;
                helpers.push(h);
            }
            Box::pin(lead_create(&mut a, &mut b, &mut helpers, "GmTest", "SimA")).await?;
            a.world.send(&schema::CHATTING, chat_line(a.cid, "/level 20 \"SimA\"")).await?;
            a.world.send(&schema::CHATTING, chat_line(a.cid, "/money 2000000")).await?;
            tokio::time::sleep(Duration::from_millis(500)).await;
            println!("\nmise en scène : SimLead prête (20 membres)");
        }
        "present" => {
            let mut b = enter_world(&cfg, "sima", "sima", "SimA", "B-").await?;
            let until = tokio::time::Instant::now() + Duration::from_secs(secs);
            while tokio::time::Instant::now() < until {
                let Ok(q) = b.world.expect_where(|q| society_sub(sub::APPOINT_ASK)(q) || society_sub(sub::SUCCESSION_ASK)(q), 500, "demande").await else { continue };
                let r = raw_of(&q);
                let guild = get_text(&r, 0x19, 49);
                if r[0x18] == sub::APPOINT_ASK {
                    b.world.send_plain(appoint_answer(&guild, "SimA", true)).await?;
                    println!("  ✓ SimA accepte d'être second de {guild}");
                } else {
                    b.world
                        .send_plain(raw_packet(sub::SUCCESSION_ANSWER, 0x80, |x| {
                            put_text(x, 0x19, 49, &guild);
                            put_text(x, 0x4a, 49, "SimA");
                            x[0x7c..0x80].copy_from_slice(&1u32.to_le_bytes());
                        }))
                        .await?;
                    println!("  ✓ SimA accepte de diriger {guild}");
                }
            }
        }
        "chef" => {
            let mut a = enter_world(&cfg, "gmtest", "gmtest", "GmTest", "A-").await?;
            tokio::time::sleep(Duration::from_secs(secs)).await;
            a.world.send_plain(appoint_request("GmTest", "SimA", 11)).await?;
            println!("  · nomination de SimA proposée");
            let _ = a.world.expect_where(|q| society_sub(sub::RANK_UPDATE)(q) && society_rank(q).1 == 11, (secs + 20) * 1000, "SimA second").await?;
            println!("  ✓ SimA second");
            tokio::time::sleep(Duration::from_secs(secs)).await;
            a.world
                .send_plain(raw_packet(sub::SUCCESSION, 0x7b, |x| {
                    put_text(x, 0x19, 49, "GmTest");
                    put_text(x, 0x4a, 49, "SimA");
                }))
                .await?;
            println!("  · passation proposée à SimA");
            let _ = a.world.expect_where(|q| society_sub(sub::RANK_UPDATE)(q) && society_rank(q).1 == 10, (secs + 20) * 1000, "SimA chef").await?;
            println!("  ✓ SimA chef");
            tokio::time::sleep(Duration::from_secs(secs)).await;
        }
        "dissoudre" => {
            for (user, name) in [("gmtest", "GmTest"), ("sima", "SimA")] {
                let mut w = enter_world(&cfg, user, user, name, "A-").await?;
                if let Ok(g) = w.world.expect_where(gop(0), 1500, "guilde").await {
                    if g.fields().text(3).as_deref() == Some(name) {
                        let gname = g.fields().text(2).unwrap_or_default();
                        w.world.send(gd, Fields::new(gd).with(0, 6).with(1, w.cid).with(2, gname.as_str()).with(9, LEAD_PASSWORD)).await?;
                        w.world.expect_where(gop(6), 3000, "dissolution").await?;
                        println!("  ✓ {gname} dissoute par {name}");
                    }
                }
            }
        }
        "guerre-prep" => {
            let ch = &schema::CHATTING;
            let mut a = enter_world(&cfg, "gmtest", "gmtest", "GmTest", "A-").await?;
            let mut b = enter_world(&cfg, "sima", "sima", "SimA", "B-").await?;
            Box::pin(leave_old_guild(&mut a)).await?;
            Box::pin(leave_old_guild(&mut b)).await?;
            let mut helpers = Vec::new();
            for (i, user) in helper_accounts(&cfg, LEAD_HELPERS).await?.iter().enumerate() {
                let mut h = enter_world(&cfg, user, user, &format!("SimG{:02}", i + 1), &format!("G{:02}-", i + 1)).await?;
                Box::pin(leave_old_guild(&mut h)).await?;
                helpers.push(h);
            }
            a.world.send(ch, chat_line(a.cid, "/level 20")).await?;
            set_money(&mut a, 20_000).await?;
            a.world.send(ch, chat_line(a.cid, "/level 20 \"SimA\"")).await?;
            a.world.send(ch, chat_line(a.cid, "/money 20000 \"SimA\"")).await?;
            b.world.expect_where(|q| q.is(&schema::CHARACTER, Some(1)) && q.int(8) == Some(20_000), 3000, "argent de SimA").await?;
            for (w, g) in [(&mut a, LEAD_NAME), (&mut b, WAR_GUILD)] {
                w.world.send(gd, Fields::new(gd).with(0, 0).with(1, w.cid).with(2, g).with(9, LEAD_PASSWORD)).await?;
                w.world.expect_where(gsys(11), 3000, "création").await?;
            }
            for (i, h) in helpers.iter_mut().enumerate() {
                let name = format!("SimG{:02}", i + 1);
                a.world.send(ch, chat_line(a.cid, &format!("/level 10 \"{name}\""))).await?;
                if i < 9 {
                    Box::pin(lead_join(&mut a, LEAD_NAME, h, &name)).await?;
                } else {
                    Box::pin(lead_join(&mut b, WAR_GUILD, h, &name)).await?;
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
            println!("\nmise en scène : {LEAD_NAME} (GmTest) et {WAR_GUILD} (SimA), 10 membres chacune");
        }
        "guerre-defi" => {
            let mut a = enter_world(&cfg, "gmtest", "gmtest", "GmTest", "A-").await?;
            tokio::time::sleep(Duration::from_secs(secs)).await;
            let m = &mini::GUILD_BATTLE;
            let request = Fields::new(gd).with(0, 15).with(1, a.cid).with(2, LEAD_NAME).with(3, "").with(17, build_mini(m, &Fields::new(m).with(0, WAR_GUILD).with(5, 0).with(6, 600)));
            a.world.send(gd, request).await?;
            println!("  · {WAR_GUILD} défiée");
            a.world.expect_where(gop(16), (secs + 60) * 1000, "acceptation").await?;
            println!("  ✓ défi accepté");
            a.world.expect_where(gop(21), 120_000, "début").await?;
            println!("  ✓ combat commencé");
            tokio::time::sleep(Duration::from_secs(secs)).await;
            a.world.send(gd, Fields::new(gd).with(0, 24).with(1, a.cid).with(2, LEAD_NAME).with(3, "GmTest")).await?;
            a.world.expect_where(gop(25), 3000, "résultat").await?;
            println!("  ✓ abandon de GmTest");
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
        "guerre-refus" => {
            let mut a = enter_world(&cfg, "gmtest", "gmtest", "GmTest", "A-").await?;
            let until = tokio::time::Instant::now() + Duration::from_secs(secs);
            while tokio::time::Instant::now() < until {
                let Ok(q) = a.world.expect_where(gop(15), 500, "défi").await else { continue };
                let from = decode_mini(&mini::GUILD_BATTLE, q.fields().bytes(17).unwrap_or_default())?.text(0).unwrap_or_default();
                a.world.send(gd, Fields::new(gd).with(0, 17).with(1, a.cid).with(2, LEAD_NAME).with(3, "GmTest")).await?;
                println!("  ✓ défi de {from} refusé");
            }
        }
        "relation-prep" => {
            let ch = &schema::CHATTING;
            let mut a = enter_world(&cfg, "gmtest", "gmtest", "GmTest", "A-").await?;
            let mut b = enter_world(&cfg, "sima", "sima", "SimA", "B-").await?;
            Box::pin(leave_old_guild(&mut a)).await?;
            Box::pin(leave_old_guild(&mut b)).await?;
            let mut helpers = Vec::new();
            for (i, user) in helper_accounts(&cfg, 38).await?.iter().enumerate() {
                let mut h = enter_world(&cfg, user, user, &format!("SimG{:02}", i + 1), &format!("G{:02}-", i + 1)).await?;
                Box::pin(leave_old_guild(&mut h)).await?;
                helpers.push(h);
            }
            a.world.send(ch, chat_line(a.cid, "/level 20")).await?;
            set_money(&mut a, 20_000).await?;
            a.world.send(ch, chat_line(a.cid, "/level 20 \"SimA\"")).await?;
            a.world.send(ch, chat_line(a.cid, "/money 20000 \"SimA\"")).await?;
            b.world.expect_where(|q| q.is(&schema::CHARACTER, Some(1)) && q.int(8) == Some(20_000), 3000, "argent de SimA").await?;
            for (w, g) in [(&mut a, LEAD_NAME), (&mut b, WAR_GUILD)] {
                w.world.send(gd, Fields::new(gd).with(0, 0).with(1, w.cid).with(2, g).with(9, LEAD_PASSWORD)).await?;
                w.world.expect_where(gsys(11), 3000, "création").await?;
            }
            for (i, h) in helpers.iter_mut().enumerate() {
                let name = format!("SimG{:02}", i + 1);
                if i < 19 {
                    Box::pin(lead_join(&mut a, LEAD_NAME, h, &name)).await?;
                } else {
                    Box::pin(lead_join(&mut b, WAR_GUILD, h, &name)).await?;
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
            println!("\nmise en scène : {LEAD_NAME} (GmTest) et {WAR_GUILD} (SimA), 20 membres chacune");
        }
        "entrepot-prep" => {
            let (it, gw) = (&schema::ITEM, &schema::GUILD_WAREHOUSE);
            let mut a = enter_world(&cfg, "gmtest", "gmtest", "GmTest", "A-").await?;
            let mut b = enter_world(&cfg, "sima", "sima", "SimA", "B-").await?;
            let keeper = go_to_keeper(&mut b).await?;
            a.world.send(&schema::CHATTING, chat_line(a.cid, "/create \"SimA\" 190 3")).await?;
            let made = b.world.expect_where(|q| q.is(it, None) && q.int(3) == Some(190) && q.int(1) == Some(2), 3000, "potions").await?;
            let iid = made.int(2).unwrap_or(0);
            a.world.send(&schema::CHATTING, chat_line(a.cid, "/money 5000 \"SimA\"")).await?;
            b.world.expect_where(|q| q.is(&schema::CHARACTER, Some(1)) && q.int(8).is_some(), 3000, "argent").await?;
            warehouse_open(&mut b, keeper).await?;
            b.world.expect_where(gware(1), 3000, "entrepôt ouvert").await?;
            b.world.send(gw, Fields::new(gw).with(0, 5).with(1, b.cid).with(4, iid).with(5, 0).with(6, 0).with(7, 0)).await?;
            b.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(iid), 3000, "pile déposée").await?;
            b.world.send(gw, Fields::new(gw).with(0, 3).with(1, b.cid).with(2, 1000)).await?;
            b.world.expect_where(gware(0), 3000, "argent déposé").await?;
            a.world.send(&schema::CHATTING, chat_line(a.cid, "/create \"SimA\" 191 2")).await?;
            tokio::time::sleep(Duration::from_millis(800)).await;
            println!("
mise en scène : SimA chez Holly, une pile et 1000 pièces dans l'entrepôt de {WAR_GUILD}");
        }
        "relation-repond" | "relation-demande" => {
            let mut a = enter_world(&cfg, "gmtest", "gmtest", "GmTest", "A-").await?;
            if mode == "relation-demande" {
                tokio::time::sleep(Duration::from_secs(secs)).await;
                a.world.send(gd, relation_fields(&a, 38, LEAD_NAME, "GmTest", WAR_GUILD)).await?;
                println!("  · {WAR_GUILD} déclarée hostile");
            }
            let until = tokio::time::Instant::now() + Duration::from_secs(if mode == "relation-demande" { 90 } else { secs });
            while tokio::time::Instant::now() < until {
                let Ok(q) = a.world.expect_where(|q| gop(34)(q) || gop(38)(q) || gop(41)(q), 500, "demande").await else { continue };
                let from = other_guild(&q);
                let answer = match q.int(0) {
                    Some(34) => 36,
                    Some(38) => 40,
                    _ => 43,
                };
                a.world.send(gd, relation_fields(&a, answer, LEAD_NAME, &from, &from)).await?;
                println!("  ✓ demande {:?} de {from} acceptée (op {answer})", q.int(0));
            }
        }
        other => anyhow::bail!("mode inconnu {other:?} : setup | present | chef | dissoudre | guerre-prep | guerre-defi | guerre-refus | relation-prep | relation-repond | relation-demande | entrepot-prep"),
    }
    Ok(())
}

/// Emblèmes : A (compte MJ) crée SimMark ; achat ignoré à 2 membres ; 9 comptes d'appoint la rejoignent
/// (11 membres) ; sans fond : code 58 ; or insuffisant : code 57 ; achat (fond 3, motif 2, symbole 5 :
/// 60 000 pièces), annoncé à B (op 32) et dans les liens (op 8) ; B est exclu, crée SimDup avec les comptes
/// d'appoint (exclus de SimMark) et demande le même emblème : code 59, puis achat malgré tout (op 33) ;
/// emblème relu à la reconnexion de A ; les deux guildes sont dissoutes.
pub async fn guild_mark(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    const MARK: &str = "SimMark";
    const DUP: &str = "SimDup";
    const TID: i64 = 3 | 2 << 8 | 5 << 16;
    const COLOR: i64 = 2 | 5 << 8 | 7 << 16;
    let cfg = crate::config::load()?;
    let (gd, c) = (&schema::GUILD, &schema::CHARACTER);
    let buy = |cid: u32, guild: &str, tid: i64, force: bool| Fields::new(gd).with(0, if force { 33 } else { 32 }).with(1, cid).with(2, guild).with(22, tid).with(23, COLOR);
    step(&format!("{ca}, {cb} et 9 comptes d'appoint entrent en jeu"));
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    let mut b = enter_world(&cfg, ub, pb, cb, "B-").await?;
    Box::pin(leave_old_guild(&mut a)).await?;
    Box::pin(leave_old_guild(&mut b)).await?;
    let mut helpers = Vec::new();
    for (i, user) in helper_accounts(&cfg, 9).await?.iter().enumerate() {
        let mut h = enter_world(&cfg, user, user, &format!("SimG{:02}", i + 1), &format!("G{:02}-", i + 1)).await?;
        Box::pin(leave_old_guild(&mut h)).await?;
        helpers.push(h);
    }

    step(&format!("{ca} crée {MARK} ; achat ignoré à 2 membres"));
    a.world.send(&schema::CHATTING, chat_line(a.cid, "/level 20")).await?;
    set_money(&mut a, 20_000).await?;
    a.world.expect_where(|q| q.is(c, Some(1)) && q.int(8) == Some(20_000), 3000, "argent").await?;
    a.world.send(gd, Fields::new(gd).with(0, 0).with(1, a.cid).with(2, MARK).with(9, LEAD_PASSWORD)).await?;
    a.world.expect_where(gsys(11), 3000, "création").await?;
    Box::pin(lead_join(&mut a, MARK, &mut b, cb)).await?;
    a.world.send(gd, buy(a.cid, MARK, TID, false)).await?;
    anyhow::ensure!(a.world.expect_where(gop(32), 800, "achat").await.is_err(), "emblème acheté à 2 membres");
    println!("  ✓ rien à 2 membres");

    step("9 comptes d'appoint rejoignent la guilde ; sans fond, puis or insuffisant");
    for (i, h) in helpers.iter_mut().enumerate() {
        Box::pin(lead_join(&mut a, MARK, h, &format!("SimG{:02}", i + 1))).await?;
    }
    a.world.send(gd, buy(a.cid, MARK, 2 << 8 | 5 << 16, false)).await?;
    a.world.expect_where(gsys(58), 3000, "sans fond (code 58)").await?;
    set_money(&mut a, 1_000).await?;
    a.world.expect_where(|q| q.is(c, Some(1)) && q.int(8) == Some(1_000), 3000, "argent").await?;
    a.world.send(gd, buy(a.cid, MARK, TID, false)).await?;
    a.world.expect_where(gsys(57), 3000, "or insuffisant (code 57)").await?;
    println!("  ✓ codes 58 et 57");

    step("achat de l'emblème");
    set_money(&mut a, 100_000).await?;
    a.world.expect_where(|q| q.is(c, Some(1)) && q.int(8) == Some(100_000), 3000, "argent").await?;
    a.world.send(gd, buy(a.cid, MARK, TID, false)).await?;
    let seen = b.world.expect_where(gop(32), 3000, "emblème annoncé").await?;
    anyhow::ensure!(seen.int(22) == Some(TID) && seen.int(23) == Some(COLOR), "emblème {:?}/{:?}", seen.int(22), seen.int(23));
    let paid = a.world.expect_where(|q| q.is(c, Some(1)) && q.int(8).is_some(), 3000, "paiement").await?;
    anyhow::ensure!(paid.int(8) == Some(40_000), "argent restant {:?}", paid.int(8));
    let link = b.world.expect_where(|q| gop(8)(q) && q.int(1) == Some(a.cid as i64), 3000, "lien de A").await?;
    anyhow::ensure!(link.int(22) == Some(TID), "lien {:?}", link.int(22));
    println!("  ✓ emblème {TID:#x} (60 000 pièces), annoncé à {cb} et dans le lien de {ca}");

    step(&format!("{cb} crée {DUP} avec les comptes d'appoint et demande le même emblème"));
    for name in std::iter::once(cb.to_string()).chain((1..=9).map(|i| format!("SimG{i:02}"))) {
        a.world.send(gd, Fields::new(gd).with(0, 5).with(1, a.cid).with(2, MARK).with(16, guild_member(&name))).await?;
        a.world.expect_where(|q| gsys(16)(q) && q.fields().text(2).as_deref() == Some(name.as_str()), 3000, "exclusion").await?;
    }
    a.world.send(&schema::CHATTING, chat_line(a.cid, &format!("/level 20 \"{cb}\""))).await?;
    a.world.send(&schema::CHATTING, chat_line(a.cid, &format!("/money 200000 \"{cb}\""))).await?;
    b.world.expect_where(|q| q.is(c, Some(1)) && q.int(8) == Some(200_000), 3000, "argent de B").await?;
    b.world.send(gd, Fields::new(gd).with(0, 0).with(1, b.cid).with(2, DUP).with(9, LEAD_PASSWORD)).await?;
    b.world.expect_where(gsys(11), 3000, "création de SimDup").await?;
    for (i, h) in helpers.iter_mut().enumerate() {
        Box::pin(lead_join(&mut b, DUP, h, &format!("SimG{:02}", i + 1))).await?;
    }
    b.world.send(gd, buy(b.cid, DUP, TID, false)).await?;
    b.world.expect_where(gsys(59), 3000, "même emblème (code 59)").await?;
    b.world.send(gd, buy(b.cid, DUP, TID, true)).await?;
    b.world.expect_where(|q| gop(32)(q) && q.int(22) == Some(TID), 3000, "achat malgré tout").await?;
    println!("  ✓ code 59, puis op 33 accordée");

    step(&format!("{ca} se reconnecte : emblème relu"));
    drop(a);
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    let back = a.world.expect_where(gop(0), 3000, "guilde à l'entrée").await?;
    anyhow::ensure!(back.int(22) == Some(TID) && back.int(23) == Some(COLOR), "emblème {:?}/{:?}", back.int(22), back.int(23));
    println!("  ✓ {MARK} : emblème {TID:#x}, couleur {COLOR:#x}");

    step("dissolution des deux guildes");
    a.world.send(gd, Fields::new(gd).with(0, 6).with(1, a.cid).with(2, MARK).with(9, LEAD_PASSWORD)).await?;
    a.world.expect_where(gop(6), 3000, "dissolution de SimMark").await?;
    b.world.send(gd, Fields::new(gd).with(0, 6).with(1, b.cid).with(2, DUP).with(9, LEAD_PASSWORD)).await?;
    b.world.expect_where(gop(6), 3000, "dissolution de SimDup").await?;
    drop(helpers);
    println!("\nemblèmes : OK");
    Ok(())
}

/// `/money n` (commande MJ) pour `w`.
async fn set_money(w: &mut InWorld, n: i64) -> anyhow::Result<()> {
    w.world.send(&schema::CHATTING, chat_line(w.cid, &format!("/money {n}"))).await
}

/// Le chef de `guild` invite `w` (personnage `name`), qui accepte.
async fn lead_join(master: &mut InWorld, guild: &str, w: &mut InWorld, name: &str) -> anyhow::Result<()> {
    let gd = &schema::GUILD;
    master.world.send(gd, Fields::new(gd).with(0, 1).with(1, master.cid).with(2, guild).with(16, guild_member(name))).await?;
    w.world.expect_where(gop(1), 3000, "invitation").await?;
    w.world.send(gd, Fields::new(gd).with(0, 3).with(1, w.cid).with(2, guild)).await?;
    master.world.expect_where(|q| gsys(13)(q) && q.fields().text(2).as_deref() == Some(name), 3000, "adhésion").await?;
    Ok(())
}

/// Lieu des combats du simulateur (Anchorville).
const WAR_SPOT: archcore_net::Vec3 = archcore_net::Vec3 { x: -476_000.0, y: 4_422.0, z: -61_000.0 };

/// Combats de guildes (le serveur a `Guild.BattleReadySeconds` court, 10 s par exemple) : A (compte MJ) crée
/// SimWarA avec 9 comptes d'appoint, B crée SimWarB avec 9 autres. Guilde inconnue (code 27), niveaux
/// insuffisants (code 22, 100 en deuxième nombre) ; défi (op 15, code 28, statut 1) refusé (op 17, code 30) ; défi au point accepté
/// (op 16 : délai), annulation trop tardive (code 31) ; avant le début, A ne peut pas frapper B ; début
/// (op 21), horloge (op 22), A tue B (op 23 : score 1), B abandonne (op 24 → op 25 : 4 et 5, bilan op 12) ;
/// défi « mort du chef » annulé en plein combat (op 18 → 19) ; nouveau défi « mort du chef » : A tue le chef B
/// (op 25 : victoire 1, défaite 3) ; bilan relu à la reconnexion ; B refuse les combats (option 0x04 : code 43).
pub async fn guild_battle(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    const GA: &str = "SimWarA";
    const GB: &str = "SimWarB";
    let cfg = crate::config::load()?;
    let (gd, c, ch) = (&schema::GUILD, &schema::CHARACTER, &schema::CHATTING);
    step(&format!("{ca}, {cb} et 18 comptes d'appoint entrent en jeu"));
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    let mut b = enter_world(&cfg, ub, pb, cb, "B-").await?;
    Box::pin(leave_old_guild(&mut a)).await?;
    Box::pin(leave_old_guild(&mut b)).await?;
    let mut helpers = Vec::new();
    for (i, user) in helper_accounts(&cfg, 18).await?.iter().enumerate() {
        let mut h = enter_world(&cfg, user, user, &format!("SimG{:02}", i + 1), &format!("G{:02}-", i + 1)).await?;
        Box::pin(leave_old_guild(&mut h)).await?;
        helpers.push(h);
    }

    step(&format!("{ca} crée {GA}, {cb} crée {GB} ; 9 comptes d'appoint dans chacune"));
    a.world.send(ch, chat_line(a.cid, "/level 20")).await?;
    set_money(&mut a, 20_000).await?;
    a.world.send(ch, chat_line(a.cid, &format!("/level 20 \"{cb}\""))).await?;
    a.world.send(ch, chat_line(a.cid, &format!("/money 20000 \"{cb}\""))).await?;
    b.world.expect_where(|q| q.is(c, Some(1)) && q.int(8) == Some(20_000), 3000, "argent de B").await?;
    a.world.send(gd, Fields::new(gd).with(0, 0).with(1, a.cid).with(2, GA).with(9, LEAD_PASSWORD)).await?;
    a.world.expect_where(gsys(11), 3000, "création de SimWarA").await?;
    b.world.send(gd, Fields::new(gd).with(0, 0).with(1, b.cid).with(2, GB).with(9, LEAD_PASSWORD)).await?;
    b.world.expect_where(gsys(11), 3000, "création de SimWarB").await?;
    for (i, h) in helpers.iter_mut().enumerate() {
        let name = format!("SimG{:02}", i + 1);
        a.world.send(ch, chat_line(a.cid, &format!("/level 1 \"{name}\""))).await?;
        if i < 9 {
            Box::pin(lead_join(&mut a, GA, h, &name)).await?;
        } else {
            Box::pin(lead_join(&mut b, GB, h, &name)).await?;
        }
    }
    println!("  ✓ 10 membres de chaque côté");

    step("défis refusés : guilde inconnue, niveaux insuffisants");
    let request = |w: &InWorld, enemy: &str, kind: i64, secs: i64| {
        let m = &mini::GUILD_BATTLE;
        Fields::new(gd).with(0, 15).with(1, w.cid).with(2, GA).with(3, "").with(17, build_mini(m, &Fields::new(m).with(0, enemy).with(5, kind).with(6, secs)))
    };
    a.world.send(gd, request(&a, "Inexistante", 0, 600)).await?;
    a.world.expect_where(gsys(27), 3000, "guilde inconnue (code 27)").await?;
    a.world.send(gd, request(&a, GB, 0, 600)).await?;
    let lv = a.world.expect_where(gsys(22), 3000, "niveaux insuffisants (code 22)").await?;
    let second = match lv.fields().get(4) {
        Some(archcore_net::Value::List(v)) => v.get(1).cloned(),
        _ => None,
    };
    anyhow::ensure!(second == Some(archcore_net::Value::Int(100)), "code 22 : deuxième nombre {second:?}");
    for i in 1..=18 {
        a.world.send(ch, chat_line(a.cid, &format!("/level 10 \"SimG{i:02}\""))).await?;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    println!("  ✓ codes 27 et 22 ; comptes d'appoint au niveau 10");

    step(&format!("{ca} défie {GB}, {cb} refuse"));
    a.world.send(gd, request(&a, GB, 2, 300)).await?;
    a.world.expect_where(gsys(28), 3000, "défi lancé (code 28)").await?;
    let ask = b.world.expect_where(gop(15), 3000, "défi reçu").await?;
    let m = decode_mini(&mini::GUILD_BATTLE, ask.fields().bytes(17).unwrap_or_default())?;
    anyhow::ensure!(m.text(0).as_deref() == Some(GA) && m.int(5) == Some(2) && m.int(6) == Some(300), "défi {:?} {:?} {:?}", m.text(0), m.int(5), m.int(6));
    b.world.send(gd, Fields::new(gd).with(0, 17).with(1, b.cid).with(2, GB).with(3, cb)).await?;
    a.world.expect_where(gsys(30), 3000, "refus (code 30)").await?;
    println!("  ✓ op 15 reçue par {cb} (SimWarA, type 2, 5 min), refus : code 30");

    step(&format!("défi aux points accepté ; annulation trop tardive ; pas de coups avant le début"));
    for w in [&mut a, &mut b] {
        let mv = &schema::OPT_CHAR_MOVE;
        w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, WAR_SPOT).with(2, WAR_SPOT).with(5, 0x04).with(6, 0)).await?;
    }
    a.world.send(gd, request(&a, GB, 0, 600)).await?;
    b.world.expect_where(gop(15), 3000, "défi reçu").await?;
    b.world.send(gd, Fields::new(gd).with(0, 16).with(1, b.cid).with(2, GB).with(3, cb)).await?;
    let acc = a.world.expect_where(gop(16), 3000, "acceptation").await?;
    let m = decode_mini(&mini::GUILD_BATTLE, acc.fields().bytes(17).unwrap_or_default())?;
    anyhow::ensure!(m.text(0).as_deref() == Some(GB) && m.text(1).as_deref() == Some(cb), "acceptation {:?} {:?}", m.text(0), m.text(1));
    let ready = m.int(3).unwrap_or(0);
    b.world.expect_where(gop(16), 3000, "acceptation vue de B").await?;
    a.world.send(gd, Fields::new(gd).with(0, 18).with(1, a.cid).with(2, GA).with(3, ca)).await?;
    a.world.expect_where(gsys(31), 3000, "annulation trop tardive (code 31)").await?;
    let act = &schema::OPT_CHAR_ACTION;
    a.world.send(act, Fields::new(act).with(0, a.cid).with(1, b.cid).with(6, 1)).await?;
    anyhow::ensure!(b.world.expect_where(|q| q.is(act, None) && q.int(1) == Some(b.cid as i64), 1500, "coup").await.is_err(), "{cb} frappé avant le début");
    println!("  ✓ op 16 (délai {ready} s) des deux côtés, code 31, aucun coup avant le début");

    step("début, coups, abandon de B");
    a.world.expect_where(gop(21), (ready as u64 + 10) * 1000, "début").await?;
    b.world.expect_where(gop(21), 3000, "début vu de B").await?;
    a.world.expect_where(gop(22), 3000, "horloge").await?;
    a.world.send(ch, chat_line(a.cid, &format!("/level 1 \"{cb}\""))).await?;
    tokio::time::sleep(Duration::from_millis(300)).await;
    Box::pin(kill_player(&mut a, &mut b)).await?;
    let sc = a.world.expect_where(|q| gop(23)(q), 5000, "score").await?;
    let m = decode_mini(&mini::GUILD_BATTLE, sc.fields().bytes(17).unwrap_or_default())?;
    anyhow::ensure!(m.int(9) == Some(1) && m.int(11) == Some(1), "score {:?} gain {:?}", m.int(9), m.int(11));
    b.world.send(gd, Fields::new(gd).with(0, 24).with(1, b.cid).with(2, GB).with(3, cb)).await?;
    let res = a.world.expect_where(gop(25), 3000, "résultat").await?;
    let m = decode_mini(&mini::GUILD_BATTLE, res.fields().bytes(17).unwrap_or_default())?;
    anyhow::ensure!(m.int(13) == Some(4), "résultat {:?}", m.int(13));
    let rb = b.world.expect_where(gop(25), 3000, "résultat vu de B").await?;
    anyhow::ensure!(decode_mini(&mini::GUILD_BATTLE, rb.fields().bytes(17).unwrap_or_default())?.int(13) == Some(5), "résultat de B");
    let rec = a.world.expect_where(gop(12), 3000, "bilan").await?;
    anyhow::ensure!((rec.int(12), rec.int(15)) == (Some(1), Some(1)), "bilan {:?} {:?}", rec.int(12), rec.int(15));
    a.world.send(ch, chat_line(a.cid, &format!("/level 20 \"{cb}\""))).await?;
    println!("  ✓ op 21, op 22, {cb} tué (score 1), abandon : résultats 4 et 5, bilan 1 victoire, 1 point");

    step("« mort du chef » annulé en plein combat");
    b.world.send(c, Fields::new(c).with(0, 9).with(1, b.cid)).await?;
    b.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(b.cid as i64) && q.int(11) == Some(0), 5000, "résurrection").await?;
    a.world.send(gd, request(&a, GB, 2, 300)).await?;
    b.world.expect_where(gop(15), 3000, "défi reçu").await?;
    b.world.send(gd, Fields::new(gd).with(0, 16).with(1, b.cid).with(2, GB).with(3, cb)).await?;
    a.world.expect_where(gop(21), (ready as u64 + 10) * 1000, "début").await?;
    a.world.send(gd, Fields::new(gd).with(0, 18).with(1, a.cid).with(2, GA).with(3, ca)).await?;
    b.world.expect_where(gop(18), 3000, "demande d'annulation").await?;
    b.world.send(gd, Fields::new(gd).with(0, 19).with(1, b.cid).with(2, GB).with(3, cb)).await?;
    a.world.expect_where(gop(19), 3000, "annulation").await?;
    b.world.expect_where(gop(19), 3000, "annulation vue de B").await?;
    println!("  ✓ op 18 → 19 des deux côtés");

    step("« mort du chef » : A tue le chef de SimWarB");
    a.world.send(gd, request(&a, GB, 2, 300)).await?;
    b.world.expect_where(gop(15), 3000, "défi reçu").await?;
    b.world.send(gd, Fields::new(gd).with(0, 16).with(1, b.cid).with(2, GB).with(3, cb)).await?;
    a.world.expect_where(gop(21), (ready as u64 + 10) * 1000, "début").await?;
    {
        let mv = &schema::OPT_CHAR_MOVE;
        b.world.send(mv, Fields::new(mv).with(0, b.cid).with(1, WAR_SPOT).with(2, WAR_SPOT).with(5, 0x04).with(6, 0)).await?;
    }
    a.world.send(ch, chat_line(a.cid, &format!("/level 1 \"{cb}\""))).await?;
    tokio::time::sleep(Duration::from_millis(300)).await;
    Box::pin(kill_player(&mut a, &mut b)).await?;
    let res = a.world.expect_where(gop(25), 5000, "résultat").await?;
    let m = decode_mini(&mini::GUILD_BATTLE, res.fields().bytes(17).unwrap_or_default())?;
    anyhow::ensure!(m.int(13) == Some(1), "résultat {:?}", m.int(13));
    let rb = b.world.expect_where(gop(25), 3000, "résultat vu de B").await?;
    anyhow::ensure!(decode_mini(&mini::GUILD_BATTLE, rb.fields().bytes(17).unwrap_or_default())?.int(13) == Some(3), "résultat de B");
    println!("  ✓ chef tué : victoire (1) et défaite (3)");

    step(&format!("{ca} se reconnecte : bilan relu ; {cb} refuse les combats"));
    drop(a);
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    let rec = a.world.expect_where(gop(12), 3000, "bilan à l'entrée").await?;
    anyhow::ensure!((rec.int(12), rec.int(13), rec.int(14), rec.int(15)) == (Some(2), Some(0), Some(0), Some(2)), "bilan {:?}", (rec.int(12), rec.int(13), rec.int(14), rec.int(15)));
    b.world.send(c, Fields::new(c).with(0, 9).with(1, b.cid)).await?;
    b.world.send(c, Fields::new(c).with(0, 0x11).with(1, b.cid).with(22, 0x04)).await?;
    b.world.expect_where(|q| q.is(c, Some(0x11)), 3000, "drapeaux").await?;
    a.world.send(gd, request(&a, GB, 0, 600)).await?;
    a.world.expect_where(gsys(43), 3000, "combats refusés (code 43)").await?;
    b.world.send(c, Fields::new(c).with(0, 0x11).with(1, b.cid).with(22, 0)).await?;
    println!("  ✓ 2 victoires, 2 points ; code 43");

    step("dissolution des deux guildes");
    a.world.send(gd, Fields::new(gd).with(0, 6).with(1, a.cid).with(2, GA).with(9, LEAD_PASSWORD)).await?;
    a.world.expect_where(gop(6), 3000, "dissolution de SimWarA").await?;
    b.world.send(gd, Fields::new(gd).with(0, 6).with(1, b.cid).with(2, GB).with(9, LEAD_PASSWORD)).await?;
    b.world.expect_where(gop(6), 3000, "dissolution de SimWarB").await?;
    drop(helpers);
    println!("\ncombats de guildes : OK");
    Ok(())
}

/// A frappe B jusqu'à sa mort (B sait qu'il est mort par l'op 1, statut 2).
async fn kill_player(a: &mut InWorld, b: &mut InWorld) -> anyhow::Result<()> {
    let (act, c) = (&schema::OPT_CHAR_ACTION, &schema::CHARACTER);
    let target = b.cid as i64;
    a.world.send(act, Fields::new(act).with(0, a.cid).with(1, b.cid).with(6, 1)).await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        if b.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(target) && q.int(11) == Some(2), 1500, "mort").await.is_ok() {
            println!("  · {} tué", b.cid);
            return Ok(());
        }
        anyhow::ensure!(tokio::time::Instant::now() < deadline, "B toujours en vie au bout d'une minute");
        a.world.send(act, Fields::new(act).with(0, a.cid).with(1, b.cid).with(6, 1)).await?;
    }
}

/// Paquet d'alliance ou d'hostilité : op `o`, guilde (2), nom (3), autre guilde (17, GUILD_BATTLE champ 0).
fn relation_fields(w: &InWorld, o: i64, guild: &str, name: &str, other: &str) -> Fields {
    let (gd, m) = (&schema::GUILD, &mini::GUILD_BATTLE);
    Fields::new(gd).with(0, o).with(1, w.cid).with(2, guild).with(3, name).with(17, build_mini(m, &Fields::new(m).with(0, other)))
}

/// Autre guilde (champ 17) d'un paquet reçu.
fn other_guild(q: &Recv) -> String {
    q.fields().bytes(17).and_then(|b| decode_mini(&mini::GUILD_BATTLE, b).ok()).and_then(|m| m.text(0)).unwrap_or_default()
}

/// Op `n` dont l'autre guilde est `other`.
fn relation_op(n: i64, other: &'static str) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| gop(n)(q) && other_guild(q) == other
}

/// Alliances et hostilités : A (compte MJ) crée SimRelA avec 19 comptes d'appoint (20 membres), B crée SimRelB
/// avec 18 autres. Alliance avec une guilde inconnue (0x3c), avec une guilde de 19 membres (0x42) ; 20e membre ;
/// demande (op 34, attente 0x43) refusée (op 35, 0x44), puis acceptée (op 36 des deux côtés, rôles 2 et 1,
/// 0x46, détails op 44) ; déjà alliées (0x3e) ; hostilité envers une alliée (0x4b) ; B quitte l'alliance
/// (op 37, 0x47 ; dissoute pour A : 0x49) ; hostilité (op 38, 0x51) refusée (op 39, 0x52), puis acceptée
/// (op 40 des deux côtés, 0x54, détails op 45) ; déjà hostiles (0x4c) ; trêve (op 41, 0x56) refusée (op 39,
/// 0x57), puis acceptée (op 43 des deux côtés, 0x59) ; hostilité relue à la reconnexion de A ; la dissolution
/// de SimRelA y met fin pour B (op 43).
pub async fn guild_relation(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    const GA: &str = "SimRelA";
    const GB: &str = "SimRelB";
    let cfg = crate::config::load()?;
    let (gd, ch) = (&schema::GUILD, &schema::CHATTING);
    step(&format!("{ca}, {cb} et 38 comptes d'appoint entrent en jeu"));
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    let mut b = enter_world(&cfg, ub, pb, cb, "B-").await?;
    Box::pin(leave_old_guild(&mut a)).await?;
    Box::pin(leave_old_guild(&mut b)).await?;
    let mut helpers = Vec::new();
    for (i, user) in helper_accounts(&cfg, 38).await?.iter().enumerate() {
        let mut h = enter_world(&cfg, user, user, &format!("SimG{:02}", i + 1), &format!("G{:02}-", i + 1)).await?;
        Box::pin(leave_old_guild(&mut h)).await?;
        helpers.push(h);
    }

    step(&format!("{ca} crée {GA} (20 membres), {cb} crée {GB} (19 membres)"));
    a.world.send(ch, chat_line(a.cid, "/level 20")).await?;
    set_money(&mut a, 20_000).await?;
    a.world.send(ch, chat_line(a.cid, &format!("/level 20 \"{cb}\""))).await?;
    a.world.send(ch, chat_line(a.cid, &format!("/money 20000 \"{cb}\""))).await?;
    b.world.expect_where(|q| q.is(&schema::CHARACTER, Some(1)) && q.int(8) == Some(20_000), 3000, "argent de B").await?;
    a.world.send(gd, Fields::new(gd).with(0, 0).with(1, a.cid).with(2, GA).with(9, LEAD_PASSWORD)).await?;
    a.world.expect_where(gsys(11), 3000, "création de SimRelA").await?;
    b.world.send(gd, Fields::new(gd).with(0, 0).with(1, b.cid).with(2, GB).with(9, LEAD_PASSWORD)).await?;
    b.world.expect_where(gsys(11), 3000, "création de SimRelB").await?;
    for (i, h) in helpers.iter_mut().enumerate().take(37) {
        let name = format!("SimG{:02}", i + 1);
        if i < 19 {
            Box::pin(lead_join(&mut a, GA, h, &name)).await?;
        } else {
            Box::pin(lead_join(&mut b, GB, h, &name)).await?;
        }
    }
    println!("  ✓ {GA} : 20 membres, {GB} : 19 membres");

    step("alliance impossible : guilde inconnue, effectif de l'autre guilde");
    a.world.send(gd, relation_fields(&a, 34, GA, ca, "Inexistante")).await?;
    a.world.expect_where(gsys(0x3c), 3000, "guilde inconnue (0x3c)").await?;
    a.world.send(gd, relation_fields(&a, 34, GA, ca, GB)).await?;
    a.world.expect_where(gsys(0x42), 3000, "effectif de SimRelB (0x42)").await?;
    Box::pin(lead_join(&mut b, GB, &mut helpers[37], "SimG38")).await?;
    println!("  ✓ codes 0x3c et 0x42 ; {GB} : 20 membres");

    step("alliance refusée, puis acceptée");
    a.world.send(gd, relation_fields(&a, 34, GA, ca, GB)).await?;
    a.world.expect_where(gsys(0x43), 3000, "attente (0x43)").await?;
    let ask = b.world.expect_where(relation_op(34, GA), 3000, "demande d'alliance").await?;
    anyhow::ensure!(ask.fields().text(2).as_deref() == Some(GB) && ask.fields().text(3).as_deref() == Some(ca), "demande {:?} {:?}", ask.fields().text(2), ask.fields().text(3));
    b.world.send(gd, relation_fields(&b, 35, GB, cb, GA)).await?;
    a.world.expect_where(gsys(0x44), 3000, "refus (0x44)").await?;
    a.world.send(gd, relation_fields(&a, 34, GA, ca, GB)).await?;
    b.world.expect_where(relation_op(34, GA), 3000, "demande d'alliance").await?;
    b.world.send(gd, relation_fields(&b, 36, GB, cb, GA)).await?;
    let ja = a.world.expect_where(relation_op(36, GB), 3000, "alliée vue de A").await?;
    a.world.expect_where(gsys(0x46), 3000, "alliance (0x46)").await?;
    let jb = b.world.expect_where(relation_op(36, GA), 3000, "alliée vue de B").await?;
    anyhow::ensure!((ja.int(11), jb.int(11)) == (Some(1), Some(2)), "rôles {:?} {:?}", ja.int(11), jb.int(11));
    b.world.expect_where(gop(44), 3000, "détails des alliées").await?;
    a.world.send(gd, relation_fields(&a, 34, GA, ca, GB)).await?;
    a.world.expect_where(gsys(0x3e), 3000, "déjà alliées (0x3e)").await?;
    a.world.send(gd, relation_fields(&a, 38, GA, ca, GB)).await?;
    a.world.expect_where(gsys(0x4b), 3000, "hostilité envers une alliée (0x4b)").await?;
    println!("  ✓ op 34 → 35 (0x44), 34 → 36 (rôles 2 et 1, op 44), 0x3e, 0x4b");

    step(&format!("{GB} quitte l'alliance"));
    b.world.send(gd, relation_fields(&b, 37, GB, "", GB)).await?;
    b.world.expect_where(relation_op(37, GB), 3000, "départ vu de B").await?;
    b.world.expect_where(gsys(0x47), 3000, "départ (0x47)").await?;
    a.world.expect_where(relation_op(37, GB), 3000, "départ vu de A").await?;
    a.world.expect_where(gsys(0x49), 3000, "alliance dissoute (0x49)").await?;
    println!("  ✓ op 37, 0x47 pour B, 0x49 pour A");

    step("hostilité refusée, puis acceptée");
    a.world.send(gd, relation_fields(&a, 38, GA, ca, GB)).await?;
    a.world.expect_where(gsys(0x51), 3000, "attente (0x51)").await?;
    b.world.expect_where(relation_op(38, GA), 3000, "déclaration d'hostilité").await?;
    b.world.send(gd, relation_fields(&b, 39, GB, cb, GA)).await?;
    a.world.expect_where(gsys(0x52), 3000, "refus (0x52)").await?;
    a.world.send(gd, relation_fields(&a, 38, GA, ca, GB)).await?;
    b.world.expect_where(relation_op(38, GA), 3000, "déclaration d'hostilité").await?;
    b.world.send(gd, relation_fields(&b, 40, GB, cb, GA)).await?;
    a.world.expect_where(relation_op(40, GB), 3000, "hostile vue de A").await?;
    a.world.expect_where(gsys(0x54), 3000, "hostilité (0x54)").await?;
    b.world.expect_where(relation_op(40, GA), 3000, "hostile vue de B").await?;
    b.world.expect_where(gop(45), 3000, "détails des hostiles").await?;
    a.world.send(gd, relation_fields(&a, 38, GA, ca, GB)).await?;
    a.world.expect_where(gsys(0x4c), 3000, "déjà hostiles (0x4c)").await?;
    println!("  ✓ op 38 → 39 (0x52), 38 → 40 (op 45), 0x4c");

    step("trêve refusée, puis acceptée");
    a.world.send(gd, relation_fields(&a, 41, GA, ca, GB)).await?;
    a.world.expect_where(gsys(0x56), 3000, "attente (0x56)").await?;
    b.world.expect_where(relation_op(41, GA), 3000, "demande de trêve").await?;
    b.world.send(gd, relation_fields(&b, 39, GB, cb, GA)).await?;
    a.world.expect_where(gsys(0x57), 3000, "refus (0x57)").await?;
    a.world.send(gd, relation_fields(&a, 41, GA, ca, GB)).await?;
    b.world.expect_where(relation_op(41, GA), 3000, "demande de trêve").await?;
    b.world.send(gd, relation_fields(&b, 43, GB, GA, GA)).await?;
    a.world.expect_where(relation_op(43, GB), 3000, "trêve vue de A").await?;
    a.world.expect_where(gsys(0x59), 3000, "trêve (0x59)").await?;
    b.world.expect_where(relation_op(43, GA), 3000, "trêve vue de B").await?;
    println!("  ✓ op 41 → 39 (0x57), 41 → 43 (0x59)");

    step(&format!("hostilité relue à la reconnexion de {ca} ; dissolution"));
    a.world.send(gd, relation_fields(&a, 38, GA, ca, GB)).await?;
    b.world.expect_where(relation_op(38, GA), 3000, "déclaration d'hostilité").await?;
    b.world.send(gd, relation_fields(&b, 40, GB, cb, GA)).await?;
    a.world.expect_where(gsys(0x54), 3000, "hostilité (0x54)").await?;
    drop(a);
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    a.world.expect_where(relation_op(40, GB), 3000, "hostile à l'entrée").await?;
    a.world.expect_where(gop(45), 3000, "détails à l'entrée").await?;
    a.world.send(gd, Fields::new(gd).with(0, 6).with(1, a.cid).with(2, GA).with(9, LEAD_PASSWORD)).await?;
    a.world.expect_where(gop(6), 3000, "dissolution de SimRelA").await?;
    b.world.expect_where(relation_op(43, GA), 3000, "fin de l'hostilité pour B").await?;
    b.world.send(gd, Fields::new(gd).with(0, 6).with(1, b.cid).with(2, GB).with(9, LEAD_PASSWORD)).await?;
    b.world.expect_where(gop(6), 3000, "dissolution de SimRelB").await?;
    drop(helpers);
    println!("  ✓ op 40 et 45 à l'entrée ; op 43 à la dissolution");
    println!("\nalliances et hostilités : OK");
    Ok(())
}

/// Près de Holly (gardienne de banque d'Anchorville, fonctions 20 et 35).
const KEEPER_SPOT: archcore_net::Vec3 = archcore_net::Vec3 { x: -480_400.0, y: 4_920.0, z: -69_100.0 };

/// Va chez Holly et rend son CID.
async fn go_to_keeper(w: &mut InWorld) -> anyhow::Result<i64> {
    let mv = &schema::OPT_CHAR_MOVE;
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, KEEPER_SPOT).with(2, KEEPER_SPOT).with(5, 0x04).with(6, 0)).await?;
    let mut keeper = w.npcs.iter().find(|n| n.1.ends_with(" Holly")).map(|n| n.0);
    while let Ok(p) = w.world.expect_where(|q| q.is(&schema::OPT_VIEW, Some(1)), 1500, "PNJ").await {
        if let Ok(view) = decode_mini(&mini::OPT_VIEW_CHAR, p.fields().bytes(1).unwrap_or_default()) {
            if view.text(2).is_some_and(|n| n.ends_with(" Holly")) {
                keeper = view.int(0);
            }
        }
    }
    keeper.ok_or_else(|| anyhow::anyhow!("Holly introuvable"))
}

/// AgpmEventGuild op 2 chez le gardien (fonction 35) ← op 3, puis AgpmGuildWarehouse op 1.
async fn warehouse_open(w: &mut InWorld, keeper: i64) -> anyhow::Result<()> {
    let (eg, eb, gw) = (&schema::EVENT_GUILD, &mini::EVENT_BASE, &schema::GUILD_WAREHOUSE);
    let base = build_mini(eb, &Fields::new(eb).with(0, 3).with(1, keeper).with(3, 35));
    w.world.send(eg, Fields::new(eg).with(0, 2).with(1, base).with(2, w.cid)).await?;
    w.world.expect_where(|q| q.is(eg, Some(3)), 3000, "accord du gardien (op 3)").await?;
    w.world.send(gw, Fields::new(gw).with(0, 1).with(1, w.cid)).await
}

fn gware(op: i64) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| q.is(&schema::GUILD_WAREHOUSE, Some(op))
}

/// Entrepôt de guilde : A (compte MJ) crée SimWare, B la rejoint. Chez Holly (fonction 35) : accord (AgpmEventGuild
/// op 3), refus sous 10 membres (code 0x5f) ; 8 comptes d'appoint ; ouverture (op 0 argent, op 1 résultat 2) ;
/// A dépose une pile (op 5 → AGPMITEM statut 53, case au champ 7) et 1000 pièces (op 3 → op 0) ; B ouvre et voit
/// la pile, ne peut ni retirer d'argent ni reprendre la pile (le chef seul), dépose 200 pièces ; dissolution refusée
/// (code 0x60) ; à la reconnexion de A, pile et argent relus ; A reprend la pile (op 6 → statut 2 ; B la voit
/// partir) et l'argent (op 4) ; agrandissement : résultat 4 (faute d'or) ; dissolution.
pub async fn guild_warehouse(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    const GW: &str = "SimWare";
    const POTION: i64 = 190;
    const STATUS: i64 = 53;
    let cfg = crate::config::load()?;
    let (gd, gw, it, c, ch) = (&schema::GUILD, &schema::GUILD_WAREHOUSE, &schema::ITEM, &schema::CHARACTER, &schema::CHATTING);
    step(&format!("{ca}, {cb} et 8 comptes d'appoint entrent en jeu"));
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    let mut b = enter_world(&cfg, ub, pb, cb, "B-").await?;
    Box::pin(leave_old_guild(&mut a)).await?;
    Box::pin(leave_old_guild(&mut b)).await?;
    let mut helpers = Vec::new();
    for (i, user) in helper_accounts(&cfg, 8).await?.iter().enumerate() {
        let mut h = enter_world(&cfg, user, user, &format!("SimG{:02}", i + 1), &format!("G{:02}-", i + 1)).await?;
        Box::pin(leave_old_guild(&mut h)).await?;
        helpers.push(h);
    }

    step(&format!("{ca} crée {GW}, {cb} la rejoint ; tous deux chez Holly"));
    a.world.send(ch, chat_line(a.cid, "/level 20")).await?;
    set_money(&mut a, 20_000).await?;
    a.world.expect_where(|q| q.is(c, Some(1)) && q.int(8) == Some(20_000), 3000, "argent").await?;
    a.world.send(gd, Fields::new(gd).with(0, 0).with(1, a.cid).with(2, GW).with(9, LEAD_PASSWORD)).await?;
    a.world.expect_where(gsys(11), 3000, "création").await?;
    Box::pin(lead_join(&mut a, GW, &mut b, cb)).await?;
    let keeper = go_to_keeper(&mut a).await?;
    go_to_keeper(&mut b).await?;
    println!("  ✓ {GW} : 2 membres ; gardienne {keeper:#x}");

    step("ouverture refusée sous 10 membres");
    warehouse_open(&mut a, keeper).await?;
    a.world.expect_where(gsys(0x5f), 3000, "effectif insuffisant (0x5f)").await?;
    for (i, h) in helpers.iter_mut().enumerate() {
        Box::pin(lead_join(&mut a, GW, h, &format!("SimG{:02}", i + 1))).await?;
    }
    println!("  ✓ code 0x5f ; {GW} : 10 membres");

    step(&format!("{ca} ouvre l'entrepôt, dépose une pile et 1000 pièces"));
    warehouse_open(&mut a, keeper).await?;
    let info = a.world.expect_where(gware(0), 3000, "argent de l'entrepôt (op 0)").await?;
    anyhow::ensure!(info.int(2) == Some(0), "argent {:?}", info.int(2));
    let opened = a.world.expect_where(gware(1), 3000, "ouverture (op 1)").await?;
    anyhow::ensure!(opened.int(8) == Some(2), "résultat {:?}", opened.int(8));
    a.world.send(ch, chat_line(a.cid, &format!("/create \"{ca}\" {POTION} 3"))).await?;
    let made = a.world.expect_where(|q| q.is(it, None) && q.int(3) == Some(POTION) && q.int(1) == Some(2), 3000, "potions créées").await?;
    let iid = made.int(2).unwrap_or(0);
    a.world.send(gw, Fields::new(gw).with(0, 5).with(1, a.cid).with(4, iid).with(5, 0).with(6, 6).with(7, 3)).await?;
    let put = a.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(iid), 3000, "pile dans l'entrepôt").await?;
    let cell = decode_mini(&mini::ITEM_GRID, put.fields().bytes(7).unwrap_or_default())?;
    anyhow::ensure!(put.int(1) == Some(STATUS) && (cell.int(0), cell.int(1), cell.int(2)) == (Some(0), Some(6), Some(3)), "statut {:?}, case {:?}", put.int(1), (cell.int(0), cell.int(1), cell.int(2)));
    a.world.send(gw, Fields::new(gw).with(0, 3).with(1, a.cid).with(2, 1000)).await?;
    let m = a.world.expect_where(|q| q.is(c, Some(1)) && q.int(8).is_some(), 3000, "argent porté").await?;
    // 20 000 pièces, moins 10 000 pour la création et 1000 déposées.
    anyhow::ensure!(m.int(8) == Some(9_000), "argent {:?}", m.int(8));
    let info = a.world.expect_where(gware(0), 3000, "argent de l'entrepôt").await?;
    anyhow::ensure!(info.int(2) == Some(1000), "entrepôt {:?}", info.int(2));
    println!("  ✓ op 0 et op 1 (2) ; pile {iid} au statut 53 en (0, 6, 3) ; 1000 pièces (op 3 → op 0)");

    step(&format!("{cb} ouvre l'entrepôt : droits d'un membre"));
    warehouse_open(&mut b, keeper).await?;
    let seen = b.world.expect_where(|q| q.is(it, Some(0)) && q.int(2) == Some(iid), 3000, "pile vue de B").await?;
    anyhow::ensure!(seen.int(1) == Some(STATUS), "statut {:?}", seen.int(1));
    let info = b.world.expect_where(gware(0), 3000, "argent vu de B").await?;
    anyhow::ensure!(info.int(2) == Some(1000), "entrepôt {:?}", info.int(2));
    b.world.expect_where(gware(1), 3000, "ouverture de B").await?;
    b.world.send(gw, Fields::new(gw).with(0, 4).with(1, b.cid).with(2, 500)).await?;
    anyhow::ensure!(b.world.expect_where(gware(0), 1200, "retrait").await.is_err(), "{cb} a retiré de l'argent");
    b.world.send(gw, Fields::new(gw).with(0, 6).with(1, b.cid).with(3, 2).with(4, iid).with(5, -1).with(6, -1).with(7, -1)).await?;
    let back = b.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(iid), 3000, "pile remise").await?;
    anyhow::ensure!(back.int(1) == Some(STATUS), "statut {:?}", back.int(1));
    a.world.send(ch, chat_line(a.cid, &format!("/money 300 \"{cb}\""))).await?;
    b.world.expect_where(|q| q.is(c, Some(1)) && q.int(8).is_some(), 3000, "argent de B").await?;
    b.world.send(gw, Fields::new(gw).with(0, 3).with(1, b.cid).with(2, 200)).await?;
    a.world.expect_where(|q| gware(0)(q) && q.int(2) == Some(1200), 3000, "dépôt de B vu de A").await?;
    b.world.expect_where(|q| gware(0)(q) && q.int(2) == Some(1200), 3000, "dépôt de B").await?;
    println!("  ✓ {cb} voit la pile ; retrait d'argent et de la pile refusés ; dépôt de 200 pièces (vu des deux)");

    step("dissolution refusée, entrepôt non vide");
    a.world.send(gd, Fields::new(gd).with(0, 6).with(1, a.cid).with(2, GW).with(9, LEAD_PASSWORD)).await?;
    a.world.expect_where(gsys(0x60), 3000, "dissolution refusée (0x60)").await?;
    println!("  ✓ code 0x60");

    step(&format!("{ca} se reconnecte, reprend la pile et l'argent"));
    drop(a);
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    go_to_keeper(&mut a).await?;
    warehouse_open(&mut a, keeper).await?;
    a.world.expect_where(|q| q.is(it, Some(0)) && q.int(2) == Some(iid) && q.int(1) == Some(STATUS), 3000, "pile relue").await?;
    a.world.expect_where(|q| gware(0)(q) && q.int(2) == Some(1200), 3000, "argent relu").await?;
    a.world.expect_where(gware(1), 3000, "ouverture").await?;
    a.world.send(gw, Fields::new(gw).with(0, 6).with(1, a.cid).with(3, 2).with(4, iid).with(5, -1).with(6, -1).with(7, -1)).await?;
    let out = a.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(iid), 3000, "pile reprise").await?;
    anyhow::ensure!(out.int(1) == Some(2), "statut {:?}", out.int(1));
    b.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(iid), 3000, "pile partie, vue de B").await?;
    a.world.send(gw, Fields::new(gw).with(0, 4).with(1, a.cid).with(2, 1200)).await?;
    a.world.expect_where(|q| gware(0)(q) && q.int(2) == Some(0), 3000, "argent repris").await?;
    a.world.send(gw, Fields::new(gw).with(0, 7).with(1, a.cid)).await?;
    let ex = a.world.expect_where(gware(7), 3000, "agrandissement").await?;
    anyhow::ensure!(ex.int(8) == Some(4), "résultat {:?}", ex.int(8));
    println!("  ✓ pile et argent relus ; pile reprise (statut 2, partie chez B) ; argent repris ; agrandissement : 4");

    step("dissolution");
    a.world.send(it, Fields::new(it).with(0, 21).with(2, iid)).await?;
    a.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(iid), 3000, "pile détruite").await?;
    a.world.send(gd, Fields::new(gd).with(0, 6).with(1, a.cid).with(2, GW).with(9, LEAD_PASSWORD)).await?;
    a.world.expect_where(gop(6), 3000, "dissolution").await?;
    drop(helpers);
    println!("\nentrepôt de guilde : OK");
    Ok(())
}
