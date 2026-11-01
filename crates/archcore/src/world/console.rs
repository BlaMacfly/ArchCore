//! Console d'administration : commandes reçues par l'interface SOAP (`soap/`), au nom d'un compte, sans personnage.
//!
//! * Commandes de gestion façon AzerothCore / TrinityCore, que leurs outils (panneaux web, bots, scripts) envoient :
//!   `account create`, `account set password`, `account set gmlevel`, `account onlinelist`, `ban account`,
//!   `unban account`, `kick`, `announce`, `notify`, `server info`, `server shutdown|restart`, `commands`. Leurs réponses
//!   reprennent les textes anglais d'AzerothCore (« Account created: … », « Connected players: … ») que ces outils
//!   analysent.
//! * Toute autre commande passe par l'interpréteur des commandes MJ du jeu (`gm.rs`), avec ou sans « / » ou « . » et
//!   sans tenir compte de la casse ; celles qui agissent sur le personnage du MJ répondent « commande réservée au jeu ».
//! * Droits : ceux du compte authentifié (niveau MJ de `account.gm_level`), avec les niveaux par défaut de la table
//!   ci-dessous ou ceux de `GM.CommandLevel.<mots séparés par des points>` (`GM.CommandLevel.account.create = 3`).
//! * Chaque commande est notée dans `log_gm` de la base des journaux : personnage 0, nom « SOAP <adresse IP> », mots de
//!   passe masqués.
//!
//! Les commandes qui touchent au monde passent par le verrou du monde, comme un paquet de joueur ; les autres (comptes)
//! ne le prennent que pour prévenir les joueurs en jeu.

use super::{broadcast_notice, gm, send_system, World, WorldState};
use crate::db::col;
use crate::soap::{mask_secrets, Auth, Backend, Caller};
use crate::{info, warn};
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

/// Session fictive de la console : les sessions des joueurs commencent à 1 (`net.rs`).
pub const CONSOLE_SID: u32 = 0;

/// Longueur maximale d'un nom de compte créé par `account create`, et d'un mot de passe (octets Windows-1252, comme le
/// paquet de connexion du client).
pub const MAX_ACCOUNT_NAME: usize = 32;
pub const MAX_PASSWORD: usize = 64;

static STARTED: OnceLock<Instant> = OnceLock::new();
static PEAK: AtomicUsize = AtomicUsize::new(0);

/// Note l'heure de démarrage du serveur monde (`server info`).
pub fn mark_started() {
    let _ = STARTED.set(Instant::now());
}

/// Note le nombre de joueurs connectés, pour le pic de `server info`.
pub fn note_players(n: usize) {
    PEAK.fetch_max(n, Ordering::Relaxed);
}

pub struct ConsoleDef {
    pub words: &'static [&'static str],
    pub level: i64,
    pub usage: &'static str,
}

pub static CONSOLE_COMMANDS: &[ConsoleDef] = &[
    ConsoleDef { words: &["account", "create"], level: 3, usage: "account create <compte> <mot de passe> [courriel]" },
    ConsoleDef { words: &["account", "set", "password"], level: 3, usage: "account set password <compte> <mot de passe> [<mot de passe>]" },
    ConsoleDef { words: &["account", "set", "gmlevel"], level: 3, usage: "account set gmlevel <compte> <niveau> [royaume]" },
    ConsoleDef { words: &["account", "onlinelist"], level: 1, usage: "account onlinelist" },
    ConsoleDef { words: &["ban", "account"], level: 3, usage: "ban account <compte> <durée : 1d2h30m, secondes, -1 sans fin> [raison]" },
    ConsoleDef { words: &["unban", "account"], level: 3, usage: "unban account <compte>" },
    ConsoleDef { words: &["kick"], level: 2, usage: "kick <personnage> [raison]" },
    ConsoleDef { words: &["announce"], level: 2, usage: "announce <texte>" },
    ConsoleDef { words: &["notify"], level: 2, usage: "notify <texte>" },
    ConsoleDef { words: &["server", "info"], level: 1, usage: "server info" },
    ConsoleDef { words: &["server", "shutdown"], level: 3, usage: "server shutdown <délai : secondes ou 1h30m>|cancel" },
    ConsoleDef { words: &["server", "restart"], level: 3, usage: "server restart <délai : secondes ou 1h30m>|cancel" },
    ConsoleDef { words: &["commands"], level: 0, usage: "commands" },
    ConsoleDef { words: &["help"], level: 0, usage: "help" },
];

/// Commande de console désignée par les premiers mots (sans tenir compte de la casse).
pub fn find_console(words: &[&str]) -> Option<&'static ConsoleDef> {
    CONSOLE_COMMANDS.iter().find(|d| d.words.len() <= words.len() && d.words.iter().zip(words).all(|(a, b)| a.eq_ignore_ascii_case(b)))
}

fn level_of(world: &World, def: &ConsoleDef) -> i64 {
    world.ctx.config.world.gm_command_levels.get(&def.words.join(".")).copied().unwrap_or(def.level)
}

/// Texte qui suit les `n` premiers mots, tel quel (annonces, raisons).
pub fn rest_after(line: &str, n: usize) -> &str {
    let mut rest = line.trim_start();
    for _ in 0..n {
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        rest = rest[end..].trim_start();
    }
    rest.trim_end()
}

/// Durée façon AzerothCore : « 1d2h30m10s », un nombre seul en secondes ; `Some(None)` : sans fin (« -1 » ou « 0 »).
pub fn parse_duration(s: &str) -> Option<Option<u64>> {
    if s == "-1" || s == "0" {
        return Some(None);
    }
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) {
        return s.parse().ok().map(Some);
    }
    let (mut total, mut num) = (0u64, String::new());
    for c in s.chars() {
        if c.is_ascii_digit() {
            num.push(c);
            continue;
        }
        let n: u64 = num.parse().ok()?;
        num.clear();
        let unit = match c.to_ascii_lowercase() {
            'd' => 86_400,
            'h' => 3_600,
            'm' => 60,
            's' => 1,
            _ => return None,
        };
        total = total.checked_add(n.checked_mul(unit)?)?;
    }
    (num.is_empty() && total > 0).then_some(Some(total))
}

/// Durée au format de la console d'AzerothCore / TrinityCore : « 1 Day(s) 2 Hour(s) 3 Minute(s) 4 Second(s) ».
pub fn duration_text(secs: u64) -> String {
    let (d, h, m, s) = (secs / 86_400, secs / 3_600 % 24, secs / 60 % 60, secs % 60);
    let mut parts = Vec::new();
    if d > 0 {
        parts.push(format!("{d} Day(s)"));
    }
    if h > 0 {
        parts.push(format!("{h} Hour(s)"));
    }
    if m > 0 {
        parts.push(format!("{m} Minute(s)"));
    }
    if s > 0 || parts.is_empty() {
        parts.push(format!("{s} Second(s)"));
    }
    parts.join(" ")
}

/// Nom de compte accepté par `account create` : lettres et chiffres ASCII, « _ », « - », « . ».
pub fn valid_account_name(name: &str) -> Result<(), String> {
    if name.chars().count() > MAX_ACCOUNT_NAME {
        return Err(format!("Account name can't be longer than {MAX_ACCOUNT_NAME} characters, account not created!"));
    }
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')) {
        return Err("Account name contains invalid characters (letters, digits, _ - . only), account not created!".into());
    }
    Ok(())
}

fn password_hash(password: &str) -> Result<String, String> {
    let bytes = crate::tools::to_cp1252(password);
    if bytes.is_empty() || bytes.len() > MAX_PASSWORD {
        return Err(format!("Password must be 1 to {MAX_PASSWORD} characters long."));
    }
    Ok(crate::login::sha256_hex(&bytes))
}

/// Déconnecte un joueur (sans attente de reconnexion), comme la sanction de l'anti-triche.
fn disconnect(st: &mut WorldState, sid: u32, why: &str) {
    let Some(p) = st.players.get_mut(&sid) else { return };
    if p.kicked {
        return;
    }
    p.kicked = true;
    p.link.key = 0;
    info!("[console] {} déconnecté : {why}", p.ch.name);
    p.sess.close();
}

/// Compte par nom : (id, nom exact, niveau MJ).
async fn account(world: &World, name: &str) -> anyhow::Result<Option<(u32, String, i64)>> {
    let row = world.ctx.db.auth.first("SELECT id, username, gm_level FROM account WHERE username = ?", (name.to_string(),)).await?;
    Ok(row.map(|r| (col(&r, "id"), col(&r, "username"), col(&r, "gm_level"))))
}

/// Exécute une ligne de commande de console au nom de `caller` ; rend le texte de la réponse, ou le message d'échec.
pub async fn execute(world: &Arc<World>, caller: &Caller, text: &str) -> Result<String, String> {
    let line = text.trim().trim_start_matches(['.', '/']).trim_start();
    let words: Vec<&str> = line.split_whitespace().collect();
    if words.is_empty() {
        return Err("Command can not be empty".into());
    }
    let masked = mask_secrets(line);
    let (key, n, result) = match find_console(&words) {
        Some(def) => {
            let key = def.words.join(" ");
            let required = level_of(world, def);
            let result = if caller.gm_level < required {
                Err(format!("Niveau MJ {required} requis pour {key} (le vôtre : {}).", caller.gm_level))
            } else {
                let args: Vec<&str> = words[def.words.len()..].to_vec();
                match run_console(world, caller, def, &args, rest_after(line, def.words.len())).await {
                    Ok(r) => r,
                    Err(e) => {
                        warn!("[console] {key} : {e:#}");
                        Err(format!("Erreur du serveur : {e:#}"))
                    }
                }
            };
            (key, def.words.len(), result)
        }
        None => {
            let (key, result) = run_gm(world, caller, line).await;
            (key, 1, result)
        }
    };
    let (ok, reply) = match &result {
        Ok(r) => (true, r.as_str()),
        Err(e) => (false, e.as_str()),
    };
    let who = format!("SOAP {}", caller.ip);
    crate::journal::gm(&world.ctx.db, caller.account_id, (0, who.as_str()), caller.gm_level, &key, rest_after(&masked, n), ok, reply);
    result
}

/// Commande MJ du jeu (`gm.rs`) tapée à la console : (nom de la commande, résultat).
async fn run_gm(world: &Arc<World>, caller: &Caller, line: &str) -> (String, Result<String, String>) {
    let parsed = gm::parse_command(&format!("/{line}"));
    let (name, tokens) = match parsed {
        Some(Ok(v)) => v,
        Some(Err(e)) => return (line.split_whitespace().next().unwrap_or("").to_string(), Err(format!("Commande invalide : {}", e.0))),
        None => return (String::new(), Err("Command can not be empty".into())),
    };
    // Casse exacte d'abord (SetExp), puis sans tenir compte de la casse, comme les consoles d'AzerothCore.
    let cmd = gm::command(&name).or_else(|| gm::COMMANDS.iter().find(|c| c.name.eq_ignore_ascii_case(&name)));
    let Some(cmd) = cmd else { return (name.clone(), Err(format!("Commande inconnue : {name} (liste : commands)"))) };
    let key = cmd.name.to_string();
    let required = gm::level_for(world, cmd.name);
    if caller.gm_level < required {
        return (key, Err(format!("Niveau MJ {required} requis pour {} (le vôtre : {}).", cmd.name, caller.gm_level)));
    }
    if gm::needs_character(cmd.name, &tokens) {
        return (key, Err(format!("{} : commande réservée au jeu (elle agit sur le personnage du MJ). Syntaxe : {}", cmd.name, cmd.usage)));
    }
    let args = match cmd.args {
        Some(spec) => match gm::parse_args(spec, &tokens) {
            Ok(a) => a,
            Err(e) => return (key, Err(format!("{} : {}. Syntaxe : {}", cmd.name, e.0, cmd.usage))),
        },
        None => Vec::new(),
    };
    let mut st = world.state.lock().await;
    let r = gm::run(world, &mut st, CONSOLE_SID, caller.gm_level, cmd, &args, &tokens).await;
    drop(st);
    let result = match r {
        Ok(Ok(reply)) => Ok(reply),
        Ok(Err(e)) => Err(format!("{} : {}. Syntaxe : {}", cmd.name, e.0, cmd.usage)),
        Err(e) => {
            warn!("[console] {} : {e:#}", cmd.name);
            Err(format!("Erreur du serveur : {e:#}"))
        }
    };
    (key, result)
}

fn usage(def: &ConsoleDef) -> Result<String, String> {
    Err(format!("Syntaxe : {}", def.usage))
}

async fn run_console(world: &Arc<World>, caller: &Caller, def: &ConsoleDef, args: &[&str], rest: &str) -> anyhow::Result<Result<String, String>> {
    let ctx = &world.ctx;
    let key = def.words.join(" ");
    Ok(match key.as_str() {
        "account create" => {
            let (Some(name), Some(pass)) = (args.first(), args.get(1)) else { return Ok(usage(def)) };
            if args.len() > 3 {
                return Ok(usage(def));
            }
            if let Err(e) = valid_account_name(name) {
                return Ok(Err(e));
            }
            let hash = match password_hash(pass) {
                Ok(h) => h,
                Err(e) => return Ok(Err(format!("{e} Account not created!"))),
            };
            if account(world, name).await?.is_some() {
                return Ok(Err("Account with this name already exist!".into()));
            }
            if let Err(e) = ctx.db.auth.exec("INSERT INTO account (username, password_hash, gm_level) VALUES (?, ?, 0)", (name.to_string(), hash)).await {
                if format!("{e:#}").contains("Duplicate") {
                    return Ok(Err("Account with this name already exist!".into()));
                }
                return Err(e);
            }
            info!("[console] compte {name} créé par {} (SOAP {})", caller.username, caller.ip);
            Ok(format!("Account created: {name}"))
        }
        "account set password" => {
            let (Some(name), Some(pass)) = (args.first(), args.get(1)) else { return Ok(usage(def)) };
            if args.len() > 3 {
                return Ok(usage(def));
            }
            if args.get(2).is_some_and(|p| p != pass) {
                return Ok(Err("The new passwords do not match".into()));
            }
            let Some((id, exact, level)) = account(world, name).await? else { return Ok(Err(format!("Account {name} does not exist"))) };
            if level > caller.gm_level {
                return Ok(Err("Your security level is too low for this.".into()));
            }
            let hash = match password_hash(pass) {
                Ok(h) => h,
                Err(e) => return Ok(Err(e)),
            };
            ctx.db.auth.exec("UPDATE account SET password_hash = ? WHERE id = ?", (hash, id)).await?;
            info!("[console] mot de passe du compte {exact} changé par {} (SOAP {})", caller.username, caller.ip);
            Ok("The password was changed".into())
        }
        "account set gmlevel" => {
            let (Some(name), Some(level)) = (args.first(), args.get(1)) else { return Ok(usage(def)) };
            // Le royaume (dernier argument d'AzerothCore, -1 : tous) est accepté et ignoré : un seul monde.
            if args.len() > 3 || args.get(2).is_some_and(|r| r.parse::<i64>().is_err()) {
                return Ok(usage(def));
            }
            let Ok(level) = level.parse::<i64>() else { return Ok(usage(def)) };
            if !(0..=255).contains(&level) {
                return Ok(Err("Niveau MJ entre 0 et 255.".into()));
            }
            let Some((id, exact, current)) = account(world, name).await? else { return Ok(Err(format!("Account {name} does not exist"))) };
            if level > caller.gm_level || current > caller.gm_level {
                return Ok(Err("Your security level is too low for this.".into()));
            }
            ctx.db.auth.exec("UPDATE account SET gm_level = ? WHERE id = ?", (level, id)).await?;
            // Joueurs en jeu sur ce compte : droits changés tout de suite, panneau MJ du client mis à jour.
            let mut st = world.state.lock().await;
            let sids: Vec<u32> = st.players.iter().filter(|(_, p)| p.account.id == id).map(|(s, _)| *s).collect();
            for sid in &sids {
                let p = st.players.get_mut(sid).unwrap();
                p.gm_level = level;
                if level > 0 {
                    p.send_raw(super::chat::admin_level_packet(p.ch.id, &p.ch.name, level, &p.account.username));
                }
                send_system(&st, *sid, &format!("Votre niveau MJ est maintenant {level}."));
            }
            drop(st);
            info!("[console] compte {exact} : niveau MJ {current} → {level}, par {} (SOAP {})", caller.username, caller.ip);
            Ok(format!("You change security level of account {exact} to {level}."))
        }
        "account onlinelist" => {
            let st = world.state.lock().await;
            let mut rows: Vec<(String, String, String, i32, i64)> =
                st.players.values().map(|p| (p.account.username.clone(), p.ch.name.clone(), p.sess.ip().to_string(), p.factors.status[7], p.gm_level)).collect();
            drop(st);
            if rows.is_empty() {
                return Ok(Ok("Empty list".into()));
            }
            rows.sort();
            let bar = format!("-{}-", "=".repeat(78));
            let mut out = vec![bar.clone(), format!("| {:<16} | {:<24} | {:<15} | {:>5} | {:>5} |", "Account", "Character", "IP", "Level", "GMLev"), bar.clone()];
            for (acc, ch, ip, lvl, gm) in &rows {
                out.push(format!("| {acc:<16} | {ch:<24} | {ip:<15} | {lvl:>5} | {gm:>5} |"));
            }
            out.push(bar);
            Ok(out.join("\n"))
        }
        "ban account" => {
            let (Some(name), Some(duration)) = (args.first(), args.get(1)) else { return Ok(usage(def)) };
            let Some(secs) = parse_duration(duration) else { return Ok(Err(format!("Durée invalide : {duration}. {}", def.usage))) };
            let reason = match rest_after(rest, 2) {
                "" => "aucune raison donnée",
                r => r,
            };
            let reason: String = reason.chars().take(255).collect();
            let Some((id, exact, level)) = account(world, name).await? else { return Ok(Err(format!("Account {name} does not exist"))) };
            if level > caller.gm_level {
                return Ok(Err("Your security level is too low for this.".into()));
            }
            let by: String = format!("SOAP {}", caller.username).chars().take(48).collect();
            let s = secs.unwrap_or(0);
            ctx.db
                .auth
                .exec("INSERT INTO account_banned (account_id, banned_until, banned_by, reason) VALUES (?, IF(? > 0, NOW() + INTERVAL ? SECOND, NULL), ?, ?)", (id, s, s, by, reason.clone()))
                .await?;
            let mut st = world.state.lock().await;
            let sids: Vec<u32> = st.players.iter().filter(|(_, p)| p.account.id == id).map(|(s, _)| *s).collect();
            for sid in sids {
                disconnect(&mut st, sid, &format!("compte banni ({reason})"));
            }
            drop(st);
            info!("[console] compte {exact} banni {} par {} (SOAP {}) : {reason}", secs.map(duration_text).unwrap_or_else(|| "sans fin".into()), caller.username, caller.ip);
            Ok(match secs {
                Some(s) => format!("{exact} is banned for {}. Reason: {reason}.", duration_text(s)),
                None => format!("{exact} is banned permanently. Reason: {reason}."),
            })
        }
        "unban account" => {
            let Some(name) = args.first().filter(|_| args.len() == 1) else { return Ok(usage(def)) };
            let Some((id, exact, _)) = account(world, name).await? else { return Ok(Err(format!("Account {name} does not exist"))) };
            // Même condition que la connexion (login.rs) : table account_banned, ou ancienne colonne banned_until.
            let row = ctx
                .db
                .auth
                .first(
                    "SELECT ((SELECT banned_until FROM account WHERE id = ?) > NOW() \
                     OR EXISTS (SELECT 1 FROM account_banned b WHERE b.account_id = ? AND b.active = 1 AND (b.banned_until IS NULL OR b.banned_until > NOW()))) AS banned",
                    (id, id),
                )
                .await?;
            if row.map(|r| col::<i64>(&r, "banned")).unwrap_or(0) == 0 {
                return Ok(Err(format!("{exact} is not banned.")));
            }
            ctx.db.auth.atomic(vec![
                ("UPDATE account_banned SET active = 0 WHERE account_id = ? AND active = 1", (id,).into()),
                ("UPDATE account SET banned_until = NULL WHERE id = ?", (id,).into()),
            ])
            .await?;
            info!("[console] compte {exact} débanni par {} (SOAP {})", caller.username, caller.ip);
            Ok(format!("{exact} unbanned."))
        }
        "kick" => {
            let Some(name) = args.first() else { return Ok(usage(def)) };
            let reason = rest_after(rest, 1);
            let mut st = world.state.lock().await;
            let Some(sid) = st.by_name(name) else { return Ok(Err("Player not found!".into())) };
            if st.players[&sid].gm_level > caller.gm_level {
                return Ok(Err("Your security level is too low for this.".into()));
            }
            let exact = st.players[&sid].ch.name.clone();
            let why = if reason.is_empty() { String::new() } else { format!(" : {reason}") };
            send_system(&st, sid, &format!("Vous êtes déconnecté par un MJ{why}."));
            disconnect(&mut st, sid, &format!("kick de {} (SOAP {}){why}", caller.username, caller.ip));
            Ok(format!("Player {exact} kicked."))
        }
        "announce" | "notify" => {
            if rest.is_empty() {
                return Ok(usage(def));
            }
            let text: String = rest.chars().take(200).collect();
            let st = world.state.lock().await;
            broadcast_notice(&st, &text);
            // Pas de sortie, comme AzerothCore.
            Ok(String::new())
        }
        "server info" => {
            let st = world.state.lock().await;
            let (connected, in_world) = (st.players.len(), st.players.values().filter(|p| p.in_game).count());
            drop(st);
            note_players(connected);
            let uptime = STARTED.get().map(|t| t.elapsed().as_secs()).unwrap_or(0);
            let profile = if cfg!(debug_assertions) { "Debug" } else { "Release" };
            Ok([
                format!("ArchCore rev. {} ({}, {profile})", env!("CARGO_PKG_VERSION"), std::env::consts::OS),
                format!("Connected players: {connected}. Characters in world: {in_world}."),
                format!("Connection peak: {}.", PEAK.load(Ordering::Relaxed)),
                format!("Server uptime: {}", duration_text(uptime)),
            ]
            .join("\n"))
        }
        "server shutdown" | "server restart" => {
            let Some(arg) = args.first().filter(|_| args.len() == 1) else { return Ok(usage(def)) };
            let what = def.words[1];
            let mut st = world.state.lock().await;
            if arg.eq_ignore_ascii_case("cancel") {
                return Ok(if gm::cancel_restart(&mut st) { Ok(format!("Server {what} cancelled.")) } else { Err(format!("No server {what} in progress.")) });
            }
            let Some(Some(secs)) = parse_duration(arg) else { return Ok(usage(def)) };
            let total = secs.min(7 * 86_400) as i64;
            // Même compte à rebours que /announce_restart ; l'arrêt dépend de World.ExitOnRestart.
            gm::schedule_restart(world, &mut st, total, total.min(60));
            drop(st);
            info!("[console] {what} du serveur dans {total} s, par {} (SOAP {})", caller.username, caller.ip);
            let note = if ctx.config.world.exit_on_restart { "" } else { " (World.ExitOnRestart = 0 : annonce seulement, le serveur ne s'arrête pas)" };
            Ok(format!("Server {what} in {}.{note}", duration_text(total as u64)))
        }
        "commands" | "help" => {
            let mut out = vec!["Commandes de console :".to_string()];
            for d in CONSOLE_COMMANDS.iter().filter(|d| level_of(world, d) <= caller.gm_level) {
                out.push(format!("  {}", d.usage));
            }
            let mut names: Vec<&str> = gm::COMMANDS.iter().filter(|c| gm::level_for(world, c.name) <= caller.gm_level).map(|c| c.name).collect();
            names.sort_unstable_by_key(|n| n.to_ascii_lowercase());
            out.push(format!("Commandes MJ (syntaxe de la page Administration du wiki, sans « / ») : {}", names.join(", ")));
            Ok(out.join("\n"))
        }
        other => Err(format!("commande de console non implémentée : {other}")),
    })
}

/// Comptes et commandes du serveur pour l'interface SOAP.
pub struct SoapBackend(pub Arc<World>);

#[async_trait]
impl Backend for SoapBackend {
    async fn authenticate(&self, username: &str, password: &str) -> anyhow::Result<Auth> {
        // Même vérification que la connexion au jeu (login.rs) : SHA-256 du mot de passe en Windows-1252, bannissements.
        let row = self
            .0
            .ctx
            .db
            .auth
            .first(
                "SELECT id, username, password_hash, gm_level, \
                 ((banned_until IS NOT NULL AND banned_until > NOW()) \
                  OR EXISTS (SELECT 1 FROM account_banned b WHERE b.account_id = account.id AND b.active = 1 \
                             AND (b.banned_until IS NULL OR b.banned_until > NOW()))) AS banned \
                 FROM account WHERE username = ?",
                (username.to_string(),),
            )
            .await?;
        let Some(row) = row else { return Ok(Auth::Unknown) };
        if col::<String>(&row, "password_hash").to_lowercase() != crate::login::sha256_hex(&crate::tools::to_cp1252(password)) {
            return Ok(Auth::BadPassword);
        }
        if col::<i64>(&row, "banned") != 0 {
            return Ok(Auth::Banned);
        }
        Ok(Auth::Granted { account_id: col(&row, "id"), username: col(&row, "username"), gm_level: col(&row, "gm_level") })
    }

    async fn execute(&self, caller: &Caller, command: &str) -> Result<String, String> {
        execute(&self.0, caller, command).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commandes_de_console() {
        assert_eq!(find_console(&["Account", "SET", "gmlevel", "bob", "3"]).map(|d| d.usage), Some("account set gmlevel <compte> <niveau> [royaume]"));
        assert_eq!(find_console(&["server", "info"]).map(|d| d.level), Some(1));
        assert!(find_console(&["account", "set"]).is_none());
        assert!(find_console(&["SetExp", "2"]).is_none(), "commande MJ du jeu");
        // Aucune commande de console ne masque une commande MJ du jeu.
        assert!(CONSOLE_COMMANDS.iter().all(|d| gm::COMMANDS.iter().all(|c| !c.name.eq_ignore_ascii_case(d.words[0]))));
        assert_eq!(rest_after("  announce   Bonjour  à   tous ", 1), "Bonjour  à   tous");
        assert_eq!(rest_after("kick Bob", 2), "");
    }

    #[test]
    fn durees() {
        assert_eq!(parse_duration("-1"), Some(None));
        assert_eq!(parse_duration("90"), Some(Some(90)));
        assert_eq!(parse_duration("1d2h30m10s"), Some(Some(86_400 + 7_200 + 1_800 + 10)));
        assert_eq!(parse_duration("2H"), Some(Some(7_200)));
        assert_eq!(parse_duration("1x"), None);
        assert_eq!(parse_duration("1d5"), None);
        assert_eq!(parse_duration("h"), None);
        assert_eq!(parse_duration(""), None);
        assert_eq!(duration_text(0), "0 Second(s)");
        assert_eq!(duration_text(3_600), "1 Hour(s)");
        assert_eq!(duration_text(93_784), "1 Day(s) 2 Hour(s) 3 Minute(s) 4 Second(s)");
    }

    #[test]
    fn noms_et_mots_de_passe() {
        assert!(valid_account_name("Joueur_01").is_ok());
        assert!(valid_account_name("a b").is_err() && valid_account_name("").is_err() && valid_account_name("é").is_err());
        assert!(valid_account_name(&"a".repeat(33)).unwrap_err().starts_with("Account name can't be longer than 32"));
        // Même empreinte que db-init et la connexion : SHA-256 des octets Windows-1252.
        assert_eq!(password_hash("abc").unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(password_hash("é").unwrap(), crate::login::sha256_hex(&[0xe9]));
        assert!(password_hash("").is_err() && password_hash(&"x".repeat(65)).is_err());
    }
}
