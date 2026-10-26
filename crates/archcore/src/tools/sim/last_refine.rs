//! `archcore sim lastrefine` : commande MJ `/last_refine` (« Use last refine recipe » du panneau
//! « Admin Commands » d'Archonia).
//!
//! Refusée tant qu'aucun raffinage n'a réussi ; après un raffinage par recette (op 6, recette 4013 :
//! 100 Breath Elemental Stone → Gale Elemental Stone), elle relance la même recette avec les ingrédients de
//! l'inventaire, puis échoue faute d'ingrédients (résultat 1).

use super::{enter_world, step};
use archcore_net::schema::{self, chat_type};
use archcore_net::Fields;

const BREATH: i64 = 244;
const GALE: i64 = 245;

pub async fn run(user: &str, pass: &str, char_name: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let (it, ch, rf) = (&schema::ITEM, &schema::CHATTING, &schema::REFINERY);
    let say = |cid: u32, text: &str| Fields::new(ch).with(0, 0).with(1, cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text));
    let reply = |text: &'static str| move |q: &super::Recv| q.ptype == ch.id && q.int(2) == Some(chat_type::SYSTEM) && q.fields().text(6).is_some_and(|t| t.contains(text));

    step(&format!("{char_name} entre en jeu"));
    let mut w = enter_world(&cfg, user, pass, char_name, "").await?;
    let me = w.cid as i64;
    w.world.send(ch, say(w.cid, "/money 100000")).await?;
    // Piles de pierres Breath laissées par d'autres essais : détruites, seules comptent celles ajoutées ici.
    for (iid, _, _, _) in w.items.iter().filter(|i| i.1 == BREATH) {
        w.world.send(it, Fields::new(it).with(0, 21).with(2, *iid)).await?;
        w.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(*iid), 3000, "pile détruite").await?;
    }

    step("/last_refine sans raffinage : refusée");
    w.world.send(ch, say(w.cid, "/last_refine")).await?;
    let no = w.world.expect_where(reply("aucun raffinage"), 3000, "refus").await?;
    println!("  ✓ {}", no.fields().text(6).unwrap_or_default());

    step("raffinage par recette (4013)");
    w.world.send(ch, say(w.cid, &format!("/additem {BREATH} 100"))).await?;
    w.world.expect_where(|q| q.is(it, None) && q.int(3) == Some(BREATH), 3000, "pierres").await?;
    w.world.send(rf, Fields::new(rf).with(0, 6).with(1, me).with(2, 4013)).await?;
    let r = w.world.expect_where(|q| q.is(rf, Some(1)), 3000, "résultat").await?;
    anyhow::ensure!(r.int(4) == Some(0) && r.int(5) == Some(GALE), "résultat {:?}, TID {:?}", r.int(4), r.int(5));
    println!("  ✓ Gale Elemental Stone");

    step("/last_refine relance la recette");
    w.world.send(ch, say(w.cid, &format!("/additem {BREATH} 100"))).await?;
    w.world.expect_where(|q| q.is(it, None) && q.int(3) == Some(BREATH), 3000, "pierres").await?;
    w.world.send(ch, say(w.cid, "/last_refine")).await?;
    let r = w.world.expect_where(|q| q.is(rf, Some(1)), 3000, "résultat").await?;
    anyhow::ensure!(r.int(4) == Some(0) && r.int(5) == Some(GALE), "résultat {:?}, TID {:?}", r.int(4), r.int(5));
    let ok = w.world.expect_where(reply("4013"), 3000, "réponse").await?;
    println!("  ✓ réussite ; {}", ok.fields().text(6).unwrap_or_default());

    step("/last_refine sans ingrédients : résultat 1");
    w.world.send(ch, say(w.cid, "/last_refine")).await?;
    let r = w.world.expect_where(|q| q.is(rf, Some(1)), 3000, "résultat").await?;
    anyhow::ensure!(r.int(4) == Some(1), "résultat {:?}", r.int(4));
    println!("  ✓ ingrédients manquants");
    println!("\n/last_refine : OK");
    Ok(())
}
