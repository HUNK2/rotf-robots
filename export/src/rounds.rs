//! Each weapon's `round` in `<pack>.json`: what its shots show, sound and light from the muzzle to
//! where they end, as the game's data has them (`character::WeaponEffects`, and the weapon's muzzle
//! light from `point_light::load_flash_lights`). Templates are named; their data is the robot's
//! `effects.templates`. The rules that run them are MECHANICS.md's "Effects" and the reference's
//! `arsenal`.

use serde_json::{Value, json};
use tf2_core::character::{CharacterData, ImpactPreset, WeaponEffects};
use tf2_core::debris::DebrisPiece;
use tf2_core::formats::scene::PlacedEmitterDef;
use tf2_core::particles::Template;
use tf2_core::point_light::{FlashLight, LevelLight};
use tf2_core::rumble::RumbleDef;

use crate::names::Names;
use crate::run::num;

fn v3(v: glam::Vec3) -> Value {
    json!([num(v.x), num(v.y), num(v.z)])
}

fn template(t: &Template, names: &Names) -> Value {
    json!(names.name(t.name))
}

/// A Light as the game stores it, in its object's space (game axes, metres; angles radians).
pub fn light(l: &LevelLight) -> Value {
    json!({
        "kind": format!("{:?}", l.kind).to_lowercase(),
        "position": v3(l.position),
        "direction": v3(l.direction),
        "colour": v3(l.colour),
        "colour2": v3(l.colour2),
        "range": num(l.range),
        "range2": num(l.range2),
        "attenuate": num(l.attenuate),
        "attenuate2": num(l.attenuate2),
        "hotspot": num(l.hotspot),
        "falloff": num(l.falloff),
        "affect_diffuse": l.affect_diffuse,
        "affect_specular": l.affect_specular,
        "ambient_only": l.ambient_only,
    })
}

/// A Light that removes itself: `time` seconds, fading over the last `time - fade_start` when
/// `fade_start` is 0 or more (its colour times what is left over that span), not at all when -1.
pub fn flash(f: &FlashLight) -> Value {
    json!({ "light": light(&f.light), "time": num(f.time), "fade_start": num(f.fade_start) })
}

fn rumble(r: &RumbleDef) -> Value {
    json!({
        "kind": r.kind,
        "duration": num(r.duration),
        "strength": num(r.strength),
        "scale_type": r.scale_type,
        "distance_near": num(r.distance_near),
        "distance_far": num(r.distance_far),
    })
}

fn emitter(e: &PlacedEmitterDef, names: &Names) -> Value {
    json!({
        "node_index": e.node,
        "bind_position": [num(e.bind[3][0]), num(e.bind[3][1]), num(e.bind[3][2])],
        "template": template(&e.template, names),
        "auto_start": e.auto_start,
        "use_rotation": e.use_rot,
        "use_scale": e.use_scale,
        "delete_when_done": e.delete_when_done,
        "instant_kill": e.instant_kill,
        "spawn_chance": num(e.spawn_chance),
    })
}

fn scorch(info: &tf2_core::ssd::Info) -> Value {
    json!({
        "radius": num(info.radius),
        "frames": info.frames,
        "noise_ratio": num(info.noise_ratio),
        "chaos": num(info.chaos),
        "thresholds": info.thresholds.map(num),
        "softness": info.softness.map(num),
    })
}

/// What a round does on each surface: the entry for a surface is the first that lists it, else the
/// first that lists the default surface.
pub fn impact(p: &ImpactPreset, names: &Names) -> Value {
    json!({
        "by_surface": p.infos.iter().map(|info| json!({
            "surfaces": info.materials.iter().map(|&m| names.name(m)).collect::<Vec<_>>(),
            "sound": (info.sound != 0).then(|| names.name(info.sound)),
            "effect": info.effect.map(|e| names.name(e)),
            "effect_flags": info.effect_flags,
        })).collect::<Vec<_>>(),
        "scorch": p.ssd.iter().map(scorch).collect::<Vec<_>>(),
    })
}

fn debris(d: &DebrisPiece, file: Option<String>, names: &Names) -> Value {
    let f = &d.def;
    json!({
        "file": file,
        "at": v3(d.at),
        "turn": [num(d.turn.x), num(d.turn.y), num(d.turn.z), num(d.turn.w)],
        "scale": num(d.scale),
        "damage_point": v3(d.damage_point),
        "mesh_nodes": d.object.nodes.iter().map(|n| n.name.clone()).collect::<Vec<_>>(),
        "emitters": d.emitters.iter().map(|e| emitter(e, names)).collect::<Vec<_>>(),
        "throw": {
            "speed": num(f.speed), "speed_min": num(f.speed_min),
            "rot_speed": num(f.rot_speed), "rot_speed_min": num(f.rot_speed_min),
            "spread": num(f.spread), "start_angle": num(f.start_angle),
            "radius_xy": num(f.radius_xy), "radius_z": num(f.radius_z), "force_speed": num(f.force_speed),
            "die_condition": f.die_condition, "die_time": num(f.die_time), "die_type": f.die_type,
            "spin_type": f.spin_type, "linear_damping": num(f.linear_damping), "angular_damping": num(f.angular_damping),
            "inflict_damage": num(f.inflict_damage),
        },
    })
}

/// One effects row: a weapon's round, or its alternative round.
pub fn round(r: &WeaponEffects, pieces: &[crate::Piece], names: &Names) -> Value {
    let debris_file = |k: usize| pieces.iter().filter(|p| p.kind == crate::PieceKind::Debris && p.weapons.contains(&r.name)).nth(k).map(|p| format!("{}.glb", p.file));
    let events = |ids: &[u32]| ids.iter().map(|&id| names.name(id)).collect::<Vec<_>>();
    json!({
        "muzzle_flash": r.muzzle_flash.as_ref().map(|t| template(t, names)),
        "impact": impact(&r.impact, names),
        "trail": r.trail.as_ref().map(|t| template(t, names)),
        "emitters": r.emitters.iter().map(|e| emitter(e, names)).collect::<Vec<_>>(),
        "flight_lights": r.flight_lights.iter().map(light).collect::<Vec<_>>(),
        "flight_sounds": events(&r.flight_sounds),
        "blast": r.blast.as_ref().map(|t| template(t, names)),
        "blast_scorch": r.blast_ssd.iter().map(scorch).collect::<Vec<_>>(),
        "blast_sounds": events(&r.blast_sounds),
        "blast_light": r.blast_light.as_ref().map(flash),
        "blast_rumble": r.blast_rumble.as_ref().map(rumble),
        "after_blast": r.after_blast.iter().map(|p| json!({
            "template": template(&p.template, names),
            "offset": v3(p.offset),
            "use_rotation": p.use_rot,
            "use_scale": p.use_scale,
            "on": [num(p.on.0), num(p.on.1)],
            "off": [num(p.off.0), num(p.off.1)],
        })).collect::<Vec<_>>(),
        "wave": r.wave.as_ref().map(|w| json!({
            "emitter": w.emitter.as_ref().map(|e| emitter(e, names)),
            "light": w.light.as_ref().map(flash),
            "rumble": w.rumble.as_ref().map(rumble),
            "life": num(w.life),
            "check_ground_height": num(w.check_ground_height),
            "init_check_ground_radius": num(w.init_check_ground_radius),
            "radius": num(w.radius),
            "sounds": events(&w.sounds),
        })),
        "debris": r.debris.iter().enumerate().map(|(k, d)| debris(d, debris_file(k), names)).collect::<Vec<_>>(),
    })
}

/// The effects row named `key` (a weapon's name, or its alternative round's key), if he has one.
pub fn row(data: &CharacterData, key: u32) -> Option<&WeaponEffects> {
    data.weapon_effects.iter().find(|r| r.name == key)
}

/// Every template name these rounds use, so `effects.templates` can hold each of them.
pub fn templates<'a>(rows: impl IntoIterator<Item = &'a WeaponEffects>) -> Vec<u32> {
    let mut out = Vec::new();
    for r in rows {
        out.extend(r.muzzle_flash.iter().chain(&r.trail).chain(&r.blast).map(|t| t.name));
        out.extend(r.emitters.iter().map(|e| e.template.name));
        out.extend(r.after_blast.iter().map(|p| p.template.name));
        out.extend(r.wave.iter().flat_map(|w| w.emitter.iter()).map(|e| e.template.name));
        out.extend(r.debris.iter().flat_map(|d| d.emitters.iter()).map(|e| e.template.name));
        out.extend(r.impact.infos.iter().flat_map(|i| i.template.iter()).map(|t| t.name));
    }
    out.sort_unstable();
    out.dedup();
    out
}
