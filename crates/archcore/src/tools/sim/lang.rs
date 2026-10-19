//! `archcore sim lang` : capacité `language/1` des extensions ArchRust.
//!
//! Le même personnage entre en jeu en français puis en anglais (langue annoncée au `HELLO`) : les noms des
//! PNJ reçus changent. En jeu, `TEXT_REQUEST` rend les noms des PNJ dans la langue courante, `SET_LANGUAGE`
//! la change (réponse `LANGUAGE_ACK`), une langue inconnue retombe sur `World.Language`, une demande mal
//! formée vaut `INVALID_REQUEST`. Enfin un client d'origine (sans `HELLO`) ne reçoit aucun paquet 0xAC et
//! obtient la dernière langue enregistrée sur le compte.

use super::{enter_world_opts, o, step, InWorld, Recv, TestClient};
use crate::config::Config;
use std::future::Future;
use std::pin::Pin;
use crate::world::npcs::NPC_CID_BASE;
use archcore_net::ext_text::{decode_texts, encode_ids, normalize_language};
use archcore_net::schema::{self, ext_op, ext_result, ext_text_kind};
use archcore_net::Fields;
use std::collections::BTreeMap;
use std::time::Duration;

fn names(npcs: &[(i64, String)]) -> BTreeMap<i64, String> {
    npcs.iter().cloned().collect()
}

/// Entrée en jeu derrière une fonction ordinaire, jamais en ligne : le futur d'`enter_world_opts` est gros,
/// et le construire à chaque appel dans `run` ferait déborder la pile du fil principal.
#[inline(never)]
fn enter<'a>(
    cfg: &'a Config,
    user: &'a str,
    pass: &'a str,
    char_name: &'a str,
    label: &'a str,
    ext: bool,
    language: Option<&'a str>,
) -> Pin<Box<dyn Future<Output = anyhow::Result<InWorld>> + 'a>> {
    Box::pin(enter_world_opts(cfg, user, pass, char_name, label, ext, language))
}

fn hello_language(w: &TestClient) -> anyhow::Result<(String, String, String)> {
    let f = w.hello_ack.clone().ok_or_else(|| anyhow::anyhow!("pas de HELLO_ACK"))?;
    Ok((f.text(5).unwrap_or_default(), f.text(16).unwrap_or_default(), f.text(17).unwrap_or_default()))
}

async fn set_language(w: &mut TestClient, request_id: i64, lang: &str) -> anyhow::Result<Recv> {
    let s = &schema::ARCHCORE_EXT;
    w.send(s, Fields::new(s).with(0, ext_op::SET_LANGUAGE).with(1, request_id).with(16, lang.as_bytes().to_vec())).await?;
    w.expect_where(|p| p.ptype == s.id && p.int(1) == Some(request_id), 3000, &format!("réponse à SET_LANGUAGE {lang}")).await
}

async fn request_texts(w: &mut TestClient, request_id: i64, kind: &str, ids: &[u32]) -> anyhow::Result<Recv> {
    let s = &schema::ARCHCORE_EXT;
    w.send(s, Fields::new(s).with(0, ext_op::TEXT_REQUEST).with(1, request_id).with(18, kind.as_bytes().to_vec()).with(19, encode_ids(ids))).await?;
    w.expect_where(|p| p.ptype == s.id && p.int(1) == Some(request_id), 3000, &format!("réponse à TEXT_REQUEST {kind}")).await
}

fn texts_of(r: &Recv) -> anyhow::Result<Vec<(u32, String)>> {
    anyhow::ensure!(r.int(0) == Some(ext_op::TEXT_REPLY), "TEXT_REPLY attendu, op {} reçu (résultat {})", o(r.int(0)), o(r.int(15)));
    Ok(decode_texts(r.fields().bytes(20).unwrap_or_default()))
}

pub async fn run(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let default_lang = normalize_language(&cfg.world.language).unwrap_or_else(|| "en".into());

    step(&format!("{char_name} entre en jeu en français (HELLO avec langue fr)"));
    let fr = enter(&cfg, user, pass, char_name, "FR-", true, Some("fr")).await?;
    let (caps, applied, available) = hello_language(&fr.world)?;
    anyhow::ensure!(caps.split(';').any(|c| c == "language/1"), "capacité language/1 refusée : « {caps} »");
    anyhow::ensure!(applied == "fr", "langue appliquée {applied:?} au lieu de fr");
    anyhow::ensure!(available.split(';').any(|l| l == "fr") && available.split(';').any(|l| l == "en"), "langues proposées {available:?}");
    anyhow::ensure!(!fr.npcs.is_empty(), "aucun PNJ reçu");
    let fr_names = names(&fr.npcs);
    println!("  ✓ language/1 accordée, fr appliqué, {} PNJ ; langues proposées : {available}", fr_names.len());
    drop(fr);
    tokio::time::sleep(Duration::from_millis(400)).await;

    step(&format!("{char_name} revient en anglais"));
    let mut en = enter(&cfg, user, pass, char_name, "EN-", true, Some("en")).await?;
    let (_, applied, _) = hello_language(&en.world)?;
    anyhow::ensure!(applied == "en", "langue appliquée {applied:?} au lieu de en");
    let en_names = names(&en.npcs);
    let changed: Vec<(&i64, &String)> = en_names.iter().filter(|(cid, n)| fr_names.get(cid).is_some_and(|f| f != *n)).collect();
    anyhow::ensure!(!changed.is_empty(), "aucun nom de PNJ ne change entre fr et en");
    for (cid, n) in changed.iter().take(4) {
        println!("  PNJ {cid} : « {} » (fr) → « {n} » (en)", fr_names[cid]);
    }
    println!("  ✓ {} nom(s) de PNJ sur {} diffèrent entre les deux langues", changed.len(), en_names.len());

    step("TEXT_REQUEST : noms de PNJ dans la langue courante, puis SET_LANGUAGE fr");
    let ids: Vec<u32> = changed.iter().take(3).map(|(cid, _)| (**cid as u32).wrapping_sub(NPC_CID_BASE)).collect();
    let r = request_texts(&mut en.world, 1, ext_text_kind::NPC_NAME, &ids).await?;
    anyhow::ensure!(r.fields().text(16).as_deref() == Some("en"), "langue servie {:?}", r.fields().text(16));
    let got = texts_of(&r)?;
    anyhow::ensure!(got.len() == ids.len(), "{} texte(s) au lieu de {}", got.len(), ids.len());
    for (id, text) in &got {
        let cid = (NPC_CID_BASE + id) as i64;
        anyhow::ensure!(en_names.get(&cid) == Some(text), "PNJ {id} : « {text} » au lieu de « {} »", en_names[&cid]);
    }
    println!("  ✓ {} nom(s) en anglais, identiques aux paquets de vue", got.len());
    let ack = set_language(&mut en.world, 2, "fr").await?;
    anyhow::ensure!(ack.int(0) == Some(ext_op::LANGUAGE_ACK) && ack.fields().text(16).as_deref() == Some("fr"), "LANGUAGE_ACK fr attendu");
    let got = texts_of(&request_texts(&mut en.world, 3, ext_text_kind::NPC_NAME, &ids).await?)?;
    for (id, text) in &got {
        let cid = (NPC_CID_BASE + id) as i64;
        anyhow::ensure!(fr_names.get(&cid) == Some(text), "PNJ {id} : « {text} » au lieu de « {} »", fr_names[&cid]);
    }
    println!("  ✓ après SET_LANGUAGE fr, les mêmes PNJ reviennent en français");

    step("repli et refus");
    let ack = set_language(&mut en.world, 4, "xx-YY").await?;
    anyhow::ensure!(ack.fields().text(16).as_deref() == Some(default_lang.as_str()), "langue inconnue : {:?} appliqué au lieu de {default_lang}", ack.fields().text(16));
    println!("  ✓ langue inconnue xx-YY : le serveur applique {default_lang} (World.Language)");
    let r = set_language(&mut en.world, 5, "n'importe quoi").await?;
    anyhow::ensure!(r.int(0) == Some(ext_op::RESULT) && r.int(15) == Some(ext_result::INVALID_REQUEST), "code de langue invalide : résultat {}", o(r.int(15)));
    let r = request_texts(&mut en.world, 6, "bidule", &ids).await?;
    anyhow::ensure!(r.int(0) == Some(ext_op::RESULT) && r.int(15) == Some(ext_result::INVALID_REQUEST), "sorte inconnue : résultat {}", o(r.int(15)));
    println!("  ✓ code mal formé et sorte inconnue : INVALID_REQUEST");
    let r = request_texts(&mut en.world, 7, ext_text_kind::QUEST_NAME, &[1, 2, 3]).await?;
    let quests = texts_of(&r)?;
    println!("  quêtes 1 à 3 en {} : {:?}", r.fields().text(16).unwrap_or_default(), quests.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>());
    anyhow::ensure!(quests.len() == 3, "3 textes attendus");
    let ack = set_language(&mut en.world, 8, "en").await?;
    anyhow::ensure!(ack.fields().text(16).as_deref() == Some("en"));
    drop(en);
    tokio::time::sleep(Duration::from_millis(400)).await;

    step(&format!("{char_name} avec ArchRust en « automatique » (HELLO sans langue) : langue du compte"));
    let auto = enter(&cfg, user, pass, char_name, "A-", true, None).await?;
    let (_, applied, _) = hello_language(&auto.world)?;
    anyhow::ensure!(applied == default_lang, "HELLO_ACK sans souhait : {applied:?} au lieu de {default_lang}");
    anyhow::ensure!(names(&auto.npcs) == en_names, "la langue du compte (en) devrait s'appliquer");
    anyhow::ensure!(auto.language_ack.as_deref() == Some("en"), "LANGUAGE_ACK d'entrée {:?} au lieu de en", auto.language_ack);
    println!("  ✓ HELLO_ACK annonce {default_lang}, puis LANGUAGE_ACK en à l'entrée : noms en anglais");
    drop(auto);
    tokio::time::sleep(Duration::from_millis(400)).await;

    step(&format!("{char_name} avec le client d'origine (sans HELLO)"));
    let orig = enter(&cfg, user, pass, char_name, "O-", false, None).await?;
    let _ = orig.world.ext_seen; // compté pendant l'entrée
    anyhow::ensure!(orig.world.ext_seen == 0, "{} paquet(s) 0xAC envoyés au client d'origine", orig.world.ext_seen);
    let orig_names = names(&orig.npcs);
    anyhow::ensure!(orig_names == en_names, "le client d'origine devrait recevoir la langue du compte (en)");
    println!("  ✓ aucun paquet 0xAC ; noms dans la dernière langue du compte (en){}", if default_lang == "en" { " — World.Language vaut aussi en, test moins discriminant" } else { "" });
    drop(orig);
    tokio::time::sleep(Duration::from_millis(400)).await;

    step("retour du compte en français");
    let back = enter(&cfg, user, pass, char_name, "R-", true, Some("fr")).await?;
    anyhow::ensure!(names(&back.npcs) == fr_names, "noms français attendus au retour");
    println!("  ✓ le compte est de nouveau en fr");
    println!("\nlangues : OK");
    Ok(())
}
