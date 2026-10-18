//! Commandes de maître du jeu (MJ), reçues comme messages de discussion « /commande arguments ».
//!
//! Le panneau « Admin Commands » du client tape les mêmes commandes que la saisie manuelle. Le serveur
//! décide seul si une commande est permise, selon `account.gm_level`. Syntaxe : noms entre guillemets
//! doubles (espaces permis), nombres sans guillemets, la casse du nom de commande compte.

use super::{broadcast_notice, chat_ban, create_item, kill_character, rename_character, send_system, set_money, teleport, World, WorldState};
use crate::info;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq)]
pub struct GmError(pub String);

fn err<T>(msg: impl Into<String>) -> Result<T, GmError> {
    Err(GmError(msg.into()))
}

/// Découpe une ligne de commande : « texte entre guillemets » ou mots séparés par des espaces.
pub fn tokenize(input: &str) -> Result<Vec<String>, GmError> {
    let chars: Vec<char> = input.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        if chars[i] == '"' {
            let Some(end) = chars[i + 1..].iter().position(|&c| c == '"') else { return err("guillemet fermant manquant") };
            tokens.push(chars[i + 1..i + 1 + end].iter().collect());
            i += end + 2;
        } else {
            let mut j = i;
            while j < chars.len() && !chars[j].is_whitespace() && chars[j] != '"' {
                j += 1;
            }
            tokens.push(chars[i..j].iter().collect());
            i = j;
        }
    }
    Ok(tokens)
}

/// « /nom arg1 "arg 2" » → (nom, arguments) ; `None` si le texte n'est pas une commande.
pub fn parse_command(text: &str) -> Option<Result<(String, Vec<String>), GmError>> {
    let s = text.trim();
    let rest = s.strip_prefix('/').filter(|r| !r.is_empty())?;
    let (name, args) = match rest.find(char::is_whitespace) {
        Some(i) => (&rest[..i], rest[i..].trim_start()),
        None => (rest, ""),
    };
    Some(tokenize(args).map(|a| (name.to_string(), a)))
}

#[derive(Clone, Debug, PartialEq)]
pub enum Arg {
    Int(i64),
    Float(f64),
    Name(String),
    Word(String),
    Missing,
}

impl Arg {
    fn int(&self) -> Option<i64> {
        match self {
            Arg::Int(v) => Some(*v),
            _ => None,
        }
    }
    fn float(&self) -> f64 {
        match self {
            Arg::Float(v) => *v,
            Arg::Int(v) => *v as f64,
            _ => 0.0,
        }
    }
    fn name(&self) -> Option<&str> {
        match self {
            Arg::Name(v) | Arg::Word(v) => Some(v),
            _ => None,
        }
    }
}

fn is_integer(s: &str) -> bool {
    let d = s.strip_prefix(['-', '+']).unwrap_or(s);
    !d.is_empty() && d.chars().all(|c| c.is_ascii_digit())
}

fn is_number(s: &str) -> bool {
    let d = s.strip_prefix(['-', '+']).unwrap_or(s);
    let mut parts = d.splitn(2, ['.', ',']);
    let a = parts.next().unwrap_or("");
    let ok_a = !a.is_empty() && a.chars().all(|c| c.is_ascii_digit());
    match parts.next() {
        None => ok_a,
        Some(b) => ok_a && !b.is_empty() && b.chars().all(|c| c.is_ascii_digit()),
    }
}

/// Convertit les arguments selon une spécification : « name », « int », « float », « word », avec
/// « ? » pour un argument facultatif.
pub fn parse_args(spec: &[&str], tokens: &[String]) -> Result<Vec<Arg>, GmError> {
    let required = spec.iter().filter(|t| !t.ends_with('?')).count();
    if tokens.len() < required {
        return err("argument manquant");
    }
    if tokens.len() > spec.len() {
        return err("trop d'arguments");
    }
    spec.iter()
        .enumerate()
        .map(|(i, t)| {
            let Some(tok) = tokens.get(i) else { return Ok(Arg::Missing) };
            match t.trim_end_matches('?') {
                "int" => {
                    if !is_integer(tok) {
                        return err(format!("nombre entier attendu : {tok}"));
                    }
                    Ok(Arg::Int(tok.parse().map_err(|_| GmError(format!("nombre entier attendu : {tok}")))?))
                }
                "float" => {
                    if !is_number(tok) {
                        return err(format!("nombre attendu : {tok}"));
                    }
                    Ok(Arg::Float(tok.replace(',', ".").parse().unwrap_or(0.0)))
                }
                "name" => {
                    if tok.trim().is_empty() {
                        return err("nom vide");
                    }
                    Ok(Arg::Name(tok.trim().to_string()))
                }
                _ => Ok(Arg::Word(tok.clone())),
            }
        })
        .collect()
}

/// Coordonnées de /move : « x,z », « x z », « x,y,z » ou « x y z ».
pub fn parse_coordinates(tokens: &[String]) -> Result<(f64, Option<f64>, f64), GmError> {
    let joined = tokens.join(" ");
    let parts: Vec<&str> = joined.split(|c: char| c.is_whitespace() || c == ',' || c == ';').filter(|p| !p.is_empty()).collect();
    if parts.len() != 2 && parts.len() != 3 {
        return err("coordonnées attendues : x,z");
    }
    let mut nums = Vec::new();
    for p in &parts {
        let d = p.strip_prefix(['-', '+']).unwrap_or(p);
        let valid = d.split_once('.').map_or(!d.is_empty() && d.chars().all(|c| c.is_ascii_digit()), |(a, b)| {
            !a.is_empty() && !b.is_empty() && a.chars().all(|c| c.is_ascii_digit()) && b.chars().all(|c| c.is_ascii_digit())
        });
        if !valid {
            return err(format!("coordonnée invalide : {p}"));
        }
        nums.push(p.parse::<f64>().unwrap_or(0.0));
    }
    Ok(if nums.len() == 2 { (nums[0], None, nums[1]) } else { (nums[0], Some(nums[1]), nums[2]) })
}

// ---------------------------------------------------------------------------------------------
// Taux du serveur (en mémoire, valeurs initiales dans config.world.rates)
// ---------------------------------------------------------------------------------------------
pub const MAX_RATE: f64 = 100.0;

/// (clé, libellé, commande Set, commande Get)
pub const RATES: [(&str, &str, &str, &str); 6] = [
    ("exp", "expérience", "SetExp", "GetExp"),
    ("gold", "or", "SetGold", "GetGold"),
    ("drop", "butin", "SetDrop", "GetDrop"),
    ("uniqueDrop", "butin unique", "SetUniqueDrop", "GetUniqueDrop"),
    ("charisma", "charisme", "SetCharisma", "GetCharisma"),
    ("prestige", "prestige", "SetPrestigeRate", "GetPrestigeRate"),
];

#[derive(Clone, Debug)]
pub struct ServerRates(pub HashMap<String, f64>);

impl Default for ServerRates {
    fn default() -> Self {
        let mut m: HashMap<String, f64> = RATES.iter().map(|r| (r.0.to_string(), 1.0)).collect();
        m.insert("chatBuff".into(), 1.0);
        ServerRates(m)
    }
}

impl ServerRates {
    pub fn new(initial: &HashMap<String, f64>) -> Self {
        let mut r = ServerRates::default();
        for (k, v) in initial {
            if r.0.contains_key(k) && v.is_finite() {
                r.0.insert(k.clone(), *v);
            }
        }
        r
    }
    pub fn get(&self, k: &str) -> f64 {
        self.0.get(k).copied().unwrap_or(1.0)
    }
    pub fn set(&mut self, k: &str, v: f64) {
        self.0.insert(k.to_string(), v);
    }
}

fn fmt_rate(v: f64) -> String {
    if v.fract() == 0.0 { format!("{v:.1}") } else { format!("{v}") }
}

fn rate_label(label: &str) -> String {
    if label.starts_with(['a', 'e', 'é', 'i', 'o', 'u', 'A', 'E', 'I', 'O', 'U']) { format!("d'{label}") } else { format!("de {label}") }
}

pub fn format_duration(total: i64) -> String {
    let (m, s) = (total / 60, total % 60);
    if m == 0 {
        return format!("{s} seconde{}", if s > 1 { "s" } else { "" });
    }
    if s == 0 {
        return format!("{m} minute{}", if m > 1 { "s" } else { "" });
    }
    format!("{m} min {s} s")
}

// ---------------------------------------------------------------------------------------------
// Table des commandes
// ---------------------------------------------------------------------------------------------
pub struct CommandDef {
    pub name: &'static str,
    pub level: i64,
    /// `None` : arguments bruts (découpés), interprétés par la commande.
    pub args: Option<&'static [&'static str]>,
    pub usage: &'static str,
}

pub static COMMANDS: &[CommandDef] = &[
    CommandDef { name: "SetExp", level: 3, args: Some(&["float"]), usage: "/SetExp <taux>" },
    CommandDef { name: "GetExp", level: 1, args: Some(&[]), usage: "/GetExp" },
    CommandDef { name: "SetGold", level: 3, args: Some(&["float"]), usage: "/SetGold <taux>" },
    CommandDef { name: "GetGold", level: 1, args: Some(&[]), usage: "/GetGold" },
    CommandDef { name: "SetDrop", level: 3, args: Some(&["float"]), usage: "/SetDrop <taux>" },
    CommandDef { name: "GetDrop", level: 1, args: Some(&[]), usage: "/GetDrop" },
    CommandDef { name: "SetUniqueDrop", level: 3, args: Some(&["float"]), usage: "/SetUniqueDrop <taux>" },
    CommandDef { name: "GetUniqueDrop", level: 1, args: Some(&[]), usage: "/GetUniqueDrop" },
    CommandDef { name: "SetCharisma", level: 3, args: Some(&["float"]), usage: "/SetCharisma <taux>" },
    CommandDef { name: "GetCharisma", level: 1, args: Some(&[]), usage: "/GetCharisma" },
    CommandDef { name: "SetPrestigeRate", level: 3, args: Some(&["float"]), usage: "/SetPrestigeRate <taux>" },
    CommandDef { name: "GetPrestigeRate", level: 1, args: Some(&[]), usage: "/GetPrestigeRate" },
    CommandDef { name: "SetChatBuff", level: 3, args: Some(&["int"]), usage: "/SetChatBuff <0|1>" },
    CommandDef { name: "GetChatBuff", level: 1, args: Some(&[]), usage: "/GetChatBuff" },
    CommandDef { name: "chatban", level: 1, args: Some(&["name", "int"]), usage: "/chatban \"nom\" <minutes>" },
    CommandDef { name: "kill", level: 2, args: Some(&["name?"]), usage: "/kill \"nom\"" },
    CommandDef { name: "rename_char", level: 2, args: Some(&["name", "name"]), usage: "/rename_char \"ancien\" \"nouveau\"" },
    CommandDef { name: "create", level: 3, args: Some(&["name", "int", "int?", "int?"]), usage: "/create \"nom\" <TID> <quantité> <0|1>" },
    CommandDef { name: "money", level: 3, args: Some(&["int", "name?"]), usage: "/money <montant> \"nom\"" },
    CommandDef { name: "announce_restart", level: 3, args: Some(&["int", "int?"]), usage: "/announce_restart <minutes> <secondes>" },
    CommandDef { name: "move", level: 1, args: None, usage: "/move x,z" },
];

pub fn command(name: &str) -> Option<&'static CommandDef> {
    COMMANDS.iter().find(|c| c.name == name)
}

/// Niveau MJ requis pour une commande (config.world.gmCommandLevels prioritaire).
pub fn level_for(world: &World, name: &str) -> i64 {
    world.ctx.config.world.gm_command_levels.get(name).copied().or_else(|| command(name).map(|c| c.level)).unwrap_or(i64::MAX)
}

/// Exécute un texte commençant par « / ». Rend `false` si ce n'est pas une commande. Les erreurs de
/// commande sont rapportées au MJ.
pub async fn execute(world: &Arc<World>, st: &mut WorldState, sid: u32, text: &str) -> anyhow::Result<bool> {
    let Some(parsed) = parse_command(text) else { return Ok(false) };
    let gm_level = st.players.get(&sid).map(|p| p.gm_level).unwrap_or(0);
    let (name, tokens) = match parsed {
        Ok(v) => v,
        Err(e) => {
            send_system(st, sid, &format!("Commande invalide : {}", e.0));
            return Ok(true);
        }
    };
    let cmd = command(&name);
    // Un joueur sans droits ne voit pas la liste des commandes MJ.
    let Some(cmd) = cmd.filter(|_| gm_level > 0) else {
        send_system(st, sid, &format!("Commande inconnue : /{name}"));
        return Ok(true);
    };
    let required = level_for(world, &name);
    if gm_level < required {
        send_system(st, sid, &format!("Niveau MJ {required} requis pour /{name} (le vôtre : {gm_level})."));
        return Ok(true);
    }
    let args = match cmd.args {
        Some(spec) => parse_args(spec, &tokens),
        None => Ok(Vec::new()),
    };
    let result = match args {
        Ok(args) => run(world, st, sid, cmd, &args, &tokens).await?,
        Err(e) => Err(e),
    };
    match result {
        Ok(reply) => send_system(st, sid, &reply),
        Err(e) => send_system(st, sid, &format!("/{name} : {}. Syntaxe : {}", e.0, cmd.usage)),
    }
    Ok(true)
}

async fn run(world: &Arc<World>, st: &mut WorldState, sid: u32, cmd: &CommandDef, a: &[Arg], tokens: &[String]) -> anyhow::Result<Result<String, GmError>> {
    let ctx = &world.ctx;
    if let Some(r) = RATES.iter().find(|r| r.2 == cmd.name) {
        let v = a[0].float();
        if !(0.0..=MAX_RATE).contains(&v) {
            return Ok(err(format!("le taux doit être compris entre 0 et {MAX_RATE}")));
        }
        st.rates.set(r.0, v);
        return Ok(Ok(format!("Taux {} réglé à {}.", rate_label(r.1), fmt_rate(v))));
    }
    if let Some(r) = RATES.iter().find(|r| r.3 == cmd.name) {
        return Ok(Ok(format!("Taux {} : {}", rate_label(r.1), fmt_rate(st.rates.get(r.0)))));
    }
    Ok(match cmd.name {
        "SetChatBuff" => {
            let on = a[0].int().unwrap_or(-1);
            if on != 0 && on != 1 {
                return Ok(err("valeur attendue : 0 ou 1"));
            }
            st.rates.set("chatBuff", on as f64);
            Ok(if on == 1 { "Bonus de discussion activé.".into() } else { "Bonus de discussion désactivé.".into() })
        }
        "GetChatBuff" => Ok(format!("Bonus de discussion : {}", if st.rates.get("chatBuff") != 0.0 { "activé" } else { "désactivé" })),
        "chatban" => {
            let (name, minutes) = (a[0].name().unwrap_or(""), a[1].int().unwrap_or(0));
            if minutes < 0 {
                return Ok(err("durée négative"));
            }
            match chat_ban(ctx, st, name, minutes).await? {
                None => err(format!("personnage introuvable : {name}")),
                Some(t) if minutes > 0 => Ok(format!("{t} ne peut plus parler pendant {minutes} min.")),
                Some(t) => Ok(format!("{t} peut de nouveau parler.")),
            }
        }
        "kill" => {
            let target = match a[0].name() {
                Some(n) => st.by_name(n),
                None => Some(sid),
            };
            let Some(target) = target else { return Ok(err(format!("personnage absent du monde : {}", a[0].name().unwrap_or("")))) };
            kill_character(st, target);
            Ok(format!("{} a été tué.", st.players[&target].ch.name))
        }
        "rename_char" => {
            let (old, new) = (a[0].name().unwrap_or(""), a[1].name().unwrap_or(""));
            let valid = (2..=24).contains(&new.chars().count()) && !new.chars().any(|c| c.is_whitespace() || c == '"' || c == '#' || c == '\\');
            if !valid {
                return Ok(err(format!("nom invalide : {new} (2 à 24 caractères, sans espace)")));
            }
            match rename_character(ctx, st, old, new).await? {
                "missing" => err(format!("personnage introuvable : {old}")),
                "taken" => err(format!("nom déjà pris : {new}")),
                r => Ok(format!("{old} s'appelle désormais {new}{}.", if r == "online" { " (visible à sa prochaine connexion)" } else { "" })),
            }
        }
        "create" => {
            let (name, tid) = (a[0].name().unwrap_or(""), a[1].int().unwrap_or(0));
            let qty = a[2].int().unwrap_or(1);
            let bound = a[3].int() == Some(1);
            if !(1..=9999).contains(&qty) {
                return Ok(err("quantité entre 1 et 9999"));
            }
            match create_item(ctx, st, name, tid as u32, qty, bound).await? {
                Err("character") => err(format!("personnage introuvable : {name}")),
                Err("item") => err(format!("objet inconnu : TID {tid}")),
                Err(_) => err(format!("inventaire plein pour {name}")),
                Ok((item, ch)) => Ok(format!("{qty} × {item} (TID {tid}) donné(s) à {ch}{}.", if bound { ", non échangeable(s)" } else { "" })),
            }
        }
        "money" => {
            let amount = a[0].int().unwrap_or(-1);
            let target = match a[1].name() {
                Some(n) => st.by_name(n),
                None => Some(sid),
            };
            let Some(target) = target else { return Ok(err(format!("personnage absent du monde : {}", a[1].name().unwrap_or("")))) };
            if !(0..=1_000_000_000).contains(&amount) {
                return Ok(err("montant entre 0 et 1 000 000 000"));
            }
            set_money(ctx, st, target, amount).await?;
            Ok(format!("{} a maintenant {amount} pièce(s) d'or.", st.players[&target].ch.name))
        }
        "announce_restart" => {
            let (minutes, seconds) = (a[0].int().unwrap_or(0), a[1].int().unwrap_or(0));
            if minutes < 0 || seconds < 0 {
                return Ok(err("durée négative"));
            }
            let total = minutes * 60 + seconds;
            if total == 0 {
                return Ok(Ok(if cancel_restart(st) { "Redémarrage annulé.".into() } else { "Aucun redémarrage prévu.".into() }));
            }
            schedule_restart(world, st, total, seconds);
            Ok(format!("Redémarrage annoncé dans {}.", format_duration(total)))
        }
        "move" => match parse_coordinates(tokens) {
            Err(e) => Err(e),
            Ok((x, y, z)) => {
                let pos = teleport(ctx, st, sid, x, y, z).await?;
                Ok(format!("Téléporté en {}, {}.", pos.x.round(), pos.z.round()))
            }
        },
        other => err(format!("commande non implémentée : {other}")),
    })
}

/// Annonce un redémarrage dans `total` secondes ; compte à rebours sur les `last` dernières secondes.
pub fn schedule_restart(world: &Arc<World>, st: &mut WorldState, total: i64, last: i64) {
    if let Some(r) = st.restart.take() {
        r.task.abort();
    }
    let at = Instant::now() + Duration::from_secs(total as u64);
    broadcast_notice(st, &format!("Redémarrage du serveur dans {}.", format_duration(total)));
    let w = world.clone();
    let exit = world.ctx.config.world.exit_on_restart;
    let task = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let mut st = w.state.lock().await;
            let left = at.saturating_duration_since(Instant::now()).as_secs_f64().round() as i64;
            if left <= 0 {
                st.restart = None;
                broadcast_notice(&st, "Redémarrage du serveur.");
                if exit {
                    tokio::spawn(async {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        std::process::exit(0);
                    });
                }
                return;
            }
            let countdown = left <= last && (left <= 10 || left % 10 == 0);
            if countdown || (left % 60 == 0 && left != total) {
                broadcast_notice(&st, &format!("Redémarrage du serveur dans {}.", format_duration(left)));
            }
        }
    });
    st.restart = Some(super::RestartState { task });
}

pub fn cancel_restart(st: &mut WorldState) -> bool {
    let Some(r) = st.restart.take() else { return false };
    r.task.abort();
    broadcast_notice(st, "Redémarrage du serveur annulé.");
    info!("[mj] redémarrage annulé");
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoupage_et_arguments() {
        assert_eq!(tokenize(r#"  "Le Héros" 12  abc"#).unwrap(), vec!["Le Héros", "12", "abc"]);
        assert!(tokenize(r#""ouvert"#).is_err());
        let (name, args) = parse_command(r#"/create "Testeur" 190 5"#).unwrap().unwrap();
        assert_eq!(name, "create");
        let a = parse_args(&["name", "int", "int?", "int?"], &args).unwrap();
        assert_eq!(a, vec![Arg::Name("Testeur".into()), Arg::Int(190), Arg::Int(5), Arg::Missing]);
        assert!(parse_command("bonjour").is_none());
        assert_eq!(parse_args(&["float"], &["2,5".into()]).unwrap(), vec![Arg::Float(2.5)]);
        assert!(parse_args(&["int"], &["x".into()]).is_err());
        assert!(parse_args(&["int"], &[]).is_err());
    }

    #[test]
    fn coordonnees() {
        assert_eq!(parse_coordinates(&["-479091,-66202".into()]).unwrap(), (-479091.0, None, -66202.0));
        assert_eq!(parse_coordinates(&["1".into(), "2".into(), "3".into()]).unwrap(), (1.0, Some(2.0), 3.0));
        assert!(parse_coordinates(&["1".into()]).is_err());
    }

    #[test]
    fn libelles() {
        assert_eq!(format_duration(90), "1 min 30 s");
        assert_eq!(format_duration(120), "2 minutes");
        assert_eq!(format_duration(1), "1 seconde");
        assert_eq!(rate_label("expérience"), "d'expérience");
        assert_eq!(rate_label("or"), "d'or");
        assert_eq!(rate_label("butin"), "de butin");
        assert_eq!(fmt_rate(2.0), "2.0");
        assert_eq!(fmt_rate(2.5), "2.5");
    }
}
