//! Personnages : modèles (`character_template`), croissance par niveau (`growup_factor`), lignes de
//! `characters`, facteurs et paquets AGPMCHARACTER (type 0x02) avec leurs mini-paquets (déplacement, facteurs).

use crate::db::{col, Databases};
use crate::info;
use archcore_net::schema::{self, mini};
use archcore_net::{build_mini, build_packet, Fields, Vec3};
use mysql_async::Row;
use std::collections::HashMap;

/// Classes jouables de CharacterDataTable : modèles complétés d'après cette table.
pub const BASE_TEMPLATE_NAMES: [&str; 14] = [
    "Knight", "Archer", "Mage", "Berserker", "Hunter", "Sorcerer", "Swashbuckler", "Ranger", "Elementalist", "Scion",
    "Slayer", "Orbiter", "Summoner", "Warrior",
];

/// Évolutions du Scion au niveau 20 : complétées comme les autres, mais absentes de l'écran de création (Archonia n'y
/// a qu'un bouton Scion pour le Dragon Scion).
const EVOLVED_CLASS_NAMES: [&str; 3] = ["Slayer", "Orbiter", "Summoner"];

/// Modèle de personnage (charactertemplate.ini).
#[derive(Clone, Debug, Default)]
pub struct Template {
    pub tid: u32,
    pub name: String,
    pub race: i32,
    pub gender: i32,
    pub class: i32,
    pub level: i32,
    pub con: f32,
    pub str_: f32,
    pub intel: f32,
    pub dex: f32,
    pub luk: f32,
    pub wis: f32,
    pub movement: i32,
    pub movement_fast: i32,
    pub hp: i32,
    pub mp: i32,
    pub sp: i32,
    pub attack_point: i32,
    pub magic_attack: i32,
    pub ar: i32,
    pub dr: i32,
    pub base_exp: i32,
    pub min_damage: i32,
    pub max_damage: i32,
    pub attack_range: i32,
    pub attack_speed: i32,
    /// Croissance par niveau (GrowUpFactor.txt), indice = niveau - 1 ; vide si le modèle n'en a pas.
    pub growth: Vec<Growth>,
}

/// Une ligne de GrowUpFactor.txt : caractéristiques et points d'un modèle à un niveau donné (valeurs
/// absolues, pas des écarts). Seules les colonnes non nulles remplacent celles du modèle.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Growth {
    pub str_: i32,
    pub dex: i32,
    pub con: i32,
    pub wis: i32,
    pub intel: i32,
    pub cha: i32,
    pub hp: i32,
    pub mp: i32,
    pub sp: i32,
    /// Défense physique (« AC »).
    pub ac: i32,
    pub ar: i32,
    pub dr: i32,
    pub min_damage: i32,
    pub max_damage: i32,
}

impl Growth {
    fn from_row(r: &Row) -> Self {
        Growth {
            str_: col(r, "str"),
            dex: col(r, "dex"),
            con: col(r, "con"),
            wis: col(r, "wis"),
            intel: col(r, "intel"),
            cha: col(r, "cha"),
            hp: col(r, "hp"),
            mp: col(r, "mp"),
            sp: col(r, "sp"),
            ac: col(r, "ac"),
            ar: col(r, "ar"),
            dr: col(r, "dr"),
            min_damage: col(r, "min_damage"),
            max_damage: col(r, "max_damage"),
        }
    }
}

impl Template {
    fn from_row(r: &Row) -> Self {
        Template {
            tid: col(r, "tid"),
            name: col(r, "name"),
            race: col(r, "race"),
            gender: col(r, "gender"),
            class: col(r, "class"),
            level: col(r, "level"),
            con: col(r, "con"),
            str_: col(r, "str"),
            intel: col(r, "intel"),
            dex: col(r, "dex"),
            luk: col(r, "luk"),
            wis: col(r, "wis"),
            movement: col(r, "movement"),
            movement_fast: col(r, "movement_fast"),
            hp: col(r, "hp"),
            mp: col(r, "mp"),
            sp: col(r, "sp"),
            attack_point: col(r, "attack_point"),
            magic_attack: col(r, "magic_attack"),
            ar: col(r, "ar"),
            dr: col(r, "dr"),
            base_exp: col(r, "base_exp"),
            min_damage: col(r, "min_damage"),
            max_damage: col(r, "max_damage"),
            attack_range: col(r, "attack_range"),
            attack_speed: col(r, "attack_speed"),
            growth: Vec::new(),
        }
    }

    /// Croissance au niveau `level` ; au-delà de la table, sa dernière ligne.
    pub fn growth_at(&self, level: i32) -> Option<&Growth> {
        let i = (level.max(1) as usize - 1).min(self.growth.len().checked_sub(1)?);
        self.growth.get(i)
    }

    /// PV, PM et PE max au niveau `level` : ceux de la croissance, sinon ceux du modèle (100, 50, 100 à défaut).
    pub fn max_points(&self, level: i32) -> (i32, i32, i32) {
        let g = self.growth_at(level).copied().unwrap_or_default();
        (nz(g.hp, nz(self.hp, 100)), nz(g.mp, nz(self.mp, 50)), nz(g.sp, nz(self.sp, 100)))
    }
}

/// Modèles jouables d'après CharacterDataTable.txt (`character_data_table.data`, JSON), qui fait foi pour
/// Archonia : l'ancien charactertemplate.ini n'a pas les classes ajoutées depuis (Ranger, classes du Dragon
/// Scion), donne au TID 377 un PNJ au lieu du Swashbuckler et laisse l'Elementalist sans race ni caractéristiques.
/// Le modèle existant est gardé s'il est de la même race et de la même classe (ses autres valeurs servent),
/// sinon remplacé ; race, sexe, classe, caractéristiques, points et vitesses viennent de la table.
fn apply_playable(all: &mut HashMap<u32, Template>, tid: u32, name: &str, data: &serde_json::Value) {
    let num = |key: &str| data.get(key).and_then(|v| v.as_str()).and_then(|s| s.trim().parse::<f32>().ok());
    let int = |key: &str| num(key).map(|v| v as i32);
    let (Some(race), Some(class)) = (int("Race"), int("Class")) else { return };
    if race <= 0 || class <= 0 {
        return;
    }
    let keep = all.get(&tid).is_some_and(|t| t.race == race && t.class == class);
    let t = all.entry(tid).or_default();
    if !keep {
        *t = Template { tid, name: name.to_string(), ..Default::default() };
    }
    t.race = race;
    t.class = class;
    t.gender = int("Gender").unwrap_or(t.gender);
    t.level = int("LVL").unwrap_or(t.level).max(1);
    t.str_ = num("STR").unwrap_or(t.str_);
    t.dex = num("DEX").unwrap_or(t.dex);
    t.intel = num("INT").unwrap_or(t.intel);
    t.con = num("CON").unwrap_or(t.con);
    t.wis = num("Wis").unwrap_or(t.wis);
    t.hp = int("MAX_HP").unwrap_or(t.hp);
    t.mp = int("MAX_MP").unwrap_or(t.mp);
    t.sp = int("MAX_SP").unwrap_or(t.sp);
    t.movement = int("Movement Walk").unwrap_or(t.movement);
    t.movement_fast = int("Movement Run").unwrap_or(t.movement_fast);
    if t.attack_range == 0 {
        t.attack_range = int("Range").unwrap_or(0);
    }
    if t.attack_speed == 0 {
        t.attack_speed = int("ATK Speed").unwrap_or(0);
    }
}

/// Donne à chaque modèle sa croissance : son propre bloc de GrowUpFactor.txt, sinon celui d'un modèle jouable
/// de même race et de même classe (du même sexe de préférence, puis du plus petit TID).
fn attach_growth(all: &mut HashMap<u32, Template>, mut blocks: HashMap<u32, Vec<Growth>>) {
    let mut donors: Vec<(u32, i32, i32, i32)> =
        all.values().filter(|t| t.race > 0 && t.class > 0 && blocks.contains_key(&t.tid)).map(|t| (t.tid, t.race, t.class, t.gender)).collect();
    donors.sort();
    let mut borrowed: Vec<(u32, u32)> = Vec::new();
    for t in all.values().filter(|t| t.race > 0 && t.class > 0 && !blocks.contains_key(&t.tid)) {
        let same = |d: &&(u32, i32, i32, i32)| d.1 == t.race && d.2 == t.class;
        if let Some(d) = donors.iter().filter(same).find(|d| d.3 == t.gender).or_else(|| donors.iter().find(same)) {
            borrowed.push((t.tid, d.0));
        }
    }
    for (tid, from) in borrowed {
        let g = blocks[&from].clone();
        blocks.insert(tid, g);
    }
    for (tid, g) in blocks {
        if let Some(t) = all.get_mut(&tid) {
            t.growth = g;
        }
    }
}

pub struct Templates {
    pub all: HashMap<u32, Template>,
    /// Modèles jouables proposés à l'écran de création.
    pub base: Vec<u32>,
}

impl Templates {
    pub async fn load(db: &Databases) -> anyhow::Result<Self> {
        let rows = db.world.rows("SELECT * FROM character_template", ()).await?;
        let mut all: HashMap<u32, Template> = rows.iter().map(Template::from_row).map(|t| (t.tid, t)).collect();
        let mut blocks: HashMap<u32, Vec<Growth>> = HashMap::new();
        for r in db.world.rows("SELECT * FROM growup_factor ORDER BY template_id, level", ()).await? {
            let (tid, level): (u32, usize) = (col(&r, "template_id"), col(&r, "level"));
            let g = blocks.entry(tid).or_default();
            // Niveaux attendus à la suite depuis 1 : un trou arrête le bloc.
            if level == g.len() + 1 {
                g.push(Growth::from_row(&r));
            }
        }
        let placeholders = vec!["?"; BASE_TEMPLATE_NAMES.len()].join(",");
        let sql = format!("SELECT tid, name, data FROM character_data_table WHERE name IN ({placeholders}) ORDER BY tid");
        let params: Vec<mysql_async::Value> = BASE_TEMPLATE_NAMES.iter().map(|n| (*n).into()).collect();
        // Modèles jouables complétés d'après CharacterDataTable avant la croissance, qu'ils empruntent au besoin.
        let mut base = Vec::new();
        for r in db.world.rows(&sql, params).await? {
            let (tid, name, data): (u32, String, String) = (col(&r, "tid"), col(&r, "name"), col(&r, "data"));
            let Ok(data) = serde_json::from_str::<serde_json::Value>(&data) else { continue };
            apply_playable(&mut all, tid, &name, &data);
            if all.get(&tid).is_some_and(|t| t.race > 0 && t.class > 0) && !EVOLVED_CLASS_NAMES.contains(&name.as_str()) {
                base.push(tid);
            }
        }
        let block_count = blocks.len();
        attach_growth(&mut all, blocks);
        let grown = all.values().filter(|t| !t.growth.is_empty()).count();
        info!("croissance par niveau : {block_count} bloc(s) dans growup_factor, {grown} modèle(s) concerné(s)");
        let list: Vec<String> = base.iter().map(|t| t.to_string()).collect();
        info!("{} template(s) de personnage chargé(s), {} de base : {}", all.len(), base.len(), list.join(", "));
        Ok(Templates { all, base })
    }

    pub fn get(&self, tid: u32) -> Option<&Template> {
        self.all.get(&tid)
    }
}

/// Charisme au plus (borne du client, `SetCharisma`).
pub const MAX_CHARISMA: u32 = 1_000_000;

/// Ligne de la table `characters`.
#[derive(Clone, Debug, Default)]
pub struct CharacterRow {
    pub id: u32,
    pub slot: i32,
    pub name: String,
    pub template_id: u32,
    pub level: i32,
    pub exp: u64,
    pub hp: i32,
    pub mp: i32,
    pub sp: i32,
    pub pos_x: f64,
    pub pos_y: f64,
    pub pos_z: f64,
    pub money: u64,
    pub bank_money: u64,
    pub cash: u64,
    pub hair: i32,
    pub face: i32,
    pub bank_size: i32,
    pub option_flag: u32,
    /// Points de charisme (0 à 1 000 000).
    pub charisma: u32,
    /// Combat entre joueurs : victoires, défaites, points de meurtrier, fins du statut de premier attaquant et du
    /// délai de meurtrier (secondes Unix, 0 : aucun). Voir `world/pvp.rs`.
    pub pvp_win: u32,
    pub pvp_lose: u32,
    pub murderer: u32,
    pub criminal_until: i64,
    pub murderer_until: i64,
}

impl CharacterRow {
    pub fn from_row(r: &Row) -> Self {
        CharacterRow {
            id: col(r, "id"),
            slot: col(r, "slot"),
            name: col(r, "name"),
            template_id: col(r, "template_id"),
            level: col(r, "level"),
            exp: col(r, "exp"),
            hp: col(r, "hp"),
            mp: col(r, "mp"),
            sp: col(r, "sp"),
            pos_x: col(r, "pos_x"),
            pos_y: col(r, "pos_y"),
            pos_z: col(r, "pos_z"),
            money: col(r, "money"),
            bank_money: col(r, "bank_money"),
            cash: col(r, "cash"),
            hair: col(r, "hair"),
            face: col(r, "face"),
            bank_size: col(r, "bank_size"),
            option_flag: col(r, "option_flag"),
            charisma: col(r, "charisma"),
            pvp_win: col(r, "pvp_win"),
            pvp_lose: col(r, "pvp_lose"),
            murderer: col(r, "murderer_points"),
            criminal_until: col(r, "criminal_until"),
            murderer_until: col(r, "murderer_until"),
        }
    }

    pub fn pos(&self) -> Vec3 {
        Vec3::new(self.pos_x as f32, self.pos_y as f32, self.pos_z as f32)
    }

    pub fn set_pos(&mut self, p: Vec3) {
        self.pos_x = p.x as f64;
        self.pos_y = p.y as f64;
        self.pos_z = p.z as f64;
    }
}

/// Facteurs d'un personnage (AgpmFactors), tels qu'envoyés au client.
#[derive(Clone, Debug, Default)]
pub struct Factors {
    pub status: [i32; 13],
    pub ftype: [i32; 3],
    pub point: [i32; 20],
    pub point_max: [i32; 11],
    pub recovery: [i32; 3],
    pub attribute: [i32; 15],
    pub damage_min: [i32; 15],
    pub damage_max: [i32; 15],
    pub defense_point: [i32; 15],
    pub defense_rate: [i32; 15],
    pub attack: [i32; 8],
    pub item: [i32; 5],
    pub dirt: [i32; 8],
    pub price: [i32; 4],
}

/// Indices utiles dans `point`.
pub const POINT_HP: usize = 0;

fn nz(v: i32, d: i32) -> i32 {
    if v != 0 { v } else { d }
}

/// Facteurs d'un personnage à partir de son modèle et, s'il existe, de sa ligne en base. Au niveau du
/// personnage, la croissance (GrowUpFactor) remplace les caractéristiques, les PV, PM et PE max, la défense
/// physique et, quand elle les donne, précision, esquive et dégâts du modèle.
pub fn make_factors(tpl: &Template, row: Option<&CharacterRow>) -> Factors {
    let level = row.map(|r| r.level).unwrap_or(tpl.level);
    let g = tpl.growth_at(level).copied().unwrap_or_default();
    let stat = |grown: i32, base: f32| if grown != 0 { grown * 100 } else { (base * 100.0).round() as i32 };
    let (max_hp, max_mp, max_sp) = tpl.max_points(level);
    let exp = row.map(|r| r.exp).unwrap_or(0);
    let mut f = Factors::default();
    f.status = [
        stat(g.con, tpl.con), stat(g.str_, tpl.str_), stat(g.intel, tpl.intel), stat(g.dex, tpl.dex), row.map(|r| r.charisma.min(MAX_CHARISMA) as i32).unwrap_or(0),
        stat(0, tpl.luk), stat(g.wis, tpl.wis), level, tpl.movement, tpl.movement_fast, 0, row.map(|r| r.murderer as i32).unwrap_or(0), 0,
    ];
    f.ftype = [tpl.race, tpl.gender, tpl.class];
    f.point[0] = row.map(|r| r.hp).filter(|&v| v != 0).unwrap_or(max_hp);
    f.point[1] = row.map(|r| r.mp).filter(|&v| v != 0).unwrap_or(max_mp);
    f.point[2] = row.map(|r| r.sp).filter(|&v| v != 0).unwrap_or(max_sp);
    f.point[3] = (exp % (1u64 << 32)) as u32 as i32;
    f.point[4] = (exp >> 32) as i32;
    f.point[5] = tpl.attack_point;
    f.point[6] = tpl.magic_attack;
    f.point_max = [max_hp, max_mp, max_sp, 0, tpl.base_exp, tpl.attack_point, tpl.magic_attack, 0, nz(g.ar, tpl.ar), nz(g.dr, tpl.dr), 0];
    f.attack = [nz(tpl.attack_range, 50), 50, nz(tpl.attack_speed, 60), 0, 0, 0, 0, 0];
    f.damage_min[0] = nz(g.min_damage, tpl.min_damage);
    f.damage_max[0] = nz(g.max_damage, tpl.max_damage);
    f.defense_point[0] = g.ac;
    f
}

fn ints(schema: &'static archcore_net::Schema, values: &[i32]) -> Vec<u8> {
    let mut fields = Fields::new(schema);
    for (i, v) in values.iter().enumerate() {
        fields.set(i, *v);
    }
    build_mini(schema, &fields)
}

fn factor_parts(f: &Factors) -> Vec<Vec<u8>> {
    let attr = |a: &[i32; 15]| ints(&mini::FACTOR_ATTRIBUTE, a);
    let pair = |s: &'static archcore_net::Schema, a: Vec<u8>, b: Vec<u8>| build_mini(s, &Fields::new(s).with(0, a).with(1, b));
    vec![
        ints(&mini::FACTOR_STATUS, &f.status),
        ints(&mini::FACTOR_TYPE, &f.ftype),
        ints(&mini::FACTOR_POINT, &f.point),
        ints(&mini::FACTOR_POINTMAX, &f.point_max),
        ints(&mini::FACTOR_RECOVERY, &f.recovery),
        attr(&f.attribute),
        pair(&mini::FACTOR_DMG, attr(&f.damage_min), attr(&f.damage_max)),
        pair(&mini::FACTOR_DEFENSE, attr(&f.defense_point), attr(&f.defense_rate)),
        ints(&mini::FACTOR_ATTACK, &f.attack),
        ints(&mini::FACTOR_ITEM, &f.item),
        ints(&mini::FACTOR_DIRT, &f.dirt),
        ints(&mini::FACTOR_PRICE, &f.price),
    ]
}

/// Mini-paquet AGPMCHAR_FACTORS : [résultat, statut, type, points, max, récup, attributs, dégâts,
/// défense, attaque, objet, dirt, prix] ; le « résultat » reprend les mêmes facteurs.
pub fn factors_mini(f: &Factors) -> Vec<u8> {
    let parts = factor_parts(f);
    let s = &mini::CHAR_FACTORS;
    let mut result = Fields::new(s);
    for (i, p) in parts.iter().enumerate() {
        result.set(i + 1, p.clone());
    }
    let mut full = result.clone();
    full.set(0, build_mini(s, &result));
    build_mini(s, &full)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MoveMini {
    pub pos: Vec3,
    pub dest: Option<Vec3>,
    pub flags: i64,
}

pub fn move_mini(m: MoveMini) -> Vec<u8> {
    let s = &mini::CHAR_MOVE;
    build_mini(
        s,
        &Fields::new(s)
            .with(0, m.pos)
            .with(1, m.dest.unwrap_or(m.pos))
            .with(2, 0)
            .with(3, 0)
            .with(4, 0.0f32)
            .with(5, 0.0f32)
            .with(6, m.flags)
            .with(7, 0),
    )
}

/// Données d'un personnage nécessaires au paquet AGPMCHARACTER complet.
#[derive(Clone, Debug, Default)]
pub struct CharView {
    pub id: u32,
    pub template_id: u32,
    pub name: String,
    pub status: i64,
    pub is_new: i64,
    pub pos: Vec3,
    pub money: u64,
    pub bank_money: u64,
    pub cash: u64,
    pub hair: i32,
    pub face: i32,
    pub bank_size: i32,
    pub option_flag: u32,
    /// Statut criminel (champ 12 : 1 premier attaquant) et secondes restantes de ce statut (champ 25, 1200 au plus)
    /// et du délai de meurtrier (champ 26, 7200 au plus).
    pub criminal: i64,
    pub criminal_left: i64,
    pub murderer_left: i64,
    /// Champ 18 (« transformation ») : bit 0 transformé, bit 1 monté (`world/ride.rs`), bit 2 évolution.
    pub transform: i64,
}

/// Secondes Unix actuelles.
pub fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

impl From<&CharacterRow> for CharView {
    fn from(c: &CharacterRow) -> Self {
        CharView {
            id: c.id,
            template_id: c.template_id,
            name: c.name.clone(),
            status: 0,
            is_new: 0,
            pos: c.pos(),
            money: c.money,
            bank_money: c.bank_money,
            cash: c.cash,
            hair: c.hair,
            face: c.face,
            bank_size: c.bank_size,
            option_flag: c.option_flag,
            criminal: (c.criminal_until > unix_now()) as i64,
            criminal_left: (c.criminal_until - unix_now()).clamp(0, 1200),
            murderer_left: (c.murderer_until - unix_now()).clamp(0, 7200),
            transform: 0,
        }
    }
}

/// Paquet AGPMCHARACTER complet (op 0 par défaut) pour un personnage.
pub fn character_packet(ch: &CharView, factors: &Factors, op: i64) -> Vec<u8> {
    let s = &schema::CHARACTER;
    let mut name = archcore_net::text::encode(&ch.name);
    name.push(0);
    let fields = Fields::new(s)
        .with(0, op)
        .with(1, ch.id)
        .with(2, ch.template_id)
        .with(3, name)
        .with(4, ch.status)
        .with(5, move_mini(MoveMini { pos: ch.pos, dest: None, flags: 0 }))
        .with(7, factors_mini(factors))
        .with(8, ch.money)
        .with(9, ch.bank_money)
        .with(10, ch.cash)
        .with(11, 0)
        .with(12, ch.criminal)
        .with(13, 0)
        .with(14, ch.is_new)
        .with(15, 0)
        .with(16, 0)
        .with(17, 0)
        .with(18, ch.transform)
        .with(19, "")
        .with(20, ch.face)
        .with(21, ch.hair)
        .with(22, ch.option_flag)
        .with(23, ch.bank_size)
        .with(24, 0)
        .with(25, ch.criminal_left)
        .with(26, ch.murderer_left)
        .with(27, "")
        .with(29, 0);
    build_packet(s, &fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modeles_jouables_de_characterdatatable() {
        let mut all = HashMap::new();
        // TID 377 : PNJ dans l'ancien charactertemplate.ini, Swashbuckler dans CharacterDataTable.
        all.insert(377, Template { tid: 377, name: "<Bûcheron>".into(), hp: 5000, ..Default::default() });
        let swash = serde_json::json!({ "Race": "3", "Gender": "2", "Class": "1", "LVL": "1", "STR": "15", "MAX_HP": "104", "Movement Run": "6000" });
        apply_playable(&mut all, 377, "Swashbuckler", &swash);
        let t = &all[&377];
        assert_eq!((t.name.as_str(), t.race, t.gender, t.class, t.hp, t.movement_fast), ("Swashbuckler", 3, 2, 1, 104, 6000));
        assert_eq!(t.str_, 15.0);
        // Modèle absent (Scion, TID 1724) : créé ; modèle de même race et classe : gardé et complété.
        apply_playable(&mut all, 1724, "Scion", &serde_json::json!({ "Race": "4", "Class": "3", "MAX_HP": "80" }));
        assert_eq!((all[&1724].race, all[&1724].hp), (4, 80));
        all.insert(96, Template { tid: 96, name: "휴먼 남자전사".into(), race: 1, class: 1, ar: 19, ..Default::default() });
        apply_playable(&mut all, 96, "Knight", &serde_json::json!({ "Race": "1", "Class": "1", "MAX_HP": "90" }));
        assert_eq!((all[&96].ar, all[&96].hp, all[&96].name.as_str()), (19, 90, "휴먼 남자전사"));
        // Sans race ni classe : rien n'est fait.
        apply_playable(&mut all, 5, "Warrior", &serde_json::json!({ "Race": "0" }));
        assert!(!all.contains_key(&5));
    }

    /// Guerrier humain (TID 96) : modèle de charactertemplate.ini et deux lignes de GrowUpFactor.txt.
    fn human_warrior(levels: usize) -> Template {
        let lv1 = Growth { str_: 17, dex: 12, con: 15, wis: 6, intel: 6, hp: 120, mp: 32, sp: 65, ac: 3, ..Default::default() };
        let lv25 = Growth { str_: 73, dex: 59, con: 57, wis: 20, intel: 30, hp: 456, mp: 108, sp: 318, ac: 4, ..Default::default() };
        let mut growth = vec![lv1; levels];
        if levels >= 25 {
            growth[24] = lv25;
        }
        Template {
            tid: 96, race: 1, gender: 1, class: 1, level: 1, con: 13.0, str_: 14.0, intel: 7.0, dex: 11.0, wis: 7.0,
            hp: 90, mp: 46, sp: 74, ar: 19, dr: 13, growth, ..Default::default()
        }
    }

    fn at(level: i32) -> CharacterRow {
        CharacterRow { level, ..Default::default() }
    }

    #[test]
    fn croissance_niveau_1() {
        let f = make_factors(&human_warrior(25), Some(&at(1)));
        // CON, FOR, INT, DEX, CHA (points de charisme, bruts), CHANCE, SAG, niveau.
        assert_eq!(f.status[..8], [1500, 1700, 600, 1200, 0, 0, 600, 1]);
        assert_eq!(f.point_max[..3], [120, 32, 65], "PV, PM, PE max de la table, pas du modèle (90, 46, 74)");
        assert_eq!(f.point[..3], [120, 32, 65], "personnage neuf : points pleins");
        assert_eq!((f.point_max[8], f.point_max[9], f.defense_point[0]), (19, 13, 3), "précision et esquive du modèle, défense de la table");
    }

    #[test]
    fn croissance_niveau_25() {
        let tpl = human_warrior(25);
        let f = make_factors(&tpl, Some(&at(25)));
        assert_eq!(f.status[..8], [5700, 7300, 3000, 5900, 0, 0, 2000, 25]);
        assert_eq!(f.point_max[..3], [456, 108, 318]);
        assert_eq!(f.defense_point[0], 4);
        // Points enregistrés gardés tant qu'ils ne sont pas nuls.
        let f = make_factors(&tpl, Some(&CharacterRow { level: 25, hp: 200, mp: 0, ..Default::default() }));
        assert_eq!((f.point[0], f.point[1]), (200, 108));
        // Au-delà de la table : sa dernière ligne.
        assert_eq!(make_factors(&tpl, Some(&at(40))).point_max[..3], [456, 108, 318]);
        assert_eq!(tpl.max_points(0), (120, 32, 65), "niveau 0 ramené au niveau 1");
    }

    #[test]
    fn sans_croissance_le_modele() {
        let f = make_factors(&human_warrior(0), Some(&at(25)));
        assert_eq!(f.point_max[..3], [90, 46, 74]);
        assert_eq!((f.status[0], f.status[7], f.defense_point[0]), (1300, 25, 0));
    }

    #[test]
    fn croissance_empruntee_meme_race_et_classe() {
        let warrior = human_warrior(0);
        let female = Template { tid: 500, gender: 2, ..warrior.clone() };
        let archer = Template { tid: 501, class: 2, ..warrior.clone() };
        let npc = Template { tid: 502, race: 0, class: 0, ..warrior.clone() };
        let mut all: HashMap<u32, Template> = [warrior, female, archer, npc].into_iter().map(|t| (t.tid, t)).collect();
        let lv1 = Growth { hp: 120, ..Default::default() };
        attach_growth(&mut all, HashMap::from([(96, vec![lv1; 3]), (7, vec![lv1])]));
        assert_eq!(all[&96].growth.len(), 3);
        assert_eq!(all[&500].growth.len(), 3, "guerrière humaine : bloc du guerrier humain");
        assert!(all[&501].growth.is_empty(), "autre classe");
        assert!(all[&502].growth.is_empty(), "PNJ sans race ni classe");
    }
}
