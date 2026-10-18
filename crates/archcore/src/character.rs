//! Personnages : modèles (`character_template`), lignes de `characters`, facteurs et paquets
//! AGPMCHARACTER (type 0x02) avec leurs mini-paquets (déplacement, facteurs).

use crate::db::{col, Databases};
use crate::info;
use archcore_net::schema::{self, mini};
use archcore_net::{build_mini, build_packet, Fields, Vec3};
use mysql_async::Row;
use std::collections::HashMap;

/// TID des personnages de base proposés à la création (CharacterDataTable).
pub const BASE_TEMPLATE_NAMES: [&str; 14] = [
    "Knight", "Archer", "Mage", "Berserker", "Hunter", "Sorcerer", "Swashbuckler", "Ranger", "Elementalist", "Scion",
    "Slayer", "Orbiter", "Summoner", "Warrior",
];

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
    pub cha: f32,
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
            cha: col(r, "cha"),
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
        let all: HashMap<u32, Template> = rows.iter().map(Template::from_row).map(|t| (t.tid, t)).collect();
        let placeholders = vec!["?"; BASE_TEMPLATE_NAMES.len()].join(",");
        let sql = format!("SELECT tid, name FROM character_data_table WHERE name IN ({placeholders}) ORDER BY tid");
        let params: Vec<mysql_async::Value> = BASE_TEMPLATE_NAMES.iter().map(|n| (*n).into()).collect();
        let base_rows = db.world.rows(&sql, params).await?;
        // Écarte les PNJ homonymes (« <Titre> Nom ») : seuls les modèles jouables vont à l'écran de création.
        let base: Vec<u32> = base_rows
            .iter()
            .map(|r| col::<u32>(r, "tid"))
            .filter(|tid| all.get(tid).is_some_and(|t| !t.name.contains('<')))
            .collect();
        let list: Vec<String> = base.iter().map(|t| t.to_string()).collect();
        info!("{} template(s) de personnage chargé(s), {} de base : {}", all.len(), base.len(), list.join(", "));
        Ok(Templates { all, base })
    }

    pub fn get(&self, tid: u32) -> Option<&Template> {
        self.all.get(&tid)
    }
}

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

/// Facteurs d'un personnage à partir de son modèle et, s'il existe, de sa ligne en base.
pub fn make_factors(tpl: &Template, row: Option<&CharacterRow>) -> Factors {
    let level = row.map(|r| r.level).unwrap_or(tpl.level);
    let x100 = |v: f32| (v * 100.0).round() as i32;
    let (max_hp, max_mp, max_sp) = (nz(tpl.hp, 100), nz(tpl.mp, 50), nz(tpl.sp, 100));
    let exp = row.map(|r| r.exp).unwrap_or(0);
    let mut f = Factors::default();
    f.status = [
        x100(tpl.con), x100(tpl.str_), x100(tpl.intel), x100(tpl.dex), x100(tpl.cha), x100(tpl.luk), x100(tpl.wis), level,
        tpl.movement, tpl.movement_fast, 0, 0, 0,
    ];
    f.ftype = [tpl.race, tpl.gender, tpl.class];
    f.point[0] = row.map(|r| r.hp).filter(|&v| v != 0).unwrap_or(max_hp);
    f.point[1] = row.map(|r| r.mp).filter(|&v| v != 0).unwrap_or(max_mp);
    f.point[2] = row.map(|r| r.sp).filter(|&v| v != 0).unwrap_or(max_sp);
    f.point[3] = (exp % (1u64 << 32)) as u32 as i32;
    f.point[4] = (exp >> 32) as i32;
    f.point[5] = tpl.attack_point;
    f.point[6] = tpl.magic_attack;
    f.point_max = [max_hp, max_mp, max_sp, 0, tpl.base_exp, tpl.attack_point, tpl.magic_attack, 0, tpl.ar, tpl.dr, 0];
    f.attack = [nz(tpl.attack_range, 50), 50, nz(tpl.attack_speed, 60), 0, 0, 0, 0, 0];
    f.damage_min[0] = tpl.min_damage;
    f.damage_max[0] = tpl.max_damage;
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
        .with(12, 0)
        .with(13, 0)
        .with(14, ch.is_new)
        .with(15, 0)
        .with(16, 0)
        .with(17, 0)
        .with(18, 0)
        .with(19, "")
        .with(20, ch.face)
        .with(21, ch.hair)
        .with(22, ch.option_flag)
        .with(23, ch.bank_size)
        .with(24, 0)
        .with(25, 0)
        .with(26, 0)
        .with(27, "")
        .with(29, 0);
    build_packet(s, &fields)
}
