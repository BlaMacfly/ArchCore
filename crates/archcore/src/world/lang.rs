//! Capacité `language/1` côté monde : changement de langue en cours de partie (`SET_LANGUAGE`) et textes
//! localisés à la demande (`TEXT_REQUEST` → `TEXT_REPLY`).
//!
//! Les noms des PNJ et des monstres sont déjà envoyés dans la langue du joueur par leurs paquets de vue
//! (`send_nearby_npcs`, `sync_monsters`). `TEXT_REQUEST` sert aux textes que le protocole d'origine ne
//! transporte pas (noms de quêtes, d'objets, de compétences, de régions…) : ArchRust les demande par sorte
//! et identifiants, et reçoit chaque texte dans la langue appliquée, avec le repli de `locale.rs`.

use super::{WorldCtx, WorldState};
use crate::ext::{self, LANGUAGE};
use crate::locale::Catalog;
use crate::{debug, info, warn};
use archcore_net::ext_text::{decode_ids, encode_texts};
use archcore_net::schema::{self, ext_op, ext_result, ext_text_kind};
use archcore_net::{Fields, Packet};

pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    match p.int(0).unwrap_or(-1) {
        ext_op::SET_LANGUAGE => set_language(ctx, st, sid, p).await,
        ext_op::TEXT_REQUEST => {
            text_request(ctx, st, sid, p);
            Ok(())
        }
        op => {
            warn!("[langue] #{sid} opération d'extension {op} inattendue, ignorée");
            Ok(())
        }
    }
}

fn fail(st: &WorldState, sid: u32, request_id: i64, code: i64) {
    if let Some(player) = st.players.get(&sid) {
        player.send(&schema::ARCHCORE_EXT, ext::result(request_id, code));
    }
}

async fn set_language(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some(player) = st.players.get(&sid) else { return Ok(()) };
    let request_id = p.int(1).unwrap_or(0);
    if !player.sess.has_capability(LANGUAGE) {
        debug!("[langue] #{sid} SET_LANGUAGE sans la capacité {LANGUAGE}");
        return Ok(fail(st, sid, request_id, ext_result::INVALID_REQUEST));
    }
    let Some(wish) = ext::language_field(p) else {
        return Ok(fail(st, sid, request_id, ext_result::INVALID_REQUEST));
    };
    let applied = ctx.locale.pick(Some(&wish));
    player.sess.set_language(Some(wish.clone()));
    let account_id = player.account.id;
    let player = st.players.get_mut(&sid).unwrap();
    player.language = applied.clone();
    info!("[langue] {} passe en {applied} (souhait {wish})", player.ch.name);
    player.send(&schema::ARCHCORE_EXT, ext::language_ack(request_id, &applied, &ctx.locale.summary.available_list()));
    crate::locale::save_account_language(&ctx.db, account_id, &wish).await;
    Ok(())
}

/// `TEXT_REPLY` pour la demande `request_id` : chaque identifiant demandé, dans l'ordre, avec son texte
/// dans la langue `lang` (chaîne vide si rien n'est connu). `None` si la demande est mal formée.
pub fn text_reply(catalog: &Catalog, lang: &str, kind: &str, ids: &[u32], request_id: i64) -> Option<Fields> {
    if !ext_text_kind::ALL.contains(&kind) || ids.is_empty() || ids.len() > ext_text_kind::MAX_IDS {
        return None;
    }
    let texts: Vec<(u32, String)> = ids.iter().map(|&id| (id, catalog.text(kind, id, lang).unwrap_or_default())).collect();
    let payload = encode_texts(texts.iter().map(|(i, t)| (*i, t.as_str())));
    let s = &schema::ARCHCORE_EXT;
    let f = Fields::new(s).with(0, ext_op::TEXT_REPLY).with(1, request_id).with(16, lang.as_bytes().to_vec()).with(18, kind.as_bytes().to_vec());
    Some(if payload.is_empty() { f } else { f.with(20, payload) })
}

fn text_request(ctx: &WorldCtx, st: &WorldState, sid: u32, p: &Packet) {
    let Some(player) = st.players.get(&sid) else { return };
    let request_id = p.int(1).unwrap_or(0);
    if !player.sess.has_capability(LANGUAGE) {
        return fail(st, sid, request_id, ext_result::INVALID_REQUEST);
    }
    let kind = String::from_utf8_lossy(p.bytes(18).unwrap_or_default()).into_owned();
    let ids = decode_ids(p.bytes(19).unwrap_or_default());
    match text_reply(&ctx.locale, &player.language, &kind, &ids, request_id) {
        Some(reply) => {
            debug!("[langue] {} demande {} texte(s) {kind} en {}", player.ch.name, ids.len(), player.language);
            player.send(&schema::ARCHCORE_EXT, reply);
        }
        None => {
            warn!("[langue] {} : TEXT_REQUEST invalide (sorte {kind:?}, {} identifiant(s))", player.ch.name, ids.len());
            fail(st, sid, request_id, ext_result::INVALID_REQUEST);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locale::kind;
    use crate::locale::tests::catalog;
    use archcore_net::ext_text::decode_texts;
    use archcore_net::{build_packet, decode_packet};

    #[test]
    fn reponse_de_textes() {
        let c = catalog();
        let s = &schema::ARCHCORE_EXT;
        let f = text_reply(&c, "fr", kind::QUEST_NAME, &[100, 999], 5).unwrap();
        let p = decode_packet(s, &build_packet(s, &f)).unwrap();
        assert_eq!((p.int(0), p.int(1)), (Some(ext_op::TEXT_REPLY), Some(5)));
        assert_eq!(p.text(16).as_deref(), Some("fr"));
        assert_eq!(p.text(18).as_deref(), Some(kind::QUEST_NAME));
        assert_eq!(decode_texts(p.bytes(20).unwrap()), vec![(100, "Premiers pas".to_string()), (999, String::new())]);
        // Repli : texte d'origine en allemand, titre traduit pour un PNJ.
        let f = text_reply(&c, "de", kind::ITEM_NAME, &[190], 1).unwrap();
        assert_eq!(decode_texts(f.bytes(20).unwrap())[0].1, "Short sword (fixed)");
        let f = text_reply(&c, "fr-CA", kind::NPC_NAME, &[3], 1).unwrap();
        assert_eq!(decode_texts(f.bytes(20).unwrap())[0].1, "<Marchand> Darryl");
        // Demandes invalides.
        assert!(text_reply(&c, "fr", "bidule", &[1], 1).is_none());
        assert!(text_reply(&c, "fr", kind::QUEST_NAME, &[], 1).is_none());
        assert!(text_reply(&c, "fr", kind::QUEST_NAME, &vec![1; ext_text_kind::MAX_IDS + 1], 1).is_none());
    }
}
