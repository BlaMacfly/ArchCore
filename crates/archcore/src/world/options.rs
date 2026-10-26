//! Options des objets (`itemoptiontable.txt`) : caractéristiques en plus, tirées à la création d'un équipement.
//!
//! ```text
//!   Option : TID, type (« Type » : une option au plus par type sur un objet), parties admises (Upper_Body,
//!            Lower_Body, Weapon, Shield, Head, Ring, Necklace, Shoes, Gloves), rangs (« Rank_Min » — 0 vaut 1 —,
//!            « Rank_Max »), niveau minimal de la source (« Level_limit »), poids (« Probability », 0 au-delà de
//!            100 000), colonnes de caractéristiques. Une cellule « Refined » ou « Gacha » non vide (même « 0 »),
//!            ou un « CategoryName », range l'option dans une autre famille : elle ne tombe pas avec le butin.
//!   Tirage : nombre d'options entre « MinOption » et « MaxOption » de l'objet, pondéré par
//!            optionnumdroprate.txt (colonne = MaxOption) ; puis, tant qu'il en manque, un type tiré au hasard
//!            parmi ceux qui ont une option admise pour la partie, le rang de l'objet et le niveau de la source
//!            (monstre qui le lâche), et une option de ce type au poids. Sockets de même, entre « MinSocket » et
//!            « MaxSocket », pondérés par socketnumdroprate.txt.
//!   Effet : chaque colonne de caractéristique vise un facteur (type, sous-type, attribut) ; « PointType » 0
//!            ajoute la valeur, 1 l'applique en pour cent ; ATK_Speed_bonus, Movement_Run_bonus et les
//!            « *_Percent » sont toujours en pour cent. Le client ajoute lui-même les options aux facteurs de
//!            l'objet pour l'affichage ; ceux du personnage viennent du serveur, qui ajoute les options portées.
//!            Options à déclenchement (« skill_type ») : 1 critique, 2 étourdissement, 3 coup reçu changé en PV,
//!            4 régénération, 5 blessure, 6 esquive ; avec « Ignore_Physical_Defense_Rate » et
//!            « Ignore_Attribute_Defense_Rate », elles forment les effets de combat de l'équipement (`Gear`), joués
//!            par le combat. Les colonnes bonus_*, on_hit_*… ne sont pas appliquées.
//!   Options liées : un objet unique cite des groupes (« Link_id » d'itemdatatable.txt) ; chaque groupe donne
//!            l'option dont « Level_min » ≤ niveau du porteur ≤ « Level_max », ajoutée comme les autres.
//!   Réseau : AGPMITEM champ 20, mini-paquet de 5 UINT16 (TID des options).
//! ```
//! Règles relevées dans `Archonia.exe` (lecteur de la table, application des options, tirage partagé).

use super::items::EquipData;
use rand::Rng;
use std::collections::HashMap;

/// Options au plus sur un objet.
pub const MAX_OPTIONS: usize = 5;

/// Colonnes des parties dans la table, dans l'ordre de `part_slot`.
const PART_COLUMNS: [&str; 9] = ["Upper_Body", "Lower_Body", "Weapon", "Shield", "Head", "Ring", "Necklace", "Shoes", "Gloves"];

/// Colonnes qui décrivent l'option, pas ses effets.
const META_COLUMNS: [&str; 25] = [
    "tid", "Description", "Upper_Body", "Lower_Body", "Weapon", "Shield", "Head", "Ring", "Necklace", "Shoes", "Gloves", "Refined", "Gacha", "Type",
    "CategoryName", "PointType", "Item_Level_Min", "Item_Level_Max", "Level_limit", "Rank_Min", "Rank_Max", "Probability", "Link_id", "Level_min", "Level_max",
];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemOption {
    pub tid: u16,
    pub kind: i64,
    /// Bits des parties admises (indices de `PART_COLUMNS`).
    pub parts: u16,
    pub refined: bool,
    pub gacha: bool,
    pub rank_min: i64,
    pub rank_max: i64,
    /// Niveau minimal de la source (« Level_limit »).
    pub level_limit: i64,
    /// Famille à part (« CategoryName » renseigné).
    pub category: bool,
    /// Groupe d'options liées (« Link_id ») et niveaux du porteur où celle-ci vaut (« Level_min », « Level_max »).
    pub link: i64,
    pub link_level: (i64, i64),
    pub probability: u64,
    /// « PointType » 1 : valeurs en pour cent.
    pub percent: bool,
    /// Colonnes d'effet non nulles : (nom, valeur).
    pub stats: Vec<(String, f64)>,
}

impl ItemOption {
    /// Peut tomber sur un objet de ce rang, lâché par une source de ce niveau.
    fn suits(&self, rank: i64, level: i64) -> bool {
        let rank = rank.max(1);
        self.probability > 0 && rank >= self.rank_min.max(1) && rank <= self.rank_max && level >= self.level_limit
    }
}

/// Tableau « nombre obtenu × valeur maximale » d'optionnumdroprate.txt ou socketnumdroprate.txt :
/// `rates[max][n]`, poids d'obtenir `n` quand le maximum vaut `max`.
pub type CountRates = Vec<Vec<u64>>;

#[derive(Clone, Debug, Default)]
pub struct OptionCatalog {
    pub options: HashMap<u16, ItemOption>,
    /// Options du butin par (type, partie).
    pool: HashMap<(i64, usize), Vec<u16>>,
    /// Options liées par groupe (« Link_id »).
    links: HashMap<i64, Vec<u16>>,
    pub option_rates: CountRates,
    pub socket_rates: CountRates,
}

fn cell<'a>(header: &[String], row: &'a [String], name: &str) -> &'a str {
    header.iter().position(|h| h == name).and_then(|i| row.get(i)).map(|c| c.trim()).unwrap_or("")
}

fn num(s: &str) -> f64 {
    s.parse::<f64>().unwrap_or(0.0)
}

/// Lignes tabulées non vides, sans retours chariot.
fn rows(text: &str) -> Vec<Vec<String>> {
    text.lines().map(|l| l.trim_end_matches('\r').split('\t').map(str::to_string).collect::<Vec<_>>()).filter(|r| r.iter().any(|c| !c.trim().is_empty())).collect()
}

/// Lit optionnumdroprate.txt ou socketnumdroprate.txt : en-tête, ligne des maximums, puis une ligne par nombre.
pub fn parse_count_rates(text: &str) -> CountRates {
    let mut out: CountRates = Vec::new();
    for r in rows(text).iter().skip(2) {
        let n = num(r.first().map(String::as_str).unwrap_or("")) as usize;
        if n == 0 {
            continue;
        }
        for (max, c) in r.iter().enumerate().skip(1) {
            if max >= out.len() {
                out.resize(max + 1, Vec::new());
            }
            if n >= out[max].len() {
                out[max].resize(n + 1, 0);
            }
            out[max][n] = num(c.trim()) as u64;
        }
    }
    out
}

/// Nombre tiré entre `min` et `max` selon `rates` ; `min` si l'intervalle est vide ou la table muette.
pub fn roll_count(rates: &CountRates, min: i64, max: i64, rng: &mut impl Rng) -> i64 {
    if min <= 0 || min >= max {
        return min.max(0);
    }
    let w = |n: i64| rates.get(max as usize).and_then(|r| r.get(n as usize)).copied().unwrap_or(0);
    let total: u64 = (min..=max).map(w).sum();
    if total == 0 {
        return min;
    }
    let mut r = rng.gen_range(0..total);
    for n in (min..=max).rev() {
        if r < w(n) {
            return n;
        }
        r -= w(n);
    }
    min
}

/// Case de partie d'un équipement pour les options (`None` : il n'en reçoit pas). Une arme tenue à gauche
/// (arc, dague) prend les options d'arme.
pub fn part_slot(e: &EquipData) -> Option<usize> {
    match e.part {
        2 => Some(0),
        6 => Some(1),
        10 => Some(2),
        9 if e.kind == super::equipment::KIND_WEAPON => Some(2),
        9 => Some(3),
        3 => Some(4),
        11 | 12 => Some(5),
        13 => Some(6),
        7 => Some(7),
        5 => Some(8),
        _ => None,
    }
}

impl OptionCatalog {
    /// Tables importées dans `client_table` (`archcore import`) ; vides si elles manquent.
    pub async fn load(db: &crate::db::Databases) -> anyhow::Result<Self> {
        let mut text = HashMap::new();
        for r in db.world.rows("SELECT name, data FROM client_table WHERE row_idx = 0 AND name IN ('itemoptiontable.txt', 'optionnumdroprate.txt', 'socketnumdroprate.txt')", ()).await? {
            let name: String = crate::db::col(&r, "name");
            text.insert(name, serde_json::from_str::<String>(&crate::db::col::<String>(&r, "data")).unwrap_or_default());
        }
        let t = |n: &str| text.get(n).map(String::as_str).unwrap_or("");
        let c = Self::parse(t("itemoptiontable.txt"), t("optionnumdroprate.txt"), t("socketnumdroprate.txt"));
        crate::info!("options d'objets : {} option(s), {} groupe(s) de butin, tables de nombre {}", c.options.len(), c.pool.len(), if c.option_rates.is_empty() { "absentes" } else { "chargées" });
        Ok(c)
    }

    pub fn parse(table: &str, option_rates: &str, socket_rates: &str) -> Self {
        let all = rows(table);
        let Some((header, body)) = all.split_first() else { return Self::default() };
        let header: Vec<String> = header.iter().map(|h| h.trim().to_string()).collect();
        let mut c = OptionCatalog { option_rates: parse_count_rates(option_rates), socket_rates: parse_count_rates(socket_rates), ..Default::default() };
        for r in body {
            let Ok(tid) = cell(&header, r, "tid").parse::<u16>() else { continue };
            let mut parts = 0u16;
            for (i, p) in PART_COLUMNS.iter().enumerate() {
                if !cell(&header, r, p).is_empty() {
                    parts |= 1 << i;
                }
            }
            let stats = header
                .iter()
                .zip(r.iter())
                .filter(|(h, _)| !h.is_empty() && !META_COLUMNS.contains(&h.as_str()))
                .filter_map(|(h, v)| v.trim().parse::<f64>().ok().filter(|x| *x != 0.0).map(|x| (h.clone(), x)))
                .collect();
            let o = ItemOption {
                tid,
                kind: num(cell(&header, r, "Type")) as i64,
                parts,
                refined: !cell(&header, r, "Refined").is_empty(),
                gacha: !cell(&header, r, "Gacha").is_empty(),
                rank_min: num(cell(&header, r, "Rank_Min")) as i64,
                rank_max: num(cell(&header, r, "Rank_Max")) as i64,
                level_limit: num(cell(&header, r, "Level_limit")) as i64,
                category: !cell(&header, r, "CategoryName").is_empty(),
                link: num(cell(&header, r, "Link_id")) as i64,
                link_level: (num(cell(&header, r, "Level_min")) as i64, num(cell(&header, r, "Level_max")) as i64),
                probability: Some(num(cell(&header, r, "Probability")) as u64).filter(|p| *p <= 100_000).unwrap_or(0),
                percent: !matches!(cell(&header, r, "PointType"), "" | "0"),
                stats,
            };
            if o.kind > 0 && o.probability > 0 && !o.refined && !o.gacha && !o.category {
                for slot in (0..PART_COLUMNS.len()).filter(|i| parts & (1 << i) != 0) {
                    c.pool.entry((o.kind, slot)).or_default().push(tid);
                }
            }
            if o.link > 0 {
                c.links.entry(o.link).or_default().push(tid);
            }
            c.options.insert(tid, o);
        }
        for list in c.pool.values_mut().chain(c.links.values_mut()) {
            list.sort_unstable();
        }
        c
    }

    /// Option du groupe lié `link` pour un porteur de niveau `level` (« Level_min » ≤ niveau ≤ « Level_max »).
    pub fn linked(&self, link: i64, level: i64) -> Option<u16> {
        self.links.get(&link)?.iter().copied().find(|t| self.options.get(t).is_some_and(|o| level >= o.link_level.0 && level <= o.link_level.1))
    }

    /// Options tirées pour un équipement neuf lâché par une source de niveau `level` (1 à 120) : nombre selon
    /// `MinOption`/`MaxOption`, un type au plus de chaque, chaque option au poids parmi celles de son type qui
    /// conviennent à la partie, au rang et au niveau.
    pub fn roll_options(&self, e: &EquipData, level: i64, rng: &mut impl Rng) -> Vec<u16> {
        let count = roll_count(&self.option_rates, e.min_option, e.max_option, rng);
        self.pick_options(e, level, count, rng)
    }

    /// `count` options (5 au plus), une par type, tirées comme celles du butin.
    pub fn pick_options(&self, e: &EquipData, level: i64, count: i64, rng: &mut impl Rng) -> Vec<u16> {
        let Some(slot) = part_slot(e) else { return Vec::new() };
        if !(1..=120).contains(&level) {
            return Vec::new();
        }
        let mut count = count.min(MAX_OPTIONS as i64);
        let mut kinds: Vec<(i64, Vec<&ItemOption>)> = {
            let mut by_kind: HashMap<i64, Vec<&ItemOption>> = HashMap::new();
            for ((kind, s), tids) in &self.pool {
                if *s != slot {
                    continue;
                }
                let ok: Vec<&ItemOption> = tids.iter().filter_map(|t| self.options.get(t)).filter(|o| o.suits(e.rank, level)).collect();
                if !ok.is_empty() {
                    by_kind.insert(*kind, ok);
                }
            }
            let mut v: Vec<_> = by_kind.into_iter().collect();
            v.sort_by_key(|(k, _)| *k);
            v
        };
        let mut out = Vec::new();
        while count > 0 && !kinds.is_empty() {
            let (_, choices) = kinds.swap_remove(rng.gen_range(0..kinds.len()));
            let total: u64 = choices.iter().map(|o| o.probability).sum();
            let mut r = rng.gen_range(0..total);
            for o in choices {
                if r < o.probability {
                    out.push(o.tid);
                    count -= 1;
                    break;
                }
                r -= o.probability;
            }
        }
        out
    }

    /// Sockets d'un équipement neuf, entre `MinSocket` et `MaxSocket`.
    pub fn roll_sockets(&self, e: &EquipData, rng: &mut impl Rng) -> u8 {
        roll_count(&self.socket_rates, e.min_socket, e.max_socket, rng).clamp(0, super::convert::MAX_SOCKETS as i64) as u8
    }
}

/// Options et sockets tirés pour un objet neuf du modèle `tid`, lâché par une source de niveau `level` ;
/// rien pour ce qui ne se porte pas.
pub fn generate(catalog: &super::items::ItemCatalog, options: &OptionCatalog, tid: u32, level: i64, rng: &mut impl Rng) -> (Vec<u16>, u8) {
    match catalog.get(tid).and_then(|d| d.equip.as_ref()) {
        Some(e) => (options.roll_options(e, level, rng), options.roll_sockets(e, rng)),
        None => (Vec::new(), 0),
    }
}

/// Facteur visé par une colonne d'option.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Target {
    /// Caractéristique (FOR, DEX… ; stockées × 100) ou vitesse de déplacement.
    Status(usize),
    PointMax(usize),
    /// Dégâts min et max d'un attribut.
    Damage(usize),
    DefensePoint(usize),
    DefenseRate(usize),
    Attack(usize),
}

/// Colonnes d'effet appliquées : (colonne, facteurs visés, toujours en pour cent).
const EFFECTS: &[(&str, &[Target], bool)] = {
    use Target::*;
    &[
        ("PhysicalDamage", &[Damage(0)], false),
        ("MagicDamage", &[Damage(1)], false),
        ("WaterDamage", &[Damage(2)], false),
        ("FireDamage", &[Damage(3)], false),
        ("EarthDamage", &[Damage(4)], false),
        ("AirDamage", &[Damage(5)], false),
        ("PoisonDamage", &[Damage(6)], false),
        ("LinghtningDamage", &[Damage(7)], false),
        ("IceDamage", &[Damage(8)], false),
        ("Physical_Defense_Point", &[DefensePoint(0)], false),
        ("Magic_Defense_Point", &[DefensePoint(1)], false),
        ("Water_Defense_Point", &[DefensePoint(2)], false),
        ("Fire_Defense_Point", &[DefensePoint(3)], false),
        ("Earth_Defense_Point", &[DefensePoint(4)], false),
        ("Air_Defense_Point", &[DefensePoint(5)], false),
        ("Poison_Defense_Point", &[DefensePoint(6)], false),
        ("Linghtning_Defense_Point", &[DefensePoint(7)], false),
        ("Ice_Defense_Point", &[DefensePoint(8)], false),
        ("PhysicalAttackResist", &[DefenseRate(0)], false),
        ("Magic Res.", &[DefenseRate(1)], false),
        ("Water Res.", &[DefenseRate(2)], false),
        ("Fire Res.", &[DefenseRate(3)], false),
        ("Earth Res.", &[DefenseRate(4)], false),
        ("Air Res.", &[DefenseRate(5)], false),
        ("Poison Res.", &[DefenseRate(6)], false),
        ("Thunder Res.", &[DefenseRate(7)], false),
        ("Ice Res.", &[DefenseRate(8)], false),
        ("BlockRate", &[DefenseRate(9)], false),
        ("SkillBlockRate", &[DefenseRate(10)], false),
        ("Hp_Bonus_Point", &[PointMax(0)], false),
        ("Mp_Bonus_Point", &[PointMax(1)], false),
        ("Hp_Bonus_Percent", &[PointMax(0)], true),
        ("Mp_Bonus_Percent", &[PointMax(1)], true),
        ("Physical_ATKRate", &[PointMax(8)], false),
        ("Magic_ATKRate", &[PointMax(10)], false),
        ("Con_Bonus_Point", &[Status(0)], false),
        ("Str_Bonus_Point", &[Status(1)], false),
        ("Int_Bonus_Point", &[Status(2)], false),
        ("Dex_Bonus_Point", &[Status(3)], false),
        ("Wis_Bonus_Point", &[Status(6)], false),
        ("Movement_Run_bonus", &[Status(8), Status(9)], true),
        ("ATK_Range_bonus", &[Attack(0)], false),
        ("ATK_Speed_bonus", &[Attack(2)], true),
    ]
};

/// Sortes d'options à déclenchement (colonne « skill_type »).
pub mod trigger {
    /// Coup critique : chance « skill_rate », dégâts « Critical » (pour cent d'un coup normal) ; sans chance,
    /// « Critical » s'ajoute aux dégâts critiques.
    pub const CRITICAL: i64 = 1;
    /// Étourdit la cible d'un coup : chance, durée « stun_time ».
    pub const STUN: i64 = 2;
    /// Change une part d'un coup reçu en PV : chance, part « damage_convert_hp ».
    pub const DRAIN: i64 = 3;
    /// Rend « regen_hp » PV et « regen_mp » PM toutes les 10 s.
    pub const REGEN: i64 = 4;
    /// Blesse la cible d'un coup : chance, dégâts de la colonne de leur élément chaque seconde pendant « duration ».
    pub const WOUND: i64 = 5;
    /// Esquive (« evade_rate ») : chance d'éviter un coup.
    pub const EVADE: i64 = 6;
}

/// Colonnes de dégâts d'une option qui blesse, et leur indice dans le résultat d'un coup.
const WOUND_COLUMNS: [(&str, usize); 9] = [
    ("PhysicalDamage", 9),
    ("MagicDamage", 10),
    ("WaterDamage", 11),
    ("FireDamage", 12),
    ("EarthDamage", 13),
    ("AirDamage", 14),
    ("PoisonDamage", 15),
    ("LinghtningDamage", 16),
    ("IceDamage", 17),
];

/// Dégâts sur la durée qu'une option pose sur la cible d'un coup.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Wound {
    pub rate: f64,
    pub duration_ms: u64,
    /// (indice dans le résultat, dégâts par seconde).
    pub damages: Vec<(usize, i32)>,
}

/// Effets de combat de l'équipement porté : options à déclenchement, défense ignorée, compétences des runes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Gear {
    /// Critiques, coups reçus changés en PV, esquive, défense ignorée.
    pub mods: super::effects::Mods,
    /// Étourdissements de la cible d'un coup : (chance, durée en ms).
    pub stuns: Vec<(f64, u64)>,
    pub wounds: Vec<Wound>,
    /// Rendus toutes les 10 s, en combat aussi.
    pub regen_hp: i32,
    pub regen_mp: i32,
    /// Compétences des runes serties (« Skill_Name », « Skill_Level », « Skill_Rate ») : (TID, niveau, chance).
    pub rune_skills: Vec<(u32, u32, f64)>,
}

impl ItemOption {
    fn stat(&self, column: &str) -> f64 {
        self.stats.iter().find(|(c, _)| c == column).map(|(_, v)| *v).unwrap_or(0.0)
    }
    /// Option à déclenchement (« skill_type ») : ses colonnes de dégâts ne s'ajoutent pas aux facteurs.
    pub fn trigger(&self) -> i64 {
        self.stat("skill_type") as i64
    }
}

impl OptionCatalog {
    /// Effets de combat des options `tids`.
    pub fn gear(&self, tids: &[u16]) -> Gear {
        let mut g = Gear::default();
        for o in tids.iter().filter_map(|t| self.options.get(t)) {
            let (rate, crit) = (o.stat("skill_rate"), o.stat("Critical"));
            match o.trigger() {
                trigger::CRITICAL if rate > 0.0 => {
                    g.mods.crit_rate += rate;
                    g.mods.crit_dmg = g.mods.crit_dmg.max(crit);
                }
                trigger::CRITICAL => g.mods.crit_dmg += crit,
                trigger::STUN if rate > 0.0 => {
                    let ms = if o.stat("stun_time") > 0.0 { o.stat("stun_time") } else { o.stat("duration") };
                    g.stuns.push((rate, ms.max(0.0) as u64));
                }
                trigger::DRAIN if rate > 0.0 => {
                    g.mods.to_hp_rate += rate;
                    g.mods.to_hp = g.mods.to_hp.max(o.stat("damage_convert_hp"));
                }
                trigger::REGEN => {
                    g.regen_hp += o.stat("regen_hp") as i32;
                    g.regen_mp += o.stat("regen_mp") as i32;
                }
                trigger::WOUND if rate > 0.0 => {
                    let damages: Vec<(usize, i32)> = WOUND_COLUMNS.iter().map(|(c, i)| (*i, o.stat(c) as i32)).filter(|(_, d)| *d > 0).collect();
                    if !damages.is_empty() {
                        // « dot_damage_time », lu après « duration », la remplace (`Archonia.exe`) : c'est la durée totale.
                        let ms = if o.stat("dot_damage_time") > 0.0 { o.stat("dot_damage_time") } else { o.stat("duration") };
                        g.wounds.push(Wound { rate, duration_ms: ms.max(0.0) as u64, damages });
                    }
                }
                trigger::EVADE => g.mods.dodge += o.stat("evade_rate").max(o.stat("dodge_rate")),
                // Sans « skill_type », « Critical » seul donne une chance de coup critique (« Critical Hit Rate +1% »).
                0 if crit > 0.0 => g.mods.crit_rate += crit,
                _ => {}
            }
            g.mods.ignore_defense += o.stat("Ignore_Physical_Defense_Rate");
            g.mods.ignore_resist += o.stat("Ignore_Attribute_Defense_Rate");
        }
        g
    }

    /// Ajoute aux facteurs d'un personnage les options de son équipement porté : d'abord les valeurs, puis les
    /// pourcentages, appliqués au total. Les dégâts d'une option qui blesse sont ceux de sa blessure, pas un bonus.
    pub fn apply(&self, f: &mut crate::character::Factors, tids: &[u16]) {
        let mut percent: Vec<(Target, f64)> = Vec::new();
        for o in tids.iter().filter_map(|t| self.options.get(t)).filter(|o| o.trigger() != trigger::WOUND) {
            for (column, value) in &o.stats {
                let Some((_, targets, always_pct)) = EFFECTS.iter().find(|(c, _, _)| c == column) else { continue };
                for t in *targets {
                    if *always_pct || o.percent {
                        percent.push((*t, *value));
                    } else {
                        add(f, *t, |x| x + value_of(*t, *value));
                    }
                }
            }
        }
        for (t, pct) in percent {
            add(f, t, |x| (x as f64 * (100.0 + pct) / 100.0).round() as i32);
        }
    }
}

/// Valeur ajoutée à un facteur : les caractéristiques sont stockées × 100.
fn value_of(t: Target, v: f64) -> i32 {
    match t {
        Target::Status(i) if i < 7 => (v * 100.0).round() as i32,
        _ => v.round() as i32,
    }
}

fn add(f: &mut crate::character::Factors, t: Target, op: impl Fn(i32) -> i32) {
    match t {
        Target::Status(i) => f.status[i] = op(f.status[i]),
        Target::PointMax(i) => f.point_max[i] = op(f.point_max[i]),
        Target::Damage(i) => {
            f.damage_min[i] = op(f.damage_min[i]);
            f.damage_max[i] = op(f.damage_max[i]);
        }
        Target::DefensePoint(i) => f.defense_point[i] = op(f.defense_point[i]),
        Target::DefenseRate(i) => f.defense_rate[i] = op(f.defense_rate[i]),
        Target::Attack(i) => f.attack[i] = op(f.attack[i]),
    }
}

/// Colonne `option_tids` : TID séparés par des virgules.
pub fn options_column(tids: &[u16]) -> String {
    tids.iter().map(u16::to_string).collect::<Vec<_>>().join(",")
}

pub fn options_from_column(s: &str) -> Vec<u16> {
    s.split(',').filter_map(|t| t.trim().parse().ok()).take(MAX_OPTIONS).collect()
}

/// Mini-paquet du champ 20 d'AGPMITEM : les TID d'options, 5 au plus.
pub fn options_mini(tids: &[u16]) -> Vec<u8> {
    use archcore_net::schema::mini;
    use archcore_net::{build_mini, Fields};
    let m = &mini::ITEM_OPTION;
    let mut f = Fields::new(m);
    for (i, t) in tids.iter().take(MAX_OPTIONS).enumerate() {
        f.set(i, *t as i64);
    }
    build_mini(m, &f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    const TABLE: &str = "tid\tDescription\tUpper_Body\tLower_Body\tWeapon\tShield\tHead\tRing\tNecklace\tShoes\tGloves\tRefined\tGacha\tType\tCategoryName\tPointType\tItem_Level_Min\tItem_Level_Max\tLevel_limit\tRank_Min\tRank_Max\tProbability\tPhysicalDamage\tHp_Bonus_Point\r\n\
1\tDamage +1\t\t\t\t\t\t\t\t\t\t\t\t1\t\t\t\t\t1\t1\t4\t450\t1\t\r\n\
10\tW Damage +2\t\t\tO\t\t\t\t\t\t\t\t\t1\t\t\t\t\t1\t1\t4\t100\t2\t\r\n\
11\tW Damage +3\t\t\tO\t\t\t\t\t\t\t\t\t1\t\t\t\t\t1\t1\t4\t100\t3\t\r\n\
20\tW Hp +50\t\t\tO\t\t\t\t\t\t\t\t\t2\t\t\t\t\t1\t1\t4\t100\t\t50\r\n\
21\tB Hp +60\tO\t\t\t\t\t\t\t\t\t\t\t2\t\t\t\t\t1\t1\t4\t100\t\t60\r\n\
30\tG Hp +70\t\t\tO\t\t\t\t\t\t\t\tO\t3\t\t\t\t\t1\t1\t4\t100\t\t70\r\n\
31\tHigh Hp\t\t\tO\t\t\t\t\t\t\t\t\t4\t\t\t\t\t50\t1\t4\t100\t\t70\r\n\
32\tRefined 0\t\t\tO\t\t\t\t\t\t\t0\t\t5\t\t\t\t\t1\t1\t4\t100\t\t70\r\n\
40\tHp +10%\t\t\tO\t\t\t\t\t\t\t\t\t6\t\t1\t\t\t90\t1\t4\t100\t\t10\r\n";

    const NUM: &str = "OptionNum\tDropRate1\tDropRate2\tDropRate3\r\n\t1\t2\t3\r\n1\t100000\t90000\t75000\r\n2\t\t10000\t20000\r\n3\t\t\t5000\r\n";

    #[test]
    fn table_et_tirage() {
        let c = OptionCatalog::parse(TABLE, NUM, NUM);
        assert_eq!(c.options[&10].stats, vec![("PhysicalDamage".to_string(), 2.0)]);
        assert_eq!(c.option_rates[3][3], 5000);
        let sword = EquipData { kind: super::super::equipment::KIND_WEAPON, part: 10, level: 10, rank: 1, min_option: 2, max_option: 3, ..Default::default() };
        let mut rng = rand::rngs::StdRng::seed_from_u64(3);
        for _ in 0..50 {
            let o = c.roll_options(&sword, 10, &mut rng);
            // Deux types possibles, lâchée par un monstre de niveau 10 (1 et 2) : deux options au plus, une
            // par type ; jamais la 1 (aucune partie), la 30 (gacha), la 31 et la 40 (source de niveau 50 et 90
            // au moins), la 32 (« Refined » 0), la 21 (corps).
            assert_eq!(o.len(), 2, "{o:?}");
            assert!(o.iter().any(|t| *t == 10 || *t == 11) && o.contains(&20), "{o:?}");
        }
        // Un objet sans fourchette n'en reçoit pas.
        assert!(c.roll_options(&EquipData { min_option: 0, max_option: 0, ..sword.clone() }, 10, &mut rng).is_empty());
        assert!(c.roll_options(&sword, 0, &mut rng).is_empty(), "source sans niveau");
        // Options portées : +2 dégâts (10), +50 PV (20), puis +10 % de PV (40, PointType 1).
        let mut f = crate::character::Factors::default();
        f.point_max[0] = 100;
        f.damage_min[0] = 5;
        f.damage_max[0] = 7;
        c.apply(&mut f, &[10, 20, 40]);
        assert_eq!((f.damage_min[0], f.damage_max[0], f.point_max[0]), (7, 9, 165));
    }

    #[test]
    fn options_a_declenchement() {
        let table = "tid\tDescription\tType\tPhysicalDamage\tPoisonDamage\tskill_type\tduration\tskill_rate\tCritical\tstun_time\tdamage_convert_hp\tregen_hp\tregen_mp\tdot_damage_time\tevade_rate\tdodge_rate\tIgnore_Physical_Defense_Rate\r\n\
            2151\t1% chance on hit to strike your target for 110% damage.\t22\t\t\t1\t\t1\t110\r\n\
            890\tIncreases Critical Hit Damage by +50%\t145\t\t\t1\t\t\t50\r\n\
            2161\t1% chance on hit to stun your target for 1 sec.\t23\t\t\t2\t\t1\t\t1000\r\n\
            2171\t4% chance when hit to convert 10% of damage taken into restored health.\t24\t\t\t3\t\t4\t\t\t10\r\n\
            2181\tHP Recovery +20 every 10 sec\t25\t\t\t4\t\t\t\t\t\t20\r\n\
            888\tMana Recovery +200 every 10 sec\t145\t\t\t4\t\t\t\t\t\t\t200\r\n\
            2325\tpoison wound\t39\t\t60\t5\t6000\t4\t\t\t\t\t\t6000\r\n\
            2253\tDodge +4%\t42\t\t\t6\t\t\t\t\t\t\t\t\t4\t4\r\n\
            13822\tCritical Hit Rate +1%\t55\t\t\t\t\t\t1\r\n\
            50\tIgnore Defense +5%\t60\t3\t\t\t\t\t\t\t\t\t\t\t\t\t5\r\n";
        let c = OptionCatalog::parse(table, "", "");
        let g = c.gear(&[2151, 890, 2161, 2171, 2181, 888, 2325, 2253, 13822, 50]);
        assert_eq!((g.mods.crit_rate, g.mods.crit_dmg), (2.0, 160.0), "1 % + 1 % de chance ; 110 % puis +50 %");
        assert_eq!(g.stuns, vec![(1.0, 1000)]);
        assert_eq!((g.mods.to_hp_rate, g.mods.to_hp), (4.0, 10.0));
        assert_eq!((g.regen_hp, g.regen_mp), (20, 200));
        assert_eq!(g.wounds, vec![Wound { rate: 4.0, duration_ms: 6000, damages: vec![(15, 60)] }]);
        assert_eq!((g.mods.dodge, g.mods.ignore_defense), (4.0, 5.0));
        // Les dégâts d'une blessure ne s'ajoutent pas aux facteurs ; ceux d'une option ordinaire, si.
        let mut f = crate::character::Factors::default();
        c.apply(&mut f, &[2325, 50]);
        assert_eq!((f.damage_min[6], f.damage_min[0]), (0, 3));
    }

    #[test]
    fn options_liees() {
        let table = "tid\tDescription\tType\tLink_id\tLevel_min\tLevel_max\tSkillBlockRate\r\n\
            2021\tSkill Block +2%\t\t3\t1\t10\t2\r\n\
            2022\tSkill Block +3%\t\t3\t11\t20\t3\r\n";
        let c = OptionCatalog::parse(table, "", "");
        assert_eq!((c.linked(3, 1), c.linked(3, 15), c.linked(3, 30), c.linked(4, 5)), (Some(2021), Some(2022), None, None));
        assert!(c.options[&2021].stats.iter().all(|(k, _)| k == "SkillBlockRate"));
    }

    #[test]
    fn nombres_tires() {
        let rates = parse_count_rates(NUM);
        let mut rng = rand::rngs::StdRng::seed_from_u64(9);
        let mut seen = [0; 4];
        for _ in 0..2000 {
            seen[roll_count(&rates, 1, 3, &mut rng) as usize] += 1;
        }
        // 75 000 / 20 000 / 5 000 : environ 75 %, 20 %, 5 %.
        assert!(seen[1] > 1350 && seen[2] > 300 && seen[3] > 50 && seen[3] < 160, "{seen:?}");
        assert_eq!(roll_count(&rates, 2, 2, &mut rng), 2);
        assert_eq!(roll_count(&rates, 0, 3, &mut rng), 0);
        assert_eq!(options_from_column(&options_column(&[3, 70])), vec![3, 70]);
    }
}

