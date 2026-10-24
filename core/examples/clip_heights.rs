//! How high a clip draws the body, every `step` ticks: the root channel's own height (which the
//! drawing strips), the hips' height and the lowest and highest node, in model space with the root
//! held still as the game draws it. For seeing a sink or a pop between clips that follow each other.
//! `cargo run -p tf2-core --example clip_heights -- <pack> <step> <Set/Clip> [Set/Clip ...]`
use tf2_core::formats::anim::ROOT_CHANNEL;
use tf2_core::pose::Rig;

fn main() {
    let mut a = std::env::args().skip(1);
    let pack = a.next().unwrap();
    let step: usize = a.next().and_then(|s| s.parse().ok()).unwrap_or(5);
    let data = tf2_core::character::load(tf2_core::install::dir(), &pack).expect("load");
    let rig = Rig::new(&data.robot);
    let hips = data.robot.nodes.iter().position(|n| n.name.to_lowercase().contains("pelvis") || n.name.to_lowercase().contains("hips"));
    println!("hips node: {:?}", hips.map(|i| &data.robot.nodes[i].name));
    let rest = rig.rest();
    let (lo, hi) = rest.iter().fold((f32::MAX, f32::MIN), |(lo, hi), m| (lo.min(m.translation.z), hi.max(m.translation.z)));
    println!("rest: hips {:6.2} lowest {lo:6.2} highest {hi:6.2}", hips.map_or(0.0, |i| rest[i].translation.z));
    for name in a {
        let (set, id) = name.split_once('/').expect("Set/Clip");
        let Some(r) = data.animations.find(set, id) else { println!("{name}: none"); continue };
        let clip = r.clip.as_ref().unwrap();
        println!("{name}: {} ticks, speed {}, looping {}", clip.length, r.speed, r.looping);
        let len = clip.length.ceil() as usize;
        for t in (0..=len).step_by(step).chain(std::iter::once(len)) {
            let tick = (t as f32).min(clip.length);
            let pose = rig.pose(clip, r.looping, tick);
            let root = clip.channel(ROOT_CHANNEL).and_then(|c| c.position.sample(tick, clip.length, r.looping));
            let (lo, hi) = pose.iter().fold((f32::MAX, f32::MIN), |(lo, hi), m| (lo.min(m.translation.z), hi.max(m.translation.z)));
            println!(
                "  {tick:5.0}  root z {:7.2}  hips {:6.2}  lowest {lo:6.2}  highest {hi:6.2}",
                root.map_or(f32::NAN, |p| p[2]),
                hips.map_or(0.0, |i| pose[i].translation.z),
            );
        }
    }
}
