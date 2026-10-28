//! `archcore sim nature` : horloge du monde et météo (AGPMTIMER 0x11, AgpmEventNature 0x30).
//!
//! Le MJ entre (vitesse 6, temps du jeu A), SimA 3 s plus tard (temps B) : B vaut A plus l'écoulé réel × 6, à la
//! précision de la réception près — même heure pour les deux. Météo à l'entrée : temps clair (0). `/weather rain` :
//! les deux reçoivent la pluie (3) ; SimA, reconnecté, la reçoit à l'entrée ; `/weather` sans argument la décrit ;
//! météo inconnue refusée ; `/weather clear` : les deux reçoivent 0.

use super::{enter_world, step, InWorld, Recv};
use archcore_net::schema::{self, chat_type, weather};
use archcore_net::Fields;
use std::time::Duration;

fn say(w: &InWorld, text: &str) -> Fields {
    let ch = &schema::CHATTING;
    Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text))
}

async fn gm(w: &mut InWorld, text: &str, expect: &str) -> anyhow::Result<String> {
    w.world.send(&schema::CHATTING, say(w, text)).await?;
    reply(w, text, expect).await
}

/// Réponse (message système) à une commande déjà envoyée.
async fn reply(w: &mut InWorld, text: &str, expect: &str) -> anyhow::Result<String> {
    let ch = &schema::CHATTING;
    let r = w.world.expect_where(|q| q.ptype == ch.id && q.fields().text(6).is_some_and(|t| t.contains(expect)), 3000, text).await?;
    Ok(r.fields().text(6).unwrap_or_default())
}

fn is_weather(q: &Recv) -> bool {
    q.is(&schema::EVENT_NATURE, None)
}

async fn expect_weather(w: &mut InWorld, code: i64, what: &str) -> anyhow::Result<()> {
    let r = w.world.expect_where(is_weather, 3000, what).await?;
    anyhow::ensure!(r.int(0) == Some(code), "{what} : météo {:?}", r.int(0));
    println!("  ✓ {what} : météo {code}");
    Ok(())
}

fn hour(ms: i64) -> String {
    let (h, m) = crate::world::nature::hour_minute(ms);
    format!("{h:02}:{m:02}")
}

pub async fn run(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let speed = i64::from(cfg.world.time_speed.max(1));

    step("deux connexions décalées de 3 s : même horloge");
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    anyhow::ensure!(a.time_speed == speed, "vitesse {} (attendue {speed})", a.time_speed);
    expect_weather(&mut a, weather::CLEAR, &format!("{ca} : météo à l'entrée")).await?;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let mut b = enter_world(&cfg, ub, pb, cb, "B-").await?;
    let elapsed = b.timer_at.duration_since(a.timer_at).as_millis() as i64;
    let expected = a.game_ms + elapsed * speed;
    let gap = b.game_ms - expected;
    println!("  {ca} : {} ({} ms) ; {cb} {elapsed} ms plus tard : {} ({} ms) ; écart {gap} ms de jeu", hour(a.game_ms), a.game_ms, hour(b.game_ms), b.game_ms);
    anyhow::ensure!(b.time_speed == speed && gap.abs() <= 3_000, "horloges différentes : écart {gap} ms de jeu");
    println!("  ✓ même heure du jeu (écart {gap} ms, vitesse ×{speed})");
    expect_weather(&mut b, weather::CLEAR, &format!("{cb} : météo à l'entrée")).await?;

    step("/weather rain : tous les joueurs en jeu");
    // La météo part avant la réponse à la commande.
    a.world.send(&schema::CHATTING, say(&a, "/weather rain")).await?;
    expect_weather(&mut a, weather::RAIN, ca).await?;
    reply(&mut a, "/weather rain", "Météo : pluie").await?;
    expect_weather(&mut b, weather::RAIN, cb).await?;

    step(&format!("{cb} se reconnecte : la pluie l'attend"));
    drop(b);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut b = enter_world(&cfg, ub, pb, cb, "B-").await?;
    expect_weather(&mut b, weather::RAIN, &format!("{cb} : météo à l'entrée")).await?;
    let text = gm(&mut a, "/weather", "Heure du jeu").await?;
    anyhow::ensure!(text.contains("pluie"), "réponse « {text} »");
    println!("  ✓ /weather : « {text} »");
    gm(&mut a, "/weather grêle", "météo inconnue").await?;

    step("/weather clear");
    a.world.send(&schema::CHATTING, say(&a, "/weather clear")).await?;
    expect_weather(&mut a, weather::CLEAR, ca).await?;
    reply(&mut a, "/weather clear", "Météo : temps clair").await?;
    expect_weather(&mut b, weather::CLEAR, cb).await?;
    println!("\nhorloge et météo : OK");
    Ok(())
}
