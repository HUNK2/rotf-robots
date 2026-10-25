//! For each node of a character's objects whose bind matrix is not a plain move, turn and scale:
//! how far the nearest move, turn and scale is from it (the largest difference of a matrix entry),
//! its axes' lengths and the angles between them.
//! `cargo run -p rotf-export --example bind_peek -- <pack>`

use glam::Mat4;
use rotf_export::model::matrix;
use tf2_core::formats::scene::ObjectDef;

fn peek(label: &str, object: &ObjectDef) {
    let world: Vec<Mat4> = object.nodes.iter().map(|n| matrix(&n.bind)).collect();
    for (i, node) in object.nodes.iter().enumerate() {
        let local = node.parent.map_or(world[i], |p| world[p].inverse() * world[i]);
        let (s, r, t) = local.to_scale_rotation_translation();
        let off = (Mat4::from_scale_rotation_translation(s, r, t) - local).to_cols_array().iter().fold(0.0f32, |a, v| a.max(v.abs()));
        if off > 1e-3 {
            let axes = [local.x_axis.truncate(), local.y_axis.truncate(), local.z_axis.truncate()];
            let angle = |a: glam::Vec3, b: glam::Vec3| a.angle_between(b).to_degrees();
            println!(
                "{label} {} ({i}): off {off:.4}, axis lengths {:.3} {:.3} {:.3}, angles xy {:.1} yz {:.1} zx {:.1}, det {:.3}",
                node.name,
                axes[0].length(),
                axes[1].length(),
                axes[2].length(),
                angle(axes[0], axes[1]),
                angle(axes[1], axes[2]),
                angle(axes[2], axes[0]),
                local.determinant()
            );
        }
    }
}

fn main() {
    let pack = std::env::args().nth(1).unwrap_or_else(|| "starscream".into());
    let data = tf2_core::character::load(tf2_core::install::dir(), &pack).unwrap();
    peek("robot", &data.robot);
    if let Some(v) = &data.vehicle {
        peek("vehicle", v);
    }
    for (i, o) in data.weapon_objects.iter().enumerate() {
        if let Some(o) = o {
            peek(&format!("weapon{i}"), o);
        }
    }
    for (i, m) in data.melee_weapons.iter().enumerate() {
        if let Some(o) = &m.object {
            peek(&format!("melee{i}"), o);
        }
    }
}
