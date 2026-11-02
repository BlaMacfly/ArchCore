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
/// Niveau MJ requis pour déplacer un autre joueur avec `/move x,z "nom"`.
pub const MOVE_OTHER_LEVEL: i64 = 2;

/// Arguments de `/move` : coordonnées, puis le nom d'un autre joueur s'il y en a un (dernier mot qui n'est pas un nombre).
pub fn split_move_target(tokens: &[String]) -> (&[String], Option<&str>) {
    match tokens.split_last() {
        Some((last, rest)) if !rest.is_empty() && last.trim_start_matches(['-', '+']).parse::<f64>().is_err() && !last.contains(',') => (rest, Some(last.as_str())),
        _ => (tokens, None),
    }
}

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
/// Multiplicateur maximal de /speed.
pub const MAX_GM_SPEED: i64 = 10;

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
    CommandDef { name: "invincible", level: 2, args: Some(&["int?"]), usage: "/invincible <0|1>" },
    CommandDef { name: "rename_char", level: 2, args: Some(&["name", "name"]), usage: "/rename_char \"ancien\" \"nouveau\"" },
    CommandDef { name: "create", level: 3, args: Some(&["name", "int", "int?", "int?"]), usage: "/create \"nom\" <TID> <quantité> <0|1>" },
    CommandDef { name: "additem", level: 3, args: Some(&["int", "int?"]), usage: "/additem <TID> <quantité>" },
    CommandDef { name: "speed", level: 2, args: Some(&["int"]), usage: "/speed <1-10>" },
    CommandDef { name: "weather", level: 2, args: Some(&["word?"]), usage: "/weather clear|rain|snow" },
    CommandDef { name: "durability", level: 3, args: Some(&["int"]), usage: "/durability <valeur|-1>" },
    CommandDef { name: "questreset", level: 3, args: Some(&["int"]), usage: "/questreset <TID>" },
    CommandDef { name: "recast", level: 3, args: Some(&[]), usage: "/recast" },
    CommandDef { name: "last_refine", level: 3, args: Some(&[]), usage: "/last_refine" },
    CommandDef { name: "level", level: 3, args: Some(&["int", "name?"]), usage: "/level <niveau> \"nom\"" },
    CommandDef { name: "money", level: 3, args: Some(&["int", "name?"]), usage: "/money <montant> \"nom\"" },
    CommandDef { name: "charisma", level: 3, args: Some(&["int", "name?"]), usage: "/charisma <points> \"nom\"" },
    CommandDef { name: "announce_restart", level: 3, args: Some(&["int", "int?"]), usage: "/announce_restart <minutes> <secondes>" },
    CommandDef { name: "move", level: 1, args: None, usage: "/move x,z [\"nom\"]" },
    CommandDef { name: "anomalies", level: 1, args: Some(&["name"]), usage: "/anomalies \"nom\"" },
    CommandDef { name: "title", level: 1, args: Some(&["name?"]), usage: "/title [\"nom\"]" },
    CommandDef { name: "shrine", level: 3, args: Some(&["word?", "int?"]), usage: "/shrine [open|close <n>]" },
    CommandDef { name: "siege", level: 3, args: Some(&["word?", "word?", "word?"]), usage: "/siege [start <château> | end <château> | owner <château> <guilde ou -> | hp <château> <PV>]" },
    CommandDef { name: "tax", level: 3, args: Some(&["word?", "word?"]), usage: "/tax [pay [château]]" },
    CommandDef { name: "simloot", level: 2, args: Some(&["int", "int", "float?", "float?"]), usage: "/simloot <TID du monstre> <nombre> [taux de butin] [taux d'or]" },
];

/// Lignes d'objets affichées au plus par /simloot ; les autres sont résumées sur une dernière ligne.
pub const SIMLOOT_LINES: usize = 40;

/// Réponse de /simloot : un en-tête, puis une ligne « · quantité × nom (TID n) » par objet, du plus fréquent au plus
/// rare. L'en-tête annonce le nombre de sortes d'objets (les lignes qui suivent : au plus `SIMLOOT_LINES`, plus une
/// ligne de résumé s'il y en a davantage).
pub fn simloot_report(sim: &super::loot::Simulation, monster: &str, m: &super::loot::Source, killer_level: i32, rates: super::loot::Rates, name_of: impl Fn(u32) -> String) -> String {
    let items = sim.sorted();
    let total: i64 = items.iter().map(|i| i.1).sum();
    let mut lines = vec![format!(
        "Butin simulé : {} × {monster} (TID {}, niveau {}), taux de butin {}, taux d'or {}, tueur niveau {killer_level} : {} pièce(s) d'or, {total} objet(s) en {} sorte(s).",
        sim.kills,
        m.tid,
        m.level,
        fmt_rate(rates.drop),
        fmt_rate(rates.gold),
        sim.gold,
        items.len()
    )];
    for (tid, n) in items.iter().take(SIMLOOT_LINES) {
        lines.push(format!("· {n} × {} (TID {tid})", name_of(*tid)));
    }
    if items.len() > SIMLOOT_LINES {
        let rest = &items[SIMLOOT_LINES..];
        lines.push(format!("· … et {} autre(s) sorte(s), {} objet(s).", rest.len(), rest.iter().map(|i| i.1).sum::<i64>()));
    }
    lines.join("\n")
}

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
            if gm_level > 0 {
                send_system(st, sid, &format!("Commande invalide : {}", e.0));
            }
            return Ok(true);
        }
    };
    let cmd = command(&name);
    // Un joueur sans droits ne voit pas la liste des commandes MJ : sa commande inconnue est ignorée sans réponse
    // (les préfixes de discussion d'Archonia sont lus avant, dans chat.rs).
    let Some(cmd) = cmd.filter(|_| gm_level > 0) else {
        if gm_level > 0 {
            send_system(st, sid, &format!("Commande inconnue : /{name}"));
        } else {
            crate::debug!("[mj] /{name} d'un joueur sans droits : ignorée");
        }
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
        Ok(args) => run(world, st, sid, gm_level, cmd, &args, &tokens).await?,
        Err(e) => Err(e),
    };
    // Trace dans la base des journaux : auteur, commande, arguments, réponse.
    if let Some(p) = st.players.get(&sid) {
        let (ok, reply) = match &result {
            Ok(r) => (true, r.clone()),
            Err(e) => (false, e.0.clone()),
        };
        crate::journal::gm(&world.ctx.db, p.account.id, (p.ch.id, &p.ch.name), gm_level, &name, &tokens.join(" "), ok, &reply);
    }
    match result {
        Ok(reply) => send_system(st, sid, &reply),
        Err(e) => send_system(st, sid, &format!("/{name} : {}. Syntaxe : {}", e.0, cmd.usage)),
    }
    Ok(true)
}

/// Commandes qui agissent sur le personnage du MJ qui les tape : refusées à la console (SOAP, `world/console.rs`),
/// qui n'a pas de personnage. Les autres y passent telles quelles (avec un nom de personnage quand il est facultatif).
pub fn needs_character(name: &str, tokens: &[String]) -> bool {
    match name {
        "invincible" | "additem" | "speed" | "durability" | "questreset" | "recast" | "last_refine" => true,
        "kill" => tokens.is_empty(),
        "level" | "money" | "charisma" => tokens.len() < 2,
        "move" => split_move_target(tokens).1.is_none(),
        _ => false,
    }
}

/// Nom de l'auteur d'une commande pour le journal du serveur : son personnage, ou « la console ».
fn caller_name(st: &WorldState, sid: u32) -> String {
    st.players.get(&sid).map_or_else(|| "la console".to_string(), |p| p.ch.name.clone())
}

/// Exécute une commande dont les arguments sont déjà vérifiés. `sid` : session du MJ, ou `console::CONSOLE_SID` pour la
/// console (SOAP), qui n'a pas de personnage : `needs_character` écarte avant les commandes qui en demandent un.
pub(super) async fn run(world: &Arc<World>, st: &mut WorldState, sid: u32, gm_level: i64, cmd: &CommandDef, a: &[Arg], tokens: &[String]) -> anyhow::Result<Result<String, GmError>> {
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
        "invincible" => {
            let on = a[0].int().unwrap_or(1) != 0;
            let p = st.players.get_mut(&sid).unwrap();
            // Pas d'échéance : une année suffit.
            p.invincible_until = on.then(|| std::time::Instant::now() + std::time::Duration::from_secs(365 * 24 * 3600));
            info!("[mj] {} : invincibilité {}", p.ch.name, if on { "activée" } else { "désactivée" });
            Ok(if on { "Invincibilité activée : les monstres ne vous blessent plus.".into() } else { "Invincibilité désactivée.".into() })
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
        "additem" => {
            // Raccourci de /create sur soi : objet échangeable.
            let (tid, qty) = (a[0].int().unwrap_or(0), a[1].int().unwrap_or(1));
            if !(1..=9999).contains(&qty) {
                return Ok(err("quantité entre 1 et 9999"));
            }
            let me = st.players[&sid].ch.name.clone();
            match create_item(ctx, st, &me, tid as u32, qty, false).await? {
                Err("item") => err(format!("objet inconnu : TID {tid}")),
                Err(_) => err("inventaire plein"),
                Ok((item, _)) => Ok(format!("{qty} × {item} (TID {tid}) ajouté(s) à votre inventaire.")),
            }
        }
        "speed" => {
            let n = a[0].int().unwrap_or(0);
            if !(1..=MAX_GM_SPEED).contains(&n) {
                return Ok(err(format!("multiplicateur entre 1 et {MAX_GM_SPEED}")));
            }
            st.players.get_mut(&sid).unwrap().gm_speed = n as i32;
            super::equipment::refresh_factors(ctx, st, sid);
            let p = &st.players[&sid];
            info!("[mj] {} : vitesse de déplacement ×{n}", p.ch.name);
            Ok(format!("Vitesse de déplacement ×{n} (course {}).", p.factors.status[9]))
        }
        "weather" => match a[0].name() {
            // Sans argument : météo et heure du jeu.
            None => Ok(super::nature::describe(ctx, st)),
            Some(word) => match super::nature::parse_weather(word) {
                None => err(format!("météo inconnue : {word}")),
                Some(code) => {
                    let by = format!("/weather de {}", caller_name(st, sid));
                    super::nature::set_weather(st, code, &by);
                    Ok(format!("Météo : {}.", super::nature::weather_label(code)))
                }
            },
        },
        "durability" => {
            // Durabilité de l'équipement porté qui s'use (-1 : pleine), pour essayer l'usure et la réparation.
            let value = a[0].int().unwrap_or(-1).clamp(-1, i32::MAX as i64) as i32;
            let n = super::repair::set_worn_durability(ctx, st, sid, value).await?;
            info!("[mj] {} : durabilité de l'équipement porté à {value} ({n} objet(s))", st.players[&sid].ch.name);
            Ok(if value < 0 { format!("Durabilité pleine pour {n} objet(s) porté(s).") } else { format!("Durabilité {value} pour {n} objet(s) porté(s).") })
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
        "charisma" => {
            let points = a[0].int().unwrap_or(-1);
            let target = match a[1].name() {
                Some(n) => st.by_name(n),
                None => Some(sid),
            };
            let Some(target) = target else { return Ok(err(format!("personnage absent du monde : {}", a[1].name().unwrap_or("")))) };
            if !(1..=1_000_000).contains(&points) {
                return Ok(err("points entre 1 et 1 000 000"));
            }
            let gain = super::skills::gain_charisma(ctx, st, target, points as u32, super::skills::CharismaSource::Gm).await?;
            let p = &st.players[&target];
            info!("[mj] {} donne {gain} point(s) de charisme à {}", caller_name(st, sid), p.ch.name);
            Ok(format!("{} gagne {gain} point(s) de charisme ({} en tout).", p.ch.name, p.ch.charisma))
        }
        "questreset" => {
            let tid = a[0].int().unwrap_or(0) as u32;
            let p = st.players.get_mut(&sid).unwrap();
            let had = p.quests.done.remove(&tid) | p.quests.active.iter().any(|q| q.tid == tid);
            p.quests.active.retain(|q| q.tid != tid);
            ctx.db.characters.exec("DELETE FROM character_quests WHERE character_id = ? AND quest_tid = ?", (p.ch.id, tid)).await?;
            info!("[mj] {} : quête {tid} remise à zéro", p.ch.name);
            Ok(if had { format!("Quête {tid} remise à zéro (visible à la prochaine connexion).") } else { format!("Quête {tid} : rien à remettre à zéro.") })
        }
        "recast" => {
            let p = st.players.get_mut(&sid).unwrap();
            let n = p.skills.list.iter().filter(|s| s.ready_at.is_some()).count();
            for s in p.skills.list.iter_mut() {
                s.ready_at = None;
            }
            // Recharges des consommables à compétence et des orbes aussi (consumables.rs).
            p.uses.clear();
            Ok(format!("Délais de relance remis à zéro ({n} compétence(s))."))
        }
        "last_refine" => match st.players[&sid].last_recipe {
            None => err("aucun raffinage réussi depuis la connexion"),
            Some(id) => {
                // Même chemin que l'op 6 du client : ingrédients pris dans l'inventaire, résultat (op 1) au client.
                super::refinery::refine_recipe(ctx, st, sid, id).await?;
                Ok(format!("Recette de raffinage {id} relancée."))
            }
        },
        "level" => {
            let level = a[0].int().unwrap_or(0);
            let target = match a[1].name() {
                Some(n) => st.by_name(n),
                None => Some(sid),
            };
            let Some(target) = target else { return Ok(err(format!("personnage absent du monde : {}", a[1].name().unwrap_or("")))) };
            if !(1..=120).contains(&level) {
                return Ok(err("niveau entre 1 et 120"));
            }
            super::combat::set_level(ctx, st, target, level as i32).await?;
            let p = &st.players[&target];
            info!("[mj] {} passe {} au niveau {level}", caller_name(st, sid), p.ch.name);
            Ok(format!("{} est maintenant niveau {level} ({} point(s) de compétence).", p.ch.name, p.factors.dirt[super::skills::DIRT_SKILL_POINT]))
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
        "move" => {
            // « /move x,z "nom" » : un autre joueur en jeu (niveau MOVE_OTHER_LEVEL). C'est par là qu'on pose ailleurs un
            // joueur : le serveur refuse qu'un client se téléporte lui-même (anticheat.rs).
            let (coords, target) = split_move_target(tokens);
            let target_sid = match target {
                None => sid,
                Some(name) => {
                    if gm_level < MOVE_OTHER_LEVEL {
                        return Ok(err(format!("niveau MJ {MOVE_OTHER_LEVEL} requis pour déplacer un autre joueur")));
                    }
                    match st.by_name(name) {
                        Some(t) => t,
                        None => return Ok(err(format!("personnage absent du monde : {name}"))),
                    }
                }
            };
            match parse_coordinates(coords) {
                Err(e) => Err(e),
                Ok((x, y, z)) => {
                    let pos = teleport(ctx, st, target_sid, x, y, z).await?;
                    if target_sid == sid {
                        Ok(format!("Téléporté en {}, {}.", pos.x.round(), pos.z.round()))
                    } else {
                        Ok(format!("{} téléporté en {}, {}.", st.players[&target_sid].ch.name, pos.x.round(), pos.z.round()))
                    }
                }
            }
        }
        "anomalies" => {
            let name = a[0].name().unwrap_or("");
            match super::find_character(ctx, st, name).await? {
                None => err(format!("personnage introuvable : {name}")),
                Some((cid, exact)) => Ok(super::anticheat::describe(ctx, st, cid, &exact).await?),
            }
        }
        "title" => {
            // Titres d'un joueur en jeu (soi-même sans nom) : actif, possédés, objectifs, rang de charisme (titles.rs).
            let target = match a[0].name() {
                Some(n) => st.by_name(n),
                None => Some(sid),
            };
            match target {
                Some(t) => Ok(super::titles::describe(ctx, st, t)),
                None => err(format!("personnage absent du monde : {}", a[0].name().unwrap_or(""))),
            }
        }
        "shrine" => {
            // Sans argument : état des Shrines ; « open » / « close » : ouverture ou fermeture forcée d'un combat.
            let id = a[1].int().unwrap_or(0).clamp(0, 255) as u8;
            match a[0].name() {
                None => Ok(super::shrine::describe(ctx, st)),
                Some("open") => super::shrine::force_open(ctx, st, id, Instant::now()).map(|n| format!("{n} : combat ouvert.")).map_err(GmError),
                Some("close") => super::shrine::force_close(ctx, st, id, Instant::now()).map(|n| format!("{n} : combat fermé.")).map_err(GmError),
                Some(other) => err(format!("action inconnue : {other} (open ou close)")),
            }
        }
        "siege" => {
            // Sans argument : état des châteaux ; start, end, owner, hp : commandes d'essai des guerres de siège.
            let castle = a[1].name().unwrap_or("");
            match a[0].name() {
                None => Ok(super::siege::describe(ctx, st)),
                Some("start") => super::siege::force_start(ctx, st, castle, Instant::now()).await?.map_err(GmError),
                Some("end") => super::siege::force_end(ctx, st, castle, Instant::now()).await?.map_err(GmError),
                Some("owner") => super::siege::set_owner(ctx, st, castle, a[2].name().unwrap_or("-")).await?.map_err(GmError),
                Some("hp") => match a[2].name().and_then(|v| v.parse::<i32>().ok()) {
                    Some(hp) => super::siege::war::set_hp(ctx, st, castle, hp).map_err(GmError),
                    None => err("PV attendus : /siege hp <château> <PV>"),
                },
                Some(other) => err(format!("action inconnue : {other} (start, end, owner ou hp)")),
            }
        }
        "tax" => match a[0].name() {
            // Sans argument : taux et recettes des régions ; « pay » : versement immédiat des recettes (tax.rs).
            None => Ok(super::tax::describe(ctx, st, st.players.get(&sid).map_or(ctx.config.world.language.as_str(), |p| p.language.as_str()))),
            Some("pay") => super::tax::force_payout(ctx, st, a[1].name()).await?.map_err(GmError),
            Some(other) => err(format!("action inconnue : {other} (pay)")),
        },
        "simloot" => {
            // Simulation de butin : le tirage de la mort d'un monstre (loot.rs), répété, sans rien créer. Taux du serveur
            // par défaut ; le niveau du personnage du MJ tient lieu de niveau du tueur (bonus de niveau des groupes).
            let (tid, kills) = (a[0].int().unwrap_or(0), a[1].int().unwrap_or(0));
            if !(1..=super::loot::SIM_MAX_KILLS).contains(&kills) {
                return Ok(err(format!("nombre de monstres entre 1 et {}", super::loot::SIM_MAX_KILLS)));
            }
            let rate = |arg: &Arg, key: &str| if *arg == Arg::Missing { st.rates.get(key) } else { arg.float() };
            let rates = super::loot::Rates { drop: rate(&a[2], "drop"), gold: rate(&a[3], "gold") };
            if !(0.0..=MAX_RATE).contains(&rates.drop) || !(0.0..=MAX_RATE).contains(&rates.gold) {
                return Ok(err(format!("le taux doit être compris entre 0 et {MAX_RATE}")));
            }
            let tid = u32::try_from(tid).unwrap_or(0);
            // Niveau et nom d'un monstre de ce modèle présent dans le monde (comme la fenêtre des boss), sinon du modèle.
            let instance = st.monsters.monsters.iter().find(|m| m.tid == tid && m.owner.is_none());
            let name = instance.map(|m| m.name.clone()).or_else(|| st.monsters.names.get(&tid).cloned());
            let Some(name) = name.filter(|_| st.monsters.stats.contains_key(&tid)) else { return Ok(err(format!("monstre inconnu : TID {tid}"))) };
            let stats = st.monsters.stats_of(tid);
            let source = super::loot::Source::of(tid, instance.map(|m| m.level).unwrap_or(stats.level as i64), &stats);
            // À la console (sans personnage), le tueur a le niveau du monstre.
            let killer = st.players.get(&sid).map_or(source.level.clamp(1, i32::MAX as i64) as i32, |p| p.factors.status[7]);
            let sim = ctx.loot.simulate(&mut rand::thread_rng(), &source, killer, rates, kills as u32);
            info!("[mj] {} simule le butin de {kills} × {name} (TID {tid}) : {} pièce(s) d'or, {} sorte(s) d'objet", caller_name(st, sid), sim.gold, sim.items.len());
            Ok(simloot_report(&sim, &name, &source, killer, rates, |t| ctx.catalog.get(t).map(|d| d.name.clone()).unwrap_or_else(|| format!("objet {t}"))))
        }
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
        // Clic sur la carte : coordonnées monde à décimales, hauteur laissée au serveur (hauteur du sol).
        assert_eq!(parse_coordinates(&["-355895.69,80414.52".into()]).unwrap(), (-355895.69, None, 80414.52));
        assert_eq!(parse_coordinates(&["+12.5".into(), "-7".into()]).unwrap(), (12.5, None, -7.0));
        let t = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<String>>();
        let move_a = t(&["-479091,-66202", "SimA"]);
        assert_eq!(split_move_target(&move_a), (&move_a[..1], Some("SimA")));
        let move_b = t(&["1", "2", "3", "Le Héros"]);
        assert_eq!(split_move_target(&move_b), (&move_b[..3], Some("Le Héros")));
        let alone = t(&["1", "-3"]);
        assert_eq!(split_move_target(&alone), (&alone[..], None), "coordonnées seules");
        let one = t(&["-479091,-66202"]);
        assert_eq!(split_move_target(&one), (&one[..], None));
    }

    #[test]
    fn rapport_de_simloot() {
        use super::super::loot::{Rates, Simulation, Source};
        let m = Source { tid: 450, level: 1, ..Default::default() };
        let rates = Rates { drop: 2.0, gold: 1.0 };
        let mut sim = Simulation { kills: 500, gold: 6123, ..Default::default() };
        sim.items.insert(190, 8);
        sim.items.insert(12, 31);
        let r = simloot_report(&sim, "Green Worm", &m, 50, rates, |t| if t == 190 { "Small Life Potion".into() } else { format!("objet {t}") });
        assert_eq!(
            r.split('\n').collect::<Vec<_>>(),
            vec![
                "Butin simulé : 500 × Green Worm (TID 450, niveau 1), taux de butin 2.0, taux d'or 1.0, tueur niveau 50 : 6123 pièce(s) d'or, 39 objet(s) en 2 sorte(s).",
                "· 31 × objet 12 (TID 12)",
                "· 8 × Small Life Potion (TID 190)",
            ]
        );
        // Au-delà de SIMLOOT_LINES sortes : une ligne de résumé.
        let sim = Simulation { kills: 1, gold: 0, items: (1..=45).map(|t| (t, 1)).collect() };
        let lines: Vec<String> = simloot_report(&sim, "X", &m, 1, rates, |t| format!("o{t}")).split('\n').map(String::from).collect();
        assert_eq!(lines.len(), 1 + SIMLOOT_LINES + 1);
        assert_eq!(lines.last().unwrap(), "· … et 5 autre(s) sorte(s), 5 objet(s).");
        assert!(command("simloot").is_some_and(|c| c.level == 2));
        assert!(parse_args(command("simloot").unwrap().args.unwrap(), &["450".into(), "100".into(), "1,5".into()]).is_ok());
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

    #[test]
    fn commandes_reservees_au_jeu() {
        let t = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<String>>();
        assert!(needs_character("speed", &t(&["2"])));
        assert!(needs_character("kill", &[]) && !needs_character("kill", &t(&["Héros"])));
        assert!(needs_character("level", &t(&["50"])) && !needs_character("level", &t(&["50", "Héros"])));
        assert!(needs_character("move", &t(&["1,2"])) && !needs_character("move", &t(&["1,2", "Héros"])));
        assert!(!needs_character("SetExp", &t(&["2"])) && !needs_character("announce_restart", &t(&["5"])));
    }
}
