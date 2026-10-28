//! Changement d'apparence chez les coiffeurs (AgpmEventCharCustomize, 0x43 ; fonction de PNJ 29, « <Stylist> »).
//!
//! Coiffures et visages proposés : table `character_customize` (charactercustomizelist.txt), une ligne par numéro et
//! par modèle de personnage, avec son niveau exigé et son prix. Le numéro est celui des champs 20 (visage) et 21
//! (coiffure) d'AGPMCHARACTER.
//!
//! Op 0 : le client demande la fenêtre au coiffeur (PNJ de fonction 29 à portée) → op 1, événement et CID renvoyés ;
//! le client ouvre `UI_Customizing` (ou affiche lui-même son refus s'il est monté ou transformé). Op 2 : achat du
//! visage (champ 3) et de la coiffure (champ 4), -1 pour ce qui ne change pas ; le client n'attend aucune réponse.
//! Calcul de la logique partagée d'Archonia (`FUN_14022d860`) : prix = somme des prix (taxe de région nulle) ; niveau
//! exigé = celui du visage s'il est choisi, aucun pour une coiffure seule ; la colonne « Case » n'est pas vérifiée au
//! coiffeur. ArchCore refuse en plus, avec un message système (le protocole n'a pas de code de résultat) : un joueur
//! monté, une valeur absente de la table pour son modèle, un prix en objets (colonne « Price(Skull) », nulle dans les
//! données). Accepté : argent débité, apparence enregistrée, AGPMCHARACTER op 1 (champs 20 / 21 changés) au joueur et
//! à ceux qui le voient ; le client du joueur affiche alors « Your appearance has changed to … ».
//!
//! À la création d'un personnage (`login.rs`), visage et coiffure doivent être des lignes du modèle choisi proposées à
//! la création (Case 0 ou 2) : sinon 0.

use super::npc_events::{npc_distance, resolve, MAX_NPC_DISTANCE};
use super::npcs::{event_function, Npc};
use super::{send_system, set_money, WorldCtx, WorldState};
use crate::db::{col, Databases};
use crate::{debug, info, warn};
use archcore_net::schema::{self, customize_op, mini};
use archcore_net::{build_mini, build_packet, decode_mini, Fields, Packet};

/// Genres de ligne (colonne « Type »).
pub mod kind {
    pub const HAIR: u8 = 1;
    pub const FACE: u8 = 2;
}

/// Une coiffure ou un visage proposé à un modèle.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Look {
    pub kind: u8,
    pub number: i32,
    pub name: String,
    /// 0 création et coiffeur, 1 coiffeur seulement, 2 création seulement.
    pub case_kind: i32,
    pub tid: u32,
    pub use_level: i32,
    pub price: i64,
    /// Objet payé à la place de l'or (`price` exemplaires), 0 : or.
    pub price_item: u32,
}

/// Table des coiffures et visages, chargée au démarrage.
#[derive(Default)]
pub struct CustomizeTable {
    pub rows: Vec<Look>,
}

impl CustomizeTable {
    pub async fn load(db: &Databases) -> Self {
        let sql = "SELECT kind, number, name, case_kind, character_tid, use_level, price, price_item_tid FROM character_customize ORDER BY row_order";
        match db.world.rows(sql, ()).await {
            Ok(rows) => CustomizeTable {
                rows: rows
                    .iter()
                    .map(|r| Look {
                        kind: col(r, "kind"),
                        number: col(r, "number"),
                        name: col(r, "name"),
                        case_kind: col(r, "case_kind"),
                        tid: col(r, "character_tid"),
                        use_level: col(r, "use_level"),
                        price: col(r, "price"),
                        price_item: col(r, "price_item_tid"),
                    })
                    .collect(),
            },
            Err(e) => {
                warn!("coiffures et visages non chargés ({e:#}) : lancer `archcore db-init` puis `archcore import`");
                CustomizeTable::default()
            }
        }
    }

    pub fn find(&self, tid: u32, kind: u8, number: i32) -> Option<&Look> {
        self.rows.iter().find(|l| l.tid == tid && l.kind == kind && l.number == number)
    }
}

/// Ce que coûte un changement d'apparence : prix, niveau exigé, nouvelles valeurs (`None` : inchangée).
#[derive(Clone, Debug, PartialEq)]
pub struct Quote {
    pub price: i64,
    pub level: i32,
    pub face: Option<i32>,
    pub hair: Option<i32>,
}

/// Raison d'un refus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Rien de choisi, ou les valeurs actuelles.
    Nothing,
    /// Numéro absent de la table pour ce modèle.
    Unknown,
    /// Prix en objets, pas pris en charge.
    ItemPrice,
}

/// Devis d'un achat pour le modèle `tid` dont l'apparence actuelle est (visage, coiffure).
pub fn quote(table: &CustomizeTable, tid: u32, current: (i32, i32), face: i32, hair: i32) -> Result<Quote, Refusal> {
    let face = Some(face).filter(|f| *f >= 0 && *f != current.0);
    let hair = Some(hair).filter(|h| *h >= 0 && *h != current.1);
    if face.is_none() && hair.is_none() {
        return Err(Refusal::Nothing);
    }
    let (mut price, mut level) = (0, 0);
    for (k, number) in [(kind::HAIR, hair), (kind::FACE, face)] {
        let Some(number) = number else { continue };
        let look = table.find(tid, k, number).ok_or(Refusal::Unknown)?;
        if look.price_item != 0 {
            return Err(Refusal::ItemPrice);
        }
        price += look.price;
        // Comme le client : seul le visage fixe un niveau exigé.
        if k == kind::FACE {
            level = look.use_level;
        }
    }
    Ok(Quote { price, level, face, hair })
}

/// Visage ou coiffure choisi à la création : gardé s'il est proposé à la création pour ce modèle (Case 0 ou 2), sinon
/// 0. Lignes du modèle pour ce genre : (numéro, Case) ; aucune (table pas importée) : valeur gardée.
pub fn creation_value(rows: &[(i32, i32)], number: i32) -> i32 {
    if rows.is_empty() || rows.iter().any(|(n, c)| *n == number && (*c == 0 || *c == 2)) {
        number
    } else {
        0
    }
}

/// Paquet AgpmEventCharCustomize op 1 (accord) : événement renvoyé et CID du joueur.
pub fn grant_packet(base: Vec<u8>, cid: u32) -> Vec<u8> {
    let s = &schema::EVENT_CHAR_CUSTOMIZE;
    build_packet(s, &Fields::new(s).with(0, customize_op::GRANT).with(1, base).with(2, cid))
}

/// Coiffeur visé par l'événement du paquet (champ 1), à portée ; sans événement, le plus proche à portée.
fn stylist<'a>(ctx: &'a WorldCtx, p: &super::Player, base: Option<&Fields>) -> Option<&'a Npc> {
    let styles = |n: &Npc| n.events.iter().any(|e| e.function == event_function::CHAR_CUSTOMIZE);
    let near = |n: &Npc| npc_distance(n, p.ch.pos_x, p.ch.pos_z) <= MAX_NPC_DISTANCE;
    match base {
        Some(b) => resolve(&ctx.npcs, b).map(|(n, _)| n).filter(|n| styles(n) && near(n)),
        None => ctx.npcs.near(p.ch.pos(), MAX_NPC_DISTANCE).find(|n| styles(n)),
    }
}

/// AgpmEventCharCustomize reçu du client.
pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some(player) = st.players.get(&sid) else { return Ok(()) };
    if !player.in_game || player.dead {
        return Ok(());
    }
    let cid = player.ch.id;
    if p.int(2).is_some_and(|c| c != 0 && c != cid as i64) {
        warn!("[apparence] #{sid} CID {:?} usurpé", p.int(2));
        return Ok(());
    }
    let base = p.bytes(1).and_then(|b| decode_mini(&mini::EVENT_BASE, b).ok());
    let Some(npc) = stylist(ctx, player, base.as_ref()) else {
        warn!("[apparence] #{sid} {} : pas de coiffeur à portée", player.ch.name);
        if p.int(0) == Some(customize_op::BUY) {
            send_system(st, sid, "Vous êtes trop loin du coiffeur.");
        }
        return Ok(());
    };
    let npc_name = npc.name.clone();
    match p.int(0) {
        Some(customize_op::REQUEST) => {
            let echo = base.as_ref().map(|b| build_mini(&mini::EVENT_BASE, b)).unwrap_or_else(|| super::npcs::event_base(npc.cid, event_function::CHAR_CUSTOMIZE));
            player.send_raw(grant_packet(echo, cid));
            debug!("[apparence] {} ouvre la fenêtre de {npc_name}", player.ch.name);
        }
        Some(customize_op::BUY) => buy(ctx, st, sid, p.int(3).unwrap_or(-1) as i32, p.int(4).unwrap_or(-1) as i32, &npc_name).await?,
        op => debug!("[apparence] #{sid} op {op:?} ignorée"),
    }
    Ok(())
}

async fn buy(ctx: &WorldCtx, st: &mut WorldState, sid: u32, face: i32, hair: i32, npc_name: &str) -> anyhow::Result<()> {
    let p = &st.players[&sid];
    if super::ride::is_mounted(&p.inventory) {
        send_system(st, sid, "Descendez de votre monture pour changer d'apparence.");
        return Ok(());
    }
    let q = match quote(&ctx.customize, p.ch.template_id, (p.ch.face, p.ch.hair), face, hair) {
        Ok(q) => q,
        Err(r) => {
            debug!("[apparence] {} : refus {r:?} (visage {face}, coiffure {hair})", p.ch.name);
            let text = match r {
                Refusal::Nothing => "Choisissez une coiffure ou un visage différents des vôtres.",
                Refusal::Unknown => "Cette apparence n'est pas proposée à votre personnage.",
                Refusal::ItemPrice => "Cette apparence se paie en objets : ce n'est pas encore pris en charge.",
            };
            send_system(st, sid, text);
            return Ok(());
        }
    };
    if p.ch.level < q.level {
        send_system(st, sid, &format!("Niveau {} requis pour ce visage.", q.level));
        return Ok(());
    }
    if (p.ch.money as i64) < q.price {
        send_system(st, sid, &format!("Il vous faut {} pièce(s) d'or.", q.price));
        return Ok(());
    }
    let money = p.ch.money as i64 - q.price;
    if q.price > 0 {
        set_money(ctx, st, sid, money).await?;
    }
    let p = st.players.get_mut(&sid).unwrap();
    p.ch.face = q.face.unwrap_or(p.ch.face);
    p.ch.hair = q.hair.unwrap_or(p.ch.hair);
    let (cid, name, f, h) = (p.ch.id, p.ch.name.clone(), p.ch.face, p.ch.hair);
    ctx.db.characters.exec("UPDATE characters SET face = ?, hair = ? WHERE id = ?", (f, h, cid)).await?;
    let c = &schema::CHARACTER;
    let plain = build_packet(c, &Fields::new(c).with(0, 1).with(1, cid).with_opt(20, q.face).with_opt(21, q.hair));
    super::combat::send_around(st, sid, &plain);
    crate::journal::shop(&ctx.db, (cid, &name), npc_name, "style", 0, 0, 1, q.price);
    info!("[apparence] {name} chez {npc_name} : visage {f}, coiffure {h} pour {} pièce(s)", q.price);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> CustomizeTable {
        let l = |kind, number, case_kind, use_level, price| Look { kind, number, case_kind, tid: 96, use_level, price, ..Default::default() };
        CustomizeTable {
            rows: vec![l(kind::HAIR, 0, 0, 1, 0), l(kind::HAIR, 4, 1, 7, 1000), l(kind::FACE, 1, 1, 7, 3000), l(kind::FACE, 21, 1, 10, 3000), Look { price_item: 9, ..l(kind::FACE, 5, 1, 1, 2) }],
        }
    }

    #[test]
    fn devis_comme_le_client() {
        let t = table();
        assert_eq!(quote(&t, 96, (2, 1), -1, 4), Ok(Quote { price: 1000, level: 0, face: None, hair: Some(4) }), "coiffure seule : aucun niveau");
        assert_eq!(quote(&t, 96, (2, 1), 21, -1), Ok(Quote { price: 3000, level: 10, face: Some(21), hair: None }));
        assert_eq!(quote(&t, 96, (2, 1), 1, 4).map(|q| (q.price, q.level)), Ok((4000, 7)), "niveau du visage");
        assert_eq!(quote(&t, 96, (2, 4), -1, 4), Err(Refusal::Nothing), "coiffure actuelle");
        assert_eq!(quote(&t, 96, (2, 1), -1, -1), Err(Refusal::Nothing));
        assert_eq!(quote(&t, 96, (2, 1), -1, 9), Err(Refusal::Unknown));
        assert_eq!(quote(&t, 1, (2, 1), -1, 4), Err(Refusal::Unknown), "autre modèle");
        assert_eq!(quote(&t, 96, (2, 1), 5, -1), Err(Refusal::ItemPrice));
        assert_eq!(quote(&t, 96, (2, 1), -1, 0).map(|q| q.price), Ok(0), "Case 0 permise au coiffeur");
    }

    #[test]
    fn valeurs_de_creation() {
        let rows = [(0, 0), (1, 1), (2, 2)];
        assert_eq!([0, 1, 2, 7].map(|n| creation_value(&rows, n)), [0, 0, 2, 0], "Case 0 ou 2 seulement");
        assert_eq!(creation_value(&[], 7), 7, "table absente");
    }

    #[test]
    fn paquet_d_accord() {
        let base = super::super::npcs::event_base(0x4000_00de, 29);
        let p = archcore_net::decode_packet(&schema::EVENT_CHAR_CUSTOMIZE, &grant_packet(base, 3)).unwrap();
        assert_eq!((p.int(0), p.int(2)), (Some(1), Some(3)));
        let ev = decode_mini(&mini::EVENT_BASE, p.bytes(1).unwrap()).unwrap();
        assert_eq!((ev.int(1), ev.int(3)), (Some(0x4000_00de), Some(29)));
        let s = &schema::EVENT_CHAR_CUSTOMIZE;
        let buy = archcore_net::decode_packet(s, &build_packet(s, &Fields::new(s).with(0, 2).with(2, 3).with(3, -1).with(4, 4))).unwrap();
        assert_eq!((buy.int(3), buy.int(4)), (Some(-1), Some(4)));
    }
}
