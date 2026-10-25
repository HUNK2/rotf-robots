//! The change of form frame by frame, as `game\src\animation.rs` drives the two models: the
//! robot's clip and tick from the movement, its model events, and the vehicle model's driver
//! (started by those events, advanced per frame, handed to its follow-on clip at its end). Prints,
//! per frame, which vehicle clip and tick get SAMPLED (the game samples the clip it looked up at
//! the top of the frame, so a hand-over frame samples the old clip at 0), the vehicle's drawn span
//! and `Flipper`'s height in model space. For the robot -> vehicle snap (2026-10-24).
//! `cargo run -p tf2-core --example form_swap_frames -- <pack:AttributesName> [fps] [frames]`; the
//! fixed rules by default, `OLD_RULES=1` for the rules before the 2026-10-24 fix (the camera is
//! always the current one).

use std::path::Path;

use tf2_core::control::button;
use tf2_core::formats::anim::TICKS_PER_SECOND;
use tf2_core::pose::Rig;
use tf2_core::sim::{self, Arena, Input, State};
use tf2_core::tuning::{self, Actions, Locomotion, RuleSet, Tuning};
use tf2_core::camera::{Camera, Mode, Target};

/// `game\src\camera.rs` `target`, without the weapon aim, death and ground punch cases.
fn camera_target(state: &State, tuning: &Tuning) -> Target {
    let vehicle = state.is_vehicle();
    let flying = state.is_flying();
    let pose = state.car_presentation();
    let (yaw, pitch, _) = pose.angles();
    let up = pose.rotation * glam::Vec3::Z;
    let right = pose.rotation * glam::Vec3::X;
    let lift = tuning.vehicle_height + (up.z * (tuning.drive.body_offset_z - tuning.vehicle_height * 0.5)).max(0.0);
    Target {
        position: pose.pos,
        height: if vehicle {
            lift + if flying { tf2_core::flycam::look_drop(pose.pos, up.z, tuning.drive.body_offset_z, tuning.vehicle_height) } else { 0.0 }
        } else {
            tuning.robot_height
        },
        facing: if vehicle { glam::Vec2::new(-yaw.sin(), yaw.cos()) } else { state.facing() },
        velocity: pose.camera_velocity,
        motion: pose.velocity,
        vehicle,
        climbing: state.climb.is_some(),
        aim: None,
        radius: if vehicle { tuning.vehicle_radius } else { tuning.robot_radius },
        pitch: if vehicle { pitch } else { 0.0 },
        roll: if vehicle { (-right.z).clamp(-1.0, 1.0).asin() } else { 0.0 },
        fighting: state.fight > 0.0,
        mode: if state.fly.as_ref().is_some_and(|fly| flying && fly.avatar_blend > 0.0) { Mode::Transition } else { Mode::Normal },
        ground_distance: pose.wheels.iter().map(|w| w.probe_distance).fold(100.0, f32::min),
        body_height: vehicle.then_some(up.z * tuning.drive.body_offset_z),
        flying,
    }
}

fn main() {
    let dir = tf2_core::install::dir_string();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (pack, name) = args[0].split_once(':').expect("pack:Name");
    let fps: f32 = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(60.0);
    let frames: usize = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(100);
    let mut t = tuning::load(Path::new(&dir), name).expect("tuning");
    let data = tf2_core::character::load(Path::new(&dir), pack).expect("character");
    t.locomotion = Locomotion::from_animations(&data.animations, &mut t.missing);
    t.actions = Actions::from_animations(&data.animations, &data.robot, &mut t.missing).with_vehicle(data.vehicle.as_ref());
    t.weapon_set = RuleSet::from_animations(&data.animations, "WeaponSet", &mut t.missing);
    t.rule_sets = RuleSet::all(&data.animations);
    let vehicle = data.vehicle.as_ref().expect("a vehicle model");
    let vrig = Rig::new(vehicle);
    let flipper = vehicle.nodes.iter().position(|n| n.name == "Flipper");
    let find = |id: &str| data.vehicle_animations.iter().find(|a| a.id == id).and_then(|a| a.clip.as_ref().map(|c| (a, c)));
    // Each vehicle clip's own fade (what the game's object clip player uses: the PlayAnim's -1
    // means the entry's own, 007c3870), speed and loop.
    for a in &data.vehicle_animations {
        println!("vehicle ref {:<22} crossfade {:6.3} speed {:4.2} looping {}", a.id, a.crossfade, a.speed, a.looping);
    }
    println!(
        "camera heights: robot {:.2} (radius {:.2}), vehicle {:.2} (radius {:.2}), body offset z {:.2}",
        t.robot_height, t.robot_radius, t.vehicle_height, t.vehicle_radius, t.drive.body_offset_z
    );
    let mut s = State::new(&t);
    let arena = Arena::default();
    let dt = 1.0 / fps;
    // The game's `Animator` (events done, clip shown) and `VehicleAnimator` (playing, then, time).
    let (mut shown, mut events_done, mut last_tick) = (None::<(String, String)>, 0usize, 0.0f32);
    let (mut playing, mut then, mut time) = (None::<String>, None::<String>, 0.0f32);
    let mut last_span = None::<(f32, f32)>;
    let mut cam = None::<Camera>;
    let old = std::env::var_os("OLD_RULES").is_some();
    let (mut driver, mut started, mut last_local) = (tf2_core::vehicle_clip::Driver::default(), None::<()>, None);
    for f in 0..frames {
        let mut input = Input { vehicle: 1.0, ..Default::default() };
        input.buttons_down |= 1u64 << button::FLYING_MODE;
        sim::step(&mut s, &input, &t, &arena, dt);
        let mut notes = String::new();
        if let Some((set, id, tick)) = s.action_clip(&t) {
            let current = Some((set.to_owned(), id.to_owned()));
            let r = data.animations.find(set, id);
            let looping = r.is_some_and(|r| r.looping);
            if shown != current || (!looping && tick < last_tick) {
                shown = current;
                events_done = 0;
            }
            last_tick = tick;
            if let Some(clip) = r.and_then(|r| r.clip.as_ref()) {
                while let Some(e) = clip.model_events.get(events_done).filter(|e| e.time <= tick) {
                    events_done += 1;
                    if let Some(c) = &e.alternate_clip {
                        notes += &format!(" [event {:.0}: vehicle starts {c}]", e.time);
                        playing = Some(c.clone());
                        then = e.alternate_then.clone();
                        time = 0.0;
                        started = Some(());
                    } else {
                        notes += &format!(" [event {:.0}: robot {} vehicle {}]", e.time, e.show_true, e.show_alternate);
                    }
                }
            }
        }
        let robot = s.action_clip(&t).map(|(set, id, tick)| format!("{set}/{id} {tick:5.1}")).unwrap_or("-".into());
        let mut sampled = String::from("rest");
        let mut pose = vrig.rest();
        if !old {
            // The fixed rules (`tf2_core::vehicle_clip::Driver`, unkeyed nodes keep their last
            // local place), as `game\src\animation.rs` runs them since 2026-10-24.
            if started.take().is_some() {
                driver.start(playing.as_deref().unwrap(), then.clone());
            }
            let ticks = dt * TICKS_PER_SECOND * s.special_rate;
            if let Some(step) = driver.advance(ticks, |id| find(id).map(|(r, c)| (c.length, r.looping))) {
                let (r, clip) = find(&step.clip).unwrap();
                sampled = format!("{} {:5.1}", r.id, step.time);
                let model = vrig.pose(clip, r.looping, step.time);
                let local: Vec<_> = model.iter().enumerate()
                    .map(|(i, m)| vehicle.nodes[i].parent.map_or(*m, |p| model[p].inverse() * *m)).collect();
                let local: Vec<_> = local.iter().enumerate()
                    .map(|(i, l)| if clip.channel(vehicle.nodes[i].name_hash).is_none() { last_local.as_ref().map_or(*l, |v: &Vec<glam::Affine3A>| v[i]) } else { *l }).collect();
                pose.clear();
                for (i, l) in local.iter().enumerate() {
                    let m = vehicle.nodes[i].parent.map_or(*l, |p| pose[p] * *l);
                    pose.push(m);
                }
                last_local = Some(local);
            }
        } else if let Some((r, clip)) = playing.as_deref().and_then(&find) {
            time += dt * TICKS_PER_SECOND * r.speed * s.special_rate;
            let mut at = time;
            if r.looping {
                time %= clip.length.max(1.0);
                at = time;
            } else if time >= clip.length {
                time = clip.length;
                at = time;
                if let Some(next) = then.take() {
                    notes += &format!(" [vehicle {} ends -> {next}]", r.id);
                    playing = Some(next);
                    time = 0.0;
                    // As the game does: the clip looked up this frame, at the new time.
                    at = 0.0;
                }
            }
            sampled = format!("{} {at:5.1}", r.id);
            pose = vrig.pose(clip, r.looping, at);
        }
        let span = pose.iter().fold((f32::MAX, f32::MIN), |(lo, hi), m| (lo.min(m.translation.z), hi.max(m.translation.z)));
        let jump = last_span.map_or(0.0, |(lo, hi): (f32, f32)| (span.0 - lo).abs().max((span.1 - hi).abs()));
        last_span = Some(span);
        let fl = flipper.map_or(f32::NAN, |i| pose[i].translation.z);
        if jump > 1.0 {
            let (top, _) = pose.iter().enumerate().fold((0, f32::MIN), |(b, z), (i, m)| if m.translation.z > z { (i, m.translation.z) } else { (b, z) });
            notes += &format!(" [highest node {}, scale {:.3}]", vehicle.nodes[top].name, pose[top].matrix3.x_axis.length());
        }
        println!(
            "{f:3} {:5.3}s {:<16} robot {robot:<26} vehicle sampled {sampled:<20} drawn {:5.2}..{:5.2} jump {jump:4.2} Flipper z {fl:5.2} pos z {:5.2}{notes}",
            (f + 1) as f32 * dt, s.control_state(&t).unwrap_or("-"), span.0, span.1, s.pos.z,
        );
        // The camera, as `game\src\camera.rs` `target` feeds it.
        let target = camera_target(&s, &t);
        let view = cam.get_or_insert_with(|| {
            let mut c = Camera::default();
            c.reset(&camera_target(&State::new(&t), &t), &t);
            c
        }).step(&target, glam::Vec2::ZERO, &t, &arena, dt);
        let look = view.eye + view.forward * (target.position - view.eye).length();
        println!(
            "      camera target height {:5.2} (vehicle {}, mode {:?}) eye z {:6.2} looks at z {:6.2} pitch {:6.1}",
            target.height, target.vehicle, target.mode, view.eye.z, look.z, view.forward.z.asin().to_degrees()
        );
    }
}
