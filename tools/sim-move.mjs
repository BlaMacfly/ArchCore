// Test de déplacement à deux joueurs : chacun voit l'autre, les déplacements sont relayés,
// et un joueur qui s'éloigne disparaît de la vue de l'autre.
//   node tools/sim-move.mjs [compteA] [mdpA] [persoA] [compteB] [mdpB] [persoB]
// À lancer contre un serveur de test : ses personnages sont déplacés et leur position sauvegardée.

import { sparse } from '../src/net/packet.js';
import { Main, CharMgrOp } from '../src/net/schema.js';
import { enterWorld } from './lib/sim.mjs';

const [,, ua = 'sima', pa = 'sima', ca = 'SimA', ub = 'simb', pb = 'simb', cb = 'SimB'] = process.argv;
const STOP = 0x04;
const step = (t) => console.log(`\n=== ${t}`);
const isChar = (op, cid) => (p) => p.type === Main.CHARACTER.id && p.values[0] === op && p.values[1] === cid;

step(`${ca} entre en jeu`);
const A = await enterWorld({ user: ua, pass: pa, charName: ca, label: 'A-' });
step(`${cb} entre en jeu`);
const B = await enterWorld({ user: ub, pass: pb, charName: cb, label: 'B-' });
const start = B.loading.values[5];

// 1. Visibilité mutuelle à l'arrivée de B (même village de départ).
await B.world.expectWhere(isChar(0, A.cid), 3000, `${cb} reçoit ${ca}`);
await A.world.expectWhere(isChar(0, B.cid), 3000, `${ca} reçoit ${cb}`);
console.log(`  ✓ ${ca} et ${cb} se voient`);

// 2. A se déplace : B reçoit le déplacement.
const dest = { x: start.x + 600, y: start.y, z: start.z + 300 };
A.world.send(Main.OPT_CHAR_MOVE, sparse(Main.OPT_CHAR_MOVE, { 0: A.cid, 1: start, 2: dest, 5: 0, 6: 0 }));
const mv = await B.world.expectWhere((p) => p.type === Main.OPT_CHAR_MOVE.id && p.values[0] === A.cid, 3000, 'déplacement relayé');
const d = mv.values[2];
if (Math.abs(d.x - dest.x) > 0.5 || Math.abs(d.z - dest.z) > 0.5) throw new Error(`destination relayée inattendue : ${JSON.stringify(d)}`);
console.log(`  ✓ ${cb} voit ${ca} partir vers ${d.x.toFixed(0)}, ${d.z.toFixed(0)}`);

// 3. A part très loin et s'arrête : chacun retire l'autre de sa vue.
const far = { x: start.x + 50_000, y: start.y, z: start.z };
A.world.send(Main.OPT_CHAR_MOVE, sparse(Main.OPT_CHAR_MOVE, { 0: A.cid, 1: far, 2: far, 5: STOP, 6: 0 }));
await B.world.expectWhere(isChar(2, A.cid), 3000, `${cb} retire ${ca}`);
await A.world.expectWhere(isChar(2, B.cid), 3000, `${ca} retire ${cb}`);
console.log('  ✓ hors de portée : chacun retire l’autre');

// 4. A revient près de B : ils se revoient.
A.world.send(Main.OPT_CHAR_MOVE, sparse(Main.OPT_CHAR_MOVE, { 0: A.cid, 1: start, 2: start, 5: STOP, 6: 0 }));
await B.world.expectWhere(isChar(0, A.cid), 3000, `${cb} revoit ${ca}`);
console.log('  ✓ de retour à portée : ils se revoient');

// 5. A se déconnecte : B le retire.
A.world.send(Main.CHAR_MANAGER, sparse(Main.CHAR_MANAGER, { 0: CharMgrOp.KEEPALIVE, 2: 0 }));
A.world.close();
await B.world.expectWhere(isChar(2, A.cid), 3000, `${cb} retire ${ca} déconnecté`);
console.log('  ✓ déconnexion : le personnage disparaît chez l’autre joueur');

B.world.close();
console.log('\ndéplacement à deux joueurs : OK');
