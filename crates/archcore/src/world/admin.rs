//! Outil MJ du client de Webzen (AgpmAdmin, 0x12, client → serveur).
//!
//! Le client de Webzen charge `AlefAdminDLL.dll` (« ArchLord Admin Client », fenêtres MFC) pour un compte MJ ; la DLL
//! construit elle-même ses paquets AgpmAdmin sur le module du client. Archonia n'a pas cet outil : son panneau
//! « Admin Commands » tape des commandes de discussion, et ArchRust n'a donc aucune fenêtre à reproduire. Ops que la
//! DLL envoie (le reste de l'outil passe par la discussion) :
//! - **op 2** à l'ouverture (CID, ADMIN_CHAR vide) : le serveur renvoie l'op 0 (niveau MJ, `chat::admin_level_packet`),
//!   déjà envoyée à l'entrée ;
//! - **op 3** recherche (champ 3 : genre, critère 0 nom / 1 compte, texte) → une **op 4** par personnage trouvé
//!   (champ 3, avec le résultat `ADMIN_SEARCH_RESULT` dans son sous-paquet) ;
//! - **op 10** fiche du personnage (champ 5, bloc de 0x152 octets : +0 genre, +4 nom, +0x35 code, +0x38 valeur INT32,
//!   +0x40 valeur INT64, +0x14a écart d'expérience INT64) ;
//! - **op 11** sanction (champ 10) : la DLL ne remplit que la déconnexion (+0x62 = 1) ;
//! - **op 12** objets (champ 12, bloc de 0x21c octets : +0 nom, +0x62 sous-op, +0x70 TID, +0x75 nombre ; sous-op 0
//!   création, 2 sockets, 3 suppression, 5 option, 6 attributs) ;
//! - **op 14** guildes (bloc 0x152, sous-ops 1 à 7), **op 19** quêtes de titre (bloc 0x83) : non pris en charge.
//!
//! Choix d'ArchCore : chaque op devient la commande de discussion équivalente de `gm.rs` (`gm::execute_quiet`) :
//! mêmes niveaux MJ (`World.GmCommandLevels` compris), même journal, réponse en message système. Seule la recherche a
//! sa propre réponse (op 4). Un joueur sans droits est ignoré, comme une commande MJ qu'il taperait. L'op 15
//! (compensation à la sélection des personnages, AgcmUILogin) n'est jamais proposée par ArchCore : ignorée.

use super::gm;
use super::{send_system, World, WorldState};
use crate::db::col;
use crate::{debug, info};
use archcore_net::schema::{self, mini};
use archcore_net::{build_mini, build_packet, decode_mini, Fields, Packet};
use std::sync::Arc;

pub mod op {
    /// Ouverture de l'outil MJ.
    pub const CONNECT: i64 = 2;
    /// Recherche de personnage (client → serveur).
    pub const SEARCH: i64 = 3;
    /// Personnage trouvé (serveur → client).
    pub const SEARCH_RESULT: i64 = 4;
    /// Fiche du personnage.
    pub const CHAR_EDIT: i64 = 10;
    /// Sanction (déconnexion).
    pub const SANCTION: i64 = 11;
    /// Opération sur un objet.
    pub const ITEM: i64 = 12;
    /// Guildes.
    pub const GUILD: i64 = 14;
    /// Compensation à la connexion (sélection des personnages).
    pub const COMPENSATION: i64 = 15;
    /// Quêtes de titre.
    pub const TITLE: i64 = 19;
}

/// Critères de recherche (champ 3, 2ᵉ octet).
pub mod criterion {
    pub const NAME: i64 = 0;
    pub const ACCOUNT: i64 = 1;
}

/// Codes de la fiche (op 10, +0x35), relevés avec les libellés de la DLL : 0 HP, 1 MP, 3 niveau, 4 expérience, 5 or
/// de l'inventaire, 6 or de la banque, 0xf AFM, 0x10 points de méchant, 0x11 et 0x12 leurs durées, 0x17 charisme.
/// ArchCore traite le niveau, l'expérience et l'or de l'inventaire.
pub mod edit_code {
    /// Relevé, non traité (les tests s'en servent pour un code refusé).
    #[cfg(test)]
    pub const HP: u16 = 0;
    pub const LEVEL: u16 = 3;
    pub const EXP: u16 = 4;
    pub const MONEY: u16 = 5;
}

/// Sous-op de création d'un objet (op 12, +0x62).
pub const ITEM_CREATE: u8 = 0;

/// Personnages rendus au plus par une recherche.
pub const SEARCH_LIMIT: usize = 50;

/// Personnage trouvé par une recherche.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub cid: u32,
    pub name: String,
    pub account: String,
    pub tid: u32,
    pub level: i32,
    pub online: bool,
}

/// Fiche du personnage (bloc de l'op 10).
#[derive(Clone, Debug, PartialEq)]
pub struct CharEdit {
    pub kind: i32,
    pub name: String,
    pub code: u16,
    pub value: i32,
    pub value64: i64,
    pub exp_delta: i64,
}

/// Opération sur un objet (bloc de l'op 12).
#[derive(Clone, Debug, PartialEq)]
pub struct ItemOp {
    pub name: String,
    pub sub_op: u8,
    pub tid: u32,
    pub count: i32,
}

/// Texte terminé par un zéro dans `b[at..at + len]` (Windows-1252).
fn c_text(b: &[u8], at: usize, len: usize) -> String {
    let s = b.get(at..(at + len).min(b.len())).unwrap_or_default();
    let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    archcore_net::text::decode(&s[..end])
}

fn le_i32(b: &[u8], at: usize) -> i32 {
    b.get(at..at + 4).map_or(0, |v| i32::from_le_bytes(v.try_into().unwrap()))
}

fn le_i64(b: &[u8], at: usize) -> i64 {
    b.get(at..at + 8).map_or(0, |v| i64::from_le_bytes(v.try_into().unwrap()))
}

/// Bloc de la fiche (0x152 octets), `None` s'il est trop court pour le nom et le code.
pub fn decode_char_edit(b: &[u8]) -> Option<CharEdit> {
    if b.len() < 0x40 {
        return None;
    }
    Some(CharEdit {
        kind: le_i32(b, 0),
        name: c_text(b, 4, 49),
        code: u16::from_le_bytes([b[0x35], b[0x36]]),
        value: le_i32(b, 0x38),
        value64: le_i64(b, 0x40),
        exp_delta: le_i64(b, 0x14a),
    })
}

/// Bloc d'une opération sur un objet (0x21c octets), `None` s'il est trop court.
pub fn decode_item_op(b: &[u8]) -> Option<ItemOp> {
    if b.len() < 0x79 {
        return None;
    }
    Some(ItemOp { name: c_text(b, 0, 49), sub_op: b[0x62], tid: le_i32(b, 0x70) as u32, count: le_i32(b, 0x75) })
}

/// Nom passé entre guillemets à une commande de `gm.rs` (les guillemets ne sont pas permis dans un nom).
fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', ""))
}

/// Personnages dont le nom (critère 0) ou le compte (1) vaut `text` : en jeu d'abord, puis en base.
pub async fn search(world: &World, st: &WorldState, text: &str, by_account: bool) -> anyhow::Result<Vec<Found>> {
    let ctx = &world.ctx;
    let key = text.trim().to_lowercase();
    let mut found: Vec<Found> = Vec::new();
    if key.is_empty() {
        return Ok(found);
    }
    let mut online: Vec<&super::Player> =
        st.players.values().filter(|p| if by_account { p.account.username.to_lowercase() == key } else { p.ch.name.to_lowercase() == key }).collect();
    online.sort_by_key(|p| p.ch.id);
    for p in online {
        found.push(Found { cid: p.ch.id, name: p.ch.name.clone(), account: p.account.username.clone(), tid: p.ch.template_id, level: p.ch.level, online: true });
    }
    // Personnages hors ligne (et ceux du compte qui ne sont pas en jeu).
    let rows = if by_account {
        let Some(acc) = ctx.db.auth.first("SELECT id, username FROM account WHERE username = ?", (text.trim().to_string(),)).await? else { return Ok(found) };
        let (id, username): (u32, String) = (col(&acc, "id"), col(&acc, "username"));
        let rows = ctx.db.characters.rows("SELECT id, name, template_id, level FROM characters WHERE account_id = ? AND deleted_at IS NULL ORDER BY id", (id,)).await?;
        rows.into_iter().map(|r| (r, Some(username.clone()))).collect::<Vec<_>>()
    } else {
        let rows = ctx.db.characters.rows("SELECT id, name, template_id, level, account_id FROM characters WHERE name = ? AND deleted_at IS NULL", (text.trim().to_string(),)).await?;
        let mut out = Vec::new();
        for r in rows {
            let account_id: u32 = col(&r, "account_id");
            let username = ctx.db.auth.first("SELECT username FROM account WHERE id = ?", (account_id,)).await?.map(|a| col::<String>(&a, "username"));
            out.push((r, username));
        }
        out
    };
    for (r, username) in rows {
        let cid: u32 = col(&r, "id");
        if found.iter().any(|f| f.cid == cid) {
            continue;
        }
        found.push(Found { cid, name: col(&r, "name"), account: username.unwrap_or_default(), tid: col(&r, "template_id"), level: col(&r, "level"), online: false });
    }
    found.truncate(SEARCH_LIMIT);
    Ok(found)
}

/// Réponse de `/search` : une ligne par personnage.
pub fn describe(found: &[Found], text: &str) -> String {
    if found.is_empty() {
        return format!("Aucun personnage pour « {text} ».");
    }
    let mut lines = vec![format!("{} personnage(s) pour « {text} » :", found.len())];
    for f in found {
        lines.push(format!("· {} (compte {}, CID {}, niveau {}, {})", f.name, f.account, f.cid, f.level, if f.online { "en jeu" } else { "hors ligne" }));
    }
    lines.join("\n")
}

/// Op 4 : un personnage trouvé, dans le sous-paquet du champ 3 (recherche renvoyée avec son critère et son texte).
pub fn search_result_packet(gm_cid: u32, crit: i64, text: &str, f: &Found, server: &str) -> Vec<u8> {
    let r = &mini::ADMIN_SEARCH_RESULT;
    let result = build_mini(
        r,
        &Fields::new(r)
            .with(0, 0)
            .with(1, 0)
            .with(2, f.name.as_str())
            .with(3, f.cid)
            .with(4, f.account.as_str())
            .with(5, server)
            .with(6, f.tid)
            .with(7, f.level)
            .with(11, i64::from(f.online)),
    );
    let m = &mini::ADMIN_SEARCH;
    let search = build_mini(m, &Fields::new(m).with(0, 0).with(1, crit).with(2, 0).with(3, text).with(4, result));
    let s = &schema::ADMIN;
    build_packet(s, &Fields::new(s).with(0, op::SEARCH_RESULT).with(1, gm_cid).with(3, search))
}

/// Commande de discussion pour une modification de la fiche, `Err` avec le message si le code n'est pas pris en charge.
pub fn char_edit_command(e: &CharEdit) -> Result<String, String> {
    let who = quoted(&e.name);
    match e.code {
        edit_code::LEVEL => Ok(format!("/level {} {who}", e.value)),
        edit_code::EXP if e.exp_delta > 0 => Ok(format!("/exp {} {who}", e.exp_delta)),
        edit_code::EXP => Err("outil MJ : seule une hausse de l'expérience est prise en charge".into()),
        edit_code::MONEY => Ok(format!("/money {} {who}", e.value64)),
        code => Err(format!("outil MJ : champ {code:#x} de la fiche non pris en charge par ArchCore")),
    }
}

/// Exécute une commande de `gm.rs` au nom du MJ et lui envoie la réponse.
async fn run_command(world: &Arc<World>, st: &mut WorldState, sid: u32, line: &str) -> anyhow::Result<()> {
    info!("[mj] outil MJ de {} : {line}", st.players.get(&sid).map_or("?", |p| p.ch.name.as_str()));
    if let Some(Some(reply)) = gm::execute_quiet(world, st, sid, line).await? {
        send_system(st, sid, &reply);
    }
    Ok(())
}

pub async fn on_packet(world: &Arc<World>, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some(player) = st.players.get(&sid) else { return Ok(()) };
    let opc = p.int(0).unwrap_or(-1);
    // Comme une commande MJ tapée par un joueur : sans droits, rien ne se passe et rien n'est répondu.
    if player.gm_level <= 0 {
        debug!("[mj] AGPMADMIN op {opc} de {} sans droits : ignoré", player.ch.name);
        return Ok(());
    }
    let (cid, name, level, account) = (player.ch.id, player.ch.name.clone(), player.gm_level, player.account.username.clone());
    match opc {
        op::CONNECT => player.send_raw(super::chat::admin_level_packet(cid, &name, level, &account)),
        op::SEARCH => {
            let Some(m) = p.bytes(3).and_then(|b| decode_mini(&mini::ADMIN_SEARCH, b).ok()) else { return Ok(()) };
            let (crit, text) = (m.int(1).unwrap_or(criterion::NAME), m.text(3).unwrap_or_default());
            let required = gm::level_for(world, "search");
            if level < required {
                send_system(st, sid, &format!("Niveau MJ {required} requis pour /search (le vôtre : {level})."));
                return Ok(());
            }
            let found = search(world, st, &text, crit == criterion::ACCOUNT).await?;
            info!("[mj] outil MJ de {name} : recherche « {text} » (critère {crit}) : {} personnage(s)", found.len());
            let server = world.ctx.config.world.name.clone();
            let p = &st.players[&sid];
            for f in &found {
                p.send_raw(search_result_packet(cid, crit, &text, f, &server));
            }
            if found.is_empty() {
                send_system(st, sid, &describe(&found, &text));
            }
        }
        op::CHAR_EDIT => {
            let Some(e) = p.bytes(5).and_then(|b| decode_mini(&mini::ADMIN_BLOCK2, b).ok()).and_then(|m| m.bytes(0).and_then(decode_char_edit)) else { return Ok(()) };
            match char_edit_command(&e) {
                Ok(line) => run_command(world, st, sid, &line).await?,
                Err(msg) => send_system(st, sid, &msg),
            }
        }
        op::SANCTION => {
            let Some(m) = p.bytes(10).and_then(|b| decode_mini(&mini::ADMIN_SANCTION, b).ok()) else { return Ok(()) };
            let target = m.text(0).unwrap_or_default();
            if m.int(2) == Some(1) {
                run_command(world, st, sid, &format!("/disconnect {}", quoted(&target))).await?;
            }
            if [3, 5, 7].iter().any(|&i| m.int(i).unwrap_or(0) != 0) {
                send_system(st, sid, "outil MJ : sanctions (discussion, personnage, compte) non prises en charge ; /chatban pour la discussion");
            }
        }
        op::ITEM => {
            let Some(o) = p.bytes(12).and_then(|b| decode_mini(&mini::ADMIN_BLOCK, b).ok()).and_then(|m| m.bytes(0).and_then(decode_item_op)) else { return Ok(()) };
            if o.sub_op == ITEM_CREATE {
                run_command(world, st, sid, &format!("/create {} {} {}", quoted(&o.name), o.tid, o.count.max(1))).await?;
            } else {
                send_system(st, sid, &format!("outil MJ : opération {} sur les objets non prise en charge par ArchCore", o.sub_op));
            }
        }
        op::GUILD | op::TITLE => send_system(st, sid, &format!("outil MJ : op {opc} (guildes, titres) non prise en charge par ArchCore")),
        // Jamais proposée par ArchCore ; arriverait au serveur de la sélection des personnages.
        op::COMPENSATION => debug!("[world] #{sid} AGPMADMIN op 15 (compensation) ignorée"),
        _ => debug!("[world] #{sid} AGPMADMIN op {opc} ignorée"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(len: usize, put: &[(usize, &[u8])]) -> Vec<u8> {
        let mut b = vec![0u8; len];
        for (at, v) in put {
            b[*at..*at + v.len()].copy_from_slice(v);
        }
        b
    }

    #[test]
    fn fiche_du_personnage() {
        let b = block(0x152, &[(0, &1i32.to_le_bytes()), (4, b"SimA"), (0x35, &3u16.to_le_bytes()), (0x38, &42i32.to_le_bytes())]);
        let e = decode_char_edit(&b).unwrap();
        assert_eq!((e.kind, e.name.as_str(), e.code, e.value), (1, "SimA", edit_code::LEVEL, 42));
        assert_eq!(char_edit_command(&e).unwrap(), "/level 42 \"SimA\"");
        let b = block(0x152, &[(4, b"SimA"), (0x35, &4u16.to_le_bytes()), (0x14a, &5000i64.to_le_bytes())]);
        assert_eq!(char_edit_command(&decode_char_edit(&b).unwrap()).unwrap(), "/exp 5000 \"SimA\"");
        let b = block(0x152, &[(4, b"SimA"), (0x35, &5u16.to_le_bytes()), (0x40, &123456i64.to_le_bytes())]);
        assert_eq!(char_edit_command(&decode_char_edit(&b).unwrap()).unwrap(), "/money 123456 \"SimA\"");
        let b = block(0x152, &[(4, b"SimA"), (0x35, &edit_code::HP.to_le_bytes())]);
        assert!(char_edit_command(&decode_char_edit(&b).unwrap()).is_err());
        assert!(decode_char_edit(&[0u8; 0x20]).is_none());
    }

    #[test]
    fn creation_d_objet() {
        let b = block(0x21c, &[(0, b"Le \"Heros\""), (0x62, &[ITEM_CREATE]), (0x70, &190i32.to_le_bytes()), (0x75, &3i32.to_le_bytes())]);
        let o = decode_item_op(&b).unwrap();
        assert_eq!(o, ItemOp { name: "Le \"Heros\"".into(), sub_op: 0, tid: 190, count: 3 });
        assert_eq!(quoted(&o.name), "\"Le Heros\"");
        assert!(decode_item_op(&[0u8; 0x40]).is_none());
    }

    #[test]
    fn resultat_de_recherche() {
        let f = Found { cid: 7, name: "SimA".into(), account: "sima".into(), tid: 96, level: 20, online: true };
        let raw = search_result_packet(1, criterion::ACCOUNT, "sima", &f, "ArchCore");
        let p = archcore_net::decode_packet(&schema::ADMIN, &raw).unwrap();
        assert_eq!((p.int(0), p.int(1)), (Some(op::SEARCH_RESULT), Some(1)));
        let m = decode_mini(&mini::ADMIN_SEARCH, p.bytes(3).unwrap()).unwrap();
        assert_eq!((m.int(1), m.text(3).as_deref()), (Some(criterion::ACCOUNT), Some("sima")));
        let r = decode_mini(&mini::ADMIN_SEARCH_RESULT, m.bytes(4).unwrap()).unwrap();
        assert_eq!((r.text(2).as_deref(), r.int(3), r.text(4).as_deref(), r.int(6), r.int(7), r.int(11)), (Some("SimA"), Some(7), Some("sima"), Some(96), Some(20), Some(1)));
        assert_eq!(describe(&[f], "SimA").lines().count(), 2);
        assert_eq!(describe(&[], "x"), "Aucun personnage pour « x ».");
    }
}
