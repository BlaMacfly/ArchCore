//! Recrutement de groupe de l'épisode 8 (AgcmRecruit 0x0A) : annonces « cherche un groupe » / « cherche des membres ».
//!
//! ```text
//!   Relevé (W = client de Webzen, A = Archonia, modules identiques ; détail dans SPEC-RECRUIT.md) :
//!   C → S  op 3 / op 6 : recherche (champ 5 RECRUIT_SEARCH : « ma classe », classe, page, classes cherchées) des
//!            joueurs sans groupe (3) ou des groupes (6) ; boutons des onglets et des pages d'AcUIRecruit.
//!          op 4 : inscription de son groupe (champ 4 RECRUIT_PARTY : texte, valeur, niveaux mini et maxi, classes
//!            voulues, nom du chef, CID), bouton « OK » d'AcUIRecruitWrite ; op 5 : retrait (nom du chef).
//!   S → C  op 3 / op 6 : une ligne de la liste (champ 3 ou 4 : total, rang 0 à 4, colonnes) ; op 8 champ 6 = 9 :
//!            « aucun résultat » (texte coréen en dur). Ops 1, 2, 4, 5 ignorées à la réception.
//!   Fenêtres AcUIRecruit / AcUIRecruitWrite : construites par le module (W 0x78fc90), jamais ajoutées au
//!   gestionnaire d'interface ni ouvertes (aucune référence au module hors de son constructeur, aucune fenêtre
//!   UI_* ni touche) : le système est mort dans les deux clients. ArchCore accepte donc le type (pas de refus à la
//!   réception) et note ce qu'il reçoit, sans rien répondre.
//! ```

use super::WorldState;
use crate::{debug, info};
use archcore_net::schema::{mini, recruit_op};
use archcore_net::{decode_mini, Fields, Packet};

/// Ce qu'une demande annonce, pour le journal.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Request {
    pub op: i64,
    /// Groupe inscrit ou retiré (ops 4, 5) : texte, valeur, niveaux, classes, chef.
    pub party: Option<PartyAd>,
    /// Recherche (ops 3, 6) : page et classes cherchées.
    pub search: Option<(i64, i64)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PartyAd {
    pub text: String,
    pub value: i64,
    pub levels: (i64, i64),
    pub classes: i64,
    pub leader: String,
}

/// Décode une demande d'AgcmRecruit.
pub fn decode(p: &Fields) -> Request {
    let op = p.int(0).unwrap_or(-1);
    let party = p.bytes(4).and_then(|b| decode_mini(&mini::RECRUIT_PARTY, b).ok()).map(|f| PartyAd {
        text: f.text(2).unwrap_or_default(),
        value: f.int(3).unwrap_or(0),
        levels: (f.int(4).unwrap_or(0), f.int(5).unwrap_or(0)),
        classes: f.int(7).unwrap_or(0),
        leader: f.text(8).unwrap_or_default(),
    });
    let search = p.bytes(5).and_then(|b| decode_mini(&mini::RECRUIT_SEARCH, b).ok()).map(|f| (f.int(2).unwrap_or(0), f.int(3).unwrap_or(0)));
    Request { op, party, search }
}

/// AGCMRECRUIT reçu : noté, sans réponse (voir l'en-tête).
pub async fn on_packet(st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let who = st.players.get(&sid).map(|p| p.ch.name.clone()).unwrap_or_else(|| format!("#{sid}"));
    let r = decode(&p.fields);
    match r.op {
        recruit_op::REGISTER_PARTY | recruit_op::REMOVE_PARTY => info!("[recrutement] {who} : op {} {:?} (fenêtre d'origine jamais ouverte : sans effet)", r.op, r.party),
        recruit_op::SEARCH_MEMBER | recruit_op::SEARCH_PARTY => info!("[recrutement] {who} : recherche op {} {:?} (sans effet)", r.op, r.search),
        op => debug!("[recrutement] {who} : op {op} ignorée"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use archcore_net::schema::{self, ptype};
    use archcore_net::{build_mini, build_packet, decode_packet};

    #[test]
    fn inscription_et_recherche() {
        // Op 4 comme W 0x6e9d30 : champ 1 (module + 0xb98), 2 CID, 4 groupe.
        let rp = &mini::RECRUIT_PARTY;
        let ad = build_mini(rp, &Fields::new(rp).with(0, 0).with(1, 0).with(2, "Chasse").with(3, 3).with(4, 20).with(5, 30).with(6, 0).with(7, 0b1010).with(8, "Halo").with(9, 7));
        let s = &schema::RECRUIT;
        assert_eq!(s.id, ptype::RECRUIT);
        let bytes = build_packet(s, &Fields::new(s).with(0, recruit_op::REGISTER_PARTY).with(1, 0).with(2, 7).with(4, ad).with(6, 0));
        let back = decode_packet(s, &bytes).unwrap();
        let r = decode(&back.fields);
        assert_eq!(r.op, 4);
        assert_eq!(r.party, Some(PartyAd { text: "Chasse".into(), value: 3, levels: (20, 30), classes: 10, leader: "Halo".into() }));
        assert_eq!(r.search, None);
        // Op 6 : recherche des groupes, page 1, classes 1 | 4.
        let rs = &mini::RECRUIT_SEARCH;
        let q = build_mini(rs, &Fields::new(rs).with(0, 0).with(1, 0).with(2, 1).with(3, 5));
        let bytes = build_packet(s, &Fields::new(s).with(0, recruit_op::SEARCH_PARTY).with(1, 0).with(2, 7).with(5, q));
        let r = decode(&decode_packet(s, &bytes).unwrap().fields);
        assert_eq!((r.op, r.search, r.party), (6, Some((1, 5)), None));
    }
}
