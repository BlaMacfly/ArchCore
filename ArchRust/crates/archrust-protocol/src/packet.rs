//! Codec des paquets ArchLord (format « AuPacket »).
//!
//! Paquet principal (en clair, avant chiffrement) — relevé dans l'exécutable x64 :
//!
//! | Octets | Contenu |
//! |--------|---------|
//! | 0      | garde `0xD6` |
//! | 1–4    | taille totale (u32) |
//! | 5      | type de paquet (u8) |
//! | 6      | drapeaux d'en-tête (u8) |
//! | 7–10   | identifiant propriétaire (u32) |
//! | 11–18  | horodatage (u64) |
//! | 19…    | drapeaux de champs (1, 2 ou 4 octets selon le module) puis les champs présents |
//! | fin    | pied `0x6B` |
//!
//! Mini-paquet (imbriqué dans un champ `PACKET`) : `u32` longueur, drapeaux, champs.
//! Bloc mémoire : `u32` longueur + octets. (Le client 32 bits d'origine utilisait des `u16`.)
//! Tous les entiers et flottants sont petit-boutistes.

use crate::schema::{FieldType, Schema};
use bytes::{Buf, BufMut, BytesMut};
use std::fmt;

pub const GUARD_HEAD: u8 = 0xd6;
pub const GUARD_TAIL: u8 = 0x6b;
pub const HEADER_SIZE: usize = 19;

#[derive(Debug, thiserror::Error)]
pub enum PacketError {
    #[error("en-tête de paquet invalide")]
    BadHeader,
    #[error("type {got} différent du schéma {schema} ({expected})")]
    TypeMismatch {
        got: u8,
        expected: u8,
        schema: &'static str,
    },
    #[error("lecture hors limites (champ {field} de {schema})")]
    OutOfBounds { field: usize, schema: &'static str },
    #[error("type de champ {0:?} non pris en charge")]
    Unsupported(FieldType),
    #[error("valeur incompatible avec le champ {field} de {schema} (attendu {expected:?})")]
    BadValue {
        field: usize,
        schema: &'static str,
        expected: FieldType,
    },
    #[error("longueur de drapeaux invalide {0}")]
    BadFlagLen(u8),
}

/// Valeur d'un champ. `None` dans un tableau de valeurs signifie « champ absent ».
#[derive(Clone, PartialEq)]
pub enum Value {
    I8(i8),
    U8(u8),
    I16(i16),
    U16(u16),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    F32(f32),
    /// Liste de scalaires (champs répétés `size > 1`).
    Ints(Vec<i64>),
    Vec3([f32; 3]),
    Vec3List(Vec<[f32; 3]>),
    /// Chaîne à taille fixe (`CHAR[n]`), octets bruts sans remplissage.
    Str(Vec<u8>),
    /// Mini-paquet brut, **sans** son `u32` de longueur.
    Packet(Vec<u8>),
    /// Liste de mini-paquets (champ `PACKET` avec `size > 1`, précédée d'un compteur u8).
    PacketList(Vec<Vec<u8>>),
    /// Bloc mémoire (`u32` longueur + octets).
    Block(Vec<u8>),
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::I8(v) => write!(f, "{v}i8"),
            Value::U8(v) => write!(f, "{v}u8"),
            Value::I16(v) => write!(f, "{v}i16"),
            Value::U16(v) => write!(f, "{v}u16"),
            Value::I32(v) => write!(f, "{v}"),
            Value::U32(v) => write!(f, "{v}u32"),
            Value::I64(v) => write!(f, "{v}i64"),
            Value::U64(v) => write!(f, "{v}u64"),
            Value::F32(v) => write!(f, "{v}f"),
            Value::Ints(v) => write!(f, "{v:?}"),
            Value::Vec3(v) => write!(f, "({}, {}, {})", v[0], v[1], v[2]),
            Value::Vec3List(v) => write!(f, "{v:?}"),
            Value::Str(v) => write!(f, "{:?}", String::from_utf8_lossy(v)),
            Value::Packet(v) => write!(f, "<mini {} o>", v.len()),
            Value::PacketList(v) => write!(f, "<{} minis>", v.len()),
            Value::Block(v) => write!(f, "<bloc {} o>", v.len()),
        }
    }
}

impl Value {
    /// Chaîne à taille fixe depuis un texte (latin-1/CP949 non converti).
    pub fn str<S: AsRef<[u8]>>(s: S) -> Value {
        Value::Str(s.as_ref().to_vec())
    }
    pub fn as_i64(&self) -> Option<i64> {
        Some(match self {
            Value::I8(v) => *v as i64,
            Value::U8(v) => *v as i64,
            Value::I16(v) => *v as i64,
            Value::U16(v) => *v as i64,
            Value::I32(v) => *v as i64,
            Value::U32(v) => *v as i64,
            Value::I64(v) => *v,
            Value::U64(v) => *v as i64,
            _ => return None,
        })
    }
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Str(v) | Value::Packet(v) | Value::Block(v) => Some(v),
            _ => None,
        }
    }
    /// Texte d'une chaîne fixe, tronqué au premier octet nul.
    pub fn as_text(&self) -> Option<String> {
        let b = self.as_bytes()?;
        let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
        Some(b[..end].iter().map(|&c| c as char).collect())
    }
    pub fn as_vec3(&self) -> Option<[f32; 3]> {
        match self {
            Value::Vec3(v) => Some(*v),
            _ => None,
        }
    }
}

/// En-tête décodé d'un paquet principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub size: u32,
    pub packet_type: u8,
    pub header_flags: u8,
    pub owner: u32,
    pub timestamp: u64,
}

/// Paquet principal décodé.
#[derive(Debug, Clone)]
pub struct Packet {
    pub header: Header,
    pub flags: u32,
    pub values: Vec<Option<Value>>,
}

impl Packet {
    pub fn op(&self) -> Option<i64> {
        self.values
            .first()
            .and_then(|v| v.as_ref())
            .and_then(Value::as_i64)
    }
    pub fn get(&self, i: usize) -> Option<&Value> {
        self.values.get(i).and_then(|v| v.as_ref())
    }
}

fn write_scalar(w: &mut BytesMut, ty: FieldType, v: i64) -> Result<(), PacketError> {
    match ty {
        FieldType::Int8 => w.put_i8(v as i8),
        FieldType::UInt8 => w.put_u8(v as u8),
        FieldType::Int16 => w.put_i16_le(v as i16),
        FieldType::UInt16 => w.put_u16_le(v as u16),
        FieldType::Int32 => w.put_i32_le(v as i32),
        FieldType::UInt32 => w.put_u32_le(v as u32),
        FieldType::Int64 => w.put_i64_le(v),
        FieldType::UInt64 => w.put_u64_le(v as u64),
        other => return Err(PacketError::Unsupported(other)),
    }
    Ok(())
}

fn read_scalar(buf: &mut &[u8], ty: FieldType) -> Option<Value> {
    Some(match ty {
        FieldType::Int8 => Value::I8(get(buf, 1)?.get_i8()),
        FieldType::UInt8 => Value::U8(get(buf, 1)?.get_u8()),
        FieldType::Int16 => Value::I16(get(buf, 2)?.get_i16_le()),
        FieldType::UInt16 => Value::U16(get(buf, 2)?.get_u16_le()),
        FieldType::Int32 => Value::I32(get(buf, 4)?.get_i32_le()),
        FieldType::UInt32 => Value::U32(get(buf, 4)?.get_u32_le()),
        FieldType::Int64 => Value::I64(get(buf, 8)?.get_i64_le()),
        FieldType::UInt64 => Value::U64(get(buf, 8)?.get_u64_le()),
        FieldType::Float => Value::F32(get(buf, 4)?.get_f32_le()),
        _ => return None,
    })
}

fn get<'a>(buf: &mut &'a [u8], n: usize) -> Option<&'a [u8]> {
    if buf.len() < n {
        return None;
    }
    let (head, tail) = buf.split_at(n);
    *buf = tail;
    Some(head)
}

/// Encode les champs d'un schéma : rend `(drapeaux, corps)`.
pub fn encode_fields(
    schema: &Schema,
    values: &[Option<Value>],
) -> Result<(u32, BytesMut), PacketError> {
    let mut w = BytesMut::with_capacity(128);
    let mut flags: u32 = 0;
    for (i, field) in schema.fields.iter().enumerate() {
        let Some(v) = values.get(i).and_then(|v| v.as_ref()) else {
            continue;
        };
        let bit = 1u32 << i;
        let bad = || PacketError::BadValue {
            field: i,
            schema: schema.name,
            expected: field.ty,
        };
        match field.ty {
            FieldType::Char => {
                let Value::Str(s) = v else { return Err(bad()) };
                let n = field.size as usize;
                let take = s.len().min(n);
                w.put_slice(&s[..take]);
                w.put_bytes(0, n - take);
                flags |= bit;
            }
            FieldType::Vec3F => {
                let list: Vec<[f32; 3]> = match v {
                    Value::Vec3(a) => vec![*a],
                    Value::Vec3List(l) => l.clone(),
                    _ => return Err(bad()),
                };
                for p in list {
                    w.put_f32_le(p[0]);
                    w.put_f32_le(p[1]);
                    w.put_f32_le(p[2]);
                }
                flags |= bit;
            }
            FieldType::Packet => {
                if field.size > 1 {
                    let list: Vec<&[u8]> = match v {
                        Value::PacketList(l) => l.iter().map(|m| m.as_slice()).collect(),
                        Value::Packet(m) => vec![m.as_slice()],
                        _ => return Err(bad()),
                    };
                    w.put_u8(list.len() as u8);
                    for m in list {
                        w.put_u32_le(m.len() as u32);
                        w.put_slice(m);
                    }
                } else {
                    let Value::Packet(m) = v else {
                        return Err(bad());
                    };
                    w.put_u32_le(m.len() as u32);
                    w.put_slice(m);
                }
                flags |= bit;
            }
            FieldType::MemoryBlock => {
                let Value::Block(b) = v else {
                    return Err(bad());
                };
                w.put_u32_le(b.len() as u32);
                if !b.is_empty() {
                    w.put_slice(b);
                    // Comme le client : un bloc vide n'active pas le drapeau.
                    flags |= bit;
                }
            }
            FieldType::Float => {
                let Value::F32(f) = v else { return Err(bad()) };
                w.put_f32_le(*f);
                flags |= bit;
            }
            FieldType::Matrix | FieldType::PosBaseMeter | FieldType::WChar => {
                return Err(PacketError::Unsupported(field.ty));
            }
            scalar => {
                if field.size > 1 {
                    let Value::Ints(list) = v else {
                        return Err(bad());
                    };
                    for x in list {
                        write_scalar(&mut w, scalar, *x)?;
                    }
                } else {
                    let x = v.as_i64().ok_or_else(bad)?;
                    write_scalar(&mut w, scalar, x)?;
                }
                flags |= bit;
            }
        }
    }
    Ok((flags, w))
}

fn write_flags(w: &mut BytesMut, flags: u32, flag_len: u8) -> Result<(), PacketError> {
    match flag_len {
        1 => w.put_u8(flags as u8),
        2 => w.put_u16_le(flags as u16),
        4 => w.put_u32_le(flags),
        n => return Err(PacketError::BadFlagLen(n)),
    }
    Ok(())
}

fn read_flags(buf: &mut &[u8], flag_len: u8) -> Result<u32, PacketError> {
    Ok(match flag_len {
        1 => get(buf, 1).ok_or(PacketError::BadHeader)?.get_u8() as u32,
        2 => get(buf, 2).ok_or(PacketError::BadHeader)?.get_u16_le() as u32,
        4 => get(buf, 4).ok_or(PacketError::BadHeader)?.get_u32_le(),
        n => return Err(PacketError::BadFlagLen(n)),
    })
}

/// Options d'en-tête d'un paquet principal.
#[derive(Debug, Clone, Copy, Default)]
pub struct HeaderOpts {
    pub owner: u32,
    pub timestamp: u64,
}

/// Construit un paquet principal complet (en clair).
pub fn build_packet(schema: &Schema, values: &[Option<Value>]) -> Result<Vec<u8>, PacketError> {
    build_packet_with(schema, values, HeaderOpts::default())
}

pub fn build_packet_with(
    schema: &Schema,
    values: &[Option<Value>],
    opts: HeaderOpts,
) -> Result<Vec<u8>, PacketError> {
    let (flags, body) = encode_fields(schema, values)?;
    let mut w = BytesMut::with_capacity(HEADER_SIZE + 4 + body.len() + 1);
    w.put_u8(GUARD_HEAD);
    w.put_u32_le(0); // taille, corrigée ci-dessous
    w.put_u8(schema.id);
    w.put_u8(0);
    w.put_u32_le(opts.owner);
    w.put_u64_le(opts.timestamp);
    write_flags(&mut w, flags, schema.flag_len)?;
    w.put_slice(&body);
    w.put_u8(GUARD_TAIL);
    let len = w.len() as u32;
    w[1..5].copy_from_slice(&len.to_le_bytes());
    Ok(w.to_vec())
}

/// Construit la charge utile d'un mini-paquet : drapeaux + champs, **sans** le `u32` de longueur
/// (il est ajouté par l'encodeur du champ `Packet` qui l'embarque, ou par [`frame_mini`]).
pub fn build_mini(schema: &Schema, values: &[Option<Value>]) -> Result<Vec<u8>, PacketError> {
    let (flags, body) = encode_fields(schema, values)?;
    let mut w = BytesMut::with_capacity(4 + body.len());
    write_flags(&mut w, flags, schema.flag_len)?;
    w.put_slice(&body);
    Ok(w.to_vec())
}

/// Préfixe une charge utile de mini-paquet de sa longueur (`u32`), forme transmise sur le fil.
pub fn frame_mini(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

/// Décode les champs d'un schéma à partir de `buf` selon `flags`.
pub fn decode_fields(
    schema: &Schema,
    mut buf: &[u8],
    flags: u32,
) -> Result<Vec<Option<Value>>, PacketError> {
    let mut values: Vec<Option<Value>> = vec![None; schema.fields.len()];
    for (i, field) in schema.fields.iter().enumerate() {
        let bit = 1u32 << i;
        if bit > flags {
            break;
        }
        if flags & bit == 0 {
            continue;
        }
        let oob = || PacketError::OutOfBounds {
            field: i,
            schema: schema.name,
        };
        let v = match field.ty {
            FieldType::Char => {
                Value::Str(get(&mut buf, field.size as usize).ok_or_else(oob)?.to_vec())
            }
            FieldType::Vec3F => {
                let mut list = Vec::with_capacity(field.size as usize);
                for _ in 0..field.size {
                    let mut s = get(&mut buf, 12).ok_or_else(oob)?;
                    list.push([s.get_f32_le(), s.get_f32_le(), s.get_f32_le()]);
                }
                if field.size > 1 {
                    Value::Vec3List(list)
                } else {
                    Value::Vec3(list[0])
                }
            }
            FieldType::Packet => {
                if field.size > 1 {
                    let count = get(&mut buf, 1).ok_or_else(oob)?.get_u8() as usize;
                    let mut list = Vec::with_capacity(count);
                    for _ in 0..count {
                        let len = get(&mut buf, 4).ok_or_else(oob)?.get_u32_le() as usize;
                        list.push(get(&mut buf, len).ok_or_else(oob)?.to_vec());
                    }
                    Value::PacketList(list)
                } else {
                    let len = get(&mut buf, 4).ok_or_else(oob)?.get_u32_le() as usize;
                    Value::Packet(get(&mut buf, len).ok_or_else(oob)?.to_vec())
                }
            }
            FieldType::MemoryBlock => {
                let len = get(&mut buf, 4).ok_or_else(oob)?.get_u32_le() as usize;
                Value::Block(get(&mut buf, len).ok_or_else(oob)?.to_vec())
            }
            FieldType::Matrix | FieldType::PosBaseMeter | FieldType::WChar => {
                return Err(PacketError::Unsupported(field.ty));
            }
            scalar => {
                if field.size > 1 {
                    let mut list = Vec::with_capacity(field.size as usize);
                    for _ in 0..field.size {
                        let v = read_scalar(&mut buf, scalar).ok_or_else(oob)?;
                        list.push(v.as_i64().unwrap_or_default());
                    }
                    Value::Ints(list)
                } else {
                    read_scalar(&mut buf, scalar).ok_or_else(oob)?
                }
            }
        };
        values[i] = Some(v);
    }
    Ok(values)
}

/// Lit l'en-tête d'un paquet principal en clair.
pub fn parse_header(buf: &[u8]) -> Option<Header> {
    if buf.len() < HEADER_SIZE + 1 || buf[0] != GUARD_HEAD {
        return None;
    }
    let mut b = &buf[1..];
    Some(Header {
        size: b.get_u32_le(),
        packet_type: b.get_u8(),
        header_flags: b.get_u8(),
        owner: b.get_u32_le(),
        timestamp: b.get_u64_le(),
    })
}

/// Décode un paquet principal complet selon `schema` (doit correspondre au type).
pub fn decode_packet(schema: &Schema, buf: &[u8]) -> Result<Packet, PacketError> {
    let header = parse_header(buf).ok_or(PacketError::BadHeader)?;
    if header.packet_type != schema.id {
        return Err(PacketError::TypeMismatch {
            got: header.packet_type,
            expected: schema.id,
            schema: schema.name,
        });
    }
    let end = (header.size as usize).min(buf.len()).saturating_sub(1); // exclut le pied
    if end < HEADER_SIZE {
        return Err(PacketError::BadHeader);
    }
    let mut body = &buf[HEADER_SIZE..end];
    let flags = read_flags(&mut body, schema.flag_len)?;
    let values = decode_fields(schema, body, flags)?;
    Ok(Packet {
        header,
        flags,
        values,
    })
}

/// Décode un mini-paquet (tampon **sans** son `u32` de longueur).
pub fn decode_mini(
    schema: &Schema,
    payload: &[u8],
) -> Result<(u32, Vec<Option<Value>>), PacketError> {
    let mut b = payload;
    let flags = read_flags(&mut b, schema.flag_len)?;
    Ok((flags, decode_fields(schema, b, flags)?))
}

/// Aide : tableau de valeurs à partir de paires `(indice, valeur)`.
pub fn sparse(schema: &Schema, pairs: &[(usize, Value)]) -> Vec<Option<Value>> {
    let mut values: Vec<Option<Value>> = vec![None; schema.fields.len()];
    for (i, v) in pairs {
        values[*i] = Some(v.clone());
    }
    values
}

/// Affichage hexadécimal compact pour les journaux.
pub fn hex_dump(buf: &[u8], max: usize) -> String {
    let shown: Vec<String> = buf.iter().take(max).map(|b| format!("{b:02x}")).collect();
    let mut s = shown.join(" ");
    if buf.len() > max {
        s.push_str(&format!(" … (+{})", buf.len() - max));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::main::{LOGIN, STARTUP_ENCRYPTION};
    use crate::schema::mini::CHAR_MOVE;

    #[test]
    fn roundtrip_login() {
        let values = sparse(
            LOGIN,
            &[
                (0, Value::I8(1)),
                (2, Value::str("test")),
                (3, Value::I8(4)),
                (6, Value::I32(0x1234)),
            ],
        );
        let bytes = build_packet(LOGIN, &values).unwrap();
        assert_eq!(bytes[0], GUARD_HEAD);
        assert_eq!(*bytes.last().unwrap(), GUARD_TAIL);
        assert_eq!(
            u32::from_le_bytes(bytes[1..5].try_into().unwrap()) as usize,
            bytes.len()
        );
        let pkt = decode_packet(LOGIN, &bytes).unwrap();
        assert_eq!(pkt.op(), Some(1));
        assert_eq!(pkt.get(2).unwrap().as_text().unwrap(), "test");
        assert_eq!(pkt.get(6).unwrap().as_i64(), Some(0x1234));
        assert!(pkt.get(1).is_none());
    }

    #[test]
    fn empty_block_does_not_set_flag() {
        let values = sparse(
            STARTUP_ENCRYPTION,
            &[(0, Value::I8(6)), (1, Value::Block(vec![]))],
        );
        let bytes = build_packet(STARTUP_ENCRYPTION, &values).unwrap();
        let pkt = decode_packet(STARTUP_ENCRYPTION, &bytes).unwrap();
        assert_eq!(pkt.flags, 1);
    }

    #[test]
    fn mini_roundtrip() {
        let values = sparse(
            CHAR_MOVE,
            &[(0, Value::Vec3([1.0, 2.0, 3.0])), (6, Value::I8(2))],
        );
        let mini = build_mini(CHAR_MOVE, &values).unwrap();
        assert_eq!(mini.len(), 1 + 12 + 1);
        let (flags, vals) = decode_mini(CHAR_MOVE, &mini).unwrap();
        assert_eq!(flags, 0b100_0001);
        assert_eq!(vals[0].as_ref().unwrap().as_vec3(), Some([1.0, 2.0, 3.0]));
        let framed = frame_mini(&mini);
        assert_eq!(
            u32::from_le_bytes(framed[..4].try_into().unwrap()) as usize,
            mini.len()
        );
    }

    #[test]
    fn packet_field_carries_single_length_prefix() {
        // Un champ PACKET embarque la charge utile précédée d'une seule longueur u32.
        let mini = build_mini(CHAR_MOVE, &sparse(CHAR_MOVE, &[(6, Value::I8(1))])).unwrap();
        let values = sparse(
            LOGIN,
            &[(0, Value::I8(8)), (8, Value::Packet(mini.clone()))],
        );
        let bytes = build_packet(LOGIN, &values).unwrap();
        // en-tête 19 + drapeaux 2 + op 1 + u32 longueur + mini + pied 1
        assert_eq!(bytes.len(), 19 + 2 + 1 + 4 + mini.len() + 1);
        let pkt = decode_packet(LOGIN, &bytes).unwrap();
        assert_eq!(pkt.get(8).unwrap().as_bytes().unwrap(), &mini[..]);
    }
}
