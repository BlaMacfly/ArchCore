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
/// * `dissoudre` : le chef de SimLead (GmTest ou SimA) la dissout.
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
        other => anyhow::bail!("mode inconnu {other:?} : setup | present | chef | dissoudre"),
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
