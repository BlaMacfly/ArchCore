//! Codec des paquets ArchLord (format « AuPacket »).
//!
//! Paquet principal (en clair, avant chiffrement) :
//! ```text
//!   0x00  u8   garde          0xD6
//!   0x01  u32  taille totale  (en-tête + drapeaux + champs + pied)
//!   0x05  u8   type de paquet
//!   0x06  u8   drapeaux d'en-tête
//!   0x07  u32  identifiant propriétaire (session)
//!   0x0B  u32  réservé
//!   0x0F  u32  tick / horodatage
//!   0x13  drapeaux (1, 2 ou 4 octets selon le type) : bit i = champ i présent
//!   ....  champs présents, dans l'ordre du schéma
//!   fin   u8   pied           0x6B
//! ```
//! Mini-paquet (imbriqué dans un champ PACKET) : u32 longueur (drapeaux + champs), drapeaux, champs.
//! Bloc mémoire : u32 longueur + octets. Entiers et flottants little-endian.

use crate::Error;

pub const GUARD_HEAD: u8 = 0xd6;
pub const GUARD_TAIL: u8 = 0x6b;
pub const HEADER_SIZE: usize = 19;

/// Types de champ AuPacket (numérotation du client).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ty {
    Char,
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    F32,
    Vec3,
    Matrix,
    Packet,
    MemBlock,
    PosBaseMeter,
    WChar,
}

/// Un champ : type et taille (octets pour CHAR, nombre de répétitions sinon ; pour PACKET > 1 :
/// liste de mini-paquets précédée d'un u8 compteur).
#[derive(Clone, Copy, Debug)]
pub struct Field(pub Ty, pub u16);

#[derive(Debug)]
pub struct Schema {
    pub name: &'static str,
    pub id: u8,
    pub flag_len: u8,
    pub fields: &'static [Field],
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Vec3 { x, y, z }
    }
    pub fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f32),
    Vec3(Vec3),
    /// CHAR (octets bruts), PACKET (mini complet, longueur comprise, à l'encodage ; sans longueur au
    /// décodage), bloc mémoire.
    Bytes(Vec<u8>),
    List(Vec<Value>),
}

macro_rules! int_into_value {
    ($($t:ty),*) => { $(impl From<$t> for Value { fn from(v: $t) -> Self { Value::Int(v as i64) } })* };
}
int_into_value!(i8, u8, i16, u16, i32, u32, i64, u64, usize);

impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Value::Float(v)
    }
}
impl From<Vec3> for Value {
    fn from(v: Vec3) -> Self {
        Value::Vec3(v)
    }
}
impl From<Vec<u8>> for Value {
    fn from(v: Vec<u8>) -> Self {
        Value::Bytes(v)
    }
}
impl From<&[u8]> for Value {
    fn from(v: &[u8]) -> Self {
        Value::Bytes(v.to_vec())
    }
}
impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Bytes(crate::text::encode(v))
    }
}
impl From<&String> for Value {
    fn from(v: &String) -> Self {
        Value::Bytes(crate::text::encode(v))
    }
}
impl From<Vec<Value>> for Value {
    fn from(v: Vec<Value>) -> Self {
        Value::List(v)
    }
}

/// Valeurs des champs d'un paquet : `None` = champ absent (bit de drapeau à 0).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fields(pub Vec<Option<Value>>);

impl Fields {
    pub fn new(schema: &Schema) -> Self {
        Fields(vec![None; schema.fields.len()])
    }

    /// Fixe le champ `i` (constructeur chaînable).
    pub fn with(mut self, i: usize, v: impl Into<Value>) -> Self {
        self.0[i] = Some(v.into());
        self
    }

    /// Fixe le champ `i` si `v` est `Some`.
    pub fn with_opt<V: Into<Value>>(mut self, i: usize, v: Option<V>) -> Self {
        self.0[i] = v.map(Into::into);
        self
    }

    pub fn set(&mut self, i: usize, v: impl Into<Value>) {
        self.0[i] = Some(v.into());
    }

    pub fn get(&self, i: usize) -> Option<&Value> {
        self.0.get(i).and_then(|v| v.as_ref())
    }

    pub fn int(&self, i: usize) -> Option<i64> {
        match self.get(i)? {
            Value::Int(v) => Some(*v),
            _ => None,
        }
    }

    pub fn float(&self, i: usize) -> Option<f32> {
        match self.get(i)? {
            Value::Float(v) => Some(*v),
            _ => None,
        }
    }

    pub fn vec3(&self, i: usize) -> Option<Vec3> {
        match self.get(i)? {
            Value::Vec3(v) => Some(*v),
            _ => None,
        }
    }

    pub fn bytes(&self, i: usize) -> Option<&[u8]> {
        match self.get(i)? {
            Value::Bytes(v) => Some(v),
            _ => None,
        }
    }

    /// Texte d'un champ CHAR ou bloc mémoire, coupé au premier zéro, décodé en Windows-1252.
    pub fn text(&self, i: usize) -> Option<String> {
        let b = self.bytes(i)?;
        let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
        Some(crate::text::decode(&b[..end]))
    }

    pub fn list(&self, i: usize) -> Option<&[Value]> {
        match self.get(i)? {
            Value::List(v) => Some(v),
            _ => None,
        }
    }
}

/// En-tête d'un paquet principal en clair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub size: u32,
    pub ptype: u8,
    pub header_flags: u8,
    pub session_id: u32,
    pub timestamp: u32,
}

/// Paquet principal décodé.
#[derive(Clone, Debug)]
pub struct Packet {
    pub header: Header,
    pub flags: u32,
    pub fields: Fields,
}

impl std::ops::Deref for Packet {
    type Target = Fields;
    fn deref(&self) -> &Fields {
        &self.fields
    }
}

fn write_scalar(out: &mut Vec<u8>, ty: Ty, v: &Value, schema: &Schema) -> Result<(), Error> {
    let bad = || Error::Invalid(format!("valeur de type inattendu pour {ty:?} (schéma {})", schema.name));
    match (ty, v) {
        (Ty::F32, Value::Float(f)) => out.extend_from_slice(&f.to_le_bytes()),
        (Ty::F32, Value::Int(n)) => out.extend_from_slice(&(*n as f32).to_le_bytes()),
        (_, Value::Int(n)) => {
            let n = *n;
            match ty {
                Ty::I8 | Ty::U8 => out.push(n as u8),
                Ty::I16 | Ty::U16 => out.extend_from_slice(&(n as u16).to_le_bytes()),
                Ty::I32 | Ty::U32 => out.extend_from_slice(&(n as u32).to_le_bytes()),
                Ty::I64 | Ty::U64 => out.extend_from_slice(&(n as u64).to_le_bytes()),
                _ => return Err(bad()),
            }
        }
        (Ty::I32 | Ty::U32 | Ty::I16 | Ty::U16 | Ty::I8 | Ty::U8, Value::Float(f)) => {
            return write_scalar(out, ty, &Value::Int(f.round() as i64), schema)
        }
        _ => return Err(bad()),
    }
    Ok(())
}

/// Encode les champs d'un schéma ; rend (drapeaux, corps).
pub fn encode_fields(schema: &Schema, fields: &Fields) -> Result<(u32, Vec<u8>), Error> {
    let mut out = Vec::with_capacity(64);
    let mut flags = 0u32;
    for (i, Field(ty, size)) in schema.fields.iter().enumerate() {
        let Some(v) = fields.0.get(i).and_then(|v| v.as_ref()) else { continue };
        let bit = 1u32 << i;
        let size = *size as usize;
        match ty {
            Ty::Char => {
                let Value::Bytes(b) = v else {
                    return Err(Error::Invalid(format!("CHAR attendu (champ {i} de {})", schema.name)));
                };
                let mut fixed = vec![0u8; size];
                let n = b.len().min(size);
                fixed[..n].copy_from_slice(&b[..n]);
                out.extend_from_slice(&fixed);
                flags |= bit;
            }
            Ty::Vec3 => {
                let list: Vec<&Value> = match v {
                    Value::List(l) if size > 1 => l.iter().collect(),
                    other => vec![other],
                };
                for item in list {
                    let Value::Vec3(p) = item else {
                        return Err(Error::Invalid(format!("VEC3F attendu (champ {i} de {})", schema.name)));
                    };
                    out.extend_from_slice(&p.x.to_le_bytes());
                    out.extend_from_slice(&p.y.to_le_bytes());
                    out.extend_from_slice(&p.z.to_le_bytes());
                }
                flags |= bit;
            }
            Ty::Packet => {
                if size > 1 {
                    let list: Vec<&Value> = match v {
                        Value::List(l) => l.iter().collect(),
                        other => vec![other],
                    };
                    out.push(list.len() as u8);
                    for mini in list {
                        let Value::Bytes(b) = mini else {
                            return Err(Error::Invalid(format!("mini-paquet attendu (champ {i} de {})", schema.name)));
                        };
                        out.extend_from_slice(b);
                    }
                } else {
                    let Value::Bytes(b) = v else {
                        return Err(Error::Invalid(format!("mini-paquet attendu (champ {i} de {})", schema.name)));
                    };
                    out.extend_from_slice(b);
                }
                flags |= bit;
            }
            Ty::MemBlock => {
                let Value::Bytes(b) = v else {
                    return Err(Error::Invalid(format!("bloc mémoire attendu (champ {i} de {})", schema.name)));
                };
                out.extend_from_slice(&(b.len() as u32).to_le_bytes());
                if !b.is_empty() {
                    out.extend_from_slice(b);
                    flags |= bit; // un bloc vide n'active pas le drapeau
                }
            }
            Ty::Matrix | Ty::PosBaseMeter | Ty::WChar => {
                return Err(Error::Invalid(format!("type de champ {ty:?} non pris en charge (schéma {})", schema.name)))
            }
            _ => {
                match v {
                    Value::List(l) if size > 1 => {
                        for item in l {
                            write_scalar(&mut out, *ty, item, schema)?;
                        }
                    }
                    other => write_scalar(&mut out, *ty, other, schema)?,
                }
                flags |= bit;
            }
        }
    }
    Ok((flags, out))
}

fn write_flags(out: &mut Vec<u8>, flags: u32, flag_len: u8) {
    match flag_len {
        1 => out.push(flags as u8),
        2 => out.extend_from_slice(&(flags as u16).to_le_bytes()),
        _ => out.extend_from_slice(&flags.to_le_bytes()),
    }
}

/// Construit un paquet principal complet (en clair).
pub fn build_packet(schema: &Schema, fields: &Fields) -> Vec<u8> {
    try_build_packet(schema, fields).unwrap_or_else(|e| panic!("{e}"))
}

pub fn try_build_packet(schema: &Schema, fields: &Fields) -> Result<Vec<u8>, Error> {
    let (flags, body) = encode_fields(schema, fields)?;
    let mut out = Vec::with_capacity(HEADER_SIZE + schema.flag_len as usize + body.len() + 1);
    out.push(GUARD_HEAD);
    out.extend_from_slice(&0u32.to_le_bytes());
    out.push(schema.id);
    out.push(0);
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    write_flags(&mut out, flags, schema.flag_len);
    out.extend_from_slice(&body);
    out.push(GUARD_TAIL);
    let len = out.len() as u32;
    out[1..5].copy_from_slice(&len.to_le_bytes());
    Ok(out)
}

/// Construit un mini-paquet (u32 longueur + drapeaux + champs).
pub fn build_mini(schema: &Schema, fields: &Fields) -> Vec<u8> {
    let (flags, body) = encode_fields(schema, fields).unwrap_or_else(|e| panic!("{e}"));
    let mut out = Vec::with_capacity(4 + schema.flag_len as usize + body.len());
    out.extend_from_slice(&((schema.flag_len as usize + body.len()) as u32).to_le_bytes());
    write_flags(&mut out, flags, schema.flag_len);
    out.extend_from_slice(&body);
    out
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
    end: usize,
    name: &'static str,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize, what: &str) -> Result<&'a [u8], Error> {
        if self.pos + n > self.end {
            return Err(Error::Invalid(format!("lecture hors limites ({what}, schéma {})", self.name)));
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn u32(&mut self, what: &str) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.take(4, what)?.try_into().unwrap()))
    }
    fn f32(&mut self, what: &str) -> Result<f32, Error> {
        Ok(f32::from_le_bytes(self.take(4, what)?.try_into().unwrap()))
    }
    fn scalar(&mut self, ty: Ty) -> Result<Value, Error> {
        Ok(match ty {
            Ty::I8 => Value::Int(self.take(1, "INT8")?[0] as i8 as i64),
            Ty::U8 => Value::Int(self.take(1, "UINT8")?[0] as i64),
            Ty::I16 => Value::Int(i16::from_le_bytes(self.take(2, "INT16")?.try_into().unwrap()) as i64),
            Ty::U16 => Value::Int(u16::from_le_bytes(self.take(2, "UINT16")?.try_into().unwrap()) as i64),
            Ty::I32 => Value::Int(i32::from_le_bytes(self.take(4, "INT32")?.try_into().unwrap()) as i64),
            Ty::U32 => Value::Int(self.u32("UINT32")? as i64),
            Ty::I64 => Value::Int(i64::from_le_bytes(self.take(8, "INT64")?.try_into().unwrap())),
            Ty::U64 => Value::Int(u64::from_le_bytes(self.take(8, "UINT64")?.try_into().unwrap()) as i64),
            Ty::F32 => Value::Float(self.f32("FLOAT")?),
            other => return Err(Error::Invalid(format!("type scalaire inconnu {other:?}"))),
        })
    }
}

/// Décode les champs à partir de `pos` selon `schema` et `flags`. Les PACKET sont rendus en octets
/// bruts (longueur exclue), à décoder avec [`decode_mini`].
pub fn decode_fields(schema: &Schema, buf: &[u8], pos: usize, flags: u32, end: usize) -> Result<Fields, Error> {
    let mut fields = Fields::new(schema);
    let mut r = Reader { buf, pos, end: end.min(buf.len()), name: schema.name };
    for (i, Field(ty, size)) in schema.fields.iter().enumerate() {
        let bit = 1u32 << i;
        if bit > flags {
            break;
        }
        if flags & bit == 0 {
            continue;
        }
        let size = *size as usize;
        let v = match ty {
            Ty::Char => Value::Bytes(r.take(size, "CHAR")?.to_vec()),
            Ty::Vec3 => {
                let mut list = Vec::with_capacity(size);
                for _ in 0..size {
                    list.push(Value::Vec3(Vec3::new(r.f32("VEC3F")?, r.f32("VEC3F")?, r.f32("VEC3F")?)));
                }
                if size > 1 { Value::List(list) } else { list.pop().unwrap() }
            }
            Ty::Packet => {
                if size > 1 {
                    let count = r.take(1, "PACKET[]")?[0] as usize;
                    let mut list = Vec::with_capacity(count);
                    for _ in 0..count {
                        let len = r.u32("PACKET")? as usize;
                        list.push(Value::Bytes(r.take(len, "mini-paquet")?.to_vec()));
                    }
                    Value::List(list)
                } else {
                    let len = r.u32("PACKET")? as usize;
                    Value::Bytes(r.take(len, "mini-paquet")?.to_vec())
                }
            }
            Ty::MemBlock => {
                let len = r.u32("MEMORY_BLOCK")? as usize;
                Value::Bytes(r.take(len, "bloc mémoire")?.to_vec())
            }
            Ty::Matrix | Ty::PosBaseMeter | Ty::WChar => {
                return Err(Error::Invalid(format!("type de champ {ty:?} non pris en charge en lecture (schéma {})", schema.name)))
            }
            _ => {
                let mut list = Vec::with_capacity(size);
                for _ in 0..size {
                    list.push(r.scalar(*ty)?);
                }
                if size > 1 { Value::List(list) } else { list.pop().unwrap() }
            }
        };
        fields.0[i] = Some(v);
    }
    Ok(fields)
}

fn read_flags(buf: &[u8], pos: usize, flag_len: u8) -> Result<(u32, usize), Error> {
    let n = flag_len as usize;
    if pos + n > buf.len() {
        return Err(Error::Invalid("drapeaux hors limites".into()));
    }
    let b = &buf[pos..pos + n];
    let flags = match flag_len {
        1 => b[0] as u32,
        2 => u16::from_le_bytes([b[0], b[1]]) as u32,
        _ => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
    };
    Ok((flags, pos + n))
}

/// Lit l'en-tête d'un paquet principal en clair.
pub fn parse_header(buf: &[u8]) -> Option<Header> {
    if buf.len() < HEADER_SIZE + 1 || buf[0] != GUARD_HEAD {
        return None;
    }
    let u32_at = |o: usize| u32::from_le_bytes(buf[o..o + 4].try_into().unwrap());
    Some(Header { size: u32_at(1), ptype: buf[5], header_flags: buf[6], session_id: u32_at(7), timestamp: u32_at(15) })
}

/// Décode un paquet principal complet selon `schema` (doit correspondre au type).
pub fn decode_packet(schema: &Schema, buf: &[u8]) -> Result<Packet, Error> {
    let header = parse_header(buf).ok_or_else(|| Error::Invalid("en-tête de paquet invalide".into()))?;
    if header.ptype != schema.id {
        return Err(Error::Invalid(format!("type {} ≠ schéma {} ({})", header.ptype, schema.name, schema.id)));
    }
    let end = (header.size as usize).min(buf.len()).saturating_sub(1); // exclut le pied 0x6B
    let (flags, start) = read_flags(buf, HEADER_SIZE, schema.flag_len)?;
    let fields = decode_fields(schema, buf, start, flags, end)?;
    Ok(Packet { header, flags, fields })
}

/// Décode un mini-paquet (octets sans leur u32 de longueur).
pub fn decode_mini(schema: &Schema, payload: &[u8]) -> Result<Fields, Error> {
    let (flags, start) = read_flags(payload, 0, schema.flag_len)?;
    decode_fields(schema, payload, start, flags, payload.len())
}

/// Octets d'un mini-paquet construit, sans son préfixe de longueur (pour le relire comme le décodeur).
pub fn mini_payload(mini: &[u8]) -> &[u8] {
    &mini[4.min(mini.len())..]
}

pub fn hex_dump(buf: &[u8], max: usize) -> String {
    let mut s: String = buf.iter().take(max).map(|b| format!("{b:02x} ")).collect();
    s.pop();
    if buf.len() > max {
        s.push_str(&format!(" … (+{})", buf.len() - max));
    }
    s
}
