// Test de boutique : le personnage va voir <Marchand> Darryl (Anchorville), ouvre sa boutique, achète une
// potion puis la revend.
//   node tools/sim-shop.mjs [compte] [mdp] [perso]
// À lancer contre un serveur de test : le personnage est déplacé et reçoit 1000 pièces d'or avant le test.

import mariadb from 'mariadb';
import { buildMini, decodeMini, sparse } from '../src/net/packet.js';
import { Main, Mini } from '../src/net/schema.js';
import { loadConfig } from '../src/common/config.js';
import { enterWorld } from './lib/sim.mjs';

const [,, user = 'sima', pass = 'sima', charName = 'SimA'] = process.argv;
const DARRYL = { x: -479091, y: 4753, z: -66202 };
const DARRYL_CID = 0x40000000 + 3;
const SHOP_FUNCTION = 9;
const POTION_TID = 190;
const STOP = 0x04;
const step = (t) => console.log(`\n=== ${t}`);

step('préparation : 1000 pièces d’or');
const cfg = loadConfig();
const db = await mariadb.createConnection({ host: cfg.database.host, port: cfg.database.port, user: cfg.database.user, password: cfg.database.password, database: cfg.database.characters });
await db.query('UPDATE characters SET money = 1000, pos_x = ?, pos_y = ?, pos_z = ? WHERE name = ?', [DARRYL.x + 200, DARRYL.y, DARRYL.z, charName]);
await db.end();

step(`${charName} entre en jeu près de Darryl`);
const A = await enterWorld({ user, pass, charName, label: 'A-' });
A.world.send(Main.OPT_CHAR_MOVE, sparse(Main.OPT_CHAR_MOVE, { 0: A.cid, 1: { ...DARRYL, x: DARRYL.x + 200 }, 2: { ...DARRYL, x: DARRYL.x + 200 }, 5: STOP, 6: 0 }));

step('ouverture de la boutique');
const base = buildMini(Mini.EVENT_BASE, sparse(Mini.EVENT_BASE, { 0: 3, 1: DARRYL_CID, 3: SHOP_FUNCTION }));
const items = [];
const collect = (p) => {
  if (p.type === Main.EVENT_NPC_TRADE.id && p.values[1] === 0) items.push({ iid: p.values[3], detail: decodeMini(Mini.NPC_TRADE_DETAIL, p.values[4]).values });
  return p.type === Main.EVENT_NPC_TRADE.id && p.values[1] === 1;
};
A.world.send(Main.EVENT_NPC_TRADE, sparse(Main.EVENT_NPC_TRADE, { 0: base, 1: 0, 2: A.cid }));
await A.world.expectWhere(collect, 5000, 'fin de liste (op 1)');
console.log(`  ✓ ${items.length} objet(s) reçus`);
const potion = items.find((i) => i.detail[0] === POTION_TID);
if (!potion) throw new Error('potion absente de la boutique');
console.log(`  ✓ potion TID ${POTION_TID} : IID ${potion.iid}, prix ${potion.detail[1]}, case ${potion.detail.slice(4, 7).join('/')}`);

step('achat de 3 potions');
A.world.send(Main.EVENT_NPC_TRADE, sparse(Main.EVENT_NPC_TRADE, { 0: base, 1: 2, 2: A.cid, 3: potion.iid, 4: buildMini(Mini.NPC_TRADE_DETAIL, sparse(Mini.NPC_TRADE_DETAIL, { 2: 3 })) }));
let bought = null;
await A.world.expectWhere((p) => {
  if (p.type === Main.ITEM.id && p.values[0] === 0 && p.values[3] === POTION_TID && p.values[1] === 2) bought = p.values;
  return p.type === Main.EVENT_NPC_TRADE.id && p.values[1] === 2;
}, 5000, 'résultat d’achat').then((p) => { if (p.values[6] !== 2) throw new Error(`achat refusé : résultat ${p.values[6]}`); });
if (!bought) throw new Error('objet acheté non reçu');
console.log(`  ✓ achat réussi : IID ${bought[2]}, quantité ${bought[5]}, case ${decodeMini(Mini.ITEM_GRID, bought[7]).values.join('/')}`);

step('revente');
A.world.send(Main.EVENT_NPC_TRADE, sparse(Main.EVENT_NPC_TRADE, { 0: base, 1: 3, 2: A.cid, 3: bought[2] }));
const sell = await A.world.expectWhere((p) => p.type === Main.EVENT_NPC_TRADE.id && p.values[1] === 3, 5000, 'résultat de vente');
if (sell.values[6] !== 0) throw new Error(`vente refusée : résultat ${sell.values[6]}`);
console.log('  ✓ vente réussie');

A.world.close();
console.log('\nboutique : OK');
