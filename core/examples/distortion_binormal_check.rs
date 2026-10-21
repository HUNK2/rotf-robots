//! For each robot of the roster: the parts drawn with `distortion_vc_emissive` (technique
//! 0x787f26b8), and how many of their vertices have a binormal stream that differs from the
//! port's rebuild `sign * cross(N, T)` (the sign as `game\src\assets.rs` takes it). The game's
//! vertex shader 0b5740 reads the stream; the port's distortion material uses the rebuild.
//! `cargo run -p tf2-core --example distortion_binormal_check -- [game dir]`

use std::path::PathBuf;

use glam::Vec3;
use tf2_core::character;

const ROSTER: [&str; 13] = [
    "bumblebee", "optimus", "ironhide", "ratchet", "megatron", "longhaul", "audir8", "jetfire", "firestorm",
    "starscream", "blackout", "optimusflight", "megatronflight",
];
const DISTORTION: u32 = 0x787f_26b8;

fn main() {
    let game_dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| r"C:\Games2".to_owned()));
    let (mut parts, mut verts, mut differ, mut no_stream, mut worst) = (0, 0, 0, 0, 0.0f32);
    for pack in ROSTER {
        let data = character::load(&game_dir, pack).expect("the character pack");
        let lib = &data.library;
        for mesh in lib.meshes.values() {
            for part in &mesh.parts {
                let Some(m) = part.material.and_then(|id| lib.materials.get(&id)) else { continue };
                if m.technique != DISTORTION {
                    continue;
                }
                parts += 1;
                let n = part.positions.len();
                if part.normals.len() != n || part.tangents.len() != n || part.binormals.len() != n {
                    no_stream += 1;
                    continue;
                }
                for i in 0..n {
                    let (nn, t, b) = (Vec3::from(part.normals[i]), Vec3::from(part.tangents[i]), Vec3::from(part.binormals[i]));
                    let sign = if nn.cross(t).dot(b) < 0.0 { -1.0 } else { 1.0 };
                    let rebuilt = nn.cross(t) * sign;
                    let d = (rebuilt.normalize_or_zero() - b.normalize_or_zero()).length();
                    verts += 1;
                    worst = worst.max(d);
                    if d > 0.05 {
                        differ += 1;
                    }
                }
            }
        }
        println!("{pack:<15} distortion parts so far {parts}");
    }
    println!("distortion parts {parts} (without a full N/T/B stream {no_stream}); vertices {verts}, differing by > 0.05: {differ}; largest difference {worst:.4}");
}
