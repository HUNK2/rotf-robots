//! Where the robot and the vehicle model each draw the body at the moment a change of form swaps
//! them: the clip's model events (which model shows, which vehicle clip starts), the robot's
//! lowest and highest node and the named nodes at that tick (root held still, as drawn), and the
//! vehicle model's at the start of its clip. Model space, z up, metres.
//! `cargo run -p tf2-core --example form_swap_peek -- <pack> <Set/Clip> [node ...]`
use tf2_core::pose::Rig;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let data = tf2_core::character::load(tf2_core::install::dir(), &args[0]).expect("load");
    let (set, id) = args[1].split_once('/').expect("Set/Clip");
    let names = &args[2..];
    let r = data.animations.find(set, id).expect("clip");
    let clip = r.clip.as_ref().unwrap();
    let robot = Rig::new(&data.robot);
    let vehicle = data.vehicle.as_ref().expect("a vehicle model");
    let vrig = Rig::new(vehicle);
    let span = |pose: &[glam::Affine3A]| pose.iter().fold((f32::MAX, f32::MIN), |(lo, hi), m| (lo.min(m.translation.z), hi.max(m.translation.z)));
    println!("{id}: {} ticks; {} model events", clip.length, clip.model_events.len());
    for e in &clip.model_events {
        let pose = robot.pose(clip, r.looping, e.time);
        let (lo, hi) = span(&pose);
        println!("  tick {:5.1}: robot {} vehicle {} clip {:?} then {:?}; robot drawn z {lo:6.2}..{hi:6.2}", e.time, e.show_true, e.show_alternate, e.alternate_clip, e.alternate_then);
        for (i, n) in data.robot.nodes.iter().enumerate().filter(|(_, n)| names.contains(&n.name)) {
            let p = pose[i].translation;
            println!("    robot   {:<20} ({:6.2} {:6.2} {:6.2})", n.name, p.x, p.y, p.z);
        }
        let vclip = e.alternate_clip.as_ref().and_then(|c| data.vehicle_animations.iter().find(|a| &a.id == c)).and_then(|a| a.clip.as_ref());
        let vpose = vclip.map_or_else(|| vrig.rest(), |c| vrig.pose(c, false, 0.0));
        let (lo, hi) = span(&vpose);
        println!("    vehicle drawn z {lo:6.2}..{hi:6.2} ({})", if vclip.is_some() { "its clip at 0" } else { "rest" });
        for (i, n) in vehicle.nodes.iter().enumerate().filter(|(_, n)| names.contains(&n.name)) {
            let p = vpose[i].translation;
            println!("    vehicle {:<20} ({:6.2} {:6.2} {:6.2})", n.name, p.x, p.y, p.z);
        }
    }
    // The vehicle's own clips over their length: the root channel's height (stripped when drawn)
    // and the drawn span.
    println!("vehicle clips: {:?}", data.vehicle_animations.iter().map(|a| (&a.id, a.clip.is_some())).collect::<Vec<_>>());
    for a in data.vehicle_animations.iter().filter(|a| clip.model_events.iter().any(|e| e.alternate_clip.as_ref() == Some(&a.id) || e.alternate_then.as_ref() == Some(&a.id))) {
        let Some(c) = a.clip.as_ref() else { continue };
        println!("vehicle clip {}: {} ticks, looping {}", a.id, c.length, a.looping);
        for t in (0..=c.length.ceil() as usize).step_by(4) {
            let tick = (t as f32).min(c.length);
            let root = c.channel(tf2_core::formats::anim::ROOT_CHANNEL).and_then(|ch| ch.position.sample(tick, c.length, a.looping));
            let (lo, hi) = span(&vrig.pose(c, a.looping, tick));
            let rp = robot.pose(clip, r.looping, tick.min(clip.length));
            let (rlo, rhi) = span(&rp);
            println!("  {tick:5.0}  root z {:7.2}  vehicle drawn {lo:6.2}..{hi:6.2}   robot (same tick of {id}) {rlo:6.2}..{rhi:6.2}", root.map_or(f32::NAN, |p| p[2]));
        }
    }
    // The vehicle's nodes under its root: their own (local) place at the end of the change clip and
    // at the start of the clip after it, where they differ by more than 5 cm.
    if let Some(e) = clip.model_events.iter().find(|e| e.alternate_clip.is_some()) {
        let find = |id: &Option<String>| id.as_ref().and_then(|c| data.vehicle_animations.iter().find(|a| &a.id == c)).and_then(|a| a.clip.as_ref().map(|c| (a, c)));
        if let (Some((_, a)), Some((nb, b))) = (find(&e.alternate_clip), find(&e.alternate_then)) {
            let (pa, pb) = (vrig.pose(a, false, a.length), vrig.pose(b, nb.looping, 0.0));
            let rest = vrig.rest();
            println!("vehicle nodes, local place: end of {:?} | start of {:?} | rest", e.alternate_clip, e.alternate_then);
            for (i, n) in vehicle.nodes.iter().enumerate() {
                let parent = n.parent.unwrap_or(usize::MAX);
                let local = |pose: &[glam::Affine3A]| if parent < pose.len() { (pose[parent].inverse() * pose[i]).translation } else { pose[i].translation };
                let (la, lb, lr) = (local(&pa), local(&pb), local(&rest));
                if (la - lb).length() > 0.05 && n.parent.is_some_and(|p| vehicle.nodes[p].parent.is_none()) {
                    println!("  {:<22} ({:6.2} {:6.2} {:6.2}) | ({:6.2} {:6.2} {:6.2}) | ({:6.2} {:6.2} {:6.2})", n.name, la.x, la.y, la.z, lb.x, lb.y, lb.z, lr.x, lr.y, lr.z);
                }
            }
        }
    }
    // Which of the vehicle's nodes each of its clips keys (position / rotation streams).
    for a in &data.vehicle_animations {
        let Some(c) = a.clip.as_ref() else { continue };
        let keyed: Vec<_> = vehicle.nodes.iter().filter(|n| c.channel(n.name_hash).is_some()).collect();
        let pos = keyed.iter().filter(|n| c.channel(n.name_hash).unwrap().position.sample(0.0, c.length, false).is_some()).count();
        let unkeyed: Vec<_> = vehicle.nodes.iter().filter(|n| c.channel(n.name_hash).is_none()).map(|n| n.name.as_str()).take(12).collect();
        println!("vehicle clip {:<22} keys {:3} of {} nodes ({pos} with a position); some unkeyed: {unkeyed:?}", a.id, keyed.len(), vehicle.nodes.len());
    }
    // Nodes one vehicle clip keys and the clip after it does not: their local place at the first
    // clip's end against their rest (what the second shows depends on which the game keeps).
    let pair = |x: &str, y: &str| {
        let get = |id: &str| data.vehicle_animations.iter().find(|a| a.id == id).and_then(|a| a.clip.as_ref());
        let (Some(a), Some(b)) = (get(x), get(y)) else { return };
        let (pa, rest) = (vrig.pose(a, false, a.length), vrig.rest());
        println!("keyed by {x}, not by {y}:");
        for (i, n) in vehicle.nodes.iter().enumerate().filter(|(_, n)| a.channel(n.name_hash).is_some() && b.channel(n.name_hash).is_none()) {
            let local = |pose: &[glam::Affine3A]| n.parent.map_or(pose[i], |p| pose[p].inverse() * pose[i]);
            let (la, lr) = (local(&pa), local(&rest));
            let scale = la.matrix3.x_axis.length();
            println!("  {:<26} end ({:6.2} {:6.2} {:6.2}) scale {scale:5.2} | rest ({:6.2} {:6.2} {:6.2}) scale {:5.2}", n.name, la.translation.x, la.translation.y, la.translation.z, lr.translation.x, lr.translation.y, lr.translation.z, lr.matrix3.x_axis.length());
        }
    };
    pair("Trans_R2V_A", "Trans_R2V_B");
    pair("Trans_R2V_B", "V_Idle");
    if names.is_empty() {
        let common: Vec<_> = vehicle.nodes.iter().filter(|v| data.robot.nodes.iter().any(|n| n.name == v.name)).map(|n| n.name.as_str()).collect();
        println!("nodes in both models: {common:?}");
    }
}
