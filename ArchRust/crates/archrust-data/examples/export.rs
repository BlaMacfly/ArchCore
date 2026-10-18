//! Outil d'extraction de références depuis un client (usage privé, ne pas redistribuer).
//!
//! ```text
//! cargo run -p archrust-data --example export -- <Game> table <fichier.ini> <clé1,clé2,…>
//! cargo run -p archrust-data --example export -- <Game> model <pack> <nom.ecl> <sortie.obj>
//! cargo run -p archrust-data --example export -- <Game> texture <nom> <sortie.png>
//! cargo run -p archrust-data --example export -- <Game> file <pack> <nom> <sortie>
//! cargo run -p archrust-data --example export -- <Game> list <pack> [filtre]
//! ```
//! Indépendant du moteur : lit les packs, déchiffre les textures et écrit OBJ/MTL/PNG.
use archrust_crypto::{capi_crypt, KEY_TEXTURE};
use archrust_data::ini::IniTable;
use archrust_data::pack::PackIndex;
use archrust_data::rw::{Clump, TextureDictionary};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

struct Packs {
    game: PathBuf,
    cache: HashMap<String, (PackIndex, File)>,
}

impl Packs {
    fn open(&mut self, rel: &str) -> anyhow::Result<&mut (PackIndex, File)> {
        if !self.cache.contains_key(rel) {
            let dir = self.game.join(rel);
            let find = |n: &str| {
                fs::read_dir(&dir)
                    .ok()?
                    .flatten()
                    .map(|e| e.path())
                    .find(|p| {
                        p.file_name()
                            .map(|f| f.to_string_lossy().eq_ignore_ascii_case(n))
                            .unwrap_or(false)
                    })
            };
            let idx = PackIndex::parse_encrypted(&fs::read(
                find("Reference.Dat")
                    .ok_or_else(|| anyhow::anyhow!("pack {rel} : index absent"))?,
            )?)?;
            let data = File::open(
                find("Data.Dat").ok_or_else(|| anyhow::anyhow!("pack {rel} : données absentes"))?,
            )?;
            self.cache.insert(rel.to_string(), (idx, data));
        }
        Ok(self.cache.get_mut(rel).unwrap())
    }
    fn read(&mut self, rel: &str, name: &str) -> anyhow::Result<Vec<u8>> {
        let (idx, data) = self.open(rel)?;
        let e = idx
            .find(name)
            .ok_or_else(|| anyhow::anyhow!("{name} absent de {rel}"))?
            .clone();
        Ok(idx.read_entry(data, &e)?)
    }
    fn texture_rgba(&mut self, name: &str) -> anyhow::Result<(u32, u32, Vec<u8>)> {
        let file = format!("{}.tx1", name.to_ascii_lowercase());
        let notpacked = self.game.join("texture/notpacked").join(&file);
        let raw = if notpacked.exists() {
            fs::read(&notpacked)?
        } else {
            let mut found = None;
            for g in [
                "character",
                "object",
                "world",
                "item",
                "effect",
                "skill",
                "ui",
                "etc",
            ] {
                if let Ok(b) = self.read(&format!("texture/{g}"), &file) {
                    found = Some(b);
                    break;
                }
            }
            found.ok_or_else(|| anyhow::anyhow!("texture {name} introuvable"))?
        };
        let dict = TextureDictionary::parse(&capi_crypt(KEY_TEXTURE, &raw))?;
        let t = dict
            .textures
            .first()
            .ok_or_else(|| anyhow::anyhow!("dictionnaire vide"))?;
        Ok((t.mips[0].width, t.mips[0].height, t.to_rgba8()?))
    }
}

fn save_png(path: &Path, w: u32, h: u32, rgba: Vec<u8>) -> anyhow::Result<()> {
    let img =
        image::RgbaImage::from_raw(w, h, rgba).ok_or_else(|| anyhow::anyhow!("image invalide"))?;
    img.save(path)?;
    Ok(())
}

fn export_obj(packs: &mut Packs, pack: &str, name: &str, out: &Path) -> anyhow::Result<()> {
    let clump = Clump::parse(&packs.read(pack, name)?)?;
    let world = clump.world_matrices();
    let stem = out
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "model".into());
    let folder = out
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    fs::create_dir_all(&folder)?;
    let mut obj = String::new();
    let mut mtl = String::new();
    writeln!(
        obj,
        "# Export ArchRust de {pack}/{name} — référence privée, ne pas redistribuer"
    )?;
    writeln!(obj, "mtllib {stem}.mtl")?;
    let mut base = 1usize;
    let mut textures: BTreeSet<String> = BTreeSet::new();
    let mut parts = 0;
    for (ai, atomic) in clump.atomics.iter().enumerate() {
        let Some(g) = clump.geometries.get(atomic.geometry as usize) else {
            continue;
        };
        if g.vertices.is_empty() {
            continue;
        }
        let m = world
            .get(atomic.frame as usize)
            .copied()
            .unwrap_or(glam::Mat4::IDENTITY);
        let uv0 = g.uv_sets.first();
        for v in &g.vertices {
            let p = m.transform_point3(*v);
            writeln!(obj, "v {} {} {}", p.x, p.y, p.z)?;
        }
        for i in 0..g.vertices.len() {
            let uv = uv0.and_then(|s| s.get(i)).copied().unwrap_or([0.0, 0.0]);
            writeln!(obj, "vt {} {}", uv[0], 1.0 - uv[1])?;
        }
        let has_normals = g.normals.len() == g.vertices.len();
        if has_normals {
            for n in &g.normals {
                let nn = m.transform_vector3(*n).normalize_or_zero();
                writeln!(obj, "vn {} {} {}", nn.x, nn.y, nn.z)?;
            }
        }
        let mut by_mat: BTreeMap<u16, Vec<[u32; 3]>> = BTreeMap::new();
        if !g.meshes.is_empty() {
            for mesh in &g.meshes {
                let list = by_mat.entry(mesh.material as u16).or_default();
                if g.tristrip {
                    for (i, w) in mesh.indices.windows(3).enumerate() {
                        if w[0] == w[1] || w[1] == w[2] || w[0] == w[2] {
                            continue;
                        }
                        list.push(if i % 2 == 0 {
                            [w[0], w[1], w[2]]
                        } else {
                            [w[1], w[0], w[2]]
                        });
                    }
                } else {
                    for t in mesh.indices.chunks_exact(3) {
                        list.push([t[0], t[1], t[2]]);
                    }
                }
            }
        } else {
            for t in &g.triangles {
                by_mat.entry(t.material).or_default().push([
                    t.v[0] as u32,
                    t.v[1] as u32,
                    t.v[2] as u32,
                ]);
            }
        }
        for (mat_id, tris) in by_mat {
            let material = g.materials.get(mat_id as usize);
            let tex = material
                .and_then(|mm| mm.texture.as_ref())
                .map(|t| t.name.to_ascii_lowercase());
            let mat_name = format!("atomic{ai}_mat{mat_id}");
            writeln!(obj, "o {stem}_atomic{ai}_mat{mat_id}")?;
            writeln!(obj, "usemtl {mat_name}")?;
            writeln!(mtl, "newmtl {mat_name}")?;
            if let Some(c) = material.map(|mm| mm.color) {
                writeln!(
                    mtl,
                    "Kd {} {} {}",
                    c[0] as f32 / 255.0,
                    c[1] as f32 / 255.0,
                    c[2] as f32 / 255.0
                )?;
            }
            if let Some(t) = &tex {
                writeln!(mtl, "map_Kd {t}.png")?;
                if textures.insert(t.clone()) {
                    match packs.texture_rgba(t) {
                        Ok((w, h, rgba)) => save_png(&folder.join(format!("{t}.png")), w, h, rgba)?,
                        Err(e) => println!("  texture {t} : {e}"),
                    }
                }
            }
            for [a, b, c] in tris {
                let (a, b, c) = (a as usize + base, b as usize + base, c as usize + base);
                if has_normals {
                    writeln!(obj, "f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}")?;
                } else {
                    writeln!(obj, "f {a}/{a} {b}/{b} {c}/{c}")?;
                }
            }
            parts += 1;
        }
        base += g.vertices.len();
    }
    fs::write(out, obj)?;
    fs::write(folder.join(format!("{stem}.mtl")), mtl)?;
    println!(
        "{pack}/{name} → {} ({parts} parties, {} textures, {} frames)",
        out.display(),
        textures.len(),
        clump.frames.len()
    );
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let game = PathBuf::from(
        args.get(1)
            .ok_or_else(|| anyhow::anyhow!("dossier Game attendu"))?,
    );
    let mut packs = Packs {
        game: game.clone(),
        cache: HashMap::new(),
    };
    match args.get(2).map(String::as_str) {
        Some("table") => {
            let file = &args[3];
            let keys: Vec<&str> = args
                .get(4)
                .map(|k| k.split(',').collect())
                .unwrap_or_default();
            let t = IniTable::from_encrypted(&fs::read(game.join("ini").join(file))?);
            for s in &t.sections {
                let line: Vec<String> = keys
                    .iter()
                    .map(|k| format!("{k}={}", s.get(k).unwrap_or("-")))
                    .collect();
                println!("[{}] {}", s.name, line.join(" "));
            }
        }
        Some("model") => export_obj(&mut packs, &args[3], &args[4], Path::new(&args[5]))?,
        Some("texture") => {
            let (w, h, rgba) = packs.texture_rgba(&args[3])?;
            save_png(Path::new(&args[4]), w, h, rgba)?;
            println!("{} → {} ({w}x{h})", args[3], args[4]);
        }
        Some("file") => {
            let bytes = packs.read(&args[3], &args[4])?;
            fs::write(&args[5], &bytes)?;
            println!("{}/{} → {} ({} o)", args[3], args[4], args[5], bytes.len());
        }
        Some("list") => {
            let filter = args.get(4).map(|s| s.to_ascii_lowercase());
            let (idx, _) = packs.open(&args[3])?;
            for e in &idx.entries {
                if filter
                    .as_ref()
                    .map(|f| e.name.contains(f.as_str()))
                    .unwrap_or(true)
                {
                    println!("{} {}", e.name, e.size);
                }
            }
        }
        _ => anyhow::bail!("sous-commande : table | model | texture | file | list"),
    }
    Ok(())
}
