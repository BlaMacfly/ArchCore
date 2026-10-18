// Construction des paquets AGPMCHARACTER (type 0x02) et de leurs mini-paquets
// (déplacement, facteurs). Les valeurs viennent de la base (characters) et du template
// (character_template importé de charactertemplate.ini).

import { buildMini, buildPacket, sparse } from '../net/packet.js';
import { Main, Mini } from '../net/schema.js';

const zeroAttr = () => ({ physical: 0, magic: 0, water: 0, fire: 0, earth: 0, air: 0, poison: 0, lightning: 0, ice: 0, physBlock: 0, skillBlock: 0, heroic: 0, heroicMelee: 0, heroicRange: 0, heroicMagic: 0 });

/** Facteurs d'un personnage à partir de son template et de sa ligne en base. */
export function makeFactors(tpl, row = {}) {
  const level = row.level ?? tpl.level ?? 1;
  const status = {
    stamina: Math.round((tpl.con ?? 0) * 100), strength: Math.round((tpl.str ?? 0) * 100),
    intelligence: Math.round((tpl.intel ?? 0) * 100), dexterity: Math.round((tpl.dex ?? 0) * 100),
    charisma: Math.round((tpl.cha ?? 0) * 100), luck: Math.round((tpl.luk ?? 0) * 100),
    wisdom: Math.round((tpl.wis ?? 0) * 100), level,
    moveSpeed: tpl.movement ?? 2000, runSpeed: tpl.movement_fast ?? 5000, unionRank: 0, villainPoints: 0, unk: 0,
  };
  const type = { race: tpl.race ?? 1, gender: tpl.gender ?? 1, cls: tpl.class ?? 1 };
  const maxHp = tpl.hp || 100, maxMp = tpl.mp || 50, maxSp = tpl.sp || 100;
  const point = {
    hp: row.hp || maxHp, mana: row.mp || maxMp, skillPoints: row.sp || maxSp,
    xpLow: Number(row.exp ?? 0) % 2 ** 32, xpHigh: Math.floor(Number(row.exp ?? 0) / 2 ** 32),
    AP: tpl.attack_point ?? 0, MAP: tpl.magic_attack ?? 0, MI: 0, agro: 0, damage: 0, damageMagic: 0, damageWater: 0,
    damageFire: 0, damageEarth: 0, damageAir: 0, damagePoison: 0, damageLightning: 0, damageIce: 0, bonusXP: 0, damageHeroic: 0,
  };
  const pointMax = { maxHP: maxHp, maxMana: maxMp, maxSkillPoints: maxSp, xpLow: 0, xpHigh: tpl.base_exp ?? 48, AP: tpl.attack_point ?? 0, MAP: tpl.magic_attack ?? 0, MI: 0, AR: tpl.ar ?? 0, DR: tpl.dr ?? 0, MAR: 0 };
  const recovery = { hp: 0, mana: 0, sp: 0 };
  const attack = { range: tpl.attack_range || 50, hitRange: 50, attackSpeed: tpl.attack_speed || 60, cast: 0, cooldown: 0, accuracy: 0, meleeDodge: 0, rangeDodge: 0 };
  const item = { durability: 0, hand: 0, rank: 0, physRank: 0, maxDura: 0 };
  const dirt = { duration: 0, intensity: 0, range: 0, target: 0, skillLevel: 0, skillPoint: 0, skillXP: 0, heroicPoint: 0 };
  const price = { npc: 0, player: 0, moneyHigh: 0, moneyLow: 0 };
  const damage = { min: { ...zeroAttr(), physical: tpl.min_damage ?? 0 }, max: { ...zeroAttr(), physical: tpl.max_damage ?? 0 } };
  const defense = { point: zeroAttr(), rate: zeroAttr() };
  return { status, type, point, pointMax, recovery, attribute: zeroAttr(), damage, defense, attack, item, dirt, price };
}

const attrList = (a) => [a.physical, a.magic, a.water, a.fire, a.earth, a.air, a.poison, a.lightning, a.ice, a.physBlock, a.skillBlock, a.heroic, a.heroicMelee, a.heroicRange, a.heroicMagic];

function buildFactorParts(f) {
  const s = f.status;
  const status = buildMini(Mini.FACTOR_STATUS, [s.stamina, s.strength, s.intelligence, s.dexterity, s.charisma, s.luck, s.wisdom, s.level, s.moveSpeed, s.runSpeed, s.unionRank, s.villainPoints, s.unk]);
  const type = buildMini(Mini.FACTOR_TYPE, [f.type.race, f.type.gender, f.type.cls]);
  const p = f.point;
  const point = buildMini(Mini.FACTOR_POINT, [p.hp, p.mana, p.skillPoints, p.xpLow, p.xpHigh, p.AP, p.MAP, p.MI, p.agro, p.damage, p.damageMagic, p.damageWater, p.damageFire, p.damageEarth, p.damageAir, p.damagePoison, p.damageLightning, p.damageIce, p.bonusXP, p.damageHeroic]);
  const m = f.pointMax;
  const pointMax = buildMini(Mini.FACTOR_POINTMAX, [m.maxHP, m.maxMana, m.maxSkillPoints, m.xpLow, m.xpHigh, m.AP, m.MAP, m.MI, m.AR, m.DR, m.MAR]);
  const recovery = buildMini(Mini.FACTOR_RECOVERY, [f.recovery.hp, f.recovery.mana, f.recovery.sp]);
  const attribute = buildMini(Mini.FACTOR_ATTRIBUTE, attrList(f.attribute));
  const dmg = buildMini(Mini.FACTOR_DMG, [buildMini(Mini.FACTOR_ATTRIBUTE, attrList(f.damage.min)), buildMini(Mini.FACTOR_ATTRIBUTE, attrList(f.damage.max))]);
  const def = buildMini(Mini.FACTOR_DEFENSE, [buildMini(Mini.FACTOR_ATTRIBUTE, attrList(f.defense.point)), buildMini(Mini.FACTOR_ATTRIBUTE, attrList(f.defense.rate))]);
  const a = f.attack;
  const attack = buildMini(Mini.FACTOR_ATTACK, [a.range, a.hitRange, a.attackSpeed, a.cast, a.cooldown, a.accuracy, a.meleeDodge, a.rangeDodge]);
  const it = f.item;
  const item = buildMini(Mini.FACTOR_ITEM, [it.durability, it.hand, it.rank, it.physRank, it.maxDura]);
  const d = f.dirt;
  const dirt = buildMini(Mini.FACTOR_DIRT, [d.duration, d.intensity, d.range, d.target, d.skillLevel, d.skillPoint, d.skillXP, d.heroicPoint]);
  const pr = f.price;
  const price = buildMini(Mini.FACTOR_PRICE, [pr.npc, pr.player, pr.moneyHigh, pr.moneyLow]);
  return [status, type, point, pointMax, recovery, attribute, dmg, def, attack, item, dirt, price];
}

/** Mini-paquet AGPMCHAR_FACTORS : [résultat, statut, type, points, max, récup, attributs, dégâts, défense, attaque, objet, dirt, prix]. */
export function buildFactorsMini(f) {
  const parts = buildFactorParts(f);
  const result = buildMini(Mini.CHAR_FACTORS, [null, ...parts]);
  return buildMini(Mini.CHAR_FACTORS, [result, ...parts]);
}

export function buildMoveMini(move = {}) {
  const pos = move.pos ?? { x: 0, y: 0, z: 0 };
  return buildMini(Mini.CHAR_MOVE, [pos, move.dest ?? pos, move.followTarget ?? 0, move.followDistance ?? 0, move.axisX ?? 0, move.axisY ?? 0, move.flags ?? 0, move.direction ?? 0]);
}

/**
 * Paquet AGPMCHARACTER complet (op 0 par défaut) pour un personnage.
 * @param {{id:number, template_id:number, name:string, status?:number, isNew?:number, pos?:object, money?:number, bank_money?:number, cash?:number, hair?:number, face?:number, bank_size?:number, option_flag?:number}} ch
 * @param {object} factors résultat de makeFactors
 */
export function buildCharacterPacket(ch, factors, { op = 0, includeMove = true, includeFactors = true } = {}) {
  const name = Buffer.from(`${ch.name}\0`, 'latin1');
  const values = sparse(Main.CHARACTER, {
    0: op,
    1: ch.id,
    2: ch.template_id,
    3: name,
    4: ch.status ?? 0,
    5: includeMove ? buildMoveMini({ pos: ch.pos }) : null,
    7: includeFactors ? buildFactorsMini(factors) : null,
    8: BigInt(ch.money ?? 0),
    9: BigInt(ch.bank_money ?? 0),
    10: BigInt(ch.cash ?? 0),
    11: 0, // statut d'action
    12: 0, // statut criminel
    13: 0, // attaquant
    14: ch.isNew ?? 0,
    15: 0,
    16: 0,
    17: 0n,
    18: 0,
    19: '',
    20: ch.face ?? 0,
    21: ch.hair ?? 0,
    22: ch.option_flag ?? 0,
    23: ch.bank_size ?? 0,
    24: 0,
    25: 0n,
    26: 0n,
    27: '',
    29: 0n,
  });
  return buildPacket(Main.CHARACTER, values);
}
