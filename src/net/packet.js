// Codec des paquets ArchLord (format "AuPacket").
//
// Paquet principal (en clair, avant chiffrement) :
//   0x00  u8   garde          0xD6
//   0x01  u32  taille totale  (en-tête + drapeaux + champs + pied)
//   0x05  u8   type de paquet
//   0x06  u8   drapeaux d'en-tête (bit0 effacé par MakePacket)
//   0x07  u32  identifiant propriétaire (session)
//   0x0B  u32  réservé
//   0x0F  u32  tick / timestamp
//   0x13  drapeaux (1, 2 ou 4 octets selon le type) : bit i = champ i présent
//   ....  champs présents, dans l'ordre du schéma
//   fin   u8   pied           0x6B
//
// Mini-paquet (imbriqué dans un champ PACKET) :
//   u32 longueur (drapeaux + champs), puis drapeaux, puis champs.
// Bloc mémoire : u32 longueur + octets. (u16 dans le client 32 bits d'origine.)
//
// Les entiers et flottants sont little-endian.
// NB : l'en-tête original (32 bits) faisait 13 octets (u16 taille, u16 type, u32 owner, u32 tick) ;
// le client x64 l'a élargi à 19 octets (relevé sur le fil : d6 16000000 4800 … ).

export const T = Object.freeze({
  CHAR: 0,
  INT8: 1,
  UINT8: 2,
  INT16: 3,
  UINT16: 4,
  INT32: 5,
  UINT32: 6,
  INT64: 7,
  UINT64: 8,
  FLOAT: 9,
  VEC3F: 10,
  MATRIX: 11,
  PACKET: 12,
  MEMORY_BLOCK: 13,
  POS_BASEMETER: 14,
  WCHAR: 15,
});

export const GUARD_HEAD = 0xd6;
export const GUARD_TAIL = 0x6b;
export const HEADER_SIZE = 19;

const SCALAR_SIZE = { 1: 1, 2: 1, 3: 2, 4: 2, 5: 4, 6: 4, 7: 8, 8: 8, 9: 4 };

class ByteWriter {
  constructor(initial = 64) {
    this.buf = Buffer.alloc(initial);
    this.pos = 0;
  }
  #ensure(n) {
    if (this.pos + n <= this.buf.length) return;
    let size = this.buf.length * 2;
    while (size < this.pos + n) size *= 2;
    const next = Buffer.alloc(size);
    this.buf.copy(next, 0, 0, this.pos);
    this.buf = next;
  }
  u8(v) { this.#ensure(1); this.buf.writeUInt8(v & 0xff, this.pos); this.pos += 1; }
  i8(v) { this.#ensure(1); this.buf.writeInt8(v, this.pos); this.pos += 1; }
  u16(v) { this.#ensure(2); this.buf.writeUInt16LE(v & 0xffff, this.pos); this.pos += 2; }
  i16(v) { this.#ensure(2); this.buf.writeInt16LE(v, this.pos); this.pos += 2; }
  u32(v) { this.#ensure(4); this.buf.writeUInt32LE(v >>> 0, this.pos); this.pos += 4; }
  i32(v) { this.#ensure(4); this.buf.writeInt32LE(v | 0, this.pos); this.pos += 4; }
  u64(v) { this.#ensure(8); this.buf.writeBigUInt64LE(BigInt(v), this.pos); this.pos += 8; }
  i64(v) { this.#ensure(8); this.buf.writeBigInt64LE(BigInt(v), this.pos); this.pos += 8; }
  f32(v) { this.#ensure(4); this.buf.writeFloatLE(v, this.pos); this.pos += 4; }
  bytes(b) { this.#ensure(b.length); b.copy(this.buf, this.pos); this.pos += b.length; }
  /** Chaîne à taille fixe, complétée par des zéros. */
  fixedString(str, size) {
    this.#ensure(size);
    this.buf.fill(0, this.pos, this.pos + size);
    const src = Buffer.isBuffer(str) ? str : Buffer.from(String(str ?? ''), 'latin1');
    src.copy(this.buf, this.pos, 0, Math.min(src.length, size));
    this.pos += size;
  }
  result() { return this.buf.subarray(0, this.pos); }
}

function writeScalar(w, type, v) {
  switch (type) {
    case T.INT8: return w.i8(v);
    case T.UINT8: return w.u8(v);
    case T.INT16: return w.i16(v);
    case T.UINT16: return w.u16(v);
    case T.INT32: return w.i32(v);
    case T.UINT32: return w.u32(v);
    case T.INT64: return w.i64(v);
    case T.UINT64: return w.u64(v);
    case T.FLOAT: return w.f32(v);
    default: throw new TypeError(`type scalaire inconnu ${type}`);
  }
}

function readScalar(buf, pos, type) {
  switch (type) {
    case T.INT8: return [buf.readInt8(pos), pos + 1];
    case T.UINT8: return [buf.readUInt8(pos), pos + 1];
    case T.INT16: return [buf.readInt16LE(pos), pos + 2];
    case T.UINT16: return [buf.readUInt16LE(pos), pos + 2];
    case T.INT32: return [buf.readInt32LE(pos), pos + 4];
    case T.UINT32: return [buf.readUInt32LE(pos), pos + 4];
    case T.INT64: return [buf.readBigInt64LE(pos), pos + 8];
    case T.UINT64: return [buf.readBigUInt64LE(pos), pos + 8];
    case T.FLOAT: return [buf.readFloatLE(pos), pos + 4];
    default: throw new TypeError(`type scalaire inconnu ${type}`);
  }
}

function toVec3(v) {
  if (Array.isArray(v)) return { x: v[0] ?? 0, y: v[1] ?? 0, z: v[2] ?? 0 };
  return { x: v?.x ?? 0, y: v?.y ?? 0, z: v?.z ?? 0 };
}

/**
 * Encode les champs d'un schéma. `values[i]` correspond au champ i ; `null`/`undefined`
 * signifie « champ absent » (bit de drapeau à 0, aucun octet écrit).
 * @returns {{flags:number, body:Buffer}}
 */
export function encodeFields(schema, values) {
  const w = new ByteWriter();
  let flags = 0;
  schema.fields.forEach(([type, size], i) => {
    const v = values[i];
    if (v === null || v === undefined) return;
    const bit = 1 << i;
    switch (type) {
      case T.CHAR:
        w.fixedString(v, size);
        flags |= bit;
        break;
      case T.VEC3F: {
        const list = size > 1 ? v : [v];
        for (const item of list) {
          const { x, y, z } = toVec3(item);
          w.f32(x); w.f32(y); w.f32(z);
        }
        flags |= bit;
        break;
      }
      case T.PACKET:
        if (size > 1) {
          const list = Array.isArray(v) ? v : [v];
          w.u8(list.length);
          for (const mini of list) w.bytes(mini);
        } else {
          w.bytes(v);
        }
        flags |= bit;
        break;
      case T.MEMORY_BLOCK: {
        const data = Buffer.isBuffer(v) ? v : Buffer.from(String(v), 'latin1');
        w.u32(data.length);
        if (data.length) {
          w.bytes(data);
          flags |= bit; // comme la référence : un bloc vide n'active pas le drapeau
        }
        break;
      }
      case T.MATRIX:
      case T.POS_BASEMETER:
      case T.WCHAR:
        throw new TypeError(`type de champ ${type} non pris en charge (schéma ${schema.name})`);
      default: {
        const list = size > 1 ? v : [v];
        for (const item of list) writeScalar(w, type, item);
        flags |= bit;
      }
    }
  });
  return { flags, body: w.result() };
}

function writeFlags(w, flags, flagLen) {
  if (flagLen === 1) w.u8(flags);
  else if (flagLen === 2) w.u16(flags);
  else if (flagLen === 4) w.u32(flags);
  else throw new RangeError(`longueur de drapeaux invalide ${flagLen}`);
}

function readFlags(buf, pos, flagLen) {
  if (flagLen === 1) return [buf.readUInt8(pos), pos + 1];
  if (flagLen === 2) return [buf.readUInt16LE(pos), pos + 2];
  if (flagLen === 4) return [buf.readUInt32LE(pos), pos + 4];
  throw new RangeError(`longueur de drapeaux invalide ${flagLen}`);
}

/** Construit un paquet principal complet (en clair). */
export function buildPacket(schema, values, { sessionId = 0, timestamp = 0 } = {}) {
  const { flags, body } = encodeFields(schema, values);
  const w = new ByteWriter(HEADER_SIZE + schema.flagLen + body.length + 1);
  w.u8(GUARD_HEAD);
  w.u32(0); // taille, patchée ci-dessous
  w.u8(schema.id);
  w.u8(0); // drapeaux d'en-tête
  w.u32(sessionId);
  w.u32(0);
  w.u32(timestamp);
  writeFlags(w, flags, schema.flagLen);
  w.bytes(body);
  w.u8(GUARD_TAIL);
  const out = w.result();
  out.writeUInt32LE(out.length, 1);
  return out;
}

/** Construit un mini-paquet (u16 longueur + drapeaux + champs). */
export function buildMini(schema, values) {
  const { flags, body } = encodeFields(schema, values);
  const w = new ByteWriter(4 + schema.flagLen + body.length);
  w.u32(schema.flagLen + body.length);
  writeFlags(w, flags, schema.flagLen);
  w.bytes(body);
  return w.result();
}

/**
 * Décode les champs à partir de `pos` dans `buf` selon `schema` et `flags`.
 * Les champs absents valent `null`. Les PACKET sont rendus en tampons bruts
 * (longueur exclue) à décoder avec `decodeMini`.
 */
export function decodeFields(schema, buf, pos, flags, end = buf.length) {
  const values = new Array(schema.fields.length).fill(null);
  for (let i = 0; i < schema.fields.length; i++) {
    const bit = 1 << i;
    if (bit > flags) break;
    if (!(flags & bit)) continue;
    const [type, size] = schema.fields[i];
    switch (type) {
      case T.CHAR: {
        if (pos + size > end) throw new RangeError(`lecture hors limites (CHAR, champ ${i} de ${schema.name})`);
        const raw = buf.subarray(pos, pos + size);
        const nul = raw.indexOf(0);
        values[i] = { raw: Buffer.from(raw), text: raw.toString('latin1', 0, nul < 0 ? size : nul) };
        pos += size;
        break;
      }
      case T.VEC3F: {
        const list = [];
        for (let n = 0; n < size; n++) {
          list.push({ x: buf.readFloatLE(pos), y: buf.readFloatLE(pos + 4), z: buf.readFloatLE(pos + 8) });
          pos += 12;
        }
        values[i] = size > 1 ? list : list[0];
        break;
      }
      case T.PACKET: {
        if (size > 1) {
          const count = buf.readUInt8(pos); pos += 1;
          const list = [];
          for (let n = 0; n < count; n++) {
            const len = buf.readUInt32LE(pos); pos += 4;
            if (pos + len > end) throw new RangeError(`mini-paquet hors limites (champ ${i} de ${schema.name})`);
            list.push(Buffer.from(buf.subarray(pos, pos + len)));
            pos += len;
          }
          values[i] = list;
        } else {
          const len = buf.readUInt32LE(pos); pos += 4;
          if (pos + len > end) throw new RangeError(`mini-paquet hors limites (champ ${i} de ${schema.name})`);
          values[i] = Buffer.from(buf.subarray(pos, pos + len));
          pos += len;
        }
        break;
      }
      case T.MEMORY_BLOCK: {
        const len = buf.readUInt32LE(pos); pos += 4;
        if (pos + len > end) throw new RangeError(`bloc mémoire hors limites (champ ${i} de ${schema.name})`);
        values[i] = Buffer.from(buf.subarray(pos, pos + len));
        pos += len;
        break;
      }
      case T.MATRIX:
      case T.POS_BASEMETER:
      case T.WCHAR:
        throw new TypeError(`type de champ ${type} non pris en charge en lecture (schéma ${schema.name})`);
      default: {
        const list = [];
        for (let n = 0; n < size; n++) {
          let v;
          [v, pos] = readScalar(buf, pos, type);
          list.push(v);
        }
        values[i] = size > 1 ? list : list[0];
      }
    }
  }
  return { values, pos };
}

/** Lit l'en-tête d'un paquet principal en clair. */
export function parseHeader(buf) {
  if (buf.length < HEADER_SIZE + 1 || buf[0] !== GUARD_HEAD) return null;
  return {
    size: buf.readUInt32LE(1),
    type: buf.readUInt8(5),
    headerFlags: buf.readUInt8(6),
    sessionId: buf.readUInt32LE(7),
    timestamp: buf.readUInt32LE(15),
  };
}

/** Décode un paquet principal complet selon `schema` (doit correspondre au type). */
export function decodePacket(schema, buf) {
  const header = parseHeader(buf);
  if (!header) throw new RangeError('en-tête de paquet invalide');
  if (header.type !== schema.id) throw new RangeError(`type ${header.type} ≠ schéma ${schema.name} (${schema.id})`);
  const end = Math.min(header.size, buf.length) - 1; // exclut le pied 0x6B
  const [flags, start] = readFlags(buf, HEADER_SIZE, schema.flagLen);
  const { values } = decodeFields(schema, buf, start, flags, end);
  return { ...header, flags, values };
}

/** Décode un mini-paquet (tampon sans son u16 de longueur). */
export function decodeMini(schema, payload) {
  const [flags, start] = readFlags(payload, 0, schema.flagLen);
  const { values } = decodeFields(schema, payload, start, flags, payload.length);
  return { flags, values };
}

/** Aide : construit un tableau de valeurs à partir d'un objet {indice: valeur}. */
export function sparse(schema, map) {
  const values = new Array(schema.fields.length).fill(null);
  for (const [k, v] of Object.entries(map)) values[Number(k)] = v;
  return values;
}

export function hexDump(buf, max = 96) {
  const slice = buf.subarray(0, max);
  return slice.toString('hex').replace(/(..)/g, '$1 ').trim() + (buf.length > max ? ` … (+${buf.length - max})` : '');
}
