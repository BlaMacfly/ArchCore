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
//!            Les colonnes à déclenchement (Critical, skill_*, bonus_*, Ignore_*, on_hit_*…) ne sont pas
//!            appliquées.
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

impl OptionCatalog {
    /// Ajoute aux facteurs d'un personnage les options de son équipement porté : d'abord les valeurs, puis les
    /// pourcentages, appliqués au total.
    pub fn apply(&self, f: &mut crate::character::Factors, tids: &[u16]) {
        let mut percent: Vec<(Target, f64)> = Vec::new();
        for o in tids.iter().filter_map(|t| self.options.get(t)) {
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

