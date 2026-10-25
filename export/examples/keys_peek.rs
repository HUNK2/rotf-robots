//! Prints one clip's keys for one node near a tick, for looking into a mismatch.
//!   cargo run -p rotf-export --example keys_peek -- <pack> <set> <id> <node> <tick>
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let data = tf2_core::character::load(tf2_core::install::dir(), &a[0]).unwrap();
    let r = data.animations.find(&a[1], &a[2]).unwrap();
    let clip = r.clip.as_ref().unwrap();
    let tick: f32 = a[4].parse().unwrap();
    let nodes: Vec<_> = data.robot.nodes.iter().enumerate().filter(|(_, n)| n.name == a[3]).collect();
    println!("length {} looping {} nodes named so: {}", clip.length, r.looping, nodes.len());
    for (i, n) in nodes {
        println!("node {i} parent {:?} hash {:08x}", n.parent, n.name_hash);
        let Some(c) = clip.channel(n.name_hash) else { println!("  no channel"); continue };
        for (label, t) in [("pos", c.position.times()), ("rot", c.rotation.times()), ("scale", c.scale.times()), ("own", c.own_scale.times())] {
            let near: Vec<String> = t.iter().filter(|&&x| (x - tick).abs() <= 3.0).map(|x| format!("{x}")).collect();
            println!("  {label}: {} keys, first {:?}, last {:?}, near: {}", t.len(), t.first(), t.last(), near.join(" "));
        }
        println!("  pos at tick {:?}", c.position.sample(tick, clip.length, r.looping));
    }
}
