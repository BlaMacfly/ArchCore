// Commandes de maître du jeu (MJ), reçues comme messages de discussion « /commande arguments ».
//
// Le panneau « Admin Commands » du client tape les mêmes commandes que la saisie manuelle
// (FUN_14059aeb0 ajoute le « / » puis appelle AgcmUIChatting2, FUN_1404d7dc0). Le serveur décide
// seul si une commande est permise, selon account.gm_level.
//
// Syntaxe : les noms entre guillemets doubles (espaces permis), les nombres sans guillemets, la
// casse du nom de commande compte (SetExp, chatban). Voir docs/wiki/FR-Administration.md.

export class GmError extends Error {}

/** Découpe une ligne de commande : "texte entre guillemets" ou mots séparés par des espaces. */
export function tokenize(input) {
  const tokens = [];
  let i = 0;
  const s = String(input ?? '');
  while (i < s.length) {
    if (/\s/.test(s[i])) { i++; continue; }
    if (s[i] === '"') {
      const end = s.indexOf('"', i + 1);
      if (end < 0) throw new GmError('guillemet fermant manquant');
      tokens.push(s.slice(i + 1, end));
      i = end + 1;
    } else {
      let j = i;
      while (j < s.length && !/\s/.test(s[j]) && s[j] !== '"') j++;
      tokens.push(s.slice(i, j));
      i = j;
    }
  }
  return tokens;
}

/** « /nom arg1 "arg 2" » → { name, args } ; null si le texte n'est pas une commande. */
export function parseCommand(text) {
  const s = String(text ?? '').trim();
  if (!s.startsWith('/') || s.length < 2) return null;
  const m = s.slice(1).match(/^(\S+)\s*(.*)$/s);
  if (!m) return null;
  return { name: m[1], args: tokenize(m[2]) };
}

const NUMBER = /^[-+]?\d+(?:[.,]\d+)?$/;
const INTEGER = /^[-+]?\d+$/;

/**
 * Convertit les arguments selon une spécification : 'name', 'int', 'float', 'word', avec « ? » pour
 * un argument facultatif (null s'il manque). Lève GmError en cas d'erreur.
 */
export function parseArgs(spec, tokens) {
  const out = [];
  const required = spec.filter((t) => !t.endsWith('?')).length;
  if (tokens.length < required) throw new GmError('argument manquant');
  if (tokens.length > spec.length) throw new GmError('trop d\'arguments');
  spec.forEach((t, i) => {
    const kind = t.replace(/\?$/, '');
    const tok = tokens[i];
    if (tok === undefined) { out.push(null); return; }
    switch (kind) {
      case 'int':
        if (!INTEGER.test(tok)) throw new GmError(`nombre entier attendu : ${tok}`);
        out.push(Number.parseInt(tok, 10));
        break;
      case 'float':
        if (!NUMBER.test(tok)) throw new GmError(`nombre attendu : ${tok}`);
        out.push(Number.parseFloat(tok.replace(',', '.')));
        break;
      case 'name':
        if (!tok.trim()) throw new GmError('nom vide');
        out.push(tok.trim());
        break;
      default:
        out.push(tok);
    }
  });
  return out;
}

/** Coordonnées de /move : « x,z », « x z », « x,y,z » ou « x y z ». */
export function parseCoordinates(tokens) {
  const parts = tokens.join(' ').split(/[\s,;]+/).filter(Boolean);
  if (parts.length !== 2 && parts.length !== 3) throw new GmError('coordonnées attendues : x,z');
  const nums = parts.map((p) => {
    if (!/^[-+]?\d+(?:\.\d+)?$/.test(p)) throw new GmError(`coordonnée invalide : ${p}`);
    return Number.parseFloat(p);
  });
  return nums.length === 2 ? { x: nums[0], y: null, z: nums[1] } : { x: nums[0], y: nums[1], z: nums[2] };
}

// ---------------------------------------------------------------------------------------
// Taux du serveur (en mémoire, valeurs initiales dans config.world.rates)
// ---------------------------------------------------------------------------------------
export const RATES = Object.freeze({
  exp: { label: 'expérience', set: 'SetExp', get: 'GetExp' },
  gold: { label: 'or', set: 'SetGold', get: 'GetGold' },
  drop: { label: 'butin', set: 'SetDrop', get: 'GetDrop' },
  uniqueDrop: { label: 'butin unique', set: 'SetUniqueDrop', get: 'GetUniqueDrop' },
  charisma: { label: 'charisme', set: 'SetCharisma', get: 'GetCharisma' },
  prestige: { label: 'prestige', set: 'SetPrestigeRate', get: 'GetPrestigeRate' },
});

export const MAX_RATE = 100;

export class ServerRates {
  constructor(initial = {}) {
    this.values = { exp: 1, gold: 1, drop: 1, uniqueDrop: 1, charisma: 1, prestige: 1, chatBuff: 1 };
    for (const [k, v] of Object.entries(initial)) if (k in this.values && Number.isFinite(Number(v))) this.values[k] = Number(v);
  }
  get(key) { return this.values[key]; }
  set(key, value) { this.values[key] = value; }
}

const fmtRate = (v) => (Number.isInteger(v) ? v.toFixed(1) : String(v));

// ---------------------------------------------------------------------------------------
// Table des commandes
// ---------------------------------------------------------------------------------------
// `level` = niveau MJ minimal par défaut (modifiable par config.world.gmCommandLevels).
// `run(ctx, args)` : ctx.reply(texte), ctx.world (WorldHandlers ou substitut de test), ctx.rates,
// ctx.session (session du MJ).

function rateCommands() {
  const cmds = {};
  for (const [key, def] of Object.entries(RATES)) {
    cmds[def.set] = {
      level: 3, args: ['float'], usage: `/${def.set} <taux>`,
      run(ctx, [value]) {
        if (value < 0 || value > MAX_RATE) throw new GmError(`le taux doit être compris entre 0 et ${MAX_RATE}`);
        ctx.rates.set(key, value);
        ctx.reply(`Taux ${rateLabel(def.label)} réglé à ${fmtRate(value)}.`);
      },
    };
    cmds[def.get] = {
      level: 1, args: [], usage: `/${def.get}`,
      run(ctx) { ctx.reply(`Taux ${rateLabel(def.label)} : ${fmtRate(ctx.rates.get(key))}`); },
    };
  }
  return cmds;
}

function rateLabel(label) {
  return /^[aeéiou]/i.test(label) ? `d'${label}` : `de ${label}`;
}

export const COMMANDS = Object.freeze({
  ...rateCommands(),

  SetChatBuff: {
    level: 3, args: ['int'], usage: '/SetChatBuff <0|1>',
    run(ctx, [on]) {
      if (on !== 0 && on !== 1) throw new GmError('valeur attendue : 0 ou 1');
      ctx.rates.set('chatBuff', on);
      ctx.reply(on ? 'Bonus de discussion activé.' : 'Bonus de discussion désactivé.');
    },
  },
  GetChatBuff: {
    level: 1, args: [], usage: '/GetChatBuff',
    run(ctx) { ctx.reply(`Bonus de discussion : ${ctx.rates.get('chatBuff') ? 'activé' : 'désactivé'}`); },
  },

  chatban: {
    level: 1, args: ['name', 'int'], usage: '/chatban "nom" <minutes>',
    async run(ctx, [name, minutes]) {
      if (minutes < 0) throw new GmError('durée négative');
      const target = await ctx.world.chatBan(name, minutes);
      if (!target) throw new GmError(`personnage introuvable : ${name}`);
      ctx.reply(minutes ? `${target} ne peut plus parler pendant ${minutes} min.` : `${target} peut de nouveau parler.`);
    },
  },

  kill: {
    level: 2, args: ['name?'], usage: '/kill "nom"',
    async run(ctx, [name]) {
      const target = name ? ctx.world.findOnline(name) : ctx.session;
      if (!target) throw new GmError(`personnage absent du monde : ${name}`);
      await ctx.world.killCharacter(target);
      ctx.reply(`${target.state.character.name} a été tué.`);
    },
  },

  rename_char: {
    level: 2, args: ['name', 'name'], usage: '/rename_char "ancien" "nouveau"',
    async run(ctx, [oldName, newName]) {
      if (!/^[^\s"#\\]{2,24}$/.test(newName)) throw new GmError(`nom invalide : ${newName} (2 à 24 caractères, sans espace)`);
      const result = await ctx.world.renameCharacter(oldName, newName);
      if (result === 'missing') throw new GmError(`personnage introuvable : ${oldName}`);
      if (result === 'taken') throw new GmError(`nom déjà pris : ${newName}`);
      ctx.reply(`${oldName} s'appelle désormais ${newName}${result === 'online' ? ' (visible à sa prochaine connexion)' : ''}.`);
    },
  },

  create: {
    level: 3, args: ['name', 'int', 'int?', 'int?'], usage: '/create "nom" <TID> <quantité> <0|1>',
    async run(ctx, [name, tid, count, bound]) {
      const qty = count ?? 1;
      if (qty < 1 || qty > 9999) throw new GmError('quantité entre 1 et 9999');
      const res = await ctx.world.createItem(name, tid, qty, bound === 1);
      if (res.error === 'character') throw new GmError(`personnage introuvable : ${name}`);
      if (res.error === 'item') throw new GmError(`objet inconnu : TID ${tid}`);
      if (res.error === 'full') throw new GmError(`inventaire plein pour ${name}`);
      ctx.reply(`${qty} × ${res.itemName} (TID ${tid}) donné(s) à ${res.characterName}${bound === 1 ? ', non échangeable(s)' : ''}.`);
    },
  },

  announce_restart: {
    level: 3, args: ['int', 'int?'], usage: '/announce_restart <minutes> <secondes>',
    run(ctx, [minutes, seconds]) {
      if (minutes < 0 || (seconds ?? 0) < 0) throw new GmError('durée négative');
      const total = minutes * 60 + (seconds ?? 0);
      if (total === 0) {
        ctx.reply(ctx.world.cancelRestart() ? 'Redémarrage annulé.' : 'Aucun redémarrage prévu.');
        return;
      }
      ctx.world.scheduleRestart(total, seconds ?? 0);
      ctx.reply(`Redémarrage annoncé dans ${formatDuration(total)}.`);
    },
  },

  move: {
    level: 1, args: null, usage: '/move x,z',
    async run(ctx, tokens) {
      const pos = parseCoordinates(tokens);
      const done = await ctx.world.teleport(ctx.session, pos);
      ctx.reply(`Téléporté en ${Math.round(done.x)}, ${Math.round(done.z)}.`);
    },
  },
});

export function formatDuration(totalSeconds) {
  const m = Math.floor(totalSeconds / 60);
  const s = totalSeconds % 60;
  if (!m) return `${s} seconde${s > 1 ? 's' : ''}`;
  if (!s) return `${m} minute${m > 1 ? 's' : ''}`;
  return `${m} min ${s} s`;
}

/**
 * Interpréteur de commandes MJ.
 *   const gm = new GmCommands({ world, rates, levels });
 *   await gm.execute(session, '/SetExp 2.0', reply);
 */
export class GmCommands {
  constructor({ world, rates = new ServerRates(), levels = {}, commands = COMMANDS } = {}) {
    this.world = world;
    this.rates = rates;
    this.commands = commands;
    this.levels = levels;
  }

  /** Niveau MJ requis pour une commande (config.world.gmCommandLevels prioritaire). */
  levelFor(name) {
    const override = this.levels[name];
    return Number.isInteger(override) ? override : this.commands[name]?.level ?? Infinity;
  }

  /**
   * Exécute un texte commençant par « / ». Renvoie false si ce n'est pas une commande.
   * `reply(texte)` répond au MJ. Les erreurs sont rapportées au MJ, jamais propagées.
   */
  async execute(session, text, reply) {
    const parsed = (() => {
      try { return parseCommand(text); } catch (err) { return { error: err }; }
    })();
    if (!parsed) return false;
    const gmLevel = session?.state?.gmLevel ?? 0;
    if (parsed.error) { reply(`Commande invalide : ${parsed.error.message}`); return true; }
    const cmd = this.commands[parsed.name];
    // Un joueur sans droits ne voit pas la liste des commandes MJ.
    if (!cmd || gmLevel === 0) {
      reply(`Commande inconnue : /${parsed.name}`);
      return true;
    }
    const required = this.levelFor(parsed.name);
    if (gmLevel < required) {
      reply(`Niveau MJ ${required} requis pour /${parsed.name} (le vôtre : ${gmLevel}).`);
      return true;
    }
    try {
      const args = cmd.args ? parseArgs(cmd.args, parsed.args) : parsed.args;
      await cmd.run({ session, world: this.world, rates: this.rates, reply }, args);
    } catch (err) {
      if (!(err instanceof GmError)) throw err;
      reply(`/${parsed.name} : ${err.message}. Syntaxe : ${cmd.usage}`);
    }
    return true;
  }
}
