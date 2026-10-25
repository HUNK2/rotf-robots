//! Devastator's clips as the port reads them out of the Cairo City boss mission pack: each set's
//! clips with length, root travel and turn, attack data and event counts, and his height.
//! `cargo run -p tf2-core --example devastator_clips` (reads the install `tf2_core::install` finds).

use std::path::Path;

fn main() {
    let dir = tf2_core::install::dir_string();
    if std::env::args().any(|a| a == "--graph") {
        // His control graph as the port builds it from `characterAttributesDevastator`.
        let t = tf2_core::tuning::load(Path::new(&dir), "Devastator").expect("Devastator tuning");
        let g = &t.control;
        for (i, st) in g.states.iter().enumerate() {
            println!("[{i}] {} : {} anim {:08x} set {:?} mode {:08x} overlay {}", st.name, st.class, st.anim, st.anim_set, st.motion_mode, st.overlay);
            for tr in &st.transitions {
                let c = &g.connections[tr.connection];
                println!("    -> {} by {} ({:?}) priority {}", tr.output.map_or("-".into(), |o| g.states[o].name.clone()), c.name, c.test, tr.priority);
            }
        }
        return;
    }
    if std::env::args().any(|a| a == "--ik") {
        ik_survey(Path::new(&dir));
        return;
    }
    let mut data = tf2_core::character::load(Path::new(&dir), "devastator").expect("devastator");
    // `--reroot`: his clips as the port plays them, the root on his chest (`giant::reroot`).
    if std::env::args().any(|a| a == "--reroot") {
        tf2_core::giant::reroot(&mut data.animations, &data.robot);
    }
    if std::env::args().any(|a| a == "--rules") {
        // Each set's entry rules and each clip's exits, names resolved over his own clip and set names.
        let rules = tf2_core::tuning::RuleSet::all(&data.animations);
        let mut names = std::collections::HashMap::new();
        for (set, refs) in &data.animations.sets {
            names.insert(tf2_core::formats::hash::crc32(set.as_bytes()), set.clone());
            for r in refs {
                names.insert(tf2_core::formats::hash::crc32(r.id.as_bytes()), r.id.clone());
            }
        }
        let name = |h: u32| names.get(&h).cloned().unwrap_or_else(|| format!("{h:08x}"));
        for set in rules.values() {
            println!("{}: incoming {:?}", set.set_name, set.incoming.iter().map(|i| (name(i.to_set), name(i.to_id))).collect::<Vec<_>>());
            for c in &set.clips {
                println!("    {} exits {:?}", c.name, c.exits.iter().map(|e| (name(e.to_set), name(e.to_id))).collect::<Vec<_>>());
            }
        }
        return;
    }
    if std::env::args().any(|a| a == "--frames") {
        frames(&data);
        return;
    }
    if std::env::args().any(|a| a == "--limbs") {
        limbs(&data);
        return;
    }
    if let Some(id) = std::env::args().skip_while(|a| a != "--turn").nth(1) {
        turn(&data, &id);
        return;
    }
    if let Some(id) = std::env::args().skip_while(|a| a != "--slide").nth(1) {
        slide(&data, &id);
        return;
    }
    if std::env::args().any(|a| a == "--vortex") {
        vortex(&data);
        return;
    }
    let top = data.robot.nodes.iter().map(|n| n.bind[3][2]).fold(f32::MIN, f32::max);
    println!("{} nodes, highest bind z {top:.2}", data.robot.nodes.len());
    // His shockwaves: node, material and its technique, blend mode and texture.
    for (index, wave) in &data.shockwaves {
        let material = wave.material.and_then(|id| data.library.materials.get(&id));
        let name = |h: u32| tf2_core::formats::hash::crc32(b"Additive") == h;
        println!("shockwave {} on node {} ({}): material {:?} technique {:?} additive {:?} texture {:?}",
            wave.name, index, data.robot.nodes[*index].name, wave.material, material.map(|m| format!("{:08x}", m.technique)),
            material.and_then(|m| m.word("blendmode")).map(name), material.and_then(|m| m.texture("diffuse_texture1_texture")));
    }
    let mut sets: Vec<_> = data.animations.sets.iter().collect();
    sets.sort_by(|a, b| a.0.cmp(b.0));
    for (set, refs) in sets {
        for r in refs {
            let Some(c) = &r.clip else { continue };
            let path = c.root_path();
            let end = path.last().copied().unwrap_or_default();
            let turn = c.root_turn().last().copied().unwrap_or(0.0).to_degrees();
            let root = c.channel(tf2_core::formats::anim::ROOT_CHANNEL);
            let keyed = root.is_some_and(|ch| ch.position.sample(0.0, c.length, false).is_some());
            println!("{set}/{} ticks {:.0} loop {} xfade {:.2} | root keyed {keyed} end ({:.2}, {:.2}, {:.2}) turn {turn:.1} deg | start t ({:.2}, {:.2}) end t ({:.2}, {:.2}) | exits {}",
                r.id, c.length, r.looping, r.crossfade, end.x, end.y, end.z, c.start[3][0], c.start[3][1], c.end[3][0], c.end[3][1], r.exits.len());
            println!("    events: model {} sound {} action {} shake {} weapon {} special-fire {} effect {} trigger {}; attack {:?}",
                c.model_events.len(), c.sound_events.len(), c.action_events.len(), c.shake_events.len(), c.weapon_events.len(),
                c.special_fire_events.len(), c.effect_events.len(), c.trigger_events.len(),
                r.attack.as_ref().map(|a| (a.damage, a.min_range, a.max_range, a.knock_back_speed)));
            for e in &c.sound_events {
                println!("      sound {e:?}");
            }
            if let Some(ch) = root {
                let at: Vec<String> = [0.0, 1.0, 2.0, 4.0, 8.0, 16.0, 28.0, 40.0, 52.0, c.length]
                    .iter()
                    .filter(|&&k| k <= c.length)
                    .map(|&k| ch.position.sample(k, c.length, false).map_or("-".into(), |p| format!("{k:.0}:{:.1}", p[1])))
                    .collect();
                println!("    root y by tick: {}", at.join(" "));
            }
            // The biggest root step between two ticks: a re-basing of the root, not travel.
            let step = path.windows(2).enumerate().map(|(i, w)| (i, (w[1] - w[0]).length())).fold((0, 0.0f32), |a, b| if b.1 > a.1 { b } else { a });
            println!("    biggest root step {:.2} m at tick {}", step.1, step.0);
            for e in &c.action_events {
                println!("      action {e:?}");
            }
            for e in &c.shake_events {
                println!("      shake {e:?}");
            }
            for e in &c.effect_events {
                println!("      effect {e:?}");
            }
        }
    }
}

/// `--ik`: every object type with "IK" or "LookAt" in its name anywhere in his pack (the boss
/// mission's), with how many there are and, for each, the links it names and its side. The
/// playable robots' packs hold no `FootIK` (`footik::roster_survey`).
fn ik_survey(dir: &Path) {
    use std::collections::{BTreeMap, HashSet};
    use tf2_core::formats::lxb::{DataFile, Node};
    use tf2_core::formats::pack::{Pack, DATA};
    fn walk(file: &DataFile, node: Node, depth: u32, seen: &mut HashSet<usize>, found: &mut BTreeMap<String, Vec<String>>) {
        if depth > 80 {
            return;
        }
        if let Some(at) = node.offset() && !seen.insert(at) {
            return;
        }
        if let Some(kind) = node.type_name().filter(|k| k.contains("IK") || k.contains("LookAt")) {
            let links: Vec<String> = node.get("links").into_iter().flat_map(|l| l.items())
                .map(|l| l.int().and_then(|h| file.name(h as u32).map(str::to_owned)).unwrap_or_else(|| format!("{:?}", l.int()))).collect();
            let fields: Vec<String> = node.fields().into_iter().filter_map(|(h, _)| file.name(h).map(str::to_owned)).collect();
            found.entry(kind.to_owned()).or_default().push(format!("links {links:?} side {:?} fields {fields:?}", node.get("side").and_then(Node::int)));
        }
        for (_, child) in node.fields() {
            walk(file, child, depth + 1, seen, found);
        }
        if !node.is_empty() {
            for child in node.items() {
                walk(file, child, depth + 1, seen, found);
            }
        }
    }
    let path = tf2_core::formats::characterlist::pack_file(dir, "devastator");
    let pack = Pack::open(&path).expect("his pack");
    let mut found = BTreeMap::new();
    for chunk in pack.of_type(DATA) {
        let file = DataFile::parse(pack.data(chunk).to_vec()).expect("data chunk");
        walk(&file, file.root(), 0, &mut HashSet::new(), &mut found);
    }
    println!("{}: {} kinds", path.display(), found.len());
    for (kind, list) in found {
        println!("{kind}: {}", list.len());
        let mut unique: Vec<&String> = list.iter().collect();
        unique.sort();
        unique.dedup();
        for item in unique.iter().take(12) {
            println!("    {item}");
        }
    }
}

/// `--limbs`: what the port's foot IK can build on him (`footik::FootRig::read`), each limb
/// node's renders (rigid or skinned) and the chain above it, and over `Walk` and `Idle` the lowest
/// point of each limb's rigid meshes, so which parts carry him.
fn limbs(data: &tf2_core::character::CharacterData) {
    use glam::Vec3;
    use tf2_core::formats::hash::lower33;
    let nodes = &data.robot.nodes;
    match tf2_core::footik::FootRig::read(&data.robot, &data.library) {
        Some(rig) => println!("FootRig: {:?}", rig.legs.iter().map(|l| (&nodes[l.hip].name, &nodes[l.knee].name, &nodes[l.foot].name)).collect::<Vec<_>>()),
        None => println!("FootRig: none"),
    }
    let chain = |i: usize| {
        let mut out = vec![nodes[i].name.clone()];
        let mut at = nodes[i].parent;
        while let Some(p) = at.filter(|_| out.len() < 5) {
            out.push(nodes[p].name.clone());
            at = nodes[p].parent;
        }
        out.join(" < ")
    };
    let limb_nodes: Vec<usize> = nodes.iter().enumerate()
        .filter(|(_, n)| ["Foot", "Hand", "Tread", "F_", "Midfoot", "Calf", "Forearm"].iter().any(|w| n.name.contains(w)))
        .map(|(i, _)| i).collect();
    for &i in &limb_nodes {
        let renders: Vec<String> = data.robot.renders.iter().filter(|r| r.node == i)
            .map(|r| format!("{}{}", r.model, match data.library.meshes.get(&lower33(&r.model)) {
                Some(m) if m.skeleton.is_some() => " (skinned)",
                Some(_) => " (rigid)",
                None => " (missing)",
            })).collect();
        println!("{i:3} {}  renders {:?}", chain(i), renders);
    }
    let rig = tf2_core::pose::Rig::new(&data.robot);
    for id in ["Walk", "Idle", "Attack1"] {
        let Some(r) = data.animations.sets.values().flatten().find(|r| r.id == id && r.clip.is_some()) else { continue };
        let clip = r.clip.as_ref().unwrap();
        println!("{id}: lowest point of each rigid limb mesh (z) by tick");
        let mut tick = 0.0;
        while tick <= clip.length {
            let pose = rig.pose(clip, r.looping, tick);
            let mut line = format!("{tick:5.0}");
            for &i in &limb_nodes {
                let mut low = f32::INFINITY;
                for render in data.robot.renders.iter().filter(|r| r.node == i) {
                    let Some(mesh) = data.library.meshes.get(&lower33(&render.model)).filter(|m| m.skeleton.is_none()) else { continue };
                    let bind = tf2_core::pose::affine(&nodes[i].bind);
                    let to_pose = pose[i] * bind.inverse();
                    for p in mesh.parts.iter().flat_map(|p| &p.positions) {
                        low = low.min(to_pose.transform_point3(bind.transform_point3(Vec3::from(*p))).z);
                    }
                }
                if low.is_finite() && low < 6.0 {
                    line += &format!(" {} {:.1}", nodes[i].name, low);
                }
            }
            println!("{line}");
            tick += clip.length / 8.0;
        }
    }
}

/// `--frames`: whether his clips share one frame. For each clip at its first and last tick: the
/// root channel (place, yaw), and with the root held (as the port draws him) where his feet's
/// midpoint is and which way his hips face (the left-to-right thigh line turned a quarter).
fn frames(data: &tf2_core::character::CharacterData) {
    use glam::{Quat, Vec3};
    use tf2_core::formats::anim::ROOT_CHANNEL;
    let nodes = &data.robot.nodes;
    let all = std::env::args().any(|a| a == "--all");
    for (i, n) in nodes.iter().enumerate().filter(|(_, n)| all || n.parent.is_none_or(|p| p < 3)) {
        println!("node {i} {} parent {:?} bind ({:.1}, {:.1}, {:.1})", n.name, n.parent, n.bind[3][0], n.bind[3][1], n.bind[3][2]);
    }
    if all {
        for r in &data.robot.renders {
            println!("render {} on node {} {}", r.model, r.node, nodes[r.node].name);
        }
        return;
    }
    let find = |w: &str| nodes.iter().position(|n| n.name == w);
    let (foot_l, foot_r) = (find("Foot_L").unwrap(), find("Foot_R").unwrap());
    let (hip_l, hip_r) = (nodes.iter().position(|n| n.name.contains("Thigh_L") || n.name.contains("UpperLeg_L") || n.name.contains("Hip_L")),
        nodes.iter().position(|n| n.name.contains("Thigh_R") || n.name.contains("UpperLeg_R") || n.name.contains("Hip_R")));
    println!("hips {:?} {:?}", hip_l.map(|i| &nodes[i].name), hip_r.map(|i| &nodes[i].name));
    let rig = tf2_core::pose::Rig::new(&data.robot);
    let mut refs: Vec<_> = data.animations.sets.iter().flat_map(|(s, rs)| rs.iter().map(move |r| (s, r))).collect();
    refs.sort_by(|a, b| (a.0, &a.1.id).cmp(&(b.0, &b.1.id)));
    for (set, r) in refs {
        let Some(clip) = &r.clip else { continue };
        let channel = clip.channel(ROOT_CHANNEL);
        for tick in [0.0, clip.length] {
            let p = channel.and_then(|c| c.position.sample(tick, clip.length, false)).map_or(Vec3::ZERO, Vec3::from);
            let q = channel.and_then(|c| c.rotation.sample(tick, clip.length, false)).map_or(Quat::IDENTITY, |q| Quat::from_xyzw(q[0], q[1], q[2], q[3]).normalize().conjugate());
            let f = q * Vec3::Y;
            let pose = rig.pose(clip, r.looping, tick);
            let at = |i: usize| Vec3::from(pose[i].translation);
            let mid = (at(foot_l) + at(foot_r)) * 0.5;
            let face = match (hip_l, hip_r) {
                (Some(l), Some(r)) => {
                    let across = at(r) - at(l);
                    let fwd = Vec3::new(-across.y, across.x, 0.0);
                    format!("{:6.1}", (-fwd.x).atan2(fwd.y).to_degrees())
                }
                _ => "-".into(),
            };
            println!("{set}/{} @{tick:3.0}: root ({:6.2}, {:6.2}) yaw {:6.1} | held: feet mid ({:6.2}, {:6.2}) hips face {face}",
                r.id, p.x, p.y, (-f.x).atan2(f.y).to_degrees(), mid.x, mid.y);
        }
    }
}

/// `--turn <clip id>`: where his feet and hands go over a clip, two ways. "clip" is the clip's own
/// frame, the root channel included (lined up with the port at tick 0); "port" is the model posed
/// with the root held, moved and turned as `sim::step_action` moves him (the root path in the
/// frame the clip started in, the root's turn as his yaw). Where the two agree, the port shows
/// what the clip shows.
fn turn(data: &tf2_core::character::CharacterData, id: &str) {
    use glam::{Affine3A, Quat, Vec3};
    use tf2_core::formats::anim::ROOT_CHANNEL;
    let Some((r, clip)) = data.animations.sets.values().flatten().find_map(|r| r.clip.as_ref().filter(|_| r.id == id).map(|c| (r, c))) else {
        println!("no clip {id}");
        return;
    };
    let rig = tf2_core::pose::Rig::new(&data.robot);
    let nodes = &data.robot.nodes;
    let root = nodes.iter().position(|n| n.name_hash == ROOT_CHANNEL);
    let channel = clip.channel(ROOT_CHANNEL).expect("root channel");
    let root_at = |tick: f32| {
        let p = channel.position.sample(tick, clip.length, false).map_or(Vec3::ZERO, Vec3::from);
        // Stored for row vectors: the conjugate is the rotation for glam.
        let q = channel.rotation.sample(tick, clip.length, false).map_or(Quat::IDENTITY, |q| Quat::from_xyzw(q[0], q[1], q[2], q[3]).normalize().conjugate());
        Affine3A::from_rotation_translation(q, p)
    };
    let bind = root.map_or(Affine3A::IDENTITY, |i| rig.pose(clip, r.looping, 0.0)[i]);
    let yaw = |a: &Affine3A| {
        let f = a.transform_vector3(Vec3::Y);
        (-f.x).atan2(f.y).to_degrees()
    };
    println!("{id}: {} ticks; root node {:?}, bind at ({:.2}, {:.2}, {:.2}) yaw {:.1}; root channel at 0 ({:.2}, {:.2}, {:.2}) yaw {:.1}",
        clip.length, root, bind.translation.x, bind.translation.y, bind.translation.z, yaw(&bind),
        root_at(0.0).translation.x, root_at(0.0).translation.y, root_at(0.0).translation.z, yaw(&root_at(0.0)));
    let watch: Vec<usize> = ["Foot", "Toe", "Hand", "Palm"]
        .iter()
        .flat_map(|w| nodes.iter().enumerate().filter(move |(_, n)| n.name.contains(w)).map(|(i, _)| i))
        .collect();
    println!("watching {:?}", watch.iter().map(|&i| nodes[i].name.as_str()).collect::<Vec<_>>());
    let path = clip.root_path();
    let turned = clip.root_turn();
    let start = bind * root_at(0.0).inverse();
    let step = (clip.length / 12.0).max(1.0);
    let mut tick = 0.0f32;
    while tick <= clip.length + 1e-3 {
        let k = (tick.round() as usize).min(path.len() - 1);
        let pose = rig.pose(clip, r.looping, tick);
        let body = Affine3A::from_rotation_translation(Quat::from_rotation_z(turned[k]), path[k]);
        let clip_frame = start * root_at(tick) * bind.inverse();
        print!("{tick:5.0} body ({:6.2}, {:6.2}) yaw {:6.1} | root ch ({:6.2}, {:6.2}) yaw {:6.1} |", path[k].x, path[k].y, turned[k].to_degrees(),
            root_at(tick).translation.x, root_at(tick).translation.y, yaw(&root_at(tick)));
        if std::env::args().any(|a| a == "--body") {
            // Where his body is rather than his limbs: the thighs' and the hands' midpoints, as
            // the port draws them, with the hips' facing (the thigh line turned a quarter).
            let at = |w: &str| nodes.iter().position(|n| n.name == w).map(|i| body.transform_point3(pose[i].translation.into()));
            let mid = |a: &str, b: &str| at(a).zip(at(b)).map(|(a, b)| (a + b) * 0.5);
            let (hips, hands, feet) = (mid("Thigh_L", "Thigh_R"), mid("Hand_L", "Hand_R"), mid("Foot_L", "Foot_R"));
            let face = at("Thigh_L").zip(at("Thigh_R")).map(|(l, r)| {
                let a = r - l;
                (a.y).atan2(a.x).to_degrees()
            });
            let p = |v: Option<Vec3>| v.map_or("-".into(), |v| format!("({:6.1}, {:6.1}, {:5.1})", v.x, v.y, v.z));
            print!(" hips {} face {:6.1} hands {} feet {}", p(hips), face.unwrap_or(f32::NAN), p(hands), p(feet));
            // `--watch=A,B`: more nodes by name.
            if let Some(list) = std::env::args().find_map(|a| a.strip_prefix("--watch=").map(str::to_owned)) {
                for w in list.split(',') {
                    print!(" {w} {}", p(at(w)));
                }
            }
            println!();
        } else {
            for &i in watch.iter().take(4) {
                let port = body.transform_point3(pose[i].translation.into());
                let clip_at = clip_frame.transform_point3(pose[i].translation.into());
                print!(" {} port ({:.1}, {:.1}) clip ({:.1}, {:.1})", nodes[i].name, port.x, port.y, clip_at.x, clip_at.y);
            }
            println!();
        }
        tick += step;
    }
}

/// `--slide <id>` (with `--reroot` for the clips as the port plays them): each hand's and foot's
/// place in the world tick by tick, moved by the clip's own root path ("clip") and by the clip's
/// mean forward speed in a straight line ("even", what the port's walk does), and how far each
/// slips sideways along the ground while it is down (its z under 2 m).
fn slide(data: &tf2_core::character::CharacterData, id: &str) {
    use glam::{Affine3A, Quat, Vec3};
    let nodes = &data.robot.nodes;
    let Some(r) = data.animations.sets.values().flatten().find(|r| r.id == id && r.clip.is_some()) else {
        println!("no clip {id}");
        return;
    };
    let clip = r.clip.as_ref().unwrap();
    let rig = tf2_core::pose::Rig::new(&data.robot);
    let path = clip.root_path();
    let turn = clip.root_turn();
    let speed = clip.forward_speed() / 60.0;
    let limbs: Vec<usize> = ["Foot_L", "Foot_R", "F_Middle_Tip_L", "F_Middle_Tip_R"]
        .iter()
        .filter_map(|n| nodes.iter().position(|x| x.name == *n))
        .collect();
    println!("{id}: {:.0} ticks, mean forward {:.2} m/s, root path end {:?}", clip.length, clip.forward_speed(), path.last());
    let ticks = clip.length.round() as usize;
    let mut previous: Option<(Vec<Vec3>, Vec<Vec3>)> = None;
    let mut slip_clip = vec![0.0f32; limbs.len()];
    let mut slip_even = vec![0.0f32; limbs.len()];
    for tick in 0..=ticks {
        let pose = rig.pose(clip, r.looping, tick as f32);
        let p = path.get(tick).copied().unwrap_or_default();
        let a = turn.get(tick).copied().unwrap_or(0.0);
        let by_clip = Affine3A::from_rotation_translation(Quat::from_rotation_z(a), p);
        let even = Affine3A::from_translation(Vec3::Y * speed * tick as f32);
        let at_clip: Vec<Vec3> = limbs.iter().map(|&n| by_clip.transform_point3(pose[n].translation.into())).collect();
        let at_even: Vec<Vec3> = limbs.iter().map(|&n| even.transform_point3(pose[n].translation.into())).collect();
        let mut line = format!("{tick:4} root ({:6.2},{:6.2})", p.x, p.y);
        if let Some((pc, pe)) = &previous {
            for i in 0..limbs.len() {
                let down = at_clip[i].z < 2.0;
                let dc = (at_clip[i] - pc[i]).truncate().length();
                let de = (at_even[i] - pe[i]).truncate().length();
                if down {
                    slip_clip[i] += dc;
                    slip_even[i] += de;
                }
                line += &format!(" | {} z{:5.1} clip {:.2} even {:.2}{}", nodes[limbs[i]].name, at_clip[i].z, dc, de, if down { "*" } else { "" });
            }
        }
        if tick % 4 == 0 {
            println!("{line}");
        }
        previous = Some((at_clip, at_even));
    }
    for (i, &n) in limbs.iter().enumerate() {
        println!("{}: slid {:.1} m on the ground by the clip's root, {:.1} m at the even speed", nodes[n].name, slip_clip[i], slip_even[i]);
    }
}

/// `--vortex`: the pack's vortex meshes (size and extent), and where his head is and which way its
/// +y points (the axis his `AttackSpecial` dust stream comes in along, from 100 m out) through that
/// clip, with where the axis meets the ground.
fn vortex(data: &tf2_core::character::CharacterData) {
    for mesh in data.library.meshes.values().filter(|m| m.name.to_lowercase().contains("vortex")) {
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in mesh.parts.iter().flat_map(|p| &p.positions) {
            for i in 0..3 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
        println!("mesh {} sphere {:?} half {:?} parts {} skeleton {:?} lo {lo:?} hi {hi:?}", mesh.name, mesh.sphere, mesh.half_extents, mesh.parts.len(), mesh.skeleton);
        // Its widest point at each tenth of its length (along y), and its material.
        for band in 0..10 {
            let (a, b) = (hi[1] * band as f32 / 10.0, hi[1] * (band + 1) as f32 / 10.0);
            let wide = mesh.parts.iter().flat_map(|p| &p.positions).filter(|p| (a..=b).contains(&p[1])).map(|p| p[0].hypot(p[2])).fold(0.0f32, f32::max);
            println!("  y {a:.2}..{b:.2}: radius {wide:.2}");
        }
        for part in &mesh.parts {
            let material = part.material.and_then(|m| data.library.materials.get(&m));
            println!("  part material {:?} technique {:?} uv range {:?}", material.map(|m| &m.name), material.map(|m| m.technique), part.uvs.iter().fold((f32::MAX, f32::MIN), |(lo, hi), uv| (lo.min(uv[1]), hi.max(uv[1]))));
        }
    }
    let dir = tf2_core::install::dir_string();
    match tf2_core::giant::read_vortex(Path::new(&dir)) {
        None => println!("no VortexObj"),
        Some(v) => {
            println!("VortexObj: damage {} knock-back {} at {} sound {:08x}", v.damage, v.knock_back_speed, v.knock_back_angle, v.sound);
            for (i, n) in v.object.nodes.iter().enumerate() {
                println!("  node {i} {} ({:08x}) parent {:?} bind {:?}", n.name, n.name_hash, n.parent, n.bind);
            }
            for r in &v.object.renders {
                println!("  render {} on node {}", r.model, r.node);
            }
            if let Some(clip) = &v.clip {
                println!("  clip {} ticks, streams {:?}", clip.length, clip.stream_kinds);
                for (name, channel) in clip.names.iter().zip(&clip.channels) {
                    for tick in [0.0, 20.0, 40.0, 60.0, 100.0, 200.0, 400.0, 460.0, 500.0, 520.0] {
                        println!("    {name:08x} t {tick:3}: pos {:?} rot {:?} scale {:?}", channel.position.sample(tick, clip.length, false), channel.rotation.sample(tick, clip.length, false), channel.scale.sample(tick, clip.length, false));
                    }
                }
            }
        }
    }
    let Some(r) = data.animations.find("PlayAnimSet", "AttackSpecial") else { return };
    let clip = r.clip.as_ref().unwrap();
    let rig = tf2_core::pose::Rig::new(&data.robot);
    let head = data.robot.nodes.iter().position(|n| n.name == "Head").unwrap();
    let mut tick = 0.0;
    while tick <= clip.length {
        let pose = rig.pose(clip, false, tick);
        let at = glam::Vec3::from(pose[head].translation);
        let axis = pose[head].matrix3 * glam::Vec3A::Y;
        let ground = if axis.z < -1e-3 { Some(at - glam::Vec3::from(axis) * (at.z / axis.z)) } else { None };
        println!("{tick:4} head ({:6.1},{:6.1},{:5.1}) +y ({:5.2},{:5.2},{:5.2}) ground {:?}", at.x, at.y, at.z, axis.x, axis.y, axis.z, ground.map(|g| (g.x.round(), g.y.round())));
        tick += 30.0;
    }
}
