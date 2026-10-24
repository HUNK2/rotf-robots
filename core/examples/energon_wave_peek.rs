//! What Grindor's cinematic wave (`tf2_core::energon_wave`) borrows, as read from the install:
//! Devastator's ring and its material, the effects named in its `Dressing`, the cannon's own
//! effects row, and the sounds it adds -- with the numbers the dressing is tuned on (each
//! template's elements: counts, lifetimes, sizes, speeds).
//! `cargo run -p tf2-core --example energon_wave_peek` (reads the install).

use tf2_core::energon_wave;
use tf2_core::particles::{Domain, Kind, Template};

fn range(d: &Option<Domain>) -> String {
    match d {
        None => "-".into(),
        Some(Domain::Point(v)) => format!("{:.2}", v.x),
        Some(Domain::Box { min, max }) | Some(Domain::Bump { min, max }) | Some(Domain::Line { min, max }) => {
            if min.y == 0.0 && max.y == 0.0 && min.z == 0.0 && max.z == 0.0 {
                format!("{:.2}..{:.2}", min.x, max.x)
            } else {
                format!("({:.1},{:.1},{:.1})..({:.1},{:.1},{:.1})", min.x, min.y, min.z, max.x, max.y, max.z)
            }
        }
        Some(other) => format!("{other:?}"),
    }
}

fn show(name: &str, t: Option<&Template>) {
    let Some(t) = t else {
        println!("{name}: NOT FOUND");
        return;
    };
    println!("{name}: radius {:.2}, {} elements", t.radius, t.elements.len());
    for (n, e) in t.elements.iter().enumerate() {
        println!("  [{n}] {} max {} t {:.2}..{:.2} local {} texture {}", String::from_utf8_lossy(&e.renderer.id.to_be_bytes()),
            e.max_particles, e.start, e.end, e.local_space, e.renderer.texture_path);
        for a in &e.actions {
            match &a.kind {
                Kind::Burst { count, slots } => println!("      Brst {count} at {:.2}..{:.2}: place {} radius {} life {} velocity {}", a.start, a.end,
                    range(&slots[0]), range(&slots[1]), range(&slots[9]), range(&slots[11])),
                Kind::Source { rate, slots } => println!("      SrcA {rate}/s at {:.2}..{:.2}: place {} radius {} life {} velocity {}", a.start, a.end,
                    range(&slots[0]), range(&slots[1]), range(&slots[9]), range(&slots[11])),
                Kind::LinearScale { delta } => println!("      LnSc {delta}"),
                Kind::ExponentialScale { exponent } => println!("      ExSc {exponent}"),
                Kind::Force { acc, local } => println!("      Forc {acc} local {local}"),
                other => println!("      {}", format!("{other:?}").chars().take(90).collect::<String>()),
            }
        }
    }
}

fn main() {
    let game = tf2_core::install::dir();
    let started = std::time::Instant::now();
    let (dressing, library) = energon_wave::read(game, "blackout").expect("the wave's dressing");
    println!("read in {:.0} ms", started.elapsed().as_secs_f64() * 1000.0);
    let ring = &dressing.ring;
    println!("ring: material {:?}, radius {}..{} at {} m/s, band {}..{}, fades {} / {}, damage {}, special {:?}",
        ring.material, ring.start_radius, ring.full_radius, ring.speed, ring.ring_radius, ring.full_ring_radius,
        ring.fade_in_radius, ring.fade_out_radius, ring.damage, ring.special);
    for (id, m) in &library.materials {
        println!("material {id:08x} {} technique {:08x}, {} textures in the borrowed library", m.name, m.technique, library.textures.len());
    }
    show("impact r_melee_endblast_blackout", dressing.impact.as_ref());
    show("dust r_contactdust_biglanding", dressing.dust.as_ref());
    show("sparks exp_bldg_electricity01", dressing.sparks.as_ref());
    let data = tf2_core::character::load(game, "blackout").expect("his pack");
    if let Some(row) = data.weapon_effects.iter().find(|w| w.name == energon_wave::CANNON) {
        show("the cannon's blast", row.blast.as_ref());
        println!("blast light {:?}", row.blast_light.as_ref().map(|l| (l.light.colour, l.light.range, l.time)));
        for p in &row.after_blast {
            show(&format!("after blast at {} (on {:?} off {:?})", p.offset, p.on, p.off), Some(&p.template));
        }
        if let Some(wave) = &row.wave {
            show("the wave's emitter", wave.emitter.as_ref().map(|e| &e.template));
            println!("wave light {:?}, life {}, sounds {:x?}", wave.light.as_ref().map(|l| (l.light.colour, l.light.range, l.time, l.fade_start)),
                wave.life, wave.sounds);
        }
    }
    let mut paths = vec![game.join(r"levels\us_city_00\eng\mission.fev")];
    paths.extend(energon_wave::sound_events(game));
    let (sounds, _) = tf2_core::sound::Library::load(&paths);
    for id in [energon_wave::IMPACT_SOUND, energon_wave::POUND_SOUND, energon_wave::ROLL_SOUND] {
        println!("sound {:?}: has {} infinite {} length {:?}", sounds.name(id), sounds.has(id), sounds.infinite(id), sounds.length_s(id));
    }
}
