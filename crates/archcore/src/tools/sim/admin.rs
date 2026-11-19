//! `archcore sim admin` : outil MJ de Webzen (AgpmAdmin 0x12), console de développement (AgpmScript 0x45, AgpmCasper
//! 0x19), GmTest (MJ) + SimA (sans droits).
//!
//! SimA envoie une recherche, une ligne de console et un KillGameServer : rien ne lui revient. GmTest : ouverture de
//! l'outil (op 2 → op 0), recherche par nom et par compte (op 4, une par personnage), personnage inconnu (message),
//! fiche (op 10 : or de l'inventaire → `/money` ; code HP refusé), création d'objet (op 12 sous-op 0 → `/create`),
//! ligne de console « GetExp » (op 0 → op 2), KillGameServer (op 3 → redémarrage annoncé, annulé par
//! `/announce_restart 0`), enfin déconnexion de SimA (op 11 → `/disconnect`).

use super::{enter_world, step, InWorld, Recv};
use archcore_net::schema::{self, chat_type, mini};
use archcore_net::{build_mini, decode_mini, Fields};
use std::time::Duration;

const POTION: i64 = 190;

fn say(w: &InWorld, text: &str) -> Fields {
    let ch = &schema::CHATTING;
    Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text))
}

/// Message système qui contient `text`.
fn system(text: &'static str) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| q.ptype == schema::CHATTING.id && q.int(2) == Some(chat_type::WHOLE_WORLD) && q.fields().text(6).is_some_and(|t| t.contains(text))
}

fn search(w: &InWorld, crit: i64, text: &str) -> Fields {
    let m = &mini::ADMIN_SEARCH;
    let a = &schema::ADMIN;
    Fields::new(a).with(0, 3).with(1, w.cid).with(3, build_mini(m, &Fields::new(m).with(0, 0).with(1, crit).with(2, 0).with(3, text)))
}

/// Bloc de `len` octets avec des valeurs posées aux offsets donnés.
fn block(len: usize, put: &[(usize, &[u8])]) -> Vec<u8> {
    let mut b = vec![0u8; len];
    for (at, v) in put {
        b[*at..*at + v.len()].copy_from_slice(v);
    }
    b
}

/// Fiche du personnage (op 10, champ 5).
fn char_edit(w: &InWorld, name: &str, code: u16, value64: i64) -> Fields {
    let b = block(0x152, &[(0, &1i32.to_le_bytes()), (4, name.as_bytes()), (0x35, &code.to_le_bytes()), (0x40, &value64.to_le_bytes())]);
    let m = &mini::ADMIN_BLOCK2;
    let a = &schema::ADMIN;
    Fields::new(a).with(0, 10).with(1, w.cid).with(5, build_mini(m, &Fields::new(m).with(0, b)))
}

/// Ligne de la console de développement (AgpmScript op 0).
fn console(w: &InWorld, text: &str) -> Fields {
    let s = &schema::SCRIPT;
    Fields::new(s).with(0, 0).with(1, w.cid).with(2, text)
}

/// Vérifie qu'aucun paquet satisfaisant `pred` n'arrive sous `ms`.
async fn nothing(w: &mut InWorld, pred: impl Fn(&Recv) -> bool, ms: u64, what: &str) -> anyhow::Result<()> {
    if w.world.expect_where(pred, ms, what).await.is_ok() {
        anyhow::bail!("réponse inattendue : {what}");
    }
    Ok(())
}

pub async fn run(user: &str, pass: &str, char_name: &str, user_b: &str, pass_b: &str, char_b: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let (a, sc, ca) = (&schema::ADMIN, &schema::SCRIPT, &schema::CASPER);

    step(&format!("{char_name} (MJ) et {char_b} entrent en jeu"));
    let mut gm = enter_world(&cfg, user, pass, char_name, "MJ").await?;
    let mut b = enter_world(&cfg, user_b, pass_b, char_b, "B").await?;

    step(&format!("{char_b} sans droits : recherche, console et KillGameServer ignorés"));
    b.world.send(a, search(&b, 0, char_name)).await?;
    b.world.send(sc, console(&b, "GetExp")).await?;
    b.world.send(ca, Fields::new(ca).with(0, 3)).await?;
    nothing(&mut b, |q| q.is(a, Some(4)) || q.ptype == sc.id || system("Redémarrage")(q), 1500, "réponse à un joueur sans droits").await?;
    println!("  ✓ aucune réponse");

    step("ouverture de l'outil MJ (op 2) : op 0 avec le niveau MJ");
    gm.world.send(a, Fields::new(a).with(0, 2).with(1, gm.cid)).await?;
    let r = gm.world.expect_where(|q| q.is(a, Some(0)), 3000, "op 0").await?;
    let info = decode_mini(&mini::ADMIN_CHAR, r.fields().bytes(2).unwrap_or_default())?;
    println!("  ✓ {} : niveau MJ {}", info.text(2).unwrap_or_default(), info.int(3).unwrap_or(0));

    for (crit, text, label) in [(0, char_b.to_string(), "nom"), (1, user_b.to_string(), "compte")] {
        step(&format!("recherche par {label} « {text} » (op 3 → op 4)"));
        gm.world.send(a, search(&gm, crit, &text)).await?;
        let r = gm.world.expect_where(|q| q.is(a, Some(4)), 3000, "op 4").await?;
        let m = decode_mini(&mini::ADMIN_SEARCH, r.fields().bytes(3).unwrap_or_default())?;
        let f = decode_mini(&mini::ADMIN_SEARCH_RESULT, m.bytes(4).unwrap_or_default())?;
        let (name, account, online) = (f.text(2).unwrap_or_default(), f.text(4).unwrap_or_default(), f.int(11));
        anyhow::ensure!(name.eq_ignore_ascii_case(char_b) && account.eq_ignore_ascii_case(user_b) && online == Some(1), "résultat : {name} / {account} / {online:?}");
        anyhow::ensure!(m.int(1) == Some(crit), "critère renvoyé : {:?}", m.int(1));
        println!("  ✓ {name} (compte {account}, CID {}, TID {}, niveau {}, en ligne)", f.int(3).unwrap_or(0), f.int(6).unwrap_or(0), f.int(7).unwrap_or(0));
    }

    step("recherche d'un personnage inconnu : message système");
    gm.world.send(a, search(&gm, 0, "PersonneDeCeNom")).await?;
    gm.world.expect_where(system("Aucun personnage"), 3000, "aucun personnage").await?;
    println!("  ✓ aucun personnage");

    step("fiche (op 10) : or de l'inventaire → /money ; HP non pris en charge");
    gm.world.send(a, char_edit(&gm, char_name, 5, 123_456)).await?;
    let r = gm.world.expect_where(system("123456 pièce(s) d'or"), 3000, "/money").await?;
    println!("  ✓ {}", r.fields().text(6).unwrap_or_default());
    gm.world.send(a, char_edit(&gm, char_name, 0, 1)).await?;
    gm.world.expect_where(system("non pris en charge"), 3000, "code HP refusé").await?;
    println!("  ✓ HP refusé");

    step("création d'objet (op 12, sous-op 0) → /create");
    let item = block(0x21c, &[(0, char_name.as_bytes()), (0x62, &[0]), (0x70, &(POTION as i32).to_le_bytes()), (0x75, &2i32.to_le_bytes())]);
    let m = &mini::ADMIN_BLOCK;
    gm.world.send(a, Fields::new(a).with(0, 12).with(1, gm.cid).with(12, build_mini(m, &Fields::new(m).with(0, item)))).await?;
    let (made, full) = (system("(TID 190)"), system("inventaire plein"));
    let r = gm.world.expect_where(|q| made(q) || full(q), 3000, "/create").await?;
    println!("  ✓ {}", r.fields().text(6).unwrap_or_default());

    step("console de développement : « GetExp » (AgpmScript op 0 → op 2)");
    tokio::time::sleep(Duration::from_millis(150)).await;
    gm.world.send(sc, console(&gm, "GetExp")).await?;
    let r = gm.world.expect_where(|q| q.is(sc, Some(2)), 3000, "AgpmScript op 2").await?;
    let text = r.fields().text(2).unwrap_or_default();
    anyhow::ensure!(text.contains("expérience"), "réponse : {text}");
    println!("  ✓ {text}");
    gm.world.send(sc, console(&gm, "PasUneCommande")).await?;
    let r = gm.world.expect_where(|q| q.is(sc, Some(2)), 3000, "AgpmScript op 2").await?;
    println!("  ✓ {}", r.fields().text(2).unwrap_or_default());

    step("KillGameServer (AgpmCasper op 3) : redémarrage annoncé, puis annulé");
    gm.world.send(ca, Fields::new(ca).with(0, 3)).await?;
    let r = gm.world.expect_where(system("Redémarrage annoncé"), 3000, "redémarrage annoncé").await?;
    println!("  ✓ {}", r.fields().text(6).unwrap_or_default());
    gm.world.send(&schema::CHATTING, say(&gm, "/announce_restart 0")).await?;
    gm.world.expect_where(system("Redémarrage annulé"), 3000, "redémarrage annulé").await?;
    println!("  ✓ annulé");

    step(&format!("déconnexion de {char_b} (op 11 → /disconnect)"));
    let m = &mini::ADMIN_SANCTION;
    let sanction = Fields::new(m).with(0, char_b).with(1, user_b).with(2, 1);
    gm.world.send(a, Fields::new(a).with(0, 11).with(1, gm.cid).with(10, build_mini(m, &sanction))).await?;
    gm.world.expect_where(system("a été déconnecté"), 3000, "/disconnect").await?;
    anyhow::ensure!(b.world.closed_within(3000).await, "{char_b} toujours connecté");
    println!("  ✓ {char_b} déconnecté");
    println!("\noutil MJ, console de développement et KillGameServer : OK");
    Ok(())
}
