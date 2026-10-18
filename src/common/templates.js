// Chargement des templates de personnages depuis archcore_world.character_template.

import { log } from './log.js';

/** TID des personnages de base proposés à la création (CharacterDataTable : Archer=1, Sorcerer=3, …). */
export const BASE_TEMPLATE_NAMES = ['Knight', 'Archer', 'Mage', 'Berserker', 'Hunter', 'Sorcerer', 'Swashbuckler', 'Ranger', 'Elementalist', 'Scion', 'Slayer', 'Orbiter', 'Summoner', 'Warrior'];

export async function loadTemplates(db) {
  const rows = await db.world.query('SELECT * FROM character_template');
  const templates = new Map(rows.map((r) => [Number(r.tid), r]));
  const base = await db.world.query(
    'SELECT tid, name FROM character_data_table WHERE name IN (?) ORDER BY tid',
    [BASE_TEMPLATE_NAMES],
  );
  // Écarte les PNJ homonymes (« <Titre> Nom ») : seuls les modèles jouables vont à l'écran de création.
  const baseTemplates = base.map((r) => Number(r.tid)).filter((tid) => templates.has(tid) && !String(templates.get(tid).name ?? '').includes('<'));
  log.info(`${templates.size} template(s) de personnage chargé(s), ${baseTemplates.length} de base : ${baseTemplates.join(', ')}`);
  return { templates, baseTemplates };
}
