//! `archcore sim chat` : préfixes de discussion d'Archonia et cris de zone (AGPMCHATTING 0x0F, AgpmAreaChatting 0x4C).
//!
//! GmTest (MJ) et SimA au nord d'Anchorville. SimA, sans droits MJ, envoie comme Archonia tout en type 0 avec le
//! préfixe dans le texte : « /p » et « /g » sans groupe ni guilde donnent le refus du groupe et de la guilde (pas
//! « Commande inconnue ») ; « /s » arrive chez GmTest en type 1 « [Shout] » ; une commande inconnue n'a aucune réponse.
//! « /race » sans Race Trumpet est refusé ; le MJ lui en donne 2 : le cri arrive chez les deux (AgpmAreaChatting op 0,
//! type 19, texte « [Race Trumpet] [SimA] … »), une trompette est consommée ; un second cri dans la seconde est
//! refusé, le suivant consomme la dernière.

use super::{enter_world, step, InWorld, Recv};
use archcore_net::schema::{self, chat_type};
use archcore_net::{Fields, Vec3};
use std::time::Duration;

/// Nord d'Anchorville, région 0.
const SPOT: Vec3 = Vec3 { x: -466_468.0, y: 4_700.0, z: -50_010.0 };
const TRUMPET: i64 = 3905;

fn say(w: &InWorld, text: &str) -> Fields {
    let ch = &schema::CHATTING;
    Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text))
}

fn chat_with(text: &'static str) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| q.ptype == schema::CHATTING.id && q.fields().text(6).is_some_and(|t| t.contains(text))
}

/// Message système attendu après `text` ; « Commande inconnue » est une erreur.
async fn refused(w: &mut InWorld, text: &str, expect: &'static str) -> anyhow::Result<()> {
    // Un message toutes les 100 ms au plus : le surplus serait ignoré.
    tokio::time::sleep(Duration::from_millis(150)).await;
    w.world.send(&schema::CHATTING, say(w, text)).await?;
    let ch = &schema::CHATTING;
    let r = w.world.expect_where(|q| q.ptype == ch.id && q.int(2) == Some(chat_type::WHOLE_WORLD), 3000, text).await?;
    let got = r.fields().text(6).unwrap_or_default();
    anyhow::ensure!(!got.contains("Commande inconnue") && got.contains(expect), "« {text} » : {got:?}");
    println!("  ✓ {text} → « {got} »");
    Ok(())
}

pub async fn run(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let (mv, ch, ar, it) = (&schema::OPT_CHAR_MOVE, &schema::CHATTING, &schema::AREA_CHATTING, &schema::ITEM);
    let place = |cid: u32| Fields::new(mv).with(0, cid).with(1, SPOT).with(2, SPOT).with(5, 0x04).with(6, 0);

    step(&format!("{ca} (MJ) et {cb} au nord d'Anchorville"));
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    let mut b = enter_world(&cfg, ub, pb, cb, "B-").await?;
    a.world.send(mv, place(a.cid)).await?;
    // B n'a pas de droits MJ : A (MJ) le pose, le serveur refusant qu'il se téléporte lui-même.
    super::gm_place(&mut a, cb, SPOT).await?;
    b.world.send(mv, place(b.cid)).await?;
    tokio::time::sleep(Duration::from_millis(800)).await;

    step(&format!("{cb} : préfixes de groupe et de guilde d'Archonia (type 0)"));
    refused(&mut b, "/p En route", "aucun groupe").await?;
    refused(&mut b, "/g Bonjour la guilde", "aucune guilde").await?;

    step(&format!("{cb} : cri local « /s »"));
    tokio::time::sleep(Duration::from_millis(150)).await;
    b.world.send(ch, say(&b, "/s Ohé tout le monde")).await?;
    let heard = a.world.expect_where(chat_with("Ohé tout le monde"), 3000, "cri local chez A").await?;
    anyhow::ensure!(heard.int(2) == Some(chat_type::WORD_BALLOON) && heard.fields().text(4).as_deref() == Some(cb), "type {:?}, auteur {:?}", heard.int(2), heard.fields().text(4));
    b.world.expect_where(chat_with("Ohé tout le monde"), 3000, "cri local chez B").await?;
    println!("  ✓ type 1 « [Shout] » reçu par {ca} et {cb}, sans le préfixe");

    step(&format!("{cb} : commande inconnue, ignorée sans réponse"));
    b.world.send(ch, say(&b, "/inconnue 12")).await?;
    anyhow::ensure!(b.world.expect_where(|q| q.ptype == ch.id, 1000, "réponse").await.is_err(), "réponse à une commande inconnue");
    println!("  ✓ aucune réponse");

    step(&format!("{cb} : « /race » sans Race Trumpet"));
    refused(&mut b, "/race Pour la race", "Race Trumpet").await?;

    step(&format!("{ca} donne 2 Race Trumpet à {cb}"));
    a.world.send(ch, say(&a, &format!("/create \"{cb}\" {TRUMPET} 2"))).await?;
    let got = b.world.expect_where(|q| q.is(it, Some(0)) && q.int(3) == Some(TRUMPET), 3000, "trompettes reçues").await?;
    let iid = got.int(2).unwrap_or(0);
    println!("  ✓ objet {iid} reçu");

    step(&format!("{cb} : « /race Pour la race »"));
    b.world.send(ch, say(&b, "/race Pour la race")).await?;
    let want = format!("[Race Trumpet] [{cb}] Pour la race");
    let is_cry = |q: &Recv| q.is(ar, Some(0));
    let cry = a.world.expect_where(is_cry, 3000, "cri de race chez A").await?;
    let text = cry.fields().text(6).unwrap_or_default();
    anyhow::ensure!(cry.int(4) == Some(chat_type::RACE) && cry.int(2) == Some(1) && text == want, "type {:?}, genre {:?}, texte {text:?}", cry.int(4), cry.int(2));
    // Chez l'émetteur, la trompette est consommée avant que le cri parte.
    let left = b.world.expect_where(|q| q.is(it, Some(2)) && q.int(2) == Some(iid), 3000, "trompette consommée").await?;
    anyhow::ensure!(left.int(5) == Some(1), "reste {:?}", left.int(5));
    let mine = b.world.expect_where(is_cry, 3000, "cri de race chez B").await?;
    anyhow::ensure!(mine.int(1) == Some(b.cid as i64), "émetteur {:?}", mine.int(1));
    println!("  ✓ AgpmAreaChatting type 19 « {text} » reçu par les deux ; 1 trompette restante");

    step(&format!("{cb} : second cri dans la seconde, refusé ; le suivant consomme la dernière"));
    refused(&mut b, "/race Encore", "Attendez").await?;
    tokio::time::sleep(Duration::from_millis(1100)).await;
    b.world.send(ch, say(&b, "/race Dernier appel")).await?;
    b.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(iid), 3000, "trompette retirée").await?;
    a.world.expect_where(|q| q.is(ar, Some(0)) && q.fields().text(6).is_some_and(|t| t.ends_with("Dernier appel")), 3000, "second cri chez A").await?;
    println!("  ✓ cri reçu, trompette retirée de l'inventaire");

    println!("\ndiscussion : OK");
    Ok(())
}
