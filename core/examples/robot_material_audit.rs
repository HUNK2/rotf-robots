//! Every robot pack's materials: technique, how the port draws it, textures present. Reads the
//! install; writes nothing. Usage: `robot_material_audit [pack...] [--params]` (default: every pack
//! in `characters\`). `--params` prints each material's parameters by name where `names.tsv` knows it.
use std::collections::{BTreeMap, HashMap};
use tf2_core::formats::{model::{Library, ParamValue}, pack::Pack};

/// The techniques the port's own shaders draw (`map_audit`'s list, the character side). [code]
const PORTED: &[&str] = &["aniso_paint", "aniso_paint_x", "cheap_plus", "glass_plus", "glass_plus_x", "flatcard_emissive", "glass_cheap", "emissive",
    "dull", "dull_vc", "dull_cheap", "cheap", "cheap_cheap", "basic", "basic_cheap", "shiny", "shiny_cheap",
    "cheap_plus_boost", "cheap_boost", "glass_plus_vc", "cheap_plus_vc", "cheap_vc", "emissive_vc",
    "glow_soft_vc_emissive", "glow_vc_mask_emissive", "decal", "decal_reflective", "dull_fx_emissive", "cheap_mask", "dull_mask"];

fn main() {
    let game = tf2_core::install::dir();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let params = args.iter().any(|a| a == "--params");
    let mut packs: Vec<String> = args.iter().filter(|a| !a.starts_with("--")).cloned().collect();
    if packs.is_empty() {
        packs = std::fs::read_dir(game.join("characters")).unwrap().filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".str")).map(str::to_owned)).collect();
        packs.sort();
    }
    let bytes = std::fs::read(game.join("shaders.lsa")).unwrap();
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let mut techniques = HashMap::new();
    for i in 0..word(16) {
        let at = word(20) + i * 0xa0;
        let text = &bytes[at + 32..at + 96];
        techniques.insert(word(at) as u32, String::from_utf8_lossy(&text[..text.iter().position(|&b| b == 0).unwrap_or(64)]).into_owned());
    }
    let mut names = HashMap::new();
    if params {
        if let Ok(text) = std::fs::read_to_string("C:/Bumblebee/export/data/names.tsv") {
            for line in text.lines() {
                let mut f = line.split('\t');
                if let (Some(h), Some(n)) = (f.next(), f.next()) {
                    if let Ok(h) = u32::from_str_radix(h.trim_start_matches("0x"), 16) { names.insert(h, n.to_owned()); }
                }
            }
        }
    }
    let mut totals: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for name in &packs {
        let Ok(pack) = Pack::open(&game.join("characters").join(format!("{name}.str"))) else { println!("{name}: cannot open"); continue };
        let library = Library::load(&pack);
        if args.iter().any(|a| a == "--textures") {
            // Every single-face texture's path and size, and what the 4x upscale picks for it.
            let mut rows: Vec<_> = library.textures.values().filter(|t| t.faces == 1).collect();
            rows.sort_by(|a, b| a.name.cmp(&b.name));
            for t in rows {
                let kind = tf2_core::texture_upscale::map_kind(t);
                println!("TEX {name} {} {}x{} {:?} {kind:?}", t.name, t.width, t.height, t.format);
            }
            continue;
        }
        let mut used: BTreeMap<u32, (usize, usize)> = BTreeMap::new();
        for mesh in library.meshes.values() {
            for part in &mesh.parts {
                if let Some(m) = part.material {
                    let e = used.entry(m).or_default();
                    e.0 += 1;
                    e.1 += part.indices.len() / 3;
                }
            }
        }
        println!("== {name}: {} meshes, {} materials ({} used), {} textures", library.meshes.len(), library.materials.len(), used.len(), library.textures.len());
        let mut rows: Vec<_> = used.iter().filter_map(|(id, n)| library.materials.get(id).map(|m| (m, n))).collect();
        rows.sort_by(|a, b| a.0.name.cmp(&b.0.name));
        for (m, (parts, tris)) in rows {
            let tech = techniques.get(&m.technique).cloned().unwrap_or_else(|| format!("#{:08x}", m.technique));
            let how = if PORTED.contains(&tech.as_str()) { "ported" } else { "FALLBACK" };
            let e = totals.entry(format!("{tech} [{how}]")).or_default();
            e.0 += parts; e.1 += tris;
            let tex: Vec<String> = m.params.iter().filter_map(|p| match p.value {
                ParamValue::Texture(Some(t)) if p.owner == 0 => Some(match library.textures.get(&t) {
                    Some(x) => format!("{}({}x{} {:?})", x.name.rsplit(['\\', '/']).next().unwrap_or(&x.name), x.width, x.height, x.format),
                    None => format!("{t:08x}(MISSING)"),
                }),
                _ => None,
            }).collect();
            println!("  {:<28} {:<16} {:<8} parts {parts:>3} tris {tris:>6}  {}", m.name, tech, how, tex.join(" "));
            if params {
                for p in m.params.iter().filter(|p| p.owner == 0) {
                    let v = match p.value { ParamValue::Number(n) => format!("{n}"), ParamValue::Word(w) => format!("word {w:#x}"),
                        ParamValue::Vector(v) => format!("{v:?}"), ParamValue::Texture(t) => format!("texture {t:x?}") };
                    println!("      {:<28} {v}", names.get(&p.name).cloned().unwrap_or_else(|| format!("#{:08x}", p.name)));
                }
            }
        }
    }
    println!("== totals by technique (parts, triangles)");
    for (k, (p, t)) in totals { println!("  {k:<40} {p:>5} {t:>8}"); }
}
