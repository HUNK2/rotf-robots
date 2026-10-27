//! `<pack>.json`'s `effects`: every particle effect of the robot's pack as the game stores it, with
//! the textures its elements draw, and the things on the robot's and the vehicle's models that a
//! trigger or a state starts by name (effects, emitters, sound players, skid marks, ribbons). The
//! formulas that run them are MECHANICS.md's "Effects".

use serde_json::{Map, Value, json};
use tf2_core::character::CharacterData;
use tf2_core::formats::lxb::DataFile;
use tf2_core::formats::scene::ObjectDef;
use tf2_core::particles::Template;

use crate::describe::value;
use crate::mechanics::things;
use crate::model::Shared;
use crate::names::Names;
use crate::run::num;

/// The `effects` block. `pack` is the robot's own pack's data files; `others` are the shared
/// files, from which only the templates in `wanted` (those his rounds name) are taken.
pub fn effects(data: &CharacterData, pack: &[DataFile], others: &[DataFile], wanted: &[u32], shared: &mut Shared, names: &Names) -> Value {
    let mut templates = Map::new();
    let own = things(pack, "ParticleTemplate");
    let more = things(others, "ParticleTemplate").into_iter().filter(|node| node.label().is_some_and(|l| wanted.contains(&l)));
    for node in own.into_iter().chain(more) {
        let Some(label) = node.label() else { continue };
        let name = names.name(label);
        if templates.contains_key(&name) {
            continue;
        }
        let textures: Vec<Value> = Template::read(node)
            .map(|t| t.elements.iter().map(|e| shared.texture(e.renderer.texture).map_or(Value::Null, |f| json!(f.path()))).collect())
            .unwrap_or_default();
        let mut data = value(node, names, 0);
        codes(&mut data);
        // The renderer's `Texture` is the asset's id and build path as stored: the written file instead.
        if let Some(elements) = data["elements"].as_array_mut() {
            for (element, texture) in elements.iter_mut().zip(&textures) {
                if element["renderer"].is_object() {
                    element["renderer"]["Texture"] = texture.clone();
                }
            }
        }
        templates.insert(name, json!({ "textures": textures, "data": data }));
    }
    let mut on_models = Vec::new();
    on_model(&data.robot, "robot", names, &mut on_models);
    if let Some(vehicle) = &data.vehicle {
        on_model(vehicle, "vehicle", names, &mut on_models);
    }
    json!({ "templates": templates, "on_models": on_models })
}

/// An action's or a domain's `id` is four letters stored as a number (`Brst`, `SrcA`, `1dBx`): written
/// as the letters.
fn codes(v: &mut Value) {
    match v {
        Value::Object(map) => {
            if let Some(id) = map.get("id").and_then(Value::as_i64) {
                let bytes = (id as u32).to_be_bytes();
                if bytes.iter().all(u8::is_ascii_alphanumeric) {
                    map.insert("id".into(), json!(String::from_utf8_lossy(&bytes)));
                }
            }
            map.values_mut().for_each(codes);
        }
        Value::Array(items) => items.iter_mut().for_each(codes),
        _ => {}
    }
}

/// The things of one model that answer to a name, or run by themselves.
fn on_model(object: &ObjectDef, model: &str, names: &Names, out: &mut Vec<Value>) {
    let node = |index: usize| object.nodes.get(index).map_or(Value::Null, |n| json!(n.name));
    let name = |hash: u32| if hash == 0 { Value::Null } else { json!(names.name(hash)) };
    for e in &object.effects {
        out.push(json!({
            "kind": "effect",
            "model": model,
            "trigger": name(e.script_name),
            "node": node(e.node),
            "template": name(e.template.name),
            "upright": e.upright,
            "follows_node": e.attach,
            "mesh": e.mesh,
            "timeout": e.timeout.map(|(delete, fade)| json!({ "delete": num(delete), "fade": num(fade) })),
        }));
    }
    for e in &object.emitters {
        out.push(json!({
            "kind": "emitter",
            "model": model,
            "node": node(e.node),
            "template": name(e.template.name),
            "auto_start": e.auto_start,
            "use_rotation": e.use_rot,
            "use_scale": e.use_scale,
            "delete_when_done": e.delete_when_done,
            "spawn_chance": num(e.spawn_chance),
        }));
    }
    for s in &object.sound_players {
        out.push(json!({
            "kind": "sound",
            "model": model,
            "trigger": name(s.script),
            "node": node(s.node),
            "event": name(s.id),
            "waits_for_trigger": s.wait_for_trigger,
            "once": s.once,
            "retrigger": s.retrigger,
            "key_off_on_untrigger": s.key_off_on_untrigger,
        }));
    }
    for (index, script, events) in &object.spawn_sound_players {
        out.push(json!({
            "kind": "spawned_sound",
            "model": model,
            "trigger": name(*script),
            "node": node(*index),
            "events": events.iter().map(|e| names.name(*e)).collect::<Vec<_>>(),
        }));
    }
    for m in &object.skid_marks {
        out.push(json!({
            "kind": "skid_mark",
            "model": model,
            "node": node(m.node),
            "tyre": name(m.tyre),
            "point_length": num(m.point_length),
            "point_width": num(m.point_width),
            "colour": m.colour.map(num),
            "material": name(m.material),
        }));
    }
    for f in &object.footsteps {
        out.push(json!({
            "kind": "footstep",
            "model": model,
            "node": node(f.node),
            "side": name(f.side),
            "height": num(f.height),
        }));
    }
    for (index, side) in &object.climb_steps {
        out.push(json!({
            "kind": "climb_step",
            "model": model,
            "node": node(*index),
            "limb": tf2_core::formats::anim::CLIMB_SIDES.iter().position(|s| s == side).map_or(Value::Null, |i| json!(crate::describe::LIMBS[i])),
        }));
    }
    for r in &object.ribbons {
        out.push(json!({
            "kind": "ribbon",
            "model": model,
            "trigger": name(r.name),
            "links": r.links.iter().map(|l| names.name(*l)).collect::<Vec<_>>(),
            "material": name(r.material),
            "trails": r.trails,
            "subdivisions": r.subdivisions,
            "trigger_speed": num(r.trigger_speed),
            "enabled": r.enabled,
        }));
    }
    // The knocks of the model against the world (CollisionFX, CollisionSFX): by how hard and how it
    // touches (impact, roll, scrape), within its speeds. [data] The rules: `collisionfx.rs`.
    let filter = |d: &tf2_core::collisionfx::Def| json!({
        "mask": d.mask,
        "types": d.types,
        "min_speed": num(d.min_speed),
        "max_speed": num(d.max_speed),
        "align_to_normal": d.align,
    });
    let by_kind = |row: [Value; 3]| { let [impact, roll, scrape] = row; json!({"impact": impact, "roll": roll, "scrape": scrape}) };
    for d in &object.collision_fx {
        let mut entry = filter(d);
        entry["kind"] = json!("collision_effects");
        entry["model"] = json!(model);
        entry["soft"] = by_kind(d.particles[0].each_ref().map(|t| t.as_ref().map_or(Value::Null, |t| name(t.name))));
        entry["hard"] = by_kind(d.particles[1].each_ref().map(|t| t.as_ref().map_or(Value::Null, |t| name(t.name))));
        entry["soft_sounds"] = by_kind(d.sounds[0].map(name));
        entry["hard_sounds"] = by_kind(d.sounds[1].map(name));
        out.push(entry);
    }
    for d in &object.collision_sounds {
        let mut entry = filter(&d.filter);
        entry["kind"] = json!("collision_sounds");
        entry["model"] = json!(model);
        let choice = |c: &tf2_core::collisionfx::SoundChoice| if c.id == 0 { Value::Null } else {
            json!({"event": names.name(c.id), "once": c.once, "attached": c.attached})
        };
        for (row, key) in d.choices.iter().zip(["hard", "soft", "bottom"]) {
            entry[key] = by_kind(row.each_ref().map(choice));
        }
        out.push(entry);
    }
}
