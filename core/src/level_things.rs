//! The components of a level's Objects other than its models and collision, placed in the
//! world: the player's spawn points, and whatever else is asked for by type name.
//!
//! Placement is `level_render`'s rule (the thing rides the Object node its attachment's
//! `index` names, under every enclosing `Instance` matrix). [data]

use std::collections::HashSet;
use std::path::PathBuf;

use glam::Vec3;

use crate::formats::lxb::{DataFile, Node};
use crate::formats::pack::{self, Pack};
use crate::level_render::{carries_placement, bind_of, compose, flat, flat32, matrix_of, Mat, IDENTITY, KEY_SCALE, MAX_DEPTH};

/// Calls `found(thing, world, label of the nearest Instance above)` for every attachment
/// thing of the file whose type is one of `types`, once per distinct placement.
pub fn each_thing(file: &DataFile, types: &[&str], found: &mut dyn FnMut(Node, [f32; 12], Option<&str>)) {
    each_thing_placement(file, types, &mut |thing, bind, placement, instance| {
        (found)(thing, flat32(&compose(&bind, &placement)), instance);
    });
}

/// Like `each_thing`, retaining the attachment's object-space bind and its owner's
/// world placement separately. [game] The NavSilhouette loader (006f9b20) bakes the
/// bind's x/y but discards its z; the mesh builder (006f46d0) then applies the owner
/// matrix, including its z. Composing first loses that distinction.
pub(crate) fn each_thing_placement(file: &DataFile, types: &[&str], found: &mut dyn FnMut(Node, Mat, Mat, Option<&str>)) {
    let mut walk = Walk { types, found, visited: HashSet::new() };
    visit(file.root(), IDENTITY, None, None, 0, &mut walk);
}

struct Walk<'a, 'f> {
    types: &'a [&'a str],
    found: &'a mut dyn FnMut(Node<'f>, Mat, Mat, Option<&'f str>),
    visited: HashSet<(usize, usize, [i64; 12])>,
}

/// `instance`: the label of the nearest Instance above (empty for one with no label); None
/// while no Instance is above, and nothing is placed then (`level_render::visit`).
fn visit<'f>(node: Node<'f>, acc: Mat, instance: Option<&'f str>, hint: Option<&'f str>, depth: u32, walk: &mut Walk<'_, 'f>) {
    if depth > MAX_DEPTH || !carries_placement(node) {
        return;
    }
    let placed = node.get("matrix").and_then(matrix_of).filter(|_| node.get("things").is_some());
    let acc = placed.map_or(acc, |m| compose(&m, &acc));
    let instance = match placed {
        Some(_) => Some(node.label().and_then(|hash| node.file().name(hash)).or(hint).unwrap_or("")),
        None => instance,
    };
    if let (Some(name), Some(at)) = (node.type_name(), node.offset()) {
        let key = flat(&acc).map(|v| (v * KEY_SCALE).round() as i64);
        if !walk.visited.insert((at, name.as_ptr() as usize, key)) {
            return;
        }
    }
    if let Some(attachments) = node.get("attachments") {
        for attachment in attachments.items() {
            let Some(thing) = attachment.get("thing") else { continue };
            let index = attachment.get("index").and_then(Node::int).unwrap_or(0);
            let bind = bind_of(node, index);
            let riding = compose(&bind, &acc);
            if instance.is_some() && thing.type_name().is_some_and(|t| walk.types.contains(&t)) {
                (walk.found)(thing, bind, acc, instance);
            }
            visit(thing, riding, instance, None, depth + 1, walk);
        }
    }
    for (hash, child) in node.fields() {
        if matches!(node.file().name(hash), Some("attachments" | "sub_matrices" | "matrix" | "hierarchy")) {
            continue;
        }
        visit(child, acc, instance, node.file().name(hash), depth + 1, walk);
    }
    if node.len() > 0 {
        for child in node.items() {
            visit(child, acc, instance, None, depth + 1, walk);
        }
    }
}

/// A `PlayerSpawnPoint` in the world. [data] its fields: `attributes` (the character's
/// attribute set, by name hash), `spawnInVehicleForm`, `ctrlBinding`.
#[derive(Clone, Debug, PartialEq)]
pub struct SpawnPoint {
    /// The label of the Instance that places it (`Player_A_Optimus`).
    pub name: String,
    pub pos: Vec3,
    /// Heading about z of the point's y axis (the engine's forward), radians, 0 along +y
    /// and positive to the left. [assumed: a character faces along the point's y axis]
    pub yaw: f32,
    pub attributes: u32,
    pub in_vehicle_form: bool,
    pub source: String,
}

/// Every spawn point of the packs, in file order.
pub fn spawn_points(packs: &[PathBuf]) -> Result<Vec<SpawnPoint>, String> {
    let mut out = Vec::new();
    for path in packs {
        let pack = Pack::open(path)?;
        let file_name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        for chunk in pack.of_type(pack::DATA) {
            // The type's name is in the chunk's own name table as text.
            let data = pack.data(chunk);
            if !data.windows(16).any(|w| w == b"PlayerSpawnPoint") {
                continue;
            }
            let file = DataFile::parse(data.to_vec()).map_err(|e| format!("chunk {}: {e}", chunk.index))?;
            each_thing(&file, &["PlayerSpawnPoint"], &mut |thing, world, instance| {
                out.push(SpawnPoint {
                    name: instance.unwrap_or_default().to_owned(),
                    pos: Vec3::new(world[9], world[10], world[11]),
                    yaw: (-world[3]).atan2(world[4]),
                    attributes: thing.get("attributes").and_then(Node::int).unwrap_or(0) as u32,
                    in_vehicle_form: thing.get("spawnInVehicleForm").is_some_and(Node::is_true),
                    source: format!("{file_name}#{} @ {:#x}", chunk.index, thing.offset().unwrap_or(0)),
                });
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs the game install. [data] `notes\survey-levels.md`: the test level's free-roam
    /// pack has one spawn point, `Player_A_Optimus`, at (-137.458, -366.849, 0.897).
    #[test]
    fn the_test_level_has_one_spawn_point() {
        let path = crate::install::dir().join("levels/us_test/mission.str");
        if !path.is_file() {
            return;
        }
        let points = spawn_points(&[path]).unwrap();
        assert_eq!(points.len(), 1, "{points:?}");
        assert_eq!(points[0].name, "Player_A_Optimus");
        assert!((points[0].pos - Vec3::new(-137.458, -366.849, 0.897)).length() < 0.01, "{:?}", points[0]);
    }
}
