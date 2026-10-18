// Client de test ArchCore : rejoue le flux complet d'un client ArchLord contre le login puis le monde.
//   node tools/client-sim.mjs [utilisateur] [motdepasse] [nomPerso]
// Sert à valider le serveur sans lancer le jeu. Les échanges suivent le protocole relevé
// dans l'exécutable du client ; le vrai client peut différer dans l'ordre des opérations.

import { sparse } from '../src/net/packet.js';
import { Main, ChatType } from '../src/net/schema.js';
import { enterWorld } from './lib/sim.mjs';

// Arguments suivants : commandes envoyées dans la discussion une fois en jeu (par défaut /GetExp)
const [,, user = 'test', pass = 'test', charName = 'Testeur', ...extra] = process.argv;
const chatCommands = extra.length ? extra : ['/GetExp'];
const { world, cid } = await enterWorld({ user, pass, charName });

// Commande dans la discussion, comme le client (FUN_1402e3180) : op 0, CID, type 0, message sans zéro final
for (const command of chatCommands) {
  world.send(Main.CHATTING, sparse(Main.CHATTING, { 0: 0, 1: cid, 2: ChatType.NORMAL, 6: Buffer.from(command, 'latin1') }));
  const replies = [await world.expectWhere((q) => q.type === Main.CHATTING.id, 5000, `réponse à ${command}`)];
  // une commande peut produire plusieurs messages (ex. /chatban sur soi-même) : on les recueille tous
  for (;;) {
    const more = await world.expectWhere((q) => q.type === Main.CHATTING.id, 300).catch(() => null);
    if (!more) break;
    replies.push(more);
  }
  console.log(`  ${command}`);
  for (const r of replies) console.log(`    → [type ${r.values[2]}] ${r.values[6].toString('latin1')}`);
}
console.log('  flux complet OK');
world.close();
