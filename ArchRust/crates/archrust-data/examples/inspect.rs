//! Vérifie les lecteurs sur un vrai client : `cargo run -p archrust-data --example inspect -- <dossier Game>`
use archrust_crypto::{capi_crypt, KEY_TEXTURE};
use archrust_data::ini::IniTable;
use archrust_data::magpack::MagPack;
use archrust_data::pack::PackIndex;
use archrust_data::rw::{Clump, TextureDictionary};
use std::fs;
use std::path::PathBuf;

/// Affiche la suite des blocs RenderWare d'un tampon (profondeur limitée).
fn walk_chunks(data: &[u8], depth: usize, max_depth: usize, max_items: usize) {
    use archrust_data::rw::Reader;
    let mut r = Reader::new(data);
    let mut n = 0;
    while r.remaining() >= 12 && n < max_items {
        let Ok(c) = r.chunk() else { break };
        let head: Vec<String> = data[c.start..c.end().min(c.start + 16)].iter().map(|b| format!("{b:02x}")).collect();
        println!("{}type 0x{:x} taille {} ver 0x{:x} [{}]", "    ".repeat(depth + 1), c.id, c.size, c.version, head.join(" "));
        if depth + 1 < max_depth && matches!(c.id, 0x10 | 0x0e | 0x1a | 0x0f | 0x14 | 0x0b | 0x09 | 0x0a | 0x1c | 0x03) {
            walk_chunks(&data[c.start..c.end()], depth + 1, max_depth, 6);
        }
        r.skip(&c);
        n += 1;
    }
}

fn main() -> anyhow::Result<()> {
    let game = PathBuf::from(std::env::args().nth(1).expect("dossier Game du client"));

    // 1. Index + pack des personnages
    let idx = PackIndex::parse_encrypted(&fs::read(game.join("character/Reference.Dat"))?)?;
    println!("character : racine {:?}, {} entrées, {} Mo de données", idx.root, idx.entries.len(), idx.data_size() / 1_000_000);
    let mut data = fs::File::open(game.join("character/Data.Dat"))?;
    let mut ok = 0;
    let mut skinned = 0;
    let mut failed = Vec::new();
    for e in idx.entries.iter().take(200) {
        let bytes = idx.read_entry(&mut data, e)?;
        match Clump::parse(&bytes) {
            Ok(c) => {
                ok += 1;
                if c.geometries.iter().any(|g| g.skin.is_some()) {
                    skinned += 1;
                }
            }
            Err(err) => failed.push(format!("{} : {err}", e.name)),
        }
    }
    println!("  200 premiers clumps : {ok} lus ({skinned} avec peau), {} échecs", failed.len());
    for f in failed.iter().take(5) {
        println!("    {f}");
    }
    let mut c = Clump::default();
    let mut which = &idx.entries[0];
    for e in &idx.entries {
        let bytes = idx.read_entry(&mut data, e)?;
        if let Ok(parsed) = Clump::parse(&bytes) {
            if !parsed.geometries.is_empty() {
                c = parsed;
                which = e;
                break;
            }
        }
    }
    println!(
        "  {} : {} frames, {} géométries, {} atomics ; géo 0 = {} sommets, {} triangles, {} matériaux, {} UV",
        which.name,
        c.frames.len(),
        c.geometries.len(),
        c.atomics.len(),
        c.geometries[0].vertices.len(),
        c.geometries[0].triangles.len(),
        c.geometries[0].materials.len(),
        c.geometries[0].uv_sets.len()
    );
    for f in c.frames.iter().take(4) {
        println!("    frame {:?} parent {} hanim {:?}", f.name, f.parent, f.hanim.as_ref().map(|h| h.node_id));
    }
    for m in c.geometries[0].materials.iter().take(3) {
        println!("    matériau texture {:?}", m.texture.as_ref().map(|t| &t.name));
    }

    // 2. Texture tx1
    let tx_dir = game.join("texture/notpacked");
    let mut n_tex = 0;
    let mut n_fail = 0;
    let mut formats = std::collections::BTreeMap::new();
    for entry in fs::read_dir(&tx_dir)? {
        let p = entry?.path();
        if p.extension().map(|e| e == "tx1").unwrap_or(false) {
            let plain = capi_crypt(KEY_TEXTURE, &fs::read(&p)?);
            match TextureDictionary::parse(&plain) {
                Ok(d) => {
                    n_tex += 1;
                    for t in &d.textures {
                        *formats.entry(format!("{:?}", t.format)).or_insert(0) += 1;
                        if n_tex == 1 {
                            let rgba = t.to_rgba8()?;
                            println!("tx1 {} : {}x{} {:?} alpha={} mips={} → {} octets RGBA", t.name, t.width, t.height, t.format, t.has_alpha, t.mips.len(), rgba.len());
                        }
                    }
                }
                Err(e) => {
                    n_fail += 1;
                    if n_fail <= 3 {
                        println!("  tx1 {} : {e}", p.display());
                    }
                }
            }
        }
    }
    println!("tx1 : {n_tex} dictionnaires lus, {n_fail} échecs, formats {formats:?}");

    // 3. MagPack
    let ma2 = MagPack::parse(&fs::read(game.join("world/a001717x.ma2"))?)?;
    println!("a001717x.ma2 : {} entrées, première {:?} ({} o)", ma2.entries.len(), ma2.entries[0].name, ma2.entries[0].size);
    let mut dff_ok = 0;
    let mut unpack_fail = Vec::new();
    let mut shown = false;
    for e in &ma2.entries {
        if !e.name.ends_with(".dff") {
            continue;
        }
        match ma2.unpack(e) {
            Ok(plain) => match Clump::parse(&plain) {
                Ok(c) => {
                    dff_ok += 1;
                    if !shown {
                        shown = true;
                        let g = &c.geometries[0];
                        let (mut lo, mut hi) = (glam::Vec3::splat(f32::MAX), glam::Vec3::splat(f32::MIN));
                        for v in &g.vertices {
                            lo = lo.min(*v);
                            hi = hi.max(*v);
                        }
                        println!(
                            "  {} décompressé {} o → {} frames, {} géo, géo 0 : {} sommets, {} tri, {} mat ({:?}), bornes {:?}..{:?}",
                            e.name, plain.len(), c.frames.len(), c.geometries.len(), g.vertices.len(), g.triangles.len(), g.materials.len(),
                            g.materials.iter().filter_map(|m| m.texture.as_ref().map(|t| t.name.clone())).take(4).collect::<Vec<_>>(), lo, hi
                        );
                    }
                }
                Err(err) => unpack_fail.push(format!("{} : clump {err}", e.name)),
            },
            Err(err) => unpack_fail.push(format!("{} : {err}", e.name)),
        }
    }
    println!("  secteurs .dff décompressés et lus : {dff_ok}/{} ; échecs : {:?}", ma2.entries.iter().filter(|e| e.name.ends_with(".dff")).count(), unpack_fail.iter().take(3).collect::<Vec<_>>());
    // Les entrées R sont des dictionnaires de textures : on les décrit.
    if let Some(Ok(plain)) = ma2.unpack_named("R272,272.dff") {
        match TextureDictionary::parse(&plain) {
            Ok(d) => println!("  R272,272 = TXD de {} texture(s) : {}", d.textures.len(), d.textures.iter().map(|t| format!("{} {}x{} {:?}", t.name, t.width, t.height, t.format)).collect::<Vec<_>>().join(" | ")),
            Err(e) => println!("  R272,272 : TXD illisible ({e})"),
        }
    }
    let b2 = MagPack::parse(&fs::read(game.join("world/b001717x.ma2"))?)?;
    match b2.unpack(&b2.entries[0]) {
        Ok(plain) => {
            println!("b001717x.ma2 {} : {} o décompressés ; u32 de tête {}", b2.entries[0].name, plain.len(), u32::from_le_bytes(plain[..4].try_into().unwrap()));
            walk_chunks(&plain[4..], 0, 2, 3);
            match archrust_data::rw::SectorGeometry::parse(&plain) {
                Ok(s) => {
                    let g = &s.geometry;
                    let (lo, hi) = s.bounds();
                    println!("  secteur : struct {:?}, {} sommets, {} tri, {} UV, {} mat, normales {}, couleurs {}, bornes {lo:?}..{hi:?}, meshes {}", s.atomic_struct, g.vertices.len(), g.triangles.len(), g.uv_sets.len(), g.materials.len(), g.normals.len(), g.colors.len(), g.meshes.len());
                    println!("  matériaux : {:?}", g.materials.iter().map(|m| (m.color, m.texture.as_ref().map(|t| t.name.clone()))).collect::<Vec<_>>());
                    if let Some(uv) = g.uv_sets.first() {
                        println!("  UV0 premiers : {:?}", &uv[..4.min(uv.len())]);
                    }
                }
                Err(e) => println!("  secteur : {e}"),
            }
            // Vérifie la cohérence des coordonnées sur une autre tuile (Anchorville, 325,389).
            if let Ok(bytes) = fs::read(game.join("world/b002024x.ma2")) {
                let t = MagPack::parse(&bytes)?;
                match t.unpack_named("D325,389.dff") {
                    Some(Ok(p)) => match archrust_data::rw::SectorGeometry::parse(&p).map_err(|e| {
                        println!("  D325,389 : {} o, tête {:02x?}", p.len(), &p[..p.len().min(48)]);
                        walk_chunks(&p[4..], 0, 2, 4);
                        e
                    }) {
                        Ok(s) => {
                            let (lo, hi) = s.bounds();
                            println!("  D325,389 (Anchorville, joueur à -475955, -66219) : bornes {lo:?}..{hi:?}");
                        }
                        Err(e) => println!("  D325,389 : {e}"),
                    },
                    Some(Err(e)) => println!("  D325,389 : {e}"),
                    None => println!("  D325,389 absent ; entrées : {:?}", t.entries.iter().take(3).map(|e| &e.name).collect::<Vec<_>>()),
                }
            }
        }
        Err(e) => println!("b001717x.ma2 : {e}"),
    }
    let ma1 = MagPack::parse(&fs::read(game.join("world/a001717x.ma1"))?)?;
    println!("a001717x.ma1 : {} entrées, {:?} {} o", ma1.entries.len(), ma1.entries[0].name, ma1.entries[0].size);
    match ma1.unpack(&ma1.entries[0]) {
        Ok(plain) => println!("  {} : {} o décompressés, début {:02x?}", ma1.entries[0].name, plain.len(), &plain[..plain.len().min(24)]),
        Err(e) => println!("  {} : {e}", ma1.entries[0].name),
    }

    // 4. Table ini
    let t = IniTable::from_encrypted(&fs::read(game.join("ini/charactertemplate.ini"))?);
    let s = &t.sections[0];
    println!("charactertemplate.ini : {} sections ; [{}] Name={:?} Race={:?}", t.len(), s.name, s.get("Name"), s.get("Race"));

    // 5. Autres packs
    for d in ["object", "etc", "images", "world/octree", "world/grass", "world/water", "character/animation", "effect/clump"] {
        let ref_path = ["Reference.Dat", "Reference.dat"].iter().map(|n| game.join(d).join(n)).find(|p| p.exists());
        if let Some(p) = ref_path {
            match PackIndex::parse_encrypted(&fs::read(&p)?) {
                Ok(i) => println!("{d:<20} racine {:?} {} entrées, ex. {:?}", i.root, i.entries.len(), i.entries.first().map(|e| &e.name)),
                Err(e) => println!("{d:<20} erreur {e}"),
            }
        }
    }
    Ok(())
}
