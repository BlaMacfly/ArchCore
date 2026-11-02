//! `archcore sim quickbelt` : barre de raccourcis gardée par le serveur (`world/ui_status.rs`) ; `archcore sim returnlogin` :
//! retour à la sélection des personnages (`world/return_login.rs`, `login.rs`).
//!
//! Barre, GmTest (MJ) : op 0 à l'entrée (50 minis), op 1 reçues sans réponse : compétence en case 12 puis en case 15 (retirée
//! de la 12, même rangée), potion (190) en case 13, objet inconnu en case 49 (vidé), potion de vie au champ 6 ; à la
//! reconnexion, la compétence revient en case 15 avec son nouvel identifiant, la potion en case 13 ; potion disparue
//! (IID changé en base) : remplacée par la pile de même TID ; case vidée (op 1 sans mini) : vide à l'entrée suivante.
//! Retour, GmTest : 0x2F op 1 → op 5 (clé), connexion au monde fermée ; au serveur de connexion, op 6 (compte, clé) →
//! clé de hachage, connexion avec les identifiants gardés → AGPMLOGIN op 1 puis 0x2F op 7, liste des personnages ;
//! seconde op 6 avec la même clé refusée (AGPMLOGIN op 15) ; nouvelle entrée en jeu.

use super::{enter_world, step, InWorld, TestClient};
use archcore_net::schema::{self, chat_type, login_op, mini};
use archcore_net::{build_mini, decode_mini, Fields, Value};
use std::time::Duration;

const POTION: i64 = 190;

fn base(kind: i64, id: i64) -> Vec<u8> {
    let s = &mini::SKILL_BASE;
    build_mini(s, &Fields::new(s).with(0, kind).with(1, id))
}

/// Cases de l'op 0 : (type, identifiant) ou `None`.
async fn belt_at_entry(w: &mut InWorld) -> anyhow::Result<(Vec<Option<(i64, i64)>>, Option<i64>)> {
    let u = &schema::UI_STATUS;
    let add = w.world.expect_where(|q| q.is(u, Some(0)), 5000, "AgpmUIStatus op 0").await?;
    let list = add.fields().list(2).map(|l| l.to_vec()).unwrap_or_default();
    anyhow::ensure!(list.len() == 50, "{} cases au lieu de 50", list.len());
    let cells = list
        .iter()
        .map(|v| match v {
            Value::Bytes(b) => decode_mini(&mini::SKILL_BASE, b).ok().and_then(|m| m.int(0).zip(m.int(1))),
            _ => None,
        })
        .collect();
    Ok((cells, add.int(6)))
}

async fn set_slot(w: &mut InWorld, index: i64, content: Option<(i64, i64)>) -> anyhow::Result<()> {
    let u = &schema::UI_STATUS;
    let mut f = Fields::new(u).with(0, 1).with(1, w.cid).with(3, index);
    if let Some((k, id)) = content {
        f = f.with(4, base(k, id));
    }
    w.world.send(u, f).await
}

pub async fn run(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let db = crate::db::Databases::new(&cfg.database);
    let id = super::character_id(&db, char_name).await?;
    db.characters.exec("DELETE FROM character_quickbelt WHERE character_id = ?", (id,)).await?;
    let result = Box::pin(scenario(&cfg, &db, id, user, pass, char_name)).await;
    db.characters.exec("DELETE FROM character_quickbelt WHERE character_id = ?", (id,)).await?;
    db.close().await;
    result
}

async fn scenario(cfg: &crate::config::Config, db: &crate::db::Databases, id: u32, user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    let (u, it, ch) = (&schema::UI_STATUS, &schema::ITEM, &schema::CHATTING);
    step(&format!("{char_name} entre en jeu : barre vide"));
    let mut w = enter_world(cfg, user, pass, char_name, "").await?;
    let (cells, _) = belt_at_entry(&mut w).await?;
    anyhow::ensure!(cells.iter().all(Option::is_none), "barre non vide : {cells:?}");
    println!("  ✓ op 0 : 50 cases vides");
    let (skill_id, skill_tid) = w.skills.first().map(|s| (s.0, s.1)).ok_or_else(|| anyhow::anyhow!("aucune compétence"))?;
    tokio::time::sleep(Duration::from_millis(150)).await;
    w.world.send(ch, Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(&format!("/additem {POTION} 3")))).await?;
    let potion = w.world.expect_where(|q| q.is(it, None) && q.int(3) == Some(POTION) && q.int(1) == Some(2), 3000, "potion ajoutée").await?.int(2).unwrap_or(0);

    step("op 1 : compétence, potion, objet inconnu, potion de vie");
    set_slot(&mut w, 12, Some((7, skill_id))).await?;
    set_slot(&mut w, 15, Some((7, skill_id))).await?;
    set_slot(&mut w, 13, Some((5, potion))).await?;
    set_slot(&mut w, 49, Some((5, 999_999))).await?;
    set_slot(&mut w, 7, Some((7, skill_id))).await?;
    w.world.send(u, Fields::new(u).with(0, 1).with(1, w.cid).with(6, POTION)).await?;
    anyhow::ensure!(w.world.expect_where(|q| q.is(u, None), 800, "réponse").await.is_err(), "le serveur a répondu à l'op 1");
    let rows: Vec<(u32, u32, u32)> = db
        .characters
        .rows("SELECT slot, base_type, tid FROM character_quickbelt WHERE character_id = ? ORDER BY slot", (id,))
        .await?
        .iter()
        .map(|r| (crate::db::col(r, "slot"), crate::db::col(r, "base_type"), crate::db::col(r, "tid")))
        .collect();
    let want = vec![(7, 7, skill_tid as u32), (13, 5, POTION as u32), (15, 7, skill_tid as u32)];
    anyhow::ensure!(rows == want, "barre en base {rows:?}, attendu {want:?}");
    println!("  ✓ aucune réponse ; en base : {rows:?} (case 12 libérée par la 15, case 49 inconnue vidée)");

    step("reconnexion : compétence par son nouvel identifiant, potion");
    drop(w);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut w = enter_world(cfg, user, pass, char_name, "").await?;
    let new_id = w.skills.iter().find(|s| s.1 == skill_tid).map(|s| s.0).unwrap_or(0);
    let (cells, hp_potion) = belt_at_entry(&mut w).await?;
    anyhow::ensure!(cells[15] == Some((7, new_id)) && cells[7] == Some((7, new_id)), "compétence : {:?} / {:?} (identifiant {new_id})", cells[15], cells[7]);
    anyhow::ensure!(cells[13] == Some((5, potion)) && cells[12].is_none() && cells[49].is_none(), "cases 12, 13, 49 : {:?} {:?} {:?}", cells[12], cells[13], cells[49]);
    anyhow::ensure!(hp_potion == Some(POTION), "potion de vie {hp_potion:?}");
    println!("  ✓ case 15 = compétence {new_id} (TID {skill_tid}), case 13 = potion {potion}, champ 6 = {POTION}");

    step("potion disparue : remplacée par la pile de même TID ; case vidée");
    set_slot(&mut w, 7, None).await?;
    tokio::time::sleep(Duration::from_millis(300)).await;
    drop(w);
    db.characters.exec("UPDATE character_quickbelt SET item_id = 4000000 WHERE character_id = ? AND slot = 13", (id,)).await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut w = enter_world(cfg, user, pass, char_name, "").await?;
    let (cells, _) = belt_at_entry(&mut w).await?;
    anyhow::ensure!(cells[13] == Some((5, potion)) && cells[7].is_none(), "cases 13 et 7 : {:?} {:?}", cells[13], cells[7]);
    println!("  ✓ case 13 = potion {potion} (remplacement), case 7 vide");
    w.world.send(u, Fields::new(u).with(0, 1).with(1, w.cid).with(6, 0)).await?;
    println!("\nbarre de raccourcis : OK");
    Ok(())
}

pub async fn return_login(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let (r, lg) = (&schema::RETURN_TO_LOGIN, &schema::LOGIN);
    step(&format!("{char_name} demande la sélection des personnages"));
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    w.world.send(r, Fields::new(r).with(0, 1).with(1, w.cid)).await?;
    let sent = w.world.expect_where(|q| q.is(r, Some(5)), 3000, "clé (op 5)").await?;
    let key = sent.int(5).unwrap_or(0);
    anyhow::ensure!(sent.int(1) == Some(w.cid as i64) && key > 0, "op 5 : CID {:?}, clé {key}", sent.int(1));
    anyhow::ensure!(w.world.closed_within(5000).await, "connexion au monde gardée");
    println!("  ✓ op 5 (clé {key}, adresse {:?}), connexion au monde fermée", sent.fields().text(4));

    step("serveur de connexion : op 6, clé de hachage, connexion, op 7");
    let addr = format!("127.0.0.1:{}", cfg.login.port);
    let mut login = TestClient::connect("retour-", &addr).await?;
    login.handshake().await?;
    login.send(r, Fields::new(r).with(0, 6).with(2, user).with(5, key)).await?;
    let hash = login.expect(lg, Some(login_op::HASHKEY)).await?.fields().text(1).unwrap_or_default();
    let enc_user = archcore_net::rc4::capi_crypt(hash.as_bytes(), &crate::tools::to_cp1252(user));
    let enc_pass = archcore_net::rc4::capi_crypt(hash.as_bytes(), &crate::tools::to_cp1252(pass));
    let (ul, pl) = (enc_user.len(), enc_pass.len());
    login.send(lg, Fields::new(lg).with(0, login_op::USER_LOGIN).with(2, enc_user).with(3, ul).with(4, enc_pass).with(5, pl)).await?;
    login.expect(lg, Some(login_op::USER_LOGIN)).await?;
    login.expect(r, Some(7)).await?;
    println!("  ✓ clé de hachage, connexion (op 1), fin du retour (op 7)");
    let ws = &schema::WORLD;
    login.send(ws, Fields::new(ws).with(0, schema::world_op::SERVER_LIST_REQ)).await?;
    let world_name = login.expect(ws, Some(schema::world_op::SERVER_LIST_ANS)).await?.fields().text(3).unwrap_or_default().split('=').next().unwrap_or("").to_string();
    login.send(lg, Fields::new(lg).with(0, login_op::UNION_INFO).with(2, user).with(7, world_name.as_str())).await?;
    let mut names = Vec::new();
    loop {
        let p = login.expect_where(|q| q.is(lg, Some(login_op::CHAR_NAME)) || q.is(lg, Some(login_op::CHAR_NAME_FINISH)), 5000, "noms des personnages").await?;
        if p.int(0) == Some(login_op::CHAR_NAME_FINISH) {
            break;
        }
        names.push(decode_mini(&mini::LOGIN_CHAR_INFO, p.fields().bytes(8).unwrap_or_default())?.text(1).unwrap_or_default());
    }
    anyhow::ensure!(names.iter().any(|n| n == char_name), "personnages {names:?}");
    println!("  ✓ personnages : {}", names.join(", "));
    drop(login);

    step("même clé une seconde fois : refusée");
    let mut again = TestClient::connect("retour2-", &addr).await?;
    again.handshake().await?;
    again.send(r, Fields::new(r).with(0, 6).with(2, user).with(5, key)).await?;
    let res = again.expect(lg, Some(login_op::LOGIN_RESULT)).await?;
    println!("  ✓ AGPMLOGIN op 15 (résultat {:?})", res.int(10));
    drop(again);

    step("nouvelle entrée en jeu");
    let w = enter_world(&cfg, user, pass, char_name, "").await?;
    println!("  ✓ {char_name} de nouveau en jeu (CID {})", w.cid);
    println!("\nretour à la sélection : OK");
    Ok(())
}
